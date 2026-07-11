use crate::events::EventBus;
use futures_util::StreamExt;
use lapin::{options::*, types::FieldTable, Connection, ConnectionProperties, ExchangeKind};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

/// notification exchange들에 임시 큐를 바인딩하고 수신 이벤트를 EventBus로 흘린다.
/// 접속/바인딩까지 성공해야 Ok를 돌려주고, 이후 소비는 백그라운드 태스크에서 진행.
/// cancel 시 태스크 종료 + 연결 정리.
/// `app`이 Some이면 수신 메시지를 프론트로 `mq-log` 이벤트로도 흘린다(환경 화면 로그).
pub async fn start_consumer(
    uris: &[String],
    exchanges: &[String],
    bus: Arc<EventBus>,
    cancel: CancellationToken,
    app: Option<AppHandle>,
) -> Result<(), String> {
    let conn = connect_any(uris).await?;
    let channel = conn.create_channel().await.map_err(|e| format!("채널 생성 실패: {e}"))?;

    let queue = channel
        .queue_declare(
            "".into(), // 서버가 이름 생성하는 임시 전용 큐
            QueueDeclareOptions { exclusive: true, auto_delete: true, ..Default::default() },
            FieldTable::default(),
        )
        .await
        .map_err(|e| format!("큐 생성 실패: {e}"))?;

    for ex in exchanges {
        // OpenStack notification exchange는 topic 타입. passive=true로 존재 확인만 한다.
        channel
            .exchange_declare(
                ex.as_str().into(),
                ExchangeKind::Topic,
                ExchangeDeclareOptions { passive: true, ..Default::default() },
                FieldTable::default(),
            )
            .await
            .map_err(|e| format!("exchange '{ex}' 확인 실패: {e}"))?;
        channel
            .queue_bind(
                queue.name().as_str().into(),
                ex.as_str().into(),
                // oslo.messaging 기본 notification_topics("notifications") 가정 — 커스텀 토픽 배포는 v2에서 설정화
                "notifications.#".into(),
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| format!("바인딩 실패({ex}): {e}"))?;
    }

    let mut consumer = channel
        .basic_consume(
            queue.name().as_str().into(),
            "contrabass-test-tool".into(),
            BasicConsumeOptions { no_ack: true, ..Default::default() },
            FieldTable::default(),
        )
        .await
        .map_err(|e| format!("소비 시작 실패: {e}"))?;

    if let Some(a) = &app {
        let _ = a.emit("mq-log", serde_json::json!({ "event_type": "(연결)", "text": "RabbitMQ 연결됨 — 알림 수신 대기" }));
    }
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                delivery = consumer.next() => {
                    match delivery {
                        Some(Ok(delivery)) => handle_delivery(&delivery.data, &bus, app.as_ref()),
                        Some(Err(e)) => { eprintln!("[mq] 소비 에러로 종료: {e}"); break; }
                        None => { eprintln!("[mq] 스트림 종료 (연결 끊김)"); break; }
                    }
                }
            }
        }
        if let Some(a) = &app {
            let _ = a.emit("mq-log", serde_json::json!({ "event_type": "(종료)", "text": "RabbitMQ 로그 중단됨" }));
        }
        let _ = conn.close(200, "done".into()).await;
    });
    Ok(())
}

/// 클러스터 노드들을 순서대로 시도해 처음 성공한 연결을 돌려준다(페일오버). 모두 실패하면 마지막 에러.
async fn connect_any(uris: &[String]) -> Result<Connection, String> {
    if uris.is_empty() {
        return Err("RabbitMQ 호스트가 비어 있습니다".into());
    }
    let mut last = String::new();
    for uri in uris {
        match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Connection::connect(uri, ConnectionProperties::default()),
        )
        .await
        {
            Ok(Ok(conn)) => return Ok(conn),
            Ok(Err(e)) => last = format!("접속 실패: {e}"),
            Err(_) => last = "접속 타임아웃 (10초)".into(),
        }
    }
    Err(format!("모든 RabbitMQ 노드 접속 실패 — {last}"))
}

/// 수신 바이트를 파싱·언랩해 버스에 싣는다. app이 있으면 프론트 로그로도 emit. 파싱 실패는 버리되 stderr에 남긴다.
fn handle_delivery(data: &[u8], bus: &EventBus, app: Option<&AppHandle>) {
    match serde_json::from_slice::<serde_json::Value>(data) {
        Ok(value) => {
            let ev = unwrap_oslo(value);
            if let Some(a) = app {
                let et = ev.get("event_type").and_then(|v| v.as_str()).unwrap_or("(unknown)").to_string();
                let text = serde_json::to_string(&ev).unwrap_or_default();
                let text = if text.len() > 4000 { text[..4000].to_string() } else { text };
                let _ = a.emit("mq-log", serde_json::json!({ "event_type": et, "text": text }));
            }
            bus.publish(ev);
        }
        Err(e) => eprintln!("[mq] notification JSON 파싱 실패 (버림): {e}"),
    }
}

/// oslo 봉투 언랩: {"oslo.version": "2.0", "oslo.message": "<JSON 문자열>"} → 내부 메시지
pub fn unwrap_oslo(value: serde_json::Value) -> serde_json::Value {
    if let Some(inner) = value.get("oslo.message").and_then(|m| m.as_str()) {
        if let Ok(parsed) = serde_json::from_str(inner) {
            return parsed;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unwraps_oslo_envelope() {
        let wrapped = json!({
            "oslo.version": "2.0",
            "oslo.message": "{\"event_type\":\"compute.instance.create.end\",\"payload\":{}}"
        });
        let inner = unwrap_oslo(wrapped);
        assert_eq!(inner["event_type"], "compute.instance.create.end");
    }

    #[test]
    fn passes_through_plain_message() {
        let plain = json!({"event_type": "x"});
        assert_eq!(unwrap_oslo(plain.clone()), plain);
    }

    #[test]
    fn passes_through_broken_envelope() {
        let broken = json!({"oslo.message": "not json"});
        assert_eq!(unwrap_oslo(broken.clone()), broken);
    }

    #[tokio::test]
    async fn handle_delivery_publishes_parsed_event() {
        let bus = EventBus::new();
        handle_delivery(br#"{"event_type":"compute.instance.create.end"}"#, &bus, None);
        let got = bus
            .wait_for(
                |e| e["event_type"] == "compute.instance.create.end",
                std::time::Duration::from_millis(50),
            )
            .await
            .unwrap();
        assert_eq!(got["event_type"], "compute.instance.create.end");
    }

    #[tokio::test]
    async fn handle_delivery_drops_broken_json() {
        let bus = EventBus::new();
        handle_delivery(b"not json at all", &bus, None);
        let err = bus
            .wait_for(|_| true, std::time::Duration::from_millis(50))
            .await;
        assert!(err.is_err());
    }
}

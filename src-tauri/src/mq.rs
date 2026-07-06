use crate::events::EventBus;
use futures_util::StreamExt;
use lapin::{options::*, types::FieldTable, Connection, ConnectionProperties, ExchangeKind};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// notification exchange들에 임시 큐를 바인딩하고 수신 이벤트를 EventBus로 흘린다.
/// 접속/바인딩까지 성공해야 Ok를 돌려주고, 이후 소비는 백그라운드 태스크에서 진행.
/// cancel 시 태스크 종료 + 연결 정리.
pub async fn start_consumer(
    mq_url: &str,
    exchanges: &[String],
    bus: Arc<EventBus>,
    cancel: CancellationToken,
) -> Result<(), String> {
    let conn = Connection::connect(mq_url, ConnectionProperties::default())
        .await
        .map_err(|e| format!("RabbitMQ 접속 실패: {e}"))?;
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

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                delivery = consumer.next() => {
                    let Some(Ok(delivery)) = delivery else { break };
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&delivery.data) {
                        bus.publish(unwrap_oslo(value));
                    }
                }
            }
        }
        let _ = conn.close(200, "done".into()).await;
    });
    Ok(())
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
}

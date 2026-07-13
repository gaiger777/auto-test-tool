use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

#[derive(Default)]
pub struct EventBus {
    buffer: Mutex<Vec<Value>>,
    notify: Notify,
}

impl EventBus {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn publish(&self, event: Value) {
        self.buffer.lock().unwrap().push(event);
        self.notify.notify_waiters();
    }

    /// 지금까지 버퍼된 이벤트 사본. 타임아웃 시 "왜 안 맞았나" 진단에 쓴다.
    pub fn snapshot(&self) -> Vec<Value> {
        self.buffer.lock().unwrap().clone()
    }

    /// pred에 맞는 이벤트를 버퍼(과거) + 실시간(미래)에서 찾는다. timeout 초과 시 Err.
    ///
    /// 시맨틱: "과거"는 이 wait_for 호출 시점이 아니라 **버스 생성(실행 시작) 시점**부터다.
    /// 매 호출은 버퍼를 처음부터 재스캔하므로, 같은 pred로 두 번 기다리면 같은 과거
    /// 이벤트에 다시 매칭된다. 서로 다른 대기는 조건(리소스 ID 등)으로 구분해야 한다.
    /// 의도: 스텝 순서와 이벤트 도착 순서가 어긋나도(예: B의 이벤트가 A의 이벤트보다
    /// 먼저 도착) 놓치지 않는 것이 목적이며, 소비 오프셋을 두면 이 보장이 깨진다.
    pub async fn wait_for<F>(&self, pred: F, timeout: Duration) -> Result<Value, String>
    where
        F: Fn(&Value) -> bool,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut cursor = 0usize;
        loop {
            // 알림 누락 방지: notified를 먼저 등록(enable)한 뒤 버퍼를 확인한다.
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            {
                let buf = self.buffer.lock().unwrap();
                while cursor < buf.len() {
                    if pred(&buf[cursor]) {
                        return Ok(buf[cursor].clone());
                    }
                    cursor += 1;
                }
            }

            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err(format!("이벤트 대기 타임아웃 ({:?})", timeout));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn finds_event_published_before_wait() {
        let bus = EventBus::new();
        bus.publish(json!({"event_type": "a"}));
        let got = bus
            .wait_for(|e| e["event_type"] == "a", Duration::from_millis(100))
            .await
            .unwrap();
        assert_eq!(got["event_type"], "a");
    }

    #[tokio::test]
    async fn finds_event_published_after_wait_started() {
        let bus = EventBus::new();
        let bus2 = bus.clone();
        let waiter = tokio::spawn(async move {
            bus2.wait_for(|e| e["event_type"] == "b", Duration::from_secs(2))
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        bus.publish(json!({"event_type": "b"}));
        assert!(waiter.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn rescans_history_on_each_wait_for_call() {
        // 의도된 계약: 매 wait_for는 실행 시작부터의 버퍼를 재스캔한다 (오프셋 소비 없음).
        // 서로 다른 대기의 구분은 pred(조건)의 몫이다.
        let bus = EventBus::new();
        bus.publish(json!({"event_type": "a", "id": 1}));
        let first = bus
            .wait_for(|e| e["event_type"] == "a", Duration::from_millis(50))
            .await
            .unwrap();
        let second = bus
            .wait_for(|e| e["event_type"] == "a", Duration::from_millis(50))
            .await
            .unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn times_out_when_no_match() {
        let bus = EventBus::new();
        bus.publish(json!({"event_type": "other"}));
        let err = bus
            .wait_for(|e| e["event_type"] == "never", Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(err.contains("타임아웃"));
    }
}

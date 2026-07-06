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

    /// pred에 맞는 이벤트를 버퍼(과거) + 실시간(미래)에서 찾는다. timeout 초과 시 Err.
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
                return Err(format!("이벤트 대기 타임아웃 ({}초)", timeout.as_secs()));
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

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapturedCall {
    pub id: String,
    pub method: String,
    pub url: String,
    pub request_headers: std::collections::HashMap<String, String>,
    pub request_body: Option<String>,
    pub status: u16,
    pub response_body: Option<String>,
    pub timestamp: i64,
}

/// 토큰을 검증하고 바디를 CapturedCall로 파싱한다. 토큰 불일치/잘못된 JSON은 Err.
pub fn parse_capture(expected_token: &str, got_token: &str, body: &str) -> Result<CapturedCall, String> {
    if got_token != expected_token || expected_token.is_empty() {
        return Err("토큰 불일치".into());
    }
    serde_json::from_str(body).map_err(|e| format!("캡처 파싱 실패: {e}"))
}

/// 캡처를 소비하는 대상 (실제: Tauri 이벤트 emit / 테스트: fake).
pub trait CaptureSink: Send + Sync {
    fn emit(&self, call: CapturedCall);
}

pub struct EventSink {
    pub app: AppHandle,
}

impl CaptureSink for EventSink {
    fn emit(&self, call: CapturedCall) {
        let _ = self.app.emit("capture-recorded", call);
    }
}

struct ServerCtx {
    token: String,
    sink: Box<dyn CaptureSink>,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: String,
}

async fn capture_handler(
    axum::extract::State(ctx): axum::extract::State<Arc<ServerCtx>>,
    axum::extract::Query(q): axum::extract::Query<TokenQuery>,
    body: String,
) -> axum::http::StatusCode {
    match parse_capture(&ctx.token, &q.token, &body) {
        Ok(call) => {
            ctx.sink.emit(call);
            axum::http::StatusCode::OK
        }
        Err(_) => axum::http::StatusCode::BAD_REQUEST,
    }
}

/// 127.0.0.1의 빈 포트에 수집 서버를 띄우고 실제 포트를 돌려준다.
/// cancel이 취소되면 서버가 graceful shutdown 한다.
pub async fn start(
    token: String,
    sink: Box<dyn CaptureSink>,
    cancel: CancellationToken,
) -> Result<u16, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("수집 서버 바인딩 실패: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let ctx = Arc::new(ServerCtx { token, sink });
    let router = axum::Router::new()
        .route("/capture", axum::routing::post(capture_handler))
        .with_state(ctx);

    tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move { cancel.cancelled().await })
            .await;
    });
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn body() -> String {
        r#"{"id":"1","method":"POST","url":"https://h/api/x","request_headers":{},"request_body":null,"status":202,"response_body":null,"timestamp":0}"#.to_string()
    }

    #[test]
    fn parses_valid_capture_with_matching_token() {
        let c = parse_capture("tok", "tok", &body()).unwrap();
        assert_eq!(c.method, "POST");
        assert_eq!(c.status, 202);
    }

    #[test]
    fn rejects_wrong_token() {
        assert!(parse_capture("tok", "nope", &body()).is_err());
    }

    #[test]
    fn rejects_empty_expected_token() {
        assert!(parse_capture("", "", &body()).is_err());
    }

    #[test]
    fn rejects_bad_json() {
        assert!(parse_capture("tok", "tok", "not json").is_err());
    }

    struct FakeSink(Mutex<Vec<CapturedCall>>);
    impl CaptureSink for FakeSink {
        fn emit(&self, call: CapturedCall) {
            self.0.lock().unwrap().push(call);
        }
    }

    #[test]
    fn sink_trait_receives_call() {
        let sink = FakeSink(Mutex::new(Vec::new()));
        let call = parse_capture("t", "t", &body()).unwrap();
        sink.emit(call);
        assert_eq!(sink.0.lock().unwrap().len(), 1);
    }
}

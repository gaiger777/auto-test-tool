use serde::{Deserialize, Serialize};

/// 캡처된 단일 네트워크 호출.
/// 캡처 웹뷰의 후킹 스크립트가 Tauri IPC(`capture_push` 커맨드)로 전달한다.
/// (예전에는 localhost HTTP 서버로 POST했으나, https 페이지에서 mixed content로 차단돼 IPC로 교체했다.)
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

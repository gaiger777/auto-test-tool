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

/// 요소를 찾는 후보 셀렉터 하나. 재생 시 우선순위대로 시도(자가치유)한다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiSelector {
    pub strategy: String, // "testid" | "id" | "name" | "role" | "text" | "css"
    pub value: String,
}

/// 사용자가 캡처 창에서 한 UI 조작 하나 (클릭/입력).
/// 레코더 스크립트가 `ui_record` 커맨드로 전달한다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiAction {
    pub id: String,
    pub kind: String, // "click" | "input"
    pub selectors: Vec<UiSelector>,
    pub name: String,
    pub value: Option<String>,
    pub url: String,
    pub timestamp: i64,
}

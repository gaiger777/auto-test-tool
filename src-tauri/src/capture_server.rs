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

/// UI 동작이 유발한 네트워크 호출 요약(상관보기/검증/상세 표시용).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiCall {
    pub method: String,
    pub url: String,
    pub status: u16,
    #[serde(default)]
    pub request_headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub request_body: Option<String>,
}

/// 사용자가 캡처 창에서 한 UI 조작 하나 (클릭/입력).
/// 레코더 스크립트가 `ui_record` 커맨드로 전달한다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiAction {
    pub id: String,
    pub kind: String, // "click" | "input" | "hover" | "http_call" | "wait_event" | "assert" | "sleep"
    #[serde(default)]
    pub selectors: Vec<UiSelector>,
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
    /// 링크 클릭이면 절대 URL. 재생 시 요소를 못 찾으면 이 URL로 폴백 이동.
    #[serde(default)]
    pub href: Option<String>,
    /// 이 동작이 유발한 네트워크 호출(저장 시 상관 결과를 함께 보관 → 스위트에서 표시).
    #[serde(default)]
    pub api: Vec<UiCall>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub timestamp: i64,
    /// 프로그램 스텝(http_call/wait_event/assert/sleep)의 설정. UI 스텝이면 없음.
    #[serde(default)]
    pub step: Option<serde_json::Value>,
}

/// UI 재생 중 한 스텝의 결과. 플레이어 스크립트가 `ui_replay_step` 커맨드로 보고한다.
/// index = -1 은 재생 전체 종료 신호(done=true).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiStepResult {
    pub index: i64,
    pub status: String, // "passed" | "failed"
    pub detail: String,
    pub done: bool,
}

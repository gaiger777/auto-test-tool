# contrabass E2E 자동화 테스트 툴 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** OpenStack API를 HTTP로 호출하고 RabbitMQ notification으로 비동기 완료를 감지하는 시나리오 빌더형 E2E 테스트 데스크톱 앱.

**Architecture:** Rust 엔진은 범용 스텝 4종(http_call/wait_event/assert/sleep)만 실행하고, OpenStack 리소스 스텝은 프론트엔드 프리셋이 범용 스텝으로 펼쳐 저장한다. 실행 진행 상황은 Tauri event로 프론트에 스트리밍한다.

**Tech Stack:** Tauri 2, React + TypeScript + Vite, reqwest, lapin, rusqlite, serde_json_path, keyring, wiremock(테스트)

**Spec:** `docs/superpowers/specs/2026-07-06-contrabass-e2e-test-tool-design.md`

---

## 파일 구조

```
src-tauri/src/
├── main.rs          # 엔트리 (스캐폴드 그대로)
├── lib.rs           # 모듈 선언, tauri::Builder, AppState
├── models.rs        # Scenario/StepDef/Action/Capture/Condition/Vars
├── template.rs      # {{var}} 치환
├── assertion.rs     # assert 연산 (eq/contains/regex)
├── matcher.rs       # notification 이벤트 매칭 (event_type + JSONPath 조건)
├── events.rs        # EventBus (버퍼링 + 대기), 레이스 방지 핵심
├── http.rs          # http_call 실행 + JSONPath 변수 캡처
├── keystone.rs      # Keystone 토큰 발급/캐시/401 재발급
├── engine.rs        # 시나리오 순차 실행, cleanup, 취소, ProgressSink
├── mq.rs            # lapin RabbitMQ 소비자 → EventBus (oslo.message 언랩)
├── store.rs         # SQLite CRUD (environments/scenarios/runs/step_results)
└── commands.rs      # Tauri commands + 이벤트 발행

src/                  # React
├── types.ts         # Rust 모델 미러 타입
├── api.ts           # invoke 래퍼
├── presets.ts       # OpenStack 프리셋 → StepDef[] 펼치기
├── App.tsx          # 탭 네비게이션
├── views/EnvironmentsView.tsx
├── views/ScenarioBuilder.tsx
├── views/StepForm.tsx
├── views/RunView.tsx
└── views/HistoryView.tsx
```

백엔드(Task 1~12)를 먼저 완성해 `cargo test`로 검증하고, 프론트(Task 13~17)를 붙인다.

---

### Task 1: Tauri 프로젝트 스캐폴딩

**Files:**
- Create: 프로젝트 전체 (create-tauri-app이 생성)

- [ ] **Step 1: 스캐폴드 생성**

프로젝트 루트(`/Users/mskim/ai-pjt/autoTestTool`)에서 실행. 디렉토리가 이미 존재하므로 임시 이름으로 만들고 내용물을 옮긴다:

```bash
cd /Users/mskim/ai-pjt/autoTestTool
npm create tauri-app@latest tmp-scaffold -- --template react-ts --manager npm --yes
rsync -a tmp-scaffold/ ./ && rm -rf tmp-scaffold
npm install
```

- [ ] **Step 2: 개발 빌드 확인**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```
Expected: 에러 없이 `Finished` 출력.

- [ ] **Step 3: Rust 의존성 추가**

```bash
cd src-tauri
cargo add serde --features derive
cargo add serde_json tokio --features tokio/full
cargo add reqwest --features json
cargo add regex serde_json_path lapin rusqlite --features rusqlite/bundled
cargo add keyring tokio-util futures-util chrono
cargo add tauri-plugin-dialog
cargo add --dev wiremock
cd ..
npm install @tauri-apps/plugin-dialog
```

- [ ] **Step 4: 빌드 재확인 후 커밋**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
git add -A && git commit -m "chore: Tauri 2 + React-TS 스캐폴드 및 의존성 추가"
```

---

### Task 2: 데이터 모델 (models.rs)

**Files:**
- Create: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/lib.rs` (모듈 선언 추가)

- [ ] **Step 1: 실패하는 테스트 작성**

`src-tauri/src/models.rs` 를 만들고 맨 아래에 테스트부터 작성:

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type Vars = HashMap<String, String>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<StepDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepDef {
    pub name: String,
    #[serde(default)]
    pub cleanup: bool,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    HttpCall {
        method: String,
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        expect_status: Option<u16>,
        #[serde(default)]
        captures: Vec<Capture>,
    },
    WaitEvent {
        event_type: String,
        #[serde(default)]
        conditions: Vec<Condition>,
        timeout_secs: u64,
    },
    Assert {
        left: String,
        op: AssertOp,
        right: String,
    },
    Sleep {
        seconds: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Capture {
    pub var: String,
    pub json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Condition {
    pub json_path: String,
    pub equals: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AssertOp {
    Eq,
    Contains,
    Regex,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_roundtrips_through_json() {
        let json = r#"{
          "name": "인스턴스 생성 테스트",
          "steps": [
            {
              "name": "서버 생성",
              "type": "http_call",
              "method": "POST",
              "url": "{{base_url.nova}}/servers",
              "headers": {"X-Auth-Token": "{{auth_token}}"},
              "body": "{\"server\": {}}",
              "expect_status": 202,
              "captures": [{"var": "server_id", "json_path": "$.server.id"}]
            },
            {
              "name": "생성 완료 대기",
              "type": "wait_event",
              "event_type": "compute.instance.create.end",
              "conditions": [{"json_path": "$.payload.instance_id", "equals": "{{server_id}}"}],
              "timeout_secs": 300
            },
            {
              "name": "서버 삭제",
              "cleanup": true,
              "type": "http_call",
              "method": "DELETE",
              "url": "{{base_url.nova}}/servers/{{server_id}}"
            },
            {"name": "검증", "type": "assert", "left": "{{server_id}}", "op": "regex", "right": "^[0-9a-f-]+$"},
            {"name": "잠깐 대기", "type": "sleep", "seconds": 3}
          ]
        }"#;
        let s: Scenario = serde_json::from_str(json).unwrap();
        assert_eq!(s.steps.len(), 5);
        assert!(s.steps[2].cleanup);
        assert!(!s.steps[0].cleanup);
        let back = serde_json::to_string(&s).unwrap();
        let s2: Scenario = serde_json::from_str(&back).unwrap();
        assert_eq!(s, s2);
    }
}
```

- [ ] **Step 2: lib.rs에 모듈 등록**

`src-tauri/src/lib.rs` 상단에 추가:

```rust
pub mod models;
```

- [ ] **Step 3: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml models
```
Expected: `scenario_roundtrips_through_json ... ok` (모델 정의와 테스트를 함께 작성했으므로 바로 통과해야 함. 실패하면 serde 태그/flatten 설정을 확인)

- [ ] **Step 4: 커밋**

```bash
git add -A && git commit -m "feat: 시나리오/스텝 데이터 모델"
```

---

### Task 3: 템플릿 엔진 (template.rs)

**Files:**
- Create: `src-tauri/src/template.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 실패하는 테스트 작성**

`src-tauri/src/template.rs`:

```rust
use crate::models::Vars;

/// 문자열 안의 {{var}} 를 vars 값으로 치환한다. 미정의 변수는 에러.
pub fn render(input: &str, vars: &Vars) -> Result<String, String> {
    let re = regex::Regex::new(r"\{\{\s*([\w.]+)\s*\}\}").unwrap();
    let mut missing: Option<String> = None;
    let out = re
        .replace_all(input, |caps: &regex::Captures| {
            let key = &caps[1];
            match vars.get(key) {
                Some(v) => v.clone(),
                None => {
                    missing = Some(format!("정의되지 않은 변수: {key}"));
                    String::new()
                }
            }
        })
        .into_owned();
    match missing {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars() -> Vars {
        HashMap::from([
            ("server_id".to_string(), "abc-123".to_string()),
            ("base_url.nova".to_string(), "http://nova:8774/v2.1".to_string()),
        ])
    }

    #[test]
    fn substitutes_variables() {
        assert_eq!(
            render("{{base_url.nova}}/servers/{{server_id}}", &vars()).unwrap(),
            "http://nova:8774/v2.1/servers/abc-123"
        );
    }

    #[test]
    fn allows_whitespace_inside_braces() {
        assert_eq!(render("{{ server_id }}", &vars()).unwrap(), "abc-123");
    }

    #[test]
    fn errors_on_undefined_variable() {
        let err = render("{{nope}}", &vars()).unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn passes_through_plain_text() {
        assert_eq!(render("no vars here", &vars()).unwrap(), "no vars here");
    }
}
```

`lib.rs`에 `pub mod template;` 추가.

- [ ] **Step 2: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml template
```
Expected: 4개 테스트 PASS

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: {{var}} 템플릿 치환"
```

---

### Task 4: assert 로직 (assertion.rs)

**Files:**
- Create: `src-tauri/src/assertion.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 구현 + 테스트 작성**

`src-tauri/src/assertion.rs`:

```rust
use crate::models::AssertOp;

pub fn check(left: &str, op: &AssertOp, right: &str) -> Result<(), String> {
    let ok = match op {
        AssertOp::Eq => left == right,
        AssertOp::Contains => left.contains(right),
        AssertOp::Regex => regex::Regex::new(right)
            .map_err(|e| format!("잘못된 정규식 '{right}': {e}"))?
            .is_match(left),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("assert 실패: '{left}' {op:?} '{right}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_passes_and_fails() {
        assert!(check("a", &AssertOp::Eq, "a").is_ok());
        assert!(check("a", &AssertOp::Eq, "b").is_err());
    }

    #[test]
    fn contains_works() {
        assert!(check("ACTIVE state", &AssertOp::Contains, "ACTIVE").is_ok());
        assert!(check("ERROR", &AssertOp::Contains, "ACTIVE").is_err());
    }

    #[test]
    fn regex_works_and_rejects_bad_pattern() {
        assert!(check("abc-123", &AssertOp::Regex, "^[a-z]+-\\d+$").is_ok());
        assert!(check("abc", &AssertOp::Regex, "[").is_err());
    }
}
```

`lib.rs`에 `pub mod assertion;` 추가.

- [ ] **Step 2: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml assertion
```
Expected: 3개 테스트 PASS

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: assert 연산 (eq/contains/regex)"
```

---

### Task 5: 이벤트 매칭 (matcher.rs)

**Files:**
- Create: `src-tauri/src/matcher.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 구현 + 테스트 작성**

`src-tauri/src/matcher.rs`:

```rust
use serde_json::Value;

/// JSONPath로 값을 찾아 문자열로 돌려준다. 문자열이면 그대로, 아니면 JSON 직렬화.
pub fn json_path_str(value: &Value, path: &str) -> Result<String, String> {
    let p = serde_json_path::JsonPath::parse(path)
        .map_err(|e| format!("잘못된 JSONPath '{path}': {e}"))?;
    let node = p
        .query(value)
        .exactly_one()
        .map_err(|_| format!("JSONPath '{path}' 결과가 정확히 1개가 아님"))?;
    Ok(match node {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// notification 이벤트가 event_type과 (이미 렌더링된) 조건들에 모두 일치하는가.
/// conditions: (json_path, 기대값) 쌍
pub fn matches(event: &Value, event_type: &str, conditions: &[(String, String)]) -> bool {
    if event.get("event_type").and_then(|v| v.as_str()) != Some(event_type) {
        return false;
    }
    conditions
        .iter()
        .all(|(path, expected)| json_path_str(event, path).as_deref() == Ok(expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event() -> Value {
        json!({
            "event_type": "compute.instance.create.end",
            "payload": {"instance_id": "abc-123", "state": "active"}
        })
    }

    #[test]
    fn matches_event_type_and_conditions() {
        let conds = vec![("$.payload.instance_id".to_string(), "abc-123".to_string())];
        assert!(matches(&event(), "compute.instance.create.end", &conds));
    }

    #[test]
    fn rejects_wrong_event_type() {
        assert!(!matches(&event(), "compute.instance.delete.end", &[]));
    }

    #[test]
    fn rejects_condition_mismatch() {
        let conds = vec![("$.payload.instance_id".to_string(), "other".to_string())];
        assert!(!matches(&event(), "compute.instance.create.end", &conds));
    }

    #[test]
    fn rejects_missing_path() {
        let conds = vec![("$.payload.nope".to_string(), "x".to_string())];
        assert!(!matches(&event(), "compute.instance.create.end", &conds));
    }

    #[test]
    fn json_path_str_stringifies_non_strings() {
        let v = json!({"n": 42});
        assert_eq!(json_path_str(&v, "$.n").unwrap(), "42");
    }
}
```

`lib.rs`에 `pub mod matcher;` 추가.

- [ ] **Step 2: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml matcher
```
Expected: 5개 테스트 PASS

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: notification 이벤트 매칭"
```

---

### Task 6: EventBus (events.rs) — 레이스 방지 핵심

**Files:**
- Create: `src-tauri/src/events.rs`
- Modify: `src-tauri/src/lib.rs`

`wait_event` 스텝 시작 전에 도착한 이벤트도 놓치지 않도록, 실행 시작부터 모든 이벤트를 버퍼에 쌓고 버퍼+실시간 양쪽에서 매칭한다.

- [ ] **Step 1: 구현 + 테스트 작성**

`src-tauri/src/events.rs`:

```rust
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
```

`lib.rs`에 `pub mod events;` 추가.

- [ ] **Step 2: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml events
```
Expected: 3개 테스트 PASS

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: 버퍼링 EventBus (wait_event 레이스 방지)"
```

---

### Task 7: http_call 실행 + 변수 캡처 (http.rs)

**Files:**
- Create: `src-tauri/src/http.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 구현 + 테스트 작성 (wiremock 사용)**

`src-tauri/src/http.rs`:

```rust
use crate::matcher::json_path_str;
use crate::models::{Capture, Vars};
use std::collections::HashMap;

pub struct HttpResult {
    pub status: u16,
    pub body: String,
}

pub async fn execute(
    client: &reqwest::Client,
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Result<HttpResult, String> {
    let m = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| format!("잘못된 HTTP 메서드: {method}"))?;
    let mut req = client.request(m, url);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    if let Some(b) = body {
        req = req.header("Content-Type", "application/json").body(b.to_string());
    }
    let resp = req.send().await.map_err(|e| format!("HTTP 요청 실패: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(|e| format!("응답 읽기 실패: {e}"))?;
    Ok(HttpResult { status, body })
}

/// 응답 바디에서 JSONPath로 값을 뽑아 vars에 넣는다.
pub fn capture_vars(body: &str, captures: &[Capture], vars: &mut Vars) -> Result<(), String> {
    if captures.is_empty() {
        return Ok(());
    }
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("응답이 JSON이 아님: {e}"))?;
    for c in captures {
        let v = json_path_str(&json, &c.json_path)
            .map_err(|e| format!("캡처 '{}' 실패: {e}", c.var))?;
        vars.insert(c.var.clone(), v);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn sends_request_with_headers_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/servers"))
            .and(header("X-Auth-Token", "tok"))
            .and(body_string(r#"{"server":{}}"#))
            .respond_with(ResponseTemplate::new(202).set_body_string(r#"{"server":{"id":"abc-123"}}"#))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let headers = HashMap::from([("X-Auth-Token".to_string(), "tok".to_string())]);
        let res = execute(&client, "POST", &format!("{}/servers", server.uri()), &headers, Some(r#"{"server":{}}"#))
            .await
            .unwrap();
        assert_eq!(res.status, 202);
        assert!(res.body.contains("abc-123"));
    }

    #[tokio::test]
    async fn rejects_invalid_method() {
        let client = reqwest::Client::new();
        let err = execute(&client, "굿", "http://localhost", &HashMap::new(), None)
            .await
            .unwrap_err();
        assert!(err.contains("메서드"));
    }

    #[test]
    fn captures_variables_from_body() {
        let mut vars = Vars::new();
        let caps = vec![Capture { var: "server_id".into(), json_path: "$.server.id".into() }];
        capture_vars(r#"{"server":{"id":"abc-123"}}"#, &caps, &mut vars).unwrap();
        assert_eq!(vars["server_id"], "abc-123");
    }

    #[test]
    fn capture_fails_on_missing_path() {
        let mut vars = Vars::new();
        let caps = vec![Capture { var: "x".into(), json_path: "$.nope".into() }];
        assert!(capture_vars(r#"{}"#, &caps, &mut vars).is_err());
    }
}
```

`lib.rs`에 `pub mod http;` 추가.

- [ ] **Step 2: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml http
```
Expected: 4개 테스트 PASS

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: http_call 실행 및 JSONPath 변수 캡처"
```

---

### Task 8: Keystone 클라이언트 (keystone.rs)

**Files:**
- Create: `src-tauri/src/keystone.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 구현 + 테스트 작성**

`src-tauri/src/keystone.rs`:

```rust
use std::sync::Mutex;

pub struct KeystoneAuth {
    pub auth_url: String, // 예: http://keystone:5000 (경로 /v3/auth/tokens 는 코드가 붙임)
    pub user_name: String,
    pub user_domain: String,
    pub password: String,
    pub project_name: String,
    pub project_domain: String,
}

pub struct KeystoneClient {
    client: reqwest::Client,
    auth: KeystoneAuth,
    cached: Mutex<Option<String>>,
}

impl KeystoneClient {
    pub fn new(client: reqwest::Client, auth: KeystoneAuth) -> Self {
        Self { client, auth, cached: Mutex::new(None) }
    }

    /// 캐시된 토큰이 있으면 재사용, 없으면 발급.
    pub async fn get_token(&self) -> Result<String, String> {
        if let Some(t) = self.cached.lock().unwrap().clone() {
            return Ok(t);
        }
        self.issue_token().await
    }

    /// 캐시를 버리고 강제 재발급 (401 재시도용).
    pub async fn refresh_token(&self) -> Result<String, String> {
        *self.cached.lock().unwrap() = None;
        self.issue_token().await
    }

    async fn issue_token(&self) -> Result<String, String> {
        let body = serde_json::json!({
            "auth": {
                "identity": {
                    "methods": ["password"],
                    "password": {"user": {
                        "name": self.auth.user_name,
                        "domain": {"name": self.auth.user_domain},
                        "password": self.auth.password
                    }}
                },
                "scope": {"project": {
                    "name": self.auth.project_name,
                    "domain": {"name": self.auth.project_domain}
                }}
            }
        });
        let url = format!("{}/v3/auth/tokens", self.auth.auth_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Keystone 접속 실패: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Keystone 인증 실패: HTTP {}", resp.status().as_u16()));
        }
        let token = resp
            .headers()
            .get("X-Subject-Token")
            .and_then(|v| v.to_str().ok())
            .ok_or("응답에 X-Subject-Token 헤더가 없음")?
            .to_string();
        *self.cached.lock().unwrap() = Some(token.clone());
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn auth(url: &str) -> KeystoneAuth {
        KeystoneAuth {
            auth_url: url.to_string(),
            user_name: "admin".into(),
            user_domain: "Default".into(),
            password: "pw".into(),
            project_name: "admin".into(),
            project_domain: "Default".into(),
        }
    }

    #[tokio::test]
    async fn issues_and_caches_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/auth/tokens"))
            .respond_with(ResponseTemplate::new(201).insert_header("X-Subject-Token", "tok-1"))
            .expect(1) // 캐시 덕에 딱 1번만 호출돼야 함
            .mount(&server)
            .await;

        let ks = KeystoneClient::new(reqwest::Client::new(), auth(&server.uri()));
        assert_eq!(ks.get_token().await.unwrap(), "tok-1");
        assert_eq!(ks.get_token().await.unwrap(), "tok-1");
    }

    #[tokio::test]
    async fn refresh_reissues_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/auth/tokens"))
            .respond_with(ResponseTemplate::new(201).insert_header("X-Subject-Token", "tok-2"))
            .expect(2)
            .mount(&server)
            .await;

        let ks = KeystoneClient::new(reqwest::Client::new(), auth(&server.uri()));
        ks.get_token().await.unwrap();
        assert_eq!(ks.refresh_token().await.unwrap(), "tok-2");
    }

    #[tokio::test]
    async fn reports_auth_failure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/auth/tokens"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let ks = KeystoneClient::new(reqwest::Client::new(), auth(&server.uri()));
        assert!(ks.get_token().await.unwrap_err().contains("401"));
    }
}
```

`lib.rs`에 `pub mod keystone;` 추가.

- [ ] **Step 2: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml keystone
```
Expected: 3개 테스트 PASS

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: Keystone 토큰 발급/캐시/재발급"
```

---

### Task 9: 실행 엔진 (engine.rs)

**Files:**
- Create: `src-tauri/src/engine.rs`
- Modify: `src-tauri/src/lib.rs`

핵심 규칙:
- 스텝 순차 실행. 비-cleanup 스텝이 실패하면 이후 비-cleanup 스텝은 skipped, cleanup 스텝은 항상 실행.
- 취소되면 현재 스텝 이후 비-cleanup 스텝은 skipped, cleanup 스텝은 실행.
- http_call이 401을 받으면 토큰 1회 재발급 후 같은 요청을 재시도 (Task 12에서 keystone과 연결. 엔진은 `token_refresher` 콜백으로 추상화).

- [ ] **Step 1: 구현 + 테스트 작성**

`src-tauri/src/engine.rs`:

```rust
use crate::assertion;
use crate::events::EventBus;
use crate::http;
use crate::matcher;
use crate::models::{Action, Scenario, Vars};
use crate::template::render;
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepOutcome {
    pub index: usize,
    pub name: String,
    pub status: StepStatus,
    pub detail: String, // 응답 스냅샷 또는 에러 메시지
    pub duration_ms: u64,
}

pub trait ProgressSink: Send + Sync {
    fn step_started(&self, index: usize, name: &str);
    fn step_finished(&self, outcome: &StepOutcome);
}

/// 401 시 새 토큰을 받아오는 콜백. 새 토큰 문자열을 돌려준다.
pub type TokenRefresher =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> + Send + Sync>;

pub struct RunInput {
    pub scenario: Scenario,
    pub vars: Vars, // auth_token, base_url.* 등 내장 변수 포함
    pub bus: Arc<EventBus>,
    pub cancel: CancellationToken,
    pub token_refresher: Option<TokenRefresher>,
}

pub async fn run(input: RunInput, sink: &dyn ProgressSink) -> Vec<StepOutcome> {
    let RunInput { scenario, mut vars, bus, cancel, token_refresher } = input;
    // 설계 문서의 "HTTP 요청 타임아웃 존재" — 개별 API 호출은 30초 안에 응답해야 한다.
    // (장시간 대기는 http_call이 아니라 wait_event의 몫)
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("reqwest client 생성 실패");
    let mut outcomes = Vec::new();
    let mut aborted = false; // 실패 또는 취소 발생 여부
    // cleanup 스텝은 취소된 상태에서도 끝까지 실행되어야 하므로, 이미 취소된 토큰 대신
    // 절대 취소되지 않는 토큰을 넘긴다 (자연 타임아웃으로만 종료).
    let never_cancelled = CancellationToken::new();

    for (i, step) in scenario.steps.iter().enumerate() {
        if cancel.is_cancelled() {
            aborted = true;
        }
        if aborted && !step.cleanup {
            let o = StepOutcome {
                index: i,
                name: step.name.clone(),
                status: StepStatus::Skipped,
                detail: String::new(),
                duration_ms: 0,
            };
            sink.step_finished(&o);
            outcomes.push(o);
            continue;
        }

        sink.step_started(i, &step.name);
        let started = std::time::Instant::now();
        let step_cancel = if step.cleanup { &never_cancelled } else { &cancel };
        let result = execute_action(&step.action, &mut vars, &client, &bus, step_cancel, &token_refresher).await;
        let duration_ms = started.elapsed().as_millis() as u64;

        let o = match result {
            Ok(detail) => StepOutcome { index: i, name: step.name.clone(), status: StepStatus::Passed, detail, duration_ms },
            Err(e) => {
                aborted = true;
                // 실패 메시지에 대용량 응답/캡처값이 섞여도 UI/DB가 넘치지 않게 한 곳에서 자른다
                StepOutcome { index: i, name: step.name.clone(), status: StepStatus::Failed, detail: truncate(&e, 2000), duration_ms }
            }
        };
        sink.step_finished(&o);
        outcomes.push(o);
    }
    outcomes
}

async fn execute_action(
    action: &Action,
    vars: &mut Vars,
    client: &reqwest::Client,
    bus: &Arc<EventBus>,
    cancel: &CancellationToken,
    token_refresher: &Option<TokenRefresher>,
) -> Result<String, String> {
    match action {
        Action::HttpCall { method, url, headers, body, expect_status, captures } => {
            let mut attempt = 0u8;
            loop {
                let url_r = render(url, vars)?;
                let mut headers_r: HashMap<String, String> = HashMap::new();
                for (k, v) in headers {
                    headers_r.insert(k.clone(), render(v, vars)?);
                }
                let body_r = match body {
                    Some(b) => Some(render(b, vars)?),
                    None => None,
                };
                let res = tokio::select! {
                    r = http::execute(client, method, &url_r, &headers_r, body_r.as_deref()) => r?,
                    _ = cancel.cancelled() => return Err("취소됨".into()),
                };

                // 401이면 토큰 1회 재발급 후 재시도
                if res.status == 401 && attempt == 0 {
                    if let Some(refresher) = token_refresher {
                        let new_token = refresher().await?;
                        vars.insert("auth_token".into(), new_token);
                        attempt = 1;
                        continue;
                    }
                }

                if let Some(expected) = expect_status {
                    if res.status != *expected {
                        return Err(format!("기대 상태코드 {expected}, 실제 {}: {}", res.status, res.body));
                    }
                }
                http::capture_vars(&res.body, captures, vars)?;
                return Ok(format!("HTTP {} | {}", res.status, truncate(&res.body, 2000)));
            }
        }
        Action::WaitEvent { event_type, conditions, timeout_secs } => {
            let et = render(event_type, vars)?;
            let mut conds = Vec::new();
            for c in conditions {
                conds.push((c.json_path.clone(), render(&c.equals, vars)?));
            }
            // 스텝 시작 시 1회 파싱 — JSONPath 오타가 타임아웃이 아니라 즉시 실패로 드러난다
            let compiled = matcher::compile_conditions(&conds)?;
            let event = tokio::select! {
                r = bus.wait_for(move |e| matcher::matches(e, &et, &compiled), Duration::from_secs(*timeout_secs)) => r?,
                _ = cancel.cancelled() => return Err("취소됨".into()),
            };
            Ok(truncate(&event.to_string(), 2000))
        }
        Action::Assert { left, op, right } => {
            let l = render(left, vars)?;
            let r = render(right, vars)?;
            assertion::check(&l, op, &r)?;
            Ok(format!("'{l}' {op:?} '{r}'"))
        }
        Action::Sleep { seconds } => {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(*seconds)) => Ok(format!("{seconds}초 대기")),
                _ = cancel.cancelled() => Err("취소됨".into()),
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…(잘림)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AssertOp, StepDef};
    use serde_json::json;
    use std::sync::Mutex;

    struct NullSink;
    impl ProgressSink for NullSink {
        fn step_started(&self, _: usize, _: &str) {}
        fn step_finished(&self, _: &StepOutcome) {}
    }

    struct RecordingSink(Mutex<Vec<String>>);
    impl ProgressSink for RecordingSink {
        fn step_started(&self, i: usize, name: &str) {
            self.0.lock().unwrap().push(format!("start {i} {name}"));
        }
        fn step_finished(&self, o: &StepOutcome) {
            self.0.lock().unwrap().push(format!("finish {} {:?}", o.index, o.status));
        }
    }

    fn step(name: &str, cleanup: bool, action: Action) -> StepDef {
        StepDef { name: name.into(), cleanup, action }
    }

    fn input(steps: Vec<StepDef>) -> RunInput {
        RunInput {
            scenario: Scenario { name: "t".into(), description: String::new(), steps },
            vars: Vars::new(),
            bus: EventBus::new(),
            cancel: CancellationToken::new(),
            token_refresher: None,
        }
    }

    #[tokio::test]
    async fn all_steps_pass() {
        let outcomes = run(
            input(vec![
                step("a", false, Action::Assert { left: "x".into(), op: AssertOp::Eq, right: "x".into() }),
                step("b", false, Action::Sleep { seconds: 0 }),
            ]),
            &NullSink,
        )
        .await;
        assert!(outcomes.iter().all(|o| o.status == StepStatus::Passed));
    }

    #[tokio::test]
    async fn failure_skips_rest_but_runs_cleanup() {
        let outcomes = run(
            input(vec![
                step("fail", false, Action::Assert { left: "a".into(), op: AssertOp::Eq, right: "b".into() }),
                step("normal", false, Action::Sleep { seconds: 0 }),
                step("cleanup", true, Action::Sleep { seconds: 0 }),
            ]),
            &NullSink,
        )
        .await;
        assert_eq!(outcomes[0].status, StepStatus::Failed);
        assert_eq!(outcomes[1].status, StepStatus::Skipped);
        assert_eq!(outcomes[2].status, StepStatus::Passed);
    }

    #[tokio::test]
    async fn wait_event_matches_buffered_event() {
        let bus = EventBus::new();
        bus.publish(json!({"event_type": "compute.instance.create.end",
                           "payload": {"instance_id": "abc"}}));
        let mut inp = input(vec![step(
            "wait",
            false,
            Action::WaitEvent {
                event_type: "compute.instance.create.end".into(),
                conditions: vec![crate::models::Condition {
                    json_path: "$.payload.instance_id".into(),
                    equals: "{{server_id}}".into(),
                }],
                timeout_secs: 1,
            },
        )]);
        inp.bus = bus;
        inp.vars.insert("server_id".into(), "abc".into());
        let outcomes = run(inp, &NullSink).await;
        assert_eq!(outcomes[0].status, StepStatus::Passed);
    }

    #[tokio::test]
    async fn wait_event_times_out() {
        let outcomes = run(
            input(vec![step(
                "wait",
                false,
                Action::WaitEvent { event_type: "never".into(), conditions: vec![], timeout_secs: 0 },
            )]),
            &NullSink,
        )
        .await;
        assert_eq!(outcomes[0].status, StepStatus::Failed);
        assert!(outcomes[0].detail.contains("타임아웃"));
    }

    #[tokio::test]
    async fn cancel_skips_non_cleanup_and_runs_cleanup() {
        let inp = input(vec![
            step("normal", false, Action::Sleep { seconds: 0 }),
            step("cleanup", true, Action::Sleep { seconds: 0 }),
        ]);
        inp.cancel.cancel(); // 시작 전에 이미 취소된 상황
        let outcomes = run(inp, &NullSink).await;
        assert_eq!(outcomes[0].status, StepStatus::Skipped);
        assert_eq!(outcomes[1].status, StepStatus::Passed);
    }

    #[tokio::test]
    async fn sink_receives_events_in_order() {
        let sink = RecordingSink(Mutex::new(Vec::new()));
        run(
            input(vec![step("only", false, Action::Sleep { seconds: 0 })]),
            &sink,
        )
        .await;
        let log = sink.0.lock().unwrap();
        assert_eq!(log[0], "start 0 only");
        assert!(log[1].starts_with("finish 0"));
    }
}
```

`lib.rs`에 `pub mod engine;` 추가.

- [ ] **Step 2: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml engine
```
Expected: 6개 테스트 PASS

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: 시나리오 실행 엔진 (cleanup/취소/401 재시도)"
```

---

### Task 10: RabbitMQ 소비자 (mq.rs)

**Files:**
- Create: `src-tauri/src/mq.rs`
- Modify: `src-tauri/src/lib.rs`

실 RabbitMQ 연동은 통합 환경에서만 검증 가능하므로, 단위 테스트는 oslo 봉투 언랩 로직만 다룬다. 소비자 자체는 Task 12 완료 후 실제 환경에서 수동 검증.

- [ ] **Step 1: 구현 + 언랩 테스트 작성**

`src-tauri/src/mq.rs`:

```rust
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
            "", // 서버가 이름 생성하는 임시 전용 큐
            QueueDeclareOptions { exclusive: true, auto_delete: true, ..Default::default() },
            FieldTable::default(),
        )
        .await
        .map_err(|e| format!("큐 생성 실패: {e}"))?;

    for ex in exchanges {
        // OpenStack notification exchange는 topic 타입. passive=true로 존재 확인만 한다.
        channel
            .exchange_declare(
                ex,
                ExchangeKind::Topic,
                ExchangeDeclareOptions { passive: true, ..Default::default() },
                FieldTable::default(),
            )
            .await
            .map_err(|e| format!("exchange '{ex}' 확인 실패: {e}"))?;
        channel
            .queue_bind(
                queue.name().as_str(),
                ex,
                "notifications.#",
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| format!("바인딩 실패({ex}): {e}"))?;
    }

    let mut consumer = channel
        .basic_consume(
            queue.name().as_str(),
            "contrabass-test-tool",
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
        let _ = conn.close(200, "done").await;
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
```

`lib.rs`에 `pub mod mq;` 추가.

- [ ] **Step 2: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml mq
```
Expected: 3개 테스트 PASS

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: RabbitMQ notification 소비자 (oslo 언랩)"
```

---

### Task 11: SQLite 저장소 + 키체인 (store.rs)

**Files:**
- Create: `src-tauri/src/store.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 구현 + 테스트 작성**

`src-tauri/src/store.rs`:

```rust
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Environment {
    pub id: Option<i64>,
    pub name: String,
    pub keystone_url: String,
    pub user_name: String,
    pub user_domain: String,
    pub project_name: String,
    pub project_domain: String,
    pub mq_url: String,
    pub mq_exchanges: String, // 쉼표 구분: "nova,neutron,cinder"
    pub endpoints: HashMap<String, String>, // {"nova": "http://nova:8774/v2.1", ...}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScenarioRecord {
    pub id: Option<i64>,
    pub name: String,
    pub description: String,
    pub steps_json: String, // models::Scenario의 steps 배열 JSON
}

#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub id: i64,
    pub scenario_id: i64,
    pub scenario_name: String,
    pub env_id: i64,
    pub status: String, // running | passed | failed | cancelled
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResultRecord {
    pub run_id: i64,
    pub step_index: i64,
    pub name: String,
    pub status: String,
    pub detail: String,
    pub duration_ms: i64,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS environments (
               id INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               keystone_url TEXT NOT NULL,
               user_name TEXT NOT NULL,
               user_domain TEXT NOT NULL,
               project_name TEXT NOT NULL,
               project_domain TEXT NOT NULL,
               mq_url TEXT NOT NULL,
               mq_exchanges TEXT NOT NULL,
               endpoints TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS scenarios (
               id INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               description TEXT NOT NULL DEFAULT '',
               steps_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS runs (
               id INTEGER PRIMARY KEY,
               scenario_id INTEGER NOT NULL,
               env_id INTEGER NOT NULL,
               status TEXT NOT NULL,
               started_at TEXT NOT NULL,
               finished_at TEXT
             );
             CREATE TABLE IF NOT EXISTS step_results (
               id INTEGER PRIMARY KEY,
               run_id INTEGER NOT NULL,
               step_index INTEGER NOT NULL,
               name TEXT NOT NULL,
               status TEXT NOT NULL,
               detail TEXT NOT NULL,
               duration_ms INTEGER NOT NULL
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    // --- environments ---

    pub fn save_environment(&self, env: &Environment) -> Result<i64, String> {
        let endpoints = serde_json::to_string(&env.endpoints).map_err(|e| e.to_string())?;
        match env.id {
            Some(id) => {
                self.conn
                    .execute(
                        "UPDATE environments SET name=?1, keystone_url=?2, user_name=?3, user_domain=?4,
                         project_name=?5, project_domain=?6, mq_url=?7, mq_exchanges=?8, endpoints=?9 WHERE id=?10",
                        params![env.name, env.keystone_url, env.user_name, env.user_domain,
                                env.project_name, env.project_domain, env.mq_url, env.mq_exchanges, endpoints, id],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(id)
            }
            None => {
                self.conn
                    .execute(
                        "INSERT INTO environments (name, keystone_url, user_name, user_domain,
                         project_name, project_domain, mq_url, mq_exchanges, endpoints)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                        params![env.name, env.keystone_url, env.user_name, env.user_domain,
                                env.project_name, env.project_domain, env.mq_url, env.mq_exchanges, endpoints],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn list_environments(&self) -> Result<Vec<Environment>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, keystone_url, user_name, user_domain, project_name, project_domain, mq_url, mq_exchanges, endpoints FROM environments ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                let endpoints_json: String = r.get(9)?;
                Ok(Environment {
                    id: Some(r.get(0)?),
                    name: r.get(1)?,
                    keystone_url: r.get(2)?,
                    user_name: r.get(3)?,
                    user_domain: r.get(4)?,
                    project_name: r.get(5)?,
                    project_domain: r.get(6)?,
                    mq_url: r.get(7)?,
                    mq_exchanges: r.get(8)?,
                    endpoints: serde_json::from_str(&endpoints_json).unwrap_or_default(),
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    pub fn get_environment(&self, id: i64) -> Result<Environment, String> {
        self.list_environments()?
            .into_iter()
            .find(|e| e.id == Some(id))
            .ok_or_else(|| format!("환경 {id} 없음"))
    }

    pub fn delete_environment(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM environments WHERE id=?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // --- scenarios ---

    pub fn save_scenario(&self, s: &ScenarioRecord) -> Result<i64, String> {
        // steps_json이 유효한 스텝 배열인지 저장 전에 검증
        serde_json::from_str::<Vec<crate::models::StepDef>>(&s.steps_json)
            .map_err(|e| format!("스텝 JSON이 유효하지 않음: {e}"))?;
        match s.id {
            Some(id) => {
                self.conn
                    .execute(
                        "UPDATE scenarios SET name=?1, description=?2, steps_json=?3 WHERE id=?4",
                        params![s.name, s.description, s.steps_json, id],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(id)
            }
            None => {
                self.conn
                    .execute(
                        "INSERT INTO scenarios (name, description, steps_json) VALUES (?1,?2,?3)",
                        params![s.name, s.description, s.steps_json],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn list_scenarios(&self) -> Result<Vec<ScenarioRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, description, steps_json FROM scenarios ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ScenarioRecord {
                    id: Some(r.get(0)?),
                    name: r.get(1)?,
                    description: r.get(2)?,
                    steps_json: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    pub fn get_scenario(&self, id: i64) -> Result<ScenarioRecord, String> {
        self.list_scenarios()?
            .into_iter()
            .find(|s| s.id == Some(id))
            .ok_or_else(|| format!("시나리오 {id} 없음"))
    }

    pub fn delete_scenario(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM scenarios WHERE id=?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // --- runs / step_results ---

    pub fn create_run(&self, scenario_id: i64, env_id: i64, started_at: &str) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO runs (scenario_id, env_id, status, started_at) VALUES (?1,?2,'running',?3)",
                params![scenario_id, env_id, started_at],
            )
            .map_err(|e| e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_run(&self, run_id: i64, status: &str, finished_at: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE runs SET status=?1, finished_at=?2 WHERE id=?3",
                params![status, finished_at, run_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_runs(&self) -> Result<Vec<RunRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT r.id, r.scenario_id, COALESCE(s.name, '(삭제됨)'), r.env_id, r.status, r.started_at, r.finished_at
                 FROM runs r LEFT JOIN scenarios s ON s.id = r.scenario_id ORDER BY r.id DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(RunRecord {
                    id: r.get(0)?,
                    scenario_id: r.get(1)?,
                    scenario_name: r.get(2)?,
                    env_id: r.get(3)?,
                    status: r.get(4)?,
                    started_at: r.get(5)?,
                    finished_at: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    pub fn save_step_result(&self, r: &StepResultRecord) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO step_results (run_id, step_index, name, status, detail, duration_ms)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![r.run_id, r.step_index, r.name, r.status, r.detail, r.duration_ms],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_step_results(&self, run_id: i64) -> Result<Vec<StepResultRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT run_id, step_index, name, status, detail, duration_ms FROM step_results WHERE run_id=?1 ORDER BY step_index")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([run_id], |r| {
                Ok(StepResultRecord {
                    run_id: r.get(0)?,
                    step_index: r.get(1)?,
                    name: r.get(2)?,
                    status: r.get(3)?,
                    detail: r.get(4)?,
                    duration_ms: r.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }
}

// --- OS 키체인 (환경 비밀번호) ---
// 주의: 단위 테스트 없음 — OS 키체인을 건드리므로 Task 12 이후 수동 검증.

const KEYRING_SERVICE: &str = "contrabass-test-tool";

pub fn save_password(env_id: i64, password: &str) -> Result<(), String> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("env-{env_id}"))
        .and_then(|e| e.set_password(password))
        .map_err(|e| format!("키체인 저장 실패: {e}"))
}

pub fn get_password(env_id: i64) -> Result<String, String> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("env-{env_id}"))
        .and_then(|e| e.get_password())
        .map_err(|e| format!("키체인 조회 실패 (환경 비밀번호를 다시 저장하세요): {e}"))
}

pub fn delete_password(env_id: i64) {
    if let Ok(e) = keyring::Entry::new(KEYRING_SERVICE, &format!("env-{env_id}")) {
        let _ = e.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Environment {
        Environment {
            id: None,
            name: "dev".into(),
            keystone_url: "http://ks:5000".into(),
            user_name: "admin".into(),
            user_domain: "Default".into(),
            project_name: "admin".into(),
            project_domain: "Default".into(),
            mq_url: "amqp://guest:guest@mq:5672/%2f".into(),
            mq_exchanges: "nova,neutron,cinder".into(),
            endpoints: std::collections::HashMap::from([("nova".to_string(), "http://nova:8774/v2.1".to_string())]),
        }
    }

    #[test]
    fn environment_crud() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_environment(&env()).unwrap();
        let mut loaded = store.get_environment(id).unwrap();
        assert_eq!(loaded.name, "dev");
        assert_eq!(loaded.endpoints["nova"], "http://nova:8774/v2.1");

        loaded.name = "dev2".into();
        store.save_environment(&loaded).unwrap();
        assert_eq!(store.get_environment(id).unwrap().name, "dev2");

        store.delete_environment(id).unwrap();
        assert!(store.list_environments().unwrap().is_empty());
    }

    #[test]
    fn scenario_crud_validates_steps_json() {
        let store = Store::open_in_memory().unwrap();
        let good = ScenarioRecord {
            id: None,
            name: "s1".into(),
            description: String::new(),
            steps_json: r#"[{"name":"대기","type":"sleep","seconds":1}]"#.into(),
        };
        let id = store.save_scenario(&good).unwrap();
        assert_eq!(store.get_scenario(id).unwrap().name, "s1");

        let bad = ScenarioRecord { steps_json: "not json".into(), ..good };
        assert!(store.save_scenario(&bad).is_err());
    }

    #[test]
    fn run_lifecycle_and_step_results() {
        let store = Store::open_in_memory().unwrap();
        let sid = store
            .save_scenario(&ScenarioRecord {
                id: None,
                name: "s".into(),
                description: String::new(),
                steps_json: "[]".into(),
            })
            .unwrap();
        let run_id = store.create_run(sid, 1, "2026-07-06T00:00:00Z").unwrap();
        store
            .save_step_result(&StepResultRecord {
                run_id,
                step_index: 0,
                name: "스텝".into(),
                status: "passed".into(),
                detail: "HTTP 202".into(),
                duration_ms: 42,
            })
            .unwrap();
        store.finish_run(run_id, "passed", "2026-07-06T00:01:00Z").unwrap();

        let runs = store.list_runs().unwrap();
        assert_eq!(runs[0].status, "passed");
        assert_eq!(runs[0].scenario_name, "s");
        let results = store.list_step_results(run_id).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].duration_ms, 42);
    }
}
```

`lib.rs`에 `pub mod store;` 추가.

- [ ] **Step 2: 테스트 실행**

```bash
cargo test --manifest-path src-tauri/Cargo.toml store
```
Expected: 3개 테스트 PASS

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: SQLite 저장소 및 키체인 비밀번호 저장"
```

---

### Task 12: Tauri commands 배선 (commands.rs, lib.rs)

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (전체 재작성)

- [ ] **Step 1: commands.rs 작성**

`src-tauri/src/commands.rs`:

```rust
use crate::engine::{self, ProgressSink, RunInput, StepOutcome, StepStatus, TokenRefresher};
use crate::events::EventBus;
use crate::keystone::{KeystoneAuth, KeystoneClient};
use crate::models::{Scenario, StepDef, Vars};
use crate::mq;
use crate::store::{self, Environment, RunRecord, ScenarioRecord, StepResultRecord, Store};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

pub struct AppState {
    pub db: Mutex<Store>,
    pub active_runs: Mutex<HashMap<i64, CancellationToken>>,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

// --- 환경 ---

#[tauri::command]
pub fn list_environments(state: State<AppState>) -> Result<Vec<Environment>, String> {
    state.db.lock().unwrap().list_environments()
}

#[tauri::command]
pub fn save_environment(
    state: State<AppState>,
    env: Environment,
    password: Option<String>,
) -> Result<i64, String> {
    let id = state.db.lock().unwrap().save_environment(&env)?;
    if let Some(pw) = password {
        store::save_password(id, &pw)?;
    }
    Ok(id)
}

#[tauri::command]
pub fn delete_environment(state: State<AppState>, id: i64) -> Result<(), String> {
    state.db.lock().unwrap().delete_environment(id)?;
    store::delete_password(id);
    Ok(())
}

// --- 시나리오 ---

#[tauri::command]
pub fn list_scenarios(state: State<AppState>) -> Result<Vec<ScenarioRecord>, String> {
    state.db.lock().unwrap().list_scenarios()
}

#[tauri::command]
pub fn save_scenario(state: State<AppState>, rec: ScenarioRecord) -> Result<i64, String> {
    state.db.lock().unwrap().save_scenario(&rec)
}

#[tauri::command]
pub fn delete_scenario(state: State<AppState>, id: i64) -> Result<(), String> {
    state.db.lock().unwrap().delete_scenario(id)
}

#[tauri::command]
pub fn export_scenario(state: State<AppState>, id: i64, path: String) -> Result<(), String> {
    let s = state.db.lock().unwrap().get_scenario(id)?;
    let steps: serde_json::Value =
        serde_json::from_str(&s.steps_json).map_err(|e| e.to_string())?;
    let out = serde_json::json!({"name": s.name, "description": s.description, "steps": steps});
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("파일 쓰기 실패: {e}"))
}

#[tauri::command]
pub fn import_scenario(state: State<AppState>, path: String) -> Result<i64, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("파일 읽기 실패: {e}"))?;
    let sc: Scenario =
        serde_json::from_str(&text).map_err(|e| format!("시나리오 JSON 파싱 실패: {e}"))?;
    state.db.lock().unwrap().save_scenario(&ScenarioRecord {
        id: None,
        name: sc.name,
        description: sc.description,
        steps_json: serde_json::to_string(&sc.steps).map_err(|e| e.to_string())?,
    })
}

// --- 실행 히스토리 ---

#[tauri::command]
pub fn list_runs(state: State<AppState>) -> Result<Vec<RunRecord>, String> {
    state.db.lock().unwrap().list_runs()
}

#[tauri::command]
pub fn list_step_results(state: State<AppState>, run_id: i64) -> Result<Vec<StepResultRecord>, String> {
    state.db.lock().unwrap().list_step_results(run_id)
}

// --- 실행 ---

struct TauriSink {
    app: AppHandle,
    run_id: i64,
}

impl ProgressSink for TauriSink {
    fn step_started(&self, index: usize, name: &str) {
        let _ = self.app.emit(
            "step-started",
            serde_json::json!({"run_id": self.run_id, "index": index, "name": name}),
        );
    }
    fn step_finished(&self, outcome: &StepOutcome) {
        let _ = self.app.emit(
            "step-finished",
            serde_json::json!({"run_id": self.run_id, "outcome": outcome}),
        );
    }
}

fn status_str(s: &StepStatus) -> &'static str {
    match s {
        StepStatus::Passed => "passed",
        StepStatus::Failed => "failed",
        StepStatus::Skipped => "skipped",
    }
}

#[tauri::command]
pub async fn run_scenario(
    app: AppHandle,
    state: State<'_, AppState>,
    scenario_id: i64,
    env_id: i64,
) -> Result<i64, String> {
    let (env, scenario_rec) = {
        let db = state.db.lock().unwrap();
        (db.get_environment(env_id)?, db.get_scenario(scenario_id)?)
    };
    let steps: Vec<StepDef> =
        serde_json::from_str(&scenario_rec.steps_json).map_err(|e| e.to_string())?;
    let scenario = Scenario {
        name: scenario_rec.name,
        description: scenario_rec.description,
        steps,
    };
    let password = store::get_password(env_id)?;

    // 1) Keystone 토큰 (실패 시 실행 자체를 시작하지 않음)
    let ks = Arc::new(KeystoneClient::new(
        reqwest::Client::new(),
        KeystoneAuth {
            auth_url: env.keystone_url.clone(),
            user_name: env.user_name.clone(),
            user_domain: env.user_domain.clone(),
            password,
            project_name: env.project_name.clone(),
            project_domain: env.project_domain.clone(),
        },
    ));
    let token = ks.get_token().await?;

    // 2) MQ 소비자 시작 (설계: 접속 실패 시 실행 시작 실패)
    let bus = EventBus::new();
    let cancel = CancellationToken::new();
    let exchanges: Vec<String> = env
        .mq_exchanges
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    mq::start_consumer(&env.mq_url, &exchanges, bus.clone(), cancel.clone()).await?;

    // 3) 내장 변수
    let mut vars = Vars::new();
    vars.insert("auth_token".into(), token);
    for (svc, url) in &env.endpoints {
        vars.insert(format!("base_url.{svc}"), url.trim_end_matches('/').to_string());
    }

    // 4) run 레코드 생성 + 백그라운드 실행
    let run_id = state.db.lock().unwrap().create_run(scenario_id, env_id, &now())?;
    state.active_runs.lock().unwrap().insert(run_id, cancel.clone());

    let refresher: TokenRefresher = {
        let ks = ks.clone();
        Arc::new(move || {
            let ks = ks.clone();
            Box::pin(async move { ks.refresh_token().await })
        })
    };

    let sink = TauriSink { app: app.clone(), run_id };
    let input = RunInput {
        scenario,
        vars,
        bus,
        cancel: cancel.clone(),
        token_refresher: Some(refresher),
    };
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let outcomes = engine::run(input, &sink).await;
        let state = app2.state::<AppState>();
        let status = if cancel.is_cancelled() {
            "cancelled"
        } else if outcomes.iter().any(|o| o.status == StepStatus::Failed) {
            "failed"
        } else {
            "passed"
        };
        {
            let db = state.db.lock().unwrap();
            for o in &outcomes {
                let _ = db.save_step_result(&StepResultRecord {
                    run_id,
                    step_index: o.index as i64,
                    name: o.name.clone(),
                    status: status_str(&o.status).to_string(),
                    detail: o.detail.clone(),
                    duration_ms: o.duration_ms as i64,
                });
            }
            let _ = db.finish_run(run_id, status, &now());
        }
        state.active_runs.lock().unwrap().remove(&run_id);
        cancel.cancel(); // MQ 소비자 태스크 종료
        let _ = app2.emit("run-finished", serde_json::json!({"run_id": run_id, "status": status}));
    });
    Ok(run_id)
}

#[tauri::command]
pub fn cancel_run(state: State<AppState>, run_id: i64) -> Result<(), String> {
    match state.active_runs.lock().unwrap().get(&run_id) {
        Some(t) => {
            t.cancel();
            Ok(())
        }
        None => Err(format!("실행 {run_id}은(는) 진행 중이 아님")),
    }
}
```

- [ ] **Step 2: lib.rs 전체 재작성**

`src-tauri/src/lib.rs`:

```rust
pub mod assertion;
pub mod commands;
pub mod engine;
pub mod events;
pub mod http;
pub mod keystone;
pub mod matcher;
pub mod models;
pub mod mq;
pub mod store;
pub mod template;

use commands::AppState;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = store::Store::open(&dir.join("data.sqlite"))
                .map_err(std::io::Error::other)?;
            app.manage(AppState {
                db: Mutex::new(db),
                active_runs: Mutex::new(Default::default()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_environments,
            commands::save_environment,
            commands::delete_environment,
            commands::list_scenarios,
            commands::save_scenario,
            commands::delete_scenario,
            commands::export_scenario,
            commands::import_scenario,
            commands::list_runs,
            commands::list_step_results,
            commands::run_scenario,
            commands::cancel_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

주의: 스캐폴드가 만든 `greet` command와 그 등록부는 삭제한다.

- [ ] **Step 3: 전체 테스트 + 빌드 확인**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```
Expected: 지금까지의 모든 테스트 PASS, check 에러 없음

- [ ] **Step 4: 커밋**

```bash
git add -A && git commit -m "feat: Tauri commands 및 실행 배선"
```

---

### Task 13: 프론트 타입 + API 래퍼 (types.ts, api.ts)

**Files:**
- Create: `src/types.ts`, `src/api.ts`
- Delete: 스캐폴드가 만든 데모 코드 (`src/App.tsx`의 greet 관련 내용은 Task 16에서 교체)

- [ ] **Step 1: types.ts 작성**

`src/types.ts`:

```typescript
export interface Capture { var: string; json_path: string }
export interface Condition { json_path: string; equals: string }
export type AssertOp = 'eq' | 'contains' | 'regex'

export type Action =
  | { type: 'http_call'; method: string; url: string; headers?: Record<string, string>; body?: string | null; expect_status?: number | null; captures?: Capture[] }
  | { type: 'wait_event'; event_type: string; conditions?: Condition[]; timeout_secs: number }
  | { type: 'assert'; left: string; op: AssertOp; right: string }
  | { type: 'sleep'; seconds: number }

export type StepDef = { name: string; cleanup?: boolean } & Action

export interface Environment {
  id: number | null
  name: string
  keystone_url: string
  user_name: string
  user_domain: string
  project_name: string
  project_domain: string
  mq_url: string
  mq_exchanges: string
  endpoints: Record<string, string>
}

export interface ScenarioRecord { id: number | null; name: string; description: string; steps_json: string }

export interface RunRecord {
  id: number
  scenario_id: number
  scenario_name: string
  env_id: number
  status: string
  started_at: string
  finished_at: string | null
}

export interface StepResultRecord {
  run_id: number
  step_index: number
  name: string
  status: string
  detail: string
  duration_ms: number
}

export type StepStatus = 'passed' | 'failed' | 'skipped'

export interface StepOutcome { index: number; name: string; status: StepStatus; detail: string; duration_ms: number }
```

- [ ] **Step 2: api.ts 작성**

`src/api.ts`:

```typescript
import { invoke } from '@tauri-apps/api/core'
import type { Environment, RunRecord, ScenarioRecord, StepResultRecord } from './types'

export const listEnvironments = () => invoke<Environment[]>('list_environments')
export const saveEnvironment = (env: Environment, password: string | null) =>
  invoke<number>('save_environment', { env, password })
export const deleteEnvironment = (id: number) => invoke<void>('delete_environment', { id })

export const listScenarios = () => invoke<ScenarioRecord[]>('list_scenarios')
export const saveScenario = (rec: ScenarioRecord) => invoke<number>('save_scenario', { rec })
export const deleteScenario = (id: number) => invoke<void>('delete_scenario', { id })
export const exportScenario = (id: number, path: string) => invoke<void>('export_scenario', { id, path })
export const importScenario = (path: string) => invoke<number>('import_scenario', { path })

export const listRuns = () => invoke<RunRecord[]>('list_runs')
export const listStepResults = (runId: number) => invoke<StepResultRecord[]>('list_step_results', { runId })

export const runScenario = (scenarioId: number, envId: number) =>
  invoke<number>('run_scenario', { scenarioId, envId })
export const cancelRun = (runId: number) => invoke<void>('cancel_run', { runId })
```

- [ ] **Step 3: 타입 체크 후 커밋**

```bash
npx tsc --noEmit
git add -A && git commit -m "feat: 프론트 타입 및 Tauri API 래퍼"
```

---

### Task 14: 환경 프로필 화면 (EnvironmentsView.tsx)

**Files:**
- Create: `src/views/EnvironmentsView.tsx`

- [ ] **Step 1: 컴포넌트 작성**

`src/views/EnvironmentsView.tsx`:

```tsx
import { useEffect, useState } from 'react'
import * as api from '../api'
import type { Environment } from '../types'

const empty: Environment = {
  id: null, name: '', keystone_url: '', user_name: '', user_domain: 'Default',
  project_name: '', project_domain: 'Default', mq_url: '', mq_exchanges: 'nova,neutron,cinder',
  endpoints: {},
}

export default function EnvironmentsView() {
  const [envs, setEnvs] = useState<Environment[]>([])
  const [form, setForm] = useState<Environment>(empty)
  const [password, setPassword] = useState('')
  const [endpointsText, setEndpointsText] = useState('{}')
  const [error, setError] = useState('')

  const reload = () => api.listEnvironments().then(setEnvs).catch(e => setError(String(e)))
  useEffect(() => { reload() }, [])

  const edit = (env: Environment) => {
    setForm(env)
    setEndpointsText(JSON.stringify(env.endpoints, null, 2))
    setPassword('')
  }

  const save = async () => {
    setError('')
    let endpoints: Record<string, string>
    try {
      endpoints = JSON.parse(endpointsText)
    } catch {
      setError('엔드포인트는 JSON 객체여야 합니다. 예: {"nova": "http://host:8774/v2.1"}')
      return
    }
    try {
      await api.saveEnvironment({ ...form, endpoints }, password || null)
      setForm(empty); setEndpointsText('{}'); setPassword('')
      reload()
    } catch (e) { setError(String(e)) }
  }

  const field = (key: keyof Environment, label: string, placeholder = '') => (
    <label className="field">{label}
      <input value={String(form[key] ?? '')} placeholder={placeholder}
        onChange={e => setForm({ ...form, [key]: e.target.value })} />
    </label>
  )

  return (
    <div className="two-col">
      <div>
        <h2>환경 목록</h2>
        <ul className="list">
          {envs.map(env => (
            <li key={env.id}>
              <button onClick={() => edit(env)}>{env.name}</button>
              <button className="danger" onClick={() => api.deleteEnvironment(env.id!).then(reload)}>삭제</button>
            </li>
          ))}
        </ul>
      </div>
      <div>
        <h2>{form.id ? '환경 수정' : '새 환경'}</h2>
        {field('name', '이름', 'dev')}
        {field('keystone_url', 'Keystone URL', 'http://keystone:5000')}
        {field('user_name', '사용자')}
        {field('user_domain', '사용자 도메인')}
        {field('project_name', '프로젝트')}
        {field('project_domain', '프로젝트 도메인')}
        <label className="field">비밀번호 (OS 키체인에 저장)
          <input type="password" value={password} onChange={e => setPassword(e.target.value)}
            placeholder={form.id ? '변경할 때만 입력' : ''} />
        </label>
        {field('mq_url', 'RabbitMQ URL', 'amqp://user:pw@host:5672/%2f')}
        {field('mq_exchanges', 'notification exchange (쉼표 구분)', 'nova,neutron,cinder')}
        <label className="field">서비스 엔드포인트 (JSON)
          <textarea rows={5} value={endpointsText} onChange={e => setEndpointsText(e.target.value)} />
        </label>
        {error && <p className="error">{error}</p>}
        <button onClick={save}>저장</button>
        {form.id && <button onClick={() => edit(empty)}>새로 만들기</button>}
      </div>
    </div>
  )
}
```

- [ ] **Step 2: 타입 체크 후 커밋**

```bash
npx tsc --noEmit
git add -A && git commit -m "feat: 환경 프로필 화면"
```

---

### Task 15: 프리셋 + 시나리오 빌더 (presets.ts, StepForm.tsx, ScenarioBuilder.tsx)

**Files:**
- Create: `src/presets.ts`, `src/presets.test.ts`, `src/views/StepForm.tsx`, `src/views/ScenarioBuilder.tsx`
- Modify: `package.json` (vitest), `src-tauri/capabilities/default.json` (dialog 권한)

- [ ] **Step 1: vitest 설치**

```bash
npm install -D vitest
```

`package.json`의 scripts에 추가: `"test": "vitest run"`

- [ ] **Step 2: 실패하는 프리셋 테스트 작성**

`src/presets.test.ts`:

```typescript
import { describe, expect, it } from 'vitest'
import { presets } from './presets'

const byId = (id: string) => {
  const p = presets.find(p => p.id === id)
  if (!p) throw new Error(`preset ${id} 없음`)
  return p
}

describe('presets', () => {
  it('인스턴스 생성 = http_call + wait_event, server_id 캡처와 참조가 연결된다', () => {
    const steps = byId('create_instance').expand({
      name: 'vm1', image_ref: 'img-1', flavor_ref: 'f-1', network_id: 'net-1',
    })
    expect(steps).toHaveLength(2)
    const [call, wait] = steps
    if (call.type !== 'http_call' || wait.type !== 'wait_event') throw new Error('스텝 타입 불일치')
    expect(call.captures?.[0]).toEqual({ var: 'server_id', json_path: '$.server.id' })
    expect(call.body).toContain('vm1')
    expect(wait.event_type).toBe('compute.instance.create.end')
    expect(wait.conditions?.[0].equals).toBe('{{server_id}}')
  })

  it('인스턴스 삭제 프리셋은 cleanup으로 표시된다', () => {
    const steps = byId('delete_instance').expand({ server_id_var: 'server_id' })
    expect(steps.every(s => s.cleanup)).toBe(true)
  })

  it('모든 프리셋이 최소 1개 스텝을 만든다', () => {
    for (const p of presets) {
      const input = Object.fromEntries(p.fields.map(f => [f.key, 'x']))
      expect(p.expand(input).length).toBeGreaterThan(0)
    }
  })
})
```

- [ ] **Step 3: 테스트 실패 확인**

```bash
npm test
```
Expected: FAIL — `Cannot find module './presets'`

- [ ] **Step 4: presets.ts 구현**

`src/presets.ts`:

```typescript
import type { StepDef } from './types'

export interface PresetField { key: string; label: string; placeholder?: string }

export interface Preset {
  id: string
  label: string
  fields: PresetField[]
  expand: (input: Record<string, string>) => StepDef[]
}

const token = { 'X-Auth-Token': '{{auth_token}}' }

export const presets: Preset[] = [
  {
    id: 'create_instance',
    label: '인스턴스 생성',
    fields: [
      { key: 'name', label: '서버 이름' },
      { key: 'image_ref', label: '이미지 ID' },
      { key: 'flavor_ref', label: '플레이버 ID' },
      { key: 'network_id', label: '네트워크 ID' },
    ],
    expand: i => [
      {
        name: `인스턴스 생성: ${i.name}`,
        type: 'http_call',
        method: 'POST',
        url: '{{base_url.nova}}/servers',
        headers: token,
        body: JSON.stringify({
          server: {
            name: i.name, imageRef: i.image_ref, flavorRef: i.flavor_ref,
            networks: [{ uuid: i.network_id }],
          },
        }),
        expect_status: 202,
        captures: [{ var: 'server_id', json_path: '$.server.id' }],
      },
      {
        name: '인스턴스 생성 완료 대기',
        type: 'wait_event',
        event_type: 'compute.instance.create.end',
        conditions: [{ json_path: '$.payload.instance_id', equals: '{{server_id}}' }],
        timeout_secs: 600,
      },
    ],
  },
  {
    id: 'delete_instance',
    label: '인스턴스 삭제 (cleanup)',
    fields: [{ key: 'server_id_var', label: '서버 ID 변수명', placeholder: 'server_id' }],
    expand: i => {
      const v = i.server_id_var || 'server_id'
      return [
        {
          name: '인스턴스 삭제', cleanup: true, type: 'http_call', method: 'DELETE',
          url: `{{base_url.nova}}/servers/{{${v}}}`, headers: token, expect_status: 204,
        },
        {
          name: '인스턴스 삭제 완료 대기', cleanup: true, type: 'wait_event',
          event_type: 'compute.instance.delete.end',
          conditions: [{ json_path: '$.payload.instance_id', equals: `{{${v}}}` }],
          timeout_secs: 120,
        },
      ]
    },
  },
  {
    id: 'create_network',
    label: '네트워크 생성',
    fields: [{ key: 'name', label: '네트워크 이름' }],
    expand: i => [
      {
        name: `네트워크 생성: ${i.name}`, type: 'http_call', method: 'POST',
        url: '{{base_url.neutron}}/v2.0/networks', headers: token,
        body: JSON.stringify({ network: { name: i.name } }),
        expect_status: 201,
        captures: [{ var: 'network_id', json_path: '$.network.id' }],
      },
    ],
  },
  {
    id: 'delete_network',
    label: '네트워크 삭제 (cleanup)',
    fields: [{ key: 'network_id_var', label: '네트워크 ID 변수명', placeholder: 'network_id' }],
    expand: i => {
      const v = i.network_id_var || 'network_id'
      return [{
        name: '네트워크 삭제', cleanup: true, type: 'http_call', method: 'DELETE',
        url: `{{base_url.neutron}}/v2.0/networks/{{${v}}}`, headers: token, expect_status: 204,
      }]
    },
  },
  {
    id: 'create_volume',
    label: '볼륨 생성',
    fields: [
      { key: 'name', label: '볼륨 이름' },
      { key: 'size', label: '크기(GB)', placeholder: '10' },
    ],
    expand: i => [
      {
        name: `볼륨 생성: ${i.name}`, type: 'http_call', method: 'POST',
        url: '{{base_url.cinder}}/volumes', headers: token,
        body: JSON.stringify({ volume: { name: i.name, size: Number(i.size) || 1 } }),
        expect_status: 202,
        captures: [{ var: 'volume_id', json_path: '$.volume.id' }],
      },
      {
        name: '볼륨 생성 완료 대기', type: 'wait_event',
        event_type: 'volume.create.end',
        conditions: [{ json_path: '$.payload.volume_id', equals: '{{volume_id}}' }],
        timeout_secs: 300,
      },
    ],
  },
  {
    id: 'delete_volume',
    label: '볼륨 삭제 (cleanup)',
    fields: [{ key: 'volume_id_var', label: '볼륨 ID 변수명', placeholder: 'volume_id' }],
    expand: i => {
      const v = i.volume_id_var || 'volume_id'
      return [
        {
          name: '볼륨 삭제', cleanup: true, type: 'http_call', method: 'DELETE',
          url: `{{base_url.cinder}}/volumes/{{${v}}}`, headers: token, expect_status: 202,
        },
        {
          name: '볼륨 삭제 완료 대기', cleanup: true, type: 'wait_event',
          event_type: 'volume.delete.end',
          conditions: [{ json_path: '$.payload.volume_id', equals: `{{${v}}}` }],
          timeout_secs: 120,
        },
      ]
    },
  },
]
```

- [ ] **Step 5: 테스트 통과 확인**

```bash
npm test
```
Expected: 3개 테스트 PASS

- [ ] **Step 6: StepForm.tsx 작성 (스텝 1개 편집 폼)**

`src/views/StepForm.tsx`:

```tsx
import type { AssertOp, StepDef } from '../types'

interface Props {
  step: StepDef
  onChange: (s: StepDef) => void
}

export default function StepForm({ step, onChange }: Props) {
  const set = (patch: Partial<StepDef & { [k: string]: unknown }>) =>
    onChange({ ...step, ...patch } as StepDef)

  const common = (
    <>
      <label className="field">스텝 이름
        <input value={step.name} onChange={e => set({ name: e.target.value })} />
      </label>
      <label className="check">
        <input type="checkbox" checked={!!step.cleanup}
          onChange={e => set({ cleanup: e.target.checked })} /> cleanup (실패해도 항상 실행)
      </label>
    </>
  )

  switch (step.type) {
    case 'http_call':
      return (
        <div className="step-form">
          {common}
          <label className="field">메서드
            <select value={step.method} onChange={e => set({ method: e.target.value })}>
              {['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].map(m => <option key={m}>{m}</option>)}
            </select>
          </label>
          <label className="field">URL
            <input value={step.url} onChange={e => set({ url: e.target.value })}
              placeholder="{{base_url.nova}}/servers" />
          </label>
          <label className="field">헤더 (JSON)
            <textarea rows={2} value={JSON.stringify(step.headers ?? {})}
              onChange={e => { try { set({ headers: JSON.parse(e.target.value) }) } catch { /* 입력 중 무시 */ } }} />
          </label>
          <label className="field">바디
            <textarea rows={4} value={step.body ?? ''} onChange={e => set({ body: e.target.value || null })} />
          </label>
          <label className="field">기대 상태코드
            <input value={step.expect_status ?? ''} placeholder="예: 202 (비우면 검사 안 함)"
              onChange={e => set({ expect_status: e.target.value ? Number(e.target.value) : null })} />
          </label>
          <label className="field">변수 캡처 (JSON 배열)
            <textarea rows={2} value={JSON.stringify(step.captures ?? [])}
              placeholder='[{"var":"server_id","json_path":"$.server.id"}]'
              onChange={e => { try { set({ captures: JSON.parse(e.target.value) }) } catch { /* 입력 중 무시 */ } }} />
          </label>
        </div>
      )
    case 'wait_event':
      return (
        <div className="step-form">
          {common}
          <label className="field">이벤트 타입
            <input value={step.event_type} placeholder="compute.instance.create.end"
              onChange={e => set({ event_type: e.target.value })} />
          </label>
          <label className="field">조건 (JSON 배열)
            <textarea rows={2} value={JSON.stringify(step.conditions ?? [])}
              placeholder='[{"json_path":"$.payload.instance_id","equals":"{{server_id}}"}]'
              onChange={e => { try { set({ conditions: JSON.parse(e.target.value) }) } catch { /* 입력 중 무시 */ } }} />
          </label>
          <label className="field">타임아웃(초)
            <input value={step.timeout_secs} onChange={e => set({ timeout_secs: Number(e.target.value) || 0 })} />
          </label>
        </div>
      )
    case 'assert':
      return (
        <div className="step-form">
          {common}
          <label className="field">좌변 <input value={step.left} placeholder="{{server_id}}"
            onChange={e => set({ left: e.target.value })} /></label>
          <label className="field">연산
            <select value={step.op} onChange={e => set({ op: e.target.value as AssertOp })}>
              <option value="eq">같음</option>
              <option value="contains">포함</option>
              <option value="regex">정규식</option>
            </select>
          </label>
          <label className="field">우변 <input value={step.right}
            onChange={e => set({ right: e.target.value })} /></label>
        </div>
      )
    case 'sleep':
      return (
        <div className="step-form">
          {common}
          <label className="field">대기(초)
            <input value={step.seconds} onChange={e => set({ seconds: Number(e.target.value) || 0 })} />
          </label>
        </div>
      )
  }
}
```

- [ ] **Step 7: ScenarioBuilder.tsx 작성**

`src/views/ScenarioBuilder.tsx`:

```tsx
import { useEffect, useState } from 'react'
import { open, save } from '@tauri-apps/plugin-dialog'
import * as api from '../api'
import { presets } from '../presets'
import type { ScenarioRecord, StepDef } from '../types'
import StepForm from './StepForm'

const blankStep = (type: StepDef['type']): StepDef => {
  switch (type) {
    case 'http_call': return { name: '새 HTTP 호출', type, method: 'GET', url: '', headers: { 'X-Auth-Token': '{{auth_token}}' } }
    case 'wait_event': return { name: '새 이벤트 대기', type, event_type: '', conditions: [], timeout_secs: 300 }
    case 'assert': return { name: '새 검증', type, left: '', op: 'eq', right: '' }
    case 'sleep': return { name: '새 대기', type, seconds: 5 }
  }
}

export default function ScenarioBuilder() {
  const [scenarios, setScenarios] = useState<ScenarioRecord[]>([])
  const [current, setCurrent] = useState<ScenarioRecord>({ id: null, name: '', description: '', steps_json: '[]' })
  const [steps, setSteps] = useState<StepDef[]>([])
  const [presetId, setPresetId] = useState(presets[0].id)
  const [presetInput, setPresetInput] = useState<Record<string, string>>({})
  const [error, setError] = useState('')

  const reload = () => api.listScenarios().then(setScenarios).catch(e => setError(String(e)))
  useEffect(() => { reload() }, [])

  const edit = (rec: ScenarioRecord) => {
    setCurrent(rec)
    setSteps(JSON.parse(rec.steps_json))
  }

  const newScenario = () => edit({ id: null, name: '', description: '', steps_json: '[]' })

  const saveCurrent = async () => {
    setError('')
    try {
      const id = await api.saveScenario({ ...current, steps_json: JSON.stringify(steps) })
      setCurrent({ ...current, id })
      reload()
    } catch (e) { setError(String(e)) }
  }

  const move = (i: number, delta: -1 | 1) => {
    const j = i + delta
    if (j < 0 || j >= steps.length) return
    const next = [...steps]
    ;[next[i], next[j]] = [next[j], next[i]]
    setSteps(next)
  }

  const addPreset = () => {
    const preset = presets.find(p => p.id === presetId)!
    setSteps([...steps, ...preset.expand(presetInput)])
    setPresetInput({})
  }

  const doExport = async () => {
    if (current.id == null) { setError('먼저 저장하세요'); return }
    const path = await save({ defaultPath: `${current.name || 'scenario'}.json` })
    if (path) await api.exportScenario(current.id, path).catch(e => setError(String(e)))
  }

  const doImport = async () => {
    const path = await open({ multiple: false, filters: [{ name: 'JSON', extensions: ['json'] }] })
    if (typeof path === 'string') {
      await api.importScenario(path).catch(e => setError(String(e)))
      reload()
    }
  }

  const preset = presets.find(p => p.id === presetId)!

  return (
    <div className="two-col">
      <div>
        <h2>시나리오</h2>
        <button onClick={newScenario}>새 시나리오</button>
        <button onClick={doImport}>가져오기</button>
        <ul className="list">
          {scenarios.map(s => (
            <li key={s.id}>
              <button onClick={() => edit(s)}>{s.name}</button>
              <button className="danger" onClick={() => api.deleteScenario(s.id!).then(reload)}>삭제</button>
            </li>
          ))}
        </ul>
      </div>
      <div>
        <h2>{current.id ? '시나리오 편집' : '새 시나리오'}</h2>
        <label className="field">이름
          <input value={current.name} onChange={e => setCurrent({ ...current, name: e.target.value })} />
        </label>
        <label className="field">설명
          <input value={current.description} onChange={e => setCurrent({ ...current, description: e.target.value })} />
        </label>

        <h3>스텝 ({steps.length})</h3>
        {steps.map((s, i) => (
          <details key={i} className="step">
            <summary>
              {i + 1}. [{s.type}] {s.name} {s.cleanup ? '🧹' : ''}
              <span className="step-actions">
                <button onClick={e => { e.preventDefault(); move(i, -1) }}>↑</button>
                <button onClick={e => { e.preventDefault(); move(i, 1) }}>↓</button>
                <button className="danger" onClick={e => { e.preventDefault(); setSteps(steps.filter((_, j) => j !== i)) }}>삭제</button>
              </span>
            </summary>
            <StepForm step={s} onChange={ns => setSteps(steps.map((old, j) => (j === i ? ns : old)))} />
          </details>
        ))}

        <h3>스텝 추가</h3>
        <div className="add-row">
          {(['http_call', 'wait_event', 'assert', 'sleep'] as const).map(t => (
            <button key={t} onClick={() => setSteps([...steps, blankStep(t)])}>+ {t}</button>
          ))}
        </div>
        <div className="add-row">
          <select value={presetId} onChange={e => setPresetId(e.target.value)}>
            {presets.map(p => <option key={p.id} value={p.id}>{p.label}</option>)}
          </select>
          {preset.fields.map(f => (
            <input key={f.key} placeholder={f.placeholder || f.label} value={presetInput[f.key] ?? ''}
              onChange={e => setPresetInput({ ...presetInput, [f.key]: e.target.value })} />
          ))}
          <button onClick={addPreset}>프리셋 추가</button>
        </div>

        {error && <p className="error">{error}</p>}
        <button onClick={saveCurrent}>저장</button>
        <button onClick={doExport}>내보내기</button>
      </div>
    </div>
  )
}
```

- [ ] **Step 8: dialog 플러그인 권한 추가**

`src-tauri/capabilities/default.json`의 `permissions` 배열에 추가:

```json
"dialog:default"
```

- [ ] **Step 9: 타입 체크 + 테스트 후 커밋**

```bash
npx tsc --noEmit && npm test
git add -A && git commit -m "feat: 시나리오 빌더 및 OpenStack 프리셋"
```

---

### Task 16: 실행 화면 + 앱 조립 (RunView.tsx, App.tsx)

**Files:**
- Create: `src/views/RunView.tsx`
- Modify: `src/App.tsx` (전체 교체), `src/App.css` (전체 교체)

- [ ] **Step 1: RunView.tsx 작성**

`src/views/RunView.tsx`:

```tsx
import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import * as api from '../api'
import type { Environment, ScenarioRecord, StepDef, StepOutcome } from '../types'

interface StepRow {
  name: string
  type: string
  status: 'pending' | 'running' | 'passed' | 'failed' | 'skipped'
  detail: string
  duration_ms: number
}

export default function RunView() {
  const [scenarios, setScenarios] = useState<ScenarioRecord[]>([])
  const [envs, setEnvs] = useState<Environment[]>([])
  const [scenarioId, setScenarioId] = useState<number | null>(null)
  const [envId, setEnvId] = useState<number | null>(null)
  const [rows, setRows] = useState<StepRow[]>([])
  const [runStatus, setRunStatus] = useState<string>('')
  const [error, setError] = useState('')
  const runIdRef = useRef<number | null>(null)

  useEffect(() => {
    api.listScenarios().then(setScenarios)
    api.listEnvironments().then(setEnvs)

    const unlistens = [
      listen<{ run_id: number; index: number }>('step-started', e => {
        if (e.payload.run_id !== runIdRef.current) return
        setRows(rows => rows.map((r, i) => (i === e.payload.index ? { ...r, status: 'running' } : r)))
      }),
      listen<{ run_id: number; outcome: StepOutcome }>('step-finished', e => {
        if (e.payload.run_id !== runIdRef.current) return
        const o = e.payload.outcome
        setRows(rows => rows.map((r, i) =>
          i === o.index ? { ...r, status: o.status, detail: o.detail, duration_ms: o.duration_ms } : r))
      }),
      listen<{ run_id: number; status: string }>('run-finished', e => {
        if (e.payload.run_id !== runIdRef.current) return
        setRunStatus(e.payload.status)
      }),
    ]
    return () => { unlistens.forEach(p => p.then(un => un())) }
  }, [])

  const start = async () => {
    if (scenarioId == null || envId == null) { setError('시나리오와 환경을 선택하세요'); return }
    setError('')
    const rec = scenarios.find(s => s.id === scenarioId)!
    const steps: StepDef[] = JSON.parse(rec.steps_json)
    setRows(steps.map(s => ({ name: s.name, type: s.type, status: 'pending', detail: '', duration_ms: 0 })))
    setRunStatus('running')
    try {
      runIdRef.current = await api.runScenario(scenarioId, envId)
    } catch (e) {
      setRunStatus('')
      setError(String(e))
    }
  }

  const cancel = () => {
    if (runIdRef.current != null) api.cancelRun(runIdRef.current).catch(e => setError(String(e)))
  }

  const icon = (s: StepRow['status']) =>
    ({ pending: '⚪', running: '🔵', passed: '✅', failed: '❌', skipped: '⏭️' })[s]

  return (
    <div>
      <h2>시나리오 실행</h2>
      <div className="add-row">
        <select value={scenarioId ?? ''} onChange={e => setScenarioId(e.target.value ? Number(e.target.value) : null)}>
          <option value="">시나리오 선택</option>
          {scenarios.map(s => <option key={s.id} value={s.id!}>{s.name}</option>)}
        </select>
        <select value={envId ?? ''} onChange={e => setEnvId(e.target.value ? Number(e.target.value) : null)}>
          <option value="">환경 선택</option>
          {envs.map(e2 => <option key={e2.id} value={e2.id!}>{e2.name}</option>)}
        </select>
        <button onClick={start} disabled={runStatus === 'running'}>실행</button>
        {runStatus === 'running' && <button className="danger" onClick={cancel}>취소</button>}
      </div>

      {runStatus && <p>상태: <strong>{runStatus}</strong></p>}
      {error && <p className="error">{error}</p>}

      <ol className="run-steps">
        {rows.map((r, i) => (
          <li key={i}>
            <div>{icon(r.status)} [{r.type}] {r.name}
              {r.duration_ms > 0 && <span className="dim"> — {r.duration_ms}ms</span>}
            </div>
            {r.detail && <pre className="detail">{r.detail}</pre>}
          </li>
        ))}
      </ol>
    </div>
  )
}
```

- [ ] **Step 2: App.tsx 전체 교체**

`src/App.tsx`:

```tsx
import { useState } from 'react'
import './App.css'
import EnvironmentsView from './views/EnvironmentsView'
import HistoryView from './views/HistoryView'
import RunView from './views/RunView'
import ScenarioBuilder from './views/ScenarioBuilder'

const tabs = [
  { key: 'run', label: '실행', el: <RunView /> },
  { key: 'scenarios', label: '시나리오', el: <ScenarioBuilder /> },
  { key: 'envs', label: '환경', el: <EnvironmentsView /> },
  { key: 'history', label: '히스토리', el: <HistoryView /> },
] as const

export default function App() {
  const [tab, setTab] = useState<string>('run')
  return (
    <main>
      <nav className="tabs">
        {tabs.map(t => (
          <button key={t.key} className={tab === t.key ? 'active' : ''} onClick={() => setTab(t.key)}>
            {t.label}
          </button>
        ))}
      </nav>
      {tabs.find(t => t.key === tab)?.el}
    </main>
  )
}
```

주의: HistoryView는 Task 17에서 만든다. 이 태스크에서는 임시로 `src/views/HistoryView.tsx`에 아래 스텁을 두고, Task 17에서 교체한다:

```tsx
export default function HistoryView() {
  return <p>히스토리 (Task 17에서 구현)</p>
}
```

- [ ] **Step 3: App.css 전체 교체**

`src/App.css`:

```css
main { max-width: 1100px; margin: 0 auto; padding: 1rem; font-family: system-ui, sans-serif; }
.tabs { display: flex; gap: 0.5rem; border-bottom: 1px solid #ccc; margin-bottom: 1rem; }
.tabs button { border: none; background: none; padding: 0.5rem 1rem; cursor: pointer; }
.tabs button.active { border-bottom: 2px solid #396cd8; font-weight: bold; }
.two-col { display: grid; grid-template-columns: 260px 1fr; gap: 1.5rem; align-items: start; }
.field { display: block; margin-bottom: 0.5rem; }
.field input, .field select, .field textarea { display: block; width: 100%; box-sizing: border-box; padding: 0.3rem; }
.check { display: block; margin-bottom: 0.5rem; }
.list { list-style: none; padding: 0; }
.list li { display: flex; gap: 0.4rem; margin-bottom: 0.3rem; }
.error { color: #c00; }
.dim { color: #888; }
.danger { color: #c00; }
.step { border: 1px solid #ddd; border-radius: 4px; margin-bottom: 0.4rem; padding: 0.3rem; }
.step summary { cursor: pointer; }
.step-actions { float: right; display: inline-flex; gap: 0.3rem; }
.step-form { padding: 0.5rem; }
.add-row { display: flex; gap: 0.4rem; margin-bottom: 0.6rem; flex-wrap: wrap; align-items: center; }
.run-steps li { margin-bottom: 0.5rem; }
.detail { background: #f6f6f6; padding: 0.4rem; max-height: 180px; overflow: auto; white-space: pre-wrap; font-size: 0.8rem; }
table.history { border-collapse: collapse; width: 100%; }
table.history th, table.history td { border: 1px solid #ddd; padding: 0.3rem 0.5rem; text-align: left; }
```

- [ ] **Step 4: 타입 체크 + 앱 실행 확인**

```bash
npx tsc --noEmit
npm run tauri dev
```
Expected: 앱 창이 뜨고 4개 탭 전환이 동작. 환경/시나리오 생성 → 실행 탭에서 스텝 목록이 표시되는지 확인 (OpenStack 없이는 Keystone 접속 에러가 정상).

- [ ] **Step 5: 커밋**

```bash
git add -A && git commit -m "feat: 실행 화면 및 탭 네비게이션"
```

---

### Task 17: 히스토리 화면 (HistoryView.tsx)

**Files:**
- Modify: `src/views/HistoryView.tsx` (스텁 교체)

- [ ] **Step 1: HistoryView.tsx 구현**

`src/views/HistoryView.tsx` 전체 교체:

```tsx
import { useEffect, useState } from 'react'
import * as api from '../api'
import type { RunRecord, StepResultRecord } from '../types'

export default function HistoryView() {
  const [runs, setRuns] = useState<RunRecord[]>([])
  const [selected, setSelected] = useState<number | null>(null)
  const [results, setResults] = useState<StepResultRecord[]>([])

  useEffect(() => { api.listRuns().then(setRuns) }, [])

  const show = (runId: number) => {
    setSelected(runId)
    api.listStepResults(runId).then(setResults)
  }

  return (
    <div>
      <h2>실행 히스토리</h2>
      <table className="history">
        <thead>
          <tr><th>ID</th><th>시나리오</th><th>상태</th><th>시작</th><th>종료</th><th></th></tr>
        </thead>
        <tbody>
          {runs.map(r => (
            <tr key={r.id}>
              <td>{r.id}</td>
              <td>{r.scenario_name}</td>
              <td>{r.status}</td>
              <td>{r.started_at}</td>
              <td>{r.finished_at ?? '-'}</td>
              <td><button onClick={() => show(r.id)}>상세</button></td>
            </tr>
          ))}
        </tbody>
      </table>

      {selected != null && (
        <>
          <h3>실행 #{selected} 스텝 결과</h3>
          <table className="history">
            <thead>
              <tr><th>#</th><th>스텝</th><th>상태</th><th>소요(ms)</th><th>상세</th></tr>
            </thead>
            <tbody>
              {results.map(r => (
                <tr key={r.step_index}>
                  <td>{r.step_index + 1}</td>
                  <td>{r.name}</td>
                  <td>{r.status}</td>
                  <td>{r.duration_ms}</td>
                  <td><pre className="detail">{r.detail}</pre></td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </div>
  )
}
```

- [ ] **Step 2: 타입 체크 + 전체 검증**

```bash
npx tsc --noEmit
npm test
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri dev
```
Expected: 전부 PASS. 앱에서 시나리오 실행 후 히스토리 탭에 기록이 남고 상세가 보인다.

- [ ] **Step 3: 커밋**

```bash
git add -A && git commit -m "feat: 실행 히스토리 화면"
```

---

## 최종 수동 검증 (실제 환경)

OpenStack + RabbitMQ가 있는 실제 contrabass 환경에서:

1. 환경 프로필 등록 (Keystone URL, 계정/비밀번호, MQ URL, exchange 목록, nova/neutron/cinder 엔드포인트)
2. 프리셋으로 "인스턴스 생성 → 생성 완료 대기 → 인스턴스 삭제(cleanup)" 시나리오 작성
3. 실행 → 스텝별 실시간 상태 확인, `wait_event`가 notification으로 완료 감지하는지 확인
4. 중간 스텝을 일부러 실패시켜(잘못된 이미지 ID) cleanup 스텝이 그래도 실행되는지 확인
5. 실행 중 취소 → cleanup 실행 확인
6. 시나리오 JSON 내보내기/가져오기 왕복 확인

## 주의사항 (구현자용)

- **크레이트 버전**: `cargo add`가 최신 버전을 잡는다. `serde_json_path`의 API가 계획 코드와 다르면 (`JsonPath::parse` / `.query()` / `.exactly_one()`) 해당 크레이트 문서를 확인해 맞춘다.
- **oslo notification 포맷**: 환경에 따라 `oslo.message` 봉투 유무, `payload.instance_id` 경로가 다를 수 있다. 실제 환경의 메시지를 먼저 덤프해보고 프리셋의 json_path를 조정한다.
- **keyring**: macOS에서 첫 접근 시 키체인 권한 팝업이 뜰 수 있다.

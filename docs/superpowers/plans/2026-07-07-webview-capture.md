# 웹뷰 네트워크 캡처 → 시나리오 생성 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tauri 웹뷰로 contrabass 사이트를 띄우고 fetch/XHR 후킹으로 캡처한 API 호출 중 사용자가 선택한 것을 http_call 스텝으로 변환해 새 시나리오로 저장한다.

**Architecture:** Rust는 로컬 axum 수집 서버(127.0.0.1:랜덤포트)와 캡처용 웹뷰 창 관리만 담당한다. 주입 스크립트가 fetch/XHR를 후킹해 캡처를 로컬 서버로 단순 POST(text/plain 바디 + 쿼리 토큰, 응답 무시 → CORS 프리플라이트 없음)하면, 서버가 `capture-recorded` Tauri 이벤트로 메인 창에 전달한다. 프론트는 실시간 목록/선택/변환만 한다. 변환된 스텝은 v1의 `saveScenario`로 새 시나리오로 저장한다.

**Tech Stack:** Tauri 2, axum, tokio, React+TS, vitest. (신규 의존성: axum만)

**Spec:** `docs/superpowers/specs/2026-07-07-webview-capture-design.md`

**설계 대비 계획 결정 (스펙 §4 단순화):** 스펙은 "빌더에 초안 로드 또는 편집 중이면 append"를 열어뒀으나, 탭 간 인메모리 핸드오프는 App 상태를 들어올려야 해 범위가 커진다. MVP는 변환 결과를 `api.saveScenario`로 **새 시나리오로 저장**하고 사용자가 시나리오 탭에서 열어 편집하게 한다 (기존 영속성 재사용, 크로스탭 상태 불필요). "편집 중 append"는 백로그.

---

## 파일 구조

```
src-tauri/src/
├── capture_server.rs   # CapturedCall, parse_capture(순수), CaptureSink trait, axum start
├── capture_session.rs  # hook_script(순수 템플릿), open_capture_window
├── commands.rs         # (수정) AppState에 capture 필드, 커맨드 3개 추가
└── lib.rs              # (수정) 모듈 선언 + 핸들러 등록

src/
├── capture.ts          # CapturedCall 타입, capturesToSteps(순수)
├── capture.test.ts     # 변환 로직 vitest
├── api.ts              # (수정) 캡처 커맨드 래퍼 3개
├── App.tsx             # (수정) "캡처" 탭 추가
└── views/CaptureView.tsx  # 세션 시작 + 실시간 목록 + 선택/변환
```

Rust 신규 2파일, 프론트 신규 2파일. 나머지는 소규모 수정.

---

### Task 1: 프론트 변환 로직 (capture.ts, capture.test.ts)

가장 위험도 낮고 핵심인 순수 변환 로직부터. Tauri 없이 vitest로 완결.

**Files:**
- Create: `src/capture.ts`, `src/capture.test.ts`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/capture.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { capturesToSteps, type CapturedCall } from './capture'

const call = (over: Partial<CapturedCall> = {}): CapturedCall => ({
  id: '1',
  method: 'POST',
  url: 'https://contrabass.example.com/api/servers',
  request_headers: { 'X-Auth-Token': 'live-token-abc', 'Content-Type': 'application/json' },
  request_body: '{"name":"vm1"}',
  status: 202,
  response_body: '{"id":"srv-1"}',
  timestamp: 0,
  ...over,
})

describe('capturesToSteps', () => {
  it('http_call 스텝으로 변환하고 method/url/body를 그대로 둔다', () => {
    const [step] = capturesToSteps([call()], 'X-Auth-Token')
    expect(step.type).toBe('http_call')
    if (step.type !== 'http_call') throw new Error('타입 불일치')
    expect(step.method).toBe('POST')
    expect(step.url).toBe('https://contrabass.example.com/api/servers')
    expect(step.body).toBe('{"name":"vm1"}')
  })

  it('토큰 헤더만 {{auth_token}}으로 치환한다 (대소문자 무시)', () => {
    const [step] = capturesToSteps([call({ request_headers: { 'x-auth-token': 'live', 'Accept': 'application/json' } })], 'X-Auth-Token')
    if (step.type !== 'http_call') throw new Error('타입 불일치')
    expect(step.headers!['x-auth-token']).toBe('{{auth_token}}')
    expect(step.headers!['Accept']).toBe('application/json')
  })

  it('응답 상태코드를 expect_status로 설정한다', () => {
    const [step] = capturesToSteps([call({ status: 201 })], 'X-Auth-Token')
    if (step.type !== 'http_call') throw new Error('타입 불일치')
    expect(step.expect_status).toBe(201)
  })

  it('스텝 이름을 METHOD + 경로로 만든다 (쿼리 제외)', () => {
    const [step] = capturesToSteps([call({ url: 'https://h.example.com/api/servers?x=1' })], 'X-Auth-Token')
    expect(step.name).toBe('POST /api/servers')
  })

  it('토큰 헤더명이 없으면 헤더를 그대로 둔다', () => {
    const [step] = capturesToSteps([call()], 'Authorization')
    if (step.type !== 'http_call') throw new Error('타입 불일치')
    expect(step.headers!['X-Auth-Token']).toBe('live-token-abc')
  })

  it('여러 캡처를 순서대로 변환한다', () => {
    const steps = capturesToSteps([call({ id: 'a', method: 'GET' }), call({ id: 'b', method: 'DELETE' })], 'X-Auth-Token')
    expect(steps.map(s => (s.type === 'http_call' ? s.method : ''))).toEqual(['GET', 'DELETE'])
  })
})
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `npm test`
Expected: FAIL — `Cannot find module './capture'`

- [ ] **Step 3: capture.ts 구현**

`src/capture.ts`:

```ts
import type { StepDef } from './types'

export interface CapturedCall {
  id: string
  method: string
  url: string
  request_headers: Record<string, string>
  request_body: string | null
  status: number
  response_body: string | null
  timestamp: number
}

function pathOf(url: string): string {
  try {
    return new URL(url).pathname
  } catch {
    return url
  }
}

/** 선택된 캡처들을 http_call 스텝으로 변환한다.
 *  tokenHeaderName과 (대소문자 무시) 일치하는 헤더 값만 {{auth_token}}으로 치환하고
 *  method/url/body는 그대로 둔다. */
export function capturesToSteps(calls: CapturedCall[], tokenHeaderName: string): StepDef[] {
  const tokenLower = tokenHeaderName.toLowerCase()
  return calls.map(c => {
    const headers: Record<string, string> = {}
    for (const [k, v] of Object.entries(c.request_headers)) {
      headers[k] = k.toLowerCase() === tokenLower ? '{{auth_token}}' : v
    }
    return {
      name: `${c.method} ${pathOf(c.url)}`,
      type: 'http_call',
      method: c.method,
      url: c.url,
      headers,
      body: c.request_body,
      expect_status: c.status,
      captures: [],
    }
  })
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `npm test`
Expected: capture 6개 + 기존 presets 4개 PASS

- [ ] **Step 5: 커밋**

```bash
git add src/capture.ts src/capture.test.ts
git commit -m "feat: 캡처 → http_call 변환 순수 함수"
```

---

### Task 2: axum 수집 서버 (capture_server.rs)

**Files:**
- Create: `src-tauri/src/capture_server.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod capture_server;` 추가)
- Modify: `src-tauri/Cargo.toml` (axum 추가)

- [ ] **Step 1: axum 의존성 추가**

```bash
cd src-tauri && cargo add axum && cd ..
```

- [ ] **Step 2: 구현 + 순수 함수 테스트 작성**

`src-tauri/src/capture_server.rs`:

```rust
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
```

`lib.rs` 최상단 모듈 목록에 `pub mod capture_server;` 추가.

- [ ] **Step 3: 테스트 실행**

Run: `cargo test --manifest-path src-tauri/Cargo.toml capture_server`
Expected: 5개 PASS

- [ ] **Step 4: 커밋**

```bash
git add src-tauri/src/capture_server.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat: axum 캡처 수집 서버 (토큰 인증)"
```

---

### Task 3: 후킹 스크립트 + 캡처 창 (capture_session.rs)

**Files:**
- Create: `src-tauri/src/capture_session.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod capture_session;` 추가)

- [ ] **Step 1: 구현 + 스크립트 템플릿 테스트 작성**

`src-tauri/src/capture_session.rs`:

```rust
use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};

/// 캡처 웹뷰에 주입할 fetch/XHR 후킹 스크립트를 만든다.
/// 포트와 토큰이 박힌 스크립트가 캡처를 http://127.0.0.1:{port}/capture?token={token} 로 전송한다.
/// 전송은 단순 요청(text/plain 바디, 쿼리 토큰, 응답 무시)이라 CORS 프리플라이트가 없다.
pub fn hook_script(port: u16, token: &str) -> String {
    // response_body 상한 8KB
    format!(
        r#"(function() {{
  var ENDPOINT = "http://127.0.0.1:{port}/capture?token={token}";
  var origFetch = window.fetch;
  var seq = 0;
  // 캡처 POST는 원본 fetch로 보낸다 — 몽키패치된 fetch로 보내면 자기 자신이 다시 캡처돼 무한 재귀가 된다.
  function send(call) {{
    try {{ origFetch.call(window, ENDPOINT, {{ method: "POST", body: JSON.stringify(call), keepalive: true }}).catch(function(){{}}); }} catch (e) {{}}
  }}
  function truncate(s) {{ return (typeof s === "string" && s.length > 8192) ? s.slice(0, 8192) : s; }}
  function headersToObj(h) {{
    var o = {{}};
    if (h && typeof h.forEach === "function") h.forEach(function(v, k) {{ o[k] = v; }});
    return o;
  }}

  window.fetch = function(input, init) {{
    var req;
    try {{ req = new Request(input, init); }} catch (e) {{ return origFetch.apply(this, arguments); }}
    var reqHeaders = headersToObj(req.headers);
    var id = "c" + (++seq);
    // Request 객체에 실린 body도 잡히도록 req.clone()에서 읽는다 (init.body만 보면 놓침). GET은 ""→null.
    var bodyPromise = req.clone().text().then(function(t) {{ return t && t.length ? t : null; }}).catch(function() {{ return null; }});
    // 원본 arguments 대신 정규화된 req를 넘겨 Request-first 스타일의 body 이중소비를 피한다.
    return origFetch.call(this, req).then(function(resp) {{
      try {{
        var clone = resp.clone();
        Promise.all([bodyPromise, clone.text().catch(function(){{ return null; }})]).then(function(arr) {{
          send({{ id: id, method: req.method, url: req.url, request_headers: reqHeaders,
                  request_body: arr[0], status: resp.status, response_body: truncate(arr[1]), timestamp: Date.now() }});
        }});
      }} catch (e) {{}}
      return resp;
    }});
  }};

  var XO = XMLHttpRequest.prototype.open;
  var XS = XMLHttpRequest.prototype.send;
  var XH = XMLHttpRequest.prototype.setRequestHeader;
  XMLHttpRequest.prototype.open = function(method, url) {{
    this.__cap = {{ method: method, url: url, headers: {{}} }};
    return XO.apply(this, arguments);
  }};
  XMLHttpRequest.prototype.setRequestHeader = function(k, v) {{
    if (this.__cap) this.__cap.headers[k] = v;
    return XH.apply(this, arguments);
  }};
  XMLHttpRequest.prototype.send = function(body) {{
    var self = this;
    if (self.__cap) {{
      self.addEventListener("loadend", function() {{
        try {{
          var abs;
          try {{ abs = new URL(self.__cap.url, location.href).href; }} catch (e) {{ abs = self.__cap.url; }}
          // responseType이 text/''가 아니면 responseText 접근 자체가 예외를 던지므로 먼저 걸러낸다.
          var rt = (self.responseType === "" || self.responseType === "text") ? self.responseText : null;
          send({{ id: "c" + (++seq), method: self.__cap.method, url: abs, request_headers: self.__cap.headers,
                  request_body: body != null ? String(body) : null, status: self.status,
                  response_body: truncate(rt), timestamp: Date.now() }});
        }} catch (e) {{}}
      }});
    }}
    return XS.apply(this, arguments);
  }};
}})();"#
    )
}

/// 대상 URL을 캡처 웹뷰 창("capture")으로 열고 후킹 스크립트를 주입한다.
pub fn open_capture_window(app: &AppHandle, url: &str, script: String) -> Result<tauri::WebviewWindow, String> {
    let parsed: tauri::Url = url.parse().map_err(|_| format!("잘못된 URL: {url}"))?;
    let window = WebviewWindowBuilder::new(app, "capture", WebviewUrl::External(parsed))
        .title("캡처 세션")
        .initialization_script(&script)
        .build()
        .map_err(|e| format!("캡처 창 생성 실패: {e}"))?;
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_embeds_port_and_token() {
        let s = hook_script(54321, "secret-tok");
        assert!(s.contains("54321"));
        assert!(s.contains("secret-tok"));
    }

    #[test]
    fn script_hooks_fetch_and_xhr() {
        let s = hook_script(1, "t");
        assert!(s.contains("window.fetch"));
        assert!(s.contains("XMLHttpRequest.prototype.open"));
        assert!(s.contains("XMLHttpRequest.prototype.send"));
    }

    #[test]
    fn script_truncates_at_8kb() {
        let s = hook_script(1, "t");
        assert!(s.contains("8192"));
    }
}
```

`lib.rs`에 `pub mod capture_session;` 추가.

- [ ] **Step 2: 테스트 실행**

Run: `cargo test --manifest-path src-tauri/Cargo.toml capture_session`
Expected: 3개 PASS

- [ ] **Step 3: 커밋**

```bash
git add src-tauri/src/capture_session.rs src-tauri/src/lib.rs
git commit -m "feat: fetch/XHR 후킹 스크립트 및 캡처 창"
```

---

### Task 4: 캡처 커맨드 배선 (commands.rs, lib.rs)

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: AppState에 캡처 세션 상태 추가**

`src-tauri/src/commands.rs`의 import에 추가:

```rust
use crate::capture_server::{self, EventSink};
use crate::capture_session;
```

`AppState` 구조체를 아래로 교체 (capture 필드 추가):

```rust
pub struct AppState {
    pub db: Mutex<Store>,
    pub active_runs: Mutex<HashMap<i64, CancellationToken>>,
    pub capture: Mutex<Option<CaptureHandle>>,
}

pub struct CaptureHandle {
    pub id: String,
    pub cancel: CancellationToken,
}
```

- [ ] **Step 2: 캡처 커맨드 3개 추가**

`commands.rs` 맨 끝에 추가:

```rust
// --- 캡처 세션 ---

/// 세션 시작을 위한 비암호학적 토큰 생성 (nanos 기반이라 예측·재현 가능).
/// 위협모델은 localhost 한정 우발적 끼어들기 방지 수준이며, 로컬 악성 프로세스 방어가
/// 필요해지면 CSPRNG로 교체할 것.
fn generate_capture_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("cap-{nanos:x}")
}

#[tauri::command]
pub async fn start_capture_session(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<u16, String> {
    let token = generate_capture_token();
    let cancel = CancellationToken::new();
    // 체크 + 슬롯 선점을 원자적으로 — await 구간 동안 동시 start를 막는다 (label 유일성에 비의존)
    {
        let mut guard = state.capture.lock().unwrap();
        if guard.is_some() {
            return Err("이미 캡처 세션이 진행 중입니다".into());
        }
        *guard = Some(CaptureHandle { id: token.clone(), cancel: cancel.clone() });
    }
    // 이전 세션이 남긴 창이 있으면 정리 (label 재사용)
    if let Some(win) = app.get_webview_window("capture") {
        let _ = win.close();
    }

    let sink = Box::new(EventSink { app: app.clone() });
    let port = match capture_server::start(token.clone(), sink, cancel.clone()).await {
        Ok(p) => p,
        Err(e) => {
            // 우리 예약이 아직 남아있으면 되돌린다 (좀비 방지)
            let mut guard = state.capture.lock().unwrap();
            if guard.as_ref().map(|h| h.id == token).unwrap_or(false) {
                guard.take();
            }
            cancel.cancel();
            return Err(e);
        }
    };

    let script = capture_session::hook_script(port, &token);
    let window = match capture_session::open_capture_window(&app, &url, script) {
        Ok(w) => w,
        Err(e) => {
            let mut guard = state.capture.lock().unwrap();
            if guard.as_ref().map(|h| h.id == token).unwrap_or(false) {
                guard.take();
            }
            cancel.cancel();
            return Err(e);
        }
    };

    // 사용자가 캡처 창을 직접 닫으면 세션 정리 + 알림 (슬롯의 세션이 자기 세션일 때만)
    let app_close = app.clone();
    let my_id = token.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let st = app_close.state::<AppState>();
            let mut guard = st.capture.lock().unwrap();
            if guard.as_ref().map(|h| h.id == my_id).unwrap_or(false) {
                if let Some(h) = guard.take() {
                    h.cancel.cancel();
                }
                drop(guard);
                let _ = app_close.emit("capture-session-ended", ());
            }
        }
    });
    Ok(port)
}

#[tauri::command]
pub fn stop_capture_session(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    // 락을 먼저 놓고 창을 닫아 Destroyed 핸들러와의 재진입 데드락을 피한다
    let handle = state.capture.lock().unwrap().take();
    if let Some(h) = handle {
        h.cancel.cancel();
    }
    if let Some(window) = app.get_webview_window("capture") {
        let _ = window.close();
    }
    let _ = app.emit("capture-session-ended", ());
    Ok(())
}

#[tauri::command]
pub fn capture_session_active(state: State<AppState>) -> Result<bool, String> {
    Ok(state.capture.lock().unwrap().is_some())
}
```

- [ ] **Step 3: get_webview_window import 확인**

`commands.rs`의 `use tauri::{...}` 라인에 `Manager`가 이미 있으므로 `app.get_webview_window`/`app.state` 사용 가능 (기존 run_scenario에서 이미 `app2.state` 사용 중). `WindowEvent`는 `tauri::WindowEvent`로 경로 지정했으므로 추가 import 불필요.

- [ ] **Step 4: lib.rs setup의 AppState 초기화 + 핸들러 등록**

`lib.rs`의 `app.manage(AppState { ... })`를 아래로 교체:

```rust
            app.manage(AppState {
                db: Mutex::new(db),
                active_runs: Mutex::new(Default::default()),
                capture: Mutex::new(None),
            });
            // 메인 창을 닫으면 활성 캡처 세션(서버·창)을 함께 정리 — 유령 세션 방지
            if let Some(main) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                main.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Destroyed) {
                        let st = app_handle.state::<AppState>();
                        // 락을 먼저 놓고 창을 닫아 캡처 창 Destroyed 핸들러와 재진입 데드락 회피
                        let handle = st.capture.lock().unwrap().take();
                        if let Some(h) = handle {
                            h.cancel.cancel();
                        }
                        if let Some(w) = app_handle.get_webview_window("capture") {
                            let _ = w.close();
                        }
                    }
                });
            }
```

`generate_handler!` 목록의 `commands::cancel_run` 뒤에 콤마와 3개 추가:

```rust
            commands::cancel_run,
            commands::start_capture_session,
            commands::stop_capture_session,
            commands::capture_session_active
```

- [ ] **Step 5: 전체 빌드/테스트 확인**

Run:
```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```
Expected: 58개 PASS (기존 + capture 서버/스크립트/토큰 테스트), clippy 경고 0

- [ ] **Step 6: 커밋**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: 캡처 세션 커맨드 배선 (시작/종료/상태)"
```

---

### Task 5: 캡처 화면 + 탭 (CaptureView.tsx, api.ts, App.tsx)

**Files:**
- Modify: `src/api.ts`
- Create: `src/views/CaptureView.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: api.ts에 캡처 래퍼 추가**

`src/api.ts` 맨 끝에 추가:

```ts
export const startCaptureSession = (url: string) => invoke<number>('start_capture_session', { url })
export const stopCaptureSession = () => invoke<void>('stop_capture_session')
export const captureSessionActive = () => invoke<boolean>('capture_session_active')
```

- [ ] **Step 2: CaptureView.tsx 작성**

`src/views/CaptureView.tsx`:

```tsx
import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import * as api from '../api'
import { capturesToSteps, type CapturedCall } from '../capture'
import type { ScenarioRecord } from '../types'

export default function CaptureView() {
  const [url, setUrl] = useState('')
  const [tokenHeader, setTokenHeader] = useState('X-Auth-Token')
  const [active, setActive] = useState(false)
  const [calls, setCalls] = useState<CapturedCall[]>([])
  const [selected, setSelected] = useState<Record<string, boolean>>({})
  const [scenarioName, setScenarioName] = useState('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const startedAt = useRef(0)

  useEffect(() => {
    api.captureSessionActive().then(setActive).catch(() => {})
    const unRec = listen<CapturedCall>('capture-recorded', e => {
      setCalls(prev => [e.payload, ...prev])
    })
    const unEnd = listen('capture-session-ended', () => {
      setActive(false)
      setNotice('캡처 세션이 종료되었습니다. 목록은 유지되며 변환할 수 있습니다.')
    })
    return () => { unRec.then(u => u()); unEnd.then(u => u()) }
  }, [])

  const start = async () => {
    setError(''); setNotice('')
    if (!url) { setError('대상 URL을 입력하세요'); return }
    try {
      await api.startCaptureSession(url)
      setActive(true)
      startedAt.current = Date.now()
      setCalls([]); setSelected({})
    } catch (e) { setError(String(e)) }
  }

  const stop = async () => {
    await api.stopCaptureSession().catch(e => setError(String(e)))
    setActive(false)
    if (calls.length === 0 && Date.now() - startedAt.current > 3000) {
      setNotice('캡처가 0건입니다. 대상 사이트의 CSP로 후킹이 차단됐을 수 있습니다.')
    }
  }

  const toggle = (id: string) => setSelected(s => ({ ...s, [id]: !s[id] }))

  const addToScenario = async () => {
    setError(''); setNotice('')
    const chosen = calls.filter(c => selected[c.id])
    if (chosen.length === 0) { setError('추가할 호출을 선택하세요'); return }
    const steps = capturesToSteps(chosen, tokenHeader)
    const rec: ScenarioRecord = {
      id: null,
      name: scenarioName || `캡처 시나리오 ${new Date().toISOString().slice(0, 19)}`,
      description: `${url} 캡처에서 생성`,
      steps_json: JSON.stringify(steps),
    }
    try {
      await api.saveScenario(rec)
      setNotice(`시나리오 "${rec.name}" 생성됨. 시나리오 탭에서 열어 편집하세요.`)
      setSelected({})
    } catch (e) { setError(String(e)) }
  }

  return (
    <div>
      <h2>네트워크 캡처</h2>
      <div className="add-row">
        <input placeholder="대상 사이트 URL (https://...)" value={url}
          onChange={e => setUrl(e.target.value)} disabled={active} style={{ minWidth: 320 }} />
        <input placeholder="토큰 헤더명" value={tokenHeader}
          onChange={e => setTokenHeader(e.target.value)} />
        {!active
          ? <button onClick={start}>세션 시작</button>
          : <button className="danger" onClick={stop}>세션 종료</button>}
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="dim">{notice}</p>}

      <div className="add-row">
        <input placeholder="새 시나리오 이름 (비우면 자동)" value={scenarioName}
          onChange={e => setScenarioName(e.target.value)} style={{ minWidth: 240 }} />
        <button onClick={addToScenario}>선택 항목을 시나리오로 저장</button>
        <span className="dim">캡처 {calls.length}건 · 선택 {Object.values(selected).filter(Boolean).length}건</span>
      </div>

      <table className="history">
        <thead>
          <tr><th></th><th>메서드</th><th>URL</th><th>상태</th></tr>
        </thead>
        <tbody>
          {calls.map(c => (
            <tr key={c.id}>
              <td><input type="checkbox" checked={!!selected[c.id]} onChange={() => toggle(c.id)} /></td>
              <td>{c.method}</td>
              <td>{c.url}</td>
              <td>{c.status}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
```

- [ ] **Step 3: App.tsx에 캡처 탭 추가**

`src/App.tsx`를 아래로 교체:

```tsx
import { useState } from 'react'
import './App.css'
import CaptureView from './views/CaptureView'
import EnvironmentsView from './views/EnvironmentsView'
import HistoryView from './views/HistoryView'
import RunView from './views/RunView'
import ScenarioBuilder from './views/ScenarioBuilder'

const tabs = [
  { key: 'run', label: '실행' },
  { key: 'scenarios', label: '시나리오' },
  { key: 'capture', label: '캡처' },
  { key: 'envs', label: '환경' },
  { key: 'history', label: '히스토리' },
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
      <div style={{ display: tab === 'run' ? undefined : 'none' }}>
        <RunView active={tab === 'run'} />
      </div>
      {tab === 'scenarios' && <ScenarioBuilder />}
      {tab === 'capture' && <CaptureView />}
      {tab === 'envs' && <EnvironmentsView />}
      {tab === 'history' && <HistoryView />}
    </main>
  )
}
```

- [ ] **Step 4: 타입 체크 + 테스트 + 앱 기동 확인**

Run:
```bash
npx tsc --noEmit
npm test
npm run tauri dev
```
Expected: tsc 통과, vitest 10개 PASS. 앱 기동 후 "캡처" 탭에서 URL 입력 → 세션 시작 시 별도 창이 뜨는지 확인 (실제 캡처 검증은 아래 수동 절차). 확인 후 프로세스 정리.

- [ ] **Step 5: 커밋**

```bash
git add src/api.ts src/views/CaptureView.tsx src/App.tsx
git commit -m "feat: 캡처 화면 및 탭"
```

---

## 최종 수동 검증 (실제 사이트)

로컬 mock 페이지 또는 실제 contrabass 사이트로:

1. 캡처 탭 → 대상 URL 입력 → 세션 시작 → 별도 창에서 사이트 조작
2. fetch/XHR 호출이 실시간 목록에 뜨는지 확인
3. 원하는 호출 체크 → "선택 항목을 시나리오로 저장" → 시나리오 탭에서 생성 확인
4. 생성된 http_call 스텝의 토큰 헤더가 `{{auth_token}}`으로 치환됐는지, 나머지는 리터럴인지 확인
5. 캡처 창을 직접 닫아 "세션 종료됨" 표시 + 목록 유지 확인
6. CSP 엄격한 사이트에서 캡처 0건 시 안내 문구 확인

## 주의사항 (구현자용)

- **axum 버전**: 최신(0.8x) 기준. `axum::serve`, `Router::route`, `State`/`Query` 추출자 API가 다르면 해당 버전 docs로 맞추되 시그니처(`start`, `parse_capture`)는 유지.
- **WebviewWindowBuilder / WebviewUrl::External / initialization_script / on_window_event / WindowEvent::Destroyed**: Tauri 2 API. 이름이 다르면 확인해 맞추되 동작(원격 URL 창 + 스크립트 주입 + 닫힘 감지)은 유지.
- **토큰은 비암호학적**: localhost 한정 우발적 끼어들기 방지 수준. 강화는 백로그.
- **CORS 없음 의도**: 주입 스크립트는 응답을 읽지 않으므로(fire-and-forget) 단순 요청이 서버에 도달만 하면 되고 서버에 CORS 헤더가 없어도 캡처는 수신된다.

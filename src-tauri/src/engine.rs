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
    // cleanup 스텝은 "항상 실행"되어야 하므로, 이미 취소된 뒤라도 액션 내부의 select에서
    // 중간에 잘리지 않도록 취소되지 않는 토큰을 넘긴다. (실행 여부 자체는 위 skip 분기에서 이미 결정됨)
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
            Ok(detail) => StepOutcome { index: i, name: step.name.clone(), status: StepStatus::Passed, detail: truncate(&detail, 2000), duration_ms },
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
                // 401을 기대하는 음성 테스트면 재발급하지 않는다
                if res.status == 401 && attempt == 0 && *expect_status != Some(401) {
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    /// 호출 횟수를 세면서 고정 결과를 돌려주는 TokenRefresher.
    fn counting_refresher(
        counter: Arc<AtomicUsize>,
        result: Result<String, String>,
    ) -> TokenRefresher {
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let r = result.clone();
            Box::pin(async move { r })
        })
    }

    fn http_step(url: String, expect_status: Option<u16>) -> StepDef {
        step(
            "call",
            false,
            Action::HttpCall {
                method: "GET".into(),
                url,
                headers: HashMap::from([(
                    "X-Auth-Token".to_string(),
                    "{{auth_token}}".to_string(),
                )]),
                body: None,
                expect_status,
                captures: vec![],
            },
        )
    }

    #[tokio::test]
    async fn retries_401_with_refreshed_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .and(header("X-Auth-Token", "old"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .and(header("X-Auth-Token", "new"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;

        let calls = Arc::new(AtomicUsize::new(0));
        let mut inp = input(vec![http_step(format!("{}/servers", server.uri()), Some(200))]);
        inp.vars.insert("auth_token".into(), "old".into());
        inp.token_refresher = Some(counting_refresher(calls.clone(), Ok("new".into())));

        let outcomes = run(inp, &NullSink).await;
        assert_eq!(outcomes[0].status, StepStatus::Passed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn refresh_failure_fails_step() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let calls = Arc::new(AtomicUsize::new(0));
        let mut inp = input(vec![http_step(format!("{}/servers", server.uri()), Some(200))]);
        inp.vars.insert("auth_token".into(), "old".into());
        inp.token_refresher = Some(counting_refresher(calls.clone(), Err("재발급 실패".into())));

        let outcomes = run(inp, &NullSink).await;
        assert_eq!(outcomes[0].status, StepStatus::Failed);
        assert!(outcomes[0].detail.contains("재발급 실패"));
        assert_eq!(calls.load(Ordering::SeqCst), 1); // 무한루프 없음
    }

    #[tokio::test]
    async fn expected_401_skips_refresh() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let calls = Arc::new(AtomicUsize::new(0));
        let mut inp = input(vec![http_step(format!("{}/servers", server.uri()), Some(401))]);
        inp.vars.insert("auth_token".into(), "old".into());
        inp.token_refresher = Some(counting_refresher(calls.clone(), Ok("new".into())));

        let outcomes = run(inp, &NullSink).await;
        assert_eq!(outcomes[0].status, StepStatus::Passed);
        assert_eq!(calls.load(Ordering::SeqCst), 0); // 의도된 401이면 재발급하지 않는다
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

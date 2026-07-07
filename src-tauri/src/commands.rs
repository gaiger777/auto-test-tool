use crate::capture_server::{self, EventSink};
use crate::capture_session;
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
    pub capture: Mutex<Option<CaptureHandle>>,
}

pub struct CaptureHandle {
    pub id: String,
    pub cancel: CancellationToken,
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
    // OS 키체인은 사용자 승인 모달로 무기한 블록될 수 있어 블로킹 워커에서 격리
    let password = tauri::async_runtime::spawn_blocking(move || store::get_password(env_id))
        .await
        .map_err(|e| format!("키체인 조회 태스크 실패: {e}"))??;

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
            if let Err(e) = db.finish_run(run_id, status, &now()) {
                eprintln!("[commands] run {run_id} 종료 기록 실패: {e}");
            }
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

// --- 캡처 세션 ---

/// 세션 시작을 위한 비암호학적 토큰 생성 (localhost 한정, 우발적 끼어들기 방지 수준).
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
    if state.capture.lock().unwrap().is_some() {
        return Err("이미 캡처 세션이 진행 중입니다".into());
    }
    // 이전 세션이 남긴 창이 있으면 정리 (label 재사용)
    if let Some(win) = app.get_webview_window("capture") {
        let _ = win.close();
    }
    let token = generate_capture_token();
    let cancel = CancellationToken::new();
    let sink = Box::new(EventSink { app: app.clone() });
    let port = capture_server::start(token.clone(), sink, cancel.clone()).await?;

    let script = capture_session::hook_script(port, &token);
    if let Err(e) = capture_session::open_capture_window(&app, &url, script) {
        cancel.cancel(); // 창 생성 실패 시 서버 정리 (좀비 방지)
        return Err(e);
    }

    *state.capture.lock().unwrap() = Some(CaptureHandle { id: token.clone(), cancel });

    // 사용자가 캡처 창을 직접 닫으면 세션 정리 + 알림
    if let Some(window) = app.get_webview_window("capture") {
        let app_close = app.clone();
        let my_id = token.clone();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let st = app_close.state::<AppState>();
                let mut guard = st.capture.lock().unwrap();
                // 슬롯에 있는 세션이 내 세션일 때만 정리한다.
                // (빠른 재시작으로 다른 세션이 이미 슬롯을 차지했으면 뒤늦은 이 이벤트는 무시)
                if guard.as_ref().map(|h| h.id == my_id).unwrap_or(false) {
                    if let Some(h) = guard.take() {
                        h.cancel.cancel();
                    }
                    drop(guard);
                    let _ = app_close.emit("capture-session-ended", ());
                }
            }
        });
    }
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

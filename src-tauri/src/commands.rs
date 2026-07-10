use crate::capture_server;
use crate::capture_session;
use crate::engine::{self, ProgressSink, RunInput, StepOutcome, StepStatus, TokenRefresher};
use crate::events::EventBus;
use crate::keystone::{KeystoneAuth, KeystoneClient};
use crate::models::{Scenario, StepDef, Vars};
use crate::mq;
use crate::store::{
    self, Environment, RunRecord, ScenarioRecord, StepResultRecord, Store, UiFlowRecord, UiFlowSite,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

pub struct AppState {
    pub db: Mutex<Store>,
    pub active_runs: Mutex<HashMap<i64, CancellationToken>>,
    pub capture: Mutex<Option<CaptureHandle>>,
    /// 현재 UI 재생 세션 토큰 (캡처와 독립 — 스위트 연속 실행 시 교체 가능).
    pub replay: Mutex<Option<String>>,
}

pub struct CaptureHandle {
    pub id: String,
    pub cancel: CancellationToken,
    /// 세션 단조 증가 id 카운터. 페이지 재탐색으로 스크립트 seq가 리셋돼도 id 충돌이 없도록 한다.
    pub seq: Arc<std::sync::atomic::AtomicU64>,
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
    env_id: Option<i64>,
    vars: Option<HashMap<String, String>>,
) -> Result<i64, String> {
    let scenario_rec = state.db.lock().unwrap().get_scenario(scenario_id)?;
    let steps: Vec<StepDef> =
        serde_json::from_str(&scenario_rec.steps_json).map_err(|e| e.to_string())?;
    let scenario = Scenario {
        name: scenario_rec.name,
        description: scenario_rec.description,
        steps,
    };

    // 사용자 제공 변수(예: auth_token) — 환경 없이 실행(Postman식) 시 사용
    let mut run_vars = Vars::new();
    if let Some(v) = vars {
        for (k, val) in v {
            run_vars.insert(k, val);
        }
    }

    let bus = EventBus::new();
    let cancel = CancellationToken::new();
    let mut refresher: Option<TokenRefresher> = None;

    if let Some(eid) = env_id {
        // 환경 모드: Keystone 인증 + MQ 소비 + base_url 변수
        let env = state.db.lock().unwrap().get_environment(eid)?;
        // OS 키체인은 사용자 승인 모달로 무기한 블록될 수 있어 블로킹 워커에서 격리
        let password = tauri::async_runtime::spawn_blocking(move || store::get_password(eid))
            .await
            .map_err(|e| format!("키체인 조회 태스크 실패: {e}"))??;
        let ks = Arc::new(KeystoneClient::new(
            reqwest::Client::builder()
                .danger_accept_invalid_certs(true) // 내부 서버 사설 CA 허용
                .build()
                .map_err(|e| format!("HTTP 클라이언트 생성 실패: {e}"))?,
            KeystoneAuth {
                auth_url: env.keystone_url.clone(),
                user_name: env.user_name.clone(),
                user_domain: env.user_domain.clone(),
                password,
                project_name: env.project_name.clone(),
                project_domain: env.project_domain.clone(),
            },
        ));
        // Keystone 토큰 (실패 시 실행 자체를 시작하지 않음). 환경 토큰이 사용자 제공 토큰을 덮는다.
        let token = ks.get_token().await?;
        run_vars.insert("auth_token".into(), token);
        // MQ 소비자 시작 (설계: 접속 실패 시 실행 시작 실패)
        let exchanges: Vec<String> = env
            .mq_exchanges
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        mq::start_consumer(&env.mq_url, &exchanges, bus.clone(), cancel.clone()).await?;
        // base_url 변수
        for (svc, url) in &env.endpoints {
            run_vars.insert(format!("base_url.{svc}"), url.trim_end_matches('/').to_string());
        }
        let ks2 = ks.clone();
        refresher = Some(Arc::new(move || {
            let ks = ks2.clone();
            Box::pin(async move { ks.refresh_token().await })
        }));
    }
    // else: 환경 없이 단순 실행(Postman식) — Keystone/MQ/base_url 생략, 사용자 제공 vars만 사용

    // run 레코드 생성 (환경 없으면 env_id=0 센티넬) + 백그라운드 실행
    let run_id = state
        .db
        .lock()
        .unwrap()
        .create_run(scenario_id, env_id.unwrap_or(0), &now())?;
    state.active_runs.lock().unwrap().insert(run_id, cancel.clone());

    let sink = TauriSink { app: app.clone(), run_id };
    let input = RunInput {
        scenario,
        vars: run_vars,
        bus,
        cancel: cancel.clone(),
        token_refresher: refresher,
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
pub fn start_capture_session(
    app: AppHandle,
    state: State<AppState>,
    url: String,
) -> Result<(), String> {
    let token = generate_capture_token();
    let cancel = CancellationToken::new();
    let seq = Arc::new(std::sync::atomic::AtomicU64::new(0));
    // 체크 + 슬롯 선점을 원자적으로 — 동시 start를 막는다 (label 유일성에 의존하지 않음)
    {
        let mut guard = state.capture.lock().unwrap();
        if guard.is_some() {
            return Err("이미 캡처 세션이 진행 중입니다".into());
        }
        *guard = Some(CaptureHandle { id: token.clone(), cancel: cancel.clone(), seq });
    }
    // 이전 세션이 남긴 창이 있으면 정리 (label 재사용)
    if let Some(win) = app.get_webview_window("capture") {
        let _ = win.close();
    }

    // 네트워크 후킹 + UI 레코더를 함께 주입한다 (한 세션에서 네트워크·UI를 같이 기록).
    let script = format!(
        "{}\n{}",
        capture_session::hook_script(&token),
        capture_session::recorder_script(&token)
    );
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

    // 사용자가 캡처 창을 직접 닫으면 세션 정리 + 알림 (자기 세션일 때만)
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
    Ok(())
}

/// 캡처 웹뷰의 후킹 스크립트가 IPC로 밀어넣는 캡처를 수집한다.
/// 원격 origin에서 호출되므로 capabilities/capture.json 의 remote 스코프로 허용된다.
#[tauri::command]
pub fn capture_push(
    app: AppHandle,
    state: State<AppState>,
    token: String,
    mut call: capture_server::CapturedCall,
) -> Result<(), String> {
    // 세션 신원 검증: 현재 활성 세션의 토큰과 일치할 때만 수집 (재시작·스테일 창의 오수집 방지)
    let seq = {
        let guard = state.capture.lock().unwrap();
        match guard.as_ref() {
            Some(h) if h.id == token => h.seq.clone(),
            _ => return Err("활성 캡처 세션이 아니거나 토큰 불일치".into()),
        }
    };
    // 페이지 재탐색으로 스크립트 seq가 리셋돼도 충돌하지 않도록 세션 단조 id로 덮어쓴다
    let n = seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    call.id = format!("s{n}");
    let _ = app.emit("capture-recorded", call);
    Ok(())
}

/// 캡처 웹뷰의 레코더 스크립트가 IPC로 밀어넣는 UI 조작을 수집한다.
#[tauri::command]
pub fn ui_record(
    app: AppHandle,
    state: State<AppState>,
    token: String,
    mut action: capture_server::UiAction,
) -> Result<(), String> {
    let seq = {
        let guard = state.capture.lock().unwrap();
        match guard.as_ref() {
            Some(h) if h.id == token => h.seq.clone(),
            _ => return Err("활성 캡처 세션이 아니거나 토큰 불일치".into()),
        }
    };
    let n = seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    action.id = format!("u{n}");
    let _ = app.emit("ui-recorded", action);
    Ok(())
}

/// 기록된 UI 동작을 재생 웹뷰("replay")에서 실행한다. (셀렉터 자가치유 + actionability 대기)
#[tauri::command]
pub fn start_ui_replay(
    app: AppHandle,
    state: State<AppState>,
    url: String,
    actions: Vec<capture_server::UiAction>,
) -> Result<(), String> {
    if actions.is_empty() {
        return Err("재생할 UI 동작이 없습니다".into());
    }
    // 재생 세션 토큰 설정(이전 재생을 교체 — 스위트 연속/개별 실행 지원). 캡처와 독립.
    let token = generate_capture_token();
    *state.replay.lock().unwrap() = Some(token.clone());
    if let Some(win) = app.get_webview_window("replay") {
        let _ = win.close();
    }
    let json = serde_json::to_string(&actions).map_err(|e| e.to_string())?;
    let script = capture_session::player_script(&token, &json);
    let parsed: tauri::Url = url.parse().map_err(|_| format!("잘못된 URL: {url}"))?;
    tauri::WebviewWindowBuilder::new(&app, "replay", tauri::WebviewUrl::External(parsed))
        .title("UI 재생")
        .initialization_script(&script)
        .build()
        .map_err(|e| {
            *state.replay.lock().unwrap() = None;
            format!("재생 창 생성 실패: {e}")
        })?;
    Ok(())
}

/// 재생 웹뷰의 플레이어가 IPC로 보고하는 스텝 결과를 수집한다.
#[tauri::command]
pub fn ui_replay_step(
    app: AppHandle,
    state: State<AppState>,
    token: String,
    result: capture_server::UiStepResult,
) -> Result<(), String> {
    if state.replay.lock().unwrap().as_deref() != Some(token.as_str()) {
        return Err("활성 재생 세션이 아닙니다".into());
    }
    let _ = app.emit("ui-replay-step", result);
    Ok(())
}

/// 진행 중인 UI 재생을 취소한다(재생 창을 닫고 세션 해제).
#[tauri::command]
pub fn stop_ui_replay(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    *state.replay.lock().unwrap() = None;
    if let Some(win) = app.get_webview_window("replay") {
        let _ = win.close();
    }
    Ok(())
}

/// 기록한 UI 동작 목록을 JSON 파일로 저장한다.
#[tauri::command]
pub fn save_ui_actions(path: String, actions: Vec<capture_server::UiAction>) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&actions).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("파일 쓰기 실패: {e}"))
}

/// JSON 파일에서 UI 동작 목록을 불러온다.
#[tauri::command]
pub fn load_ui_actions(path: String) -> Result<Vec<capture_server::UiAction>, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("파일 읽기 실패: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("UI 동작 JSON 파싱 실패: {e}"))
}

// --- UI 플로우 DB (사이트 URL별 시나리오 이름으로 관리) ---

/// UI 동작 플로우를 DB에 저장(사이트 URL + 이름 기준 upsert).
#[tauri::command]
pub fn save_ui_flow(
    state: State<AppState>,
    name: String,
    site_url: String,
    actions: Vec<capture_server::UiAction>,
) -> Result<i64, String> {
    if name.trim().is_empty() {
        return Err("시나리오 이름을 입력하세요".into());
    }
    // URL 정규화: 끝의 / 제거 → 같은 사이트가 여러 항목으로 갈라지지 않게
    let site = site_url.trim().trim_end_matches('/');
    let json = serde_json::to_string(&actions).map_err(|e| e.to_string())?;
    state.db.lock().unwrap().save_ui_flow(name.trim(), site, &json, &now())
}

/// DB의 모든 UI 플로우(편집용 불러오기 목록).
#[tauri::command]
pub fn list_all_ui_flows(state: State<AppState>) -> Result<Vec<UiFlowRecord>, String> {
    state.db.lock().unwrap().all_ui_flows()
}

/// 저장된 사이트 URL 목록(각 URL의 시나리오 개수).
#[tauri::command]
pub fn list_ui_flow_sites(state: State<AppState>) -> Result<Vec<UiFlowSite>, String> {
    state.db.lock().unwrap().list_ui_flow_sites()
}

/// 특정 사이트 URL의 저장된 UI 플로우 목록.
#[tauri::command]
pub fn list_ui_flows(state: State<AppState>, site_url: String) -> Result<Vec<UiFlowRecord>, String> {
    state.db.lock().unwrap().list_ui_flows(&site_url)
}

#[tauri::command]
pub fn delete_ui_flow(state: State<AppState>, id: i64) -> Result<(), String> {
    state.db.lock().unwrap().delete_ui_flow(id)
}

/// DB의 모든 UI 플로우를 JSON 파일로 내보낸다.
#[tauri::command]
pub fn export_ui_flows(state: State<AppState>, path: String) -> Result<(), String> {
    let flows = state.db.lock().unwrap().all_ui_flows()?;
    let json = serde_json::to_string_pretty(&flows).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("파일 쓰기 실패: {e}"))
}

/// JSON 파일의 UI 플로우들을 DB로 가져온다(사이트 URL+이름 기준 upsert). 가져온 개수 반환.
#[tauri::command]
pub fn import_ui_flows(state: State<AppState>, path: String) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("파일 읽기 실패: {e}"))?;
    let flows: Vec<UiFlowRecord> =
        serde_json::from_str(&text).map_err(|e| format!("UI 플로우 JSON 파싱 실패: {e}"))?;
    let db = state.db.lock().unwrap();
    let now = now();
    for f in &flows {
        let site = f.site_url.trim().trim_end_matches('/');
        db.save_ui_flow(f.name.trim(), site, &f.actions_json, &now)?;
    }
    Ok(flows.len())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_token_has_prefix_and_nonempty() {
        let t = generate_capture_token();
        assert!(t.starts_with("cap-"));
        assert!(t.len() > 4);
    }
}

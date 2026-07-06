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

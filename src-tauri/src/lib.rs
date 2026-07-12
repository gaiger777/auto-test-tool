pub mod assertion;
pub mod capture_server;
pub mod capture_session;
pub mod cert_bypass;
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
            let now = chrono::Utc::now().to_rfc3339();
            let _ = db.mark_interrupted_runs(&now);
            let _ = db.mark_interrupted_ui_runs(&now);
            app.manage(AppState {
                db: Mutex::new(db),
                active_runs: Mutex::new(Default::default()),
                capture: Mutex::new(None),
                replay: Mutex::new(None),
                replay_buses: Mutex::new(std::collections::HashMap::new()),
            });
            if let Some(main) = app.get_webview_window("main") {
                // 캡처 창이 깨진 TLS(미신뢰 CA·호스트명 불일치·만료 등) 내부 서버도 로드하도록
                // wry navigation delegate 클래스에 인증서 검증 우회를 주입한다.
                // 모든 웹뷰가 같은 delegate 클래스를 공유하므로 메인 창 웹뷰로 1회 설치하면 캡처 창에도 적용된다.
                #[cfg(target_os = "macos")]
                {
                    let _ = main.with_webview(|pw| crate::cert_bypass::install(pw.inner()));
                }
                let app_handle = app.handle().clone();
                main.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Destroyed) {
                        let st = app_handle.state::<AppState>();
                        // 락을 먼저 놓고 창을 닫아 캡처 창 Destroyed 핸들러와 재진입 데드락을 피한다
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
            commands::cancel_run,
            commands::start_capture_session,
            commands::stop_capture_session,
            commands::capture_session_active,
            commands::capture_push,
            commands::ui_record,
            commands::set_ui_recording,
            commands::start_ui_replay,
            commands::ui_replay_step,
            commands::save_ui_actions,
            commands::load_ui_actions,
            commands::save_ui_flow,
            commands::list_ui_flow_sites,
            commands::list_ui_flows,
            commands::list_all_ui_flows,
            commands::delete_ui_flow,
            commands::export_ui_flows,
            commands::import_ui_flows,
            commands::stop_ui_replay,
            commands::continue_ui_replay,
            commands::resume_ui_replay,
            commands::start_replay_mq,
            commands::stop_replay_mq,
            commands::run_wait_event,
            commands::create_ui_run,
            commands::save_ui_run_step,
            commands::finish_ui_run,
            commands::list_ui_runs,
            commands::list_ui_run_steps,
            commands::rename_ui_flow,
            commands::rename_ui_group
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

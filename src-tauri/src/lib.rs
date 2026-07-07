pub mod assertion;
pub mod capture_server;
pub mod capture_session;
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
            let _ = db.mark_interrupted_runs(&chrono::Utc::now().to_rfc3339());
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

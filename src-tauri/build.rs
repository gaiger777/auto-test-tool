fn main() {
    // 앱 커맨드를 ACL 매니페스트에 선언한다. 이렇게 해야 캡처 창(외부 원격 origin)이
    // capabilities/capture.json 을 통해 capture_push 를 호출할 수 있다.
    // 매니페스트를 선언하면 모든 앱 커맨드가 ACL 대상이 되므로, 로컬 커맨드도 전부 나열하고
    // capabilities/default.json 에서 allow-* 로 허용해야 한다.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "list_environments",
            "save_environment",
            "delete_environment",
            "list_scenarios",
            "save_scenario",
            "delete_scenario",
            "export_scenario",
            "import_scenario",
            "list_runs",
            "list_step_results",
            "run_scenario",
            "cancel_run",
            "start_capture_session",
            "stop_capture_session",
            "capture_session_active",
            "capture_push",
            "ui_record",
        ])),
    )
    .expect("failed to run tauri-build");
}

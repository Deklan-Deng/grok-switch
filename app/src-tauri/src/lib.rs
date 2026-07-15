mod config_toml;
mod health;
mod models;
mod secret_store;
mod speedtest;
mod store;
mod tools;
mod tray;
mod usage;

use std::sync::Arc;

use models::CreateProviderInput;
use store::{AppStore, ProfilePatch};
use tauri::WindowEvent;
use uuid::Uuid;

#[tauri::command]
async fn get_state(
    store: tauri::State<'_, Arc<AppStore>>,
) -> Result<models::CommandResult, String> {
    let store = Arc::clone(&store);
    // Keep even the light list path off the main thread.
    run_blocking(move || store.list_profiles()).await
}

#[tauri::command]
fn add_profile(store: tauri::State<'_, Arc<AppStore>>) -> models::CommandResult {
    store.add_profile()
}

#[tauri::command]
async fn create_provider(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<AppStore>>,
    input: CreateProviderInput,
) -> Result<models::CommandResult, String> {
    let store = Arc::clone(&store);
    let result = run_blocking(move || store.create_provider(input)).await?;
    refresh_tray_bg(app);
    Ok(result)
}

#[tauri::command]
async fn import_from_config(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<AppStore>>,
    config_path: Option<String>,
) -> Result<models::CommandResult, String> {
    let store = Arc::clone(&store);
    let result = run_blocking(move || store.import_from_config(config_path)).await?;
    refresh_tray_bg(app);
    Ok(result)
}

#[tauri::command]
async fn remove_profile(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<AppStore>>,
    id: String,
) -> Result<models::CommandResult, String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let store = Arc::clone(&store);
    let result = run_blocking(move || store.remove_profile(id)).await?;
    refresh_tray_bg(app);
    Ok(result)
}

#[tauri::command]
fn rename_profile(
    store: tauri::State<'_, Arc<AppStore>>,
    id: String,
    name: String,
) -> Result<models::CommandResult, String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    Ok(store.rename_profile(id, name))
}

#[tauri::command]
fn select_profile(store: tauri::State<'_, Arc<AppStore>>, id: String) -> Result<models::CommandResult, String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    Ok(store.select_profile(id))
}

#[tauri::command]
fn update_profile(
    store: tauri::State<'_, Arc<AppStore>>,
    patch: ProfilePatch,
) -> models::CommandResult {
    store.update_profile(patch)
}

#[tauri::command]
fn save_token(
    store: tauri::State<'_, Arc<AppStore>>,
    id: Option<String>,
    token: String,
) -> Result<models::CommandResult, String> {
    let id = match id {
        Some(v) => Some(Uuid::parse_str(&v).map_err(|e| e.to_string())?),
        None => None,
    };
    Ok(store.save_token(id, token))
}

#[tauri::command]
async fn load_token(
    store: tauri::State<'_, Arc<AppStore>>,
    id: Option<String>,
) -> Result<models::CommandResult, String> {
    let id = match id {
        Some(v) => Some(Uuid::parse_str(&v).map_err(|e| e.to_string())?),
        None => None,
    };
    let store = Arc::clone(&store);
    // May touch vault / legacy Keychain — keep off the main thread.
    run_blocking(move || store.load_token(id)).await
}

#[tauri::command]
async fn apply_token(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<AppStore>>,
    id: Option<String>,
    draft_token: Option<String>,
) -> Result<models::CommandResult, String> {
    let id = match id {
        Some(v) => Some(Uuid::parse_str(&v).map_err(|e| e.to_string())?),
        None => None,
    };
    let store = Arc::clone(&store);
    // Config write + token vault I/O off the UI/async worker.
    let result = run_blocking(move || store.apply_token(id, draft_token)).await?;
    refresh_tray_bg(app);
    Ok(result)
}

#[tauri::command]
fn read_config_file(
    store: tauri::State<'_, Arc<AppStore>>,
    config_path: Option<String>,
) -> models::CommandResult {
    store.read_config_file(config_path)
}

#[tauri::command]
fn write_config_file(
    store: tauri::State<'_, Arc<AppStore>>,
    config_path: Option<String>,
    content: String,
) -> models::CommandResult {
    store.write_config_file(config_path, content)
}

#[tauri::command]
fn refresh_config(
    store: tauri::State<'_, Arc<AppStore>>,
    config_path: Option<String>,
    quietly: Option<bool>,
) -> models::CommandResult {
    store.refresh_config(config_path, quietly.unwrap_or(false))
}

#[tauri::command]
fn verify_grok(store: tauri::State<'_, Arc<AppStore>>) -> models::CommandResult {
    store.verify_grok()
}

/// Run blocking work off the async runtime so the UI stays responsive.
async fn run_blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("后台任务失败：{e}"))
}

/// Rebuild tray menu without stalling the command that just finished.
fn refresh_tray_bg(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        tray::refresh_tray(&app);
    });
}

#[tauri::command]
async fn test_connectivity(
    store: tauri::State<'_, Arc<AppStore>>,
    id: String,
) -> Result<models::CommandResult, String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let store = Arc::clone(&store);
    run_blocking(move || store.test_connectivity(id)).await
}

#[tauri::command]
async fn check_health(
    store: tauri::State<'_, Arc<AppStore>>,
    id: String,
) -> Result<health::HealthResult, String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let store = Arc::clone(&store);
    run_blocking(move || store.check_health(id)).await
}

#[tauri::command]
async fn last_health(
    store: tauri::State<'_, Arc<AppStore>>,
) -> Result<Vec<health::HealthResult>, String> {
    let store = Arc::clone(&store);
    run_blocking(move || store.last_health()).await
}

#[tauri::command]
async fn run_speed_test(
    store: tauri::State<'_, Arc<AppStore>>,
    id: String,
) -> Result<speedtest::SpeedTestResult, String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let store = Arc::clone(&store);
    run_blocking(move || store.run_speed_test(id)).await
}

#[tauri::command]
fn last_speed_tests(store: tauri::State<'_, Arc<AppStore>>) -> Vec<speedtest::SpeedTestResult> {
    store.last_speed_tests()
}

#[tauri::command]
async fn usage_summary(
    store: tauri::State<'_, Arc<AppStore>>,
    window_hours: Option<u32>,
    force: Option<bool>,
) -> Result<usage::UsageSummary, String> {
    let store = Arc::clone(&store);
    // Log scan can be multi-MB + JSON parse; keep it off the async runtime.
    run_blocking(move || store.usage_summary(window_hours, force.unwrap_or(false))).await
}

#[tauri::command]
fn fetch_available_models(store: tauri::State<'_, Arc<AppStore>>) -> models::CommandResult {
    store.fetch_available_models()
}

#[tauri::command]
fn doctor(config_path: Option<String>) -> tools::DoctorReport {
    tools::doctor(config_path.as_deref())
}

#[tauri::command]
fn list_sessions(limit: Option<usize>) -> Vec<tools::GrokSessionItem> {
    tools::list_recent_sessions(limit.unwrap_or(30))
}

#[tauri::command]
fn delete_session(path: String) -> Result<String, String> {
    tools::delete_session_project(&path)
}

#[tauri::command]
fn open_path(path: String) -> Result<String, String> {
    tools::open_path(&path)
}

#[tauri::command]
fn open_config_dir() -> Result<String, String> {
    tools::open_config_dir()
}

#[tauri::command]
fn open_config_file(config_path: Option<String>) -> Result<String, String> {
    tools::open_config_file(config_path.as_deref())
}

#[tauri::command]
fn launch_grok(cwd: Option<String>, model: Option<String>) -> Result<String, String> {
    tools::launch_grok_terminal(cwd.as_deref(), model.as_deref())
}

#[tauri::command]
fn resume_session(cwd: String) -> Result<String, String> {
    tools::resume_session_terminal(&cwd)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = Arc::new(AppStore::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(store.clone())
        .setup(move |app| {
            if let Err(err) = tray::setup_tray(app.handle(), store.clone()) {
                eprintln!("tray setup failed: {err}");
            }
            // Warm usage cache + tray line in the background — never on the UI thread.
            let warm_store = store.clone();
            let warm_app = app.handle().clone();
            std::thread::spawn(move || {
                let _ = warm_store.usage_summary(Some(24), false);
                tray::refresh_tray(&warm_app);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Close to menu bar instead of quitting.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            add_profile,
            create_provider,
            import_from_config,
            remove_profile,
            rename_profile,
            select_profile,
            update_profile,
            save_token,
            load_token,
            apply_token,
            read_config_file,
            write_config_file,
            refresh_config,
            verify_grok,
            test_connectivity,
            check_health,
            last_health,
            run_speed_test,
            last_speed_tests,
            usage_summary,
            fetch_available_models,
            doctor,
            list_sessions,
            delete_session,
            open_path,
            open_config_dir,
            open_config_file,
            launch_grok,
            resume_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

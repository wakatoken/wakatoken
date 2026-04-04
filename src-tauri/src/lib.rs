pub mod collector;
pub mod config;
pub mod heartbeat;
pub mod reporter;
mod scheduler;
mod tray;

use config::AppConfig;
use scheduler::SyncStatus;
use std::sync::Arc;
use tauri::RunEvent;

pub const BASE_URL: &str = "https://wkt.tftt.cc";

type SharedCollectors = Arc<Vec<Box<dyn collector::Collector>>>;

#[tauri::command]
fn get_base_url() -> &'static str {
    BASE_URL
}

#[tauri::command]
fn get_config() -> AppConfig {
    AppConfig::load()
}

#[tauri::command]
fn save_config(api_key: String) -> Result<(), String> {
    let config = AppConfig { api_key };
    config.save()
}

#[tauri::command]
async fn test_api_key(api_key: String) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("{BASE_URL}/api/v1/validate");
    let resp = client
        .get(&url)
        .header("x-api-key", &api_key)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        Ok("Connected successfully".to_string())
    } else if resp.status().as_u16() == 401 {
        Err("Invalid API key".to_string())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("HTTP {status}: {body}"))
    }
}

#[tauri::command]
async fn get_sync_status(
    state: tauri::State<'_, scheduler::SharedStatus>,
) -> Result<SyncStatus, String> {
    let s = state.lock().await;
    Ok(s.clone())
}

#[tauri::command]
async fn sync_now(
    app: tauri::AppHandle,
    collectors: tauri::State<'_, SharedCollectors>,
    state: tauri::State<'_, scheduler::SharedStatus>,
) -> Result<SyncStatus, String> {
    scheduler::run_sync(&Some(app.clone()), &collectors, &state).await;
    let s = state.lock().await;
    tray::update_tray(&app, &s)?;
    Ok(s.clone())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let status = scheduler::new_shared_status();
    let collectors: SharedCollectors = Arc::new(collector::create_collectors());
    let status_for_tray = status.clone();
    let status_for_scheduler = status.clone();
    let collectors_for_scheduler = collectors.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(status)
        .manage(collectors)
        .invoke_handler(tauri::generate_handler![
            get_base_url,
            get_config,
            save_config,
            test_api_key,
            get_sync_status,
            sync_now,
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::create_tray(&app.handle().clone(), &status_for_tray)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            scheduler::start_periodic_sync(
                app.handle().clone(),
                collectors_for_scheduler,
                status_for_scheduler,
            );
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building wakatoken")
        .run(|_app, event| {
            if let RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}

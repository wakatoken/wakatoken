pub mod collector;
pub mod config;
pub mod heartbeat;
pub mod reporter;
mod scheduler;
mod tray;

use config::AppConfig;
use scheduler::SyncStatus;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::RunEvent;

pub const BASE_URL: &str = "https://wkt.tftt.cc";

type SharedCollectors = Arc<Vec<Box<dyn collector::Collector>>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    #[serde(rename = "deviceCode", alias = "device_code")]
    pub device_code: String,
    #[serde(rename = "userCode", alias = "user_code")]
    pub user_code: String,
    #[serde(rename = "verificationUri", alias = "verification_uri")]
    pub verification_uri: String,
    #[serde(
        rename = "verificationUriComplete",
        alias = "verification_uri_complete"
    )]
    pub verification_uri_complete: Option<String>,
    #[serde(rename = "expiresIn", alias = "expires_in")]
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[tauri::command]
fn get_base_url() -> &'static str {
    BASE_URL
}

#[tauri::command]
fn get_config() -> AppConfig {
    AppConfig::load()
}

#[tauri::command]
async fn start_device_auth() -> Result<DeviceCodeResponse, String> {
    let client = reqwest::Client::new();
    let url = format!("{BASE_URL}/api/auth/device/code");

    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "client_id": "wkt-client" }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(response_error("Failed to get device code", resp).await);
    }

    let data: DeviceCodeResponse = resp.json().await.map_err(|e| e.to_string())?;

    let machine_id = crate::heartbeat::get_machine_id()?;
    let hostname = get_hostname()?;

    let link_resp = client
        .post(format!("{BASE_URL}/api/v1/device/link"))
        .json(&serde_json::json!({
            "deviceCode": data.device_code,
            "deviceId": machine_id,
            "hostname": hostname,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !link_resp.status().is_success() {
        return Err(response_error("Failed to link device", link_resp).await);
    }

    Ok(data)
}

fn get_hostname() -> Result<String, String> {
    let value = platform_hostname().unwrap_or_default();
    let hostname = value.trim();
    if hostname.is_empty() {
        return Ok("unknown".to_string());
    }
    Ok(hostname.to_string())
}

#[cfg(target_os = "macos")]
fn platform_hostname() -> Result<String, String> {
    let output = std::process::Command::new("scutil")
        .args(["--get", "LocalHostName"])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("failed to read LocalHostName".to_string());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_hostname() -> Result<String, String> {
    let output = std::process::Command::new("hostname")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("failed to read hostname".to_string());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn platform_hostname() -> Result<String, String> {
    std::env::var("COMPUTERNAME").map_err(|e| e.to_string())
}

#[tauri::command]
async fn poll_device_auth(device_code: String) -> Result<bool, String> {
    let client = reqwest::Client::new();
    let url = format!("{BASE_URL}/api/auth/device/token");

    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "client_id": "wkt-client",
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        let data: TokenResponse = resp.json().await.map_err(|e| e.to_string())?;
        let config = AppConfig {
            access_token: data.access_token,
        };
        config.save()?;
        Ok(true)
    } else {
        Ok(false)
    }
}

async fn response_error(prefix: &str, resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if body.is_empty() {
        format!("{prefix}: {status}")
    } else {
        format!("{prefix}: {status}: {body}")
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
            get_sync_status,
            sync_now,
            start_device_auth,
            poll_device_auth,
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

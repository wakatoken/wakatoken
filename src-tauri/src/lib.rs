mod app_menu;
pub mod auto_update;
pub mod collector;
pub mod config;
pub mod credentials;
pub mod heartbeat;
pub mod local_stats;
pub mod reporter;
mod scheduler;
mod tray;

use config::AppConfig;
use credentials::AuthCredentials;
use scheduler::SyncStatus;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, RunEvent};

pub const BASE_URL: &str = "https://wkt.tftt.cc";

type SharedCollectors = Arc<Vec<Box<dyn collector::Collector>>>;
static LOCAL_STATS_WRITE_LOCK: Mutex<()> = Mutex::new(());

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountInfo {
    signed_in: bool,
    name: String,
    email: String,
    image: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProgress {
    phase: String,
    runtime: String,
    detail: String,
    completed: usize,
    total: usize,
    sessions: usize,
    file_completed: usize,
    file_total: usize,
}

struct ScanProgressPayload<'a> {
    phase: &'a str,
    runtime: &'a str,
    detail: &'a str,
    completed: usize,
    total: usize,
    sessions: usize,
    file_completed: usize,
    file_total: usize,
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    user: Option<SessionUser>,
}

#[derive(Debug, Deserialize)]
struct SessionUser {
    name: Option<String>,
    email: Option<String>,
    image: Option<String>,
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
fn save_runtime_settings(enabled_runtimes: Vec<String>) -> Result<AppConfig, String> {
    let mut config = AppConfig::load();
    config.enabled_runtimes = enabled_runtimes;
    config.save()?;
    Ok(config)
}

#[tauri::command]
fn complete_onboarding(enabled_runtimes: Vec<String>) -> Result<AppConfig, String> {
    let mut config = AppConfig::load();
    config.enabled_runtimes = enabled_runtimes;
    config.onboarding_completed = true;
    config.save()?;
    Ok(config)
}

#[tauri::command]
fn sign_out() -> Result<AppConfig, String> {
    AuthCredentials::clear()?;
    Ok(AppConfig::load())
}

#[tauri::command]
async fn get_account() -> Result<AccountInfo, String> {
    let credentials = AuthCredentials::load();
    if !credentials.signed_in() {
        return Ok(signed_out_account());
    }

    let resp = reqwest::Client::new()
        .get(format!("{BASE_URL}/api/auth/get-session"))
        .header(
            "Authorization",
            format!("Bearer {}", credentials.access_token),
        )
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Ok(signed_out_account());
    }

    let session: SessionResponse = resp.json().await.map_err(|e| e.to_string())?;
    let Some(user) = session.user else {
        return Ok(signed_out_account());
    };

    Ok(AccountInfo {
        signed_in: true,
        name: user.name.unwrap_or_else(|| "Signed in".to_string()),
        email: user.email.unwrap_or_default(),
        image: user.image,
    })
}

fn signed_out_account() -> AccountInfo {
    AccountInfo {
        signed_in: false,
        name: String::new(),
        email: String::new(),
        image: None,
    }
}

#[tauri::command]
fn get_local_dashboard() -> Result<local_stats::LocalDashboard, String> {
    local_stats::get_local_dashboard()
}

#[tauri::command]
fn has_local_stats() -> Result<bool, String> {
    local_stats::has_store()
}

#[tauri::command]
fn list_sessions(runtime: Option<String>) -> Result<Vec<local_stats::SessionSummary>, String> {
    local_stats::list_sessions(runtime, Some(100))
}

#[tauri::command]
async fn rescan_local_stats(
    app: tauri::AppHandle,
    collectors: tauri::State<'_, SharedCollectors>,
) -> Result<local_stats::LocalDashboard, String> {
    let config = AppConfig::load();
    rescan_runtimes_with_progress(
        Some(app),
        collectors.inner().clone(),
        config.enabled_runtimes,
    )
    .await
}

#[tauri::command]
async fn rescan_runtimes(
    app: tauri::AppHandle,
    collectors: tauri::State<'_, SharedCollectors>,
    runtimes: Vec<String>,
) -> Result<local_stats::LocalDashboard, String> {
    rescan_runtimes_with_progress(Some(app), collectors.inner().clone(), runtimes).await
}

#[tauri::command]
async fn rescan_runtime_stats(
    app: tauri::AppHandle,
    collectors: tauri::State<'_, SharedCollectors>,
    runtime: String,
) -> Result<local_stats::LocalDashboard, String> {
    rescan_runtimes_with_progress(Some(app), collectors.inner().clone(), vec![runtime]).await
}

async fn rescan_runtimes_with_progress(
    app: Option<tauri::AppHandle>,
    collectors: SharedCollectors,
    runtimes: Vec<String>,
) -> Result<local_stats::LocalDashboard, String> {
    let machine_id = crate::heartbeat::get_machine_id()?;
    let enabled: Vec<(usize, String)> = collectors
        .iter()
        .enumerate()
        .filter(|(_, collector)| runtimes.iter().any(|runtime| runtime == collector.name()))
        .map(|(index, collector)| (index, collector.name().to_string()))
        .collect();

    let total = enabled.len();
    emit_scan_progress(
        &app,
        ScanProgressPayload {
            phase: "scanning",
            runtime: "",
            detail: "Scanning local sessions...",
            completed: 0,
            total,
            sessions: 0,
            file_completed: 0,
            file_total: 0,
        },
    );

    let mut tasks = tokio::task::JoinSet::new();
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    for (index, runtime) in enabled {
        emit_scan_progress(
            &app,
            ScanProgressPayload {
                phase: "runtime-started",
                runtime: &runtime,
                detail: &format!("Scanning {runtime}..."),
                completed: 0,
                total,
                sessions: 0,
                file_completed: 0,
                file_total: 0,
            },
        );

        let collectors = collectors.clone();
        let machine_id = machine_id.clone();
        let progress_tx = progress_tx.clone();
        tasks.spawn_blocking(move || {
            let mut progress = |file_completed: usize, file_total: usize| {
                progress_tx
                    .send((runtime.clone(), file_completed, file_total))
                    .ok();
            };
            let result = collectors[index].scan_all_with_progress(&machine_id, &mut progress);
            (runtime, result)
        });
    }
    drop(progress_tx);

    let mut sessions = Vec::new();
    let mut errors = Vec::new();
    let mut completed = 0usize;

    while completed < total {
        tokio::select! {
            Some((runtime, file_completed, file_total)) = progress_rx.recv() => {
                emit_scan_progress(
                    &app,
                    ScanProgressPayload {
                        phase: "runtime-progress",
                        runtime: &runtime,
                        detail: &format!("{runtime} scanned {file_completed}/{file_total} files"),
                        completed,
                        total,
                        sessions: 0,
                        file_completed,
                        file_total,
                    },
                );
            }
            Some(result) = tasks.join_next() => {
                let (runtime, scan_result) = result.map_err(|e| e.to_string())?;
                completed += 1;
                match scan_result {
                    Ok(mut runtime_sessions) => {
                        let session_count = runtime_sessions.len();
                        sessions.append(&mut runtime_sessions);
                        emit_scan_progress(
                            &app,
                            ScanProgressPayload {
                                phase: "runtime-done",
                                runtime: &runtime,
                                detail: &format!("{runtime} scanned {session_count} sessions"),
                                completed,
                                total,
                                sessions: session_count,
                                file_completed: 1,
                                file_total: 1,
                            },
                        );
                    }
                    Err(error) => {
                        emit_scan_progress(
                            &app,
                            ScanProgressPayload {
                                phase: "runtime-error",
                                runtime: &runtime,
                                detail: &format!("{runtime} failed: {error}"),
                                completed,
                                total,
                                sessions: 0,
                                file_completed: 1,
                                file_total: 1,
                            },
                        );
                        errors.push(format!("{runtime}: {error}"));
                    }
                }
            }
        }
    }

    if !errors.is_empty() {
        let error = errors.join("; ");
        emit_scan_progress(
            &app,
            ScanProgressPayload {
                phase: "error",
                runtime: "",
                detail: &error,
                completed,
                total,
                sessions: sessions.len(),
                file_completed: 0,
                file_total: 0,
            },
        );
        return Err(error);
    }

    let scanned_runtimes = enabled_runtime_names(&collectors, &runtimes);
    let _guard = LOCAL_STATS_WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    let dashboard = local_stats::replace_runtime_sessions(&scanned_runtimes, &sessions)?;
    emit_scan_progress(
        &app,
        ScanProgressPayload {
            phase: "done",
            runtime: "",
            detail: &format!("Scanned {} sessions", dashboard.session_count),
            completed,
            total,
            sessions: dashboard.session_count as usize,
            file_completed: 1,
            file_total: 1,
        },
    );
    Ok(dashboard)
}

fn enabled_runtime_names(collectors: &SharedCollectors, runtimes: &[String]) -> Vec<String> {
    collectors
        .iter()
        .filter(|collector| runtimes.iter().any(|runtime| runtime == collector.name()))
        .map(|collector| collector.name().to_string())
        .collect()
}

fn emit_scan_progress(app: &Option<tauri::AppHandle>, progress: ScanProgressPayload<'_>) {
    if let Some(app) = app {
        app.emit(
            "scan-progress",
            ScanProgress {
                phase: progress.phase.to_string(),
                runtime: progress.runtime.to_string(),
                detail: progress.detail.to_string(),
                completed: progress.completed,
                total: progress.total,
                sessions: progress.sessions,
                file_completed: progress.file_completed,
                file_total: progress.file_total,
            },
        )
        .ok();
    }
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
        AuthCredentials {
            access_token: data.access_token,
        }
        .save()?;
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
        .menu(app_menu::build)
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(status)
        .manage(collectors)
        .invoke_handler(tauri::generate_handler![
            get_base_url,
            get_config,
            save_runtime_settings,
            complete_onboarding,
            sign_out,
            get_account,
            get_local_dashboard,
            has_local_stats,
            list_sessions,
            rescan_local_stats,
            rescan_runtimes,
            rescan_runtime_stats,
            get_sync_status,
            sync_now,
            start_device_auth,
            poll_device_auth,
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            tray::create_tray(&app.handle().clone(), &status_for_tray)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            tray::show_main_window(&app.handle().clone());
            scheduler::start_periodic_sync(
                app.handle().clone(),
                collectors_for_scheduler,
                status_for_scheduler,
            );
            auto_update::start_periodic_update_check(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building wakatoken")
        .run(|_app, event| {
            if let RunEvent::ExitRequested { code, api, .. } = event {
                if code != Some(tauri::RESTART_EXIT_CODE) {
                    api.prevent_exit();
                }
            }
        });
}

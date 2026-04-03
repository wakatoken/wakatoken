use crate::collector::{self, Collector};
use crate::config::AppConfig;
use crate::reporter;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio::time::{interval_at, Duration, Instant};

const SYNC_INTERVAL_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub last_sync_ts: i64,
    pub last_sync_ok: bool,
    pub last_error: String,
    pub total_synced: u64,
    pub today_input_tokens: u64,
    pub today_output_tokens: u64,
    #[serde(default)]
    pub today_date: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub phase: String,
    pub detail: String,
    pub percent: u8,
}

pub type SharedStatus = Arc<Mutex<SyncStatus>>;

pub fn new_shared_status() -> SharedStatus {
    Arc::new(Mutex::new(SyncStatus::default()))
}

fn emit_progress(app: &Option<AppHandle>, detail: &str) {
    if let Some(app) = app {
        crate::tray::set_status_text(app, detail);
        app.emit(
            "sync-progress",
            SyncProgress {
                phase: "syncing".to_string(),
                detail: detail.to_string(),
                percent: 0,
            },
        )
        .ok();
    }
}

pub async fn run_sync(
    app: &Option<AppHandle>,
    collectors: &[Box<dyn Collector>],
    status: &SharedStatus,
) {
    let config = AppConfig::load();
    if config.api_key.is_empty() {
        let mut s = status.lock().await;
        s.last_sync_ok = false;
        s.last_error = "API key not configured".to_string();
        return;
    }

    // 1. Scan all session files
    emit_progress(app, "Scanning...");
    eprintln!("[wakatoken] scanning...");

    let mut all_sessions = Vec::new();
    for c in collectors {
        match c.collect() {
            Ok(sessions) => all_sessions.extend(sessions),
            Err(e) => eprintln!("[wakatoken] collector {} error: {e}", c.name()),
        }
    }

    let file_count = all_sessions.len();
    let msg_count: usize = all_sessions.iter().map(|s| s.heartbeats.len()).sum();
    eprintln!("[wakatoken] found {file_count} sessions, {msg_count} messages");

    if all_sessions.is_empty() {
        eprintln!("[wakatoken] nothing to upload");
        emit_progress(app, "No new data");
        let mut s = status.lock().await;
        s.last_sync_ts = chrono::Utc::now().timestamp();
        s.last_sync_ok = true;
        s.last_error.clear();
        reset_if_new_day(&mut s, today_start_millis());
        return;
    }

    crate::tray::set_syncing(true);
    emit_progress(
        app,
        &format!("Uploading {file_count} sessions ({msg_count} msgs)..."),
    );

    // 2. Upload per-session, commit offset after each succeeds
    let client = reqwest::Client::new();
    let today_start = today_start_millis();
    let mut total_new = 0u64;
    let mut total_dedup = 0u64;
    let mut batch_today_input = 0u64;
    let mut batch_today_output = 0u64;
    let start = std::time::Instant::now();

    for (i, session) in all_sessions.iter().enumerate() {
        let eta = if i > 0 {
            let per_session = start.elapsed().as_secs_f64() / i as f64;
            let remaining = (file_count - i) as f64 * per_session;
            format_eta(remaining.ceil() as u64)
        } else {
            String::new()
        };
        let progress = format!("Uploading session {}/{file_count}{eta}", i + 1);
        emit_progress(app, &progress);
        eprintln!("[wakatoken] {progress}");

        match reporter::send_heartbeats(&client, &config.api_key, session.heartbeats.clone()).await
        {
            Ok(result) => {
                // Find which collector owns this file and commit its offset
                for c in collectors {
                    c.commit_file(&session.path, session.offset);
                }
                total_new += result.new_count;
                total_dedup += result.dedup_count;

                // Accumulate today tokens
                for hb in &session.heartbeats {
                    if hb.event_ts >= today_start {
                        batch_today_input += hb.input_tokens;
                        batch_today_output += hb.output_tokens;
                    }
                }
            }
            Err(e) => {
                // Skip failed session, its offset stays uncommitted so it'll retry next sync
                eprintln!(
                    "[wakatoken] session {} failed, skipping: {e}",
                    session.path.display()
                );
            }
        }
    }

    crate::tray::set_syncing(false);
    let msg = format!("{total_new} new, {total_dedup} dedup");
    eprintln!("[wakatoken] done: {msg}");
    emit_progress(app, &msg);
    let mut s = status.lock().await;
    s.last_sync_ts = chrono::Utc::now().timestamp();
    s.last_sync_ok = true;
    s.last_error.clear();
    s.total_synced += total_new;
    reset_if_new_day(&mut s, today_start);
    s.today_input_tokens += batch_today_input;
    s.today_output_tokens += batch_today_output;
}

fn reset_if_new_day(status: &mut SyncStatus, today_start: i64) {
    if status.today_date != today_start {
        status.today_date = today_start;
        status.today_input_tokens = 0;
        status.today_output_tokens = 0;
    }
}

fn format_eta(secs: u64) -> String {
    if secs > 60 {
        format!(", ETA {}m{}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!(", ETA {secs}s")
    } else {
        String::new()
    }
}

pub fn start_periodic_sync(
    app: tauri::AppHandle,
    collectors: Arc<Vec<Box<dyn collector::Collector>>>,
    status: SharedStatus,
) {
    tauri::async_runtime::spawn(async move {
        run_sync(&Some(app.clone()), &collectors, &status).await;
        update_tray_from_status(&app, &status).await;

        let mut ticker = interval_at(
            Instant::now() + Duration::from_secs(SYNC_INTERVAL_SECS),
            Duration::from_secs(SYNC_INTERVAL_SECS),
        );
        loop {
            ticker.tick().await;
            run_sync(&Some(app.clone()), &collectors, &status).await;
            update_tray_from_status(&app, &status).await;
        }
    });
}

async fn update_tray_from_status(app: &tauri::AppHandle, status: &SharedStatus) {
    let s = status.lock().await;
    crate::tray::update_tray(app, &s).ok();
}

fn today_start_millis() -> i64 {
    let now = chrono::Local::now();
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp_millis()
}

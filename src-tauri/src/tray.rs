use crate::scheduler::{SharedStatus, SyncStatus};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

const TRAY_ID: &str = "main-tray";
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-icon@2x.png");

fn dashboard_url() -> String {
    format!("{}/dashboard", crate::BASE_URL)
}

/// Menu items that get updated in-place (no menu rebuild needed).
struct TrayMenuItems {
    today_total: MenuItem<tauri::Wry>,
    today_input: MenuItem<tauri::Wry>,
    today_output: MenuItem<tauri::Wry>,
    sync_status: MenuItem<tauri::Wry>,
    sync_now: MenuItem<tauri::Wry>,
}

static MENU_ITEMS: Mutex<Option<TrayMenuItems>> = Mutex::new(None);

pub fn create_tray(app: &AppHandle, status: &SharedStatus) -> Result<(), String> {
    let status_clone = status.clone();
    let (menu, items) = build_menu(app, &SyncStatus::default())?;
    *MENU_ITEMS.lock().unwrap() = Some(items);

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(TRAY_ICON).map_err(|e| e.to_string())?)
        .icon_as_template(true)
        .tooltip("WakaToken")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "sync_now" => {
                let status = status_clone.clone();
                let app_clone = app.clone();
                let collectors: std::sync::Arc<Vec<Box<dyn crate::collector::Collector>>> = app
                    .state::<std::sync::Arc<Vec<Box<dyn crate::collector::Collector>>>>()
                    .inner()
                    .clone();
                tauri::async_runtime::spawn(async move {
                    crate::scheduler::run_sync(&Some(app_clone.clone()), &collectors, &status)
                        .await;
                    let s = status.lock().await;
                    update_tray(&app_clone, &s).ok();
                });
            }
            "dashboard" => open_url(app, &dashboard_url()),
            "settings" => open_settings_window(app),
            "quit" => std::process::exit(0),
            _ => {}
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Full update after sync — refreshes all menu item texts and tooltip.
pub fn update_tray(app: &AppHandle, status: &SyncStatus) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_tooltip(Some(&format_tooltip(status))).ok();
    }

    if let Ok(guard) = MENU_ITEMS.lock() {
        if let Some(items) = guard.as_ref() {
            let total = status.today_input_tokens + status.today_output_tokens;
            items
                .today_total
                .set_text(format!("Today: {}", format_tokens(total)))
                .ok();
            items
                .today_input
                .set_text(format!(
                    "  Input:  {}",
                    format_tokens(status.today_input_tokens)
                ))
                .ok();
            items
                .today_output
                .set_text(format!(
                    "  Output: {}",
                    format_tokens(status.today_output_tokens)
                ))
                .ok();
            items.sync_status.set_text(format_status_line(status)).ok();
        }
    }

    Ok(())
}

/// Update sync status line in-place — menu stays open, no flicker.
pub fn set_status_text(_app: &AppHandle, text: &str) {
    if let Ok(guard) = MENU_ITEMS.lock() {
        if let Some(items) = guard.as_ref() {
            items.sync_status.set_text(text).ok();
        }
    }
}

pub fn set_syncing(syncing: bool) {
    if let Ok(guard) = MENU_ITEMS.lock() {
        if let Some(items) = guard.as_ref() {
            items.sync_now.set_enabled(!syncing).ok();
            if syncing {
                items.sync_now.set_text("Syncing...").ok();
            } else {
                items.sync_now.set_text("Sync Now").ok();
            }
        }
    }
}

fn build_menu(
    app: &AppHandle,
    status: &SyncStatus,
) -> Result<(tauri::menu::Menu<tauri::Wry>, TrayMenuItems), String> {
    let total = status.today_input_tokens + status.today_output_tokens;

    let today_total = mi(
        app,
        "today_total",
        format!("Today: {}", format_tokens(total)),
        false,
    )?;
    let today_input = mi(
        app,
        "today_input",
        format!("  Input:  {}", format_tokens(status.today_input_tokens)),
        false,
    )?;
    let today_output = mi(
        app,
        "today_output",
        format!("  Output: {}", format_tokens(status.today_output_tokens)),
        false,
    )?;
    let sync_status = mi(app, "sync_status", format_status_line(status), false)?;

    let sync_now = mi(app, "sync_now", "Sync Now".to_string(), true)?;

    let menu = MenuBuilder::new(app)
        .item(&today_total)
        .item(&today_input)
        .item(&today_output)
        .item(&PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?)
        .item(&sync_status)
        .item(&PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?)
        .item(&sync_now)
        .item(&mi(app, "dashboard", "Open Dashboard".to_string(), true)?)
        .item(&mi(app, "settings", "Settings...".to_string(), true)?)
        .item(&PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?)
        .item(&mi(app, "quit", "Quit WakaToken".to_string(), true)?)
        .build()
        .map_err(|e| e.to_string())?;

    let items = TrayMenuItems {
        today_total,
        today_input,
        today_output,
        sync_status,
        sync_now,
    };
    Ok((menu, items))
}

fn mi(
    app: &AppHandle,
    id: &str,
    label: String,
    enabled: bool,
) -> Result<MenuItem<tauri::Wry>, String> {
    MenuItemBuilder::with_id(id, label)
        .enabled(enabled)
        .build(app)
        .map_err(|e| e.to_string())
}

fn format_status_line(status: &SyncStatus) -> String {
    if status.last_sync_ts == 0 {
        "Not synced yet".to_string()
    } else if status.last_sync_ok {
        format!(
            "Synced {} · {}",
            format_count(status.total_synced),
            format_time_ago(status.last_sync_ts)
        )
    } else {
        format!("Sync failed: {}", truncate(&status.last_error, 40))
    }
}

fn format_tooltip(status: &SyncStatus) -> String {
    let total = status.today_input_tokens + status.today_output_tokens;
    if status.last_sync_ts == 0 {
        "WakaToken".to_string()
    } else {
        format!(
            "WakaToken — Today {} | {}",
            format_tokens(total),
            format_time_ago(status.last_sync_ts)
        )
    }
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M tokens", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k tokens", n as f64 / 1_000.0)
    } else {
        format!("{n} tokens")
    }
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{n}")
    }
}

fn format_time_ago(ts: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = (now - ts).max(0);
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

fn open_url(app: &AppHandle, url: &str) {
    app.opener().open_url(url, None::<&str>).ok();
}

fn open_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        window.set_focus().ok();
        return;
    }

    tauri::WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("index.html".into()))
        .title("WakaToken Settings")
        .inner_size(480.0, 420.0)
        .resizable(false)
        .center()
        .build()
        .ok();
}

use crate::scheduler::{SharedStatus, SyncStatus};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_opener::OpenerExt;

const TRAY_ID: &str = "main-tray";
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-icon@2x.png");

fn dashboard_url() -> String {
    format!("{}/dashboard", crate::BASE_URL)
}

/// Menu items that get updated in-place (no menu rebuild needed).
struct TrayMenuItems {
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
            "show_app" => show_main_window(app),
            "quit" => quit_after_sync(app),
            _ => {}
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn quit_after_sync(app: &AppHandle) {
    if !crate::scheduler::is_running() {
        std::process::exit(0);
    }

    crate::scheduler::request_stop();
    set_status_text(app, "Stopping sync before quit...");
    set_syncing(true);
    tauri::async_runtime::spawn(async move {
        while crate::scheduler::is_running() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        std::process::exit(0);
    });
}

/// Full update after sync — refreshes all menu item texts and tooltip.
pub fn update_tray(app: &AppHandle, status: &SyncStatus) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_tooltip(Some(&format_tooltip(status))).ok();
    }

    if let Ok(guard) = MENU_ITEMS.lock() {
        if let Some(items) = guard.as_ref() {
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
    let sync_status = mi(app, "sync_status", format_status_line(status), false)?;

    let sync_now = mi(app, "sync_now", "Sync Now".to_string(), true)?;

    let menu = MenuBuilder::new(app)
        .item(&mi(app, "show_app", "Show WakaToken".to_string(), true)?)
        .item(&sync_status)
        .item(&PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?)
        .item(&sync_now)
        .item(&mi(
            app,
            "dashboard",
            "Open Cloud Dashboard".to_string(),
            true,
        )?)
        .item(&PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?)
        .item(&mi(app, "quit", "Quit WakaToken".to_string(), true)?)
        .build()
        .map_err(|e| e.to_string())?;

    let items = TrayMenuItems {
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

pub fn show_main_window(app: &AppHandle) {
    show_in_dock(app);

    if let Some(window) = app.get_webview_window("main") {
        window.show().ok();
        window.set_focus().ok();
        return;
    }

    if let Ok(window) =
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
            .title("WakaToken")
            .inner_size(1120.0, 760.0)
            .min_inner_size(940.0, 640.0)
            .resizable(true)
            .center()
            .build()
    {
        handle_close_to_tray(app, &window);
        window.set_focus().ok();
    }
}

fn handle_close_to_tray(app: &AppHandle, window: &WebviewWindow) {
    let app = app.clone();
    let window = window.clone();

    window.clone().on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            window.hide().ok();
            hide_from_dock(&app);
        }
    });
}

#[cfg(target_os = "macos")]
fn show_in_dock(app: &AppHandle) {
    app.set_activation_policy(tauri::ActivationPolicy::Regular)
        .ok();
}

#[cfg(not(target_os = "macos"))]
fn show_in_dock(_app: &AppHandle) {}

#[cfg(target_os = "macos")]
fn hide_from_dock(app: &AppHandle) {
    app.set_activation_policy(tauri::ActivationPolicy::Accessory)
        .ok();
}

#[cfg(not(target_os = "macos"))]
fn hide_from_dock(_app: &AppHandle) {}

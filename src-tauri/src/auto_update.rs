use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;
use tokio::time::{sleep, Duration};

const INITIAL_CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

pub fn start_periodic_update_check(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        log::info!("auto update checker started");
        sleep(INITIAL_CHECK_DELAY).await;

        let mut skipped_version = None;
        loop {
            if let Err(error) = check_once(&app, &mut skipped_version).await {
                log::warn!("auto update check failed: {error}");
            }
            sleep(CHECK_INTERVAL).await;
        }
    });
}

async fn check_once(
    app: &tauri::AppHandle,
    skipped_version: &mut Option<String>,
) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(());
    };

    if skipped_version.as_deref() == Some(update.version.as_str()) {
        return Ok(());
    }

    let version = update.version.clone();
    let current_version = update.current_version.clone();
    let confirmed = app
        .dialog()
        .message(format!(
            "WakaToken {version} is available. You are running {current_version}. Install it now?"
        ))
        .title("Update Available")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Update now".to_string(),
            "Later".to_string(),
        ))
        .blocking_show();

    if !confirmed {
        *skipped_version = Some(version);
        return Ok(());
    }

    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.request_restart();
    Ok(())
}

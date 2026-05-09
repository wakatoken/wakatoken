#[cfg(target_os = "macos")]
use tauri::image::Image;
use tauri::menu::Menu;
#[cfg(target_os = "macos")]
use tauri::menu::{AboutMetadata, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID};
use tauri::AppHandle;

#[cfg(target_os = "macos")]
const ABOUT_ICON: &[u8] = include_bytes!("../icons/icon.png");

pub fn build(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    #[cfg(target_os = "macos")]
    {
        build_macos_menu(app)
    }

    #[cfg(not(target_os = "macos"))]
    {
        Menu::default(app)
    }
}

#[cfg(target_os = "macos")]
fn build_macos_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let about_metadata = about_metadata(app)?;
    let window_menu = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    let help_menu = Submenu::with_id_and_items(app, HELP_SUBMENU_ID, "Help", true, &[])?;

    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                "WakaToken",
                true,
                &[
                    &PredefinedMenuItem::about(app, None, Some(about_metadata))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "File",
                true,
                &[&PredefinedMenuItem::close_window(app, None)?],
            )?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "View",
                true,
                &[&PredefinedMenuItem::fullscreen(app, None)?],
            )?,
            &window_menu,
            &help_menu,
        ],
    )
}

#[cfg(target_os = "macos")]
fn about_metadata(app: &AppHandle) -> tauri::Result<AboutMetadata<'static>> {
    let pkg_info = app.package_info();
    let config = app.config();
    Ok(AboutMetadata {
        name: Some("WakaToken".to_string()),
        version: Some(pkg_info.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config
            .bundle
            .publisher
            .clone()
            .map(|publisher| vec![publisher]),
        icon: Some(Image::from_bytes(ABOUT_ICON)?),
        ..Default::default()
    })
}

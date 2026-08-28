use std::error::Error;

use tauri::{
    AppHandle, Manager, Runtime,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

const TRAY_ID: &str = "snaprust-tray";
const CAPTURE_ID: &str = "tray_capture";
const HISTORY_ID: &str = "tray_history";
const SETTINGS_ID: &str = "tray_settings";
const QUIT_ID: &str = "tray_quit";

pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), Box<dyn Error>> {
    let capture_item = MenuItem::with_id(app, CAPTURE_ID, "开始截图", true, None::<&str>)?;
    let history_item = MenuItem::with_id(app, HISTORY_ID, "截图历史", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, SETTINGS_ID, "翻译设置", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, QUIT_ID, "退出 SnapRust", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &capture_item,
            &history_item,
            &settings_item,
            &separator,
            &quit_item,
        ],
    )?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("default SnapRust icon is unavailable")?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("SnapRust")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(handle_menu_event)
        .build(app)?;

    Ok(())
}

fn close_current_overlay<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let overlay_visible = crate::window::is_capture_overlay_visible(app)?;
    let capture_active = app
        .state::<crate::screenshot::CaptureSession>()
        .is_active()?;
    if overlay_visible || capture_active {
        crate::screenshot::cancel_capture(app)?;
    }
    Ok(())
}

fn start_capture<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    close_current_overlay(app)?;
    crate::screenshot::begin_capture(app)
}

fn open_history<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    close_current_overlay(app)?;
    crate::history::show_history_window(app)
}

fn open_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    close_current_overlay(app)?;
    crate::window::prepare_settings_window(app)
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: tauri::menu::MenuEvent) {
    let result = match event.id.as_ref() {
        CAPTURE_ID => start_capture(app),
        HISTORY_ID => open_history(app),
        SETTINGS_ID => open_settings(app),
        QUIT_ID => {
            app.exit(0);
            Ok(())
        }
        _ => Ok(()),
    };

    if let Err(error) = result {
        eprintln!("tray action '{}' failed: {error}", event.id.as_ref());
    }
}

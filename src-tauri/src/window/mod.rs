use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Runtime};

use crate::screenshot::VirtualDesktop;

const OVERLAY_LABEL: &str = "overlay";
const HISTORY_LABEL: &str = "history";
const SETTINGS_LABEL: &str = "settings";

pub fn prepare_capture_overlay<R: Runtime>(
    app: &AppHandle<R>,
    desktop: &VirtualDesktop,
) -> Result<(), String> {
    let overlay = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "capture overlay window is unavailable".to_owned())?;

    overlay
        .set_fullscreen(false)
        .map_err(|error| error.to_string())?;
    overlay
        .set_position(PhysicalPosition::new(desktop.x, desktop.y))
        .map_err(|error| error.to_string())?;
    overlay
        .set_size(PhysicalSize::new(desktop.width, desktop.height))
        .map_err(|error| error.to_string())?;
    overlay
        .emit("capture://reset", ())
        .map_err(|error| error.to_string())?;

    Ok(())
}

pub fn reveal_capture_overlay<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let overlay = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "capture overlay window is unavailable".to_owned())?;

    overlay.show().map_err(|error| error.to_string())?;
    overlay.set_focus().map_err(|error| error.to_string())
}

pub fn is_capture_overlay_visible<R: Runtime>(app: &AppHandle<R>) -> Result<bool, String> {
    let overlay = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "capture overlay window is unavailable".to_owned())?;

    overlay.is_visible().map_err(|error| error.to_string())
}

pub fn hide_capture_overlay<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let overlay = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "capture overlay window is unavailable".to_owned())?;

    overlay.hide().map_err(|error| error.to_string())
}

pub fn prepare_history_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    hide_settings_window(app)?;
    let overlay = app
        .get_webview_window(HISTORY_LABEL)
        .ok_or_else(|| "history window is unavailable".to_owned())?;
    overlay
        .set_fullscreen(false)
        .map_err(|error| error.to_string())?;
    overlay
        .set_size(LogicalSize::new(1_080_f64, 720_f64))
        .map_err(|error| error.to_string())?;
    overlay.center().map_err(|error| error.to_string())?;
    overlay
        .emit("history://show", ())
        .map_err(|error| error.to_string())?;
    overlay.show().map_err(|error| error.to_string())?;
    overlay.set_focus().map_err(|error| error.to_string())
}

pub fn prepare_settings_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    hide_history_window(app)?;
    let overlay = app
        .get_webview_window(SETTINGS_LABEL)
        .ok_or_else(|| "settings window is unavailable".to_owned())?;
    overlay
        .set_fullscreen(false)
        .map_err(|error| error.to_string())?;
    overlay
        .set_size(LogicalSize::new(640_f64, 600_f64))
        .map_err(|error| error.to_string())?;
    overlay.center().map_err(|error| error.to_string())?;
    overlay
        .emit("settings://show", ())
        .map_err(|error| error.to_string())?;
    overlay.show().map_err(|error| error.to_string())?;
    overlay.set_focus().map_err(|error| error.to_string())
}

pub fn hide_history_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.get_webview_window(HISTORY_LABEL)
        .ok_or_else(|| "history window is unavailable".to_owned())?
        .hide()
        .map_err(|error| error.to_string())
}

pub fn hide_settings_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.get_webview_window(SETTINGS_LABEL)
        .ok_or_else(|| "settings window is unavailable".to_owned())?
        .hide()
        .map_err(|error| error.to_string())
}

pub fn hide_auxiliary_windows<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    for label in [HISTORY_LABEL, SETTINGS_LABEL] {
        let window = app
            .get_webview_window(label)
            .ok_or_else(|| format!("{label} window is unavailable"))?;
        if window.is_visible().map_err(|error| error.to_string())? {
            window.hide().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

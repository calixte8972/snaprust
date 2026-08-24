use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Runtime};

use crate::screenshot::VirtualDesktop;

const OVERLAY_LABEL: &str = "overlay";

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

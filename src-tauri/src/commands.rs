use tauri::{AppHandle, Manager, Runtime, State};

use crate::{
    annotation::Annotation,
    screenshot::{
        CapturePayload, CaptureSession, CopyPayload, PhysicalSelectionRect, SelectionPayload,
    },
};

#[tauri::command]
pub fn show_capture_overlay<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    crate::screenshot::begin_capture(&app)
}

#[tauri::command]
pub fn reveal_capture_overlay<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    if !app.state::<CaptureSession>().is_active()? {
        return Err("there is no prepared screen capture to reveal".to_owned());
    }
    crate::window::reveal_capture_overlay(&app)
}

#[tauri::command]
pub fn cancel_capture<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    crate::screenshot::cancel_capture(&app)
}

#[tauri::command]
pub fn get_current_capture(session: State<'_, CaptureSession>) -> Result<CapturePayload, String> {
    session.payload()
}

#[tauri::command]
pub async fn get_current_capture_image<R: Runtime>(
    app: AppHandle<R>,
) -> Result<tauri::ipc::Response, String> {
    let png =
        tauri::async_runtime::spawn_blocking(move || app.state::<CaptureSession>().capture_png())
            .await
            .map_err(|error| format!("screen PNG worker failed: {error}"))??;
    Ok(tauri::ipc::Response::new(png))
}

#[tauri::command]
pub fn select_capture_region(
    selection: PhysicalSelectionRect,
    session: State<'_, CaptureSession>,
) -> Result<SelectionPayload, String> {
    session.select(selection)
}

#[tauri::command]
pub async fn get_selected_capture_image<R: Runtime>(
    app: AppHandle<R>,
) -> Result<tauri::ipc::Response, String> {
    let png =
        tauri::async_runtime::spawn_blocking(move || app.state::<CaptureSession>().selected_png())
            .await
            .map_err(|error| format!("selected PNG worker failed: {error}"))??;
    Ok(tauri::ipc::Response::new(png))
}

#[tauri::command]
pub fn set_capture_annotations(
    annotations: Vec<Annotation>,
    session: State<'_, CaptureSession>,
) -> Result<(), String> {
    session.set_annotations(annotations)
}

#[tauri::command]
pub fn copy_selected_capture<R: Runtime>(app: AppHandle<R>) -> Result<CopyPayload, String> {
    crate::screenshot::copy_selected_capture(&app)
}

#[tauri::command]
pub async fn pin_selected_capture<R: Runtime>(
    app: AppHandle<R>,
) -> Result<crate::pin::PinCreatedPayload, String> {
    crate::screenshot::pin_selected_capture(&app)
}

#[tauri::command]
pub fn get_pinned_capture(
    label: String,
    store: State<'_, crate::pin::PinStore>,
) -> Result<crate::pin::PinMetadata, String> {
    store.metadata(&label)
}

#[tauri::command]
pub fn get_pinned_capture_image(
    label: String,
    store: State<'_, crate::pin::PinStore>,
) -> Result<tauri::ipc::Response, String> {
    Ok(tauri::ipc::Response::new(store.png(&label)?))
}

#[tauri::command]
pub fn warmup_pin_window<R: Runtime>(label: String, app: AppHandle<R>) -> Result<(), String> {
    crate::pin::warmup_pin_window(&app, &label)
}

#[tauri::command]
pub fn reveal_pin_window<R: Runtime>(label: String, app: AppHandle<R>) -> Result<(), String> {
    crate::pin::reveal_pin_window(&app, &label)
}

#[tauri::command]
pub fn set_pin_opacity<R: Runtime>(
    label: String,
    opacity: f64,
    app: AppHandle<R>,
) -> Result<(), String> {
    crate::pin::set_pin_opacity(&app, &label, opacity)
}

#[tauri::command]
pub fn close_pin<R: Runtime>(label: String, app: AppHandle<R>) -> Result<(), String> {
    crate::pin::close_pin(&app, &label)
}

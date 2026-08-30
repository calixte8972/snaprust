use tauri::{AppHandle, Manager, Runtime, State};

use crate::{
    annotation::Annotation,
    screenshot::{
        CapturePayload, CaptureSession, CopyPayload, FrameStyle, PhysicalSelectionRect,
        SelectionCropRect, SelectionPayload,
    },
};

#[tauri::command]
pub async fn show_capture_overlay<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::screenshot::begin_capture(&app))
        .await
        .map_err(|error| format!("capture worker failed: {error}"))?
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
pub fn crop_selected_capture(
    crop: SelectionCropRect,
    session: State<'_, CaptureSession>,
) -> Result<SelectionPayload, String> {
    session.crop_selection(crop)
}

#[tauri::command]
pub async fn capture_scrolling_selection<R: Runtime>(
    app: AppHandle<R>,
) -> Result<crate::screenshot::ScrollCapturePayload, String> {
    crate::screenshot::capture_scrolling(app).await
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
pub fn set_capture_frame(
    style: FrameStyle,
    session: State<'_, CaptureSession>,
) -> Result<(), String> {
    session.set_frame_style(style)
}

#[tauri::command]
pub fn rotate_selected_capture(
    delta_quarters: i32,
    session: State<'_, CaptureSession>,
) -> Result<crate::screenshot::RotationPayload, String> {
    session.rotate_selection(delta_quarters)
}

#[tauri::command]
pub async fn recognize_selected_capture<R: Runtime>(
    app: AppHandle<R>,
    language: Option<String>,
) -> Result<crate::ocr::OcrPayload, String> {
    // Clone only the selected pixels while holding the capture lock, then do
    // PNG decoding and OCR on a blocking worker so the WebView stays responsive.
    let image = app.state::<CaptureSession>().raw_selection()?;
    tauri::async_runtime::spawn_blocking(move || crate::ocr::recognize(image, language))
        .await
        .map_err(|error| format!("OCR worker failed: {error}"))?
}

#[tauri::command]
pub async fn list_ocr_languages() -> Result<Vec<crate::ocr::OcrLanguagePayload>, String> {
    tauri::async_runtime::spawn_blocking(crate::ocr::available_languages)
        .await
        .map_err(|error| format!("OCR language worker failed: {error}"))?
}

#[tauri::command]
pub fn list_translation_models(
    provider: Option<String>,
    store: State<'_, crate::translation::TranslationConfigStore>,
) -> Result<Vec<crate::translation::TranslationModelPayload>, String> {
    let configured_provider = store.provider()?;
    let configured_model = store.model()?;
    let provider = provider.as_deref().unwrap_or(&configured_provider);
    let configured_model = (provider == configured_provider).then_some(configured_model.as_str());
    crate::translation::available_models(provider, configured_model)
}

#[tauri::command]
pub fn list_translation_providers() -> Vec<crate::translation::TranslationProviderPayload> {
    crate::translation::available_providers()
}

#[tauri::command]
pub fn get_translation_config(
    store: State<'_, crate::translation::TranslationConfigStore>,
) -> Result<crate::translation::TranslationConfigPayload, String> {
    store.payload()
}

#[tauri::command]
pub fn save_translation_config(
    config: crate::translation::TranslationConfigInput,
    store: State<'_, crate::translation::TranslationConfigStore>,
) -> Result<crate::translation::TranslationConfigPayload, String> {
    store.save(config)
}

#[tauri::command]
pub async fn translate_text(
    text: String,
    target_language: String,
    source_language: Option<String>,
    model: Option<String>,
    store: State<'_, crate::translation::TranslationConfigStore>,
    request_store: State<'_, crate::translation::TranslationRequestStore>,
    request_id: Option<u64>,
) -> Result<crate::translation::TranslationPayload, String> {
    let config = store.snapshot()?;
    let result = if let Some(request_id) = request_id {
        let cancellation = request_store.begin(request_id)?;
        match futures_util::future::select(
            Box::pin(crate::translation::translate(
                text,
                target_language,
                source_language,
                model,
                config,
            )),
            Box::pin(cancellation.notified()),
        )
        .await
        {
            futures_util::future::Either::Left((result, _)) => result,
            futures_util::future::Either::Right(_) => Err("翻译已取消".to_owned()),
        }
    } else {
        crate::translation::translate(text, target_language, source_language, model, config).await
    };
    if let Some(request_id) = request_id {
        let cancelled = request_store.is_cancelled(request_id)?;
        request_store.finish(request_id)?;
        if cancelled {
            return Err("翻译已取消".to_owned());
        }
    }
    result
}

#[tauri::command]
pub fn cancel_translation(
    request_id: u64,
    request_store: State<'_, crate::translation::TranslationRequestStore>,
) -> Result<(), String> {
    request_store.cancel(request_id)
}

#[tauri::command]
pub fn copy_text(text: String) -> Result<(), String> {
    crate::clipboard::write_text(&text)
}

#[tauri::command]
pub async fn copy_selected_capture<R: Runtime>(
    app: AppHandle<R>,
    ocr_text: Option<String>,
) -> Result<CopyPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::screenshot::copy_selected_capture(&app, ocr_text.as_deref())
    })
    .await
    .map_err(|error| format!("copy worker failed: {error}"))?
}

#[tauri::command]
pub async fn pin_selected_capture<R: Runtime>(
    app: AppHandle<R>,
    ocr_text: Option<String>,
) -> Result<crate::pin::PinCreatedPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::screenshot::pin_selected_capture(&app, ocr_text.as_deref())
    })
    .await
    .map_err(|error| format!("pin worker failed: {error}"))?
}

#[tauri::command]
pub async fn list_history<R: Runtime>(
    query: Option<String>,
    favorites_only: bool,
    app: AppHandle<R>,
) -> Result<Vec<crate::history::HistoryItemPayload>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::history::HistoryStore>()
            .list(query.as_deref(), favorites_only)
    })
    .await
    .map_err(|error| format!("history list worker failed: {error}"))?
}

#[tauri::command]
pub async fn get_history_usage<R: Runtime>(
    app: AppHandle<R>,
) -> Result<crate::history::HistoryUsagePayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::history::HistoryStore>().usage()
    })
    .await
    .map_err(|error| format!("history usage worker failed: {error}"))?
}

#[tauri::command]
pub async fn get_history_thumbnail<R: Runtime>(
    id: i64,
    app: AppHandle<R>,
) -> Result<tauri::ipc::Response, String> {
    let png = tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::history::HistoryStore>()
            .thumbnail_png(id)
    })
    .await
    .map_err(|error| format!("history thumbnail worker failed: {error}"))??;
    Ok(tauri::ipc::Response::new(png))
}

#[tauri::command]
pub async fn copy_history_capture<R: Runtime>(id: i64, app: AppHandle<R>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::history::HistoryStore>().copy(id)
    })
    .await
    .map_err(|error| format!("history copy worker failed: {error}"))?
}

#[tauri::command]
pub async fn pin_history_capture<R: Runtime>(
    id: i64,
    app: AppHandle<R>,
) -> Result<crate::pin::PinCreatedPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let image = app.state::<crate::history::HistoryStore>().image(id)?;
        crate::pin::create_pin(&app, image)
    })
    .await
    .map_err(|error| format!("history pin worker failed: {error}"))?
}

#[tauri::command]
pub fn set_history_favorite(
    id: i64,
    favorite: bool,
    store: State<'_, crate::history::HistoryStore>,
) -> Result<(), String> {
    store.set_favorite(id, favorite)
}

#[tauri::command]
pub fn set_history_tags(
    id: i64,
    tags: Vec<String>,
    store: State<'_, crate::history::HistoryStore>,
) -> Result<(), String> {
    store.set_tags(id, tags)
}

#[tauri::command]
pub fn set_history_favorite_batch(
    ids: Vec<i64>,
    favorite: bool,
    store: State<'_, crate::history::HistoryStore>,
) -> Result<(), String> {
    store.set_favorite_batch(ids, favorite)
}

#[tauri::command]
pub async fn export_history_captures<R: Runtime>(
    ids: Vec<i64>,
    app: AppHandle<R>,
) -> Result<crate::history::HistoryExportPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::history::HistoryStore>()
            .export(&app, ids)
    })
    .await
    .map_err(|error| format!("history export worker failed: {error}"))?
}

#[tauri::command]
pub async fn delete_history_capture<R: Runtime>(id: i64, app: AppHandle<R>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::history::HistoryStore>().delete(id)
    })
    .await
    .map_err(|error| format!("history delete worker failed: {error}"))?
}

#[tauri::command]
pub async fn delete_history_captures<R: Runtime>(
    ids: Vec<i64>,
    app: AppHandle<R>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::history::HistoryStore>()
            .delete_batch(ids)
    })
    .await
    .map_err(|error| format!("history batch delete worker failed: {error}"))?
}

#[tauri::command]
pub fn hide_history_window<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    crate::history::hide_history_window(&app)
}

#[tauri::command]
pub fn hide_settings_window<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    crate::window::hide_settings_window(&app)
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
pub fn set_pin_window_geometry<R: Runtime>(
    label: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    app: AppHandle<R>,
) -> Result<(), String> {
    crate::pin::set_pin_window_geometry(&app, &label, x, y, width, height)
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

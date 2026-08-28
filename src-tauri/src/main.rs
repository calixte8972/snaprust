#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;

mod annotation;
mod clipboard;
mod commands;
mod history;
mod hotkey;
mod ocr;
mod pin;
mod screenshot;
mod translation;
mod tray;
mod window;

fn main() {
    tauri::Builder::default()
        .manage(screenshot::CaptureSession::default())
        .manage(pin::PinStore::default())
        .manage(translation::TranslationRequestStore::default())
        .plugin(hotkey::plugin())
        .setup(|app| {
            app.manage(history::HistoryStore::open(app.handle())?);
            app.manage(translation::TranslationConfigStore::open(app.handle())?);
            hotkey::register(app.handle())?;
            tray::install(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::show_capture_overlay,
            commands::reveal_capture_overlay,
            commands::cancel_capture,
            commands::get_current_capture,
            commands::get_current_capture_image,
            commands::select_capture_region,
            commands::crop_selected_capture,
            commands::get_selected_capture_image,
            commands::set_capture_annotations,
            commands::set_capture_frame,
            commands::rotate_selected_capture,
            commands::recognize_selected_capture,
            commands::list_ocr_languages,
            commands::list_translation_providers,
            commands::list_translation_models,
            commands::get_translation_config,
            commands::save_translation_config,
            commands::translate_text,
            commands::cancel_translation,
            commands::list_history,
            commands::get_history_usage,
            commands::get_history_thumbnail,
            commands::copy_history_capture,
            commands::pin_history_capture,
            commands::set_history_favorite,
            commands::set_history_tags,
            commands::set_history_favorite_batch,
            commands::export_history_captures,
            commands::delete_history_capture,
            commands::delete_history_captures,
            commands::hide_history_window,
            commands::copy_text,
            commands::copy_selected_capture,
            commands::pin_selected_capture,
            commands::get_pinned_capture,
            commands::get_pinned_capture_image,
            commands::warmup_pin_window,
            commands::reveal_pin_window,
            commands::set_pin_window_geometry,
            commands::set_pin_opacity,
            commands::close_pin
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SnapRust");
}

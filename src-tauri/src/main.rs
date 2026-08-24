#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod annotation;
mod clipboard;
mod commands;
mod hotkey;
mod pin;
mod screenshot;
mod window;

fn main() {
    tauri::Builder::default()
        .manage(screenshot::CaptureSession::default())
        .manage(pin::PinStore::default())
        .plugin(hotkey::plugin())
        .setup(|app| {
            hotkey::register(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::show_capture_overlay,
            commands::reveal_capture_overlay,
            commands::cancel_capture,
            commands::get_current_capture,
            commands::get_current_capture_image,
            commands::select_capture_region,
            commands::get_selected_capture_image,
            commands::set_capture_annotations,
            commands::copy_selected_capture,
            commands::pin_selected_capture,
            commands::get_pinned_capture,
            commands::get_pinned_capture_image,
            commands::warmup_pin_window,
            commands::reveal_pin_window,
            commands::set_pin_opacity,
            commands::close_pin
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SnapRust");
}

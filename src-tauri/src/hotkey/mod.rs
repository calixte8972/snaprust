use std::error::Error;

use tauri::{AppHandle, Runtime, plugin::TauriPlugin};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn capture_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyA)
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let capture_shortcut = capture_shortcut();

    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, shortcut, event| {
            if shortcut == &capture_shortcut
                && event.state() == ShortcutState::Pressed
                && let Err(error) = crate::screenshot::begin_capture(app)
            {
                eprintln!("failed to enter capture mode: {error}");
            }
        })
        .build()
}

pub fn register<R: Runtime>(app: &AppHandle<R>) -> Result<(), Box<dyn Error>> {
    app.global_shortcut().register(capture_shortcut())?;
    Ok(())
}

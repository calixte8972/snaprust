use std::error::Error;

use tauri::{AppHandle, Runtime, plugin::TauriPlugin};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn capture_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyA)
}

fn history_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::ALT), Code::KeyH)
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let capture_shortcut = capture_shortcut();
    let history_shortcut = history_shortcut();

    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, shortcut, event| {
            if shortcut == &capture_shortcut && event.state() == ShortcutState::Pressed {
                let capture_app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(error) = crate::screenshot::begin_capture(&capture_app) {
                        eprintln!("failed to enter capture mode: {error}");
                    }
                });
            } else if shortcut == &history_shortcut
                && event.state() == ShortcutState::Pressed
                && let Err(error) = crate::screenshot::cancel_capture_for_auxiliary_window(app)
                    .and_then(|()| crate::history::show_history_window(app))
            {
                eprintln!("failed to open history: {error}");
            }
        })
        .build()
}

pub fn register<R: Runtime>(app: &AppHandle<R>) -> Result<(), Box<dyn Error>> {
    app.global_shortcut().register(capture_shortcut())?;
    app.global_shortcut().register(history_shortcut())?;
    Ok(())
}

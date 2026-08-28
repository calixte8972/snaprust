use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use image::RgbaImage;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

#[cfg(not(windows))]
use tauri::{WebviewUrl, WebviewWindowBuilder, WindowEvent};

#[cfg(windows)]
mod native;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinCreatedPayload {
    label: String,
    width: u32,
    height: u32,
    render_ms: f64,
    png_encode_ms: f64,
    window_create_ms: f64,
    total_ms: f64,
}

impl PinCreatedPayload {
    pub(crate) fn set_pipeline_performance(&mut self, render_ms: f64, total_ms: f64) {
        self.render_ms = render_ms;
        self.total_ms = total_ms;
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinMetadata {
    width: u32,
    height: u32,
}

struct PinnedImage {
    png: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Default)]
pub struct PinStore {
    next_id: AtomicU64,
    images: Arc<Mutex<HashMap<String, PinnedImage>>>,
}

#[cfg(windows)]
fn clamp_native_axis(
    desired: i32,
    window_extent: i32,
    work_start: i32,
    work_extent: i32,
    minimum_visible: i32,
) -> i32 {
    if window_extent <= work_extent {
        desired.clamp(work_start, work_start + work_extent - window_extent)
    } else {
        let visible = minimum_visible.min(window_extent).min(work_extent).max(1);
        desired.clamp(
            work_start + visible - window_extent,
            work_start + work_extent - visible,
        )
    }
}

impl PinStore {
    #[cfg(any(not(windows), test))]
    fn insert(&self, image: &RgbaImage) -> Result<PinCreatedPayload, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let label = format!("pin-{id}");
        let encode_started = Instant::now();
        let png = crate::screenshot::encode_png(image)?;
        let png_encode_ms = crate::screenshot::elapsed_ms(encode_started);
        let payload = PinnedImage {
            png,
            width: image.width(),
            height: image.height(),
        };
        self.images
            .lock()
            .map_err(|_| "pin store lock is poisoned".to_owned())?
            .insert(label.clone(), payload);
        Ok(PinCreatedPayload {
            label,
            width: image.width(),
            height: image.height(),
            render_ms: 0.0,
            png_encode_ms,
            window_create_ms: 0.0,
            total_ms: 0.0,
        })
    }

    #[cfg(windows)]
    fn insert_native(&self, image: &RgbaImage) -> Result<PinCreatedPayload, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let label = format!("pin-{id}");
        self.images
            .lock()
            .map_err(|_| "pin store lock is poisoned".to_owned())?
            .insert(
                label.clone(),
                PinnedImage {
                    png: Vec::new(),
                    width: image.width(),
                    height: image.height(),
                },
            );
        Ok(PinCreatedPayload {
            label,
            width: image.width(),
            height: image.height(),
            render_ms: 0.0,
            png_encode_ms: 0.0,
            window_create_ms: 0.0,
            total_ms: 0.0,
        })
    }

    pub fn metadata(&self, label: &str) -> Result<PinMetadata, String> {
        let images = self
            .images
            .lock()
            .map_err(|_| "pin store lock is poisoned".to_owned())?;
        let image = images
            .get(label)
            .ok_or_else(|| format!("pinned capture does not exist: {label}"))?;
        Ok(PinMetadata {
            width: image.width,
            height: image.height,
        })
    }

    pub fn png(&self, label: &str) -> Result<Vec<u8>, String> {
        self.images
            .lock()
            .map_err(|_| "pin store lock is poisoned".to_owned())?
            .get(label)
            .map(|image| image.png.clone())
            .ok_or_else(|| format!("pinned capture does not exist: {label}"))
    }

    fn contains(&self, label: &str) -> Result<bool, String> {
        Ok(self
            .images
            .lock()
            .map_err(|_| "pin store lock is poisoned".to_owned())?
            .contains_key(label))
    }

    fn remove(&self, label: &str) -> Result<(), String> {
        self.images
            .lock()
            .map_err(|_| "pin store lock is poisoned".to_owned())?
            .remove(label);
        Ok(())
    }
}

pub fn create_pin<R: Runtime>(
    app: &AppHandle<R>,
    image: RgbaImage,
) -> Result<PinCreatedPayload, String> {
    let store = app.state::<PinStore>();
    #[cfg(windows)]
    let mut payload = store.insert_native(&image)?;
    #[cfg(not(windows))]
    let mut payload = store.insert(&image)?;
    let label = payload.label.clone();
    let scale = (960.0 / f64::from(payload.width))
        .min(720.0 / f64::from(payload.height))
        .min(1.0);
    let initial_width = (f64::from(payload.width) * scale).max(1.0);
    let initial_height = (f64::from(payload.height) * scale).max(1.0);

    #[cfg(windows)]
    {
        let window_started = Instant::now();
        let result = native::create(
            label.clone(),
            image,
            store.images.clone(),
            initial_width.round() as i32,
            initial_height.round() as i32,
        );
        if let Err(error) = result {
            let _ = store.remove(&label);
            return Err(error);
        }
        payload.window_create_ms = crate::screenshot::elapsed_ms(window_started);
        Ok(payload)
    }

    #[cfg(not(windows))]
    {
        let window_started = Instant::now();
        let build_result =
            WebviewWindowBuilder::new(app, &label, WebviewUrl::App("pin.html".into()))
                .title("SnapRust 钉图")
                .inner_size(initial_width, initial_height)
                .visible(false)
                .transparent(true)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .shadow(false)
                .center()
                .build();

        let window = match build_result {
            Ok(window) => window,
            Err(error) => {
                let _ = store.remove(&label);
                return Err(format!("failed to create pin window: {error}"));
            }
        };
        if let Err(error) = apply_window_opacity(&window, 0.0) {
            let _ = window.close();
            let _ = store.remove(&label);
            return Err(error);
        }
        payload.window_create_ms = crate::screenshot::elapsed_ms(window_started);

        let cleanup_app = app.clone();
        let cleanup_label = label;
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                let _ = cleanup_app.state::<PinStore>().remove(&cleanup_label);
            }
        });

        Ok(payload)
    }
}

fn pin_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> Result<tauri::WebviewWindow<R>, String> {
    if !app.state::<PinStore>().contains(label)? {
        return Err(format!("pinned capture does not exist: {label}"));
    }
    app.get_webview_window(label)
        .ok_or_else(|| format!("pin window does not exist: {label}"))
}

fn target_outer_extent(
    target_inner: u32,
    current_inner: u32,
    current_outer: u32,
) -> Result<i32, String> {
    target_inner
        .checked_add(current_outer.saturating_sub(current_inner))
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| "pin window extent is too large".to_owned())
}

pub fn warmup_pin_window<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<(), String> {
    let window = pin_window(app, label)?;
    window
        .show()
        .map_err(|error| format!("failed to warm up pin window: {error}"))
}

pub fn reveal_pin_window<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<(), String> {
    let window = pin_window(app, label)?;
    window
        .set_shadow(true)
        .map_err(|error| format!("failed to enable pin window shadow: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("failed to focus pin window: {error}"))?;
    apply_window_opacity(&window, 1.0)
}

pub fn set_pin_window_geometry<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("pin window size must be non-zero".to_owned());
    }
    let window = pin_window(app, label)?;

    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::RECT,
            UI::WindowsAndMessaging::{
                GetClientRect, GetWindowRect, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
            },
        };

        let hwnd = window
            .hwnd()
            .map_err(|error| format!("failed to get pin HWND: {error}"))?;
        let mut inner = RECT::default();
        let mut outer = RECT::default();

        unsafe {
            GetClientRect(hwnd, &mut inner)
                .map_err(|error| format!("failed to read pin client rectangle: {error}"))?;
            GetWindowRect(hwnd, &mut outer)
                .map_err(|error| format!("failed to read pin window rectangle: {error}"))?;
            let inner_width = u32::try_from(inner.right.saturating_sub(inner.left))
                .map_err(|_| "pin client width is invalid".to_owned())?;
            let inner_height = u32::try_from(inner.bottom.saturating_sub(inner.top))
                .map_err(|_| "pin client height is invalid".to_owned())?;
            let outer_width = u32::try_from(outer.right.saturating_sub(outer.left))
                .map_err(|_| "pin window width is invalid".to_owned())?;
            let outer_height = u32::try_from(outer.bottom.saturating_sub(outer.top))
                .map_err(|_| "pin window height is invalid".to_owned())?;
            let target_outer_width = target_outer_extent(width, inner_width, outer_width)?;
            let target_outer_height = target_outer_extent(height, inner_height, outer_height)?;
            SetWindowPos(
                hwnd,
                None,
                x,
                y,
                target_outer_width,
                target_outer_height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(|error| format!("failed to update pin window geometry: {error}"))?;
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        use tauri::{PhysicalPosition, PhysicalSize};

        window
            .set_size(PhysicalSize::new(width, height))
            .map_err(|error| format!("failed to resize pin window: {error}"))?;
        window
            .set_position(PhysicalPosition::new(x, y))
            .map_err(|error| format!("failed to reposition pin window: {error}"))
    }
}

pub fn close_pin<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        if !app.state::<PinStore>().contains(label)? {
            return Err(format!("pinned capture does not exist: {label}"));
        }
        native::close(label)
    }

    #[cfg(not(windows))]
    {
        let window = pin_window(app, label)?;
        window
            .close()
            .map_err(|error| format!("failed to close pin window: {error}"))?;
        app.state::<PinStore>().remove(label)
    }
}

#[cfg(windows)]
fn apply_window_opacity<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    opacity: f64,
) -> Result<(), String> {
    use windows::Win32::{
        Foundation::COLORREF,
        UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes,
            SetWindowLongPtrW, WS_EX_LAYERED,
        },
    };

    let hwnd = window
        .hwnd()
        .map_err(|error| format!("failed to get pin HWND: {error}"))?;
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;

    unsafe {
        let extended_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, extended_style | WS_EX_LAYERED.0 as isize);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA)
            .map_err(|error| format!("failed to set pin opacity: {error}"))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn apply_window_opacity<R: Runtime>(
    _window: &tauri::WebviewWindow<R>,
    _opacity: f64,
) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub fn set_pin_opacity<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    opacity: f64,
) -> Result<(), String> {
    if !opacity.is_finite() || !(0.2..=1.0).contains(&opacity) {
        return Err("pin opacity must be between 0.2 and 1.0".to_owned());
    }
    if !app.state::<PinStore>().contains(label)? {
        return Err(format!("pinned capture does not exist: {label}"));
    }
    apply_window_opacity(&pin_window(app, label)?, opacity)
}

#[cfg(not(windows))]
pub fn set_pin_opacity<R: Runtime>(
    _app: &AppHandle<R>,
    _label: &str,
    _opacity: f64,
) -> Result<(), String> {
    Err("pin opacity is currently supported on Windows only".to_owned())
}

#[cfg(test)]
mod tests {
    use image::{GenericImageView, RgbaImage};

    use super::PinStore;

    #[test]
    fn converts_target_inner_extent_to_the_native_outer_extent() {
        assert_eq!(super::target_outer_extent(376, 376, 378).unwrap(), 378);
        assert_eq!(super::target_outer_extent(250, 250, 252).unwrap(), 252);
        assert_eq!(super::target_outer_extent(100, 101, 100).unwrap(), 100);
        assert!(super::target_outer_extent(u32::MAX, 1, 2).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn keeps_native_zoomed_windows_reachable_inside_the_work_area() {
        assert_eq!(super::clamp_native_axis(-50, 500, 0, 1920, 64), 0);
        assert_eq!(super::clamp_native_axis(1600, 500, 0, 1920, 64), 1420);
        assert_eq!(super::clamp_native_axis(-3000, 2400, 0, 1920, 64), -2336);
        assert_eq!(super::clamp_native_axis(3000, 2400, 0, 1920, 64), 1856);
    }

    #[test]
    fn stores_and_removes_pinned_images() {
        let store = PinStore::default();
        let image = RgbaImage::new(320, 180);
        let payload = store.insert(&image).unwrap();

        let stored = store.metadata(&payload.label).unwrap();
        assert_eq!((stored.width, stored.height), (320, 180));
        let png = store.png(&payload.label).unwrap();
        assert!(png.starts_with(&[137, 80, 78, 71]));
        assert_eq!(
            image::load_from_memory(&png).unwrap().dimensions(),
            (320, 180)
        );

        let created_json = serde_json::to_string(&payload).unwrap();
        assert!(!created_json.contains("imageDataUrl"));
        assert!(created_json.contains("pngEncodeMs"));
        assert!(created_json.contains("windowCreateMs"));
        assert!(created_json.len() < 240);

        store.remove(&payload.label).unwrap();
        assert!(store.metadata(&payload.label).is_err());
    }
}

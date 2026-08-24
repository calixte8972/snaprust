//! Screen discovery, capture, and image cropping live in this module.

mod capture;
mod monitor;

pub(crate) use self::monitor::VirtualDesktop;

use std::{sync::Mutex, time::Instant};

use image::{
    ImageEncoder, RgbaImage,
    codecs::png::{CompressionType, FilterType, PngEncoder},
    imageops::crop_imm,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::annotation::Annotation;

pub struct CapturedScreen {
    desktop: VirtualDesktop,
    image: Option<RgbaImage>,
    selection: Option<RgbaImage>,
    annotations: Vec<Annotation>,
    capture_ms: f64,
}

#[derive(Default)]
pub struct CaptureSession {
    current: Mutex<Option<CapturedScreen>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePayload {
    width: u32,
    height: u32,
    desktop: VirtualDesktop,
    capture_ms: f64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalSelectionRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPayload {
    width: u32,
    height: u32,
    crop_ms: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyPayload {
    width: u32,
    height: u32,
    render_ms: f64,
    clipboard_ms: f64,
    total_ms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PixelRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl CaptureSession {
    fn replace(&self, capture: CapturedScreen) -> Result<(), String> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        *current = Some(capture);
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        *current = None;
        Ok(())
    }

    pub fn is_active(&self) -> Result<bool, String> {
        Ok(self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?
            .is_some())
    }

    pub fn payload(&self) -> Result<CapturePayload, String> {
        let current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        let capture = current
            .as_ref()
            .ok_or_else(|| "there is no active screen capture".to_owned())?;
        let image = capture
            .image
            .as_ref()
            .ok_or_else(|| "the full screen image was released after selection".to_owned())?;

        Ok(CapturePayload {
            width: image.width(),
            height: image.height(),
            desktop: capture.desktop.clone(),
            capture_ms: capture.capture_ms,
        })
    }

    pub fn capture_png(&self) -> Result<Vec<u8>, String> {
        let current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        let capture = current
            .as_ref()
            .ok_or_else(|| "there is no active screen capture".to_owned())?;
        let image = capture
            .image
            .as_ref()
            .ok_or_else(|| "the full screen image was released after selection".to_owned())?;
        encode_png(image)
    }

    pub fn select(&self, selection: PhysicalSelectionRect) -> Result<SelectionPayload, String> {
        let started = Instant::now();
        let mut current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        let capture = current
            .as_mut()
            .ok_or_else(|| "there is no active screen capture".to_owned())?;
        let image = capture
            .image
            .as_ref()
            .ok_or_else(|| "the full screen image was released after selection".to_owned())?;
        let rect = selection.to_pixel_rect(&capture.desktop, image.width(), image.height())?;
        let selected = crop_imm(image, rect.x, rect.y, rect.width, rect.height).to_image();
        let payload = SelectionPayload {
            width: selected.width(),
            height: selected.height(),
            crop_ms: elapsed_ms(started),
        };

        capture.selection = Some(selected);
        capture.image = None;
        capture.annotations.clear();
        Ok(payload)
    }

    pub fn selected_png(&self) -> Result<Vec<u8>, String> {
        let current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        let capture = current
            .as_ref()
            .ok_or_else(|| "there is no active screen capture".to_owned())?;
        let selection = capture
            .selection
            .as_ref()
            .ok_or_else(|| "select an area before opening the annotation editor".to_owned())?;
        encode_png(selection)
    }

    pub fn set_annotations(&self, mut annotations: Vec<Annotation>) -> Result<(), String> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        let capture = current
            .as_mut()
            .ok_or_else(|| "there is no active screen capture".to_owned())?;
        let selection = capture
            .selection
            .as_ref()
            .ok_or_else(|| "select an area before applying annotations".to_owned())?;

        crate::annotation::validate_annotations(
            &annotations,
            selection.width(),
            selection.height(),
        )?;
        for annotation in &mut annotations {
            annotation.simplify_brush_path();
        }
        capture.annotations = annotations;
        Ok(())
    }

    pub(crate) fn rendered_selection(&self) -> Result<RgbaImage, String> {
        let (mut selection, annotations) = {
            let current = self
                .current
                .lock()
                .map_err(|_| "capture session lock is poisoned".to_owned())?;
            let capture = current
                .as_ref()
                .ok_or_else(|| "there is no active screen capture".to_owned())?;
            let selection = capture
                .selection
                .as_ref()
                .ok_or_else(|| "select an area before copying".to_owned())?;
            (selection.clone(), capture.annotations.clone())
        };

        crate::annotation::render_annotations(&mut selection, &annotations)?;
        Ok(selection)
    }

    fn copy_selection(&self) -> Result<CopyPayload, String> {
        let total_started = Instant::now();
        let render_started = Instant::now();
        let selection = self.rendered_selection()?;
        let render_ms = elapsed_ms(render_started);
        let clipboard_started = Instant::now();
        crate::clipboard::write_image(&selection)?;
        let clipboard_ms = elapsed_ms(clipboard_started);
        Ok(CopyPayload {
            width: selection.width(),
            height: selection.height(),
            render_ms,
            clipboard_ms,
            total_ms: elapsed_ms(total_started),
        })
    }
}

pub(crate) fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("failed to encode screen capture: {error}"))?;
    Ok(png)
}

pub(crate) fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

impl PhysicalSelectionRect {
    fn to_pixel_rect(
        self,
        desktop: &VirtualDesktop,
        image_width: u32,
        image_height: u32,
    ) -> Result<PixelRect, String> {
        if self.width == 0 || self.height == 0 {
            return Err("selection must have a non-zero width and height".to_owned());
        }
        let desktop_left = i64::from(desktop.x);
        let desktop_top = i64::from(desktop.y);
        let desktop_right = desktop_left + i64::from(desktop.width);
        let desktop_bottom = desktop_top + i64::from(desktop.height);
        let left = i64::from(self.x).clamp(desktop_left, desktop_right);
        let top = i64::from(self.y).clamp(desktop_top, desktop_bottom);
        let right = (i64::from(self.x) + i64::from(self.width)).clamp(desktop_left, desktop_right);
        let bottom =
            (i64::from(self.y) + i64::from(self.height)).clamp(desktop_top, desktop_bottom);
        if right <= left || bottom <= top {
            return Err("selection is outside the virtual desktop".to_owned());
        }

        let x = u32::try_from(left - desktop_left)
            .map_err(|_| "selection x coordinate overflowed".to_owned())?
            .min(image_width);
        let y = u32::try_from(top - desktop_top)
            .map_err(|_| "selection y coordinate overflowed".to_owned())?
            .min(image_height);
        let right = u32::try_from(right - desktop_left)
            .map_err(|_| "selection right coordinate overflowed".to_owned())?
            .min(image_width);
        let bottom = u32::try_from(bottom - desktop_top)
            .map_err(|_| "selection bottom coordinate overflowed".to_owned())?
            .min(image_height);

        if right <= x || bottom <= y {
            return Err("selection maps to an empty image region".to_owned());
        }

        Ok(PixelRect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

pub fn begin_capture<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let session = app.state::<CaptureSession>();
    if session.is_active()? || crate::window::is_capture_overlay_visible(app)? {
        return Ok(());
    }

    let desktop = monitor::virtual_desktop()?;
    let capture_started = Instant::now();
    let image = capture::capture_virtual_desktop(&desktop)?;
    let capture_ms = elapsed_ms(capture_started);
    session.replace(CapturedScreen {
        desktop: desktop.clone(),
        image: Some(image),
        selection: None,
        annotations: Vec::new(),
        capture_ms,
    })?;

    if let Err(error) = crate::window::prepare_capture_overlay(app, &desktop) {
        let _ = session.clear();
        return Err(error);
    }

    Ok(())
}

pub fn cancel_capture<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    crate::window::hide_capture_overlay(app)?;
    app.state::<CaptureSession>().clear()
}

pub fn copy_selected_capture<R: Runtime>(app: &AppHandle<R>) -> Result<CopyPayload, String> {
    let session = app.state::<CaptureSession>();
    let result = session.copy_selection()?;
    crate::window::hide_capture_overlay(app)?;
    session.clear()?;
    Ok(result)
}

pub fn pin_selected_capture<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<crate::pin::PinCreatedPayload, String> {
    let total_started = Instant::now();
    let session = app.state::<CaptureSession>();
    let render_started = Instant::now();
    let image = session.rendered_selection()?;
    let render_ms = elapsed_ms(render_started);
    let mut result = crate::pin::create_pin(app, image)?;
    result.set_pipeline_performance(render_ms, elapsed_ms(total_started));
    crate::window::hide_capture_overlay(app)?;
    session.clear()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use image::{GenericImageView, Rgba, RgbaImage, imageops::crop_imm};

    use super::{
        CaptureSession, CapturedScreen, PhysicalSelectionRect, VirtualDesktop, monitor::MonitorInfo,
    };

    fn desktop(x: i32, y: i32, width: u32, height: u32) -> VirtualDesktop {
        VirtualDesktop {
            x,
            y,
            width,
            height,
            monitors: Vec::new(),
        }
    }

    #[test]
    fn exposes_capture_images_as_binary_png_without_embedding_them_in_metadata() {
        let session = CaptureSession::default();
        assert!(!session.is_active().unwrap());
        session
            .replace(CapturedScreen {
                desktop: VirtualDesktop {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 6,
                    monitors: Vec::new(),
                },
                image: Some(RgbaImage::from_pixel(8, 6, Rgba([10, 20, 30, 255]))),
                selection: None,
                annotations: Vec::new(),
                capture_ms: 0.0,
            })
            .unwrap();
        assert!(session.is_active().unwrap());

        let metadata = serde_json::to_string(&session.payload().unwrap()).unwrap();
        assert!(!metadata.contains("imageDataUrl"));
        assert!(metadata.len() < 160);

        let capture_png = session.capture_png().unwrap();
        assert!(capture_png.starts_with(&[137, 80, 78, 71]));
        assert_eq!(
            image::load_from_memory(&capture_png).unwrap().dimensions(),
            (8, 6)
        );

        session
            .select(PhysicalSelectionRect {
                x: 2,
                y: 1,
                width: 3,
                height: 4,
            })
            .unwrap();
        assert!(session.capture_png().is_err());
        assert_eq!(
            image::load_from_memory(&session.selected_png().unwrap())
                .unwrap()
                .dimensions(),
            (3, 4)
        );

        session.clear().unwrap();
        assert!(!session.is_active().unwrap());
    }

    #[test]
    fn maps_absolute_pixels_across_mixed_dpi_monitors_with_a_negative_origin() {
        let desktop = VirtualDesktop {
            x: -1920,
            y: 0,
            width: 4480,
            height: 1440,
            monitors: vec![
                MonitorInfo {
                    index: 1,
                    x: -1920,
                    y: 0,
                    width: 1920,
                    height: 1080,
                    dpi_x: 96,
                    dpi_y: 96,
                    scale_factor: 1.0,
                    is_primary: false,
                },
                MonitorInfo {
                    index: 2,
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                    dpi_x: 144,
                    dpi_y: 144,
                    scale_factor: 1.5,
                    is_primary: true,
                },
            ],
        };
        let rect = PhysicalSelectionRect {
            x: -960,
            y: 100,
            width: 2240,
            height: 800,
        };

        assert_eq!(
            rect.to_pixel_rect(&desktop, 4480, 1440).unwrap(),
            super::PixelRect {
                x: 960,
                y: 100,
                width: 2240,
                height: 800,
            }
        );
    }

    #[test]
    fn clamps_absolute_selection_to_the_virtual_desktop() {
        let desktop = desktop(-100, -20, 1000, 500);
        let rect = PhysicalSelectionRect {
            x: -200,
            y: -40,
            width: 250,
            height: 100,
        };

        assert_eq!(
            rect.to_pixel_rect(&desktop, 1000, 500).unwrap(),
            super::PixelRect {
                x: 0,
                y: 0,
                width: 150,
                height: 80,
            }
        );
    }

    #[test]
    fn rejects_empty_or_outside_absolute_selection() {
        let desktop = desktop(0, 0, 100, 100);
        let empty = PhysicalSelectionRect {
            x: 1,
            y: 1,
            width: 0,
            height: 10,
        };
        let outside = PhysicalSelectionRect {
            x: 200,
            y: 200,
            width: 10,
            height: 10,
        };

        assert!(empty.to_pixel_rect(&desktop, 100, 100).is_err());
        assert!(outside.to_pixel_rect(&desktop, 100, 100).is_err());
    }

    #[test]
    fn crop_dimensions_match_the_mapped_selection() {
        let image = RgbaImage::new(1_250, 625);
        let desktop = desktop(0, 0, image.width(), image.height());
        let rect = PhysicalSelectionRect {
            x: 0,
            y: 0,
            width: 1_250,
            height: 625,
        }
        .to_pixel_rect(&desktop, image.width(), image.height())
        .unwrap();
        let crop = crop_imm(&image, rect.x, rect.y, rect.width, rect.height).to_image();

        assert_eq!(crop.dimensions(), (1_250, 625));
    }

    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn captures_virtual_desktop_with_expected_dimensions() {
        let desktop = super::monitor::virtual_desktop().expect("virtual desktop should exist");
        let image = super::capture::capture_virtual_desktop(&desktop)
            .expect("virtual desktop capture should succeed");

        assert_eq!(image.dimensions(), (desktop.width, desktop.height));
    }
}

//! Screen discovery, capture, and image cropping live in this module.

mod capture;
mod monitor;

pub(crate) use self::monitor::VirtualDesktop;

use std::{sync::Mutex, time::Instant};

use image::{
    ImageEncoder, Rgba, RgbaImage,
    codecs::png::{CompressionType, FilterType, PngEncoder},
    imageops::{crop_imm, overlay, rotate90, rotate180, rotate270},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::annotation::Annotation;

pub struct CapturedScreen {
    desktop: VirtualDesktop,
    image: Option<RgbaImage>,
    selection: Option<RgbaImage>,
    annotations: Vec<Annotation>,
    rotation_quarters: u8,
    capture_ms: f64,
}

#[derive(Default)]
pub struct CaptureSession {
    current: Mutex<Option<CapturedScreen>>,
    frame_style: Mutex<FrameStyle>,
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

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionCropRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FrameStyle {
    #[default]
    None,
    Macos,
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
pub struct RotationPayload {
    quarters: u8,
    width: u32,
    height: u32,
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
        {
            let mut current = self
                .current
                .lock()
                .map_err(|_| "capture session lock is poisoned".to_owned())?;
            *current = Some(capture);
        }
        *self
            .frame_style
            .lock()
            .map_err(|_| "capture frame lock is poisoned".to_owned())? = FrameStyle::None;
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        {
            let mut current = self
                .current
                .lock()
                .map_err(|_| "capture session lock is poisoned".to_owned())?;
            *current = None;
        }
        *self
            .frame_style
            .lock()
            .map_err(|_| "capture frame lock is poisoned".to_owned())? = FrameStyle::None;
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
        capture.rotation_quarters = 0;
        *self
            .frame_style
            .lock()
            .map_err(|_| "capture frame lock is poisoned".to_owned())? = FrameStyle::None;
        Ok(payload)
    }

    pub fn crop_selection(&self, crop: SelectionCropRect) -> Result<SelectionPayload, String> {
        let started = Instant::now();
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
            .ok_or_else(|| "select an area before cropping".to_owned())?;
        if crop.width == 0 || crop.height == 0 {
            return Err("crop must have a non-zero width and height".to_owned());
        }
        let right = crop
            .x
            .checked_add(crop.width)
            .ok_or_else(|| "crop right coordinate overflowed".to_owned())?;
        let bottom = crop
            .y
            .checked_add(crop.height)
            .ok_or_else(|| "crop bottom coordinate overflowed".to_owned())?;
        if right > selection.width() || bottom > selection.height() {
            return Err("crop is outside the selected image".to_owned());
        }

        let cropped = crop_imm(selection, crop.x, crop.y, crop.width, crop.height).to_image();
        let payload = SelectionPayload {
            width: cropped.width(),
            height: cropped.height(),
            crop_ms: elapsed_ms(started),
        };
        capture.selection = Some(cropped);
        capture.annotations.clear();
        capture.rotation_quarters = 0;
        *self
            .frame_style
            .lock()
            .map_err(|_| "capture frame lock is poisoned".to_owned())? = FrameStyle::None;
        Ok(payload)
    }

    pub fn set_frame_style(&self, style: FrameStyle) -> Result<(), String> {
        let current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        let capture = current
            .as_ref()
            .ok_or_else(|| "there is no active screen capture".to_owned())?;
        if capture.selection.is_none() {
            return Err("select an area before adding an image frame".to_owned());
        }
        drop(current);

        *self
            .frame_style
            .lock()
            .map_err(|_| "capture frame lock is poisoned".to_owned())? = style;
        Ok(())
    }

    pub fn rotate_selection(&self, delta_quarters: i32) -> Result<RotationPayload, String> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        let capture = current
            .as_mut()
            .ok_or_else(|| "there is no active screen capture".to_owned())?;
        if capture.selection.is_none() {
            return Err("select an area before rotating".to_owned());
        }
        capture.rotation_quarters =
            (((i32::from(capture.rotation_quarters) + delta_quarters) % 4 + 4) % 4) as u8;
        let selection = capture.selection.as_ref().expect("checked above");
        let (width, height) = if capture.rotation_quarters % 2 == 0 {
            (selection.width(), selection.height())
        } else {
            (selection.height(), selection.width())
        };
        Ok(RotationPayload {
            quarters: capture.rotation_quarters,
            width,
            height,
        })
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
        let (mut selection, annotations, rotation_quarters, frame_style) = {
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
            let frame_style = *self
                .frame_style
                .lock()
                .map_err(|_| "capture frame lock is poisoned".to_owned())?;
            (
                selection.clone(),
                capture.annotations.clone(),
                capture.rotation_quarters,
                frame_style,
            )
        };

        crate::annotation::render_annotations(&mut selection, &annotations)?;
        selection = match rotation_quarters {
            0 => selection,
            1 => rotate90(&selection),
            2 => rotate180(&selection),
            3 => rotate270(&selection),
            _ => unreachable!("rotation is normalized to four quarters"),
        };
        apply_frame(selection, frame_style)
    }

    /// Returns the untouched selected pixels for OCR. Annotations are excluded
    /// deliberately so arrows, mosaic blocks, and labels cannot pollute text
    /// recognition.
    pub(crate) fn raw_selection(&self) -> Result<RgbaImage, String> {
        let current = self
            .current
            .lock()
            .map_err(|_| "capture session lock is poisoned".to_owned())?;
        let capture = current
            .as_ref()
            .ok_or_else(|| "there is no active screen capture".to_owned())?;
        capture
            .selection
            .clone()
            .ok_or_else(|| "select an area before recognizing text".to_owned())
    }
}

#[derive(Clone, Copy, Debug)]
struct MacosFrameMetrics {
    side: u32,
    top: u32,
    bottom: u32,
    dot_radius: u32,
    dot_first_x: u32,
    dot_step: u32,
    dot_y: u32,
}

fn macos_frame_metrics(image_width: u32, image_height: u32) -> MacosFrameMetrics {
    let area = f64::from(image_width.max(1)) * f64::from(image_height.max(1));
    let scale = (area / (1280.0 * 720.0)).sqrt().clamp(0.75, 2.0);
    let scaled = |value: f64| (value * scale).round().max(1.0) as u32;
    MacosFrameMetrics {
        side: scaled(12.0),
        top: scaled(38.0),
        bottom: scaled(12.0),
        dot_radius: scaled(6.0),
        dot_first_x: scaled(20.0),
        dot_step: scaled(16.0),
        dot_y: scaled(19.0),
    }
}

fn apply_frame(image: RgbaImage, style: FrameStyle) -> Result<RgbaImage, String> {
    match style {
        FrameStyle::None => Ok(image),
        FrameStyle::Macos => apply_macos_frame(image),
    }
}

fn apply_macos_frame(image: RgbaImage) -> Result<RgbaImage, String> {
    let metrics = macos_frame_metrics(image.width(), image.height());
    let window_width = image
        .width()
        .checked_add(metrics.side * 2)
        .ok_or_else(|| "macOS frame would make the image too wide".to_owned())?;
    let window_height = image
        .height()
        .checked_add(metrics.top + metrics.bottom)
        .ok_or_else(|| "macOS frame would make the image too tall".to_owned())?;
    let mut framed = RgbaImage::from_pixel(window_width, window_height, Rgba([27, 28, 31, 255]));
    fill_macos_header(&mut framed, metrics.top);

    overlay(
        &mut framed,
        &image,
        i64::from(metrics.side),
        i64::from(metrics.top),
    );
    let separator_y = metrics.top.saturating_sub(1);
    for x in 0..window_width {
        blend_opaque_pixel(&mut framed, x, separator_y, Rgba([12, 13, 15, 180]), 1.0);
    }
    for (index, color) in [
        Rgba([255, 95, 86, 255]),
        Rgba([255, 189, 46, 255]),
        Rgba([39, 201, 63, 255]),
    ]
    .into_iter()
    .enumerate()
    {
        draw_antialiased_circle(
            &mut framed,
            f64::from(metrics.dot_first_x + metrics.dot_step * index as u32),
            f64::from(metrics.dot_y),
            f64::from(metrics.dot_radius),
            color,
        );
    }

    Ok(framed)
}

fn fill_macos_header(image: &mut RgbaImage, height: u32) {
    let height = height.min(image.height());
    for y in 0..height {
        let progress = f64::from(y) / f64::from(height.max(1));
        let value = (43.0 - progress * 8.0).round() as u8;
        let color = Rgba([value, value, value.saturating_add(3), 255]);
        for x in 0..image.width() {
            *image.get_pixel_mut(x, y) = color;
        }
    }
}

fn draw_antialiased_circle(
    image: &mut RgbaImage,
    center_x: f64,
    center_y: f64,
    radius: f64,
    color: Rgba<u8>,
) {
    let left = (center_x - radius - 1.0).floor().max(0.0) as u32;
    let right = (center_x + radius + 1.0)
        .ceil()
        .min(f64::from(image.width())) as u32;
    let top = (center_y - radius - 1.0).floor().max(0.0) as u32;
    let bottom = (center_y + radius + 1.0)
        .ceil()
        .min(f64::from(image.height())) as u32;
    for y in top..bottom {
        for x in left..right {
            let distance = (f64::from(x) + 0.5 - center_x).hypot(f64::from(y) + 0.5 - center_y);
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend_opaque_pixel(image, x, y, color, coverage);
            }
        }
    }
}

fn blend_opaque_pixel(image: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>, coverage: f64) {
    if x >= image.width() || y >= image.height() {
        return;
    }
    let alpha = f64::from(color.0[3]) / 255.0 * coverage.clamp(0.0, 1.0);
    let pixel = image.get_pixel_mut(x, y);
    for (destination, source) in pixel.0[..3].iter_mut().zip(color.0[..3].iter()) {
        *destination =
            (f64::from(*source) * alpha + f64::from(*destination) * (1.0 - alpha)).round() as u8;
    }
    pixel.0[3] = 255;
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
    if session.is_active()? {
        return Ok(());
    }
    // History and settings reuse the capture WebView. Leave those modes before
    // preparing a new capture, otherwise the global shortcut appears to do
    // nothing while the overlay is still visible.
    if crate::window::is_capture_overlay_visible(app)? {
        crate::window::hide_capture_overlay(app)?;
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
        rotation_quarters: 0,
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

pub fn copy_selected_capture<R: Runtime>(
    app: &AppHandle<R>,
    ocr_text: Option<&str>,
) -> Result<CopyPayload, String> {
    let session = app.state::<CaptureSession>();
    let total_started = Instant::now();
    let render_started = Instant::now();
    let image = session.rendered_selection()?;
    let render_ms = elapsed_ms(render_started);
    let clipboard_started = Instant::now();
    crate::clipboard::write_image(&image)?;
    let clipboard_ms = elapsed_ms(clipboard_started);
    if let Err(error) = app
        .state::<crate::history::HistoryStore>()
        .save(&image, ocr_text)
    {
        eprintln!("failed to save copied screenshot in history: {error}");
    }
    let result = CopyPayload {
        width: image.width(),
        height: image.height(),
        render_ms,
        clipboard_ms,
        total_ms: elapsed_ms(total_started),
    };
    crate::window::hide_capture_overlay(app)?;
    session.clear()?;
    Ok(result)
}

pub fn pin_selected_capture<R: Runtime>(
    app: &AppHandle<R>,
    ocr_text: Option<&str>,
) -> Result<crate::pin::PinCreatedPayload, String> {
    let total_started = Instant::now();
    let session = app.state::<CaptureSession>();
    let render_started = Instant::now();
    let image = session.rendered_selection()?;
    let render_ms = elapsed_ms(render_started);
    let mut result = crate::pin::create_pin(app, image.clone())?;
    if let Err(error) = app
        .state::<crate::history::HistoryStore>()
        .save(&image, ocr_text)
    {
        eprintln!("failed to save pinned screenshot in history: {error}");
    }
    result.set_pipeline_performance(render_ms, elapsed_ms(total_started));
    crate::window::hide_capture_overlay(app)?;
    session.clear()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use image::{GenericImageView, Rgba, RgbaImage, imageops::crop_imm};

    use super::{
        CaptureSession, CapturedScreen, FrameStyle, PhysicalSelectionRect, SelectionCropRect,
        VirtualDesktop, monitor::MonitorInfo,
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
                rotation_quarters: 0,
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
    fn crops_selected_pixels_and_resets_rotation() {
        let image = RgbaImage::from_fn(4, 3, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        let session = CaptureSession::default();
        session
            .replace(CapturedScreen {
                desktop: desktop(0, 0, 4, 3),
                image: None,
                selection: Some(image),
                annotations: Vec::new(),
                rotation_quarters: 0,
                capture_ms: 0.0,
            })
            .unwrap();

        session.rotate_selection(1).unwrap();
        let payload = session
            .crop_selection(SelectionCropRect {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            })
            .unwrap();

        assert_eq!((payload.width, payload.height), (2, 1));
        let cropped = image::load_from_memory(&session.selected_png().unwrap()).unwrap();
        assert_eq!(cropped.dimensions(), (2, 1));
        assert_eq!(cropped.get_pixel(0, 0).0, [1, 1, 0, 255]);
        assert_eq!(cropped.get_pixel(1, 0).0, [2, 1, 0, 255]);
        assert_eq!(session.rendered_selection().unwrap().dimensions(), (2, 1));
    }

    #[test]
    fn rotates_rendered_selection_before_output() {
        let session = CaptureSession::default();
        session
            .replace(CapturedScreen {
                desktop: desktop(0, 0, 2, 1),
                image: None,
                selection: Some(
                    RgbaImage::from_raw(
                        2,
                        1,
                        vec![
                            255, 0, 0, 255, // red, left
                            0, 0, 255, 255, // blue, right
                        ],
                    )
                    .unwrap(),
                ),
                annotations: Vec::new(),
                rotation_quarters: 0,
                capture_ms: 0.0,
            })
            .unwrap();

        session.rotate_selection(1).unwrap();
        let rendered = session.rendered_selection().unwrap();
        assert_eq!(rendered.dimensions(), (1, 2));
        assert_eq!(rendered.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(rendered.get_pixel(0, 1).0, [0, 0, 255, 255]);
    }

    #[test]
    fn macos_frame_wraps_output_and_keeps_selected_pixels_intact() {
        let session = CaptureSession::default();
        session
            .replace(CapturedScreen {
                desktop: desktop(0, 0, 2, 1),
                image: None,
                selection: Some(
                    RgbaImage::from_raw(
                        2,
                        1,
                        vec![
                            255, 0, 0, 255, // red, left
                            0, 0, 255, 255, // blue, right
                        ],
                    )
                    .unwrap(),
                ),
                annotations: Vec::new(),
                rotation_quarters: 0,
                capture_ms: 0.0,
            })
            .unwrap();

        session.set_frame_style(FrameStyle::Macos).unwrap();
        let rendered = session.rendered_selection().unwrap();
        let metrics = super::macos_frame_metrics(2, 1);
        let content_x = metrics.side;
        let content_y = metrics.top;
        assert_eq!(
            rendered.dimensions(),
            (2 + metrics.side * 2, 1 + metrics.top + metrics.bottom)
        );
        assert_eq!(rendered.get_pixel(content_x, content_y).0, [255, 0, 0, 255]);
        assert_eq!(
            rendered.get_pixel(content_x + 1, content_y).0,
            [0, 0, 255, 255]
        );
        assert_eq!(
            rendered.get_pixel(metrics.dot_first_x, metrics.dot_y).0,
            [255, 95, 86, 255]
        );
        assert_eq!(
            rendered.get_pixel(0, rendered.height() - 1).0,
            [27, 28, 31, 255]
        );
        assert!(rendered.pixels().all(|pixel| pixel.0[3] == 255));
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

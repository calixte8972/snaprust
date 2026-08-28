//! OCR backed by the language packs built into Windows 10 and Windows 11.

use std::time::Instant;

use image::{RgbaImage, imageops::FilterType};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrPayload {
    text: String,
    language: String,
    source_width: u32,
    source_height: u32,
    recognition_width: u32,
    recognition_height: u32,
    line_count: usize,
    lines: Vec<OcrLinePayload>,
    duration_ms: f64,
}

/// A source-image rectangle returned by Windows OCR. Coordinates are always
/// mapped back to the original selection, even when OCR had to downscale it.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OcrRectPayload {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OcrLinePayload {
    text: String,
    rect: OcrRectPayload,
}

#[derive(Debug, Clone, Copy)]
struct ImageExtent {
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrLanguagePayload {
    tag: String,
    display_name: String,
    native_name: String,
}

#[cfg(windows)]
struct ComApartment {
    should_uninitialize: bool,
}

#[cfg(windows)]
impl ComApartment {
    fn initialize() -> Result<Self, String> {
        use windows::Win32::{
            Foundation::RPC_E_CHANGED_MODE,
            System::Com::{COINIT_MULTITHREADED, CoInitializeEx},
        };

        // SAFETY: This initializes COM only for the current OCR worker. A
        // successful call is balanced by CoUninitialize in Drop.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            Ok(Self {
                should_uninitialize: true,
            })
        } else if result == RPC_E_CHANGED_MODE {
            // The thread already owns another COM apartment. WinRT remains
            // usable and this call must not be balanced with CoUninitialize.
            Ok(Self {
                should_uninitialize: false,
            })
        } else {
            Err(format!(
                "failed to initialize the Windows OCR worker: {result:?}"
            ))
        }
    }
}

#[cfg(windows)]
impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.should_uninitialize {
            // SAFETY: Paired with this thread's successful CoInitializeEx.
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

#[cfg(windows)]
thread_local! {
    /// Keep COM initialized for the lifetime of each Tauri blocking worker.
    /// windows-rs caches WinRT activation factories, so tearing the apartment
    /// down between two OCR commands on the same worker can invalidate a cached
    /// factory before the next command reuses it.
    static OCR_COM_APARTMENT: Result<ComApartment, String> = ComApartment::initialize();
}

#[cfg(windows)]
fn ensure_ocr_com_apartment() -> Result<(), String> {
    OCR_COM_APARTMENT.with(|apartment| match apartment {
        Ok(_) => Ok(()),
        Err(error) => Err(error.clone()),
    })
}

#[cfg(windows)]
pub fn available_languages() -> Result<Vec<OcrLanguagePayload>, String> {
    use windows::Media::Ocr::OcrEngine;

    ensure_ocr_com_apartment()?;
    let available = OcrEngine::AvailableRecognizerLanguages()
        .map_err(|error| format!("failed to list Windows OCR languages: {error}"))?;
    let size = available
        .Size()
        .map_err(|error| format!("failed to read the Windows OCR language count: {error}"))?;
    let mut languages = Vec::with_capacity(size as usize);
    for index in 0..size {
        let language = available
            .GetAt(index)
            .map_err(|error| format!("failed to read Windows OCR language {index}: {error}"))?;
        let tag = language
            .LanguageTag()
            .map_err(|error| format!("failed to read an OCR language tag: {error}"))?
            .to_string();
        let display_name = language
            .DisplayName()
            .map(|name| name.to_string())
            .unwrap_or_else(|_| tag.clone());
        let native_name = language
            .NativeName()
            .map(|name| name.to_string())
            .unwrap_or_else(|_| display_name.clone());
        languages.push(OcrLanguagePayload {
            tag,
            display_name,
            native_name,
        });
    }
    languages.sort_by(|left, right| {
        left.native_name
            .to_lowercase()
            .cmp(&right.native_name.to_lowercase())
            .then_with(|| left.tag.cmp(&right.tag))
    });
    Ok(languages)
}

#[cfg(not(windows))]
pub fn available_languages() -> Result<Vec<OcrLanguagePayload>, String> {
    Err("OCR is currently available only on Windows".to_owned())
}

#[cfg(windows)]
pub fn recognize(image: RgbaImage, language_tag: Option<String>) -> Result<OcrPayload, String> {
    use windows::{
        Graphics::Imaging::BitmapDecoder,
        Media::Ocr::OcrEngine,
        Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
    };

    let started = Instant::now();
    ensure_ocr_com_apartment()?;
    let source_width = image.width();
    let source_height = image.height();
    let maximum_dimension = OcrEngine::MaxImageDimension()
        .map_err(|error| format!("failed to query the Windows OCR image limit: {error}"))?;
    let image = fit_for_ocr(image, maximum_dimension);
    let recognition_width = image.width();
    let recognition_height = image.height();

    let png = crate::screenshot::encode_png(&image)?;
    let stream = InMemoryRandomAccessStream::new()
        .map_err(|error| format!("failed to create the OCR image stream: {error}"))?;
    let writer = DataWriter::CreateDataWriter(&stream)
        .map_err(|error| format!("failed to create the OCR stream writer: {error}"))?;
    writer
        .WriteBytes(&png)
        .map_err(|error| format!("failed to write the OCR image stream: {error}"))?;
    writer
        .StoreAsync()
        .and_then(|operation| operation.get())
        .map_err(|error| format!("failed to store the OCR image stream: {error}"))?;
    writer
        .FlushAsync()
        .and_then(|operation| operation.get())
        .map_err(|error| format!("failed to flush the OCR image stream: {error}"))?;
    let _ = writer.DetachStream();
    stream
        .Seek(0)
        .map_err(|error| format!("failed to rewind the OCR image stream: {error}"))?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .and_then(|operation| operation.get())
        .map_err(|error| format!("Windows could not decode the selected image: {error}"))?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .and_then(|operation| operation.get())
        .map_err(|error| {
            format!("Windows could not prepare the selected image for OCR: {error}")
        })?;
    let engine = create_engine(language_tag.as_deref())?;
    let language = engine
        .RecognizerLanguage()
        .and_then(|language| language.LanguageTag())
        .map(|tag| tag.to_string())
        .unwrap_or_else(|_| "unknown".to_owned());
    let result = engine
        .RecognizeAsync(&bitmap)
        .and_then(|operation| operation.get())
        .map_err(|error| format!("Windows OCR recognition failed: {error}"))?;
    let text = result
        .Text()
        .map_err(|error| format!("failed to read the OCR result: {error}"))?
        .to_string();
    let lines = extract_lines(
        &result,
        ImageExtent {
            width: recognition_width,
            height: recognition_height,
        },
        ImageExtent {
            width: source_width,
            height: source_height,
        },
    )?;
    let line_count = lines.len();

    Ok(OcrPayload {
        text,
        language,
        source_width,
        source_height,
        recognition_width,
        recognition_height,
        line_count,
        lines,
        duration_ms: crate::screenshot::elapsed_ms(started),
    })
}

#[cfg(not(windows))]
pub fn recognize(_image: RgbaImage, _language_tag: Option<String>) -> Result<OcrPayload, String> {
    Err("OCR is currently available only on Windows".to_owned())
}

#[cfg(windows)]
fn create_engine(language_tag: Option<&str>) -> Result<windows::Media::Ocr::OcrEngine, String> {
    use windows::{Globalization::Language, Media::Ocr::OcrEngine, core::HSTRING};

    let Some(language_tag) = language_tag.filter(|tag| !tag.trim().is_empty()) else {
        return OcrEngine::TryCreateFromUserProfileLanguages().map_err(|error| {
            format!(
                "Windows OCR is unavailable for the current language profile; install the matching OCR language pack: {error}"
            )
        });
    };
    if language_tag.len() > 128 || language_tag.chars().any(char::is_control) {
        return Err("OCR language tag is invalid".to_owned());
    }

    let language = Language::CreateLanguage(&HSTRING::from(language_tag))
        .map_err(|error| format!("invalid OCR language tag '{language_tag}': {error}"))?;
    let supported = OcrEngine::IsLanguageSupported(&language)
        .map_err(|error| format!("failed to check OCR language '{language_tag}': {error}"))?;
    if !supported {
        return Err(format!(
            "OCR language '{language_tag}' is not installed in Windows"
        ));
    }
    OcrEngine::TryCreateFromLanguage(&language)
        .map_err(|error| format!("failed to create OCR engine for '{language_tag}': {error}"))
}

#[cfg(windows)]
fn extract_lines(
    result: &windows::Media::Ocr::OcrResult,
    recognition_extent: ImageExtent,
    source_extent: ImageExtent,
) -> Result<Vec<OcrLinePayload>, String> {
    let lines = result
        .Lines()
        .map_err(|error| format!("failed to read OCR result lines: {error}"))?;
    let line_count = lines
        .Size()
        .map_err(|error| format!("failed to read OCR line count: {error}"))?;
    let mut payload = Vec::with_capacity(line_count as usize);

    for line_index in 0..line_count {
        let line = lines
            .GetAt(line_index)
            .map_err(|error| format!("failed to read OCR line {line_index}: {error}"))?;
        let words = line
            .Words()
            .map_err(|error| format!("failed to read OCR words for line {line_index}: {error}"))?;
        let word_count = words.Size().map_err(|error| {
            format!("failed to read OCR word count for line {line_index}: {error}")
        })?;
        let mut text = Vec::with_capacity(word_count as usize);
        let mut left = f32::INFINITY;
        let mut top = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        let mut bottom = f32::NEG_INFINITY;

        for word_index in 0..word_count {
            let word = words.GetAt(word_index).map_err(|error| {
                format!("failed to read OCR word {word_index} for line {line_index}: {error}")
            })?;
            let word_text = word
                .Text()
                .map_err(|error| format!("failed to read OCR word text: {error}"))?
                .to_string();
            if !word_text.trim().is_empty() {
                text.push(word_text);
            }
            let rect = word
                .BoundingRect()
                .map_err(|error| format!("failed to read OCR word position: {error}"))?;
            if rect.Width <= 0.0 || rect.Height <= 0.0 {
                continue;
            }
            left = left.min(rect.X);
            top = top.min(rect.Y);
            right = right.max(rect.X + rect.Width);
            bottom = bottom.max(rect.Y + rect.Height);
        }

        let Some(rect) = map_recognition_rect_to_source(
            left,
            top,
            right - left,
            bottom - top,
            recognition_extent,
            source_extent,
        ) else {
            continue;
        };
        payload.push(OcrLinePayload {
            text: text.join(" "),
            rect,
        });
    }

    Ok(payload)
}

fn map_recognition_rect_to_source(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    recognition_extent: ImageExtent,
    source_extent: ImageExtent,
) -> Option<OcrRectPayload> {
    if !x.is_finite()
        || !y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || recognition_extent.width == 0
        || recognition_extent.height == 0
        || source_extent.width == 0
        || source_extent.height == 0
    {
        return None;
    }

    let scale_x = source_extent.width as f32 / recognition_extent.width as f32;
    let scale_y = source_extent.height as f32 / recognition_extent.height as f32;
    let left = (x * scale_x).floor().clamp(0.0, source_extent.width as f32) as u32;
    let top = (y * scale_y)
        .floor()
        .clamp(0.0, source_extent.height as f32) as u32;
    let right = ((x + width) * scale_x)
        .ceil()
        .clamp(0.0, source_extent.width as f32) as u32;
    let bottom = ((y + height) * scale_y)
        .ceil()
        .clamp(0.0, source_extent.height as f32) as u32;
    (right > left && bottom > top).then_some(OcrRectPayload {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn fit_for_ocr(image: RgbaImage, maximum_dimension: u32) -> RgbaImage {
    let largest = image.width().max(image.height());
    if largest <= maximum_dimension || maximum_dimension == 0 {
        return image;
    }

    let scale = f64::from(maximum_dimension) / f64::from(largest);
    let width = (f64::from(image.width()) * scale).round().max(1.0) as u32;
    let height = (f64::from(image.height()) * scale).round().max(1.0) as u32;
    image::imageops::resize(&image, width, height, FilterType::Lanczos3)
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::{ImageExtent, OcrRectPayload, fit_for_ocr, map_recognition_rect_to_source};

    #[test]
    fn keeps_images_that_fit_the_ocr_limit() {
        let image = RgbaImage::from_pixel(800, 600, Rgba([0, 0, 0, 255]));
        let fitted = fit_for_ocr(image, 2_000);
        assert_eq!(fitted.dimensions(), (800, 600));
    }

    #[test]
    fn preserves_aspect_ratio_when_reducing_large_images() {
        let image = RgbaImage::from_pixel(4_000, 2_000, Rgba([0, 0, 0, 255]));
        let fitted = fit_for_ocr(image, 2_000);
        assert_eq!(fitted.dimensions(), (2_000, 1_000));
    }

    #[test]
    fn maps_ocr_rectangles_back_to_a_downscaled_source_image() {
        assert_eq!(
            map_recognition_rect_to_source(
                10.2,
                20.4,
                30.1,
                40.2,
                ImageExtent {
                    width: 100,
                    height: 100,
                },
                ImageExtent {
                    width: 200,
                    height: 300,
                },
            ),
            Some(OcrRectPayload {
                x: 20,
                y: 61,
                width: 61,
                height: 121,
            })
        );
    }

    #[test]
    fn discards_ocr_rectangles_outside_or_without_visible_area() {
        assert_eq!(
            map_recognition_rect_to_source(
                200.0,
                10.0,
                4.0,
                10.0,
                ImageExtent {
                    width: 100,
                    height: 100,
                },
                ImageExtent {
                    width: 100,
                    height: 100,
                },
            ),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn rejects_control_characters_in_explicit_language_tags() {
        let error = super::create_engine(Some("en-US\0invalid")).unwrap_err();
        assert!(error.contains("invalid"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the Windows OCR service"]
    fn lists_installed_windows_ocr_languages() {
        let languages = super::available_languages().unwrap();
        assert!(!languages.is_empty());
        assert!(languages.iter().all(|language| !language.tag.is_empty()));
        eprintln!("installed OCR languages: {languages:?}");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the interactive Windows OCR service and an installed OCR language pack"]
    fn recognizes_rendered_text_with_windows_ocr() {
        let mut image = RgbaImage::from_pixel(1_400, 260, Rgba([255, 255, 255, 255]));
        crate::annotation::render_annotations(
            &mut image,
            &[crate::annotation::Annotation::Text {
                position: crate::annotation::AnnotationPoint { x: 40.0, y: 55.0 },
                text: "SNAPRUST OCR 12345".to_owned(),
                color: "#000000".to_owned(),
                font_size: 96.0,
            }],
        )
        .unwrap();

        let language = super::available_languages()
            .unwrap()
            .into_iter()
            .next()
            .expect("at least one Windows OCR language is required");
        let result = super::recognize(image, Some(language.tag)).unwrap();
        assert!(
            result.text.contains("12345"),
            "unexpected OCR result: {:?}",
            result.text
        );
        assert!(
            !result.lines.is_empty(),
            "OCR returned text but no positioned lines: {result:?}"
        );
        assert!(
            result
                .lines
                .iter()
                .all(|line| line.rect.width > 0 && line.rect.height > 0),
            "OCR returned an invalid positioned line: {result:?}"
        );
    }
}

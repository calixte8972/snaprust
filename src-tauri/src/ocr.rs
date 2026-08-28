//! OCR backed by the language packs built into Windows 10 and Windows 11.

use std::time::Instant;

use image::{Rgba, RgbaImage, imageops::FilterType};
use serde::Serialize;

const OCR_TARGET_LARGEST_DIMENSION: u32 = 1_600;
const OCR_TARGET_SMALLEST_DIMENSION: u32 = 480;
const OCR_MAX_UPSCALE: f64 = 2.0;
const OCR_PADDING: u32 = 16;

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

#[derive(Debug, Clone, Copy)]
struct OcrImageTransform {
    content_x: u32,
    content_y: u32,
    content_extent: ImageExtent,
    source_extent: ImageExtent,
}

struct PreparedOcrImage {
    image: RgbaImage,
    transform: OcrImageTransform,
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
    let prepared = prepare_for_ocr(image, maximum_dimension)?;
    let recognition_width = prepared.image.width();
    let recognition_height = prepared.image.height();

    let png = crate::screenshot::encode_png(&prepared.image)?;
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
    let lines = extract_lines(&result, prepared.transform)?;
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
    transform: OcrImageTransform,
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

        let Some(rect) =
            map_recognition_rect_to_source(left, top, right - left, bottom - top, transform)
        else {
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
    transform: OcrImageTransform,
) -> Option<OcrRectPayload> {
    if !x.is_finite()
        || !y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || transform.content_extent.width == 0
        || transform.content_extent.height == 0
        || transform.source_extent.width == 0
        || transform.source_extent.height == 0
    {
        return None;
    }

    let content_x = transform.content_x as f32;
    let content_y = transform.content_y as f32;
    let content_right = content_x + transform.content_extent.width as f32;
    let content_bottom = content_y + transform.content_extent.height as f32;
    let recognition_left = x.max(content_x).min(content_right);
    let recognition_top = y.max(content_y).min(content_bottom);
    let recognition_right = (x + width).max(content_x).min(content_right);
    let recognition_bottom = (y + height).max(content_y).min(content_bottom);
    if recognition_right <= recognition_left || recognition_bottom <= recognition_top {
        return None;
    }

    let scale_x = transform.source_extent.width as f32 / transform.content_extent.width as f32;
    let scale_y = transform.source_extent.height as f32 / transform.content_extent.height as f32;
    let left = ((recognition_left - content_x) * scale_x)
        .floor()
        .clamp(0.0, transform.source_extent.width as f32) as u32;
    let top = ((recognition_top - content_y) * scale_y)
        .floor()
        .clamp(0.0, transform.source_extent.height as f32) as u32;
    let right = ((recognition_right - content_x) * scale_x)
        .ceil()
        .clamp(0.0, transform.source_extent.width as f32) as u32;
    let bottom = ((recognition_bottom - content_y) * scale_y)
        .ceil()
        .clamp(0.0, transform.source_extent.height as f32) as u32;
    (right > left && bottom > top).then_some(OcrRectPayload {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn prepare_for_ocr(
    mut image: RgbaImage,
    maximum_dimension: u32,
) -> Result<PreparedOcrImage, String> {
    let source_extent = ImageExtent {
        width: image.width(),
        height: image.height(),
    };
    if source_extent.width == 0 || source_extent.height == 0 {
        return Err("cannot recognize an empty image".to_owned());
    }

    flatten_transparency(&mut image);
    let largest = source_extent.width.max(source_extent.height);
    let smallest = source_extent.width.min(source_extent.height);
    let padding = if maximum_dimension == 0 {
        OCR_PADDING
    } else {
        OCR_PADDING.min(maximum_dimension.saturating_sub(1) / 2)
    };
    let maximum_content_dimension = if maximum_dimension == 0 {
        u32::MAX
    } else {
        maximum_dimension.saturating_sub(padding * 2).max(1)
    };
    let largest_dimension_scale = f64::from(OCR_TARGET_LARGEST_DIMENSION) / f64::from(largest);
    let smallest_dimension_scale = f64::from(OCR_TARGET_SMALLEST_DIMENSION) / f64::from(smallest);
    let desired_scale = largest_dimension_scale
        .max(smallest_dimension_scale)
        .clamp(1.0, OCR_MAX_UPSCALE);
    let maximum_scale = f64::from(maximum_content_dimension) / f64::from(largest);
    let scale = desired_scale.min(maximum_scale);
    let content_width = (f64::from(source_extent.width) * scale).round().max(1.0) as u32;
    let content_height = (f64::from(source_extent.height) * scale).round().max(1.0) as u32;
    let mut content =
        if content_width == source_extent.width && content_height == source_extent.height {
            image
        } else {
            image::imageops::resize(&image, content_width, content_height, FilterType::Lanczos3)
        };
    if scale > 1.0 {
        content = image::imageops::unsharpen(&content, 0.8, 1);
    }

    let background = edge_average_color(&content);
    let recognition_width = content_width
        .checked_add(padding * 2)
        .ok_or_else(|| "OCR image padding would make the image too wide".to_owned())?;
    let recognition_height = content_height
        .checked_add(padding * 2)
        .ok_or_else(|| "OCR image padding would make the image too tall".to_owned())?;
    let mut prepared = RgbaImage::from_pixel(recognition_width, recognition_height, background);
    image::imageops::overlay(
        &mut prepared,
        &content,
        i64::from(padding),
        i64::from(padding),
    );

    Ok(PreparedOcrImage {
        image: prepared,
        transform: OcrImageTransform {
            content_x: padding,
            content_y: padding,
            content_extent: ImageExtent {
                width: content_width,
                height: content_height,
            },
            source_extent,
        },
    })
}

fn flatten_transparency(image: &mut RgbaImage) {
    for pixel in image.pixels_mut() {
        let alpha = u32::from(pixel.0[3]);
        let inverse_alpha = 255 - alpha;
        for channel in &mut pixel.0[..3] {
            *channel = ((u32::from(*channel) * alpha + 255 * inverse_alpha + 127) / 255) as u8;
        }
        pixel.0[3] = 255;
    }
}

fn edge_average_color(image: &RgbaImage) -> Rgba<u8> {
    let mut sums = [0_u64; 3];
    let mut count = 0_u64;
    let mut sample = |pixel: &Rgba<u8>| {
        for (sum, channel) in sums.iter_mut().zip(pixel.0[..3].iter()) {
            *sum += u64::from(*channel);
        }
        count += 1;
    };

    for x in 0..image.width() {
        sample(image.get_pixel(x, 0));
        if image.height() > 1 {
            sample(image.get_pixel(x, image.height() - 1));
        }
    }
    for y in 1..image.height().saturating_sub(1) {
        sample(image.get_pixel(0, y));
        if image.width() > 1 {
            sample(image.get_pixel(image.width() - 1, y));
        }
    }

    Rgba([
        (sums[0] / count.max(1)) as u8,
        (sums[1] / count.max(1)) as u8,
        (sums[2] / count.max(1)) as u8,
        255,
    ])
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::{
        ImageExtent, OCR_PADDING, OcrImageTransform, OcrRectPayload,
        map_recognition_rect_to_source, prepare_for_ocr,
    };

    #[test]
    fn upscales_small_images_and_adds_edge_padding() {
        let image = RgbaImage::from_pixel(400, 200, Rgba([24, 32, 40, 255]));
        let prepared = prepare_for_ocr(image, 2_600).unwrap();
        assert_eq!(prepared.transform.content_extent.width, 800);
        assert_eq!(prepared.transform.content_extent.height, 400);
        assert_eq!(prepared.image.dimensions(), (832, 432));
        assert_eq!(prepared.transform.content_x, OCR_PADDING);
        assert_eq!(prepared.image.get_pixel(0, 0).0, [24, 32, 40, 255]);
    }

    #[test]
    fn enlarges_narrow_text_strips_up_to_the_windows_limit() {
        let image = RgbaImage::from_pixel(800, 80, Rgba([255, 255, 255, 255]));
        let prepared = prepare_for_ocr(image, 1_300).unwrap();
        assert_eq!(prepared.transform.content_extent.width, 1_268);
        assert_eq!(prepared.transform.content_extent.height, 127);
        assert_eq!(prepared.image.dimensions(), (1_300, 159));
    }

    #[test]
    fn preserves_aspect_ratio_and_padding_when_reducing_large_images() {
        let image = RgbaImage::from_pixel(2_000, 1_000, Rgba([0, 0, 0, 255]));
        let prepared = prepare_for_ocr(image, 1_000).unwrap();
        assert_eq!(prepared.transform.content_extent.width, 968);
        assert_eq!(prepared.transform.content_extent.height, 484);
        assert_eq!(prepared.image.dimensions(), (1_000, 516));
    }

    #[test]
    fn flattens_transparent_pixels_before_recognition() {
        let image = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0]));
        let prepared = prepare_for_ocr(image, 2_600).unwrap();
        assert_eq!(
            prepared
                .image
                .get_pixel(prepared.transform.content_x, prepared.transform.content_y)
                .0,
            [255, 255, 255, 255]
        );
        assert!(prepared.image.pixels().all(|pixel| pixel.0[3] == 255));
    }

    #[test]
    fn maps_ocr_rectangles_back_to_a_downscaled_source_image() {
        assert_eq!(
            map_recognition_rect_to_source(
                10.2,
                20.4,
                30.1,
                40.2,
                OcrImageTransform {
                    content_x: 0,
                    content_y: 0,
                    content_extent: ImageExtent {
                        width: 100,
                        height: 100,
                    },
                    source_extent: ImageExtent {
                        width: 200,
                        height: 300,
                    },
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
    fn removes_padding_offset_when_mapping_ocr_rectangles() {
        assert_eq!(
            map_recognition_rect_to_source(
                36.0,
                26.0,
                40.0,
                20.0,
                OcrImageTransform {
                    content_x: 16,
                    content_y: 16,
                    content_extent: ImageExtent {
                        width: 200,
                        height: 100,
                    },
                    source_extent: ImageExtent {
                        width: 100,
                        height: 50,
                    },
                },
            ),
            Some(OcrRectPayload {
                x: 10,
                y: 5,
                width: 20,
                height: 10,
            })
        );
    }

    #[test]
    fn discards_ocr_rectangles_outside_the_prepared_content() {
        assert_eq!(
            map_recognition_rect_to_source(
                0.0,
                0.0,
                10.0,
                10.0,
                OcrImageTransform {
                    content_x: 16,
                    content_y: 16,
                    content_extent: ImageExtent {
                        width: 100,
                        height: 100,
                    },
                    source_extent: ImageExtent {
                        width: 100,
                        height: 100,
                    },
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

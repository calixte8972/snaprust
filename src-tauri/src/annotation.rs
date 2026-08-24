//! Structured screenshot annotations and their final Rust-side rasterization.

use std::{fs, path::Path, sync::OnceLock};

use fontdue::{Font, FontSettings};
use image::{Rgba, RgbaImage};
use serde::Deserialize;

const MAX_STROKE_WIDTH: f64 = 128.0;
const MAX_TEXT_LENGTH: usize = 512;
const MAX_BRUSH_POINTS: usize = 20_000;
static DEFAULT_FONT: OnceLock<Result<Font, String>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Annotation {
    Arrow {
        start: AnnotationPoint,
        end: AnnotationPoint,
        color: String,
        width: f64,
    },
    Rectangle {
        rect: AnnotationRect,
        color: String,
        width: f64,
    },
    Ellipse {
        rect: AnnotationRect,
        color: String,
        width: f64,
    },
    Brush {
        points: Vec<AnnotationPoint>,
        color: String,
        width: f64,
    },
    Mosaic {
        rect: AnnotationRect,
        block_size: f64,
    },
    Text {
        position: AnnotationPoint,
        text: String,
        color: String,
        font_size: f64,
    },
}

#[derive(Clone, Copy)]
struct Color(Rgba<u8>);

impl Color {
    fn parse(value: &str) -> Result<Self, String> {
        let value = value.strip_prefix('#').unwrap_or(value);
        let parse_component = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&value[range], 16)
                .map_err(|_| "annotation color contains non-hexadecimal characters".to_owned())
        };

        match value.len() {
            6 => Ok(Self(Rgba([
                parse_component(0..2)?,
                parse_component(2..4)?,
                parse_component(4..6)?,
                u8::MAX,
            ]))),
            8 => Ok(Self(Rgba([
                parse_component(0..2)?,
                parse_component(2..4)?,
                parse_component(4..6)?,
                parse_component(6..8)?,
            ]))),
            _ => Err("annotation color must be #RRGGBB or #RRGGBBAA".to_owned()),
        }
    }
}

pub fn validate_annotations(
    annotations: &[Annotation],
    image_width: u32,
    image_height: u32,
) -> Result<(), String> {
    for annotation in annotations {
        annotation.validate(image_width, image_height)?;
    }
    Ok(())
}

pub fn render_annotations(image: &mut RgbaImage, annotations: &[Annotation]) -> Result<(), String> {
    validate_annotations(annotations, image.width(), image.height())?;
    let font = if annotations
        .iter()
        .any(|annotation| matches!(annotation, Annotation::Text { .. }))
    {
        Some(default_font()?)
    } else {
        None
    };

    for annotation in annotations {
        match annotation {
            Annotation::Arrow {
                start,
                end,
                color,
                width,
            } => draw_arrow(image, *start, *end, Color::parse(color)?.0, *width),
            Annotation::Rectangle { rect, color, width } => {
                let (left, top, right, bottom) = normalized_rect(*rect);
                let color = Color::parse(color)?.0;
                draw_line(image, left, top, right, top, color, *width);
                draw_line(image, right, top, right, bottom, color, *width);
                draw_line(image, right, bottom, left, bottom, color, *width);
                draw_line(image, left, bottom, left, top, color, *width);
            }
            Annotation::Ellipse { rect, color, width } => {
                draw_ellipse(image, *rect, Color::parse(color)?.0, *width);
            }
            Annotation::Brush {
                points,
                color,
                width,
            } => {
                let color = Color::parse(color)?.0;
                for pair in points.windows(2) {
                    draw_line(
                        image, pair[0].x, pair[0].y, pair[1].x, pair[1].y, color, *width,
                    );
                }
                if let Some(point) = points.first() {
                    draw_disk(image, point.x, point.y, *width / 2.0, color);
                }
            }
            Annotation::Mosaic { rect, block_size } => mosaic(image, *rect, *block_size),
            Annotation::Text {
                position,
                text,
                color,
                font_size,
            } => draw_text(
                image,
                font.as_ref()
                    .expect("text annotations require a loaded font"),
                *position,
                text,
                Color::parse(color)?.0,
                *font_size,
            ),
        }
    }

    Ok(())
}

impl Annotation {
    pub(crate) fn simplify_brush_path(&mut self) {
        if let Self::Brush { points, width, .. } = self {
            *points = simplify_polyline(points, (0.75_f64).max(*width * 0.18));
        }
    }

    fn validate(&self, image_width: u32, image_height: u32) -> Result<(), String> {
        let point = |point: AnnotationPoint| validate_point(point, image_width, image_height);
        let rect = |rect: AnnotationRect| validate_rect(rect, image_width, image_height);
        let width = |value: f64, label: &str| validate_width(value, label);

        match self {
            Self::Arrow {
                start,
                end,
                color,
                width: stroke_width,
            } => {
                point(*start)?;
                point(*end)?;
                let _ = Color::parse(color)?;
                width(*stroke_width, "stroke width")
            }
            Self::Rectangle {
                rect: area,
                color,
                width: stroke_width,
            }
            | Self::Ellipse {
                rect: area,
                color,
                width: stroke_width,
            } => {
                rect(*area)?;
                let _ = Color::parse(color)?;
                width(*stroke_width, "stroke width")
            }
            Self::Brush {
                points,
                color,
                width: stroke_width,
            } => {
                if points.is_empty() || points.len() > MAX_BRUSH_POINTS {
                    return Err(format!(
                        "brush must contain between 1 and {MAX_BRUSH_POINTS} points"
                    ));
                }
                for brush_point in points {
                    point(*brush_point)?;
                }
                let _ = Color::parse(color)?;
                width(*stroke_width, "brush width")
            }
            Self::Mosaic {
                rect: area,
                block_size,
            } => {
                rect(*area)?;
                if !block_size.is_finite() || !(2.0..=128.0).contains(block_size) {
                    return Err("mosaic block size must be between 2 and 128 pixels".to_owned());
                }
                Ok(())
            }
            Self::Text {
                position,
                text,
                color,
                font_size,
            } => {
                point(*position)?;
                if text.is_empty() || text.chars().count() > MAX_TEXT_LENGTH {
                    return Err(format!(
                        "text must contain between 1 and {MAX_TEXT_LENGTH} characters"
                    ));
                }
                let _ = Color::parse(color)?;
                if !font_size.is_finite() || !(8.0..=192.0).contains(font_size) {
                    return Err("font size must be between 8 and 192 pixels".to_owned());
                }
                Ok(())
            }
        }
    }
}

fn squared_distance_to_segment(
    point: AnnotationPoint,
    start: AnnotationPoint,
    end: AnnotationPoint,
) -> f64 {
    let mut x = start.x;
    let mut y = start.y;
    let dx = end.x - x;
    let dy = end.y - y;
    if dx != 0.0 || dy != 0.0 {
        let offset = ((point.x - x) * dx + (point.y - y) * dy) / (dx * dx + dy * dy);
        if offset > 1.0 {
            x = end.x;
            y = end.y;
        } else if offset > 0.0 {
            x += dx * offset;
            y += dy * offset;
        }
    }
    let distance_x = point.x - x;
    let distance_y = point.y - y;
    distance_x * distance_x + distance_y * distance_y
}

fn simplify_polyline(points: &[AnnotationPoint], tolerance: f64) -> Vec<AnnotationPoint> {
    if points.len() <= 2 || !tolerance.is_finite() || tolerance <= 0.0 {
        return points.to_vec();
    }

    let squared_tolerance = tolerance * tolerance;
    let mut radial_points = Vec::with_capacity(points.len());
    radial_points.push(points[0]);
    let mut previous = points[0];
    for point in &points[1..points.len() - 1] {
        let dx = point.x - previous.x;
        let dy = point.y - previous.y;
        if dx * dx + dy * dy > squared_tolerance {
            radial_points.push(*point);
            previous = *point;
        }
    }
    radial_points.push(*points.last().expect("non-empty brush path"));
    if radial_points.len() <= 2 {
        return radial_points;
    }

    let mut markers = vec![false; radial_points.len()];
    markers[0] = true;
    markers[radial_points.len() - 1] = true;
    let mut stack = vec![(0, radial_points.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        let mut farthest = None;
        let mut farthest_distance = squared_tolerance;
        for index in first + 1..last {
            let distance = squared_distance_to_segment(
                radial_points[index],
                radial_points[first],
                radial_points[last],
            );
            if distance > farthest_distance {
                farthest_distance = distance;
                farthest = Some(index);
            }
        }
        if let Some(index) = farthest {
            markers[index] = true;
            stack.push((first, index));
            stack.push((index, last));
        }
    }

    radial_points
        .into_iter()
        .zip(markers)
        .filter_map(|(point, keep)| keep.then_some(point))
        .collect()
}

fn validate_point(
    point: AnnotationPoint,
    image_width: u32,
    image_height: u32,
) -> Result<(), String> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || point.x < 0.0
        || point.y < 0.0
        || point.x > f64::from(image_width)
        || point.y > f64::from(image_height)
    {
        return Err("annotation point is outside the selected image".to_owned());
    }
    Ok(())
}

fn validate_rect(rect: AnnotationRect, image_width: u32, image_height: u32) -> Result<(), String> {
    if !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        return Err("annotation rectangle must have a positive finite size".to_owned());
    }
    validate_point(
        AnnotationPoint {
            x: rect.x,
            y: rect.y,
        },
        image_width,
        image_height,
    )?;
    validate_point(
        AnnotationPoint {
            x: rect.x + rect.width,
            y: rect.y + rect.height,
        },
        image_width,
        image_height,
    )
}

fn validate_width(width: f64, label: &str) -> Result<(), String> {
    if !width.is_finite() || !(1.0..=MAX_STROKE_WIDTH).contains(&width) {
        return Err(format!(
            "{label} must be between 1 and {MAX_STROKE_WIDTH} pixels"
        ));
    }
    Ok(())
}

fn normalized_rect(rect: AnnotationRect) -> (f64, f64, f64, f64) {
    (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height)
}

fn draw_arrow(
    image: &mut RgbaImage,
    start: AnnotationPoint,
    end: AnnotationPoint,
    color: Rgba<u8>,
    width: f64,
) {
    draw_line(image, start.x, start.y, end.x, end.y, color, width);
    let angle = (end.y - start.y).atan2(end.x - start.x);
    let head_length = (width * 4.5).clamp(12.0, 48.0);
    for offset in [std::f64::consts::PI * 0.78, -std::f64::consts::PI * 0.78] {
        draw_line(
            image,
            end.x,
            end.y,
            end.x + head_length * (angle + offset).cos(),
            end.y + head_length * (angle + offset).sin(),
            color,
            width,
        );
    }
}

fn draw_ellipse(image: &mut RgbaImage, rect: AnnotationRect, color: Rgba<u8>, width: f64) {
    let center_x = rect.x + rect.width / 2.0;
    let center_y = rect.y + rect.height / 2.0;
    let radius_x = rect.width / 2.0;
    let radius_y = rect.height / 2.0;
    let segments =
        ((radius_x.max(radius_y) * std::f64::consts::TAU).ceil() as usize).clamp(24, 4_096);
    let mut previous = (center_x + radius_x, center_y);
    for index in 1..=segments {
        let angle = std::f64::consts::TAU * index as f64 / segments as f64;
        let next = (
            center_x + radius_x * angle.cos(),
            center_y + radius_y * angle.sin(),
        );
        draw_line(image, previous.0, previous.1, next.0, next.1, color, width);
        previous = next;
    }
}

fn draw_line(
    image: &mut RgbaImage,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    color: Rgba<u8>,
    width: f64,
) {
    let distance = (x2 - x1).hypot(y2 - y1);
    let steps = distance.ceil().max(1.0) as usize;
    for step in 0..=steps {
        let progress = step as f64 / steps as f64;
        draw_disk(
            image,
            x1 + (x2 - x1) * progress,
            y1 + (y2 - y1) * progress,
            width / 2.0,
            color,
        );
    }
}

fn draw_disk(image: &mut RgbaImage, x: f64, y: f64, radius: f64, color: Rgba<u8>) {
    let left = (x - radius).floor().max(0.0) as u32;
    let right = (x + radius)
        .ceil()
        .min(f64::from(image.width().saturating_sub(1))) as u32;
    let top = (y - radius).floor().max(0.0) as u32;
    let bottom = (y + radius)
        .ceil()
        .min(f64::from(image.height().saturating_sub(1))) as u32;
    for pixel_y in top..=bottom {
        for pixel_x in left..=right {
            let dx = f64::from(pixel_x) - x;
            let dy = f64::from(pixel_y) - y;
            if dx * dx + dy * dy <= radius * radius {
                blend(image.get_pixel_mut(pixel_x, pixel_y), color, 1.0);
            }
        }
    }
}

fn mosaic(image: &mut RgbaImage, rect: AnnotationRect, block_size: f64) {
    let (left, top, right, bottom) = normalized_rect(rect);
    let block = block_size.round() as u32;
    let left = left.floor() as u32;
    let top = top.floor() as u32;
    let right = right.ceil() as u32;
    let bottom = bottom.ceil() as u32;
    for block_y in (top..bottom).step_by(block as usize) {
        for block_x in (left..right).step_by(block as usize) {
            let end_x = (block_x + block).min(right).min(image.width());
            let end_y = (block_y + block).min(bottom).min(image.height());
            let mut totals = [0u64; 4];
            let mut count = 0u64;
            for pixel_y in block_y..end_y {
                for pixel_x in block_x..end_x {
                    let pixel = image.get_pixel(pixel_x, pixel_y).0;
                    for (total, component) in totals.iter_mut().zip(pixel) {
                        *total += u64::from(component);
                    }
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let averaged = Rgba(totals.map(|total| (total / count) as u8));
            for pixel_y in block_y..end_y {
                for pixel_x in block_x..end_x {
                    *image.get_pixel_mut(pixel_x, pixel_y) = averaged;
                }
            }
        }
    }
}

fn draw_text(
    image: &mut RgbaImage,
    font: &Font,
    position: AnnotationPoint,
    text: &str,
    color: Rgba<u8>,
    font_size: f64,
) {
    let mut pen_x = position.x;
    let mut baseline = position.y + font_size;
    for character in text.chars() {
        if character == '\n' {
            pen_x = position.x;
            baseline += font_size * 1.25;
            continue;
        }
        let (metrics, bitmap) = font.rasterize(character, font_size as f32);
        let glyph_x = pen_x + f64::from(metrics.xmin);
        let glyph_y = baseline - f64::from(metrics.ymin) - metrics.height as f64;
        for glyph_y_offset in 0..metrics.height {
            for glyph_x_offset in 0..metrics.width {
                let alpha =
                    f64::from(bitmap[glyph_y_offset * metrics.width + glyph_x_offset]) / 255.0;
                let pixel_x = glyph_x + glyph_x_offset as f64;
                let pixel_y = glyph_y + glyph_y_offset as f64;
                if pixel_x >= 0.0
                    && pixel_y >= 0.0
                    && pixel_x < f64::from(image.width())
                    && pixel_y < f64::from(image.height())
                {
                    blend(
                        image.get_pixel_mut(pixel_x as u32, pixel_y as u32),
                        color,
                        alpha,
                    );
                }
            }
        }
        pen_x += f64::from(metrics.advance_width);
    }
}

fn blend(destination: &mut Rgba<u8>, source: Rgba<u8>, coverage: f64) {
    let source_alpha = (f64::from(source.0[3]) / 255.0) * coverage;
    let destination_alpha = f64::from(destination.0[3]) / 255.0;
    let output_alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
    if output_alpha == 0.0 {
        *destination = Rgba([0, 0, 0, 0]);
        return;
    }
    for index in 0..3 {
        let source_component = f64::from(source.0[index]) / 255.0;
        let destination_component = f64::from(destination.0[index]) / 255.0;
        destination.0[index] = (((source_component * source_alpha
            + destination_component * destination_alpha * (1.0 - source_alpha))
            / output_alpha)
            * 255.0)
            .round() as u8;
    }
    destination.0[3] = (output_alpha * 255.0).round() as u8;
}

fn default_font() -> Result<&'static Font, String> {
    DEFAULT_FONT
        .get_or_init(load_default_font)
        .as_ref()
        .map_err(Clone::clone)
}

fn load_default_font() -> Result<Font, String> {
    let candidates = [
        (r"C:\Windows\Fonts\msyh.ttc", 0),
        (r"C:\Windows\Fonts\msyhbd.ttc", 0),
        (r"C:\Windows\Fonts\segoeui.ttf", 0),
    ];
    for (path, collection_index) in candidates {
        if !Path::new(path).is_file() {
            continue;
        }
        let bytes = fs::read(path)
            .map_err(|error| format!("failed to read Windows font {path}: {error}"))?;
        if let Ok(font) = Font::from_bytes(
            bytes,
            FontSettings {
                collection_index,
                ..Default::default()
            },
        ) {
            return Ok(font);
        }
    }
    Err("could not load a supported Windows UI font for text annotations".to_owned())
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::{
        Annotation, AnnotationPoint, AnnotationRect, render_annotations, validate_annotations,
    };

    #[test]
    fn deserializes_camel_case_annotation_fields_from_typescript() {
        let text: Annotation = serde_json::from_str(
            r##"{"kind":"text","position":{"x":10,"y":20},"text":"标注","color":"#43d9a3","fontSize":24}"##,
        )
        .unwrap();
        let mosaic: Annotation = serde_json::from_str(
            r#"{"kind":"mosaic","rect":{"x":1,"y":2,"width":30,"height":40},"blockSize":8}"#,
        )
        .unwrap();

        assert!(matches!(
            text,
            Annotation::Text {
                font_size: 24.0,
                ..
            }
        ));
        assert!(matches!(
            mosaic,
            Annotation::Mosaic {
                block_size: 8.0,
                ..
            }
        ));
    }

    #[test]
    fn rectangle_rasterization_changes_the_expected_border() {
        let mut image = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
        let annotations = [Annotation::Rectangle {
            rect: AnnotationRect {
                x: 4.0,
                y: 4.0,
                width: 10.0,
                height: 8.0,
            },
            color: "#ff0000".to_owned(),
            width: 2.0,
        }];

        render_annotations(&mut image, &annotations).unwrap();

        assert_eq!(image.get_pixel(4, 4).0[..3], [255, 0, 0]);
        assert_eq!(image.get_pixel(10, 10).0[..3], [0, 0, 0]);
    }

    #[test]
    fn mosaic_replaces_each_block_with_its_average_color() {
        let mut image =
            RgbaImage::from_fn(4, 4, |x, y| Rgba([(x * 20) as u8, (y * 20) as u8, 0, 255]));
        let annotations = [Annotation::Mosaic {
            rect: AnnotationRect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
            },
            block_size: 2.0,
        }];

        render_annotations(&mut image, &annotations).unwrap();

        assert_eq!(*image.get_pixel(0, 0), Rgba([10, 10, 0, 255]));
        assert_eq!(image.get_pixel(0, 0), image.get_pixel(1, 1));
        assert_ne!(image.get_pixel(0, 0), image.get_pixel(2, 0));
    }

    #[test]
    fn simplifies_long_brush_paths_without_losing_endpoints_or_corners() {
        let mut points = (0..1_000)
            .map(|x| AnnotationPoint {
                x: f64::from(x) / 10.0,
                y: 10.0,
            })
            .collect::<Vec<_>>();
        points.push(AnnotationPoint { x: 100.0, y: 80.0 });
        points.extend((1..=1_000).map(|offset| AnnotationPoint {
            x: 100.0 + f64::from(offset) / 10.0,
            y: 80.0,
        }));
        let original_first = points[0];
        let original_last = *points.last().unwrap();
        let mut annotation = Annotation::Brush {
            points,
            color: "#00aa77".to_owned(),
            width: 4.0,
        };

        annotation.simplify_brush_path();

        let Annotation::Brush { points, .. } = annotation else {
            unreachable!();
        };
        assert_eq!(points.first(), Some(&original_first));
        assert_eq!(points.last(), Some(&original_last));
        assert!(points.len() <= 5, "simplified to {} points", points.len());
        assert!(points.iter().any(|point| point.y == 80.0));
    }

    #[test]
    fn rejects_annotation_coordinates_outside_the_image() {
        let annotations = [Annotation::Arrow {
            start: AnnotationPoint { x: 1.0, y: 1.0 },
            end: AnnotationPoint { x: 99.0, y: 1.0 },
            color: "#00ff00".to_owned(),
            width: 4.0,
        }];

        assert!(validate_annotations(&annotations, 20, 20).is_err());
    }

    #[test]
    fn renders_mixed_latin_and_chinese_text_with_a_windows_font() {
        let mut image = RgbaImage::from_pixel(320, 100, Rgba([255, 255, 255, 255]));
        let annotations = [Annotation::Text {
            position: AnnotationPoint { x: 10.0, y: 10.0 },
            text: "SnapRust 标注".to_owned(),
            color: "#00aa77".to_owned(),
            font_size: 24.0,
        }];

        render_annotations(&mut image, &annotations).unwrap();

        assert!(image.pixels().any(|pixel| pixel.0[..3] != [255, 255, 255]));
    }
}

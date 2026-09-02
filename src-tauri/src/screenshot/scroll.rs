//! Automatic scrolling capture and overlap-based vertical stitching.

use std::sync::atomic::{AtomicBool, Ordering};

use image::RgbaImage;

const MAX_SCROLL_SEGMENTS: usize = 24;
// WebView2 canvas allocations are substantially more expensive than the Rust
// RGBA buffer because the editor keeps a source image and two drawing layers.
// Keep long captures below a predictable memory ceiling until the editor uses
// tiled canvases.
const MAX_SCROLL_IMAGE_HEIGHT: u32 = 16_000;
const MAX_SCROLL_IMAGE_PIXELS: u64 = 16_000_000;
const DUPLICATE_SCORE: f64 = 2.0;
const MAX_OVERLAP_SCORE: f64 = 28.0;
const FIXED_ROW_SAME_SCREEN_SCORE: f64 = 6.0;
const FIXED_ROW_ALIGNED_SCORE: f64 = 10.0;
const FIXED_ROW_MIN_RUN: u32 = 2;
const FIXED_ROW_SEARCH_MARGIN: u32 = 128;

pub(super) struct ScrollCaptureOutput {
    pub image: RgbaImage,
    pub segments: usize,
}

#[cfg(windows)]
pub(super) fn capture_scrolling_region(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    cancelled: &AtomicBool,
) -> Result<ScrollCaptureOutput, String> {
    use std::{thread, time::Duration};

    use windows::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE},
            WindowsAndMessaging::{
                GA_ROOT, GetAncestor, PostMessageW, SetForegroundWindow, WM_MOUSEWHEEL,
                WindowFromPoint,
            },
        },
    };

    if width < 32 || height < 64 {
        return Err("滚动截图区域太小，请选择完整的可滚动内容区域".to_owned());
    }
    // Hiding a WebView is asynchronous at the compositor boundary. Waiting
    // briefly prevents WindowFromPoint from selecting SnapRust's own overlay.
    thread::sleep(Duration::from_millis(90));
    let center_x = x.saturating_add(i32::try_from(width / 2).unwrap_or(i32::MAX));
    let center_y = y.saturating_add(i32::try_from(height / 2).unwrap_or(i32::MAX));
    let point = POINT {
        x: center_x,
        y: center_y,
    };
    // SAFETY: The point uses physical desktop coordinates from the validated
    // capture selection. WindowFromPoint does not retain the pointer.
    let target = unsafe { WindowFromPoint(point) };
    if target.0.is_null() {
        return Err("未找到滚动截图区域下方的目标窗口".to_owned());
    }
    // Give applications that only accept wheel input while active a chance to
    // receive it. A failure is non-fatal because Windows can scroll inactive
    // windows under the pointer as well.
    let root = unsafe { GetAncestor(target, GA_ROOT) };
    if !root.0.is_null() {
        let _ = unsafe { SetForegroundWindow(root) };
    }
    thread::sleep(Duration::from_millis(180));

    let mut previous = super::capture::capture_screen_region(x, y, width, height)?;
    let mut stitched = previous.clone();
    let mut segments = 1;
    let mut document_offset = 0_u32;
    let mut last_fixed_band = None;
    let wheel_lparam = pack_screen_point(center_x, center_y);

    let mut scroll_steps = 0_usize;
    let capture_result = (|| {
        for _ in 1..MAX_SCROLL_SEGMENTS {
            if cancelled.load(Ordering::Acquire)
                || unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE.0)) } < 0
            {
                return Err("长截图已取消".to_owned());
            }
            let wheel_wparam = pack_wheel_delta(-480);
            // SAFETY: target was returned by WindowFromPoint. The message contains
            // only ordinary mouse-wheel coordinates and no borrowed pointers.
            unsafe { PostMessageW(Some(target), WM_MOUSEWHEEL, wheel_wparam, wheel_lparam) }
                .map_err(|error| format!("无法向目标窗口发送滚轮消息：{error}"))?;
            scroll_steps += 1;
            let next = capture_stable_region(x, y, width, height, cancelled)?;
            if image_difference(&previous, &next, 0).unwrap_or(f64::MAX) <= DUPLICATE_SCORE {
                break;
            }

            let Some((offset, score)) = find_vertical_offset(&previous, &next) else {
                if segments == 1 {
                    return Err(
                        "未找到相邻画面的重叠区域；请缩小滚动速度或选择静态内容区域".to_owned()
                    );
                }
                break;
            };
            if score > MAX_OVERLAP_SCORE {
                if segments == 1 {
                    return Err(
                        "滚动画面变化过大，无法可靠拼接；请避开视频、动画或悬浮内容".to_owned()
                    );
                }
                break;
            }
            let next_height = stitched
                .height()
                .checked_add(offset)
                .ok_or_else(|| "滚动截图高度溢出".to_owned())?;
            if next_height > MAX_SCROLL_IMAGE_HEIGHT {
                break;
            }
            if u64::from(stitched.width()) * u64::from(next_height) > MAX_SCROLL_IMAGE_PIXELS {
                break;
            }
            let fixed_band = find_bottom_fixed_band(&previous, &next, offset);
            if let Some((start_y, end_y)) = fixed_band {
                patch_fixed_rows(
                    &mut stitched,
                    &next,
                    document_offset,
                    offset,
                    start_y,
                    end_y,
                )?;
            }
            stitched = append_scrolled_frame(stitched, &next, offset)?;
            previous = next;
            document_offset = document_offset
                .checked_add(offset)
                .ok_or_else(|| "滚动截图文档偏移溢出".to_owned())?;
            last_fixed_band = fixed_band;
            segments += 1;
        }

        if segments == 1 {
            return Err("目标窗口没有发生可识别的滚动，请确认选区位于可滚动内容上".to_owned());
        }
        Ok(ScrollCaptureOutput {
            image: stitched,
            segments,
        })
    })();

    // Keep the target application close to its original scroll position even
    // when stitching fails or the user cancels. Wheel messages are symmetric,
    // so this is best-effort for applications with custom scroll acceleration.
    for _ in 0..scroll_steps {
        let _ = unsafe {
            PostMessageW(
                Some(target),
                WM_MOUSEWHEEL,
                pack_wheel_delta(480),
                wheel_lparam,
            )
        };
        thread::sleep(Duration::from_millis(12));
    }
    let capture_result = capture_result?;
    let image = if let Some((start_y, _)) = last_fixed_band {
        let visible_height = document_offset
            .checked_add(start_y)
            .filter(|height| *height > 0 && *height < capture_result.image.height());
        if let Some(visible_height) = visible_height {
            image::imageops::crop_imm(
                &capture_result.image,
                0,
                0,
                capture_result.image.width(),
                visible_height,
            )
            .to_image()
        } else {
            capture_result.image
        }
    } else {
        capture_result.image
    };
    Ok(ScrollCaptureOutput {
        image,
        segments: capture_result.segments,
    })
}

#[cfg(windows)]
fn pack_wheel_delta(delta: i16) -> windows::Win32::Foundation::WPARAM {
    windows::Win32::Foundation::WPARAM(usize::from(delta as u16) << 16)
}

#[cfg(windows)]
fn pack_screen_point(x: i32, y: i32) -> windows::Win32::Foundation::LPARAM {
    let packed = u32::from(x as u16) | (u32::from(y as u16) << 16);
    windows::Win32::Foundation::LPARAM(packed as isize)
}

#[cfg(not(windows))]
pub(super) fn capture_scrolling_region(
    _x: i32,
    _y: i32,
    _width: u32,
    _height: u32,
    _cancelled: &AtomicBool,
) -> Result<ScrollCaptureOutput, String> {
    Err("滚动截图目前仅支持 Windows".to_owned())
}

#[cfg(windows)]
fn capture_stable_region(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    cancelled: &AtomicBool,
) -> Result<RgbaImage, String> {
    use std::{thread, time::Duration};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};

    thread::sleep(Duration::from_millis(120));
    let mut previous = super::capture::capture_screen_region(x, y, width, height)?;
    for _ in 0..7 {
        if cancelled.load(Ordering::Acquire)
            || unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE.0)) } < 0
        {
            return Err("长截图已取消".to_owned());
        }
        thread::sleep(Duration::from_millis(90));
        let current = super::capture::capture_screen_region(x, y, width, height)?;
        if image_difference(&previous, &current, 0).unwrap_or(f64::MAX) <= DUPLICATE_SCORE {
            return Ok(current);
        }
        previous = current;
    }
    Ok(previous)
}

fn find_vertical_offset(previous: &RgbaImage, next: &RgbaImage) -> Option<(u32, f64)> {
    if previous.dimensions() != next.dimensions() || previous.height() < 16 {
        return None;
    }
    let height = previous.height();
    let minimum_offset = (height / 20).max(4);
    let maximum_offset = (height * 4 / 5).max(minimum_offset);
    let coarse_step = (height / 160).max(2);
    let mut best: Option<(u32, f64)> = None;

    let mut offset = minimum_offset;
    while offset <= maximum_offset {
        if let Some(score) = image_difference(previous, next, offset)
            && best.is_none_or(|(_, best_score)| score < best_score)
        {
            best = Some((offset, score));
        }
        offset = offset.saturating_add(coarse_step);
        if offset == u32::MAX {
            break;
        }
    }

    let (coarse_offset, _) = best?;
    let refine_start = coarse_offset
        .saturating_sub(coarse_step)
        .max(minimum_offset);
    let refine_end = coarse_offset
        .saturating_add(coarse_step)
        .min(maximum_offset);
    for candidate in refine_start..=refine_end {
        if let Some(score) = image_difference(previous, next, candidate)
            && best.is_none_or(|(_, best_score)| score < best_score)
        {
            best = Some((candidate, score));
        }
    }
    best
}

/// Compares `previous[offset..]` with `next[..height-offset]`.
fn image_difference(previous: &RgbaImage, next: &RgbaImage, offset: u32) -> Option<f64> {
    if previous.dimensions() != next.dimensions() || offset >= previous.height() {
        return None;
    }
    let overlap_height = previous.height() - offset;
    let vertical_margin = (overlap_height / 10).min(48);
    let start_y = vertical_margin;
    let end_y = overlap_height.saturating_sub(vertical_margin);
    if end_y <= start_y {
        return None;
    }
    let horizontal_margin = (previous.width() / 40).min(32);
    let usable_width = previous.width().saturating_sub(horizontal_margin * 2);
    if usable_width < 5 {
        return None;
    }
    let band_width = (usable_width / 5).max(1);
    let mut band_scores = Vec::with_capacity(5);
    for band in 0..5 {
        let start_x = horizontal_margin + band * band_width;
        let end_x = if band == 4 {
            previous.width() - horizontal_margin
        } else {
            (start_x + band_width).min(previous.width() - horizontal_margin)
        };
        if let Some(score) =
            image_band_difference(previous, next, offset, start_y, end_y, start_x, end_x)
        {
            band_scores.push(score);
        }
    }
    band_scores.sort_by(f64::total_cmp);
    band_scores.get(band_scores.len() / 2).copied()
}

#[allow(clippy::too_many_arguments)]
fn image_band_difference(
    previous: &RgbaImage,
    next: &RgbaImage,
    offset: u32,
    start_y: u32,
    end_y: u32,
    start_x: u32,
    end_x: u32,
) -> Option<f64> {
    if end_x <= start_x || end_y <= start_y {
        return None;
    }
    let overlap_height = previous.height() - offset;
    let x_step = ((end_x - start_x) / 36).max(1) as usize;
    let y_step = (overlap_height / 120).max(1) as usize;
    let mut difference = 0_u64;
    let mut samples = 0_u64;

    for y in (start_y..end_y).step_by(y_step) {
        for x in (start_x..end_x).step_by(x_step) {
            let left = previous.get_pixel(x, y + offset).0;
            let right = next.get_pixel(x, y).0;
            difference += u64::from(left[0].abs_diff(right[0]));
            difference += u64::from(left[1].abs_diff(right[1]));
            difference += u64::from(left[2].abs_diff(right[2]));
            samples += 3;
        }
    }
    (samples > 0).then_some(difference as f64 / samples as f64)
}

fn find_bottom_fixed_band(
    previous: &RgbaImage,
    next: &RgbaImage,
    offset: u32,
) -> Option<(u32, u32)> {
    if previous.dimensions() != next.dimensions() || offset == 0 || offset >= previous.height() {
        return None;
    }
    let height = previous.height();
    let scan_start = height
        .saturating_sub(offset)
        .saturating_sub(FIXED_ROW_SEARCH_MARGIN);
    let mut run_start = None;
    for y in scan_start..height {
        let same_screen = row_difference(previous, next, y, y);
        let aligned = if y >= offset {
            row_difference(previous, next, y, y - offset)
        } else {
            None
        };
        let is_fixed = same_screen.is_some_and(|same| same <= FIXED_ROW_SAME_SCREEN_SCORE)
            && aligned.is_some_and(|score| score >= FIXED_ROW_ALIGNED_SCORE);
        if is_fixed {
            run_start.get_or_insert(y);
        } else {
            run_start = None;
        }
    }
    if let Some(start) = run_start
        && height.saturating_sub(start) >= FIXED_ROW_MIN_RUN
    {
        return Some((start, height));
    }
    None
}

fn row_difference(
    previous: &RgbaImage,
    next: &RgbaImage,
    previous_y: u32,
    next_y: u32,
) -> Option<f64> {
    if previous.width() != next.width()
        || previous_y >= previous.height()
        || next_y >= next.height()
    {
        return None;
    }
    let width = previous.width();
    let x_step = ((width / 128).max(1)) as usize;
    let mut difference = 0_u64;
    let mut samples = 0_u64;
    for x in (0..width).step_by(x_step) {
        let left = previous.get_pixel(x, previous_y).0;
        let right = next.get_pixel(x, next_y).0;
        difference += u64::from(left[0].abs_diff(right[0]));
        difference += u64::from(left[1].abs_diff(right[1]));
        difference += u64::from(left[2].abs_diff(right[2]));
        samples += 3;
    }
    (samples > 0).then_some(difference as f64 / samples as f64)
}

fn patch_fixed_rows(
    stitched: &mut RgbaImage,
    next: &RgbaImage,
    document_offset: u32,
    offset: u32,
    start_y: u32,
    end_y: u32,
) -> Result<(), String> {
    if start_y >= end_y || end_y > next.height() || start_y < offset {
        return Ok(());
    }
    let source_start = start_y - offset;
    let row_count = end_y - start_y;
    if source_start
        .checked_add(row_count)
        .is_none_or(|end| end > next.height())
        || document_offset
            .checked_add(end_y)
            .is_none_or(|end| end > stitched.height())
    {
        return Err("滚动截图固定元素回填范围无效".to_owned());
    }
    for (target_y, source_y) in (start_y..end_y)
        .map(|y| document_offset + y)
        .zip(source_start..source_start + row_count)
    {
        for x in 0..next.width() {
            stitched.put_pixel(x, target_y, *next.get_pixel(x, source_y));
        }
    }
    Ok(())
}

fn append_scrolled_frame(
    stitched: RgbaImage,
    next: &RgbaImage,
    offset: u32,
) -> Result<RgbaImage, String> {
    if stitched.width() != next.width() || offset == 0 || offset > next.height() {
        return Err("滚动截图拼接尺寸无效".to_owned());
    }
    let height = stitched
        .height()
        .checked_add(offset)
        .ok_or_else(|| "滚动截图拼接高度溢出".to_owned())?;
    let width = stitched.width();
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| "滚动截图行宽溢出".to_owned())?;
    let first_new_row = usize::try_from(next.height() - offset)
        .ok()
        .and_then(|value| value.checked_mul(row_bytes))
        .ok_or_else(|| "滚动截图像素偏移溢出".to_owned())?;
    let mut pixels = stitched.into_raw();
    pixels.extend_from_slice(&next.as_raw()[first_new_row..]);
    RgbaImage::from_raw(width, height, pixels).ok_or_else(|| "无法创建滚动截图拼接图像".to_owned())
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage, imageops::crop_imm};

    use super::{
        append_scrolled_frame, find_bottom_fixed_band, find_vertical_offset, image_difference,
        patch_fixed_rows,
    };

    fn document(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                ((x * 17 + y * 3) % 251) as u8,
                ((x * 5 + y * 11) % 241) as u8,
                ((x * 13 + y * 7) % 239) as u8,
                255,
            ])
        })
    }

    #[test]
    fn finds_the_vertical_displacement_between_two_viewports() {
        let source = document(120, 500);
        let first = crop_imm(&source, 0, 0, 120, 200).to_image();
        let second = crop_imm(&source, 0, 120, 120, 200).to_image();
        let (offset, score) = find_vertical_offset(&first, &second).unwrap();
        assert_eq!(offset, 120);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn appends_only_rows_that_are_new_after_scrolling() {
        let source = document(120, 500);
        let first = crop_imm(&source, 0, 0, 120, 200).to_image();
        let second = crop_imm(&source, 0, 120, 120, 200).to_image();
        let stitched = append_scrolled_frame(first, &second, 120).unwrap();
        assert_eq!(stitched.dimensions(), (120, 320));
        assert_eq!(stitched, crop_imm(&source, 0, 0, 120, 320).to_image());
    }

    #[test]
    fn duplicate_frames_have_zero_difference() {
        let frame = document(80, 120);
        assert_eq!(image_difference(&frame, &frame, 0), Some(0.0));
    }

    #[test]
    fn detects_and_repairs_a_fixed_bottom_bar_before_stitching() {
        let source = document(120, 500);
        let mut first = crop_imm(&source, 0, 0, 120, 200).to_image();
        let mut second = crop_imm(&source, 0, 120, 120, 200).to_image();
        for frame in [&mut first, &mut second] {
            for y in 190..200 {
                for x in 0..120 {
                    frame.put_pixel(x, y, Rgba([238, 216, 170, 255]));
                }
            }
        }

        let band = find_bottom_fixed_band(&first, &second, 120).unwrap();
        assert_eq!(band, (190, 200));
        let mut stitched = first.clone();
        patch_fixed_rows(&mut stitched, &second, 0, 120, band.0, band.1).unwrap();
        assert_eq!(
            crop_imm(&stitched, 0, 190, 120, 10).to_image(),
            crop_imm(&source, 0, 190, 120, 10).to_image()
        );
    }

    #[test]
    fn does_not_mark_a_normal_scrolling_document_as_fixed() {
        let source = document(120, 500);
        let first = crop_imm(&source, 0, 0, 120, 200).to_image();
        let second = crop_imm(&source, 0, 120, 120, 200).to_image();
        assert_eq!(find_bottom_fixed_band(&first, &second, 120), None);
    }
}

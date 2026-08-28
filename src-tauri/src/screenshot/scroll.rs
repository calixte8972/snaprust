//! Automatic scrolling capture and overlap-based vertical stitching.

use image::RgbaImage;

const MAX_SCROLL_SEGMENTS: usize = 24;
const MAX_SCROLL_IMAGE_HEIGHT: u32 = 40_000;
const MAX_SCROLL_IMAGE_PIXELS: u64 = 60_000_000;
const DUPLICATE_SCORE: f64 = 2.0;
const MAX_OVERLAP_SCORE: f64 = 28.0;

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
) -> Result<ScrollCaptureOutput, String> {
    use std::{thread, time::Duration};

    use windows::Win32::{
        Foundation::POINT,
        UI::WindowsAndMessaging::{
            GA_ROOT, GetAncestor, PostMessageW, SetForegroundWindow, WM_MOUSEWHEEL, WindowFromPoint,
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
    let wheel_lparam = pack_screen_point(center_x, center_y);

    for _ in 1..MAX_SCROLL_SEGMENTS {
        let wheel_wparam = pack_wheel_delta(-480);
        // SAFETY: target was returned by WindowFromPoint. The message contains
        // only ordinary mouse-wheel coordinates and no borrowed pointers.
        unsafe { PostMessageW(Some(target), WM_MOUSEWHEEL, wheel_wparam, wheel_lparam) }
            .map_err(|error| format!("无法向目标窗口发送滚轮消息：{error}"))?;
        thread::sleep(Duration::from_millis(260));

        let next = super::capture::capture_screen_region(x, y, width, height)?;
        if image_difference(&previous, &next, 0).unwrap_or(f64::MAX) <= DUPLICATE_SCORE {
            break;
        }

        let Some((offset, score)) = find_vertical_offset(&previous, &next) else {
            if segments == 1 {
                return Err("未找到相邻画面的重叠区域；请缩小滚动速度或选择静态内容区域".to_owned());
            }
            break;
        };
        if score > MAX_OVERLAP_SCORE {
            if segments == 1 {
                return Err("滚动画面变化过大，无法可靠拼接；请避开视频、动画或悬浮内容".to_owned());
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
        stitched = append_scrolled_frame(stitched, &next, offset)?;
        previous = next;
        segments += 1;
    }

    if segments == 1 {
        return Err("目标窗口没有发生可识别的滚动，请确认选区位于可滚动内容上".to_owned());
    }
    Ok(ScrollCaptureOutput {
        image: stitched,
        segments,
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
) -> Result<ScrollCaptureOutput, String> {
    Err("滚动截图目前仅支持 Windows".to_owned())
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
    let x_step = (previous.width() / 180).max(1) as usize;
    let y_step = (overlap_height / 120).max(1) as usize;
    let mut difference = 0_u64;
    let mut samples = 0_u64;

    for y in (start_y..end_y).step_by(y_step) {
        for x in (0..previous.width()).step_by(x_step) {
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

    use super::{append_scrolled_frame, find_vertical_offset, image_difference};

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
}

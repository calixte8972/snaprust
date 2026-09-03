//! Manual scrolling capture and overlap-based vertical stitching.

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
const FIXED_ROW_MIN_RUN: u32 = 8;
const FIXED_ROW_SEARCH_MARGIN: u32 = 128;
const MANUAL_SCROLL_POLL_INTERVAL_MS: u64 = 75;
const ROW_MATCH_SAMPLES: u32 = 96;
const MIN_ROW_MATCH_VOTES: u32 = 5;
const MIN_SCROLL_OVERLAP_ROWS: u32 = 32;

pub(super) struct ScrollCaptureOutput {
    pub image: RgbaImage,
    pub segments: usize,
}

#[cfg(windows)]
enum StitchFrameResult {
    Accepted,
    NoChange,
    LimitReached,
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
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_RETURN},
            WindowsAndMessaging::{GA_ROOT, GetAncestor, SetForegroundWindow, WindowFromPoint},
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
    let mut segments: usize = 1;
    let mut document_offset = 0_u32;
    let mut last_fixed_band = None;
    let mut pending = None;
    let capture_result = (|| {
        loop {
            if cancelled.load(Ordering::Acquire)
                || unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE.0)) } < 0
            {
                return Err("长截图已取消".to_owned());
            }
            if unsafe { GetAsyncKeyState(i32::from(VK_RETURN.0)) } < 0 {
                // Enter can arrive before the next 75 ms poll. Take one fresh
                // frame here so the last wheel movement is not left out.
                let candidate = if let Some(candidate) = pending.take() {
                    Some(candidate)
                } else {
                    thread::sleep(Duration::from_millis(MANUAL_SCROLL_POLL_INTERVAL_MS));
                    let current = super::capture::capture_screen_region(x, y, width, height)?;
                    (image_difference(&previous, &current, 0).unwrap_or(f64::MAX) > DUPLICATE_SCORE)
                        .then_some(current)
                };
                if let Some(candidate) = candidate {
                    let final_frame =
                        wait_for_stable_manual_frame(x, y, width, height, candidate, cancelled)?;
                    match stitch_manual_frame(
                        &mut previous,
                        &mut stitched,
                        &mut document_offset,
                        &mut last_fixed_band,
                        final_frame,
                    )? {
                        StitchFrameResult::Accepted => {
                            segments = segments.saturating_add(1);
                        }
                        StitchFrameResult::NoChange | StitchFrameResult::LimitReached => {}
                    }
                }
                break;
            }
            if segments >= MAX_SCROLL_SEGMENTS {
                break;
            }

            // The user owns the scroll wheel. We poll the selected region and
            // wait for two equal frames before accepting a manual scroll, which
            // avoids stitching an intermediate smooth-scroll animation frame.
            thread::sleep(Duration::from_millis(MANUAL_SCROLL_POLL_INTERVAL_MS));
            let current = super::capture::capture_screen_region(x, y, width, height)?;
            if let Some(candidate) = pending.take() {
                if image_difference(&candidate, &current, 0).unwrap_or(f64::MAX) > DUPLICATE_SCORE {
                    pending = Some(current);
                    continue;
                }
                match stitch_manual_frame(
                    &mut previous,
                    &mut stitched,
                    &mut document_offset,
                    &mut last_fixed_band,
                    current,
                )? {
                    StitchFrameResult::Accepted => {
                        segments = segments.saturating_add(1);
                    }
                    StitchFrameResult::NoChange => {}
                    StitchFrameResult::LimitReached => break,
                }
            } else if image_difference(&previous, &current, 0).unwrap_or(f64::MAX) > DUPLICATE_SCORE
            {
                pending = Some(current);
            }
        }

        if segments == 1 {
            return Err("尚未记录滚动内容，请先向下滚动至少一次，再按 Enter 完成".to_owned());
        }
        Ok(ScrollCaptureOutput {
            image: stitched,
            segments,
        })
    })();
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
fn wait_for_stable_manual_frame(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    initial: RgbaImage,
    cancelled: &AtomicBool,
) -> Result<RgbaImage, String> {
    use std::{thread, time::Duration};

    let mut candidate = initial;
    for _ in 0..4 {
        if cancelled.load(Ordering::Acquire) {
            return Err("长截图已取消".to_owned());
        }
        thread::sleep(Duration::from_millis(MANUAL_SCROLL_POLL_INTERVAL_MS));
        let current = super::capture::capture_screen_region(x, y, width, height)?;
        if image_difference(&candidate, &current, 0).unwrap_or(f64::MAX) <= DUPLICATE_SCORE {
            return Ok(current);
        }
        candidate = current;
    }
    Ok(candidate)
}

#[cfg(windows)]
fn stitch_manual_frame(
    previous: &mut RgbaImage,
    stitched: &mut RgbaImage,
    document_offset: &mut u32,
    last_fixed_band: &mut Option<(u32, u32)>,
    next: RgbaImage,
) -> Result<StitchFrameResult, String> {
    if image_difference(previous, &next, 0).unwrap_or(f64::MAX) <= DUPLICATE_SCORE {
        return Ok(StitchFrameResult::NoChange);
    }

    let Some((offset, score)) = find_vertical_offset(previous, &next) else {
        return Err("未找到相邻画面的重叠区域；请放慢滚动速度并保持纵向滚动".to_owned());
    };
    if score > MAX_OVERLAP_SCORE {
        return Err("滚动画面变化过大，无法可靠拼接；请一次滚动少一些并避开动画区域".to_owned());
    }
    let next_height = stitched
        .height()
        .checked_add(offset)
        .ok_or_else(|| "滚动截图高度溢出".to_owned())?;
    if next_height > MAX_SCROLL_IMAGE_HEIGHT
        || u64::from(stitched.width()) * u64::from(next_height) > MAX_SCROLL_IMAGE_PIXELS
    {
        return Ok(StitchFrameResult::LimitReached);
    }
    let fixed_band = find_bottom_fixed_band(previous, &next, offset);
    let seam = find_seam(previous, &next, offset);
    if let Some((start_y, end_y)) = fixed_band {
        patch_fixed_rows(stitched, &next, *document_offset, offset, start_y, end_y)?;
    }
    let old_stitched = std::mem::replace(stitched, RgbaImage::new(0, 0));
    *stitched = append_scrolled_frame_at_seam(old_stitched, &next, offset, seam)?;
    *previous = next;
    *document_offset = document_offset
        .checked_add(offset)
        .ok_or_else(|| "滚动截图文档偏移溢出".to_owned())?;
    *last_fixed_band = fixed_band;
    Ok(StitchFrameResult::Accepted)
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

fn find_vertical_offset(previous: &RgbaImage, next: &RgbaImage) -> Option<(u32, f64)> {
    if previous.dimensions() != next.dimensions()
        || previous.height() < MIN_SCROLL_OVERLAP_ROWS.saturating_add(8)
    {
        return None;
    }
    let height = previous.height();
    let minimum_offset = (height / 40).max(4);
    let maximum_offset = height
        .saturating_sub(MIN_SCROLL_OVERLAP_ROWS)
        .max(minimum_offset);
    let width = previous.width();
    let horizontal_margin = (width / 20).max(8).min(width / 3);
    let left = horizontal_margin;
    let right = width.saturating_sub(horizontal_margin);
    if right <= left {
        return None;
    }

    // Build compact row signatures once, then vote on the vertical offset.
    // This is much less sensitive to blank rows and repeated separators than
    // choosing the lowest score for one large rectangular overlap.
    let sample_count = usize::try_from((right - left).min(ROW_MATCH_SAMPLES)).ok()?;
    if sample_count == 0 {
        return None;
    }
    let previous_rows = build_row_signatures(previous, left, right, sample_count);
    let next_rows = build_row_signatures(next, left, right, sample_count);
    let max_offset = usize::try_from(maximum_offset).ok()?;
    let min_offset = usize::try_from(minimum_offset).ok()?;
    let mut votes = vec![0_u32; max_offset.saturating_add(1)];
    let mut errors = vec![0_u64; max_offset.saturating_add(1)];
    let row_match_threshold = u64::try_from(sample_count)
        .ok()?
        .saturating_mul(3)
        .saturating_mul(4);

    for (previous_y, previous_row) in previous_rows.iter().enumerate().skip(min_offset) {
        let first_next_y = previous_y.saturating_sub(max_offset);
        let last_next_y = previous_y.saturating_sub(min_offset);
        if first_next_y > last_next_y {
            continue;
        }
        let mut best_next_y = 0_usize;
        let mut best_error = u64::MAX;
        for (next_y, next_row) in next_rows
            .iter()
            .enumerate()
            .skip(first_next_y)
            .take(last_next_y - first_next_y + 1)
        {
            let error = row_signature_difference(previous_row, next_row);
            if error < best_error {
                best_error = error;
                best_next_y = next_y;
            }
        }
        if best_error <= row_match_threshold {
            let offset = previous_y - best_next_y;
            votes[offset] = votes[offset].saturating_add(1);
            errors[offset] = errors[offset].saturating_add(best_error);
        }
    }

    let minimum_votes = (height / 80).max(MIN_ROW_MATCH_VOTES);
    let mut best: Option<(usize, u32, u64)> = None;
    for offset in min_offset..=max_offset {
        let vote_count = votes[offset];
        if vote_count < minimum_votes {
            continue;
        }
        let total_error = errors[offset];
        if best.is_none_or(|(_, best_votes, best_error)| {
            vote_count > best_votes || (vote_count == best_votes && total_error < best_error)
        }) {
            best = Some((offset, vote_count, total_error));
        }
    }

    let (offset, _, _) = best?;
    let offset = u32::try_from(offset).ok()?;
    let score = image_difference(previous, next, offset)?;
    Some((offset, score))
}

fn build_row_signatures(
    image: &RgbaImage,
    left: u32,
    right: u32,
    sample_count: usize,
) -> Vec<Vec<[u8; 3]>> {
    let usable_width = right.saturating_sub(left);
    (0..image.height())
        .map(|y| {
            (0..sample_count)
                .map(|sample| {
                    let x = if sample_count <= 1 {
                        left
                    } else {
                        let position = (u64::try_from(sample).unwrap_or(0)
                            * u64::from(usable_width.saturating_sub(1)))
                            / u64::try_from(sample_count - 1).unwrap_or(1);
                        left.saturating_add(u32::try_from(position).unwrap_or(u32::MAX))
                    };
                    let pixel = image.get_pixel(x.min(right.saturating_sub(1)), y).0;
                    [pixel[0], pixel[1], pixel[2]]
                })
                .collect()
        })
        .collect()
}

fn row_signature_difference(left: &[[u8; 3]], right: &[[u8; 3]]) -> u64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| {
            u64::from(left[0].abs_diff(right[0]))
                + u64::from(left[1].abs_diff(right[1]))
                + u64::from(left[2].abs_diff(right[2]))
        })
        .sum()
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

fn find_seam(previous: &RgbaImage, next: &RgbaImage, offset: u32) -> u32 {
    if previous.dimensions() != next.dimensions() || offset >= next.height() {
        return next.height().saturating_sub(offset);
    }
    let overlap_height = next.height() - offset;
    if overlap_height < 4 {
        return overlap_height;
    }
    let margin = (overlap_height / 10).min(48);
    let start = margin.max(1);
    let end = overlap_height.saturating_sub(margin).max(start + 1);
    let mut best_row = start;
    let mut best_score = f64::MAX;
    for row in start..end.min(overlap_height) {
        if let Some(score) = row_difference(previous, next, row + offset, row)
            && score < best_score
        {
            best_score = score;
            best_row = row;
        }
    }
    best_row
}

fn append_scrolled_frame_at_seam(
    stitched: RgbaImage,
    next: &RgbaImage,
    offset: u32,
    seam: u32,
) -> Result<RgbaImage, String> {
    if stitched.width() != next.width()
        || offset == 0
        || offset > next.height()
        || seam > next.height() - offset
    {
        return Err("滚动截图拼接尺寸无效".to_owned());
    }
    let old_overlap_after_seam = next.height() - offset - seam;
    let kept_height = stitched
        .height()
        .checked_sub(old_overlap_after_seam)
        .ok_or_else(|| "滚动截图重叠区域超出已拼接图像".to_owned())?;
    let height = kept_height
        .checked_add(next.height() - seam)
        .ok_or_else(|| "滚动截图拼接高度溢出".to_owned())?;
    let width = stitched.width();
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| "滚动截图行宽溢出".to_owned())?;
    let kept_bytes = usize::try_from(kept_height)
        .ok()
        .and_then(|value| value.checked_mul(row_bytes))
        .ok_or_else(|| "滚动截图保留区域溢出".to_owned())?;
    let first_new_row = usize::try_from(seam)
        .ok()
        .and_then(|value| value.checked_mul(row_bytes))
        .ok_or_else(|| "滚动截图像素偏移溢出".to_owned())?;
    let mut pixels = stitched.into_raw();
    if kept_bytes > pixels.len() || first_new_row > next.as_raw().len() {
        return Err("滚动截图像素范围无效".to_owned());
    }
    pixels.truncate(kept_bytes);
    pixels.extend_from_slice(&next.as_raw()[first_new_row..]);
    RgbaImage::from_raw(width, height, pixels).ok_or_else(|| "无法创建滚动截图拼接图像".to_owned())
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage, imageops::crop_imm};

    use super::{
        append_scrolled_frame_at_seam, find_bottom_fixed_band, find_vertical_offset,
        image_difference, patch_fixed_rows,
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
        let stitched = append_scrolled_frame_at_seam(first, &second, 120, 80).unwrap();
        assert_eq!(stitched.dimensions(), (120, 320));
        assert_eq!(stitched, crop_imm(&source, 0, 0, 120, 320).to_image());
    }

    #[test]
    fn appends_using_a_seam_without_dropping_or_duplicating_rows() {
        let source = document(120, 500);
        let first = crop_imm(&source, 0, 0, 120, 200).to_image();
        let second = crop_imm(&source, 0, 120, 120, 200).to_image();
        let stitched = append_scrolled_frame_at_seam(first, &second, 120, 40).unwrap();
        assert_eq!(stitched.dimensions(), (120, 320));
        assert_eq!(stitched, crop_imm(&source, 0, 0, 120, 320).to_image());
    }

    #[test]
    fn finds_a_large_manual_scroll_when_a_small_overlap_remains() {
        let source = document(120, 500);
        let first = crop_imm(&source, 0, 0, 120, 200).to_image();
        let second = crop_imm(&source, 0, 160, 120, 200).to_image();
        let (offset, score) = find_vertical_offset(&first, &second).unwrap();
        assert_eq!(offset, 160);
        assert_eq!(score, 0.0);
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

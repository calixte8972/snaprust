//! Windows image clipboard integration using the native `CF_DIB` format.

use std::{mem::size_of, ptr::copy_nonoverlapping, thread::sleep, time::Duration};

use image::RgbaImage;
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HGLOBAL},
    Graphics::Gdi::{BI_RGB, BITMAPINFOHEADER},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
    },
};

const CF_DIB_FORMAT: u32 = 8;
const CLIPBOARD_RETRY_DELAY: Duration = Duration::from_millis(15);
const CLIPBOARD_ATTEMPTS: usize = 6;

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<Self, String> {
        let mut last_error = None;

        for _ in 0..CLIPBOARD_ATTEMPTS {
            // SAFETY: Passing no owner window is permitted. The matching
            // CloseClipboard call is guaranteed by ClipboardGuard::drop.
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Ok(Self),
                Err(error) => {
                    last_error = Some(error);
                    sleep(CLIPBOARD_RETRY_DELAY);
                }
            }
        }

        Err(format!(
            "could not open the Windows clipboard after {CLIPBOARD_ATTEMPTS} attempts: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "unknown clipboard error".to_owned())
        ))
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: The guard is constructed only after OpenClipboard succeeds.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

struct GlobalMemory {
    handle: Option<HGLOBAL>,
}

impl GlobalMemory {
    fn allocate(size: usize) -> Result<Self, String> {
        // SAFETY: GlobalAlloc receives a checked allocation size and its handle
        // is freed by Drop until clipboard ownership is transferred.
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) }
            .map_err(|error| format!("failed to allocate clipboard memory: {error}"))?;

        Ok(Self {
            handle: Some(handle),
        })
    }

    fn handle(&self) -> HGLOBAL {
        self.handle
            .expect("clipboard memory has not been transferred")
    }

    fn clipboard_handle(&self) -> HANDLE {
        HANDLE(self.handle().0)
    }

    fn relinquish_to_clipboard(&mut self) {
        self.handle.take();
    }
}

impl Drop for GlobalMemory {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: The application owns this global allocation until it is
            // passed successfully to SetClipboardData.
            unsafe {
                let _ = GlobalFree(Some(handle));
            }
        }
    }
}

pub fn write_image(image: &RgbaImage) -> Result<(), String> {
    let width = i32::try_from(image.width())
        .map_err(|_| "selected image width exceeds the DIB limit".to_owned())?;
    let height = i32::try_from(image.height())
        .map_err(|_| "selected image height exceeds the DIB limit".to_owned())?;
    let pixel_len = image
        .width()
        .checked_mul(image.height())
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(|| "selected image is too large for the clipboard".to_owned())?;
    let total_len = size_of::<BITMAPINFOHEADER>()
        .checked_add(pixel_len)
        .ok_or_else(|| "clipboard DIB size overflowed".to_owned())?;

    let mut bgra = image.as_raw().clone();
    rgba_to_bgra(&mut bgra);

    let bitmap_header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        // A negative height stores the DIB in top-down order, matching the
        // captured image buffer without an extra vertical copy.
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let mut global_memory = GlobalMemory::allocate(total_len)?;

    // SAFETY: The global allocation is valid and movable. Lock returns an
    // address valid until GlobalUnlock, and exactly total_len bytes are copied.
    let destination = unsafe { GlobalLock(global_memory.handle()) };
    if destination.is_null() {
        return Err("failed to lock clipboard memory".to_owned());
    }
    unsafe {
        copy_nonoverlapping(
            (&raw const bitmap_header).cast::<u8>(),
            destination.cast::<u8>(),
            size_of::<BITMAPINFOHEADER>(),
        );
        copy_nonoverlapping(
            bgra.as_ptr(),
            destination.cast::<u8>().add(size_of::<BITMAPINFOHEADER>()),
            bgra.len(),
        );
        // GlobalUnlock returns false when this call releases the final lock, so
        // its result cannot distinguish successful unlock from failure here.
        let _ = GlobalUnlock(global_memory.handle());
    }

    let _clipboard = ClipboardGuard::open()?;
    // SAFETY: The clipboard is open for this thread. After SetClipboardData
    // succeeds, Windows owns the global handle and GlobalMemory will not free it.
    unsafe {
        EmptyClipboard().map_err(|error| format!("failed to empty clipboard: {error}"))?;
        SetClipboardData(CF_DIB_FORMAT, Some(global_memory.clipboard_handle()))
            .map_err(|error| format!("failed to set image on clipboard: {error}"))?;
    }
    global_memory.relinquish_to_clipboard();

    Ok(())
}

fn rgba_to_bgra(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = u8::MAX;
    }
}

#[cfg(test)]
mod tests {
    use super::rgba_to_bgra;

    #[test]
    fn converts_rgba_pixels_to_opaque_windows_bgra() {
        let mut pixels = [30, 20, 10, 1, 60, 50, 40, 127];

        rgba_to_bgra(&mut pixels);

        assert_eq!(pixels, [10, 20, 30, 255, 40, 50, 60, 255]);
    }
}

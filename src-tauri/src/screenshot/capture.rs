//! Windows screen capture and image cropping.

use std::{ffi::c_void, mem::size_of, ptr::null_mut, slice};

use image::RgbaImage;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY,
    SelectObject,
};

use super::monitor::VirtualDesktop;

struct ScreenDc(HDC);

impl ScreenDc {
    fn acquire() -> Result<Self, String> {
        // SAFETY: A null HWND requests the DC for the entire screen. The handle
        // is paired with ReleaseDC in Drop.
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            Err("failed to acquire the Windows screen device context".to_owned())
        } else {
            Ok(Self(dc))
        }
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: This is the screen DC acquired by GetDC(None), released once.
        unsafe {
            ReleaseDC(None, self.0);
        }
    }
}

struct MemoryDc(HDC);

impl MemoryDc {
    fn create(compatible_with: HDC) -> Result<Self, String> {
        // SAFETY: The supplied screen DC is valid for the duration of this call.
        let dc = unsafe { CreateCompatibleDC(Some(compatible_with)) };
        if dc.is_invalid() {
            Err("failed to create a compatible Windows device context".to_owned())
        } else {
            Ok(Self(dc))
        }
    }
}

impl Drop for MemoryDc {
    fn drop(&mut self) {
        // SAFETY: This DC was created by CreateCompatibleDC and is deleted once.
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

struct SelectedBitmap {
    dc: HDC,
    previous: HGDIOBJ,
    bitmap: HBITMAP,
}

impl SelectedBitmap {
    fn select(dc: HDC, bitmap: HBITMAP) -> Result<Self, String> {
        // SAFETY: Both handles are valid GDI handles. Drop restores the previous
        // object before deleting the bitmap.
        let previous = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
        if previous.is_invalid() {
            // SAFETY: Selection failed, so the bitmap is still owned solely here.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
            Err("failed to select the capture bitmap into the device context".to_owned())
        } else {
            Ok(Self {
                dc,
                previous,
                bitmap,
            })
        }
    }
}

impl Drop for SelectedBitmap {
    fn drop(&mut self) {
        // SAFETY: Restore the exact object returned by SelectObject before
        // deleting the bitmap that this guard owns.
        unsafe {
            let _ = SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
        }
    }
}

pub fn capture_virtual_desktop(desktop: &VirtualDesktop) -> Result<RgbaImage, String> {
    let width = i32::try_from(desktop.width)
        .map_err(|_| "virtual desktop width exceeds the Windows GDI limit".to_owned())?;
    let height = i32::try_from(desktop.height)
        .map_err(|_| "virtual desktop height exceeds the Windows GDI limit".to_owned())?;
    let byte_len = usize::try_from(desktop.width)
        .ok()
        .zip(usize::try_from(desktop.height).ok())
        .and_then(|(width, height)| width.checked_mul(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "screen capture buffer size overflowed".to_owned())?;

    let screen_dc = ScreenDc::acquire()?;
    let memory_dc = MemoryDc::create(screen_dc.0)?;
    let mut bits: *mut c_void = null_mut();
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // A negative height creates a top-down DIB, matching browser/image
            // coordinates without a later vertical flip.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    // SAFETY: bitmap_info is initialized for a 32-bit top-down DIB. `bits`
    // receives storage owned by the returned HBITMAP and remains valid until
    // SelectedBitmap is dropped.
    let bitmap = unsafe {
        CreateDIBSection(
            Some(screen_dc.0),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )
    }
    .map_err(|error| format!("failed to allocate the screen capture bitmap: {error}"))?;

    if bits.is_null() {
        // SAFETY: CreateDIBSection returned the bitmap, so it is ours to delete.
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return Err("Windows returned a null screen capture buffer".to_owned());
    }

    let _selected_bitmap = SelectedBitmap::select(memory_dc.0, bitmap)?;

    // SAFETY: Both DCs and dimensions are valid. The destination bitmap is
    // selected into memory_dc and large enough for exactly width x height pixels.
    unsafe {
        BitBlt(
            memory_dc.0,
            0,
            0,
            width,
            height,
            Some(screen_dc.0),
            desktop.x,
            desktop.y,
            SRCCOPY | CAPTUREBLT,
        )
    }
    .map_err(|error| format!("failed to copy screen pixels: {error}"))?;

    // SAFETY: CreateDIBSection allocated byte_len bytes for this 32-bit DIB and
    // the bitmap guard keeps that memory alive while the slice is copied.
    let mut rgba = unsafe { slice::from_raw_parts(bits.cast::<u8>(), byte_len) }.to_vec();
    bgra_to_rgba(&mut rgba);

    RgbaImage::from_raw(desktop.width, desktop.height, rgba)
        .ok_or_else(|| "failed to construct the captured image buffer".to_owned())
}

fn bgra_to_rgba(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = u8::MAX;
    }
}

#[cfg(test)]
mod tests {
    use super::bgra_to_rgba;

    #[test]
    fn converts_windows_bgra_pixels_to_opaque_rgba() {
        let mut pixels = [10, 20, 30, 0, 40, 50, 60, 128];

        bgra_to_rgba(&mut pixels);

        assert_eq!(pixels, [30, 20, 10, 255, 60, 50, 40, 255]);
    }
}

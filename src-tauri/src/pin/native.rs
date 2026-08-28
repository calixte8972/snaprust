use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use image::{
    RgbaImage,
    imageops::{rotate90, rotate270},
};
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, CreateSolidBrush, DEFAULT_GUI_FONT,
            DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW,
            EndPaint, FillRect, GetMonitorInfoW, GetStockObject, HALFTONE, HGDIOBJ, InvalidateRect,
            MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, PAINTSTRUCT, SRCCOPY,
            SelectObject, SetBkMode, SetBrushOrgEx, SetStretchBltMode, SetTextColor, StretchDIBits,
            TRANSPARENT, UpdateWindow,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{ReleaseCapture, VK_ESCAPE},
            WindowsAndMessaging::{
                AppendMenuW, CREATESTRUCTW, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CreatePopupMenu,
                CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
                GWLP_USERDATA, GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW,
                GetWindowRect, IDC_SIZEALL, KillTimer, LWA_ALPHA, LWA_COLORKEY, LoadCursorW,
                MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
                SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW,
                SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW,
                SetWindowPos, ShowWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_TOPALIGN,
                TrackPopupMenu, TranslateMessage, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY,
                WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_MOUSEWHEEL,
                WM_MOVE, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_QUIT, WM_SIZE,
                WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::{Error, PCWSTR, w},
};

use super::{PinnedImage, clamp_native_axis};

const MIN_VISIBLE_EDGE: i32 = 64;
const MIN_OPACITY: f64 = 0.2;
const MAX_ZOOM: f64 = 5.0;
const WINDOW_CLASS: PCWSTR = w!("SnapRustNativePinWindow");
const WINDOW_TITLE: PCWSTR = w!("SnapRust 钉图");
const HUD_WINDOW_CLASS: PCWSTR = w!("SnapRustNativePinHudWindow");
const HUD_WIDTH: i32 = 112;
const HUD_HEIGHT: i32 = 20;
const HUD_GAP: i32 = 4;
// The HUD uses a color-keyed layered window. Keep its transparent background
// distinct from the requested black text, otherwise Windows would key the text
// out together with the background.
const HUD_TRANSPARENT_COLOR: COLORREF = COLORREF(0x00ff_00ff);
const HUD_TEXT_COLOR: COLORREF = COLORREF(0x0000_0000);
const HUD_SHADOW_COLOR: COLORREF = COLORREF(0x00ff_ffff);
const MENU_COPY: usize = 1;
const MENU_ROTATE_LEFT: usize = 2;
const MENU_ROTATE_RIGHT: usize = 3;
const MENU_RESET_VIEW: usize = 4;
const MENU_CLOSE: usize = 5;

static WINDOW_CLASS_RESULT: OnceLock<Result<(), String>> = OnceLock::new();
static NATIVE_WINDOWS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

fn native_windows() -> &'static Mutex<HashMap<String, usize>> {
    NATIVE_WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct NativePinState {
    label: String,
    pixels: Vec<u8>,
    source_width: i32,
    source_height: i32,
    base_width: i32,
    base_height: i32,
    zoom: f64,
    opacity: f64,
    hud_hwnd: usize,
    hud_text: String,
    images: Arc<Mutex<HashMap<String, PinnedImage>>>,
}

struct NativePinCreatePayload {
    state: Option<Box<NativePinState>>,
}

pub fn create(
    label: String,
    image: RgbaImage,
    images: Arc<Mutex<HashMap<String, PinnedImage>>>,
    initial_width: i32,
    initial_height: i32,
) -> Result<(), String> {
    let source_width = i32::try_from(image.width())
        .map_err(|_| "native pin source width is too large".to_owned())?;
    let source_height = i32::try_from(image.height())
        .map_err(|_| "native pin source height is too large".to_owned())?;
    let mut pixels = image.into_raw();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }

    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let cancel_requested = Arc::new(AtomicBool::new(false));
    let thread_cancel_requested = Arc::clone(&cancel_requested);
    let thread_label = label.clone();
    thread::Builder::new()
        .name(format!("snaprust-{label}"))
        .spawn(move || {
            let result = run_window_thread(
                thread_label,
                pixels,
                source_width,
                source_height,
                initial_width,
                initial_height,
                images,
                &result_tx,
                &thread_cancel_requested,
            );
            if let Err(error) = result {
                let _ = result_tx.send(Err(error));
            }
        })
        .map_err(|error| format!("failed to start native pin thread: {error}"))?;

    match result_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(result) => result,
        Err(error) => {
            cancel_requested.store(true, Ordering::Release);
            let _ = close(&label);
            Err(format!("native pin window did not start in time: {error}"))
        }
    }
}

pub fn close(label: &str) -> Result<(), String> {
    let hwnd = native_windows()
        .lock()
        .map_err(|_| "native pin registry lock is poisoned".to_owned())?
        .get(label)
        .copied()
        .ok_or_else(|| format!("native pin window does not exist: {label}"))?;
    unsafe {
        PostMessageW(
            Some(HWND(hwnd as *mut c_void)),
            WM_CLOSE,
            WPARAM(0),
            LPARAM(0),
        )
    }
    .map_err(|error| format!("failed to close native pin window: {error}"))
}

#[allow(clippy::too_many_arguments)]
fn run_window_thread(
    label: String,
    pixels: Vec<u8>,
    source_width: i32,
    source_height: i32,
    initial_width: i32,
    initial_height: i32,
    images: Arc<Mutex<HashMap<String, PinnedImage>>>,
    result_tx: &mpsc::SyncSender<Result<(), String>>,
    cancel_requested: &AtomicBool,
) -> Result<(), String> {
    register_window_class()?;
    if cancel_requested.load(Ordering::Acquire) {
        return Err("native pin window creation was cancelled".to_owned());
    }
    let (x, y) = centered_position(initial_width, initial_height);
    let state = Box::new(NativePinState {
        label: label.clone(),
        pixels,
        source_width,
        source_height,
        base_width: initial_width,
        base_height: initial_height,
        zoom: 1.0,
        opacity: 1.0,
        hud_hwnd: 0,
        hud_text: String::new(),
        images,
    });
    let mut create_payload = NativePinCreatePayload { state: Some(state) };
    let module = unsafe { GetModuleHandleW(None) }
        .map_err(|error| format!("failed to get SnapRust module handle: {error}"))?;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            WINDOW_CLASS,
            WINDOW_TITLE,
            WS_POPUP,
            x,
            y,
            initial_width,
            initial_height,
            None,
            None,
            Some(HINSTANCE(module.0)),
            Some((&raw mut create_payload).cast()),
        )
    }
    .map_err(|error| format!("failed to create native pin window: {error}"))?;

    if cancel_requested.load(Ordering::Acquire) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err("native pin window creation was cancelled".to_owned());
    }

    let state_pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut NativePinState;
    if state_pointer.is_null() {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err("native pin state was not attached to the window".to_owned());
    }
    let hud_hwnd = match create_hud_window(hwnd, state_pointer) {
        Ok(window) => window,
        Err(error) => {
            let _ = unsafe { DestroyWindow(hwnd) };
            return Err(error);
        }
    };
    unsafe { (*state_pointer).hud_hwnd = hud_hwnd.0 as usize };

    if cancel_requested.load(Ordering::Acquire) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err("native pin window creation was cancelled".to_owned());
    }

    native_windows()
        .lock()
        .map_err(|_| "native pin registry lock is poisoned".to_owned())?
        .insert(label, hwnd.0 as usize);
    if cancel_requested.load(Ordering::Acquire) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err("native pin window creation was cancelled".to_owned());
    }
    unsafe {
        if let Err(error) = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA) {
            let _ = DestroyWindow(hwnd);
            return Err(format!("failed to initialize native pin opacity: {error}"));
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        let _ = SetForegroundWindow(hwnd);
        show_hud(hwnd, &mut *state_pointer);
    }
    if result_tx.send(Ok(())).is_err() {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err("failed to report native pin readiness".to_owned());
    }

    let mut message = MSG::default();
    loop {
        let status = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if status.0 == -1 {
            let _ = unsafe { DestroyWindow(hwnd) };
            return Err(format!(
                "native pin message loop failed: {}",
                Error::from_win32()
            ));
        }
        if !status.as_bool() || message.message == WM_QUIT {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn register_window_class() -> Result<(), String> {
    WINDOW_CLASS_RESULT
        .get_or_init(|| {
            let module = unsafe { GetModuleHandleW(None) }
                .map_err(|error| format!("failed to get module handle: {error}"))?;
            let cursor = unsafe { LoadCursorW(None, IDC_SIZEALL) }
                .map_err(|error| format!("failed to load native pin cursor: {error}"))?;
            let class = WNDCLASSW {
                style: CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: HINSTANCE(module.0),
                hCursor: cursor,
                lpszClassName: WINDOW_CLASS,
                ..Default::default()
            };
            if unsafe { RegisterClassW(&class) } == 0 {
                return Err(format!(
                    "failed to register native pin window class: {}",
                    Error::from_win32()
                ));
            }
            let hud_class = WNDCLASSW {
                lpfnWndProc: Some(hud_window_proc),
                hInstance: HINSTANCE(module.0),
                lpszClassName: HUD_WINDOW_CLASS,
                ..Default::default()
            };
            if unsafe { RegisterClassW(&hud_class) } == 0 {
                return Err(format!(
                    "failed to register native pin HUD class: {}",
                    Error::from_win32()
                ));
            }
            Ok(())
        })
        .clone()
}

fn create_hud_window(owner: HWND, state_pointer: *mut NativePinState) -> Result<HWND, String> {
    let module = unsafe { GetModuleHandleW(None) }
        .map_err(|error| format!("failed to get HUD module handle: {error}"))?;
    let hud = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            HUD_WINDOW_CLASS,
            w!(""),
            WS_POPUP,
            0,
            0,
            HUD_WIDTH,
            HUD_HEIGHT,
            Some(owner),
            None,
            Some(HINSTANCE(module.0)),
            Some(state_pointer.cast()),
        )
    }
    .map_err(|error| format!("failed to create native pin HUD: {error}"))?;
    unsafe {
        SetLayeredWindowAttributes(hud, HUD_TRANSPARENT_COLOR, 255, LWA_COLORKEY | LWA_ALPHA)
            .map_err(|error| format!("failed to initialize native pin HUD: {error}"))?;
    }
    Ok(hud)
}

fn centered_position(width: i32, height: i32) -> (i32, i32) {
    let mut cursor = POINT::default();
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut cursor) };
    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let work = info.rcWork;
        return (
            work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - height) / 2,
        );
    }
    (0, 0)
}

unsafe fn state_from_window(hwnd: HWND) -> Option<&'static mut NativePinState> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut NativePinState;
    unsafe { pointer.as_mut() }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            let payload = unsafe { &mut *(create.lpCreateParams.cast::<NativePinCreatePayload>()) };
            let Some(state) = payload.state.take() else {
                return LRESULT(0);
            };
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize) };
            LRESULT(1)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_CONTEXTMENU => {
            if let Some(state) = unsafe { state_from_window(hwnd) } {
                unsafe { show_context_menu(hwnd, state, lparam) };
            }
            LRESULT(0)
        }
        WM_PAINT => {
            if let Some(state) = unsafe { state_from_window(hwnd) } {
                unsafe { paint(hwnd, state) };
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_MOUSEWHEEL => {
            if let Some(state) = unsafe { state_from_window(hwnd) } {
                let key_flags = (wparam.0 & 0xffff) as u16;
                let wheel_delta = ((wparam.0 >> 16) as u16 as i16) as f64;
                if key_flags & 0x0004 != 0 {
                    let change = if wheel_delta > 0.0 { 0.05 } else { -0.05 };
                    state.opacity = (state.opacity + change).clamp(MIN_OPACITY, 1.0);
                    let alpha = (state.opacity * 255.0).round() as u8;
                    let _ =
                        unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA) };
                    unsafe { show_hud(hwnd, state) };
                } else {
                    let cursor = POINT {
                        x: (lparam.0 as u32 & 0xffff) as u16 as i16 as i32,
                        y: ((lparam.0 as u32 >> 16) & 0xffff) as u16 as i16 as i32,
                    };
                    unsafe { resize_around(hwnd, state, cursor, wheel_delta / 120.0) };
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let _ = unsafe { ReleaseCapture() };
            unsafe {
                SendMessageW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::WM_NCLBUTTONDOWN,
                    Some(WPARAM(2)),
                    Some(LPARAM(0)),
                )
            };
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == usize::from(VK_ESCAPE.0) {
                let _ = unsafe { DestroyWindow(hwnd) };
            } else if wparam.0 == usize::from(b'0') {
                if let Some(state) = unsafe { state_from_window(hwnd) } {
                    let mut rect = RECT::default();
                    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
                        let center = POINT {
                            x: rect.left + (rect.right - rect.left) / 2,
                            y: rect.top + (rect.bottom - rect.top) / 2,
                        };
                        unsafe { reset_view(hwnd, state, center) };
                    }
                }
            } else if (wparam.0 == 0xdb || wparam.0 == 0xdd)
                && let Some(state) = unsafe { state_from_window(hwnd) }
            {
                let change = if wparam.0 == 0xdd { 0.05 } else { -0.05 };
                state.opacity = (state.opacity + change).clamp(MIN_OPACITY, 1.0);
                let alpha = (state.opacity * 255.0).round() as u8;
                let _ = unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA) };
                unsafe { show_hud(hwnd, state) };
            }
            LRESULT(0)
        }
        WM_MOVE | WM_SIZE => {
            if let Some(state) = unsafe { state_from_window(hwnd) }
                && state.hud_hwnd != 0
            {
                unsafe { position_hud_window(hwnd, state) };
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_TIMER => {
            if wparam.0 == 1 {
                if let Some(state) = unsafe { state_from_window(hwnd) } {
                    let _ = unsafe { KillTimer(Some(hwnd), 1) };
                    if state.hud_hwnd != 0 {
                        let _ = unsafe { ShowWindow(HWND(state.hud_hwnd as *mut c_void), SW_HIDE) };
                    }
                }
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut NativePinState;
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            if !pointer.is_null() {
                let state = unsafe { Box::from_raw(pointer) };
                if state.hud_hwnd != 0 {
                    let _ = unsafe { DestroyWindow(HWND(state.hud_hwnd as *mut c_void)) };
                }
                if let Ok(mut registry) = native_windows().lock() {
                    registry.remove(&state.label);
                }
                if let Ok(mut images) = state.images.lock() {
                    images.remove(&state.label);
                }
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe extern "system" fn hud_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            }
            LRESULT(1)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_NCHITTEST => LRESULT(-1),
        WM_PAINT => {
            let state = unsafe { state_from_window(hwnd) };
            unsafe { paint_hud_window(hwnd, state.as_deref()) };
            LRESULT(0)
        }
        WM_NCDESTROY => {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn paint_hud_window(hwnd: HWND, state: Option<&NativePinState>) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    let mut client = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client) }.is_ok() {
        let background = unsafe { CreateSolidBrush(HUD_TRANSPARENT_COLOR) };
        unsafe {
            FillRect(hdc, &client, background);
            let _ = DeleteObject(HGDIOBJ(background.0));
        }
        if let Some(state) = state {
            let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
            let previous_font = unsafe { SelectObject(hdc, font) };
            unsafe {
                SetBkMode(hdc, TRANSPARENT);
            }
            let mut shadow_rect = client;
            shadow_rect.left += 1;
            shadow_rect.top += 1;
            shadow_rect.right += 1;
            shadow_rect.bottom += 1;
            let mut shadow_text: Vec<u16> = state.hud_text.encode_utf16().collect();
            unsafe {
                SetTextColor(hdc, HUD_SHADOW_COLOR);
                DrawTextW(
                    hdc,
                    &mut shadow_text,
                    &mut shadow_rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
            let mut text: Vec<u16> = state.hud_text.encode_utf16().collect();
            unsafe {
                SetTextColor(hdc, HUD_TEXT_COLOR);
                DrawTextW(
                    hdc,
                    &mut text,
                    &mut client,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                SelectObject(hdc, previous_font);
            }
        }
    }
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

unsafe fn paint(hwnd: HWND, state: &NativePinState) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    let mut client = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client) }.is_ok() {
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: state.source_width,
                biHeight: -state.source_height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: (state.source_width * state.source_height * 4) as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            SetStretchBltMode(hdc, HALFTONE);
            let _ = SetBrushOrgEx(hdc, 0, 0, None);
            StretchDIBits(
                hdc,
                0,
                0,
                client.right - client.left,
                client.bottom - client.top,
                0,
                0,
                state.source_width,
                state.source_height,
                Some(state.pixels.as_ptr().cast()),
                &bitmap_info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
        }
    }
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

unsafe fn show_context_menu(hwnd: HWND, state: &mut NativePinState, lparam: LPARAM) {
    let menu = match unsafe { CreatePopupMenu() } {
        Ok(menu) => menu,
        Err(error) => {
            state.hud_text = format!("菜单失败：{error}");
            unsafe { show_hud(hwnd, state) };
            return;
        }
    };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, MENU_COPY, w!("复制图片")) };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, MENU_ROTATE_LEFT, w!("向左旋转 90°")) };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, MENU_ROTATE_RIGHT, w!("向右旋转 90°")) };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, MENU_RESET_VIEW, w!("重置视图")) };
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()) };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, MENU_CLOSE, w!("销毁钉图")) };

    let mut point = POINT {
        x: (lparam.0 as u32 & 0xffff) as u16 as i16 as i32,
        y: ((lparam.0 as u32 >> 16) & 0xffff) as u16 as i16 as i32,
    };
    if point.x == -1 && point.y == -1 {
        let _ = unsafe { GetCursorPos(&mut point) };
    }
    let command = unsafe {
        TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD,
            point.x,
            point.y,
            Some(0),
            hwnd,
            None,
        )
    }
    .0 as usize;
    let _ = unsafe { DestroyMenu(menu) };

    match command {
        MENU_COPY => {
            if let Err(error) = copy_pin_image(state) {
                state.hud_text = format!("复制失败：{error}");
            } else {
                state.hud_text = "已复制图片".to_owned();
            }
            unsafe { show_hud(hwnd, state) };
        }
        MENU_ROTATE_LEFT => unsafe { rotate_pin(hwnd, state, false) },
        MENU_ROTATE_RIGHT => unsafe { rotate_pin(hwnd, state, true) },
        MENU_RESET_VIEW => {
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
                let center = POINT {
                    x: rect.left + (rect.right - rect.left) / 2,
                    y: rect.top + (rect.bottom - rect.top) / 2,
                };
                unsafe { reset_view(hwnd, state, center) };
            }
        }
        MENU_CLOSE => {
            let _ = unsafe { DestroyWindow(hwnd) };
        }
        _ => {}
    }
}

fn copy_pin_image(state: &NativePinState) -> Result<(), String> {
    let width = u32::try_from(state.source_width).map_err(|_| "钉图宽度无效".to_owned())?;
    let height = u32::try_from(state.source_height).map_err(|_| "钉图高度无效".to_owned())?;
    let mut image = RgbaImage::from_raw(width, height, state.pixels.clone())
        .ok_or_else(|| "钉图像素尺寸无效".to_owned())?;
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    crate::clipboard::write_image(&image)
}

unsafe fn rotate_pin(hwnd: HWND, state: &mut NativePinState, clockwise: bool) {
    let Some(image) = RgbaImage::from_raw(
        u32::try_from(state.source_width).unwrap_or(0),
        u32::try_from(state.source_height).unwrap_or(0),
        state.pixels.clone(),
    ) else {
        state.hud_text = "旋转失败：钉图像素尺寸无效".to_owned();
        unsafe { show_hud(hwnd, state) };
        return;
    };
    let rotated = if clockwise {
        rotate90(&image)
    } else {
        rotate270(&image)
    };
    state.pixels = rotated.into_raw();
    std::mem::swap(&mut state.source_width, &mut state.source_height);
    std::mem::swap(&mut state.base_width, &mut state.base_height);

    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
        let center = POINT {
            x: rect.left + (rect.right - rect.left) / 2,
            y: rect.top + (rect.bottom - rect.top) / 2,
        };
        unsafe { set_window_size_around_center(hwnd, state, center) };
    }
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
        let _ = UpdateWindow(hwnd);
        show_hud(hwnd, state);
    }
}

unsafe fn reset_view(hwnd: HWND, state: &mut NativePinState, center: POINT) {
    let steps = state.zoom.log(1.1);
    unsafe { resize_around(hwnd, state, center, -steps) };
    state.opacity = 1.0;
    let _ = unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA) };
    unsafe { show_hud(hwnd, state) };
}

unsafe fn set_window_size_around_center(hwnd: HWND, state: &NativePinState, center: POINT) {
    let target_width = (f64::from(state.base_width) * state.zoom).round() as i32;
    let target_height = (f64::from(state.base_height) * state.zoom).round() as i32;
    let desired_x = center.x - target_width / 2;
    let desired_y = center.y - target_height / 2;
    let monitor = unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (target_x, target_y) = if unsafe { GetMonitorInfoW(monitor, &mut monitor_info) }.as_bool() {
        let work = monitor_info.rcWork;
        (
            clamp_native_axis(
                desired_x,
                target_width,
                work.left,
                work.right - work.left,
                MIN_VISIBLE_EDGE,
            ),
            clamp_native_axis(
                desired_y,
                target_height,
                work.top,
                work.bottom - work.top,
                MIN_VISIBLE_EDGE,
            ),
        )
    } else {
        (desired_x, desired_y)
    };
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            target_x,
            target_y,
            target_width,
            target_height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
    };
}

unsafe fn show_hud(hwnd: HWND, state: &mut NativePinState) {
    if state.hud_hwnd == 0 {
        return;
    }
    state.hud_text = format!(
        "{}%  {}%",
        (state.zoom * 100.0).round() as i32,
        (state.opacity * 100.0).round() as i32
    );
    let hud = HWND(state.hud_hwnd as *mut c_void);
    unsafe {
        let _ =
            SetLayeredWindowAttributes(hud, HUD_TRANSPARENT_COLOR, 255, LWA_COLORKEY | LWA_ALPHA);
        position_hud_window(hwnd, state);
        let _ = InvalidateRect(Some(hud), None, false);
        let _ = ShowWindow(hud, SW_SHOWNOACTIVATE);
        let _ = UpdateWindow(hud);
        SetTimer(Some(hwnd), 1, 1_300, None);
    }
}

fn hud_position(image: RECT, work: RECT) -> (i32, i32) {
    let x = (image.right - HUD_WIDTH).clamp(work.left, work.right - HUD_WIDTH);
    let above = image.top - HUD_HEIGHT - HUD_GAP;
    let y = if above >= work.top {
        above
    } else {
        (image.bottom + HUD_GAP).min(work.bottom - HUD_HEIGHT)
    };
    (x, y)
}

unsafe fn position_hud_window(hwnd: HWND, state: &NativePinState) {
    if state.hud_hwnd == 0 {
        return;
    }
    let mut image = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut image) }.is_err() {
        return;
    }
    let center = POINT {
        x: image.left + (image.right - image.left) / 2,
        y: image.top + (image.bottom - image.top) / 2,
    };
    let monitor = unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let work = if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        info.rcWork
    } else {
        RECT {
            left: image.left - HUD_WIDTH,
            top: image.top - HUD_HEIGHT - HUD_GAP,
            right: image.right + HUD_WIDTH,
            bottom: image.bottom + HUD_HEIGHT + HUD_GAP,
        }
    };
    let (x, y) = hud_position(image, work);
    let _ = unsafe {
        SetWindowPos(
            HWND(state.hud_hwnd as *mut c_void),
            None,
            x,
            y,
            HUD_WIDTH,
            HUD_HEIGHT,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
    };
}

unsafe fn resize_around(hwnd: HWND, state: &mut NativePinState, cursor: POINT, steps: f64) {
    let minimum_zoom = (f64::from(MIN_VISIBLE_EDGE) / f64::from(state.base_width))
        .max(f64::from(MIN_VISIBLE_EDGE) / f64::from(state.base_height))
        .clamp(0.1, 1.0);
    let target_zoom =
        (state.zoom * 1.1_f64.powf(steps.clamp(-4.0, 4.0))).clamp(minimum_zoom, MAX_ZOOM);
    if (target_zoom - state.zoom).abs() < f64::EPSILON {
        return;
    }

    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return;
    }
    let current_width = (rect.right - rect.left).max(1);
    let current_height = (rect.bottom - rect.top).max(1);
    let target_width = (f64::from(state.base_width) * target_zoom).round() as i32;
    let target_height = (f64::from(state.base_height) * target_zoom).round() as i32;
    let anchor_x = f64::from(cursor.x - rect.left) / f64::from(current_width);
    let anchor_y = f64::from(cursor.y - rect.top) / f64::from(current_height);
    let desired_x = (f64::from(cursor.x) - anchor_x * f64::from(target_width)).round() as i32;
    let desired_y = (f64::from(cursor.y) - anchor_y * f64::from(target_height)).round() as i32;

    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (target_x, target_y) = if unsafe { GetMonitorInfoW(monitor, &mut monitor_info) }.as_bool() {
        let work = monitor_info.rcWork;
        (
            clamp_native_axis(
                desired_x,
                target_width,
                work.left,
                work.right - work.left,
                MIN_VISIBLE_EDGE,
            ),
            clamp_native_axis(
                desired_y,
                target_height,
                work.top,
                work.bottom - work.top,
                MIN_VISIBLE_EDGE,
            ),
        )
    } else {
        (desired_x, desired_y)
    };

    if unsafe {
        SetWindowPos(
            hwnd,
            None,
            target_x,
            target_y,
            target_width,
            target_height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
    }
    .is_ok()
    {
        state.zoom = target_zoom;
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
            let _ = UpdateWindow(hwnd);
            show_hud(hwnd, state);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        ffi::c_void,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };

    use image::{Rgba, RgbaImage};
    use windows::Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        UI::WindowsAndMessaging::{GetWindowRect, SendMessageW, WM_MOUSEWHEEL},
    };

    use super::{
        HUD_GAP, HUD_HEIGHT, HUD_TEXT_COLOR, HUD_TRANSPARENT_COLOR, HUD_WIDTH, create,
        hud_position, native_windows,
    };
    use crate::pin::PinnedImage;

    #[test]
    fn keeps_the_hud_outside_the_image_and_inside_the_work_area() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let image = RECT {
            left: 400,
            top: 300,
            right: 900,
            bottom: 650,
        };
        let (x, y) = hud_position(image, work);
        assert_eq!(x + HUD_WIDTH, image.right);
        assert_eq!(y + HUD_HEIGHT + HUD_GAP, image.top);

        let top_edge_image = RECT {
            left: 400,
            top: 2,
            right: 900,
            bottom: 352,
        };
        let (_, fallback_y) = hud_position(top_edge_image, work);
        assert_eq!(fallback_y, top_edge_image.bottom + HUD_GAP);
    }

    #[test]
    fn keeps_black_hud_text_distinct_from_the_transparent_color_key() {
        assert_eq!(HUD_TEXT_COLOR.0, 0);
        assert_ne!(HUD_TEXT_COLOR.0, HUD_TRANSPARENT_COLOR.0);
    }

    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn creates_resizes_and_closes_a_native_pin_window() {
        let label = "native-pin-runtime-test".to_owned();
        let images = Arc::new(Mutex::new(HashMap::from([(
            label.clone(),
            PinnedImage {
                png: Vec::new(),
                width: 320,
                height: 180,
            },
        )])));
        let image = RgbaImage::from_pixel(320, 180, Rgba([32, 160, 224, 255]));
        create(label.clone(), image, images.clone(), 320, 180).unwrap();

        let raw_hwnd = native_windows().lock().unwrap()[&label];
        let hwnd = HWND(raw_hwnd as *mut c_void);
        let mut before = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut before) }.unwrap();
        let cursor_x = before.left + (before.right - before.left) / 2;
        let cursor_y = before.top + (before.bottom - before.top) / 2;
        let packed_point = ((cursor_y as u16 as u32) << 16) | cursor_x as u16 as u32;
        unsafe {
            SendMessageW(
                hwnd,
                WM_MOUSEWHEEL,
                Some(WPARAM((120_u32 << 16) as usize)),
                Some(LPARAM(packed_point as isize)),
            );
        }

        let mut after = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut after) }.unwrap();
        assert!(after.right - after.left > before.right - before.left);
        assert!(after.bottom - after.top > before.bottom - before.top);

        super::close(&label).unwrap();
        for _ in 0..50 {
            if !native_windows().lock().unwrap().contains_key(&label) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!native_windows().lock().unwrap().contains_key(&label));
        assert!(!images.lock().unwrap().contains_key(&label));
    }
}

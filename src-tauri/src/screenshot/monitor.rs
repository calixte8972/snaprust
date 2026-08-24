//! Windows virtual-desktop, monitor, and DPI discovery.

use std::mem::size_of;

use serde::Serialize;
use windows::Win32::{
    Foundation::LPARAM,
    Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{
            GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
            SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        },
    },
};
use windows::core::BOOL;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub index: usize,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpi_x: u32,
    pub dpi_y: u32,
    pub scale_factor: f64,
    pub is_primary: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDesktop {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub monitors: Vec<MonitorInfo>,
}

pub fn virtual_desktop() -> Result<VirtualDesktop, String> {
    // SAFETY: GetSystemMetrics has no pointer parameters and these metrics are
    // read-only physical virtual-desktop bounds for the DPI-aware process.
    let (x, y, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if width <= 0 || height <= 0 {
        return Err(format!(
            "Windows returned invalid virtual desktop bounds: ({x}, {y}) {width}x{height}"
        ));
    }

    let mut monitors: Vec<MonitorInfo> = Vec::new();
    let context = LPARAM((&raw mut monitors).cast::<()>() as isize);
    // SAFETY: The callback receives the exact Vec pointer passed in LPARAM and
    // runs synchronously for the duration of this call.
    let succeeded = unsafe { EnumDisplayMonitors(None, None, Some(enumerate_monitor), context) };
    if !succeeded.as_bool() {
        return Err("failed to enumerate Windows displays".to_owned());
    }
    if monitors.is_empty() {
        return Err("Windows reported a virtual desktop without any monitors".to_owned());
    }

    monitors.sort_by_key(|monitor| (monitor.x, monitor.y));
    for (index, monitor) in monitors.iter_mut().enumerate() {
        monitor.index = index + 1;
    }

    Ok(VirtualDesktop {
        x,
        y,
        width: width as u32,
        height: height as u32,
        monitors,
    })
}

unsafe extern "system" fn enumerate_monitor(
    monitor: HMONITOR,
    _: HDC,
    _: *mut windows::Win32::Foundation::RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: virtual_desktop passes a valid mutable Vec pointer in LPARAM and
    // EnumDisplayMonitors invokes this callback synchronously.
    let monitors = unsafe { &mut *(data.0 as *mut Vec<MonitorInfo>) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: info points to initialized writable MONITORINFO storage.
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return BOOL(1);
    }

    let mut dpi_x = 96;
    let mut dpi_y = 96;
    // SAFETY: monitor comes from EnumDisplayMonitors; older systems can fail
    // this query, in which case the documented 96-DPI fallback is retained.
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    let rect = info.rcMonitor;
    let width = rect.right.saturating_sub(rect.left);
    let height = rect.bottom.saturating_sub(rect.top);
    if width <= 0 || height <= 0 {
        return BOOL(1);
    }

    monitors.push(MonitorInfo {
        index: 0,
        x: rect.left,
        y: rect.top,
        width: width as u32,
        height: height as u32,
        dpi_x,
        dpi_y,
        scale_factor: f64::from(dpi_x) / 96.0,
        is_primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
    });

    BOOL(1)
}

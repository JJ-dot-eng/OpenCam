//! 녹화 대상으로 고를 수 있는 최상위 창과 현재 화면 좌표.
use crate::capture::Rect;
use serde::Serialize;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::core::BOOL;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetShellWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, SetForegroundWindow, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

#[derive(Serialize)]
pub struct CaptureWindow {
    id: String,
    pid: u32,
    title: String,
}

fn bounds(hwnd: HWND, expected_pid: Option<u32>) -> Option<Rect> {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool()
            || hwnd == GetShellWindow()
        {
            return None;
        }
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if style & (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) != 0 { return None; }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() || expected_pid.is_some_and(|p| p != pid) {
            return None;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4).is_ok() && cloaked != 0 {
            return None;
        }
        let mut r = RECT::default();
        DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, (&mut r as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32).ok()?;
        let w = r.right.checked_sub(r.left)?;
        let h = r.bottom.checked_sub(r.top)?;
        (w >= 16 && h >= 16).then_some(Rect { x: r.left, y: r.top, w, h })
    }
}

unsafe extern "system" fn collect(hwnd: HWND, param: LPARAM) -> BOOL {
    // EnumWindows는 반환할 때까지 동기적으로 호출되며 param은 호출자의 Vec을 가리킨다.
    unsafe {
        if bounds(hwnd, None).is_none() { return BOOL(1); }
        let len = GetWindowTextLengthW(hwnd).clamp(0, 4096) as usize;
        if len == 0 { return BOOL(1); }
        let mut text = vec![0u16; len + 1];
        let n = GetWindowTextW(hwnd, &mut text).max(0) as usize;
        let title = String::from_utf16_lossy(&text[..n]).trim().to_owned();
        if !title.is_empty() {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let out = &mut *(param.0 as *mut Vec<CaptureWindow>);
            out.push(CaptureWindow { id: (hwnd.0 as usize).to_string(), pid, title });
        }
        BOOL(1)
    }
}

pub fn list() -> Result<Vec<CaptureWindow>, String> {
    let mut windows = Vec::new();
    unsafe { EnumWindows(Some(collect), LPARAM((&mut windows as *mut Vec<CaptureWindow>) as isize)) }
        .map_err(|e| e.to_string())?;
    Ok(windows)
}

#[derive(Serialize)]
pub struct SelectedWindow {
    rect: Rect,
    activated: bool,
}

pub fn select(id: &str, pid: u32) -> Result<SelectedWindow, String> {
    let raw = id.parse::<usize>().map_err(|_| "창 정보가 올바르지 않습니다")?;
    let hwnd = HWND(raw as *mut _);
    let rect = bounds(hwnd, Some(pid)).ok_or_else(|| "창이 닫혔거나 최소화되어 찾을 수 없습니다".to_string())?;
    let activated = unsafe { SetForegroundWindow(hwnd).as_bool() };
    Ok(SelectedWindow { rect, activated })
}

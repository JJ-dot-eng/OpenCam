//! 녹화 틀 크기 조절 중 커서 주변을 3배로 보여 주는 둥근 돋보기.
//! OS가 크기 조절 드래그를 처리하는 동안에도 그릴 수 있게 따로 스레드에서 Win32 창을 직접 띄운다.
//! 주 마우스 버튼을 떼면 스스로 닫힌다.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BitBlt, CAPTUREBLT,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, GetMonitorInfoW,
    HBITMAP, HDC, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ROP_CODE, ReleaseDC, SRCCOPY,
    SelectObject,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, GetCursorPos, GetSystemMetrics, MSG, PM_REMOVE,
    PeekMessageW, SM_SWAPBUTTON, SW_SHOWNOACTIVATE, SetWindowDisplayAffinity, ShowWindow, TranslateMessage,
    ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::w;

const ZOOM: i32 = 3;
/// 돋보기 지름(CSS 픽셀). 화면 배율을 곱해 물리 픽셀로 쓴다.
const DIAMETER: f64 = 240.0;
/// 커서와 돋보기 사이 간격
const GAP: i32 = 24;
const BORDER: i32 = 3;
/// 눈에 잘 띄는 주황 테두리와 빨간 커서 표시 (BGRA)
const RING: [u8; 4] = [0x0b, 0x9e, 0xf5, 0xff];
const MARK: [u8; 4] = [0x26, 0x26, 0xdc, 0xff];

static RUNNING: AtomicBool = AtomicBool::new(false);

/// 주 마우스 버튼이 눌려 있는지. GetAsyncKeyState는 물리 버튼을 보므로 왼손잡이 설정이면 오른쪽 버튼을 본다.
pub fn primary_button_down() -> bool {
    let primary = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 { VK_RBUTTON } else { VK_LBUTTON };
    (unsafe { GetAsyncKeyState(primary.0 as i32) } as u16 & 0x8000) != 0
}

/// 돋보기 원 안의 원본 픽셀 수(홀수라야 커서 픽셀이 한가운데 온다)와 지름(물리 픽셀)
fn lens_size(scale: f64) -> (i32, i32) {
    let src = (((DIAMETER * scale.clamp(0.5, 4.0)) as i32 / ZOOM) | 1).max(11);
    (src, src * ZOOM)
}

/// 원본(src×src BGRA)을 3배로 늘려 둥근 렌즈(size×size, 미리 곱한 알파)로 그린다.
/// 원 밖은 투명, 가장자리는 주황 테두리, 커서 아래 픽셀은 빨간 네모로 표시한다.
fn render(src: &[u8], src_n: i32, dst: &mut [u8], size: i32) {
    let c = size as f32 / 2.0;
    let r_out = c * c;
    let r_in = (c - BORDER as f32) * (c - BORDER as f32);
    let mid = src_n / 2 * ZOOM;
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let d2 = dx * dx + dy * dy;
            let o = ((y * size + x) * 4) as usize;
            let px: [u8; 4] = if d2 > r_out {
                [0; 4]
            } else if d2 > r_in {
                RING
            } else if (x == mid - 1 || x == mid + ZOOM) && (mid - 1..=mid + ZOOM).contains(&y)
                || (y == mid - 1 || y == mid + ZOOM) && (mid - 1..=mid + ZOOM).contains(&x)
            {
                MARK
            } else {
                let s = (((y / ZOOM) * src_n + x / ZOOM) * 4) as usize;
                [src[s], src[s + 1], src[s + 2], 0xff]
            };
            dst[o..o + 4].copy_from_slice(&px);
        }
    }
}

/// 커서 오른쪽 아래에 두고, 모니터 작업 영역을 넘으면 반대쪽으로 옮긴다.
fn lens_position(cursor: POINT, size: i32) -> POINT {
    let mut x = cursor.x + GAP;
    let mut y = cursor.y + GAP;
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if unsafe { GetMonitorInfoW(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), &mut mi) }.as_bool() {
        let wa = mi.rcWork;
        if x + size > wa.right {
            x = cursor.x - GAP - size;
        }
        if y + size > wa.bottom {
            y = cursor.y - GAP - size;
        }
    }
    POINT { x, y }
}

struct Dib {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
}

impl Dib {
    unsafe fn new(screen: HDC, w: i32, h: i32) -> Option<Self> {
        unsafe {
            let dc = CreateCompatibleDC(Some(screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let Ok(bmp) = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                return None;
            };
            let old = SelectObject(dc, bmp.into());
            Some(Dib { dc, bmp, old, bits: bits as *mut u8 })
        }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// 주 버튼이 눌려 있는 동안 돋보기를 띄운다. 이미 떠 있으면 아무것도 하지 않는다.
pub fn start(scale: f64) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        unsafe { run(scale) };
        RUNNING.store(false, Ordering::SeqCst);
    });
}

unsafe fn run(scale: f64) {
    unsafe {
        let (src_n, size) = lens_size(scale);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            w!("STATIC"),
            w!("OpenCam 돋보기"),
            WS_POPUP,
            0,
            0,
            size,
            size,
            None,
            None,
            None,
            None,
        ) else {
            return;
        };
        // 화면을 베껴 올 때 돋보기 자신이 찍히지 않게 한다
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        draw_loop(hwnd, src_n, size);
        let _ = DestroyWindow(hwnd);
    }
}

unsafe fn draw_loop(hwnd: HWND, src_n: i32, size: i32) {
    unsafe {
        let screen = GetDC(None);
        let (Some(src), Some(dst)) = (Dib::new(screen, src_n, src_n), Dib::new(screen, size, size)) else {
            ReleaseDC(None, screen);
            return;
        };
        let src_px = std::slice::from_raw_parts(src.bits, (src_n * src_n * 4) as usize);
        let dst_px = std::slice::from_raw_parts_mut(dst.bits, (size * size * 4) as usize);
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let started = Instant::now();
        let mut shown = false;
        let mut msg = MSG::default();
        // 버튼을 못 떼고 끝난 경우를 대비해 1분이 지나면 닫는다
        while primary_button_down() && started.elapsed() < Duration::from_secs(60) {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let mut cur = POINT::default();
            if GetCursorPos(&mut cur).is_ok() {
                let half = src_n / 2;
                // 반투명 녹화 틀 같은 겹친 창도 찍히게 CAPTUREBLT
                let rop = ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0);
                if BitBlt(src.dc, 0, 0, src_n, src_n, Some(screen), cur.x - half, cur.y - half, rop).is_ok() {
                    let _ = GdiFlush();
                    render(src_px, src_n, dst_px, size);
                    let pos = lens_position(cur, size);
                    let sz = SIZE { cx: size, cy: size };
                    let origin = POINT::default();
                    let _ = UpdateLayeredWindow(
                        hwnd,
                        Some(screen),
                        Some(&pos),
                        Some(&sz),
                        Some(dst.dc),
                        Some(&origin),
                        COLORREF(0),
                        Some(&blend),
                        ULW_ALPHA,
                    );
                    if !shown {
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        shown = true;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        drop(src);
        drop(dst);
        ReleaseDC(None, screen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lens_is_round_with_ring_and_marked_center() {
        let (src_n, size) = lens_size(1.0);
        assert_eq!(src_n % 2, 1);
        assert_eq!(size, src_n * ZOOM);
        // 원본은 모두 초록
        let src: Vec<u8> = [0, 200, 0, 255].repeat((src_n * src_n) as usize);
        let mut dst = vec![0u8; (size * size * 4) as usize];
        render(&src, src_n, &mut dst, size);
        let at = |x: i32, y: i32| -> [u8; 4] { dst[((y * size + x) * 4) as usize..][..4].try_into().unwrap() };
        assert_eq!(at(0, 0), [0; 4], "모서리는 투명");
        assert_eq!(at(size / 2, 1), RING, "가장자리는 테두리");
        assert_eq!(at(size / 2, size / 4), [0, 200, 0, 255], "안쪽은 원본");
        let mid = src_n / 2 * ZOOM;
        assert_eq!(at(mid - 1, mid + 1), MARK, "커서 픽셀 둘레 표시");
        assert_eq!(at(mid + 1, mid + 1), [0, 200, 0, 255], "커서 픽셀 자체는 원본");
    }
}

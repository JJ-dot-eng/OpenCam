//! DXGI Desktop Duplication 화면 캡처.
//!
//! 캡처 방식:
//! - 모니터마다 Duplication을 따로 연다
//! - AcquireNextFrame(timeout 0): 기다리지 않고 새 화면만 받는다
//! - GPU 텍스처를 STAGING 텍스처로 복사해서 읽는다 (STAGING은 한 번 만들고 재사용)
//! - 녹화 영역과 겹치는 부분만 잘라 붙인다
//! - HDR(R16G16B16A16_FLOAT)이면 일반 색으로 바꾼다

use anyhow::{Context, Result, anyhow};
use windows::Win32::Foundation::{HMODULE, POINT, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::Interface;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        (x1 > x0 && y1 > y0).then(|| Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }
}

struct Output {
    bounds: Rect,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    output: IDXGIOutput1,
    dup: Option<IDXGIOutputDuplication>,
    staging: Option<ID3D11Texture2D>,
    staging_desc: Option<(u32, u32, DXGI_FORMAT)>,
    got_first: bool,
}

pub struct CursorOptions {
    pub draw: bool,
    pub highlight: bool,
    pub highlight_color: u32,
    pub click_fx: bool,
    pub left_color: u32,
    pub right_color: u32,
}

pub struct Capturer {
    region: Rect,
    outputs: Vec<Output>,
    /// 녹화 영역 크기의 BGRA 캔버스. 바뀐 모니터 부분만 덮어쓴다.
    canvas: Vec<u8>,
}

/// 모든 모니터의 (위치, 크기) 목록
pub fn monitors() -> Result<Vec<Rect>> {
    let mut out = vec![];
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut ai = 0;
        while let Ok(adapter) = factory.EnumAdapters1(ai) {
            let mut oi = 0;
            while let Ok(output) = adapter.EnumOutputs(oi) {
                let d = output.GetDesc()?;
                if d.AttachedToDesktop.as_bool() {
                    out.push(rect_of(&d.DesktopCoordinates));
                }
                oi += 1;
            }
            ai += 1;
        }
    }
    Ok(out)
}

fn rect_of(r: &RECT) -> Rect {
    Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
}

impl Capturer {
    pub fn new(region: Rect) -> Result<Self> {
        let mut outputs = vec![];
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
            let mut ai = 0;
            while let Ok(adapter) = factory.EnumAdapters1(ai) {
                ai += 1;
                let mut oi = 0;
                while let Ok(output) = adapter.EnumOutputs(oi) {
                    oi += 1;
                    let desc = output.GetDesc()?;
                    if !desc.AttachedToDesktop.as_bool() {
                        continue;
                    }
                    let bounds = rect_of(&desc.DesktopCoordinates);
                    if bounds.intersect(&region).is_none() {
                        continue;
                    }
                    let mut device = None;
                    let mut context = None;
                    D3D11CreateDevice(
                        &adapter,
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        Some(&mut context),
                    )
                    .context("D3D11CreateDevice failed")?;
                    let mut o = Output {
                        bounds,
                        device: device.ok_or_else(|| anyhow!("no d3d device"))?,
                        context: context.ok_or_else(|| anyhow!("no d3d context"))?,
                        output: output.cast()?,
                        dup: None,
                        staging: None,
                        staging_desc: None,
                        got_first: false,
                    };
                    o.open()?;
                    outputs.push(o);
                }
            }
        }
        if outputs.is_empty() {
            return Err(anyhow!("녹화 영역이 어떤 모니터와도 겹치지 않습니다"));
        }
        Ok(Capturer { region, outputs, canvas: vec![0; (region.w * region.h * 4) as usize] })
    }

    pub fn region(&self) -> Rect {
        self.region
    }

    /// 영역만 옮길 때(크기 그대로) 쓰는 빠른 경로. 모니터 구성이 바뀌면 새로 만든다.
    pub fn set_region(&mut self, region: Rect) -> Result<()> {
        if region.w != self.region.w || region.h != self.region.h {
            *self = Capturer::new(region)?;
            return Ok(());
        }
        let covered = self.outputs.iter().filter(|o| o.bounds.intersect(&region).is_some()).count();
        let needed = monitors()?.iter().filter(|m| m.intersect(&region).is_some()).count();
        if covered != needed {
            *self = Capturer::new(region)?;
        } else {
            self.region = region;
            for o in &mut self.outputs {
                o.got_first = false;
            }
        }
        Ok(())
    }

    /// 현재 화면을 캔버스에 갱신하고 RGBA로 돌려준다.
    pub fn grab(&mut self, cursor: &CursorOptions) -> Result<Vec<u8>> {
        let region = self.region;
        for o in &mut self.outputs {
            let Some(part) = o.bounds.intersect(&region) else {
                continue;
            };
            // 첫 화면은 반드시 받아야 한다. 새로 연 직후에는 이미지 없이(LastPresentTime 0) 오는 경우가 있어
            // 진짜 화면이 올 때까지 잠깐(최대 1초) 기다리고, 그래도 없으면 받은 그대로 쓴다.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            loop {
                let first = !o.got_first;
                let force = first && std::time::Instant::now() >= deadline;
                let timeout = if first { 100 } else { 0 };
                match o.acquire(timeout, force, &region, &part, &mut self.canvas) {
                    Ok(copied) => o.got_first |= copied,
                    Err(e) => {
                        // 접근 권한을 잃으면(해상도 변경, UAC 화면 등) 다시 연다
                        let code = e.downcast_ref::<windows::core::Error>().map(|e| e.code());
                        if code == Some(DXGI_ERROR_ACCESS_LOST) {
                            o.dup = None;
                            let _ = o.open();
                            break;
                        } else if code != Some(DXGI_ERROR_WAIT_TIMEOUT) {
                            return Err(e);
                        }
                    }
                }
                if o.got_first || force || o.dup.is_none() {
                    break;
                }
            }
        }
        let mut bgra = self.canvas.clone();
        if cursor.draw || cursor.highlight || cursor.click_fx {
            draw_cursor(&mut bgra, region, cursor);
        }
        bgra_to_rgba(&mut bgra);
        Ok(bgra)
    }
}

impl Output {
    fn open(&mut self) -> Result<()> {
        unsafe {
            // DuplicateOutput1로 BGRA8만 요청하면 HDR 화면도 OS가 일반 색으로 바꿔 준다
            let dup = match self.output.cast::<IDXGIOutput5>() {
                Ok(o5) => o5
                    .DuplicateOutput1(&self.device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM])
                    .or_else(|_| self.output.DuplicateOutput(&self.device)),
                Err(_) => self.output.DuplicateOutput(&self.device),
            }
            .context("DuplicateOutput failed")?;
            self.dup = Some(dup);
        }
        self.got_first = false;
        Ok(())
    }

    /// 새 화면을 받아 캔버스에 옮긴다. 화면 이미지를 실제로 옮겼으면 true.
    /// force면 이미지가 없다고 표시된 프레임이라도 옮긴다.
    fn acquire(&mut self, timeout: u32, force: bool, region: &Rect, part: &Rect, canvas: &mut [u8]) -> Result<bool> {
        let dup = self.dup.as_ref().ok_or_else(|| anyhow!("duplication closed"))?.clone();
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;
            dup.AcquireNextFrame(timeout, &mut info, &mut res)?;
            let result = (|| -> Result<bool> {
                // 커서만 움직였을 때는 화면 이미지가 바뀌지 않았다
                if info.LastPresentTime == 0 && !force {
                    return Ok(false);
                }
                let tex: ID3D11Texture2D = res.ok_or_else(|| anyhow!("no frame resource"))?.cast()?;
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                tex.GetDesc(&mut desc);
                let key = (desc.Width, desc.Height, desc.Format);
                if self.staging_desc != Some(key) {
                    let sdesc = D3D11_TEXTURE2D_DESC {
                        Width: desc.Width,
                        Height: desc.Height,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: desc.Format,
                        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                        Usage: D3D11_USAGE_STAGING,
                        BindFlags: 0,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        MiscFlags: 0,
                    };
                    let mut st = None;
                    self.device.CreateTexture2D(&sdesc, None, Some(&mut st))?;
                    self.staging = st;
                    self.staging_desc = Some(key);
                }
                let staging = self.staging.as_ref().unwrap();
                self.context.CopyResource(staging, &tex);
                let mut map = D3D11_MAPPED_SUBRESOURCE::default();
                self.context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
                let hdr = desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT;
                let bpp = if hdr { 8 } else { 4 };
                let src = std::slice::from_raw_parts(map.pData as *const u8, (map.RowPitch * desc.Height) as usize);
                let sx = (part.x - self.bounds.x) as usize;
                let dx = (part.x - region.x) as usize;
                for row in 0..part.h as usize {
                    let sy = (part.y - self.bounds.y) as usize + row;
                    let dy = (part.y - region.y) as usize + row;
                    let s = &src[sy * map.RowPitch as usize + sx * bpp..][..part.w as usize * bpp];
                    let d = &mut canvas[(dy * region.w as usize + dx) * 4..][..part.w as usize * 4];
                    if hdr {
                        hdr_row_to_bgra(s, d);
                    } else {
                        d.copy_from_slice(s);
                    }
                }
                self.context.Unmap(staging, 0);
                Ok(true)
            })();
            let _ = dup.ReleaseFrame();
            result
        }
    }
}

/// scRGB(선형, 1.0 = 80nit 흰색) FP16 → sRGB 8비트
fn hdr_row_to_bgra(src: &[u8], dst: &mut [u8]) {
    for (s, d) in src.chunks_exact(8).zip(dst.chunks_exact_mut(4)) {
        let ch = |i: usize| half::f16::from_le_bytes([s[i * 2], s[i * 2 + 1]]).to_f32();
        let enc = |v: f32| {
            let v = v.clamp(0.0, 1.0);
            let g = if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (g * 255.0 + 0.5) as u8
        };
        d[0] = enc(ch(2));
        d[1] = enc(ch(1));
        d[2] = enc(ch(0));
        d[3] = 255;
    }
}

fn bgra_to_rgba(px: &mut [u8]) {
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2);
        p[3] = 255;
    }
}

/// 원을 반투명하게 덮는다(BGRA 캔버스). ring이면 테두리만.
fn blend_circle(buf: &mut [u8], w: i32, h: i32, cx: i32, cy: i32, r: i32, bgr: u32, alpha: f32, ring: bool) {
    let (cr, cg, cb) = ((bgr & 0xff) as f32, ((bgr >> 8) & 0xff) as f32, ((bgr >> 16) & 0xff) as f32);
    for y in (cy - r).max(0)..(cy + r + 1).min(h) {
        for x in (cx - r).max(0)..(cx + r + 1).min(w) {
            let d = (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt();
            let edge = (r as f32 + 0.5 - d).clamp(0.0, 1.0);
            let inner = if ring { (d - (r as f32 - 3.5)).clamp(0.0, 1.0) } else { 1.0 };
            let a = alpha * edge * inner;
            if a <= 0.0 {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            buf[i] = (buf[i] as f32 * (1.0 - a) + cb * a) as u8;
            buf[i + 1] = (buf[i + 1] as f32 * (1.0 - a) + cg * a) as u8;
            buf[i + 2] = (buf[i + 2] as f32 * (1.0 - a) + cr * a) as u8;
        }
    }
}

fn draw_cursor(canvas: &mut [u8], region: Rect, opt: &CursorOptions) {
    unsafe {
        let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
        if GetCursorInfo(&mut ci).is_err() {
            return;
        }
        let px = ci.ptScreenPos.x - region.x;
        let py = ci.ptScreenPos.y - region.y;

        if opt.highlight {
            blend_circle(canvas, region.w, region.h, px, py, 22, opt.highlight_color, 0.35, false);
        }
        if opt.click_fx {
            if GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000 != 0 {
                blend_circle(canvas, region.w, region.h, px, py, 18, opt.left_color, 0.8, true);
            } else if GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000 != 0 {
                blend_circle(canvas, region.w, region.h, px, py, 18, opt.right_color, 0.8, true);
            }
        }
        if !opt.draw || ci.flags != CURSOR_SHOWING || ci.hCursor.is_invalid() {
            return;
        }

        let mut ii = ICONINFO::default();
        if GetIconInfo(HICON(ci.hCursor.0), &mut ii).is_err() {
            return;
        }
        let hot = POINT { x: ii.xHotspot as i32, y: ii.yHotspot as i32 };
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor.into());
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(ii.hbmMask.into());
        }

        // 커서 주변 128x128을 GDI 비트맵으로 옮겨 DrawIconEx로 그린 뒤 다시 가져온다
        // (흑백 반전 커서까지 정확하게 그리기 위해)
        const S: i32 = 128;
        let ox = px - hot.x;
        let oy = py - hot.y;
        let Some(area) = (Rect { x: ox, y: oy, w: S, h: S }).intersect(&Rect { x: 0, y: 0, w: region.w, h: region.h })
        else {
            return;
        };

        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: S,
                biHeight: -S,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            return;
        };
        let old = SelectObject(mem, dib.into());
        let tile = std::slice::from_raw_parts_mut(bits as *mut u8, (S * S * 4) as usize);
        let copy = |tile: &mut [u8], canvas: &mut [u8], to_tile: bool| {
            for y in 0..area.h {
                let ty = (area.y - oy + y) as usize;
                let tx = (area.x - ox) as usize;
                let t = &mut tile[(ty * S as usize + tx) * 4..][..area.w as usize * 4];
                let c = &mut canvas[(((area.y + y) * region.w + area.x) * 4) as usize..][..area.w as usize * 4];
                if to_tile { t.copy_from_slice(c) } else { c.copy_from_slice(t) }
            }
        };
        copy(tile, canvas, true);
        let _ = DrawIconEx(mem, 0, 0, HICON(ci.hCursor.0), 0, 0, 0, None, DI_NORMAL);
        let _ = GdiFlush();
        copy(tile, canvas, false);
        SelectObject(mem, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

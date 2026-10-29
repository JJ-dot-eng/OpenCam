//! 2단계 효과: 필터, 화면 전환, 그림·텍스트·이미지 덧씌우기(등장/퇴장 애니메이션).
//! 모두 RGBA 픽셀 한 장을 제자리에서 바꾸는 함수들이다. 어느 프레임에 쓸지는 project.rs가 정한다.

use serde::Deserialize;

/// 일부 영역에만 쓰는 필터(흐리게·모자이크)의 영역
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    #[serde(default)]
    pub ellipse: bool,
}

impl Region {
    /// (x, y)가 영역 안이면 true
    fn contains(&self, x: i32, y: i32) -> bool {
        if x < self.x || y < self.y || x >= self.x + self.w || y >= self.y + self.h {
            return false;
        }
        if !self.ellipse {
            return true;
        }
        let rx = self.w as f32 / 2.0;
        let ry = self.h as f32 / 2.0;
        let dx = (x as f32 + 0.5 - self.x as f32 - rx) / rx;
        let dy = (y as f32 + 0.5 - self.y as f32 - ry) / ry;
        dx * dx + dy * dy <= 1.0
    }
}

pub type Rgb = [u8; 3];

#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Filter {
    /// -100 ~ 100
    Brightness {
        value: f32,
    },
    /// -100 ~ 100
    Contrast {
        value: f32,
    },
    /// 0.1 ~ 5 (1이면 그대로)
    Gamma {
        value: f32,
    },
    /// -100(흑백) ~ 100(두 배)
    Saturation {
        value: f32,
    },
    Grayscale,
    /// 0 ~ 100
    Sepia {
        amount: f32,
    },
    Invert,
    /// 0 ~ 100
    Sharpen {
        amount: f32,
    },
    Blur {
        radius: u32,
        region: Option<Region>,
    },
    Mosaic {
        size: u32,
        region: Option<Region>,
    },
    /// 0 ~ 100
    Vignette {
        amount: f32,
    },
    Scanline {
        gap: u32,
        strength: f32,
    },
    /// -100(차갑게) ~ 100(따뜻하게)
    Temperature {
        value: f32,
    },
    Monotone {
        color: Rgb,
    },
    Duotone {
        dark: Rgb,
        light: Rgb,
    },
    /// 가운데 띠(위치·폭은 %)만 또렷하고 위아래는 흐리게
    TiltShift {
        center: f32,
        band: f32,
        radius: u32,
    },
    /// 배경 흐림: 타원(가운데·반지름 %) 밖을 흐리게
    IrisBlur {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        radius: u32,
        feather: f32,
    },
    /// 색상 곡선: 입력 64·128·192가 나갈 값
    Curve {
        shadows: f32,
        midtones: f32,
        highlights: f32,
    },
    /// 색상 채널: -100 ~ 100
    Channels {
        brightness: f32,
        red: f32,
        green: f32,
        blue: f32,
    },
    /// 화이트밸런스: 기준 색이 없으면 자동
    WhiteBalance {
        reference: Option<Rgb>,
    },
    /// 노이즈 감소: 1 ~ 3
    Denoise {
        strength: u32,
    },
    /// 뽀샤시: 부드럽게(shiny=false) / 화사하게(shiny=true)
    Bloom {
        radius: u32,
        amount: f32,
        shiny: bool,
    },
    Cartoon {
        edges: f32,
        levels: u32,
    },
    /// 판화: 밝기 기준 %
    Print {
        threshold: f32,
    },
    Ascii {
        cell: u32,
        color: bool,
    },
    /// 잔상 효과: 앞 프레임이 남는다 (프레임 사이 효과라 project.rs에서 처리)
    Afterimage {
        strength: f32,
    },
    /// 크로마키: 그 색 근처를 투명하게(또는 대치 색으로)
    ChromaKey {
        color: Rgb,
        range: f32,
        adjacent: bool,
        replace: Option<Rgb>,
    },
    /// 지우개: 영역을 둘레 색으로 채운다
    Eraser {
        region: Option<Region>,
    },
}

fn clamp8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

fn luma(p: &[u8]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// 색 채널(R,G,B)마다 같은 표를 적용한다
fn apply_lut(px: &mut [u8], f: impl Fn(f32) -> f32) {
    let lut: Vec<u8> = (0..256).map(|v| clamp8(f(v as f32))).collect();
    for p in px.chunks_exact_mut(4) {
        for c in &mut p[..3] {
            *c = lut[*c as usize];
        }
    }
}

pub fn apply_filter(px: &mut [u8], w: u32, h: u32, f: &Filter) {
    let (wi, hi) = (w as i32, h as i32);
    match *f {
        Filter::Brightness { value } => {
            let d = value.clamp(-100.0, 100.0) * 2.55;
            apply_lut(px, |v| v + d);
        }
        Filter::Contrast { value } => {
            let c = ((value.clamp(-100.0, 100.0) + 100.0) / 100.0).powi(2);
            apply_lut(px, |v| (v - 128.0) * c + 128.0);
        }
        Filter::Gamma { value } => {
            let g = 1.0 / value.clamp(0.1, 5.0);
            apply_lut(px, |v| 255.0 * (v / 255.0).powf(g));
        }
        Filter::Saturation { value } => {
            let s = 1.0 + value.clamp(-100.0, 100.0) / 100.0;
            for p in px.chunks_exact_mut(4) {
                let l = luma(p);
                for c in &mut p[..3] {
                    *c = clamp8(l + (*c as f32 - l) * s);
                }
            }
        }
        Filter::Grayscale => {
            for p in px.chunks_exact_mut(4) {
                let l = clamp8(luma(p));
                p[..3].fill(l);
            }
        }
        Filter::Sepia { amount } => {
            let a = amount.clamp(0.0, 100.0) / 100.0;
            for p in px.chunks_exact_mut(4) {
                let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
                let s = [
                    0.393 * r + 0.769 * g + 0.189 * b,
                    0.349 * r + 0.686 * g + 0.168 * b,
                    0.272 * r + 0.534 * g + 0.131 * b,
                ];
                for k in 0..3 {
                    p[k] = clamp8(p[k] as f32 * (1.0 - a) + s[k] * a);
                }
            }
        }
        Filter::Invert => apply_lut(px, |v| 255.0 - v),
        Filter::Sharpen { amount } => {
            let a = amount.clamp(0.0, 100.0) / 50.0;
            let mut soft = px.to_vec();
            box_blur(&mut soft, w, h, 1, 1);
            for (p, s) in px.chunks_exact_mut(4).zip(soft.chunks_exact(4)) {
                for k in 0..3 {
                    p[k] = clamp8(p[k] as f32 + (p[k] as f32 - s[k] as f32) * a);
                }
            }
        }
        Filter::Blur { radius, region } => {
            let mut soft = px.to_vec();
            box_blur(&mut soft, w, h, radius.clamp(1, 100), 3);
            copy_in_region(px, &soft, wi, hi, region);
        }
        Filter::Mosaic { size, region } => {
            let size = size.clamp(2, 200) as i32;
            let r = region.unwrap_or(Region { x: 0, y: 0, w: wi, h: hi, ellipse: false });
            let mut out = px.to_vec();
            // 영역 왼쪽 위에 맞춰 칸을 나눈다
            let mut by = r.y.max(0);
            while by < (r.y + r.h).min(hi) {
                let mut bx = r.x.max(0);
                while bx < (r.x + r.w).min(wi) {
                    let (x1, y1) = ((bx + size).min(wi), (by + size).min(hi));
                    let mut sum = [0u32; 4];
                    for y in by..y1 {
                        for x in bx..x1 {
                            let i = ((y * wi + x) * 4) as usize;
                            for k in 0..4 {
                                sum[k] += px[i + k] as u32;
                            }
                        }
                    }
                    let n = ((x1 - bx) * (y1 - by)) as u32;
                    for y in by..y1 {
                        for x in bx..x1 {
                            let i = ((y * wi + x) * 4) as usize;
                            for k in 0..4 {
                                out[i + k] = (sum[k] / n) as u8;
                            }
                        }
                    }
                    bx += size;
                }
                by += size;
            }
            copy_in_region(px, &out, wi, hi, region);
        }
        Filter::Vignette { amount } => {
            let a = amount.clamp(0.0, 100.0) / 100.0;
            let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
            let maxd = (cx * cx + cy * cy).sqrt();
            for y in 0..hi {
                for x in 0..wi {
                    let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() / maxd;
                    let t = ((d - 0.35) / 0.65).clamp(0.0, 1.0);
                    let k = 1.0 - a * t * t * (3.0 - 2.0 * t);
                    let i = ((y * wi + x) * 4) as usize;
                    for c in &mut px[i..i + 3] {
                        *c = clamp8(*c as f32 * k);
                    }
                }
            }
        }
        Filter::Scanline { gap, strength } => {
            let gap = gap.clamp(2, 20);
            let k = 1.0 - strength.clamp(0.0, 100.0) / 100.0;
            for y in (0..h).step_by(gap as usize) {
                let row = &mut px[(y * w * 4) as usize..][..(w * 4) as usize];
                for p in row.chunks_exact_mut(4) {
                    for c in &mut p[..3] {
                        *c = clamp8(*c as f32 * k);
                    }
                }
            }
        }
        Filter::Temperature { value } => {
            let d = value.clamp(-100.0, 100.0) * 0.4;
            for p in px.chunks_exact_mut(4) {
                p[0] = clamp8(p[0] as f32 + d);
                p[2] = clamp8(p[2] as f32 - d);
            }
        }
        Filter::Monotone { color } => {
            for p in px.chunks_exact_mut(4) {
                let l = luma(p) / 255.0;
                for k in 0..3 {
                    p[k] = clamp8(color[k] as f32 * l);
                }
            }
        }
        Filter::Duotone { dark, light } => {
            for p in px.chunks_exact_mut(4) {
                let l = luma(p) / 255.0;
                for k in 0..3 {
                    p[k] = clamp8(dark[k] as f32 + (light[k] as f32 - dark[k] as f32) * l);
                }
            }
        }
        Filter::IrisBlur { cx, cy, rx, ry, radius, feather } => {
            crate::filters::iris_blur(px, w, h, cx, cy, rx, ry, radius, feather)
        }
        Filter::Curve { shadows, midtones, highlights } => {
            crate::filters::apply_curve(px, &crate::filters::curve_lut(shadows, midtones, highlights))
        }
        Filter::Channels { brightness, red, green, blue } => {
            crate::filters::channels(px, brightness, [red, green, blue])
        }
        Filter::WhiteBalance { reference } => crate::filters::white_balance(px, reference),
        Filter::Denoise { strength } => crate::filters::denoise(px, w, h, strength),
        Filter::Bloom { radius, amount, shiny } => crate::filters::bloom(px, w, h, radius, amount, shiny),
        Filter::Cartoon { edges, levels } => crate::filters::cartoon(px, w, h, edges, levels),
        Filter::Print { threshold } => crate::filters::print(px, threshold),
        Filter::Ascii { cell, color } => crate::filters::ascii(px, w, h, cell, color),
        // 앞 프레임이 필요해서 여기서는 하지 않는다
        Filter::Afterimage { .. } => {}
        Filter::ChromaKey { color, range, adjacent, replace } => {
            crate::filters::chroma_key(px, w, h, color, range, adjacent, replace)
        }
        Filter::Eraser { region } => {
            if let Some(r) = region {
                crate::filters::eraser(px, w, h, r);
            }
        }
        Filter::TiltShift { center, band, radius } => {
            let mut soft = px.to_vec();
            box_blur(&mut soft, w, h, radius.clamp(1, 100), 3);
            let cy = h as f32 * center.clamp(0.0, 100.0) / 100.0;
            let half = (h as f32 * band.clamp(1.0, 100.0) / 100.0 / 2.0).max(1.0);
            for y in 0..hi {
                // 띠 안은 0, 띠 밖으로 띠 폭만큼 가면서 1까지
                let t = (((y as f32 + 0.5 - cy).abs() - half) / half).clamp(0.0, 1.0);
                let row = (y * wi * 4) as usize..((y + 1) * wi * 4) as usize;
                for (p, s) in px[row.clone()].iter_mut().zip(&soft[row]) {
                    *p = clamp8(*p as f32 + (*s as f32 - *p as f32) * t);
                }
            }
        }
    }
}

/// region이 있으면 그 안만 src로, 없으면 전부 src로
fn copy_in_region(px: &mut [u8], src: &[u8], w: i32, h: i32, region: Option<Region>) {
    let Some(r) = region else {
        px.copy_from_slice(src);
        return;
    };
    for y in r.y.max(0)..(r.y + r.h).min(h) {
        for x in r.x.max(0)..(r.x + r.w).min(w) {
            if r.contains(x, y) {
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&src[i..i + 4]);
            }
        }
    }
}

/// 상자 흐리기를 passes번 (3번이면 가우시안과 거의 같다)
pub fn box_blur(px: &mut [u8], w: u32, h: u32, r: u32, passes: u32) {
    let (w, h, r) = (w as usize, h as usize, r as usize);
    let mut tmp = vec![0u8; px.len()];
    for _ in 0..passes {
        blur_line(px, &mut tmp, w, h, r, true);
        blur_line(&tmp, px, w, h, r, false);
    }
}

/// 가로(horizontal) 또는 세로 한 방향 상자 흐리기. 가장자리는 끝 픽셀을 늘려 쓴다.
fn blur_line(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let idx = |line: usize, k: usize| if horizontal { (line * w + k) * 4 } else { (k * w + line) * 4 };
    let n = (2 * r + 1) as u32;
    for line in 0..lines {
        let mut sum = [0u32; 4];
        for k in 0..=2 * r {
            let i = idx(line, k.saturating_sub(r).min(len - 1));
            for c in 0..4 {
                sum[c] += src[i + c] as u32;
            }
        }
        for k in 0..len {
            let o = idx(line, k);
            for c in 0..4 {
                dst[o + c] = (sum[c] / n) as u8;
            }
            let add = idx(line, (k + r + 1).min(len - 1));
            let sub = idx(line, k.saturating_sub(r));
            for c in 0..4 {
                sum[c] = sum[c] + src[add + c] as u32 - src[sub + c] as u32;
            }
        }
    }
}

// ---------- 화면 전환 ----------

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TransitionKind {
    FadeInWhite,
    FadeInBlack,
    FadeOutWhite,
    FadeOutBlack,
    Flash,
    /// 끝부분을 첫 프레임으로 겹쳐서 반복이 자연스럽게
    CrossfadeLoop,
}

/// 두 색을 섞는다: t=0이면 a, 1이면 b
fn mix_into(px: &mut [u8], other: impl Fn(usize) -> u8, t: f32) {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return;
    }
    for (i, v) in px.iter_mut().enumerate() {
        if i % 4 != 3 {
            *v = clamp8(*v as f32 + (other(i) as f32 - *v as f32) * t);
        }
    }
}

/// 전환 효과. tm: 이 프레임의 가운데 시각, total: 구간 전체 시간, ms: 효과 길이, first: 구간 첫 프레임 픽셀
pub fn apply_transition(px: &mut [u8], kind: TransitionKind, tm: f32, total: f32, ms: f32, first: &[u8]) {
    let ms = ms.max(1.0);
    let head = 1.0 - tm / ms; // 앞부분 효과 세기 (처음 1 → 0)
    let tail = 1.0 - (total - tm) / ms; // 뒷부분 효과 세기 (0 → 끝 1)
    match kind {
        TransitionKind::FadeInWhite => mix_into(px, |_| 255, head),
        TransitionKind::FadeInBlack => mix_into(px, |_| 0, head),
        TransitionKind::FadeOutWhite => mix_into(px, |_| 255, tail),
        TransitionKind::FadeOutBlack => mix_into(px, |_| 0, tail),
        TransitionKind::Flash => mix_into(px, |_| 255, head * head),
        TransitionKind::CrossfadeLoop => {
            if first.len() == px.len() {
                mix_into(px, |i| first[i], tail);
            }
        }
    }
}

// ---------- 둥근 모서리 (콜라주 칸) ----------

/// 모서리를 반지름 radius로 둥글게 깎는다 (바깥은 투명, 가장자리 1픽셀은 부드럽게)
pub fn round_corners(px: &mut [u8], w: u32, h: u32, radius: u32) {
    let (wi, hi) = (w as i32, h as i32);
    let r = radius.min(w.min(h) / 2) as f32;
    for y in 0..hi {
        for x in 0..wi {
            // 가장 가까운 모서리 원의 중심
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let cx = fx.clamp(r, w as f32 - r);
            let cy = fy.clamp(r, h as f32 - r);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let cover = (r - d + 0.5).clamp(0.0, 1.0);
            if cover < 1.0 {
                let i = ((y * wi + x) * 4 + 3) as usize;
                px[i] = (px[i] as f32 * cover) as u8;
            }
        }
    }
}

// ---------- 덧씌우기 (그리기·텍스트·이미지) ----------

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Anim {
    #[default]
    None,
    Fade,
    /// 작게 시작해 커진다 (퇴장이면 작아지며 사라진다)
    ZoomIn,
    /// 크게 시작해 작아진다 (퇴장이면 커지며 사라진다)
    ZoomOut,
    Left,
    Right,
    Top,
    Bottom,
    /// 작아지며 사라짐(퇴장) / 커지며 나타남과 같은 크기 변화에 서서히 투명
    FadeSmall,
    /// 회전하며 작아지며 나타남 (크게 시작)
    RotateBig,
    /// 회전하며 커지며 나타남 (작게 시작)
    RotateSmall,
}

/// 덧씌울 그림 한 장 (화면에서 만들어 보낸다)
pub struct Overlay {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
    /// 프레임에 놓일 왼쪽 위 위치
    pub x: f32,
    pub y: f32,
    pub anim_in: Anim,
    pub anim_out: Anim,
    pub in_ms: f32,
    pub out_ms: f32,
}

impl Overlay {
    /// 시각 tm(구간 안, ms)에 그린다. 구간 길이는 total.
    pub fn draw(&self, px: &mut [u8], fw: u32, fh: u32, tm: f32, total: f32) {
        let mut alpha = 1.0f32;
        let mut scale = 1.0f32;
        let mut angle = 0.0f32;
        let (mut dx, mut dy) = (0.0f32, 0.0f32);
        let mut phase = |anim: Anim, p: f32| {
            let p = p.clamp(0.0, 1.0);
            let q = 1.0 - p; // 아직 남은 움직임
            match anim {
                Anim::None => {}
                Anim::Fade => alpha *= p,
                Anim::ZoomIn => scale *= p,
                Anim::ZoomOut => {
                    scale *= 1.0 + q;
                    alpha *= p;
                }
                Anim::Left => dx -= (self.x + self.w as f32) * q,
                Anim::Right => dx += (fw as f32 - self.x) * q,
                Anim::Top => dy -= (self.y + self.h as f32) * q,
                Anim::Bottom => dy += (fh as f32 - self.y) * q,
                Anim::FadeSmall => {
                    scale *= p;
                    alpha *= p;
                }
                Anim::RotateBig => {
                    scale *= 1.0 + q;
                    alpha *= p;
                    angle += q * std::f32::consts::TAU;
                }
                Anim::RotateSmall => {
                    scale *= p;
                    angle += q * std::f32::consts::TAU;
                }
            }
        };
        if self.anim_in != Anim::None && self.in_ms > 0.0 {
            phase(self.anim_in, tm / self.in_ms);
        }
        if self.anim_out != Anim::None && self.out_ms > 0.0 {
            phase(self.anim_out, (total - tm) / self.out_ms);
        }
        if alpha <= 0.0 || scale <= 0.01 {
            return;
        }
        let cx = self.x + self.w as f32 / 2.0 + dx;
        let cy = self.y + self.h as f32 / 2.0 + dy;
        composite_rotated(px, fw, fh, &self.rgba, self.w, self.h, cx, cy, scale, angle, alpha);
    }
}

/// 그림을 (cx, cy) 중심에 scale배로, alpha만큼 덮는다 (쌍선형 보간)
#[allow(clippy::too_many_arguments)]
pub fn composite(
    dst: &mut [u8],
    fw: u32,
    fh: u32,
    src: &[u8],
    sw: u32,
    sh: u32,
    cx: f32,
    cy: f32,
    scale: f32,
    alpha: f32,
) {
    composite_rotated(dst, fw, fh, src, sw, sh, cx, cy, scale, 0.0, alpha);
}

/// composite에 회전(angle, 라디안, 시계 방향)을 더한 것
#[allow(clippy::too_many_arguments)]
pub fn composite_rotated(
    dst: &mut [u8],
    fw: u32,
    fh: u32,
    src: &[u8],
    sw: u32,
    sh: u32,
    cx: f32,
    cy: f32,
    scale: f32,
    angle: f32,
    alpha: f32,
) {
    let (sin, cos) = angle.sin_cos();
    let rotated = angle.abs() > 1e-4;
    // 돌아간 그림을 다 덮는 상자
    let (hw0, hh0) = (sw as f32 * scale / 2.0, sh as f32 * scale / 2.0);
    let (hw, hh) =
        if rotated { (hw0 * cos.abs() + hh0 * sin.abs(), hw0 * sin.abs() + hh0 * cos.abs()) } else { (hw0, hh0) };
    let x0 = ((cx - hw).floor() as i32).max(0);
    let y0 = ((cy - hh).floor() as i32).max(0);
    let x1 = ((cx + hw).ceil() as i32).min(fw as i32);
    let y1 = ((cy + hh).ceil() as i32).min(fh as i32);
    let exact = (scale - 1.0).abs() < 1e-4 && !rotated;
    let sample = |sx: f32, sy: f32| -> [f32; 4] {
        // 그림 밖은 투명. 그림 안에서는 가장자리 픽셀을 늘려 써서 테두리가 반투명해지지 않게 한다.
        if sx < 0.0 || sy < 0.0 || sx >= sw as f32 || sy >= sh as f32 {
            return [0.0; 4];
        }
        let get = |x: i32, y: i32| -> [f32; 4] {
            let (x, y) = (x.clamp(0, sw as i32 - 1) as u32, y.clamp(0, sh as i32 - 1) as u32);
            let i = ((y * sw + x) * 4) as usize;
            let a = src[i + 3] as f32 / 255.0;
            // 미리 곱한 알파로 섞어야 가장자리가 검게 번지지 않는다
            [src[i] as f32 * a, src[i + 1] as f32 * a, src[i + 2] as f32 * a, a]
        };
        if exact {
            return get(sx.floor() as i32, sy.floor() as i32);
        }
        let (fx, fy) = (sx - 0.5, sy - 0.5);
        let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
        let (tx, ty) = (fx - ix as f32, fy - iy as f32);
        let (a, b, c, d) = (get(ix, iy), get(ix + 1, iy), get(ix, iy + 1), get(ix + 1, iy + 1));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bot = c[k] + (d[k] - c[k]) * tx;
            top + (bot - top) * ty
        })
    };
    for y in y0..y1 {
        for x in x0..x1 {
            // 화면 좌표를 그림 좌표로 되돌린다 (회전도 거꾸로)
            let (ddx, ddy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let (rx, ry) = if rotated { (cos * ddx + sin * ddy, -sin * ddx + cos * ddy) } else { (ddx, ddy) };
            let sx = rx / scale + sw as f32 / 2.0;
            let sy = ry / scale + sh as f32 / 2.0;
            let s = sample(sx, sy);
            let a = s[3] * alpha;
            if a <= 0.0 {
                continue;
            }
            let i = ((y as u32 * fw + x as u32) * 4) as usize;
            for k in 0..3 {
                // s[k]는 이미 알파가 곱해진 값
                dst[i + k] = clamp8(s[k] * alpha + dst[i + k] as f32 * (1.0 - a));
            }
            dst[i + 3] = clamp8(a * 255.0 + dst[i + 3] as f32 * (1.0 - a));
        }
    }
}

// ---------- 줌 애니메이션 ----------

/// 확대 배율 고르기 (적당한 크기 / 최대의 절반 / 최대의 75% / 꽉 차게)
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ZoomScale {
    GoodFit,
    Half,
    ThreeQuarter,
    Full,
}

/// 영역을 확대해서 보여주는 효과. 기본값: 중앙으로, 그림자 60%, 총 2초, 등장·퇴장 0.3초
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ZoomFx {
    pub region: Region,
    pub scale: ZoomScale,
    /// 확대하면서 화면 가운데로 옮긴다 (끄면 제자리에서 커진다)
    pub to_center: bool,
    pub shadow: bool,
    /// 그림자 퍼짐 0 ~ 100
    pub spread: f32,
    pub total_ms: u32,
    pub intro_ms: u32,
    pub outro_ms: u32,
}

impl ZoomFx {
    /// 실제 배율. 확대한 영역이 프레임 밖으로 나가지 않는 배율이 "꽉 차게"다.
    pub fn factor(&self, w: u32, h: u32) -> f32 {
        let max = (w as f32 / self.region.w.max(1) as f32).min(h as f32 / self.region.h.max(1) as f32).max(1.0);
        let f = match self.scale {
            ZoomScale::GoodFit => (max * 0.85).min(3.0),
            ZoomScale::Half => max * 0.5,
            ZoomScale::ThreeQuarter => max * 0.75,
            ZoomScale::Full => max,
        };
        f.max(1.0)
    }

    /// 시각 tm(효과 시작부터 ms)에서 확대 진행 정도 0 ~ 1. total은 실제 효과 길이.
    pub fn progress(&self, tm: f32, total: f32) -> f32 {
        let (mut intro, mut outro) = (self.intro_ms as f32, self.outro_ms as f32);
        // 효과가 짧으면 등장·퇴장 시간을 같은 비율로 줄인다
        if intro + outro > total && intro + outro > 0.0 {
            let k = total / (intro + outro);
            intro *= k;
            outro *= k;
        }
        let p = if intro > 0.0 && tm < intro {
            tm / intro
        } else if outro > 0.0 && tm > total - outro {
            (total - tm) / outro
        } else {
            1.0
        };
        let p = p.clamp(0.0, 1.0);
        p * p * (3.0 - 2.0 * p) // 부드럽게 시작하고 끝난다
    }
}

/// 진행 정도 p(0 ~ 1)만큼 영역을 확대해 그린다. 확대되는 내용은 이 프레임의 같은 영역이다.
pub fn draw_zoom(px: &mut [u8], w: u32, h: u32, z: &ZoomFx, p: f32) {
    if p <= 0.0 {
        return;
    }
    let (wi, hi) = (w as i32, h as i32);
    let r = z.region;
    let rx = r.x.clamp(0, wi - 1);
    let ry = r.y.clamp(0, hi - 1);
    let rw = r.w.clamp(1, wi - rx);
    let rh = r.h.clamp(1, hi - ry);
    // 영역을 먼저 떼어 둔다 (그 위에 그림을 그리기 전에)
    let mut src = Vec::with_capacity((rw * rh * 4) as usize);
    for y in ry..ry + rh {
        let i = ((y * wi + rx) * 4) as usize;
        src.extend_from_slice(&px[i..i + (rw * 4) as usize]);
    }
    let scale = 1.0 + (z.factor(w, h) - 1.0) * p;
    let (pw, ph) = (rw as f32 * scale, rh as f32 * scale);
    let (c0x, c0y) = (rx as f32 + rw as f32 / 2.0, ry as f32 + rh as f32 / 2.0);
    let (c1x, c1y) = if z.to_center {
        (w as f32 / 2.0, h as f32 / 2.0)
    } else {
        // 제자리에서 커지되 프레임 밖으로는 나가지 않게
        let fit =
            |c: f32, half: f32, size: f32| if half * 2.0 >= size { size / 2.0 } else { c.clamp(half, size - half) };
        (fit(c0x, pw / 2.0, w as f32), fit(c0y, ph / 2.0, h as f32))
    };
    let cx = c0x + (c1x - c0x) * p;
    let cy = c0y + (c1y - c0y) * p;
    if z.shadow {
        draw_shadow(px, w, h, cx - pw / 2.0, cy - ph / 2.0, pw, ph, z.spread, p);
    }
    composite(px, w, h, &src, rw as u32, rh as u32, cx, cy, scale, 1.0);
}

/// 사각형 둘레에 부드러운 그림자 (오른쪽 아래로 조금 치우치게)
#[allow(clippy::too_many_arguments)]
fn draw_shadow(px: &mut [u8], w: u32, h: u32, x: f32, y: f32, pw: f32, ph: f32, spread: f32, p: f32) {
    let rad = spread.clamp(0.0, 100.0) / 100.0 * 0.25 * pw.min(ph) + 2.0;
    let off = rad * 0.3;
    let (l, t, r, b) = (x + off, y + off, x + pw + off, y + ph + off);
    let max_a = 0.55 * p;
    let x0 = ((l - rad).floor() as i32).max(0);
    let y0 = ((t - rad).floor() as i32).max(0);
    let x1 = ((r + rad).ceil() as i32).min(w as i32);
    let y1 = ((b + rad).ceil() as i32).min(h as i32);
    for yy in y0..y1 {
        for xx in x0..x1 {
            let (fx, fy) = (xx as f32 + 0.5, yy as f32 + 0.5);
            let dx = (l - fx).max(fx - r).max(0.0);
            let dy = (t - fy).max(fy - b).max(0.0);
            let k = (1.0 - (dx * dx + dy * dy).sqrt() / rad).clamp(0.0, 1.0);
            let a = max_a * k * k;
            if a <= 0.0 {
                continue;
            }
            let i = ((yy as u32 * w + xx as u32) * 4) as usize;
            for c in &mut px[i..i + 3] {
                *c = clamp8(*c as f32 * (1.0 - a));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        (0..w * h).flat_map(|_| rgba).collect()
    }

    #[test]
    fn color_filters() {
        let mut p = img(2, 2, [100, 150, 200, 255]);
        apply_filter(&mut p, 2, 2, &Filter::Grayscale);
        assert!(p[0] == p[1] && p[1] == p[2]);
        let mut p = img(1, 1, [100, 100, 100, 255]);
        apply_filter(&mut p, 1, 1, &Filter::Brightness { value: 100.0 });
        assert_eq!(p[0], 255);
        let mut p = img(1, 1, [10, 20, 30, 255]);
        apply_filter(&mut p, 1, 1, &Filter::Invert);
        assert_eq!(&p[..4], &[245, 235, 225, 255]);
    }

    #[test]
    fn blur_keeps_flat_image_and_region() {
        let mut p = img(8, 8, [50, 60, 70, 255]);
        apply_filter(&mut p, 8, 8, &Filter::Blur { radius: 3, region: None });
        assert_eq!(&p[..4], &[50, 60, 70, 255]);
        // 왼쪽 반은 흰색, 오른쪽 반은 검정. 왼쪽 위 2x2만 흐리게 하면 나머지는 그대로
        let mut p: Vec<u8> = (0..64).flat_map(|i| if i % 8 < 4 { [255; 4] } else { [0, 0, 0, 255] }).collect();
        let before = p.clone();
        let r = Region { x: 2, y: 0, w: 4, h: 2, ellipse: false };
        apply_filter(&mut p, 8, 8, &Filter::Blur { radius: 2, region: Some(r) });
        assert_ne!(p[..32], before[..32]); // 첫 줄 경계는 섞였다
        assert_eq!(p[8 * 4 * 4..], before[8 * 4 * 4..]); // 아래쪽은 그대로
    }

    #[test]
    fn mosaic_averages_blocks() {
        let mut p: Vec<u8> = [0u8, 0, 0, 255, 200, 200, 200, 255].repeat(2);
        apply_filter(&mut p, 2, 2, &Filter::Mosaic { size: 2, region: None });
        assert_eq!(&p[..3], &[100, 100, 100]);
    }

    #[test]
    fn transitions_fade() {
        let first = img(1, 1, [0, 0, 0, 255]);
        let mut p = img(1, 1, [0, 0, 0, 255]);
        // 맨 처음은 거의 흰색
        apply_transition(&mut p, TransitionKind::FadeInWhite, 0.0, 1000.0, 500.0, &first);
        assert_eq!(p[0], 255);
        let mut p = img(1, 1, [0, 0, 0, 255]);
        apply_transition(&mut p, TransitionKind::FadeInWhite, 600.0, 1000.0, 500.0, &first);
        assert_eq!(p[0], 0);
    }

    #[test]
    fn rounded_corners_are_transparent() {
        let mut p = img(20, 20, [9, 9, 9, 255]);
        round_corners(&mut p, 20, 20, 6);
        assert_eq!(p[3], 0);
        assert_eq!(p[(10 * 20 + 10) * 4 + 3], 255);
    }

    #[test]
    fn rotated_composite_quarter_turn() {
        // 가로로 긴 2x1 (왼쪽 빨강, 오른쪽 파랑)을 90도 돌리면 세로가 되고 위가 빨강... (시계 방향이면 위=빨강)
        let src = [255u8, 0, 0, 255, 0, 0, 255, 255];
        let mut dst = vec![0u8; 3 * 3 * 4];
        composite_rotated(&mut dst, 3, 3, &src, 2, 1, 1.5, 1.5, 1.0, std::f32::consts::FRAC_PI_2, 1.0);
        // 가운데 열에만 그려진다
        assert_eq!(dst[(0 * 3 + 0) * 4 + 3], 0);
        assert!(dst[(1 * 3 + 1) * 4 + 3] > 0);
    }

    #[test]
    fn overlay_composite_and_anim() {
        let ov = Overlay {
            rgba: img(2, 2, [255, 0, 0, 255]),
            w: 2,
            h: 2,
            x: 1.0,
            y: 1.0,
            anim_in: Anim::Fade,
            anim_out: Anim::None,
            in_ms: 100.0,
            out_ms: 0.0,
        };
        let mut p = img(4, 4, [0, 0, 255, 255]);
        ov.draw(&mut p, 4, 4, 500.0, 1000.0);
        let i = (1 * 4 + 1) * 4;
        assert_eq!(&p[i..i + 4], &[255, 0, 0, 255]);
        assert_eq!(&p[..4], &[0, 0, 255, 255]);
        // 등장 중간(50%)이면 반쯤 섞인다
        let mut p = img(4, 4, [0, 0, 255, 255]);
        ov.draw(&mut p, 4, 4, 50.0, 1000.0);
        assert!((120..=135).contains(&p[i]));
    }

    #[test]
    fn zoom_progress_and_draw() {
        let z = ZoomFx {
            region: Region { x: 0, y: 0, w: 2, h: 2, ellipse: false },
            scale: ZoomScale::Full,
            to_center: true,
            shadow: false,
            spread: 0.0,
            total_ms: 1000,
            intro_ms: 200,
            outro_ms: 200,
        };
        assert_eq!(z.progress(0.0, 1000.0), 0.0);
        assert_eq!(z.progress(500.0, 1000.0), 1.0);
        assert_eq!(z.progress(1000.0, 1000.0), 0.0);
        // 효과가 짧으면 등장·퇴장이 같이 줄어든다: 200ms짜리면 100ms에서 최대
        assert_eq!(z.progress(100.0, 200.0), 1.0);
        // 8x8 프레임의 왼쪽 위 2x2(빨강)를 꽉 차게(4배) 확대하면 프레임 전체가 빨강
        let mut px: Vec<u8> =
            (0..64).flat_map(|i| if i % 8 < 2 && i / 8 < 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        assert_eq!(z.factor(8, 8), 4.0);
        draw_zoom(&mut px, 8, 8, &z, 1.0);
        assert!(px.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
    }
}

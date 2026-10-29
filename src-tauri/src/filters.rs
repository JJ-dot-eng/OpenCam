//! 필터 2차분: 배경 흐림, 색상 곡선, 색상 채널, 화이트밸런스, 노이즈 감소, 뽀샤시, 카툰, 판화,
//! 아스키 아트, 잔상 효과, 크로마키, 지우개. 모두 RGBA 한 장을 제자리에서 바꾼다(잔상 효과만 프레임 사이).

use crate::effects::{Region, Rgb, box_blur};

fn clamp8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

fn luma(p: &[u8]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 배경 흐림: 타원 안은 또렷하고 밖으로 갈수록 흐리게. 위치·크기는 프레임에 대한 %
#[allow(clippy::too_many_arguments)]
pub fn iris_blur(px: &mut [u8], w: u32, h: u32, cx: f32, cy: f32, rx: f32, ry: f32, radius: u32, feather: f32) {
    let mut soft = px.to_vec();
    box_blur(&mut soft, w, h, radius.clamp(1, 100), 3);
    let (cx, cy) = (w as f32 * cx / 100.0, h as f32 * cy / 100.0);
    let (rx, ry) = ((w as f32 * rx / 100.0).max(1.0), (h as f32 * ry / 100.0).max(1.0));
    let f = (feather / 100.0).max(0.01);
    for y in 0..h {
        for x in 0..w {
            let d = (((x as f32 + 0.5 - cx) / rx).powi(2) + ((y as f32 + 0.5 - cy) / ry).powi(2)).sqrt();
            let t = smoothstep(1.0, 1.0 + f, d);
            if t <= 0.0 {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            for k in 0..3 {
                px[i + k] = clamp8(px[i + k] as f32 + (soft[i + k] as f32 - px[i + k] as f32) * t);
            }
        }
    }
}

/// 색상 곡선: (0,0) (64,어두운 곳) (128,중간) (192,밝은 곳) (255,255)을 지나는 부드러운 곡선.
/// 넘치지 않게 단조 3차 보간(PCHIP)을 쓴다.
pub fn curve_lut(shadows: f32, midtones: f32, highlights: f32) -> [u8; 256] {
    let xs = [0.0f32, 64.0, 128.0, 192.0, 255.0];
    let ys = [0.0f32, shadows.clamp(0.0, 255.0), midtones.clamp(0.0, 255.0), highlights.clamp(0.0, 255.0), 255.0];
    let n = xs.len();
    let d: Vec<f32> = (0..n - 1).map(|i| (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i])).collect();
    let mut m = vec![0.0f32; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 { 0.0 } else { 2.0 / (1.0 / d[i - 1] + 1.0 / d[i]) };
    }
    let mut lut = [0u8; 256];
    for (v, out) in lut.iter_mut().enumerate() {
        let x = v as f32;
        let i = (0..n - 1).find(|&i| x <= xs[i + 1]).unwrap_or(n - 2);
        let hgt = xs[i + 1] - xs[i];
        let t = (x - xs[i]) / hgt;
        let (t2, t3) = (t * t, t * t * t);
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * ys[i]
            + (t3 - 2.0 * t2 + t) * hgt * m[i]
            + (-2.0 * t3 + 3.0 * t2) * ys[i + 1]
            + (t3 - t2) * hgt * m[i + 1];
        *out = clamp8(y);
    }
    lut
}

pub fn apply_curve(px: &mut [u8], lut: &[u8; 256]) {
    for p in px.chunks_exact_mut(4) {
        for c in &mut p[..3] {
            *c = lut[*c as usize];
        }
    }
}

/// 색상 채널: 밝기와 빨강·녹색·파랑을 따로 (-100 ~ 100)
pub fn channels(px: &mut [u8], brightness: f32, rgb: [f32; 3]) {
    let b = brightness.clamp(-100.0, 100.0) * 2.55;
    let k = rgb.map(|v| 1.0 + v.clamp(-100.0, 100.0) / 100.0);
    for p in px.chunks_exact_mut(4) {
        for c in 0..3 {
            p[c] = clamp8(p[c] as f32 * k[c] + b);
        }
    }
}

/// 화이트밸런스. reference가 있으면 그 색이 흰색이 되게, 없으면 전체 평균이 회색이 되게(자동)
pub fn white_balance(px: &mut [u8], reference: Option<Rgb>) {
    let gains = match reference {
        Some([r, g, b]) => [r, g, b].map(|v| 255.0 / (v.max(1) as f32)),
        None => {
            let mut sum = [0f64; 3];
            let mut n = 0f64;
            for p in px.chunks_exact(4) {
                if p[3] == 0 {
                    continue;
                }
                for c in 0..3 {
                    sum[c] += p[c] as f64;
                }
                n += 1.0;
            }
            if n == 0.0 {
                return;
            }
            let avg = sum.map(|s| (s / n) as f32);
            let gray = (avg[0] + avg[1] + avg[2]) / 3.0;
            avg.map(|a| (gray / a.max(1.0)).clamp(0.5, 2.0))
        }
    };
    for p in px.chunks_exact_mut(4) {
        for c in 0..3 {
            p[c] = clamp8(p[c] as f32 * gains[c]);
        }
    }
}

/// 노이즈 감소: 3×3 가운데값(median)을 strength번
pub fn denoise(px: &mut [u8], w: u32, h: u32, strength: u32) {
    let (w, h) = (w as usize, h as usize);
    for _ in 0..strength.clamp(1, 3) {
        let src = px.to_vec();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                for c in 0..3 {
                    let mut v = [0u8; 9];
                    let mut k = 0;
                    for dy in 0..3 {
                        for dx in 0..3 {
                            let yy = (y + dy).saturating_sub(1).min(h - 1);
                            let xx = (x + dx).saturating_sub(1).min(w - 1);
                            v[k] = src[(yy * w + xx) * 4 + c];
                            k += 1;
                        }
                    }
                    v.sort_unstable();
                    px[i + c] = v[4];
                }
            }
        }
    }
}

/// 뽀샤시: 흐린 사본을 섞는다. shiny(화사하게)면 밝게 빛나게(스크린 합성), 아니면 부드럽게
pub fn bloom(px: &mut [u8], w: u32, h: u32, radius: u32, amount: f32, shiny: bool) {
    let a = amount.clamp(0.0, 100.0) / 100.0;
    let mut soft = px.to_vec();
    box_blur(&mut soft, w, h, radius.clamp(1, 60), 3);
    for (p, s) in px.chunks_exact_mut(4).zip(soft.chunks_exact(4)) {
        for c in 0..3 {
            let (o, b) = (p[c] as f32 / 255.0, s[c] as f32 / 255.0);
            let v = if shiny { 1.0 - (1.0 - o) * (1.0 - b * a) } else { o + (b - o) * a * 0.7 };
            p[c] = clamp8(v * 255.0);
        }
    }
}

/// 카툰: 색을 몇 단계로 줄이고 윤곽선을 검게
pub fn cartoon(px: &mut [u8], w: u32, h: u32, edges: f32, levels: u32) {
    let (wi, hi) = (w as usize, h as usize);
    let mut flat = px.to_vec();
    denoise(&mut flat, w, h, 1);
    let lv = levels.clamp(2, 16) as f32;
    let step = 255.0 / (lv - 1.0);
    let lum: Vec<f32> = px.chunks_exact(4).map(luma).collect();
    let at = |x: usize, y: usize| lum[y.min(hi - 1) * wi + x.min(wi - 1)];
    // 윤곽 세기가 이 값을 넘으면 선 (edges가 클수록 선이 많다)
    let thr = 255.0 * (1.0 - edges.clamp(0.0, 100.0) / 100.0).max(0.05);
    for y in 0..hi {
        for x in 0..wi {
            let (xm, ym) = (x.saturating_sub(1), y.saturating_sub(1));
            let gx =
                at(x + 1, ym) + 2.0 * at(x + 1, y) + at(x + 1, y + 1) - at(xm, ym) - 2.0 * at(xm, y) - at(xm, y + 1);
            let gy =
                at(xm, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1) - at(xm, ym) - 2.0 * at(x, ym) - at(x + 1, ym);
            let i = (y * wi + x) * 4;
            if (gx * gx + gy * gy).sqrt() > thr {
                px[i..i + 3].fill(0);
            } else {
                for c in 0..3 {
                    px[i + c] = clamp8((flat[i + c] as f32 / step).round() * step);
                }
            }
        }
    }
}

/// 판화: 밝기가 threshold(0 ~ 100%)를 넘으면 흰색, 아니면 검정
pub fn print(px: &mut [u8], threshold: f32) {
    let t = threshold.clamp(0.0, 100.0) * 2.55;
    for p in px.chunks_exact_mut(4) {
        let v = if luma(p) > t { 255 } else { 0 };
        p[..3].fill(v);
    }
}

/// 아스키 아트용 5×7 글자 (어두운 것부터 밝은 것 순서)
const ASCII_GLYPHS: [[u8; 7]; 10] = [
    [0, 0, 0, 0, 0, 0, 0],                                           // ' '
    [0, 0, 0, 0, 0, 0, 0b00100],                                     // '.'
    [0, 0, 0b00100, 0, 0, 0b00100, 0],                               // ':'
    [0, 0, 0, 0b01110, 0, 0, 0],                                     // '-'
    [0, 0, 0b11111, 0, 0b11111, 0, 0],                               // '='
    [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],             // '+'
    [0, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0],             // '*'
    [0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010], // '#'
    [0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011], // '%'
    [0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01111], // '@'
];

/// 아스키 아트: cell 크기 칸마다 밝기에 맞는 글자를 그린다. color면 글자를 그 칸의 평균 색으로
pub fn ascii(px: &mut [u8], w: u32, h: u32, cell: u32, color: bool) {
    let cell = cell.clamp(4, 64);
    let src = px.to_vec();
    for p in px.chunks_exact_mut(4) {
        p[..3].fill(0);
    }
    for by in (0..h).step_by(cell as usize) {
        for bx in (0..w).step_by(cell as usize) {
            let (x1, y1) = ((bx + cell).min(w), (by + cell).min(h));
            let mut sum = [0u32; 3];
            let mut n = 0;
            for y in by..y1 {
                for x in bx..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..3 {
                        sum[c] += src[i + c] as u32;
                    }
                    n += 1;
                }
            }
            let avg = sum.map(|s| (s / n.max(1)) as u8);
            let l = luma(&[avg[0], avg[1], avg[2], 255]);
            let glyph = &ASCII_GLYPHS[((l / 256.0) * 10.0) as usize];
            let ink = if color { avg } else { [255, 255, 255] };
            // 5×7 글자를 칸 크기에 맞게 늘린다 (둘레는 조금 비운다)
            let (gw, gh) = ((x1 - bx) as f32, (y1 - by) as f32);
            for y in by..y1 {
                for x in bx..x1 {
                    let gx = (((x - bx) as f32 + 0.5) / gw * 7.0 - 1.0).floor();
                    let gy = (((y - by) as f32 + 0.5) / gh * 9.0 - 1.0).floor();
                    if !(0.0..5.0).contains(&gx) || !(0.0..7.0).contains(&gy) {
                        continue;
                    }
                    if glyph[gy as usize] & (0b10000 >> gx as u32) != 0 {
                        let i = ((y * w + x) * 4) as usize;
                        px[i..i + 3].copy_from_slice(&ink);
                    }
                }
            }
        }
    }
}

/// 잔상 효과: 앞 프레임(이미 잔상이 섞인 것)을 strength만큼 남긴다
pub fn afterimage(px: &mut [u8], prev: &[u8], strength: f32) {
    if prev.len() != px.len() {
        return;
    }
    let s = strength.clamp(0.0, 95.0) / 100.0;
    for (p, q) in px.iter_mut().zip(prev) {
        *p = clamp8(*p as f32 * (1.0 - s) + *q as f32 * s);
    }
}

/// 크로마키: key 색과 range(%) 안의 색을 투명하게(또는 replace 색으로).
/// adjacent면 가장자리에서 이어진 부분만 (그림 속 같은 색은 남긴다)
pub fn chroma_key(px: &mut [u8], w: u32, h: u32, key: Rgb, range: f32, adjacent: bool, replace: Option<Rgb>) {
    let r = (range.clamp(0.0, 100.0) / 100.0 * 441.7).max(1.0);
    let dist = |p: &[u8]| {
        let d: f32 = (0..3).map(|c| (p[c] as f32 - key[c] as f32).powi(2)).sum();
        d.sqrt()
    };
    let n = (w * h) as usize;
    let mut hit = vec![false; n];
    if adjacent {
        // 가장자리에서 시작해 이어진 key 색만 (넓이 우선 탐색)
        let mut queue = std::collections::VecDeque::new();
        let wi = w as usize;
        for i in 0..n {
            let (x, y) = (i % wi, i / wi);
            if (x == 0 || y == 0 || x == wi - 1 || y == h as usize - 1) && dist(&px[i * 4..i * 4 + 4]) <= r {
                hit[i] = true;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i % wi, i / wi);
            let mut visit = |j: usize| {
                if !hit[j] && dist(&px[j * 4..j * 4 + 4]) <= r {
                    hit[j] = true;
                    queue.push_back(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < wi {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - wi);
            }
            if y + 1 < h as usize {
                visit(i + wi);
            }
        }
    } else {
        for (i, h) in hit.iter_mut().enumerate() {
            *h = dist(&px[i * 4..i * 4 + 4]) <= r;
        }
    }
    for (i, p) in px.chunks_exact_mut(4).enumerate() {
        if !hit[i] {
            continue;
        }
        match replace {
            Some(c) => {
                p[..3].copy_from_slice(&c);
                p[3] = 255;
            }
            None => p[3] = 0,
        }
    }
}

/// 지우개: 영역 안을 둘레 색으로 채운다 (가장자리부터 안쪽으로 이웃 평균을 번지게)
pub fn eraser(px: &mut [u8], w: u32, h: u32, region: Region) {
    let (wi, hi) = (w as i32, h as i32);
    let (x0, y0) = (region.x.max(0), region.y.max(0));
    let (x1, y1) = ((region.x + region.w).min(wi), (region.y + region.h).min(hi));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let inside = |x: i32, y: i32| {
        if !region.ellipse {
            return x >= x0 && x < x1 && y >= y0 && y < y1;
        }
        let rx = region.w as f32 / 2.0;
        let ry = region.h as f32 / 2.0;
        let dx = (x as f32 + 0.5 - region.x as f32 - rx) / rx;
        let dy = (y as f32 + 0.5 - region.y as f32 - ry) / ry;
        dx * dx + dy * dy <= 1.0
    };
    let mut known = vec![true; (w * h) as usize];
    let mut todo = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            if inside(x, y) {
                known[(y * wi + x) as usize] = false;
                todo += 1;
            }
        }
    }
    // 바깥에서 안으로 한 겹씩
    while todo > 0 {
        let mut layer = vec![];
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * wi + x) as usize;
                if known[i] {
                    continue;
                }
                let mut sum = [0u32; 4];
                let mut n = 0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (xx, yy) = (x + dx, y + dy);
                        if (dx, dy) == (0, 0) || xx < 0 || yy < 0 || xx >= wi || yy >= hi {
                            continue;
                        }
                        let j = (yy * wi + xx) as usize;
                        if known[j] {
                            for c in 0..4 {
                                sum[c] += px[j * 4 + c] as u32;
                            }
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    layer.push((i, sum.map(|s| (s / n) as u8)));
                }
            }
        }
        if layer.is_empty() {
            break; // 프레임 전체가 영역이면 채울 수 없다
        }
        for (i, c) in layer {
            px[i * 4..i * 4 + 4].copy_from_slice(&c);
            known[i] = true;
            todo -= 1;
        }
    }
    // 채운 곳의 얼룩을 조금 부드럽게
    for _ in 0..2 {
        let src = px.to_vec();
        for y in y0..y1 {
            for x in x0..x1 {
                if !inside(x, y) {
                    continue;
                }
                let mut sum = [0u32; 3];
                let mut n = 0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (xx, yy) = ((x + dx).clamp(0, wi - 1), (y + dy).clamp(0, hi - 1));
                        let j = ((yy * wi + xx) * 4) as usize;
                        for c in 0..3 {
                            sum[c] += src[j + c] as u32;
                        }
                        n += 1;
                    }
                }
                let i = ((y * wi + x) * 4) as usize;
                for c in 0..3 {
                    px[i + c] = (sum[c] / n) as u8;
                }
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
    fn curve_identity_and_monotone() {
        let lut = curve_lut(64.0, 128.0, 192.0);
        assert!(lut.iter().enumerate().all(|(i, &v)| (v as i32 - i as i32).abs() <= 1));
        let lut = curve_lut(100.0, 180.0, 230.0); // 밝게
        assert!(lut.windows(2).all(|w| w[1] >= w[0]));
        assert!(lut[128] > 170);
    }

    #[test]
    fn white_balance_reference_and_auto() {
        let mut p = img(1, 1, [200, 220, 250, 255]);
        white_balance(&mut p, Some([200, 220, 250]));
        assert_eq!(&p[..3], &[255, 255, 255]);
        // 파랗게 치우친 그림은 자동으로 회색 쪽으로
        let mut p = img(2, 2, [80, 100, 160, 255]);
        white_balance(&mut p, None);
        assert!((p[0] as i32 - p[2] as i32).abs() < 10);
    }

    #[test]
    fn denoise_removes_speck() {
        let mut p = img(5, 5, [100, 100, 100, 255]);
        p[(2 * 5 + 2) * 4] = 255; // 가운데 점 하나
        denoise(&mut p, 5, 5, 1);
        assert_eq!(p[(2 * 5 + 2) * 4], 100);
    }

    #[test]
    fn chroma_key_adjacent_only() {
        // 초록 배경, 가운데에 빨강, 빨강 안에 초록 점 하나 → 인접만이면 그 점은 남는다
        let mut p = img(5, 5, [0, 255, 0, 255]);
        for y in 1..4 {
            for x in 1..4 {
                p[((y * 5 + x) * 4) as usize..][..3].copy_from_slice(&[255, 0, 0]);
            }
        }
        p[(2 * 5 + 2) * 4..][..3].copy_from_slice(&[0, 255, 0]);
        let mut a = p.clone();
        chroma_key(&mut a, 5, 5, [0, 255, 0], 10.0, true, None);
        assert_eq!(a[3], 0); // 배경은 투명
        assert_eq!(a[(2 * 5 + 2) * 4 + 3], 255); // 갇힌 초록 점은 그대로
        chroma_key(&mut p, 5, 5, [0, 255, 0], 10.0, false, Some([9, 9, 9]));
        assert_eq!(&p[(2 * 5 + 2) * 4..][..4], &[9, 9, 9, 255]);
    }

    #[test]
    fn eraser_fills_from_border() {
        let mut p = img(6, 6, [50, 60, 70, 255]);
        p[(3 * 6 + 3) * 4..][..3].copy_from_slice(&[255, 0, 0]); // 지울 빨간 점
        eraser(&mut p, 6, 6, Region { x: 2, y: 2, w: 3, h: 3, ellipse: false });
        assert_eq!(&p[(3 * 6 + 3) * 4..][..3], &[50, 60, 70]);
    }

    #[test]
    fn print_ascii_cartoon_run() {
        let mut p: Vec<u8> = (0..64u32).flat_map(|i| [(i * 4) as u8, (i * 4) as u8, (i * 4) as u8, 255]).collect();
        let base = p.clone();
        print(&mut p, 50.0);
        assert!(p.chunks_exact(4).all(|c| c[0] == 0 || c[0] == 255));
        let mut a = base.clone();
        ascii(&mut a, 8, 8, 8, false);
        assert!(a.chunks_exact(4).any(|c| c[0] == 255)); // 밝은 칸에는 글자가 있다
        let mut c = base.clone();
        cartoon(&mut c, 8, 8, 50.0, 4);
        let mut s = base.clone();
        bloom(&mut s, 8, 8, 2, 50.0, true);
        assert!(s.chunks_exact(4).zip(base.chunks_exact(4)).all(|(a, b)| a[0] >= b[0]));
        let mut i = base;
        iris_blur(&mut i, 8, 8, 50.0, 50.0, 20.0, 20.0, 2, 30.0);
    }
}

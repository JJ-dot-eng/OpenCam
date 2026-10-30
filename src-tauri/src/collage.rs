//! 새 빈 영상, 콜라주 (여러 움짤을 한 화면에 칸으로 나눠 함께 재생).
//! 기본값: 선 폭 10, 가장자리 여백 0, 둥글기 0, 선 색·배경 흰색, 테두리 안 그림.

use crate::effects::{Rgb, round_corners};
use crate::frame::Frame;
use crate::import::{Fit, fit_rgba};
use anyhow::{Result, bail};
use rayon::prelude::*;
use serde::Deserialize;

/// 빈(한 색) 영상. fps마다 한 장, 총 sec초.
pub fn blank(w: u32, h: u32, fps: f32, sec: f32, rgba: [u8; 4]) -> Result<Vec<Frame>> {
    let (w, h) = (w.clamp(1, 8192), h.clamp(1, 8192));
    let fps = fps.clamp(1.0, 60.0);
    let n = ((sec.max(0.01) * fps).round() as u64).clamp(1, 36_000);
    let px: Vec<u8> = rgba.repeat((w * h) as usize);
    let base = Frame::from_rgba(w, h, &px, 0);
    let stamp = |k: u64| (k as f64 * 1000.0 / fps as f64).round() as u32;
    // 픽셀은 한 번만 압축해 두고 모든 프레임이 같이 쓴다
    Ok((0..n).map(|k| base.duplicate().with_delay((stamp(k + 1) - stamp(k)).max(10))).collect())
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PlayMode {
    /// 짧은 영상은 마지막 장면에서 멈춰 있다
    Hold,
    /// 짧은 영상은 가장 긴 영상이 끝날 때까지 반복
    Repeat,
    /// 모든 영상을 가장 긴 영상 길이에 맞춰 늘린다 (재생 시간 동기화)
    Sync,
    /// 한 칸씩 차례로 재생 (나머지 칸은 멈춰 있음)
    Sequence,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CollageOptions {
    pub cols: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
    /// 바깥 여백
    pub margin: u32,
    /// 칸 사이 선 폭
    pub gap: u32,
    /// 칸 모서리 둥글기 (픽셀)
    pub roundness: u32,
    /// 배경색과 불투명도(0 ~ 255)
    pub background: Rgb,
    pub background_alpha: u8,
    /// 테두리(칸 사이·바깥 선)를 선 색으로 칠한다
    pub draw_border: bool,
    pub border_color: Rgb,
    pub play: PlayMode,
}

/// 칸 위치들 (왼쪽 위부터 가로로)
fn cells(o: &CollageOptions) -> Vec<(u32, u32, u32, u32)> {
    let (cols, rows) = (o.cols.clamp(1, 10), o.rows.clamp(1, 10));
    let inner_w = o.width.saturating_sub(2 * o.margin + (cols - 1) * o.gap).max(cols);
    let inner_h = o.height.saturating_sub(2 * o.margin + (rows - 1) * o.gap).max(rows);
    let mut out = vec![];
    for r in 0..rows {
        for c in 0..cols {
            // 나머지 픽셀은 앞쪽 칸부터 1씩 나눠 준다
            let x0 = o.margin + c * o.gap + inner_w * c / cols;
            let x1 = o.margin + c * o.gap + inner_w * (c + 1) / cols;
            let y0 = o.margin + r * o.gap + inner_h * r / rows;
            let y1 = o.margin + r * o.gap + inner_h * (r + 1) / rows;
            out.push((x0, y0, x1 - x0, y1 - y0));
        }
    }
    out
}

/// 바탕: 배경 이미지(있으면) 또는 배경색, 테두리를 그리면 칸 바깥은 선 색
fn base_canvas(o: &CollageOptions, cells: &[(u32, u32, u32, u32)], bg_image: Option<&(u32, u32, Vec<u8>)>) -> Vec<u8> {
    let (w, h) = (o.width, o.height);
    let mut px = match bg_image {
        Some((bw, bh, img)) => fit_cover(*bw, *bh, img, w, h),
        None => {
            let [r, g, b] = o.background;
            [r, g, b, o.background_alpha].repeat((w * h) as usize)
        }
    };
    if o.draw_border {
        let [r, g, b] = o.border_color;
        let mut inside = vec![false; (w * h) as usize];
        for &(x, y, cw, ch) in cells {
            for yy in y..(y + ch).min(h) {
                for xx in x..(x + cw).min(w) {
                    inside[(yy * w + xx) as usize] = true;
                }
            }
        }
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            if !inside[i] {
                p.copy_from_slice(&[r, g, b, 255]);
            }
        }
    }
    px
}

/// 비율을 지키며 꽉 채우고 넘치는 부분은 잘라낸다 (배경 이미지용)
fn fit_cover(w: u32, h: u32, px: &[u8], tw: u32, th: u32) -> Vec<u8> {
    let s = (tw as f32 / w as f32).max(th as f32 / h as f32);
    let (nw, nh) = (((w as f32 * s).ceil() as u32).max(tw), ((h as f32 * s).ceil() as u32).max(th));
    let big = crate::project::resize_rgba(w, h, px, nw, nh, true);
    let (ox, oy) = ((nw - tw) / 2, (nh - th) / 2);
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        let s = (((oy + y) * nw + ox) * 4) as usize;
        out.extend_from_slice(&big[s..s + (tw * 4) as usize]);
    }
    out
}

/// 칸에 비율을 지켜 넣는다. 모서리 둥글기는 칸이 아니라 실제로 보이는 그림에 준다
/// (칸과 비율이 달라 위아래·좌우가 비면 그 빈 곳은 투명).
fn fit_rounded(w: u32, h: u32, px: &[u8], cw: u32, ch: u32, radius: u32) -> Vec<u8> {
    if radius == 0 {
        return fit_rgba(w, h, px, cw, ch, Fit::Contain);
    }
    let s = (cw as f32 / w as f32).min(ch as f32 / h as f32);
    let (nw, nh) = (((w as f32 * s).round() as u32).clamp(1, cw), ((h as f32 * s).round() as u32).clamp(1, ch));
    let mut small = crate::project::resize_rgba(w, h, px, nw, nh, true);
    round_corners(&mut small, nw, nh, radius);
    fit_rgba(nw, nh, &small, cw, ch, Fit::Contain)
}

/// 칸 그림(cw×ch)을 바탕 위에 알파 합성
fn paste(dst: &mut [u8], dw: u32, src: &[u8], x: u32, y: u32, cw: u32, ch: u32) {
    for yy in 0..ch {
        for xx in 0..cw {
            let s = ((yy * cw + xx) * 4) as usize;
            let d = (((y + yy) * dw + x + xx) * 4) as usize;
            let a = src[s + 3] as u32;
            if a == 0 {
                continue;
            }
            for k in 0..3 {
                dst[d + k] = ((src[s + k] as u32 * a + dst[d + k] as u32 * (255 - a)) / 255) as u8;
            }
            dst[d + 3] = (a + dst[d + 3] as u32 * (255 - a) / 255).min(255) as u8;
        }
    }
}

/// 한 영상의 프레임 시작 시각들과 전체 길이
fn starts(frames: &[Frame]) -> (Vec<u32>, u32) {
    let mut t = 0;
    let s = frames
        .iter()
        .map(|f| {
            let st = t;
            t += f.delay_ms;
            st
        })
        .collect();
    (s, t.max(1))
}

/// 영상 안의 시각 local에 보이는 프레임 번호
fn index_at(st: &[u32], local: u32) -> usize {
    st.partition_point(|&s| s <= local).saturating_sub(1)
}

pub fn make(clips: &[Vec<Frame>], o: &CollageOptions, bg_image: Option<(u32, u32, Vec<u8>)>) -> Result<Vec<Frame>> {
    let clips: Vec<&Vec<Frame>> = clips.iter().filter(|c| !c.is_empty()).collect();
    if clips.is_empty() {
        bail!("콜라주에 넣을 영상이 없습니다");
    }
    if o.width < 2 || o.height < 2 {
        bail!("캔버스가 너무 작습니다");
    }
    // 여백·선 폭이 캔버스를 다 차지하면 칸이 캔버스 밖으로 나간다 (칸마다 최소 1픽셀)
    let fits = |size: u32, n: u32| {
        let n = u64::from(n.clamp(1, 10));
        2 * u64::from(o.margin) + (n - 1) * u64::from(o.gap) + n <= u64::from(size)
    };
    if !fits(o.width, o.cols) || !fits(o.height, o.rows) {
        bail!("가장자리 여백과 선 폭이 캔버스보다 커서 칸을 넣을 자리가 없습니다");
    }
    let cells = cells(o);
    let used = clips.len().min(cells.len());
    let base = base_canvas(o, &cells, bg_image.as_ref());

    // 영상마다 칸 크기에 맞춘 그림을 미리 만들어 둔다 (모서리 둥글기도 여기서)
    let fitted: Vec<Vec<Vec<u8>>> = (0..used)
        .into_par_iter()
        .map(|i| {
            let (_, _, cw, ch) = cells[i];
            clips[i].iter().map(|f| fit_rounded(f.width, f.height, &f.rgba(), cw, ch, o.roundness)).collect()
        })
        .collect();

    let info: Vec<(Vec<u32>, u32)> = clips[..used].iter().map(|c| starts(c)).collect();
    let longest = info.iter().map(|(_, d)| *d).max().unwrap_or(1);
    let offsets: Vec<u32> = info
        .iter()
        .scan(0u32, |acc, (_, d)| {
            let o = *acc;
            *acc += d;
            Some(o)
        })
        .collect();
    let total = if o.play == PlayMode::Sequence { info.iter().map(|(_, d)| d).sum() } else { longest };

    // 어느 영상이든 장면이 바뀌는 시각을 모아 출력 프레임의 경계로 쓴다
    let mut cuts = vec![0u32];
    for (i, (st, d)) in info.iter().enumerate() {
        match o.play {
            PlayMode::Hold => cuts.extend(st.iter().copied()),
            PlayMode::Sync => cuts.extend(st.iter().map(|&s| (s as u64 * longest as u64 / *d as u64) as u32)),
            PlayMode::Repeat => {
                let mut k = 0;
                while k * d < total {
                    cuts.extend(st.iter().map(|&s| s + k * d).filter(|&t| t < total));
                    k += 1;
                }
            }
            PlayMode::Sequence => cuts.extend(st.iter().map(|&s| offsets[i] + s)),
        }
    }
    cuts.sort_unstable();
    // GIF는 10ms 단위라 너무 가까운 경계는 합친다
    let mut merged: Vec<u32> = vec![];
    for t in cuts {
        if t < total && merged.last().is_none_or(|&l| t >= l + 10) {
            merged.push(t);
        }
    }

    let frame_at = |i: usize, t: u32| -> usize {
        let (st, d) = &info[i];
        let local = match o.play {
            PlayMode::Hold => t.min(d - 1),
            PlayMode::Repeat => t % d,
            PlayMode::Sync => (t as u64 * *d as u64 / longest as u64) as u32,
            PlayMode::Sequence => t.saturating_sub(offsets[i]).min(d - 1),
        };
        index_at(st, local)
    };

    let out: Vec<Frame> = merged
        .par_iter()
        .enumerate()
        .map(|(k, &t)| {
            let end = merged.get(k + 1).copied().unwrap_or(total);
            let mut px = base.clone();
            for i in 0..used {
                let (x, y, cw, ch) = cells[i];
                paste(&mut px, o.width, &fitted[i][frame_at(i, t)], x, y, cw, ch);
            }
            Frame::from_rgba(o.width, o.height, &px, (end - t).max(10))
        })
        .collect();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(n: usize, delay: u32, v: u8) -> Vec<Frame> {
        (0..n).map(|i| Frame::from_rgba(2, 2, &[v, i as u8, 0, 255].repeat(4), delay)).collect()
    }

    fn opts(play: PlayMode) -> CollageOptions {
        CollageOptions {
            cols: 2,
            rows: 1,
            width: 10,
            height: 4,
            margin: 0,
            gap: 2,
            roundness: 0,
            background: [255, 255, 255],
            background_alpha: 255,
            draw_border: false,
            border_color: [0, 0, 0],
            play,
        }
    }

    #[test]
    fn rounded_corners_on_visible_picture_and_border() {
        // 넓은 그림(4x2)을 정사각형 칸(4x4)에 넣으면 위아래가 빈다. 둥글기는 그림의 모서리(1번째 줄)에 들어가야 한다.
        let px = fit_rounded(4, 2, &[200u8, 0, 0, 255].repeat(8), 4, 4, 1);
        assert_eq!(px[3], 0); // 빈 위쪽 줄은 투명
        assert!(px[(4 * 4) + 3] < 255); // 그림 첫 줄 왼쪽 끝(모서리)은 둥글게 깎임
        assert_eq!(px[(4 * 4) + 4 + 3], 255); // 그 옆은 그대로
        // 테두리를 그리면 칸 사이 선이 선 색
        let mut o = opts(PlayMode::Hold);
        o.draw_border = true;
        o.border_color = [0, 0, 255];
        let out = make(&[clip(1, 100, 9), clip(1, 100, 9)], &o, None).unwrap();
        assert_eq!(&out[0].rgba()[4 * 4..4 * 4 + 3], &[0, 0, 255]);
    }

    #[test]
    fn margin_or_gap_larger_than_canvas_is_an_error() {
        // 예전에는 칸이 캔버스 밖으로 나가 그리다가 프로그램이 꺼졌다
        let mut o = opts(PlayMode::Hold);
        o.margin = 6; // 가로 10에 여백 6×2
        assert!(make(&[clip(1, 100, 9)], &o, None).is_err());
        let mut o = opts(PlayMode::Hold);
        o.gap = 9; // 칸 2개 + 선 9 = 11 > 10
        assert!(make(&[clip(1, 100, 9), clip(1, 100, 9)], &o, None).is_err());
        // 딱 맞으면 된다: 칸 1 + 선 8 + 칸 1 = 10
        o.gap = 8;
        let out = make(&[clip(1, 100, 9), clip(1, 100, 9)], &o, None).unwrap();
        assert_eq!((out[0].width, out[0].height), (10, 4));
    }

    #[test]
    fn blank_frames() {
        let f = blank(4, 3, 10.0, 1.0, [1, 2, 3, 255]).unwrap();
        assert_eq!(f.len(), 10);
        assert_eq!(f.iter().map(|f| f.delay_ms).sum::<u32>(), 1000);
        assert_eq!(&f[5].rgba()[..4], &[1, 2, 3, 255]);
    }

    #[test]
    fn cell_layout_with_gap() {
        // 가로 10 = 칸 4 + 선 2 + 칸 4
        assert_eq!(cells(&opts(PlayMode::Hold)), vec![(0, 0, 4, 4), (6, 0, 4, 4)]);
    }

    #[test]
    fn play_modes_total_time() {
        let a = clip(4, 100, 10); // 400ms
        let b = clip(2, 100, 200); // 200ms
        let total = |m| make(&[a.clone(), b.clone()], &opts(m), None).unwrap().iter().map(|f| f.delay_ms).sum::<u32>();
        assert_eq!(total(PlayMode::Hold), 400);
        assert_eq!(total(PlayMode::Repeat), 400);
        assert_eq!(total(PlayMode::Sync), 400);
        assert_eq!(total(PlayMode::Sequence), 600);
        // 반복: 300ms 지점에서 짧은 영상은 두 번째 바퀴의 두 번째 장면(초록 1)
        let out = make(&[a.clone(), b.clone()], &opts(PlayMode::Repeat), None).unwrap();
        let px = out[3].rgba();
        let i = (6 * 4) as usize; // 오른쪽 칸 첫 픽셀 (y=0, 2x2를 4x4로 늘린 것)
        assert_eq!(&px[i..i + 2], &[200, 1]);
        // 칸 사이(선)는 배경색
        assert_eq!(&px[4 * 4..4 * 4 + 3], &[255, 255, 255]);
    }
}

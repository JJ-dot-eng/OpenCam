//! 저장: GIF(libimagequant) / WebP / APNG / MP4·WebM(ffmpeg) / PNG 연속 이미지.
//! 저장 전 "파일 크기 줄이기"(FPS 제한, 비슷한 이미지 건너뛰기, 크기 %)와 진행 막대 넣기도 여기서 한다.

use crate::frame::Frame;
use crate::project::{find_similar, resize_rgba};
use crate::settings::Settings;
use anyhow::{Context, Result, anyhow, bail};
use rayon::prelude::*;
use serde::Deserialize;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Gif,
    Webp,
    Apng,
    Mp4,
    Webm,
    Png,
    Avif,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Gif => "gif",
            Format::Webp => "webp",
            Format::Apng => "png",
            Format::Mp4 => "mp4",
            Format::Webm => "webm",
            Format::Png => "png",
            Format::Avif => "avif",
        }
    }
}

/// 저장할 때 바꿀 수 있는 옵션 (저장 창에서 넘어온다)
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub format: Format,
    pub quality: u8,
    pub limit_fps: Option<u32>,
    pub skip_similar: Option<u32>,
    pub skip_similar_time_limit_ms: Option<u32>,
    pub resize_percent: u32,
    pub embed_progress: bool,
    /// GIF에서 꼭 지킬 색들 (팔레트에 고정).
    #[serde(default)]
    pub protect_colors: Vec<[u8; 3]>,
    /// GIF를 20ms 간격(50 FPS)으로 다시 뽑는다. 20ms보다 짧은 프레임은 GIF에 쓸 수 없어서.
    #[serde(default)]
    pub gif_fit_50fps: bool,
}

pub type Progress<'a> = &'a (dyn Fn(f32, &str) + Sync);

/// 저장 전 가공. 원본 프로젝트는 건드리지 않는다.
pub fn prepare(frames: &[Frame], opt: &ExportOptions, s: &Settings) -> Vec<Frame> {
    let mut out: Vec<Frame> = frames.to_vec();

    if opt.format == Format::Gif && opt.gif_fit_50fps {
        out = resample_on_grid(&out, 20);
    }

    // FPS 제한: 프레임 시간이 1000/fps보다 짧으면 다음 프레임과 합친다
    if let Some(fps) = opt.limit_fps.filter(|&f| f > 0) {
        let min = 1000 / fps.max(1);
        let mut merged: Vec<Frame> = vec![];
        let mut acc = 0u32;
        for f in out {
            match merged.last_mut() {
                Some(last) if acc < min => {
                    acc += f.delay_ms;
                    *last = last.with_delay(last.delay_ms + f.delay_ms);
                }
                _ => {
                    acc = f.delay_ms;
                    merged.push(f);
                }
            }
        }
        out = merged;
    }

    // 비슷한 이미지 건너뛰기 (속도는 유지)
    if let Some(pct) = opt.skip_similar {
        let all: Vec<usize> = (0..out.len()).collect();
        let drop = find_similar(&out, &all, pct, s.reduce_tolerance, opt.skip_similar_time_limit_ms);
        let mut gone = vec![false; out.len()];
        for i in drop {
            gone[i] = true;
        }
        let mut kept: Vec<Frame> = vec![];
        for (i, f) in out.into_iter().enumerate() {
            match kept.last_mut() {
                Some(last) if gone[i] => *last = last.with_delay(last.delay_ms + f.delay_ms),
                _ => kept.push(f),
            }
        }
        out = kept;
    }

    let scale = opt.resize_percent.clamp(1, 400);
    let total: u32 = out.iter().map(|f| f.delay_ms).sum();
    let progress = opt.embed_progress && total >= s.embed_progress_min_sec * 1000;
    if scale != 100 || progress {
        let starts: Vec<u32> = out
            .iter()
            .scan(0u32, |t, f| {
                let st = *t;
                *t += f.delay_ms;
                Some(st)
            })
            .collect();
        out = out
            .par_iter()
            .zip(starts.par_iter())
            .map(|(f, &start)| {
                f.map_pixels(|w, h, px| {
                    let (mut w, mut h, mut px) = (w, h, px);
                    if scale != 100 {
                        let nw = (w * scale / 100).max(1);
                        let nh = (h * scale / 100).max(1);
                        px = resize_rgba(w, h, &px, nw, nh, true);
                        (w, h) = (nw, nh);
                    }
                    if progress {
                        let pos = (start + f.delay_ms) as f32 / total.max(1) as f32;
                        draw_progress(&mut px, w, h, pos, s);
                    }
                    (w, h, px)
                })
            })
            .collect();
    }
    out
}

fn bgr(c: u32) -> [u8; 3] {
    [(c & 0xff) as u8, ((c >> 8) & 0xff) as u8, ((c >> 16) & 0xff) as u8]
}

/// 아래쪽에 재생 진행 막대를 그린다
fn draw_progress(px: &mut [u8], w: u32, h: u32, pos: f32, s: &Settings) {
    let lw = s.embed_progress_line_width.clamp(1, h);
    let fg = bgr(s.embed_progress_color);
    let bg = bgr(s.embed_progress_bg_color);
    let filled = (w as f32 * pos.clamp(0.0, 1.0)).round() as u32;
    for y in h - lw..h {
        for x in 0..w {
            let c = if x < filled { fg } else { bg };
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 3].copy_from_slice(&c);
            px[i + 3] = 255;
        }
    }
}

pub fn export(frames: &[Frame], path: &Path, opt: &ExportOptions, s: &Settings, progress: Progress) -> Result<()> {
    if frames.is_empty() {
        bail!("저장할 프레임이 없습니다");
    }
    progress(0.0, "준비 중");
    let frames = prepare(frames, opt, s);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    match opt.format {
        Format::Gif => {
            gif_direct(&frames, path, opt.quality, &opt.protect_colors, gif_keep_tolerance(opt.quality), s, progress)
        }
        Format::Webp => webp(&frames, path, opt.quality, s, progress),
        Format::Apng => apng(&frames, path, s, progress),
        Format::Mp4 | Format::Webm | Format::Avif => video(&frames, path, opt.format, opt.quality, s, progress),
        Format::Png => png_sequence(&frames, path, progress),
    }
}

/// 저장하지 않고 예상 크기만 계산 (임시 폴더에 써 보고 지운다).
/// 계산마다 따로 폴더를 써서 겹쳐 계산해도 섞이지 않고, PNG 연속 이미지는 모든 장을 더한다.
pub fn estimate_size(frames: &[Frame], opt: &ExportOptions, s: &Settings) -> Result<u64> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("opencam_est_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let r = export(frames, &dir.join(format!("estimate.{}", opt.format.ext())), opt, s, &|_, _| {});
    let size = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
        .unwrap_or(0);
    let _ = std::fs::remove_dir_all(&dir);
    r.map(|_| size)
}

/// 반복 횟수 설정을 "몇 번 재생"으로. None이면 무한 반복.
fn plays(finite: bool, count: u16) -> Option<u16> {
    finite.then_some(count.max(1))
}

/// grid ms 간격의 시각마다 그때 보이는 프레임을 고른다. 짧은 프레임끼리 합치면 60 FPS가 30 FPS가 되지만,
/// 이렇게 하면 50 FPS(6장 중 1장만 빠짐)가 된다. 같은 프레임이 이어지면 하나로 합친다.
/// 마지막 프레임은 꼭 넣고 최소 grid만큼 보이게 해서 끝 화면을 잃지 않는다 (전체 시간이 grid 2칸 미만 늘 수 있음).
fn resample_on_grid(frames: &[Frame], grid: u32) -> Vec<Frame> {
    let total: u64 = frames.iter().map(|f| u64::from(f.delay_ms)).sum();
    if frames.is_empty() || total == 0 || grid == 0 {
        return frames.to_vec();
    }
    // (원본 번호, 시작 시각)
    let mut picks: Vec<(usize, u64)> = vec![];
    let (mut j, mut start_j) = (0usize, 0u64);
    let mut t = 0u64;
    while t < total {
        while j + 1 < frames.len() && start_j + u64::from(frames[j].delay_ms) <= t {
            start_j += u64::from(frames[j].delay_ms);
            j += 1;
        }
        if picks.last().map(|&(k, _)| k) != Some(j) {
            picks.push((j, t));
        }
        t += u64::from(grid);
    }
    // 마지막 프레임이 격자 사이에 끼어 빠졌으면: 마지막 칸이 한 칸짜리면 그 칸을 마지막 프레임으로 바꾸고,
    // 아니면(긴 프레임을 잃지 않게) 뒤에 한 칸 붙인다.
    let last = frames.len() - 1;
    let n = picks.len();
    let (k, st) = picks[n - 1];
    if k != last {
        if n > 1 && st + u64::from(grid) == t {
            picks[n - 1].0 = last;
        } else {
            picks.push((last, t));
        }
    }
    picks
        .iter()
        .enumerate()
        .map(|(n, &(k, st))| {
            let end = picks.get(n + 1).map_or(total.max(st + u64::from(grid)), |&(_, s)| s);
            frames[k].with_delay((end - st) as u32)
        })
        .collect()
}

/// GIF 프레임 시간(1/100초 단위). 프레임마다 버리면 오차가 쌓여(33ms → 30ms) 영상이 빨라지므로,
/// 누적 시간을 반올림한 끝 시각에서 앞까지 쓴 시간을 뺀다. 최소 2(20ms):
/// 그보다 짧으면 브라우저가 오히려 느리게(100ms) 재생한다.
fn gif_delays_cs(frames: &[Frame]) -> Vec<u32> {
    let (mut end_ms, mut written) = (0u64, 0u64);
    frames
        .iter()
        .map(|f| {
            end_ms += u64::from(f.delay_ms);
            let d = ((end_ms + 5) / 10).saturating_sub(written).clamp(2, u64::from(u16::MAX));
            written += d;
            d as u32
        })
        .collect()
}

/// 앞 프레임과 이만큼(채널별 차이) 비슷하면 바뀌지 않은 픽셀로 본다. 화질 100이면 똑같을 때만.
/// 크게 잡으면 용량이 줄지만 글자 가장자리나 천천히 변하는 면에 지난 색이 남는다.
/// 화질 80에서 기준은 4다.
fn gif_keep_tolerance(quality: u8) -> u8 {
    ((100 - u32::from(quality.clamp(1, 100))) / 5) as u8
}

/// libimagequant로 직접 GIF를 만든다 (프레임마다 팔레트를 따로 만든다).
/// 마지막으로 쓸 때의 원본 색과 거의 같은 픽셀은 투명으로 비우고(앞 프레임이 그대로 보인다),
/// 바뀐 부분을 감싸는 사각형만 새로 쓴다. 화면에 보이는 색(색을 줄인 결과)과 비교하면
/// 줄일 때 생긴 오차 때문에 안 바뀐 픽셀까지 매번 다시 쓰게 되므로 원본끼리 비교한다.
/// 색 줄이기도 바뀐 픽셀만 보고 하므로 256색을 바뀐 곳에 몰아 쓴다.
/// 팔레트는 libimagequant로 만든 뒤 가장 가까운 색 고르기로 다듬는다(`refine_nearest`). 디더링을 켜면 다듬은 팔레트로 디더링한다.
/// colors는 팔레트에 그대로 박아 둘 색(특정 색 보호).
fn gif_direct(
    frames: &[Frame],
    path: &Path,
    quality: u8,
    colors: &[[u8; 3]],
    tol: u8,
    s: &Settings,
    progress: Progress,
) -> Result<()> {
    let (w, h) = (frames[0].width as usize, frames[0].height as usize);
    let file = BufWriter::new(std::fs::File::create(path)?);
    let mut enc = gif::Encoder::new(file, w as u16, h as u16, &[])?;
    match plays(s.gif_finite_loop, s.gif_loop_count) {
        Some(n) => enc.set_repeat(gif::Repeat::Finite(n - 1))?,
        None => enc.set_repeat(gif::Repeat::Infinite)?,
    }
    // 픽셀마다 마지막으로 쓸 때의 원본 색 (RGB). 처음에는 없음.
    let mut sent: Option<Vec<[u8; 3]>> = None;
    let delays = gif_delays_cs(frames);
    // 어느 픽셀을 새로 쓸지는 원본만 보고 정해지므로 순서대로 빠르게 정하고,
    // 무거운 색 줄이기·압축은 여러 프레임을 한꺼번에 나눠 한다 (메모리를 위해 몇 장씩 끊어서).
    let batch = rayon::current_num_threads().max(1) * 2;
    let mut done = 0;
    for chunk in (0..frames.len()).collect::<Vec<_>>().chunks(batch) {
        let jobs: Vec<(usize, usize, usize, usize, usize, Vec<imagequant::RGBA>)> = chunk
            .iter()
            .map(|&i| {
                let px = frames[i].rgba();
                // 새로 써야 하는 픽셀
                let changed: Vec<bool> = match &sent {
                    None => vec![true; w * h],
                    Some(sh) => (0..w * h)
                        .map(|k| {
                            let p = &px[k * 4..k * 4 + 4];
                            // 투명해지는 픽셀은 GIF에서 지울 수 없으니(앞 프레임 위에 덧그림) 그대로 둔다
                            p[3] >= 128
                                && if s.gif_remove_similar_pixels {
                                    (0..3).any(|c| p[c].abs_diff(sh[k][c]) > tol)
                                } else {
                                    p[..3] != sh[k]
                                }
                        })
                        .collect(),
                };
                let sh = sent.get_or_insert_with(|| vec![[0, 0, 0]; w * h]);
                let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
                for y in 0..h {
                    for x in 0..w {
                        let k = y * w + x;
                        if changed[k] {
                            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                            sh[k] = [px[k * 4], px[k * 4 + 1], px[k * 4 + 2]];
                        }
                    }
                }
                if x0 >= x1 {
                    // 바뀐 곳이 없으면 1×1 투명 프레임으로 시간만 잇는다
                    (x0, y0, x1, y1) = (0, 0, 1, 1);
                }
                // 바뀌지 않은 픽셀은 알파 0으로 넣어 투명 색 하나로 모이게 한다
                let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0));
                for y in y0..y1 {
                    for x in x0..x1 {
                        let k = y * w + x;
                        let p = &px[k * 4..k * 4 + 4];
                        let a = if changed[k] && p[3] >= 128 { 255 } else { 0 };
                        rgba.push(imagequant::RGBA::new(p[0], p[1], p[2], a));
                    }
                }
                (i, x0, y0, x1 - x0, y1 - y0, rgba)
            })
            .collect();
        let encoded: Vec<Result<gif::Frame<'static>>> = jobs
            .into_par_iter()
            .map(|(i, x0, y0, bw, bh, rgba)| -> Result<gif::Frame<'static>> {
                let mut attr = imagequant::new();
                attr.set_quality(0, quality.clamp(1, 100))?;
                let px = rgba.clone();
                let mut img = attr.new_image(rgba, bw, bh, 0.0)?;
                for &[r, g, b] in colors.iter().take(255) {
                    img.add_fixed_color(imagequant::RGBA::new(r, g, b, 255))?;
                }
                let mut res = attr.quantize(&mut img)?;
                res.set_dithering_level(0.0)?;
                let (mut palette, mut indices) = res.remapped(&mut img)?;
                refine_nearest(&px, &mut palette, &mut indices, colors, 3);
                if s.gif_dither {
                    // 다듬은 팔레트로 디더링만 다시 한다
                    let mut res = imagequant::QuantizationResult::from_palette(&attr, &palette, 0.0)?;
                    res.set_dithering_level(1.0)?;
                    (palette, indices) = res.remapped(&mut attr.new_image(px, bw, bh, 0.0)?)?;
                }
                let mut frame = gif::Frame {
                    left: x0 as u16,
                    top: y0 as u16,
                    width: bw as u16,
                    height: bh as u16,
                    buffer: std::borrow::Cow::Owned(indices),
                    palette: Some(palette.iter().flat_map(|c| [c.r, c.g, c.b]).collect()),
                    transparent: palette.iter().position(|c| c.a == 0).map(|t| t as u8),
                    delay: delays[i] as u16,
                    dispose: gif::DisposalMethod::Keep,
                    ..Default::default()
                };
                frame.make_lzw_pre_encoded();
                Ok(frame)
            })
            .collect();
        for frame in encoded {
            enc.write_lzw_pre_encoded_frame(&frame?)?;
            done += 1;
            progress(done as f32 / frames.len() as f32, "GIF 인코딩 중");
        }
    }
    Ok(())
}

/// 픽셀마다 팔레트에서 화면 색(RGB)이 가장 가까운 색을 다시 고르고, 팔레트 색을 그 색을 쓰는 픽셀들의 평균으로
/// 옮기기를 rounds번 되풀이한다. libimagequant는 사람 눈 기준의 다른 색 공간에서 고르기 때문에, 고른 번호가
/// 실제로 가장 가까운 색이 아닌 경우가 많다(게임 화면에서 13%). 가장 가까운 색을 쓰면 같은 크기에서
/// 화질이 높고 압축도 잘 된다(TODO.md). 알파 0 픽셀은 투명 색으로, 보호할 색(fixed)은 옮기지 않는다.
fn refine_nearest(px: &[imagequant::RGBA], palette: &mut [imagequant::RGBA], indices: &mut [u8], fixed: &[[u8; 3]], rounds: usize) {
    let transparent = palette.iter().position(|c| c.a == 0).map(|t| t as u8);
    for round in 0..=rounds {
        if round > 0 {
            let mut sum = vec![[0u64; 4]; palette.len()];
            for (p, &i) in px.iter().zip(indices.iter()) {
                if p.a != 0 {
                    let s = &mut sum[i as usize];
                    s[0] += u64::from(p.r);
                    s[1] += u64::from(p.g);
                    s[2] += u64::from(p.b);
                    s[3] += 1;
                }
            }
            for (c, s) in palette.iter_mut().zip(&sum) {
                if s[3] > 0 && c.a != 0 && !fixed.contains(&[c.r, c.g, c.b]) {
                    let avg = |v: u64| ((v + s[3] / 2) / s[3]) as u8;
                    *c = imagequant::RGBA::new(avg(s[0]), avg(s[1]), avg(s[2]), 255);
                }
            }
        }
        // 초록 값 순으로 늘어놓고, 픽셀의 초록 값 자리에서 양옆으로 넓혀 가다가 초록 차이만으로도
        // 지금까지 찾은 거리보다 멀어지면 멈춘다. 같은 색 픽셀이 많아서 색마다 한 번만 찾는다.
        let mut by_g: Vec<(i32, i32, i32, u8)> = palette
            .iter()
            .enumerate()
            .filter(|(_, c)| c.a != 0)
            .map(|(j, c)| (i32::from(c.g), i32::from(c.r), i32::from(c.b), j as u8))
            .collect();
        by_g.sort_unstable();
        let mut cache: std::collections::HashMap<u32, u8, std::hash::BuildHasherDefault<ColorHasher>> = Default::default();
        for (p, i) in px.iter().zip(indices.iter_mut()) {
            if p.a == 0 {
                if let Some(t) = transparent {
                    *i = t;
                }
                continue;
            }
            let key = u32::from(p.r) << 16 | u32::from(p.g) << 8 | u32::from(p.b);
            *i = *cache.entry(key).or_insert_with(|| nearest_by_g(&by_g, i32::from(p.r), i32::from(p.g), i32::from(p.b)));
        }
    }
}

/// by_g(초록, 빨강, 파랑, 번호, 초록 순 정렬)에서 (r, g, b)와 가장 가까운 색의 번호
fn nearest_by_g(by_g: &[(i32, i32, i32, u8)], r: i32, g: i32, b: i32) -> u8 {
    let start = by_g.partition_point(|e| e.0 < g);
    let (mut best_d, mut best) = (i32::MAX, by_g.first().map_or(0, |e| e.3));
    let (mut lo, mut hi) = (start, start);
    loop {
        let mut moved = false;
        if hi < by_g.len() {
            let (cg, cr, cb, j) = by_g[hi];
            let dg = (cg - g).pow(2);
            if dg < best_d {
                let d = dg + (cr - r).pow(2) + (cb - b).pow(2);
                if d < best_d {
                    (best_d, best) = (d, j);
                }
                hi += 1;
                moved = true;
            } else {
                hi = by_g.len();
            }
        }
        if lo > 0 {
            let (cg, cr, cb, j) = by_g[lo - 1];
            let dg = (cg - g).pow(2);
            if dg < best_d {
                let d = dg + (cr - r).pow(2) + (cb - b).pow(2);
                if d < best_d {
                    (best_d, best) = (d, j);
                }
                lo -= 1;
                moved = true;
            } else {
                lo = 0;
            }
        }
        if !moved {
            return best;
        }
    }
}

/// RGB 색 하나(u32)용 해시. 기본 해시(SipHash)는 이 용도에 느리다.
#[derive(Default)]
struct ColorHasher(u64);

impl std::hash::Hasher for ColorHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 << 8 | u64::from(b)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_u32(&mut self, v: u32) {
        // 곱한 값의 아래 비트는 입력의 아래 비트(파랑)에만 달려 있어서 위 비트를 섞어 내린다
        let h = u64::from(v).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = h ^ (h >> 29);
    }
}

fn webp(frames: &[Frame], path: &Path, quality: u8, s: &Settings, progress: Progress) -> Result<()> {
    use webp_animation::{AnimParams, Encoder, EncoderOptions, EncodingConfig, EncodingType};
    let (w, h) = (frames[0].width, frames[0].height);
    let config = EncodingConfig {
        encoding_type: if s.webp_lossless { EncodingType::Lossless } else { EncodingType::new_lossy() },
        quality: quality as f32,
        method: s.webp_method.min(6) as usize,
    };
    let mut opts = EncoderOptions {
        anim_params: AnimParams { loop_count: plays(s.webp_finite_loop, s.webp_loop_count).map_or(0, i32::from) },
        encoding_config: Some(config),
        ..Default::default()
    };
    if s.webp_iphone_compat {
        // 아이폰(사파리)은 앞 프레임에 덧그리는 부분 프레임을 잘못 보여줄 때가 있어 모든 프레임을 통째로 저장한다
        opts.kmin = 0;
        opts.kmax = 1;
        opts.allow_mixed = false;
    }
    let mut enc = Encoder::new_with_options((w, h), opts).map_err(|e| anyhow!("WebP 인코더 오류: {e:?}"))?;
    let mut t = 0i32;
    for (i, f) in frames.iter().enumerate() {
        enc.add_frame(&f.rgba(), t).map_err(|e| anyhow!("WebP 프레임 오류: {e:?}"))?;
        t += f.delay_ms as i32;
        progress((i + 1) as f32 / frames.len() as f32, "WebP 인코딩 중");
    }
    let data = enc.finalize(t).map_err(|e| anyhow!("WebP 저장 오류: {e:?}"))?;
    std::fs::write(path, &*data)?;
    Ok(())
}

/// 모든 프레임이 함께 쓸 256색 팔레트로 줄인다 (APNG는 팔레트가 하나뿐이라서).
/// 돌려주는 값: (RGB 팔레트, 투명도 표, 프레임마다 색 번호)
fn quantize_shared(frames: &[Frame], quality: u8) -> Result<(Vec<u8>, Vec<u8>, Vec<Vec<u8>>)> {
    let mut attr = imagequant::new();
    attr.set_quality(0, quality.clamp(1, 100))?;
    let mut hist = imagequant::Histogram::new(&attr);
    let to_img = |f: &Frame| -> Result<imagequant::Image<'static>> {
        let px: Vec<imagequant::RGBA> =
            f.rgba().chunks_exact(4).map(|p| imagequant::RGBA::new(p[0], p[1], p[2], p[3])).collect();
        Ok(attr.new_image(px, f.width as usize, f.height as usize, 0.0)?)
    };
    for f in frames {
        hist.add_image(&attr, &mut to_img(f)?)?;
    }
    let mut res = hist.quantize(&attr)?;
    res.set_dithering_level(1.0)?;
    let mut indices = Vec::with_capacity(frames.len());
    let mut palette = vec![];
    for f in frames {
        let (pal, idx) = res.remapped(&mut to_img(f)?)?;
        palette = pal;
        indices.push(idx);
    }
    let rgb = palette.iter().flat_map(|c| [c.r, c.g, c.b]).collect();
    let trns = palette.iter().map(|c| c.a).collect();
    Ok((rgb, trns, indices))
}

fn apng(frames: &[Frame], path: &Path, s: &Settings, progress: Progress) -> Result<()> {
    let (w, h) = (frames[0].width, frames[0].height);
    let file = BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_depth(png::BitDepth::Eight);
    // 256색: 모든 프레임에 같은 팔레트
    let indexed = if s.apng_256_colors {
        progress(0.0, "APNG 색 줄이는 중");
        let (rgb, trns, idx) = quantize_shared(frames, 90)?;
        enc.set_color(png::ColorType::Indexed);
        enc.set_palette(rgb);
        if trns.iter().any(|&a| a < 255) {
            enc.set_trns(trns);
        }
        Some(idx)
    } else {
        enc.set_color(png::ColorType::Rgba);
        None
    };
    enc.set_compression(png::Compression::Fast);
    // APNG의 num_plays는 0이면 무한 반복
    enc.set_animated(frames.len() as u32, plays(s.apng_finite_loop, s.apng_loop_count).map_or(0, u32::from))?;
    let mut writer = enc.write_header()?;
    for (i, f) in frames.iter().enumerate() {
        writer.set_frame_delay(f.delay_ms.min(u16::MAX as u32) as u16, 1000)?;
        match &indexed {
            Some(idx) => writer.write_image_data(&idx[i])?,
            None => writer.write_image_data(&f.rgba())?,
        }
        progress((i + 1) as f32 / frames.len() as f32, "APNG 저장 중");
    }
    writer.finish()?;
    Ok(())
}

/// ffmpeg.exe 위치: 프로그램 옆 → PATH
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(beside) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("ffmpeg.exe")))
        && beside.exists()
    {
        return Some(beside);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|p| p.join("ffmpeg.exe")).find(|p| p.exists())
}

/// ffmpeg 오류 메시지는 끝부분 이만큼만 남긴다
pub(crate) const STDERR_TAIL: usize = 8 * 1024;

/// stderr를 다른 스레드에서 끝까지 읽어 마지막 keep바이트만 남긴다.
/// 읽어 주지 않으면 ffmpeg가 오류를 많이 쓸 때(깨진 동영상 등) 파이프가 가득 차 ffmpeg가 멈추고,
/// stdin에 쓰거나 stdout을 기다리는 쪽도 같이 멈춘다.
pub(crate) fn spawn_stderr_tail(
    mut err: impl Read + Send + 'static,
    keep: usize,
) -> std::io::Result<std::thread::JoinHandle<String>> {
    std::thread::Builder::new().name("ffmpeg-stderr".into()).spawn(move || {
        let mut tail: Vec<u8> = Vec::new();
        let mut cut = false;
        let mut chunk = [0u8; 4096];
        loop {
            match err.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    tail.extend_from_slice(&chunk[..n]);
                    // 매번 앞을 지우면 느리므로 두 배가 넘으면 한 번에 자른다
                    if tail.len() > keep * 2 {
                        let extra = tail.len() - keep;
                        tail.drain(..extra);
                        cut = true;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        if tail.len() > keep {
            let extra = tail.len() - keep;
            tail.drain(..extra);
            cut = true;
        }
        let text = String::from_utf8_lossy(&tail);
        // 앞이 잘렸으면 첫 줄은 중간부터라 버린다
        let text = match (cut, text.find('\n')) {
            (true, Some(i)) => &text[i + 1..],
            _ => &text[..],
        };
        text.trim().to_string()
    })
}

fn video(frames: &[Frame], path: &Path, format: Format, quality: u8, s: &Settings, progress: Progress) -> Result<()> {
    let ffmpeg = find_ffmpeg()
        .ok_or_else(|| anyhow!("ffmpeg.exe를 찾을 수 없습니다. ffmpeg를 설치하거나 프로그램 폴더에 넣어 주세요."))?;
    let (w, h) = (frames[0].width, frames[0].height);
    // 프레임마다 시간이 달라서, 가장 짧은 프레임 기준의 고정 FPS로 맞춰 같은 화면을 반복해서 넣는다
    let min_delay = frames.iter().map(|f| f.delay_ms.max(10)).min().unwrap_or(100);
    let fps = (1000 / min_delay).clamp(1, 60);
    let q = quality.min(100) as f32;
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba"])
        .args(["-s", &format!("{w}x{h}"), "-r", &fps.to_string(), "-i", "-"])
        // yuv420p는 가로·세로가 짝수여야 한다
        .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"]);
    match format {
        Format::Mp4 => {
            let crf = (51.0 - q * 0.4).round() as u32; // 화질 80 → crf 19
            cmd.args(["-c:v", "libx264", "-preset", "medium", "-crf", &crf.to_string()]).args([
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
            ]);
        }
        Format::Avif => {
            // AV1 정지/움직이는 그림. 화질 80 → crf 19 (0이 가장 좋음, 63이 가장 나쁨)
            let crf = (63.0 - q * 0.55).round() as u32;
            cmd.args(["-c:v", "libaom-av1", "-crf", &crf.to_string(), "-b:v", "0", "-cpu-used", "6", "-row-mt", "1"])
                .args(["-pix_fmt", "yuv420p", "-still-picture", "0", "-f", "avif"]);
        }
        _ => {
            let crf = (63.0 - q * 0.55).round() as u32; // 화질 80 → crf 19
            cmd.args(["-c:v", "libvpx-vp9", "-b:v", "0", "-crf", &crf.to_string(), "-pix_fmt", "yuva420p"]);
        }
    }
    cmd.arg(path).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: 검은 콘솔 창을 띄우지 않는다
    }
    let mut child = cmd.spawn().context("ffmpeg 실행 실패")?;
    // 여기부터는 어느 길로 나가든 ffmpeg를 기다리고 stderr 스레드를 join한다.
    // stderr를 따로 읽지 않으면 ffmpeg가 오류를 많이 쓸 때 서로 기다리며 멈춘다
    let err_tail = match spawn_stderr_tail(child.stderr.take().unwrap(), STDERR_TAIL) {
        Ok(t) => t,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::Error::new(e).context("ffmpeg 실행 실패"));
        }
    };
    // MP4는 투명을 못 쓴다: "다른 색으로 바꾸기"가 켜져 있으면 그 색, 아니면 검정 위에 합성
    let matte = matches!(format, Format::Mp4 | Format::Avif)
        .then(|| if s.mp4_replace_transparent { bgr(s.transparent_replace_color) } else { [0, 0, 0] });
    {
        let mut stdin = BufWriter::new(child.stdin.take().unwrap());
        let mut written = 0u64; // 지금까지 넣은 출력 프레임 수
        let mut t = 0u64;
        'outer: for (i, f) in frames.iter().enumerate() {
            t += f.delay_ms as u64;
            let until = (t * fps as u64 / 1000).max(written + 1);
            let mut px = f.rgba();
            if let Some(c) = matte {
                composite_over(&mut px, c);
            }
            for _ in written..until {
                if stdin.write_all(&px).is_err() {
                    break 'outer; // ffmpeg가 먼저 끝났다 → 아래에서 오류 메시지를 읽는다
                }
            }
            written = until;
            progress((i + 1) as f32 / frames.len() as f32 * 0.95, "동영상 인코딩 중");
        }
        let _ = stdin.flush();
        // 여기서 stdin이 닫혀야 ffmpeg가 끝난다
    }
    let status = child.wait();
    if status.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // ffmpeg가 끝나 stderr가 닫히면 스레드도 끝난다
    let err = err_tail.join().unwrap_or_default();
    if !status?.success() {
        bail!("ffmpeg 오류: {err}");
    }
    Ok(())
}

/// 녹화하면서 MP4로 바로 저장. 프레임은 다른 스레드에서 ffmpeg로 보낸다.
pub struct Mp4Stream {
    pub path: PathBuf,
    child: std::process::Child,
    tx: Option<std::sync::mpsc::SyncSender<(std::sync::Arc<Vec<u8>>, u32)>>,
    writer: Option<std::thread::JoinHandle<()>>,
}

impl Mp4Stream {
    pub fn start(path: &Path, w: u32, h: u32, fps: u32, quality: u8) -> Result<Self> {
        let ffmpeg = find_ffmpeg().ok_or_else(|| anyhow!("MP4로 바로 저장하려면 ffmpeg가 필요합니다"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let crf = (51.0 - quality.min(100) as f32 * 0.4).round() as u32; // 화질 90 → crf 15
        let mut cmd = Command::new(ffmpeg);
        cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba"])
            .args(["-s", &format!("{w}x{h}"), "-r", &fps.clamp(1, 100).to_string(), "-i", "-"])
            .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"])
            // 녹화와 동시에 해야 하므로 빠른 설정
            .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", &crf.to_string()])
            .args(["-pix_fmt", "yuv420p", "-movflags", "+faststart"])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().context("ffmpeg 실행 실패")?;
        let mut stdin = BufWriter::new(child.stdin.take().unwrap());
        // 녹화가 잠깐 밀려도 버틸 만큼만 쌓아 둔다 (너무 밀리면 녹화가 기다린다)
        let (tx, rx) = std::sync::mpsc::sync_channel::<(std::sync::Arc<Vec<u8>>, u32)>(8);
        let writer = std::thread::spawn(move || {
            for (px, copies) in rx {
                for _ in 0..copies {
                    if stdin.write_all(&px).is_err() {
                        return;
                    }
                }
            }
            let _ = stdin.flush();
        });
        Ok(Mp4Stream { path: path.to_path_buf(), child, tx: Some(tx), writer: Some(writer) })
    }

    /// 같은 화면을 copies장 넣는다 (화면이 안 바뀐 시간만큼)
    pub fn push(&self, px: std::sync::Arc<Vec<u8>>, copies: u32) {
        if copies > 0
            && let Some(tx) = &self.tx
        {
            let _ = tx.send((px, copies));
        }
    }

    /// 다 넣고 파일을 마무리한다
    pub fn finish(mut self) -> Result<PathBuf> {
        drop(self.tx.take());
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        let status = self.child.wait()?;
        if !status.success() {
            bail!("MP4 바로 저장 실패 (ffmpeg 종료 코드 {:?})", status.code());
        }
        Ok(self.path)
    }
}

/// 투명한 부분을 배경색(RGB, `bgr()`로 바꾼 값) 위에 합성한다 (MP4는 투명을 지원하지 않는다)
fn composite_over(px: &mut [u8], rgb: [u8; 3]) {
    for p in px.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a == 255 {
            continue;
        }
        for k in 0..3 {
            p[k] = ((p[k] as u32 * a + rgb[k] as u32 * (255 - a)) / 255) as u8;
        }
        p[3] = 255;
    }
}

/// name.png → name_0001.png, name_0002.png ...
fn png_sequence(frames: &[Frame], path: &Path, progress: Progress) -> Result<()> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "frame".into());
    let dir = path.parent().unwrap_or(Path::new("."));
    let done = std::sync::atomic::AtomicUsize::new(0);
    frames.par_iter().enumerate().try_for_each(|(i, f)| -> Result<()> {
        let p = dir.join(format!("{stem}_{:04}.png", i + 1));
        write_png(&p, f.width, f.height, &f.rgba())?;
        let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        progress(n as f32 / frames.len() as f32, "PNG 저장 중");
        Ok(())
    })
}

pub fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let file = BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    Ok(())
}

/// 파일 이름 규칙(%Y %m %d %H %M %S ...)을 현재 시각으로 채운다. 잘못된 규칙이면 기본 규칙을 쓴다.
pub fn file_name_from_rule(rule: &str) -> String {
    use std::fmt::Write as _;
    let now = chrono::Local::now();
    let mut s = String::new();
    if write!(s, "{}", now.format(rule)).is_err() || s.trim().is_empty() {
        s = now.format("opencam_%Y%m%d_%H%M%S").to_string();
    }
    // 파일 이름에 쓸 수 없는 글자는 바꾼다
    const BAD: &str = "\\/:*?\"<>|";
    s.chars().map(|c| if BAD.contains(c) { '_' } else { c }).collect()
}

/// 같은 이름 파일이 있으면 뒤에 (2), (3)...을 붙인다
pub fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{name}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{name} ({n}).{ext}"))).find(|p| !p.exists()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(n: usize, delay: u32) -> Vec<Frame> {
        (0..n).map(|i| Frame::from_rgba(4, 4, &vec![(i * 40) as u8; 64], delay)).collect()
    }

    fn opts(format: Format) -> ExportOptions {
        ExportOptions {
            format,
            quality: 80,
            limit_fps: None,
            skip_similar: None,
            skip_similar_time_limit_ms: None,
            resize_percent: 100,
            embed_progress: false,
            protect_colors: vec![],
            gif_fit_50fps: false,
        }
    }

    #[test]
    fn protected_colors_stay_exact() {
        // 비슷한 색이 많아도 보호한 색(정확히 250,10,10)은 그대로 남아야 한다
        let s = Settings::default();
        let mut px = vec![];
        for i in 0..64u32 {
            px.extend_from_slice(&[(i * 4) as u8, (255 - i * 4) as u8, (i * 2) as u8, 255]);
        }
        px[..4].copy_from_slice(&[250, 10, 10, 255]);
        let frames = vec![Frame::from_rgba(8, 8, &px, 100), Frame::from_rgba(8, 8, &px, 100)];
        let p = std::env::temp_dir().join(format!("opencam_protect_{}.gif", std::process::id()));
        let mut o = opts(Format::Gif);
        o.protect_colors = vec![[250, 10, 10]];
        export(&frames, &p, &o, &s, &|_, _| {}).unwrap();
        let mut dec = gif::DecodeOptions::new();
        dec.set_color_output(gif::ColorOutput::RGBA);
        let mut r = dec.read_info(std::fs::File::open(&p).unwrap()).unwrap();
        let f = r.read_next_frame().unwrap().unwrap();
        assert_eq!(&f.buffer[..4], &[250, 10, 10, 255]);
        // 두 번째 프레임은 앞과 같아서 전부 투명(비워 둠)
        let f2 = r.read_next_frame().unwrap().unwrap();
        assert!(f2.buffer.chunks_exact(4).all(|p| p[3] == 0));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn limit_fps_merges_short_frames() {
        let s = Settings::default();
        let mut o = opts(Format::Gif);
        o.limit_fps = Some(20); // 50ms 이상
        let out = prepare(&frames(6, 20), &o, &s);
        let total: u32 = out.iter().map(|f| f.delay_ms).sum();
        assert_eq!(total, 120);
        assert!(out.iter().all(|f| f.delay_ms >= 40));
    }

    #[test]
    fn resize_and_progress_bar() {
        let mut s = Settings::default();
        s.embed_progress_line_width = 1;
        let mut o = opts(Format::Gif);
        o.resize_percent = 200;
        o.embed_progress = true;
        let out = prepare(&frames(2, 100), &o, &s);
        assert_eq!((out[0].width, out[0].height), (8, 8));
        // 마지막 프레임은 막대가 끝까지 찬다(빨강 0x0000ff BGR → RGB 255,0,0)
        let px = out[1].rgba();
        let last_row = (7 * 8) * 4;
        assert_eq!(&px[last_row..last_row + 3], &[255, 0, 0]);
    }

    #[test]
    fn estimate_png_sequence_counts_all_files_and_cleans_up() {
        // 예전에는 크기 0으로 나오고 임시 폴더에 PNG가 프레임 수만큼 남았다
        let s = Settings::default();
        let input = frames(3, 100);
        let size = estimate_size(&input, &opts(Format::Png), &s).unwrap();
        let dir = std::env::temp_dir().join(format!("opencam_png_seq_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        export(&input, &dir.join("a.png"), &opts(Format::Png), &s, &|_, _| {}).unwrap();
        let real: u64 = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.metadata().unwrap().len()).sum();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(size, real);
        let prefix = format!("opencam_est_{}_", std::process::id());
        let left = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .count();
        assert_eq!(left, 0, "임시 파일이 남지 않는다");
    }

    #[test]
    fn transparent_matte_keeps_red_and_blue() {
        // 설정의 색은 BGR 정수: 0x0000ff = 빨강. 예전에는 파랑으로 합성됐다.
        let mut px = vec![0, 0, 0, 0, 10, 20, 30, 255];
        composite_over(&mut px, bgr(0x0000ff));
        assert_eq!(px, vec![255, 0, 0, 255, 10, 20, 30, 255]);
    }

    #[test]
    fn encode_gif_webp_apng() {
        let s = Settings::default();
        let dir = std::env::temp_dir().join(format!("opencam_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in [Format::Gif, Format::Webp, Format::Apng] {
            let p = dir.join(format!("t.{}", f.ext()));
            export(&frames(3, 100), &p, &opts(f), &s, &|_, _| {}).unwrap();
            assert!(std::fs::metadata(&p).unwrap().len() > 0, "{f:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gif_preserves_last_frame_duration() {
        let s = Settings::default();
        for (case, delays) in [("variable", vec![60, 6500, 70]), ("single", vec![230])] {
            let path = std::env::temp_dir().join(format!("opencam_gif_duration_{case}_{}.gif", std::process::id()));
            let input: Vec<Frame> = delays.iter().enumerate().map(|(i, &delay)| {
                let color = [(i * 100) as u8, 30, 80, 255];
                Frame::from_rgba(4, 4, &color.repeat(16), delay)
            }).collect();
            export(&input, &path, &opts(Format::Gif), &s, &|_, _| {}).unwrap();
            let mut decoder = gif::DecodeOptions::new().read_info(std::fs::File::open(&path).unwrap()).unwrap();
            let mut actual = Vec::new();
            while let Some(frame) = decoder.read_next_frame().unwrap() {
                actual.push(u32::from(frame.delay) * 10);
            }
            assert_eq!(actual, delays, "{case}");
            std::fs::remove_file(path).unwrap();
        }
    }

    fn decoded_gif_delays_ms(path: &Path) -> Vec<u32> {
        let mut decoder = gif::DecodeOptions::new().read_info(std::fs::File::open(path).unwrap()).unwrap();
        let mut actual = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            actual.push(u32::from(frame.delay) * 10);
        }
        actual
    }

    /// 저장한 GIF의 프레임별 (left, top, width, height)
    fn decoded_gif_rects(path: &Path) -> Vec<(u16, u16, u16, u16)> {
        let mut decoder = gif::DecodeOptions::new().read_info(std::fs::File::open(path).unwrap()).unwrap();
        let mut out = Vec::new();
        while let Some(f) = decoder.read_next_frame().unwrap() {
            out.push((f.left, f.top, f.width, f.height));
        }
        out
    }

    #[test]
    fn gif_direct_handles_many_colors_and_writes_only_changed_area() {
        let s = Settings::default();
        // 256색이 넘는 프레임 (예전에는 투명 색을 하나 더 붙여 257색이 되어 저장이 실패했다)
        let many: Vec<u8> = (0..32 * 32).flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i / 4) as u8, 255]).collect();
        let mut second = many.clone();
        for y in 10..14 {
            for x in 20..24 {
                second[(y * 32 + x) * 4] ^= 0xff;
            }
        }
        // 3번째는 모든 픽셀이 조금씩(3)만 다르다: 화질 80(기준 4)이면 바뀌지 않은 것으로 본다
        let third: Vec<u8> = second.chunks_exact(4).flat_map(|p| [p[0].saturating_add(3), p[1], p[2], 255]).collect();
        let input = vec![
            Frame::from_rgba(32, 32, &many, 100),
            Frame::from_rgba(32, 32, &second, 100),
            Frame::from_rgba(32, 32, &third, 100),
        ];
        let path = std::env::temp_dir().join(format!("opencam_gif_direct_{}.gif", std::process::id()));
        gif_direct(&input, &path, 80, &[], gif_keep_tolerance(80), &s, &|_, _| {}).unwrap();
        let rects = decoded_gif_rects(&path);
        assert_eq!(rects[0], (0, 0, 32, 32));
        assert_eq!(rects[1], (20, 10, 4, 4), "바뀐 4×4만 새로 쓴다");
        assert_eq!(rects[2], (0, 0, 1, 1), "거의 같으면 시간만 잇는 빈 프레임");
        assert_eq!(decoded_gif_delays_ms(&path), vec![100, 100, 100]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn refine_nearest_picks_closest_color_and_keeps_fixed_and_transparent() {
        let c = |r, g, b, a| imagequant::RGBA::new(r, g, b, a);
        // 투명 픽셀 하나, 검정 근처 둘, 흰색 근처 하나, 보호할 빨강에 가까운 하나
        let px = [c(0, 0, 0, 0), c(10, 10, 10, 255), c(20, 20, 20, 255), c(250, 250, 250, 255), c(240, 10, 10, 255)];
        let mut palette = vec![c(0, 0, 0, 0), c(100, 100, 100, 255), c(200, 200, 200, 255), c(255, 0, 0, 255)];
        // 일부러 틀린 번호에서 시작한다 (libimagequant가 다른 색 공간에서 고른 결과처럼)
        let mut indices = vec![1, 2, 1, 1, 2];
        refine_nearest(&px, &mut palette, &mut indices, &[[255, 0, 0]], 3);
        assert_eq!(indices, vec![0, 1, 1, 2, 3]);
        assert_eq!(palette[1], c(15, 15, 15, 255), "쓰는 픽셀들의 평균으로 옮겨진다");
        assert_eq!(palette[2], c(250, 250, 250, 255));
        assert_eq!(palette[3], c(255, 0, 0, 255), "보호할 색은 그대로");
        assert_eq!(palette[0].a, 0, "투명 색은 그대로");

        // 초록 순 탐색이 모든 색을 보는 방법과 같은 답을 내는지 무작위로 확인
        let pal: Vec<(i32, i32, i32)> = (0..64u32).map(|i| ((i * 37 % 256) as i32, (i * 91 % 256) as i32, (i * 53 % 256) as i32)).collect();
        let mut by_g: Vec<(i32, i32, i32, u8)> = pal.iter().enumerate().map(|(j, &(r, g, b))| (g, r, b, j as u8)).collect();
        by_g.sort_unstable();
        let dist = |j: u8, r: i32, g: i32, b: i32| {
            let (pr, pg, pb) = pal[j as usize];
            (pr - r).pow(2) + (pg - g).pow(2) + (pb - b).pow(2)
        };
        for k in 0..2000i32 {
            let (r, g, b) = (k * 17 % 256, k * 29 % 256, k * 43 % 256);
            let brute = (0..64u8).min_by_key(|&j| dist(j, r, g, b)).unwrap();
            assert_eq!(dist(nearest_by_g(&by_g, r, g, b), r, g, b), dist(brute, r, g, b));
        }
    }

    #[test]
    fn gif_delays_keep_total_time_and_minimum() {
        let ms = |delays: &[u32]| -> Vec<u32> {
            let input: Vec<Frame> = delays.iter().map(|&d| Frame::from_rgba(1, 1, &[0, 0, 0, 255], d)).collect();
            gif_delays_cs(&input).iter().map(|d| d * 10).collect()
        };
        // 30 FPS: 버리면 30ms씩이 되어 10% 빨라진다
        assert_eq!(ms(&[33, 34, 33, 33, 34, 33]), vec![30, 40, 30, 30, 40, 30]);
        // 60 FPS: GIF는 20ms보다 짧게 못 쓴다
        assert_eq!(ms(&[16, 17, 17]), vec![20, 20, 20]);
        // 짧은 마지막 프레임도 20ms
        assert_eq!(ms(&[60, 6500, 15]), vec![60, 6500, 20]);
    }

    #[test]
    fn gif_fit_50fps_resamples_60fps_to_20ms_steps() {
        let s = Settings::default();
        let mut o = opts(Format::Gif);
        o.gif_fit_50fps = true;
        // 60 FPS 1초 (16·17·17ms 반복 = 50ms마다 3장)
        let delays: Vec<u32> = (0..60).map(|i| [16, 17, 17][i % 3]).collect();
        let input: Vec<Frame> = delays.iter().map(|&d| Frame::from_rgba(1, 1, &[0, 0, 0, 255], d)).collect();
        let out = prepare(&input, &o, &s);
        assert_eq!(out.len(), 50);
        assert!(out.iter().all(|f| f.delay_ms == 20));
        assert_eq!(out.last().unwrap().id, input.last().unwrap().id, "끝 화면 유지");
        // GIF가 아니면 그대로
        let mut webp = opts(Format::Webp);
        webp.gif_fit_50fps = true;
        assert_eq!(prepare(&input, &webp, &s).len(), 60);
    }

    #[test]
    fn grid_resample_keeps_long_frames_and_last_frame() {
        let ms = |delays: &[u32]| -> Vec<u32> {
            let input: Vec<Frame> = delays.iter().map(|&d| Frame::from_rgba(1, 1, &[0, 0, 0, 255], d)).collect();
            resample_on_grid(&input, 20).iter().map(|f| f.delay_ms).collect()
        };
        assert_eq!(ms(&[100, 6500, 40]), vec![100, 6500, 40]);
        // 마지막 프레임(5ms)이 격자 사이에 끼어도 버리지 않는다
        assert_eq!(ms(&[30, 5]), vec![40, 20]);
    }

    #[test]
    fn gif_files_use_rounded_delays_with_and_without_protected_colors() {
        let s = Settings::default();
        let delays = [33, 34, 33, 16, 17, 250];
        let input: Vec<Frame> = delays.iter().enumerate().map(|(i, &delay)| {
            Frame::from_rgba(4, 4, &[(i * 40) as u8, 30, 80, 255].repeat(16), delay)
        }).collect();
        // 16·17ms를 20ms로 늘려 생긴 7ms를 마지막 프레임에서 되찾는다 (전체 383ms → 380ms)
        let expected = vec![30, 40, 30, 20, 20, 240];
        for (case, protect) in [("plain", vec![]), ("protected", vec![[255, 0, 0]])] {
            let path = std::env::temp_dir().join(format!("opencam_gif_round_{case}_{}.gif", std::process::id()));
            let mut o = opts(Format::Gif);
            o.protect_colors = protect;
            export(&input, &path, &o, &s, &|_, _| {}).unwrap();
            assert_eq!(decoded_gif_delays_ms(&path), expected, "{case}");
            std::fs::remove_file(path).unwrap();
        }
    }

    /// ffmpeg가 설치된 PC에서만 실제로 돌린다
    #[test]
    fn encode_video_if_ffmpeg() {
        if find_ffmpeg().is_none() {
            return;
        }
        let s = Settings::default();
        let dir = std::env::temp_dir().join(format!("opencam_vtest_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 홀수 크기(5x3)도 짝수로 맞춰 저장돼야 한다
        let frames: Vec<Frame> = (0..4).map(|i| Frame::from_rgba(5, 3, &vec![(i * 60) as u8; 60], 50)).collect();
        for f in [Format::Mp4, Format::Webm] {
            let p = dir.join(format!("t.{}", f.ext()));
            export(&frames, &p, &opts(f), &s, &|_, _| {}).unwrap();
            assert!(std::fs::metadata(&p).unwrap().len() > 0, "{f:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stderr_tail_keeps_last_lines() {
        let text: String = (0..1000).map(|i| format!("line {i}\n")).collect();
        let tail = spawn_stderr_tail(std::io::Cursor::new(text.into_bytes()), 100).unwrap().join().unwrap();
        assert!(tail.len() <= 100, "{tail}");
        assert!(tail.ends_with("line 999"), "{tail}");
        // 잘린 첫 줄은 버린다
        assert!(tail.starts_with("line "), "{tail}");
        // 짧으면 그대로
        let short = spawn_stderr_tail(std::io::Cursor::new(b" oops \n".to_vec()), 100).unwrap().join().unwrap();
        assert_eq!(short, "oops");
    }

    #[test]
    fn apng_256_colors_round_trip() {
        let mut s = Settings::default();
        s.apng_256_colors = true;
        let p = std::env::temp_dir().join(format!("opencam_apng256_{}.png", std::process::id()));
        export(&frames(3, 100), &p, &opts(Format::Apng), &s, &|_, _| {}).unwrap();
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&p).unwrap()));
        let r = dec.read_info().unwrap();
        assert_eq!(r.info().color_type, png::ColorType::Indexed);
        assert_eq!(r.info().animation_control.unwrap().num_frames, 3);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn avif_and_direct_mp4_if_ffmpeg() {
        if find_ffmpeg().is_none() {
            return;
        }
        let s = Settings::default();
        let dir = std::env::temp_dir().join(format!("opencam_avif_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fr: Vec<Frame> =
            (0..4).map(|i| Frame::from_rgba(16, 8, &[(i * 60) as u8, 9, 9, 255].repeat(128), 100)).collect();
        let p = dir.join("t.avif");
        export(&fr, &p, &opts(Format::Avif), &s, &|_, _| {}).unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() > 0);
        // 바로 저장: 3장을 넣으면(앞 장은 2번) 4프레임짜리 MP4
        let m = Mp4Stream::start(&dir.join("d.mp4"), 16, 8, 10, 90).unwrap();
        m.push(std::sync::Arc::new(fr[0].rgba()), 2);
        m.push(std::sync::Arc::new(fr[1].rgba()), 1);
        m.push(std::sync::Arc::new(fr[2].rgba()), 1);
        let out = m.finish().unwrap();
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_name_rule() {
        assert!(file_name_from_rule("a_%Y").starts_with("a_20"));
        assert_eq!(file_name_from_rule("x:y"), "x_y");
    }
}

//! 파일 열기: GIF·WebP·APNG(움직이는 그림), 일반 이미지 여러 장, 동영상(ffmpeg).

use crate::export::{Progress, STDERR_TAIL, find_ffmpeg, spawn_stderr_tail};
use crate::frame::{Frame, MAX_FRAME_BYTES, import_rgba_len};
use crate::project::resize_rgba;
use anyhow::{Context, Result, anyhow, bail};
use image::{AnimationDecoder, ImageDecoder, RgbaImage};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};

pub const VIDEO_EXTS: &[&str] = &["mp4", "webm", "mkv", "mov", "avi", "wmv", "m4v", "flv", "mpg", "mpeg", "ts"];
pub const IMAGE_EXTS: &[&str] = &["gif", "webp", "png", "apng", "jpg", "jpeg", "bmp"];

pub fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_video(path: &Path) -> bool {
    VIDEO_EXTS.contains(&ext_of(path).as_str())
}

/// 여러 장을 한 크기로 맞추는 방법
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Fit {
    /// 비율을 지키고 남는 곳은 투명하게
    Contain,
    /// 비율 무시하고 꽉 채우기
    Stretch,
}

/// 불러온 결과. 메모리 한도나 읽기 오류로 일부만 불러왔으면 warning이 있다.
pub struct Loaded {
    pub frames: Vec<Frame>,
    pub warning: Option<String>,
}

/// 메모리 한도를 지키며 프레임을 모은다
struct Collector {
    frames: Vec<Frame>,
    bytes: usize,
    max_bytes: usize,
    full: bool,
}

impl Collector {
    fn new(max_bytes: usize) -> Self {
        Collector { frames: vec![], bytes: 0, max_bytes, full: false }
    }

    /// 넣었으면 true. 한도를 넘으면 false (더 넣지 않는다)
    fn push(&mut self, f: Frame) -> bool {
        if self.full {
            return false;
        }
        self.bytes += f.packed_bytes();
        self.frames.push(f);
        if self.bytes >= self.max_bytes {
            self.full = true;
        }
        !self.full
    }

    fn finish(self) -> Result<Loaded> {
        if self.frames.is_empty() {
            bail!("불러올 프레임이 없습니다");
        }
        let warning = self.full.then(|| {
            format!("메모리 한도에 걸려 앞쪽 {}장만 불러왔습니다 (설정 → 녹화 → 편집 메모리 한도)", self.frames.len())
        });
        Ok(Loaded { frames: self.frames, warning })
    }
}

/// GIF 프레임 시간: 0이나 10ms는 브라우저가 100ms로 보여주므로 똑같이 맞춘다
fn gif_delay(ms: u32) -> u32 {
    if ms <= 10 { 100 } else { ms }
}

fn image_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_FRAME_BYTES as u64);
    limits
}

fn image_rgba_len(w: u32, h: u32, path: &Path) -> Result<usize> {
    import_rgba_len(w, h).ok_or_else(|| anyhow!("그림이 너무 커서 불러올 수 없습니다 ({w}×{h}): {}", path.display()))
}

fn limit_decoder(d: &mut impl ImageDecoder, path: &Path) -> Result<()> {
    let (w, h) = d.dimensions();
    image_rgba_len(w, h, path)?;
    d.set_limits(image_limits())?;
    Ok(())
}

/// 파일 하나의 프레임을 (가로, 세로, RGBA, 시간)으로 하나씩 on_frame에 넘긴다. 움직이지 않는 그림이면 한 장에 still_ms.
/// on_frame이 false를 돌려주면 (메모리 한도) 나머지 프레임은 풀지 않고 멈춘다.
fn decode_file(path: &Path, still_ms: u32, on_frame: &mut dyn FnMut(u32, u32, Vec<u8>, u32) -> bool) -> Result<()> {
    let open = || -> Result<BufReader<std::fs::File>> {
        Ok(BufReader::new(
            std::fs::File::open(path).with_context(|| format!("파일을 열 수 없습니다: {}", path.display()))?,
        ))
    };
    let mut any = false;
    let mut frames_of = |frames: image::Frames| -> Result<()> {
        for f in frames {
            let f = f?;
            let (n, d) = f.delay().numer_denom_ms();
            let ms = if d == 0 { 100 } else { n / d };
            let buf = f.into_buffer();
            any = true;
            if !on_frame(buf.width(), buf.height(), buf.into_raw(), gif_delay(ms)) {
                break;
            }
        }
        Ok(())
    };
    let still = |img: RgbaImage| (img.width(), img.height(), img.into_raw());
    let ext = ext_of(path);
    let single = match ext.as_str() {
        "gif" => {
            let mut d = image::codecs::gif::GifDecoder::new(open()?)?;
            limit_decoder(&mut d, path)?;
            frames_of(d.into_frames())?;
            None
        }
        "webp" => {
            let mut d = image::codecs::webp::WebPDecoder::new(open()?)?;
            limit_decoder(&mut d, path)?;
            if d.has_animation() {
                frames_of(d.into_frames())?;
                None
            } else {
                Some(still(image::DynamicImage::from_decoder(d)?.to_rgba8()))
            }
        }
        "png" | "apng" => {
            // PNG는 디코더를 만들 때도 버퍼를 준비하므로, IHDR만 먼저 읽어 크기를 검사한다.
            let mut header = png::Decoder::new_with_limits(open()?, png::Limits { bytes: MAX_FRAME_BYTES });
            let info = header.read_header_info()?;
            image_rgba_len(info.width, info.height, path)?;
            let mut d = image::codecs::png::PngDecoder::with_limits(open()?, image_limits())?;
            limit_decoder(&mut d, path)?;
            if d.is_apng()? {
                frames_of(d.apng()?.into_frames())?;
                None
            } else {
                Some(still(image::DynamicImage::from_decoder(d)?.to_rgba8()))
            }
        }
        _ => {
            let mut d = image::ImageReader::open(path)
                .with_context(|| format!("이미지를 읽을 수 없습니다: {}", path.display()))?
                .into_decoder()
                .with_context(|| format!("이미지를 읽을 수 없습니다: {}", path.display()))?;
            limit_decoder(&mut d, path)?;
            Some(still(
                image::DynamicImage::from_decoder(d)
                    .with_context(|| format!("이미지를 읽을 수 없습니다: {}", path.display()))?
                    .to_rgba8(),
            ))
        }
    };
    if let Some((w, h, px)) = single {
        any = true;
        on_frame(w, h, px, still_ms);
    }
    if !any {
        bail!("프레임이 없는 파일입니다: {}", path.display());
    }
    Ok(())
}

/// 그림을 (tw, th) 크기에 맞춘다
pub fn fit_rgba(w: u32, h: u32, px: &[u8], tw: u32, th: u32, fit: Fit) -> Vec<u8> {
    if (w, h) == (tw, th) {
        return px.to_vec();
    }
    match fit {
        Fit::Stretch => resize_rgba(w, h, px, tw, th, true),
        Fit::Contain => {
            let s = (tw as f32 / w as f32).min(th as f32 / h as f32);
            let (nw, nh) = (((w as f32 * s).round() as u32).clamp(1, tw), ((h as f32 * s).round() as u32).clamp(1, th));
            let small = resize_rgba(w, h, px, nw, nh, true);
            let mut out = vec![0u8; tw as usize * th as usize * 4];
            let (ox, oy) = ((tw - nw) / 2, (th - nh) / 2);
            let row_len = nw as usize * 4;
            for y in 0..nh {
                let d = ((oy as usize + y as usize) * tw as usize + ox as usize) * 4;
                let s = y as usize * row_len;
                out[d..d + row_len].copy_from_slice(&small[s..s + row_len]);
            }
            out
        }
    }
}

/// GIF·WebP·APNG·이미지 파일들을 차례로 불러온다. 크기는 첫 그림에 맞춘다.
pub fn load_files(paths: &[String], still_ms: u32, fit: Fit, max_bytes: usize, progress: Progress) -> Result<Loaded> {
    load_files_sized(paths, still_ms, fit, None, max_bytes, progress)
}

/// load_files와 같지만 size가 있으면 첫 그림 대신 그 크기에 맞춘다 (편집 중인 프레임 뒤에 붙일 때)
pub fn load_files_sized(
    paths: &[String],
    still_ms: u32,
    fit: Fit,
    mut size: Option<(u32, u32)>,
    max_bytes: usize,
    progress: Progress,
) -> Result<Loaded> {
    if let Some((w, h)) = size {
        import_rgba_len(w, h).ok_or_else(|| anyhow!("그림 크기가 너무 크거나 비었습니다 ({w}×{h})"))?;
    }
    let mut c = Collector::new(max_bytes);
    for (k, p) in paths.iter().enumerate() {
        let path = Path::new(p);
        if is_video(path) {
            bail!("동영상은 하나씩 따로 열어 주세요: {}", path.display());
        }
        progress(k as f32 / paths.len() as f32, "불러오는 중");
        decode_file(path, still_ms.max(10), &mut |w, h, px, ms| {
            let (tw, th) = *size.get_or_insert((w, h));
            let px = fit_rgba(w, h, &px, tw, th, fit);
            c.push(Frame::from_rgba(tw, th, &px, ms))
        })?;
        if c.full {
            break;
        }
    }
    c.finish()
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    pub fps: f32,
}

fn ffmpeg_command() -> Result<Command> {
    let ffmpeg =
        find_ffmpeg().ok_or_else(|| anyhow!("ffmpeg.exe를 찾을 수 없습니다. 동영상을 열려면 ffmpeg가 필요합니다."))?;
    let mut cmd = Command::new(ffmpeg);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    Ok(cmd)
}

/// `ffmpeg -i`가 알려 주는 정보에서 크기·길이·FPS를 읽는다 (ffprobe 없이)
// 지금은 테스트에서만 쓰므로 일반 빌드에서는 미사용 경고를 끈다.
#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_ffmpeg_info(text: &str) -> Option<VideoInfo> {
    parse_ffmpeg_info_inner(text).map(|(info, _)| info)
}

fn parse_ffmpeg_timestamp(text: &str) -> Option<u64> {
    let mut parts = text.trim().split(':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || m >= 60 || !s.is_finite() || !(0.0..60.0).contains(&s) {
        return None;
    }
    Some(((h as f64 * 3600.0 + m as f64 * 60.0 + s) * 1000.0).round() as u64)
}

/// 길이를 모르면 내부에서는 None, 공개 정보에서는 0으로 나타낸다.
fn parse_ffmpeg_info_inner(text: &str) -> Option<(VideoInfo, Option<u64>)> {
    let duration =
        text.split("Duration: ").nth(1).and_then(|s| s.split(',').next()).and_then(parse_ffmpeg_timestamp);
    let line = text.lines().find(|l| l.contains("Stream") && l.contains("Video:"))?;
    let (mut width, mut height, mut fps) = (0, 0, 30.0f32);
    for item in line.split(',') {
        let item = item.trim();
        let first = item.split_whitespace().next().unwrap_or("");
        if width == 0
            && let Some((a, b)) = first.split_once('x')
            && let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>())
            && a > 0
            && b > 0
        {
            (width, height) = (a, b);
        }
        if let Some(v) = item.strip_suffix(" fps") {
            fps = v.trim().parse().unwrap_or(fps);
        }
    }
    if width == 0 {
        return None;
    }
    // 휴대폰 동영상은 돌려서 보여주라는 표시가 있다. ffmpeg는 자동으로 돌리므로 가로·세로를 바꿔 둔다.
    let rotated = text.lines().any(|l| {
        let l = l.trim();
        (l.starts_with("rotate") && (l.contains("90") || l.contains("270")))
            || (l.contains("rotation of") && (l.contains("90") || l.contains("270")))
    });
    if rotated {
        std::mem::swap(&mut width, &mut height);
    }
    let fps = if fps.is_finite() { fps.clamp(1.0, 240.0) } else { 30.0 };
    Some((VideoInfo { width, height, duration_ms: duration.unwrap_or(0), fps }, duration))
}

/// 일반 진행 출력의 time=과 -progress의 out_time=을 모두 읽는다.
fn parse_ffmpeg_progress_time(text: &str) -> Option<u64> {
    text.split("time=")
        .skip(1)
        .filter_map(|s| s.split_whitespace().next().and_then(parse_ffmpeg_timestamp))
        .last()
}

/// 길이가 없는 영상을 끝까지 훑는다. 스트림 복사로 못 읽으면 디코드해서 다시 확인한다.
fn scan_video_duration(path: &str) -> Result<u64> {
    for copy in [true, false] {
        let mut cmd = ffmpeg_command()?;
        // 일반 진행 출력은 끄고, 길이 확인에 쓸 진행 정보는 stdout으로 받는다.
        cmd.args(["-v", "error", "-nostats", "-progress", "pipe:1"])
            .args(["-i", path, "-map", "0:v:0", "-an", "-sn", "-dn"]);
        if copy {
            cmd.args(["-c:v", "copy"]);
        }
        // output()으로 stdout과 stderr를 함께 읽고, 키보드 입력도 기다리지 않는다.
        let out = cmd.args(["-f", "null", "-"]).stdin(Stdio::null()).output().context("ffmpeg 실행 실패")?;
        if out.status.success()
            && let Some(ms) = parse_ffmpeg_progress_time(&String::from_utf8_lossy(&out.stdout))
            && ms > 0
        {
            return Ok(ms);
        }
    }
    bail!("동영상 길이를 확인할 수 없습니다. 다른 동영상 파일로 저장한 뒤 다시 열어 주세요.")
}

pub fn probe_video(path: &str) -> Result<VideoInfo> {
    let out = ffmpeg_command()?
        .args(["-hide_banner", "-i", path])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let (mut info, duration) =
        parse_ffmpeg_info_inner(&text).ok_or_else(|| anyhow!("동영상 정보를 읽을 수 없습니다"))?;
    if duration.is_none() || info.duration_ms == 0 {
        info.duration_ms = scan_video_duration(path)?;
    }
    info.duration_ms = info.duration_ms.max((1000.0 / info.fps as f64).ceil() as u64);
    Ok(info)
}

/// 구간을 고를 때 보여줄 장면 하나 (가로 max_w 이하 PNG)
pub fn video_frame_png(path: &str, at_ms: u64, max_w: u32) -> Result<Vec<u8>> {
    let out = ffmpeg_command()?
        .args(["-v", "error", "-ss", &format!("{:.3}", at_ms as f64 / 1000.0), "-i", path])
        .args(["-frames:v", "1", "-vf", &format!("scale='min({max_w},iw)':-2"), "-f", "image2pipe", "-c:v", "png", "-"])
        .stdin(Stdio::null())
        .output()?;
    if out.stdout.is_empty() {
        bail!("장면을 가져올 수 없습니다: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

#[derive(Debug, PartialEq)]
enum VideoReadEnd {
    Eof,
    PartialFrame,
    ReadError(String),
    MemoryLimit,
}

fn collect_raw_frames(
    mut reader: impl Read,
    w: u32,
    h: u32,
    frame_len: usize,
    fps: f32,
    expected: f32,
    c: &mut Collector,
    progress: Progress,
) -> VideoReadEnd {
    let mut buf = vec![0u8; frame_len];
    let mut k = 0u64;
    let stamp = |k: u64| (k as f64 * 1000.0 / fps as f64).round() as u32;
    loop {
        let mut filled = 0;
        while filled < frame_len {
            match reader.read(&mut buf[filled..]) {
                Ok(0) => return if filled == 0 { VideoReadEnd::Eof } else { VideoReadEnd::PartialFrame },
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return VideoReadEnd::ReadError(e.to_string()),
            }
        }
        // 고정 FPS지만 반올림 오차가 쌓이지 않게 시각 차이로 시간을 정한다
        let ms = stamp(k + 1) - stamp(k);
        k += 1;
        progress((k as f32 / expected).min(0.99), "동영상 불러오는 중");
        if !c.push(Frame::from_rgba(w, h, &buf, ms.max(10))) {
            return VideoReadEnd::MemoryLimit;
        }
    }
}

fn short_video_error(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = text.chars();
    let mut short: String = chars.by_ref().take(240).collect();
    if chars.next().is_some() {
        short.push('…');
    }
    short
}

fn finish_video(
    c: Collector,
    read_end: VideoReadEnd,
    exit_ok: bool,
    killed_for_limit: bool,
    stderr_tail: &str,
) -> Result<Loaded> {
    // 직접 멈춘 ffmpeg의 실패 종료와 파이프 오류 대신 메모리 경고를 보여 준다.
    if killed_for_limit && c.full {
        return c.finish();
    }
    let read_error = match &read_end {
        VideoReadEnd::PartialFrame => Some("프레임을 읽는 도중 데이터가 끊겼습니다"),
        VideoReadEnd::ReadError(e) => Some(e.as_str()),
        _ => None,
    };
    let reason = if !stderr_tail.trim().is_empty() {
        Some(short_video_error(stderr_tail))
    } else if let Some(e) = read_error {
        Some(short_video_error(e))
    } else if !exit_ok {
        Some("ffmpeg가 정상적으로 끝나지 않았습니다".to_string())
    } else {
        None
    };
    if let Some(reason) = reason {
        if c.frames.is_empty() {
            bail!("ffmpeg 오류: {reason}");
        }
        let warning = if matches!(&read_end, VideoReadEnd::Eof) && exit_ok {
            format!(
                "동영상을 읽는 중 ffmpeg가 오류를 알렸습니다. 일부 장면이 빠지거나 깨졌을 수 있습니다 ({}장 불러옴): {reason}",
                c.frames.len()
            )
        } else {
            format!("동영상을 끝까지 읽지 못해 앞쪽 {}장만 불러왔습니다: {reason}", c.frames.len())
        };
        let mut loaded = c.finish()?;
        loaded.warning = Some(match loaded.warning {
            Some(memory_warning) => format!("{memory_warning}\n{warning}"),
            None => warning,
        });
        return Ok(loaded);
    }
    c.finish()
}

/// 동영상의 [start, end) 구간을 fps로 뽑아 w×h 프레임으로 불러온다
pub fn load_video(
    path: &str,
    start_ms: u64,
    end_ms: u64,
    fps: f32,
    w: u32,
    h: u32,
    max_bytes: usize,
    progress: Progress,
) -> Result<Loaded> {
    if end_ms <= start_ms {
        bail!("구간이 비었습니다");
    }
    let fps = fps.clamp(1.0, 60.0);
    let (w, h) = (w.max(1), h.max(1));
    let frame_len = import_rgba_len(w, h).ok_or_else(|| anyhow!("동영상 크기가 너무 커서 불러올 수 없습니다 ({w}×{h})"))?;
    let mut child = ffmpeg_command()?
        .args(["-v", "error", "-ss", &format!("{:.3}", start_ms as f64 / 1000.0)])
        .args(["-t", &format!("{:.3}", (end_ms - start_ms) as f64 / 1000.0), "-i", path])
        .args(["-vf", &format!("fps={fps},scale={w}:{h}:flags=bicubic"), "-an"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("ffmpeg 실행 실패")?;
    // 여기부터는 어느 길로 나가든 ffmpeg를 기다리고 stderr 스레드를 join한다
    let err_tail = match spawn_stderr_tail(child.stderr.take().unwrap(), STDERR_TAIL) {
        Ok(t) => t,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::Error::new(e).context("ffmpeg 실행 실패"));
        }
    };
    let mut out = child.stdout.take().unwrap();
    let expected = ((end_ms - start_ms) as f32 / 1000.0 * fps).ceil().max(1.0);
    let mut c = Collector::new(max_bytes);
    let read_end = collect_raw_frames(&mut out, w, h, frame_len, fps, expected, &mut c, progress);
    let killed_for_limit = matches!(&read_end, VideoReadEnd::MemoryLimit);
    if killed_for_limit {
        let _ = child.kill();
    }
    // 읽기가 오류로 끝났어도 ffmpeg가 stdout에 막혀 멈추지 않게 먼저 닫는다
    drop(out);
    let status = child.wait();
    if status.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // ffmpeg가 끝나 stderr가 닫히면 스레드도 끝난다
    let mut err = err_tail.join().unwrap_or_default();
    let exit_ok = match status {
        Ok(status) => status.success(),
        Err(e) => {
            err.push_str(&format!("\nffmpeg 종료를 확인할 수 없습니다: {e}"));
            false
        }
    };
    finish_video(c, read_end, exit_ok, killed_for_limit, &err)
}

/// 동영상 전체를 tw×th 안에 비율을 지켜 불러오고 남는 곳은 투명하게 채운다 (편집 중인 프레임 뒤에 붙일 때).
/// FPS는 fps와 원본 FPS 중 작은 쪽 (동영상 열기 창의 처음 값과 같게).
pub fn load_video_contain(
    path: &str,
    fps: f32,
    tw: u32,
    th: u32,
    max_bytes: usize,
    progress: Progress,
) -> Result<Loaded> {
    import_rgba_len(tw, th).ok_or_else(|| anyhow!("그림 크기가 너무 크거나 비었습니다 ({tw}×{th})"))?;
    let info = probe_video(path)?;
    let fps = fps.min(info.fps.round()).max(1.0);
    let s = (tw as f32 / info.width as f32).min(th as f32 / info.height as f32);
    let nw = ((info.width as f32 * s).round() as u32).clamp(1, tw);
    let nh = ((info.height as f32 * s).round() as u32).clamp(1, th);
    let mut loaded = load_video(path, 0, info.duration_ms, fps, nw, nh, max_bytes, progress)?;
    if (nw, nh) != (tw, th) {
        use rayon::prelude::*;
        loaded.frames.par_iter_mut().for_each(|f| {
            *f = f.map_pixels(|w, h, px| (tw, th, fit_rgba(w, h, &px, tw, th, Fit::Contain)));
        });
    }
    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = !0u32;
        for &byte in kind.iter().chain(data) {
            crc ^= byte as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
            }
        }
        out.extend_from_slice(&(!crc).to_be_bytes());
    }

    #[test]
    fn huge_png_is_rejected_before_pixels() {
        let dir = std::env::temp_dir().join(format!("opencam_import_huge_png_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for side in [65535u32, 30000] {
            let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
            let mut ihdr = side.to_be_bytes().to_vec();
            ihdr.extend_from_slice(&side.to_be_bytes());
            ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
            png_chunk(&mut bytes, b"IHDR", &ihdr);
            png_chunk(&mut bytes, b"IDAT", &[]);
            png_chunk(&mut bytes, b"IEND", &[]);
            for ext in ["png", "apng"] {
                let path = dir.join(format!("huge_{side}.{ext}"));
                std::fs::write(&path, &bytes).unwrap();
                let err = decode_file(&path, 100, &mut |_, _, _, _| panic!("픽셀을 읽으면 안 됩니다")).unwrap_err();
                let msg = err.to_string();
                assert!(msg.contains("그림이 너무 커서 불러올 수 없습니다"), "{msg}");
                assert!(msg.contains(&format!("{side}×{side}")), "{msg}");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn huge_gif_is_rejected_before_pixels() {
        let dir = std::env::temp_dir().join(format!("opencam_import_huge_gif_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("huge.gif");
        let mut bytes = b"GIF89a".to_vec();
        // 논리 화면과 첫 프레임은 65535×65535지만 픽셀 데이터는 한 칸짜리다.
        bytes.extend_from_slice(&[255, 255, 255, 255, 0x80, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
        bytes.extend_from_slice(&[0x2c, 0, 0, 0, 0, 255, 255, 255, 255, 0, 2, 2, 0x44, 0x01, 0, 0x3b]);
        std::fs::write(&path, bytes).unwrap();
        let err = decode_file(&path, 100, &mut |_, _, _, _| panic!("픽셀을 읽으면 안 됩니다")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("그림이 너무 커서 불러올 수 없습니다 (65535×65535)"), "{msg}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn huge_video_is_rejected_without_ffmpeg() {
        for (w, h) in [(32768, 32768), (65535, 65535), (u32::MAX, u32::MAX)] {
            let err = load_video("없는_파일.mp4", 0, 1000, 30.0, w, h, usize::MAX, &|_, _| {})
                .err()
                .expect("너무 큰 동영상은 거부해야 합니다");
            assert!(err.to_string().contains("동영상 크기가 너무 커서 불러올 수 없습니다"), "{err}");
        }
    }

    #[test]
    fn invalid_target_is_rejected_before_reading_files() {
        for size in [(65535, 65535), (u32::MAX, u32::MAX), (0, 1), (1, 0)] {
            let err = load_files_sized(&[], 100, Fit::Contain, Some(size), usize::MAX, &|_, _| {})
                .err()
                .expect("잘못된 그림 크기는 거부해야 합니다");
            assert!(err.to_string().contains("그림 크기가 너무 크거나 비었습니다"), "{err}");
            let err = load_video_contain("없는_파일.mp4", 30.0, size.0, size.1, usize::MAX, &|_, _| {})
                .err()
                .expect("잘못된 그림 크기는 거부해야 합니다");
            assert!(err.to_string().contains("그림 크기가 너무 크거나 비었습니다"), "{err}");
        }
    }

    #[test]
    fn still_images_and_open_errors() {
        let dir = std::env::temp_dir().join(format!("opencam_import_still_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = image::ImageBuffer::from_pixel(3, 2, image::Rgb([11u8, 22, 33]));
        for ext in ["jpg", "bmp"] {
            let path = dir.join(format!("still.{ext}"));
            img.save(&path).unwrap();
            let loaded = load_files(&[path.to_string_lossy().into_owned()], 100, Fit::Contain, usize::MAX, &|_, _| {})
                .unwrap();
            assert_eq!(loaded.frames.len(), 1);
            assert_eq!((loaded.frames[0].width, loaded.frames[0].height), (3, 2));
            assert_eq!(loaded.frames[0].rgba().len(), 24);
        }
        let path = dir.join("없는_그림.jpg");
        let err = decode_file(&path, 100, &mut |_, _, _, _| panic!("픽셀을 읽으면 안 됩니다")).unwrap_err();
        assert_eq!(err.to_string(), format!("이미지를 읽을 수 없습니다: {}", path.display()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn parse_ffmpeg_output() {
        let text = "Input #0, mov,mp4, from 'a.mp4':\n  Duration: 00:01:02.50, start: 0.000000, bitrate: 900 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(progressive), 1920x1080 [SAR 1:1 DAR 16:9], 800 kb/s, 29.97 fps, 29.97 tbr, 30k tbn (default)\n";
        let info = parse_ffmpeg_info(text).unwrap();
        assert_eq!(info, VideoInfo { width: 1920, height: 1080, duration_ms: 62500, fps: 29.97 });
        // 세로로 찍은 휴대폰 동영상
        let rot = format!("{text}    Side data:\n      displaymatrix: rotation of -90.00 degrees\n");
        assert_eq!(parse_ffmpeg_info(&rot).unwrap().width, 1080);
    }

    #[test]
    fn parse_ffmpeg_unknown_duration() {
        let text = "Input #0, matroska,webm, from 'a.webm':\n  Duration: N/A, start: 0.000000, bitrate: N/A\n  Stream #0:0: Video: vp8, yuv420p(progressive), 640x360, SAR 1:1 DAR 16:9, 30 fps, 30 tbr, 1k tbn (default)\n";
        for text in [text.to_string(), text.replace("  Duration: N/A, start: 0.000000, bitrate: N/A\n", "")] {
            let (info, duration) = parse_ffmpeg_info_inner(&text).unwrap();
            assert_eq!(duration, None);
            assert_eq!(info, VideoInfo { width: 640, height: 360, duration_ms: 0, fps: 30.0 });
            assert_eq!(parse_ffmpeg_info(&text), Some(info));
        }
        assert!(parse_ffmpeg_info("Duration: N/A\nStream #0:0: Audio: opus").is_none());
    }

    #[test]
    fn parse_ffmpeg_last_progress_time() {
        let text = "frame=1 time=N/A speed=0x\rframe=5 time=00:00:00.50 speed=1x\r\nframe=12 time=00:01:02.50 speed=2x\rtime=N/A\n";
        assert_eq!(parse_ffmpeg_progress_time(text), Some(62500));
        let progress = "out_time=00:00:00.100000\nprogress=continue\nout_time=00:00:01.000000\nprogress=end\n";
        assert_eq!(parse_ffmpeg_progress_time(progress), Some(1000));
        assert_eq!(parse_ffmpeg_progress_time("time=N/A\rout_time=-00:00:00.10\ntime=NaN"), None);
        assert_eq!(parse_ffmpeg_progress_time("time=00:00:00.000000"), Some(0));
        assert_eq!(parse_ffmpeg_timestamp("00:00:NaN"), None);
        assert_eq!(parse_ffmpeg_timestamp("00:60:00"), None);
    }

    fn collect_test_video(bytes: Vec<u8>, max_bytes: usize) -> (Collector, VideoReadEnd) {
        let mut c = Collector::new(max_bytes);
        let end = collect_raw_frames(std::io::Cursor::new(bytes), 1, 1, 4, 10.0, 100.0, &mut c, &|_, _| {});
        (c, end)
    }

    #[test]
    fn raw_video_clean_eof_has_no_warning() {
        let (c, end) = collect_test_video(vec![255; 8], usize::MAX);
        assert_eq!(end, VideoReadEnd::Eof);
        // 요청한 구간보다 짧아도 깨끗하게 끝났으면 정상이다.
        let loaded = finish_video(c, end, true, false, "").unwrap();
        assert_eq!(loaded.frames.len(), 2);
        assert!(loaded.frames.iter().all(|f| f.delay_ms == 100));
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn raw_video_partial_frame_warns() {
        let (c, end) = collect_test_video(vec![255; 11], usize::MAX);
        assert_eq!(end, VideoReadEnd::PartialFrame);
        let loaded = finish_video(c, end, true, false, "").unwrap();
        assert_eq!(loaded.frames.len(), 2);
        let warning = loaded.warning.unwrap();
        assert!(warning.contains("앞쪽 2장만 불러왔습니다"), "{warning}");
        assert!(warning.contains("데이터가 끊겼습니다"), "{warning}");
    }

    #[test]
    fn raw_video_clean_eof_with_stderr_warns() {
        let (c, end) = collect_test_video(vec![255; 8], usize::MAX);
        assert_eq!(end, VideoReadEnd::Eof);
        let loaded = finish_video(c, end, true, false, "손상된 프레임").unwrap();
        assert_eq!(loaded.frames.len(), 2);
        assert_eq!(
            loaded.warning.as_deref(),
            Some("동영상을 읽는 중 ffmpeg가 오류를 알렸습니다. 일부 장면이 빠지거나 깨졌을 수 있습니다 (2장 불러옴): 손상된 프레임")
        );
    }

    #[test]
    fn raw_video_failed_exit_warns() {
        for stderr in ["", "손상된 프레임"] {
            let (c, end) = collect_test_video(vec![255; 8], usize::MAX);
            let loaded = finish_video(c, end, false, false, stderr).unwrap();
            assert_eq!(loaded.frames.len(), 2);
            let warning = loaded.warning.unwrap();
            assert!(warning.contains("앞쪽 2장만 불러왔습니다"), "{warning}");
            if !stderr.is_empty() {
                assert!(warning.contains(stderr), "{warning}");
            }
        }
    }

    #[test]
    fn raw_video_read_error_warns() {
        struct FailingReader(std::io::Cursor<Vec<u8>>, bool);
        impl Read for FailingReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if !self.1 {
                    self.1 = true;
                    return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "다시 읽기"));
                }
                // 한 번에 한 바이트만 읽혀도 프레임 경계를 정확히 따라야 한다.
                let n = self.0.read(&mut buf[..1])?;
                if n == 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, "읽기 실패"));
                }
                Ok(n)
            }
        }
        let mut c = Collector::new(usize::MAX);
        let reader = FailingReader(std::io::Cursor::new(vec![255; 8]), false);
        let end = collect_raw_frames(reader, 1, 1, 4, 10.0, 100.0, &mut c, &|_, _| {});
        assert_eq!(end, VideoReadEnd::ReadError("읽기 실패".to_string()));
        let loaded = finish_video(c, end, true, false, "").unwrap();
        assert_eq!(loaded.frames.len(), 2);
        assert!(loaded.warning.unwrap().contains("읽기 실패"));
    }

    #[test]
    fn raw_video_failure_without_frames_is_error() {
        let cases = [(vec![], false, "실패"), (vec![255; 3], true, ""), (vec![], true, "실패"), (vec![], true, "")];
        for (bytes, exit_ok, stderr) in cases {
            let (c, end) = collect_test_video(bytes, usize::MAX);
            assert!(finish_video(c, end, exit_ok, false, stderr).is_err());
        }
    }

    #[test]
    fn raw_video_memory_limit_keeps_memory_warning() {
        let (c, end) = collect_test_video(vec![255; 8], 1);
        assert_eq!(end, VideoReadEnd::MemoryLimit);
        let loaded = finish_video(c, end, false, true, "파이프가 닫혔습니다").unwrap();
        assert_eq!(loaded.frames.len(), 1);
        let warning = loaded.warning.unwrap();
        assert!(warning.contains("메모리 한도"), "{warning}");
        assert!(!warning.contains("끝까지 읽지 못해"), "{warning}");
        assert!(!warning.contains("파이프"), "{warning}");
    }

    #[test]
    fn video_error_is_short_and_keeps_korean_characters() {
        let (c, end) = collect_test_video(vec![255; 8], usize::MAX);
        let loaded = finish_video(c, end, false, false, &"손상\n".repeat(1000)).unwrap();
        let warning = loaded.warning.unwrap();
        assert!(warning.chars().count() < 300);
        assert!(warning.ends_with('…'));
        assert!(!warning.contains('\n'));
    }

    #[test]
    fn fit_contain_letterbox() {
        // 2x1 빨강을 2x2에 넣으면 비율을 지켜 한 줄만 차고, 남는 줄은 투명
        let px = [255u8, 0, 0, 255].repeat(2);
        let out = fit_rgba(2, 1, &px, 2, 2, Fit::Contain);
        assert_eq!(out.len(), 16);
        assert_eq!(&out[..4], &[255, 0, 0, 255]); // 남는 1줄은 나눌 수 없어 그림이 위쪽에
        assert_eq!(out[8 + 3], 0);
        // 2x2를 4x2에 넣으면 좌우에 1칸씩 여백
        let out = fit_rgba(2, 2, &[7u8; 16], 4, 2, Fit::Contain);
        assert_eq!(out[3], 0);
        assert_eq!(out[4 + 3], 7);
    }

    #[test]
    fn round_trip_gif_webp_apng_png() {
        use crate::export::{ExportOptions, Format, export};
        let s = crate::settings::Settings::default();
        let dir = std::env::temp_dir().join(format!("opencam_import_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let frames: Vec<Frame> =
            (0..3).map(|i| Frame::from_rgba(6, 4, &[(i * 80) as u8, 50, 200, 255].repeat(24), 100)).collect();
        for f in [Format::Gif, Format::Webp, Format::Apng] {
            let p = dir.join(format!("t_{f:?}.{}", f.ext()));
            let o = ExportOptions {
                format: f,
                quality: 90,
                limit_fps: None,
                skip_similar: None,
                skip_similar_time_limit_ms: None,
                resize_percent: 100,
                embed_progress: false,
                protect_colors: vec![],
                gif_fit_50fps: false,
            };
            export(&frames, &p, &o, &s, &|_, _| {}).unwrap();
            let loaded =
                load_files(&[p.to_string_lossy().into_owned()], 100, Fit::Contain, usize::MAX, &|_, _| {}).unwrap();
            assert_eq!(loaded.frames.len(), 3, "{f:?}");
            assert_eq!((loaded.frames[0].width, loaded.frames[0].height), (6, 4));
            assert_eq!(loaded.frames.iter().map(|f| f.delay_ms).sum::<u32>(), 300, "{f:?}");
        }
        // 크기가 다른 정지 이미지 두 장 → 첫 이미지 크기에 맞춘다
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        crate::export::write_png(&a, 4, 4, &[9u8; 64]).unwrap();
        crate::export::write_png(&b, 8, 2, &[9u8; 64]).unwrap();
        let paths = [a, b].map(|p| p.to_string_lossy().into_owned());
        let loaded = load_files(&paths, 250, Fit::Stretch, usize::MAX, &|_, _| {}).unwrap();
        assert_eq!(loaded.frames.len(), 2);
        assert!(loaded.frames.iter().all(|f| (f.width, f.height, f.delay_ms) == (4, 4, 250)));
        // 크기를 정해 주면 첫 그림 대신 그 크기에 맞춘다 (8×2 → 4×4 안에 4×1, 위아래는 투명)
        let loaded = load_files_sized(&paths[1..], 100, Fit::Contain, Some((4, 4)), usize::MAX, &|_, _| {}).unwrap();
        assert_eq!(loaded.frames.len(), 1);
        assert_eq!((loaded.frames[0].width, loaded.frames[0].height), (4, 4));
        assert_eq!(loaded.frames[0].rgba()[3], 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn animated_file_stops_decoding_at_memory_limit() {
        // 둘째 프레임이 깨진 GIF. 메모리 한도로 첫 장만 쓰면 둘째 장은 풀지 않아야 한다
        // (예전에는 모든 프레임을 먼저 풀어서, 긴 GIF는 한도와 상관없이 메모리를 다 썼다)
        let dir = std::env::temp_dir().join(format!("opencam_import_limit_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cut.gif");
        {
            let mut enc = gif::Encoder::new(std::fs::File::create(&path).unwrap(), 64, 64, &[0, 0, 0, 255, 255, 255])
                .unwrap();
            for k in 0..2u8 {
                let mut fr = gif::Frame { width: 64, height: 64, ..Default::default() };
                fr.buffer = std::borrow::Cow::Owned((0..64 * 64).map(|i| ((i as u8) ^ k) & 1).collect());
                enc.write_frame(&fr).unwrap();
            }
        }
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 40]).unwrap();
        let p = [path.to_string_lossy().into_owned()];
        let loaded = load_files(&p, 100, Fit::Contain, 1, &|_, _| {}).unwrap();
        assert_eq!(loaded.frames.len(), 1);
        assert!(loaded.warning.is_some());
        // 한도가 넉넉하면 깨진 프레임까지 읽으므로 오류
        assert!(load_files(&p, 100, Fit::Contain, usize::MAX, &|_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn video_if_ffmpeg() {
        use crate::export::{ExportOptions, Format, export};
        if find_ffmpeg().is_none() {
            return;
        }
        let s = crate::settings::Settings::default();
        let dir = std::env::temp_dir().join(format!("opencam_vimport_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let frames: Vec<Frame> =
            (0..10).map(|i| Frame::from_rgba(16, 8, &[(i * 25) as u8, 0, 0, 255].repeat(128), 100)).collect();
        let p = dir.join("v.mp4");
        let o = ExportOptions {
            format: Format::Mp4,
            quality: 80,
            limit_fps: None,
            skip_similar: None,
            skip_similar_time_limit_ms: None,
            resize_percent: 100,
            embed_progress: false,
            protect_colors: vec![],
            gif_fit_50fps: false,
        };
        export(&frames, &p, &o, &s, &|_, _| {}).unwrap();
        let ps = p.to_string_lossy().into_owned();
        let info = probe_video(&ps).unwrap();
        assert_eq!((info.width, info.height), (16, 8));
        assert!((900..=1100).contains(&info.duration_ms), "{info:?}");
        // 0.2초 ~ 0.7초를 10fps로 → 5장
        let loaded = load_video(&ps, 200, 700, 10.0, 8, 4, usize::MAX, &|_, _| {}).unwrap();
        assert_eq!(loaded.frames.len(), 5);
        assert_eq!((loaded.frames[0].width, loaded.frames[0].height), (8, 4));
        // 구간 고르기용 장면 (PNG 서명으로 시작)
        let png = video_frame_png(&ps, 500, 480).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        // 메모리 한도
        let loaded = load_video(&ps, 0, 1000, 10.0, 8, 4, 1, &|_, _| {}).unwrap();
        assert_eq!(loaded.frames.len(), 1);
        assert!(loaded.warning.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_duration_video_if_ffmpeg() {
        if find_ffmpeg().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("opencam_vduration_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("no_duration.m4v");
        // 생 MPEG-4 영상에는 전체 길이가 없다. 파이프로 써서 컨테이너 정보를 만들지 않는다.
        let made = ffmpeg_command()
            .unwrap()
            .args(["-v", "error", "-f", "lavfi", "-i", "testsrc=size=32x24:rate=10", "-t", "1"])
            .args(["-c:v", "mpeg4", "-pix_fmt", "yuv420p", "-f", "m4v", "-"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
        std::fs::write(&p, &made.stdout).unwrap();
        let ps = p.to_string_lossy().into_owned();
        let header =
            ffmpeg_command().unwrap().args(["-hide_banner", "-i", &ps]).stdin(Stdio::null()).output().unwrap();
        let (_, duration) = parse_ffmpeg_info_inner(&String::from_utf8_lossy(&header.stderr)).unwrap();
        assert_eq!(duration, None);
        let info = probe_video(&ps).unwrap();
        assert_eq!((info.width, info.height), (32, 24));
        assert!((900..=1100).contains(&info.duration_ms), "{info:?}");
        let opened = load_video(&ps, 0, info.duration_ms, 10.0, 32, 24, usize::MAX, &|_, _| {}).unwrap();
        assert_eq!(opened.frames.len(), 10);
        assert!(opened.warning.is_none());
        let appended = load_video_contain(&ps, 10.0, 32, 24, usize::MAX, &|_, _| {}).unwrap();
        assert_eq!(appended.frames.len(), 10);
        assert!(appended.warning.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_video_does_not_hang() {
        // 깨진 동영상은 ffmpeg가 stderr에 64KB 넘게 쓴다. 예전에는 stderr를 안 읽어서 파이프가 차 서로 기다리며 멈췄다
        if find_ffmpeg().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("opencam_vcorrupt_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bad.mp4");
        let made = ["libx264", "mpeg4"].into_iter().any(|codec| {
            ffmpeg_command()
                .unwrap()
                .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=30", "-t", "20"])
                .args(["-c:v", codec, "-pix_fmt", "yuv420p"])
                .arg(&p)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        });
        if !made {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        // 가운데 절반을 쓰레기로 덮는다
        let mut bytes = std::fs::read(&p).unwrap();
        let n = bytes.len();
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        for b in &mut bytes[n / 4..n * 3 / 4] {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        std::fs::write(&p, &bytes).unwrap();
        let ps = p.to_string_lossy().into_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = load_video(&ps, 0, 20_000, 10.0, 160, 90, usize::MAX, &|_, _| {});
            let _ = tx.send(r.map(|l| l.frames.len()).map_err(|e| e.to_string()));
        });
        let r = rx.recv_timeout(std::time::Duration::from_secs(60));
        let _ = std::fs::remove_dir_all(&dir);
        // 일부만 불러오든 오류든 괜찮다. 멈추지만 않으면 된다
        let r = r.expect("깨진 동영상을 불러오다 멈춤");
        if let Err(e) = r {
            assert!(e.len() < STDERR_TAIL + 100, "오류 메시지가 너무 김: {} 바이트", e.len());
        }
    }
}

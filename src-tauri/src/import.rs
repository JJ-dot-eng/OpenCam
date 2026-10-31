//! 파일 열기: GIF·WebP·APNG(움직이는 그림), 일반 이미지 여러 장, 동영상(ffmpeg).

use crate::export::{Progress, STDERR_TAIL, find_ffmpeg, spawn_stderr_tail};
use crate::frame::Frame;
use crate::project::resize_rgba;
use anyhow::{Context, Result, anyhow, bail};
use image::{AnimationDecoder, RgbaImage};
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

/// 불러온 결과. 메모리 한도에 걸려 일부만 불러왔으면 warning이 있다.
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
            frames_of(image::codecs::gif::GifDecoder::new(open()?)?.into_frames())?;
            None
        }
        "webp" => {
            let d = image::codecs::webp::WebPDecoder::new(open()?)?;
            if d.has_animation() {
                frames_of(d.into_frames())?;
                None
            } else {
                Some(still(image::DynamicImage::from_decoder(d)?.to_rgba8()))
            }
        }
        "png" | "apng" => {
            let d = image::codecs::png::PngDecoder::new(open()?)?;
            if d.is_apng()? {
                frames_of(d.apng()?.into_frames())?;
                None
            } else {
                Some(still(image::DynamicImage::from_decoder(d)?.to_rgba8()))
            }
        }
        _ => Some(still(
            image::open(path).with_context(|| format!("이미지를 읽을 수 없습니다: {}", path.display()))?.to_rgba8(),
        )),
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
            let mut out = vec![0u8; (tw * th * 4) as usize];
            let (ox, oy) = ((tw - nw) / 2, (th - nh) / 2);
            for y in 0..nh {
                let d = (((oy + y) * tw + ox) * 4) as usize;
                let s = (y * nw * 4) as usize;
                out[d..d + (nw * 4) as usize].copy_from_slice(&small[s..s + (nw * 4) as usize]);
            }
            out
        }
    }
}

/// GIF·WebP·APNG·이미지 파일들을 차례로 불러온다. 크기는 첫 그림에 맞춘다.
pub fn load_files(paths: &[String], still_ms: u32, fit: Fit, max_bytes: usize, progress: Progress) -> Result<Loaded> {
    let mut c = Collector::new(max_bytes);
    let mut size: Option<(u32, u32)> = None;
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
pub fn parse_ffmpeg_info(text: &str) -> Option<VideoInfo> {
    let dur = text.split("Duration: ").nth(1)?.split(',').next()?.trim();
    let mut parts = dur.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    let duration_ms = ((h * 3600.0 + m * 60.0 + s) * 1000.0) as u64;
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
    Some(VideoInfo { width, height, duration_ms, fps: fps.clamp(1.0, 240.0) })
}

pub fn probe_video(path: &str) -> Result<VideoInfo> {
    let out =
        ffmpeg_command()?.args(["-hide_banner", "-i", path]).stdout(Stdio::null()).stderr(Stdio::piped()).output()?;
    let text = String::from_utf8_lossy(&out.stderr);
    parse_ffmpeg_info(&text).ok_or_else(|| anyhow!("동영상 정보를 읽을 수 없습니다"))
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
    let frame_len = (w * h * 4) as usize;
    let mut buf = vec![0u8; frame_len];
    let mut c = Collector::new(max_bytes);
    let mut k = 0u64;
    let stamp = |k: u64| (k as f64 * 1000.0 / fps as f64).round() as u32;
    loop {
        if out.read_exact(&mut buf).is_err() {
            break; // 끝
        }
        // 고정 FPS지만 반올림 오차가 쌓이지 않게 시각 차이로 시간을 정한다
        let ms = stamp(k + 1) - stamp(k);
        k += 1;
        progress((k as f32 / expected).min(0.99), "동영상 불러오는 중");
        if !c.push(Frame::from_rgba(w, h, &buf, ms.max(10))) {
            let _ = child.kill();
            break;
        }
    }
    // 읽기가 오류로 끝났어도 ffmpeg가 stdout에 막혀 멈추지 않게 먼저 닫는다
    drop(out);
    let status = child.wait();
    if status.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // ffmpeg가 끝나 stderr가 닫히면 스레드도 끝난다
    let err = err_tail.join().unwrap_or_default();
    let status = status?;
    if c.frames.is_empty() && !status.success() {
        bail!("ffmpeg 오류: {err}");
    }
    c.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

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

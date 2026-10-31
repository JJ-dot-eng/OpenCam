//! 창 없이 파일 열기 → 붙이기 → 여섯 형식 저장 → 다시 읽기를 확인한다.

use crate::export::{self, ExportOptions, Format};
use crate::frame::Frame;
use crate::import::{self, Fit, Loaded};
use crate::project::Project;
use crate::settings::Settings;
use crate::{AppendPermit, check_append_generation, collect_append_files, file_name, load_for_append};
use image::{AnimationDecoder, ImageDecoder, Rgba, RgbaImage};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 32;
const BASE_SIZE: (u32, u32) = (64, 64);
const FPS: f32 = 10.0;
const MAX_BYTES: usize = 16 * 1024 * 1024;
// 팔레트 색 줄이기는 채널별 4, 손실 압축은 경계에서 떨어진 픽셀에 20까지 허용한다.
const PALETTE_TOLERANCE: u8 = 4;
const LOSSY_TOLERANCE: u8 = 20;
const INPUT_VIDEO_TOLERANCE: u8 = 6;
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];
static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!(
            "opencam e2e_{}_{}_{}", std::process::id(), stamp, NEXT_DIR.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            eprintln!("테스트 임시 폴더를 지우지 못했습니다: {}: {e}", self.0.display());
        }
    }
}

fn quiet(_: f32, _: &str) {}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn ffmpeg_for_tests() -> Option<PathBuf> {
    let ffmpeg = export::find_ffmpeg();
    if ffmpeg.is_none() {
        assert!(
            std::env::var("OPENCAM_REQUIRE_FFMPEG").as_deref() != Ok("1")
                && std::env::var("CI").as_deref() != Ok("true"),
            "ffmpeg가 없습니다. CI의 동영상 e2e 테스트에는 ffmpeg가 꼭 필요합니다",
        );
        eprintln!("ffmpeg가 없어 동영상 e2e 테스트를 건너뜁니다 (그림 형식은 계속 확인합니다)");
    }
    ffmpeg
}

fn ffmpeg_output(ffmpeg: &Path, args: &[&str]) -> Output {
    Command::new(ffmpeg).args(args).stdin(Stdio::null()).output().expect("ffmpeg 실행 실패")
}

fn ffmpeg_ok(ffmpeg: &Path, args: &[&str]) -> Output {
    let out = ffmpeg_output(ffmpeg, args);
    assert!(out.status.success(), "ffmpeg 실패: {}", String::from_utf8_lossy(&out.stderr));
    out
}

fn has_encoder(ffmpeg: &Path, name: &str) -> bool {
    let out = ffmpeg_ok(ffmpeg, &["-hide_banner", "-encoders"]);
    String::from_utf8_lossy(&out.stdout).lines().any(|line| line.split_whitespace().nth(1) == Some(name))
}

fn blocks(left: [u8; 4], right: [u8; 4]) -> RgbaImage {
    RgbaImage::from_fn(WIDTH, HEIGHT, |x, _| Rgba(if x < WIDTH / 2 { left } else { right }))
}

fn frame(image: &RgbaImage, delay_ms: u32) -> Frame {
    Frame::from_rgba(image.width(), image.height(), image.as_raw(), delay_ms)
}

fn base_frame() -> Frame {
    let image = RgbaImage::from_fn(BASE_SIZE.0, BASE_SIZE.1, |x, _| {
        Rgba(if x < 32 { [255, 255, 0, 255] } else { [0, 255, 255, 255] })
    });
    // GIF의 누적 시간 반올림도 확인하도록 10ms로 나누어떨어지지 않게 둔다.
    frame(&image, 105)
}

fn png_fixture(dir: &TempDir, transparent: bool) -> (PathBuf, Vec<Frame>) {
    let path = dir.path(if transparent { "transparent.png" } else { "opaque.png" });
    let image = blocks(if transparent { CLEAR } else { RED }, BLUE);
    image.save(&path).unwrap();
    (path, vec![frame(&image, 100)])
}

fn gif_fixture(dir: &TempDir) -> (PathBuf, Vec<Frame>) {
    let path = dir.path("transparency.gif");
    let mut encoder = gif::Encoder::new(
        std::fs::File::create(&path).unwrap(), WIDTH as u16, HEIGHT as u16,
        &[0, 0, 0, 255, 0, 0, 0, 0, 255, 0, 255, 0],
    ).unwrap();
    let mut expected = Vec::new();
    for (left, right, delay_cs) in [(1u8, 2u8, 0u16), (0, 3, 1), (1, 2, 13)] {
        let indices: Vec<u8> = (0..WIDTH * HEIGHT)
            .map(|k| if k % WIDTH < WIDTH / 2 { left } else { right }).collect();
        let gif_frame = gif::Frame {
            width: WIDTH as u16,
            height: HEIGHT as u16,
            buffer: std::borrow::Cow::Owned(indices),
            transparent: Some(0),
            // 앞 장에서 칠한 왼쪽이 다음 장에서는 투명해진다.
            dispose: gif::DisposalMethod::Background,
            delay: delay_cs,
            ..Default::default()
        };
        encoder.write_frame(&gif_frame).unwrap();
        let image = blocks(if left == 0 { CLEAR } else { RED }, if right == 3 { GREEN } else { BLUE });
        // 불러오기는 GIF의 0·10ms를 브라우저와 같이 100ms로 바꾼다.
        expected.push(frame(&image, if delay_cs <= 1 { 100 } else { u32::from(delay_cs) * 10 }));
    }
    drop(encoder);
    (path, expected)
}

#[derive(Clone, Copy)]
enum VideoFixture {
    Mp4,
    NoDurationWebm,
    CorruptAvi,
}

fn video_fixture(dir: &TempDir, ffmpeg: &Path, kind: VideoFixture) -> PathBuf {
    // 왼쪽은 빨강·초록을 번갈아 쓰고, 오른쪽은 계속 파랑으로 둔다.
    let source = concat!(
        "color=c=red:s=64x32:r=10:d=0.5,",
        "drawbox=x=32:y=0:w=32:h=32:color=blue:t=fill,",
        "drawbox=x=0:y=0:w=32:h=32:color=lime:t=fill:enable=mod(n\\,2)",
    );
    let path = dir.path(match kind {
        VideoFixture::Mp4 => "normal.mp4",
        VideoFixture::NoDurationWebm => "without-duration.webm",
        VideoFixture::CorruptAvi => "broken.avi",
    });
    let mut args = vec!["-y", "-v", "error", "-f", "lavfi", "-i", source];
    match kind {
        VideoFixture::Mp4 => {
            let codec = if has_encoder(ffmpeg, "libx264") { "libx264" } else { "mpeg4" };
            args.extend(["-c:v", codec, "-pix_fmt", "yuv420p"]);
        }
        VideoFixture::NoDurationWebm => {
            let codec = if has_encoder(ffmpeg, "libvpx-vp9") { "libvpx-vp9" } else { "libvpx" };
            args.extend(["-c:v", codec, "-pix_fmt", "yuv420p", "-f", "webm", "pipe:1"]);
            let out = ffmpeg_ok(ffmpeg, &args);
            std::fs::write(&path, out.stdout).unwrap();
            // 일반 파일에 바로 쓰면 ffmpeg가 길이를 기록하므로 반드시 파이프로 만든다.
            let info = ffmpeg_output(ffmpeg, &["-hide_banner", "-i", &path_text(&path)]);
            let text = String::from_utf8_lossy(&info.stderr);
            assert!(text.contains("Duration: N/A"), "길이 정보 없는 테스트 파일이 아닙니다: {text}");
            return path;
        }
        VideoFixture::CorruptAvi => args.extend(["-c:v", "rawvideo", "-pix_fmt", "bgr24"]),
    }
    let name = path_text(&path);
    args.push(&name);
    ffmpeg_ok(ffmpeg, &args);
    if matches!(kind, VideoFixture::CorruptAvi) {
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        let len = file.metadata().unwrap().len();
        // AVI의 길이 정보는 남겨 두고 마지막 무압축 프레임의 중간에서 자른다.
        // ffmpeg 7.1은 앞 4장을 읽고, 종료 코드 0이어도 잘린 프레임 오류를 stderr에 남긴다.
        file.set_len(len - u64::from(WIDTH * HEIGHT * 3 / 2)).unwrap();
        drop(file);
        let decoded = ffmpeg_output(ffmpeg, &[
            "-v", "error", "-i", &name, "-an", "-f", "rawvideo", "-pix_fmt", "rgba", "-",
        ]);
        let count = decoded.stdout.len() / (WIDTH * HEIGHT * 4) as usize;
        // ffmpeg 버전마다 오류 문구·종료 코드는 달라도 일부 장면과 오류가 함께 있어야 한다.
        assert!((1..5).contains(&count), "손상 테스트 파일에서 읽은 장수: {count}");
        assert!(!decoded.stderr.is_empty(), "손상 테스트 파일에 ffmpeg 오류가 없습니다");
    }
    path
}

fn pixel(px: &[u8], width: u32, x: u32, y: u32) -> &[u8] {
    let i = ((y * width + x) * 4) as usize;
    &px[i..i + 4]
}

fn assert_pixel(actual: &[u8], expected: &[u8], tolerance: u8, label: &str) {
    assert_eq!(actual[3], expected[3], "{label}: 알파 {actual:?}, 기대 {expected:?}");
    // 완전히 투명한 픽셀의 숨은 RGB는 표시되지 않으며 인코더가 바꿔도 괜찮다.
    if expected[3] != 0 {
        assert!((0..3).all(|c| actual[c].abs_diff(expected[c]) <= tolerance),
            "{label}: 색 {actual:?}, 기대 {expected:?}, 허용 차이 {tolerance}");
    }
}

fn assert_samples(actual: &RgbaImage, expected: &Frame, tolerance: u8, label: &str) {
    assert_eq!(actual.dimensions(), (expected.width, expected.height), "{label}: 크기");
    let rgba = expected.rgba();
    // 색 경계와 투명 여백의 경계에서 8픽셀 이상 떨어진 점을 본다.
    for y in (8..expected.height).step_by(16) {
        for x in (8..expected.width).step_by(16) {
            assert_pixel(pixel(actual.as_raw(), actual.width(), x, y),
                pixel(&rgba, expected.width, x, y), tolerance, &format!("{label} ({x}, {y})"));
        }
    }
}

fn assert_frames(actual: &[Frame], expected: &[Frame], tolerance: u8, exact: bool) {
    assert_eq!(actual.len(), expected.len(), "불러온 장수");
    for (i, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(actual.delay_ms, expected.delay_ms, "{i}번 프레임 시간");
        let image = RgbaImage::from_raw(actual.width, actual.height, actual.rgba()).unwrap();
        assert_samples(&image, expected, tolerance, &format!("{i}번 프레임"));
        if exact {
            assert_eq!(image.as_raw(), &expected.rgba(), "{i}번 전체 픽셀");
        }
    }
}

fn import_fixture(path: &Path) -> Loaded {
    let name = path_text(path);
    if import::is_video(path) {
        let info = import::probe_video(&name).unwrap();
        assert_eq!((info.width, info.height), (WIDTH, HEIGHT));
        assert!((info.fps - FPS).abs() < 0.1, "원본 FPS: {}", info.fps);
        assert!((400..=600).contains(&info.duration_ms), "원본 길이: {}ms", info.duration_ms);
        import::load_video(&name, 0, info.duration_ms, FPS, info.width, info.height, MAX_BYTES, &quiet).unwrap()
    } else {
        import::load_files(&[name], 100, Fit::Contain, MAX_BYTES, &quiet).unwrap()
    }
}

fn letterboxed(frame: &Frame) -> Frame {
    assert_eq!((frame.width, frame.height), (WIDTH, HEIGHT));
    let rgba = frame.rgba();
    let mut canvas = vec![0; (BASE_SIZE.0 * BASE_SIZE.1 * 4) as usize];
    // 테스트 그림과 밑바탕의 가로는 같으므로 확대 없이 위아래에 16픽셀씩 남는다.
    let offset = (16 * BASE_SIZE.0 * 4) as usize;
    canvas[offset..offset + rgba.len()].copy_from_slice(&rgba);
    Frame::from_rgba(BASE_SIZE.0, BASE_SIZE.1, &canvas, frame.delay_ms)
}

fn append_fixture(path: &Path, imported: &[Frame], later: Option<&Path>) -> Project {
    let mut project = Project::default();
    let base = base_frame();
    project.load(vec![base.clone()]);
    let started = project.generation;
    let first = &project.frames[0];
    let size = (first.width, first.height);
    let mut paths = vec![path_text(path)];
    if let Some(later) = later {
        paths.push(path_text(later));
    }
    let (frames, skipped, warning) = collect_append_files(&paths, MAX_BYTES, |_, path, left| {
        load_for_append(path, size, FPS, left, &quiet)
    });
    assert!(skipped.is_empty(), "붙일 수 있는 파일을 건너뛰었습니다");
    if later.is_some() {
        assert!(warning.as_deref().unwrap().starts_with(&format!("{}: ", file_name(&paths[0]))),
            "파일 이름이 붙은 경고가 아닙니다: {warning:?}");
    } else {
        assert!(warning.is_none(), "정상 파일의 붙이기 경고: {warning:?}");
    }
    let mut expected: Vec<Frame> = imported.iter().map(letterboxed).collect();
    if let Some(later) = later {
        expected.extend(import_fixture(later).frames.iter().map(letterboxed));
    }
    assert_frames(&frames, &expected, 0, true);
    check_append_generation(started, project.generation).unwrap();
    let count = frames.len();
    assert_eq!(project.append(frames), Some((1..=count).collect()));
    let mut all = vec![base];
    all.extend(expected);
    assert_frames(&project.frames, &all, 0, true);
    project
}

fn options(format: Format) -> ExportOptions {
    ExportOptions {
        format,
        quality: 90,
        limit_fps: None,
        skip_similar: None,
        skip_similar_time_limit_ms: None,
        resize_percent: 100,
        embed_progress: false,
        protect_colors: vec![],
        gif_fit_50fps: false,
    }
}

fn check_animation(decoded: Vec<image::Frame>, expected: &[Frame], tolerance: u8, label: &str) {
    assert_eq!(decoded.len(), expected.len(), "{label}: 장수");
    for (i, (actual, expected)) in decoded.iter().zip(expected).enumerate() {
        let (numer, denom) = actual.delay().numer_denom_ms();
        assert_eq!(u64::from(numer), u64::from(expected.delay_ms) * u64::from(denom),
            "{label}: {i}번 프레임 시간");
        assert_samples(actual.buffer(), expected, tolerance, &format!("{label} {i}번"));
    }
}

fn check_gif(path: &Path, expected: &[Frame]) {
    let decoded = export::composite_gif(&std::fs::read(path).unwrap());
    assert_eq!(decoded.len(), expected.len(), "GIF 장수");
    let (mut end_ms, mut written_cs) = (0u64, 0u64);
    for (i, (actual, expected)) in decoded.iter().zip(expected).enumerate() {
        assert_eq!(actual.size, (expected.width as u16, expected.height as u16));
        // 누적 끝 시각을 가장 가까운 10ms로 맞추되, GIF 한 장은 최소 20ms다.
        end_ms += u64::from(expected.delay_ms);
        let delay_cs = ((end_ms + 5) / 10).saturating_sub(written_cs).clamp(2, u16::MAX as u64);
        assert_eq!(u64::from(actual.delay_cs), delay_cs, "GIF {i}번 프레임 시간");
        written_cs += delay_cs;
        let image = RgbaImage::from_raw(expected.width, expected.height, actual.canvas.clone()).unwrap();
        // 처분 방법으로 지운 캔버스를 비교하므로, 앞 장의 불투명 픽셀이 남으면 실패한다.
        assert_samples(&image, expected, PALETTE_TOLERANCE, &format!("GIF {i}번"));
    }
}

fn check_video(ffmpeg: &Path, path: &Path, format: Format, expected: &[Frame], settings: &Settings) {
    let name = path_text(path);
    let info = import::probe_video(&name).unwrap();
    assert_eq!((info.width, info.height), BASE_SIZE);
    assert!((info.fps - FPS).abs() < 0.1, "저장한 동영상 FPS: {}", info.fps);
    let total_ms: u64 = expected.iter().map(|f| u64::from(f.delay_ms)).sum();
    // 10 FPS 동영상은 마지막 100ms보다 짧은 시간을 버릴 수 있다. 컨테이너 반올림도 허용한다.
    const VIDEO_TIME_TOLERANCE_MS: u64 = 101;
    assert!(info.duration_ms.abs_diff(total_ms) <= VIDEO_TIME_TOLERANCE_MS,
        "동영상 길이 {}ms, 기대 {total_ms}ms", info.duration_ms);
    let mut args = vec!["-v", "error"];
    if format == Format::Webm {
        // ffmpeg 기본 VP9 디코더는 알파를 버린다. libvpx로 읽어 저장된 알파도 확인한다.
        args.extend(["-c:v", "libvpx-vp9"]);
    }
    args.extend(["-i", &name, "-an", "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
    let out = ffmpeg_ok(ffmpeg, &args);
    let frame_bytes = (BASE_SIZE.0 * BASE_SIZE.1 * 4) as usize;
    assert_eq!(out.stdout.len() % frame_bytes, 0, "동영상의 끝 프레임이 잘렸습니다");
    let count = out.stdout.len() / frame_bytes;
    assert_eq!(count as u64, total_ms / 100, "10 FPS로 저장한 동영상 장수");
    for (i, rgba) in out.stdout.chunks_exact(frame_bytes).enumerate() {
        // 이 테스트의 100~130ms 프레임은 각 출력 구간의 가운데 시각으로 구분할 수 있다.
        let at_ms = i as u64 * 100 + 50;
        let mut end_ms = 0;
        let source = expected.iter().find(|f| {
            end_ms += u64::from(f.delay_ms);
            at_ms < end_ms
        }).unwrap();
        let source = if format == Format::Mp4 {
            let mut px = source.rgba();
            let color = if settings.mp4_replace_transparent {
                let bgr = settings.transparent_replace_color;
                [(bgr & 255) as u8, ((bgr >> 8) & 255) as u8, ((bgr >> 16) & 255) as u8]
            } else {
                [0, 0, 0]
            };
            for p in px.chunks_exact_mut(4) {
                let alpha = u32::from(p[3]);
                for c in 0..3 {
                    p[c] = ((u32::from(p[c]) * alpha + u32::from(color[c]) * (255 - alpha)) / 255) as u8;
                }
                p[3] = 255;
            }
            Frame::from_rgba(source.width, source.height, &px, 100)
        } else {
            source.clone()
        };
        let image = RgbaImage::from_raw(BASE_SIZE.0, BASE_SIZE.1, rgba.to_vec()).unwrap();
        assert_samples(&image, &source, LOSSY_TOLERANCE, &format!("{format:?} {i}번"));
    }
}

fn export_and_check(dir: &TempDir, project: &Project, ffmpeg: Option<&Path>) {
    let settings = Settings::default();
    for format in [Format::Gif, Format::Webp, Format::Apng, Format::Mp4, Format::Webm, Format::Png] {
        if matches!(format, Format::Mp4 | Format::Webm) && ffmpeg.is_none() {
            continue;
        }
        let opt = options(format);
        // 가공을 끈 옵션으로 장수·시간·픽셀이 모두 유지되는지 먼저 확인한다.
        assert_frames(&export::prepare(&project.frames, &opt, &settings), &project.frames, 0, true);
        let path = dir.path(&format!("saved_{format:?}.{}", format.ext()));
        let paths = export::export(&project.frames, &path, &opt, &settings, &quiet).unwrap();
        if format != Format::Png {
            assert_eq!(paths, vec![path.clone()], "저장 결과 경로");
        }
        match format {
            Format::Gif => check_gif(&paths[0], &project.frames),
            Format::Webp => {
                let decoder = image::codecs::webp::WebPDecoder::new(
                    BufReader::new(std::fs::File::open(&paths[0]).unwrap()),
                ).unwrap();
                assert_eq!(decoder.dimensions(), BASE_SIZE);
                assert!(decoder.has_animation());
                check_animation(decoder.into_frames().collect_frames().unwrap(), &project.frames,
                    LOSSY_TOLERANCE, "WebP");
            }
            Format::Apng => {
                let decoder = image::codecs::png::PngDecoder::new(
                    BufReader::new(std::fs::File::open(&paths[0]).unwrap()),
                ).unwrap();
                assert_eq!(decoder.dimensions(), BASE_SIZE);
                assert!(decoder.is_apng().unwrap());
                check_animation(decoder.apng().unwrap().into_frames().collect_frames().unwrap(),
                    &project.frames, PALETTE_TOLERANCE, "APNG");
            }
            Format::Mp4 | Format::Webm => check_video(ffmpeg.unwrap(), &paths[0], format, &project.frames, &settings),
            Format::Png => {
                assert_eq!(paths.len(), project.frames.len(), "PNG 연속 저장 장수");
                for (i, (path, expected)) in paths.iter().zip(&project.frames).enumerate() {
                    assert_eq!(path.file_name().unwrap().to_string_lossy(), format!("saved_Png_{:04}.png", i + 1));
                    let image = image::open(path).unwrap().to_rgba8();
                    assert_eq!(image.dimensions(), BASE_SIZE);
                    // PNG 연속 이미지는 시간 정보가 없고, 픽셀은 숨은 RGB까지 그대로여야 한다.
                    assert_eq!(image.as_raw(), &expected.rgba(), "PNG {i}번 전체 픽셀");
                }
            }
            Format::Avif => unreachable!(),
        }
    }
}

fn image_flow(make: impl FnOnce(&TempDir) -> (PathBuf, Vec<Frame>)) {
    let dir = TempDir::new();
    let ffmpeg = ffmpeg_for_tests();
    let (path, expected) = make(&dir);
    let loaded = import_fixture(&path);
    assert!(loaded.warning.is_none());
    assert!(!loaded.memory_limited);
    assert_frames(&loaded.frames, &expected, 0, true);
    let project = append_fixture(&path, &loaded.frames, None);
    export_and_check(&dir, &project, ffmpeg.as_deref());
}

fn video_flow(kind: VideoFixture) {
    let Some(ffmpeg) = ffmpeg_for_tests() else { return };
    let dir = TempDir::new();
    let path = video_fixture(&dir, &ffmpeg, kind);
    let loaded = import_fixture(&path);
    assert!(!loaded.memory_limited);
    let corrupt = matches!(kind, VideoFixture::CorruptAvi);
    if corrupt {
        assert!((1..5).contains(&loaded.frames.len()), "앞부분만 불러와야 합니다");
        assert!(loaded.warning.is_some(), "손상 동영상의 부분 읽기 경고가 없습니다");
    } else {
        assert_eq!(loaded.frames.len(), 5);
        assert!(loaded.warning.is_none(), "정상 동영상 경고: {:?}", loaded.warning);
    }
    let expected: Vec<Frame> = (0..loaded.frames.len()).map(|i| {
        frame(&blocks(if i % 2 == 0 { RED } else { GREEN }, BLUE), 100)
    }).collect();
    assert_frames(&loaded.frames, &expected, INPUT_VIDEO_TOLERANCE, false);
    let later = corrupt.then(|| png_fixture(&dir, true).0);
    let project = append_fixture(&path, &loaded.frames, later.as_deref());
    export_and_check(&dir, &project, Some(&ffmpeg));
}

#[test]
fn png_import_append_export_all_formats() {
    image_flow(|dir| png_fixture(dir, false));
}

#[test]
fn transparent_png_import_append_export_all_formats() {
    image_flow(|dir| png_fixture(dir, true));
}

#[test]
fn transparent_gif_import_append_export_all_formats() {
    image_flow(gif_fixture);
}

#[test]
fn mp4_import_append_export_all_formats() {
    video_flow(VideoFixture::Mp4);
}

#[test]
fn durationless_webm_import_append_export_all_formats() {
    video_flow(VideoFixture::NoDurationWebm);
}

#[test]
fn corrupt_video_warns_and_appends_next_file_then_exports_all_formats() {
    video_flow(VideoFixture::CorruptAvi);
}

#[test]
fn append_permit_and_generation_reject_overlaps_and_reloaded_project() {
    let dir = TempDir::new();
    let (path, _) = png_fixture(&dir, false);
    let busy = AtomicBool::new(false);
    let permit = AppendPermit::acquire(&busy).unwrap();
    assert!(AppendPermit::acquire(&busy).is_err());
    let mut project = Project::default();
    project.load(vec![base_frame()]);
    let started = project.generation;
    let (frames, skipped, warning) = collect_append_files(&[path_text(&path)], MAX_BYTES, |_, path, left| {
        load_for_append(path, BASE_SIZE, FPS, left, &quiet)
    });
    assert_eq!(frames.len(), 1);
    assert!(skipped.is_empty() && warning.is_none());
    assert!(check_append_generation(started, project.generation).is_ok());
    project.load(vec![base_frame().with_delay(200)]);
    assert!(check_append_generation(started, project.generation).is_err());
    // 옛 결과를 버린 뒤에도 새로 연 작업은 그대로여야 한다.
    assert_eq!(project.frames.len(), 1);
    assert_eq!(project.frames[0].delay_ms, 200);
    drop(permit);
    let next = AppendPermit::acquire(&busy).unwrap();
    drop(next);
    assert!(!busy.load(Ordering::SeqCst));
}

#[test]
fn append_memory_limit_keeps_first_file_and_skips_later_files() {
    let dir = TempDir::new();
    let (first, _) = gif_fixture(&dir);
    let (second, _) = png_fixture(&dir, false);
    let paths = vec![path_text(&first), path_text(&second)];
    let (frames, skipped, warning) = collect_append_files(&paths, 1, |_, path, left| {
        let loaded = load_for_append(path, BASE_SIZE, FPS, left, &quiet)?;
        assert!(loaded.memory_limited, "1바이트 한도인데 memory_limited가 꺼져 있습니다");
        Ok(loaded)
    });
    assert_eq!(frames.len(), 1);
    assert_eq!((frames[0].width, frames[0].height), BASE_SIZE);
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].name, file_name(&paths[1]));
    assert_eq!(skipped[0].reason, "편집 메모리 한도에 닿았습니다");
    assert!(warning.unwrap().starts_with(&format!("{}: ", file_name(&paths[0]))));
    let mut project = Project::default();
    project.load(vec![base_frame()]);
    let started = project.generation;
    check_append_generation(started, project.generation).unwrap();
    assert_eq!(project.append(frames), Some(vec![1]));
    assert_eq!(project.frames.len(), 2);
}

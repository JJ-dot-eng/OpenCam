//! 녹화 스레드. FPS 간격으로 화면을 받아 프레임 목록에 쌓는다.
//!
//! - "화면 변화 없으면 프레임 합치기": 새 화면이 직전과 완전히 같으면 직전 프레임 시간만 늘린다
//! - 프레임 시간은 실제로 흐른 시간으로 정한다(캡처가 밀려도 재생 속도가 실제와 같게)

use crate::capture::{Capturer, CursorOptions, Rect};
use crate::export::Mp4Stream;
use crate::frame::Frame;
use parking_lot::Mutex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct RecordOptions {
    pub region: Rect,
    pub fps: u32,
    pub skip_same_frame: bool,
    pub max_ms: Option<u64>,
    pub max_bytes: usize,
    pub cursor: CursorOptions,
    /// 녹화하면서 MP4로 바로 저장
    pub direct: Option<Mp4Stream>,
    /// 바로 저장 중 메모리 한도에 닿으면 편집용 프레임만 그만 모으고 녹화는 계속
    pub drop_frames_after_limit: bool,
}

/// 녹화 결과
pub struct RecordResult {
    pub frames: Vec<Frame>,
    /// MP4 바로 저장 결과 (저장한 파일 또는 오류)
    pub direct: Option<Result<PathBuf, String>>,
    /// 메모리 한도 때문에 편집용 프레임을 중간부터 버렸으면 true
    pub dropped: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecStatus {
    pub recording: bool,
    pub paused: bool,
    pub frames: usize,
    pub elapsed_ms: u64,
    pub memory_mb: u64,
    pub stop_reason: Option<String>,
}

struct Shared {
    frames: Mutex<Vec<Frame>>,
    latest: Mutex<Option<(u32, u32, Arc<Vec<u8>>)>>,
    status: Mutex<RecStatus>,
    stop: AtomicBool,
    paused: AtomicBool,
    region: Mutex<Option<Rect>>,
    direct_result: Mutex<Option<Result<PathBuf, String>>>,
    dropped: AtomicBool,
}

pub struct Recorder {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn start(opts: RecordOptions, on_status: impl Fn(RecStatus) + Send + 'static) -> anyhow::Result<Self> {
        // 캡처 준비가 실패하면 바로 알려준다
        let capturer = Capturer::new(opts.region)?;
        let shared = Arc::new(Shared {
            frames: Mutex::new(vec![]),
            latest: Mutex::new(None),
            status: Mutex::new(RecStatus {
                recording: true,
                paused: false,
                frames: 0,
                elapsed_ms: 0,
                memory_mb: 0,
                stop_reason: None,
            }),
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            region: Mutex::new(None),
            direct_result: Mutex::new(None),
            dropped: AtomicBool::new(false),
        });
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("opencam-recorder".into())
            .spawn(move || run(capturer, opts, s, on_status))?;
        Ok(Recorder { shared, thread: Some(thread) })
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::SeqCst)
    }

    /// 녹화 중 녹화 틀이 움직였을 때
    pub fn move_region(&self, region: Rect) {
        *self.shared.region.lock() = Some(region);
    }

    pub fn status(&self) -> RecStatus {
        self.shared.status.lock().clone()
    }

    /// 편집 프레임 수집을 중단했더라도 마지막으로 캡처한 화면을 반환한다.
    pub fn latest_image(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        self.shared.latest.lock().clone()
    }

    /// 녹화를 끝내고 결과를 돌려준다
    pub fn stop(mut self) -> RecordResult {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        RecordResult {
            frames: std::mem::take(&mut *self.shared.frames.lock()),
            direct: self.shared.direct_result.lock().take(),
            dropped: self.shared.dropped.load(Ordering::SeqCst),
        }
    }
}

/// 직전 프레임이 화면에 보인 시간만큼 delay를 늘린다
fn extend_last(s: &Shared, dt: Duration) {
    if let Some(prev) = s.frames.lock().last_mut() {
        prev.delay_ms += dt.as_millis() as u32;
    }
}

fn run(mut cap: Capturer, mut opts: RecordOptions, s: Arc<Shared>, on_status: impl Fn(RecStatus)) {
    let fps = opts.fps.clamp(1, 100);
    let interval = Duration::from_secs_f64(1.0 / fps as f64);
    let mut next = Instant::now();
    let mut last_px: Option<Arc<Vec<u8>>> = None;
    let direct = opts.direct.take();
    // MP4에 넣은 프레임 수. 녹화 시간 × FPS만큼 되도록 맞춰 넣는다.
    let mut written = 0u64;
    // 메모리 한도 뒤로는 편집용 프레임을 모으지 않는다 (바로 저장일 때)
    let mut keep = true;
    // 마지막 캡처 시각. 그 뒤로 흐른 시간은 마지막 프레임이 보인 시간이다.
    let mut last_tick: Option<Instant> = None;
    let mut recorded = Duration::ZERO; // 일시정지 시간을 뺀 녹화 시간
    let mut bytes = 0usize;
    let mut last_report = Instant::now() - Duration::from_secs(1);
    let mut stop_reason = None;

    while !s.stop.load(Ordering::SeqCst) {
        if let Some(r) = s.region.lock().take()
            && r != cap.region()
            && let Err(e) = cap.set_region(r)
        {
            stop_reason = Some(format!("녹화 영역 변경 실패: {e}"));
            break;
        }

        if s.paused.load(Ordering::SeqCst) {
            // 일시정지 직전까지의 시간만 넣고, 쉬는 시간은 프레임 시간에 넣지 않는다
            if let Some(t) = last_tick.take() {
                let dt = t.elapsed();
                extend_last(&s, dt);
                recorded += dt;
            }
            std::thread::sleep(Duration::from_millis(15));
            next = Instant::now();
            report(&s, &on_status, &mut last_report, true, recorded, bytes);
            continue;
        }

        let now = Instant::now();
        if now < next {
            std::thread::sleep((next - now).min(Duration::from_millis(5)));
            continue;
        }
        next += interval;
        if next < now {
            next = now + interval; // 밀렸으면 따라잡지 말고 다음 칸부터
        }

        let px = match cap.grab(&opts.cursor) {
            Ok(px) => Arc::new(px),
            Err(e) => {
                stop_reason = Some(format!("화면 캡처 실패: {e}"));
                break;
            }
        };
        let r = cap.region();
        *s.latest.lock() = Some((r.w as u32, r.h as u32, px.clone()));
        if let Some(t) = last_tick.replace(now) {
            let dt = now.duration_since(t);
            extend_last(&s, dt);
            recorded += dt;
        }
        if let Some(d) = &direct {
            let target = (recorded.as_secs_f64() * fps as f64).round() as u64 + 1;
            d.push(px.clone(), target.saturating_sub(written) as u32);
            written = written.max(target);
        }
        let same = opts.skip_same_frame && last_px.as_ref().is_some_and(|l| **l == *px);
        if !same && keep {
            // 새 프레임은 시간 0에서 시작해서, 다음 캡처까지 흐른 시간만큼 늘어난다
            let f = Frame::from_rgba(r.w as u32, r.h as u32, &px, 0);
            bytes += f.packed_bytes();
            s.frames.lock().push(f);
        }
        if !same {
            last_px = Some(px);
        }

        if let Some(max) = opts.max_ms
            && recorded.as_millis() as u64 >= max
        {
            stop_reason = Some("최대 녹화 시간이 지나 녹화를 멈췄습니다".into());
            break;
        }
        if keep && bytes >= opts.max_bytes {
            if direct.is_some() && opts.drop_frames_after_limit {
                // MP4에는 계속 저장하고, 편집용 프레임만 더 모으지 않는다
                keep = false;
                s.dropped.store(true, Ordering::SeqCst);
            } else {
                stop_reason =
                    Some("메모리가 부족해 녹화를 멈췄습니다 (설정에서 편집 메모리 한도를 바꿀 수 있습니다)".into());
                break;
            }
        }
        report(&s, &on_status, &mut last_report, false, recorded, bytes);
    }

    // 마지막 프레임: 멈춘 시점까지 보인 시간, 최소 한 칸
    if let Some(t) = last_tick {
        let dt = t.elapsed();
        if keep {
            extend_last(&s, dt);
        }
        recorded += dt;
    }
    if let Some(d) = direct {
        // 마지막 화면을 멈춘 시점까지 채우고 파일을 마무리한다
        if let Some(px) = &last_px {
            let target = (recorded.as_secs_f64() * fps as f64).round() as u64;
            d.push(px.clone(), target.saturating_sub(written) as u32);
        }
        *s.direct_result.lock() = Some(d.finish().map_err(|e| format!("{e:#}")));
    }
    if let Some(last) = s.frames.lock().last_mut() {
        last.delay_ms = last.delay_ms.max(interval.as_millis() as u32);
    }

    let mut st = s.status.lock();
    st.recording = false;
    st.paused = false;
    st.stop_reason = stop_reason;
    st.frames = s.frames.lock().len();
    on_status(st.clone());
}

fn report(
    s: &Shared,
    on_status: &impl Fn(RecStatus),
    last: &mut Instant,
    paused: bool,
    recorded: Duration,
    bytes: usize,
) {
    if last.elapsed() < Duration::from_millis(200) {
        return;
    }
    *last = Instant::now();
    let st = {
        let mut st = s.status.lock();
        st.paused = paused;
        st.frames = s.frames.lock().len();
        st.elapsed_ms = recorded.as_millis() as u64;
        st.memory_mb = (bytes / (1024 * 1024)) as u64;
        st.clone()
    };
    on_status(st);
}

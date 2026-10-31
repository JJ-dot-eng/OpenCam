//! OpenCam 본체: 창 두 개(녹화 틀 "recorder", 편집 "main")와 화면에서 부르는 명령들.

mod capture;
mod collage;
mod effects;
mod export;
mod filters;
mod frame;
mod import;
mod magnifier;
mod project;
mod recorder;
mod recovery;
mod settings;
mod superres;
mod window_picker;

use capture::{CursorOptions, Rect};
use effects::{Anim, Filter, Overlay, TransitionKind, ZoomFx};
use export::ExportOptions;
use parking_lot::Mutex;
use project::{FrameInfo, Project, ReduceMode, Rotate, SummaryOptions, SummaryPreview};
use rayon::prelude::*;
use recorder::{RecStatus, RecordOptions, Recorder};
use serde::{Deserialize, Serialize};
use settings::Settings;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

struct AppState {
    settings: Mutex<Settings>,
    project: Mutex<Project>,
    append_busy: AtomicBool,
    recorder: Mutex<Option<Recorder>>,
    /// 녹화 틀 창 안에서 뚫린 구멍의 위치 (물리 픽셀, 창 왼쪽 위 기준)
    hole: Mutex<Rect>,
    full_capture: Mutex<Option<Rect>>,
    /// 등록된 단축키 (녹화/정지, 일시정지)
    hotkeys: Mutex<(Option<Shortcut>, Option<Shortcut>)>,
    ctrl_prtsc_hotkey: Mutex<Option<Shortcut>>,
    /// 시작할 때 생긴 문제(단축키 등록 실패 등). 녹화 틀이 뜨면 한 번 보여 준다.
    startup_notice: Mutex<Option<String>>,
    snapshot_hotkey: Mutex<bool>,
    /// 작업 자동 저장 파일을 쓰는 동안 잡는다. true면 프로그램을 끝내는 중이라 더 쓰지 않는다.
    recovery_io: Mutex<bool>,
    /// 쓰고 있는 자동 저장을 그만두라는 신호. 끝내기·녹화 틀로 돌아가기가 큰 저장을 끝까지 기다리지 않게 한다.
    recovery_stop: std::sync::atomic::AtomicBool,
}

type CmdResult<T> = Result<T, String>;

/// 오류·취소로 끝나도 붙이기 처리 중 표시를 푼다. 프로젝트 잠금은 기다리는 동안 잡지 않는다.
struct AppendPermit<'a>(&'a AtomicBool);

impl<'a> AppendPermit<'a> {
    fn acquire(busy: &'a AtomicBool) -> CmdResult<Self> {
        busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "파일을 붙이는 중입니다. 끝난 뒤 다시 해 주세요".to_string())?;
        Ok(Self(busy))
    }
}

impl Drop for AppendPermit<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn check_append_generation(started: u64, current: u64) -> CmdResult<()> {
    if started == current {
        Ok(())
    } else {
        Err("붙이는 동안 다른 작업으로 바뀌어 붙이지 않았습니다".into())
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn recorder_window(app: &AppHandle) -> CmdResult<WebviewWindow> {
    app.get_webview_window("recorder").ok_or_else(|| "녹화 창이 없습니다".into())
}

// ---------- 녹화 틀 ----------

/// 화면 좌표로 본 녹화 영역 (창 위치 + 구멍 위치)
fn capture_rect(app: &AppHandle) -> CmdResult<Rect> {
    if let Some(full) = *app.state::<AppState>().full_capture.lock() {
        return Ok(full);
    }
    let w = recorder_window(app)?;
    let pos = w.inner_position().map_err(err)?;
    let hole = *app.state::<AppState>().hole.lock();
    if hole.w < 1 || hole.h < 1 {
        return Err("녹화 영역 크기가 0입니다".into());
    }
    Ok(Rect { x: pos.x + hole.x, y: pos.y + hole.y, ..hole })
}

/// 창에 구멍을 뚫는다. 구멍 부분은 마우스 클릭도 아래 창으로 그대로 넘어간다.
#[cfg(windows)]
/// keep: 창에서 남길 부분(툴바, 틀). 비어 있으면 창 전체를 남긴다.
fn apply_hole(w: &WebviewWindow, hole: Rect, keep: &[Rect]) -> CmdResult<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, RGN_DIFF, RGN_OR, SetWindowRgn};
    let hwnd = HWND(w.hwnd().map_err(err)?.0);
    // 창 테두리(비클라이언트) 두께만큼 구멍을 옮긴다. 테두리 없는 창이면 0.
    let inner = w.inner_position().map_err(err)?;
    let outer = w.outer_position().map_err(err)?;
    let (ox, oy) = (inner.x - outer.x, inner.y - outer.y);
    unsafe {
        let all = if keep.is_empty() {
            CreateRectRgn(0, 0, 30000, 30000)
        } else {
            // 틀이 툴바보다 좁으면 창 오른쪽에 남는 곳이 생긴다. 그곳도 잘라내 아래가 보이고 클릭되게 한다.
            let all = CreateRectRgn(0, 0, 0, 0);
            for r in keep {
                let part = CreateRectRgn(ox + r.x, oy + r.y, ox + r.x + r.w, oy + r.y + r.h);
                CombineRgn(Some(all), Some(all), Some(part), RGN_OR);
                let _ = DeleteObject(part.into());
            }
            all
        };
        let hole_rgn = CreateRectRgn(ox + hole.x, oy + hole.y, ox + hole.x + hole.w, oy + hole.y + hole.h);
        CombineRgn(Some(all), Some(all), Some(hole_rgn), RGN_DIFF);
        let _ = DeleteObject(hole_rgn.into());
        // 성공하면 영역은 시스템 것이 되므로 지우지 않는다
        if SetWindowRgn(hwnd, Some(all), true) == 0 {
            let _ = DeleteObject(all.into());
        }
    }
    Ok(())
}

#[tauri::command]
fn set_hole(
    app: AppHandle,
    state: State<AppState>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    keep: Option<Vec<Rect>>,
) -> CmdResult<()> {
    let hole = Rect { x, y, w: w.max(1), h: h.max(1) };
    *state.hole.lock() = hole;
    apply_hole(&recorder_window(&app)?, hole, &keep.unwrap_or_default())
}

#[tauri::command]
fn monitors() -> CmdResult<Vec<Rect>> {
    capture::monitors().map_err(err)
}

/// OS가 크기 조절 드래그를 처리하는 동안에도 마우스 버튼의 현재 상태를 확인한다.
#[tauri::command]
fn resize_mouse_down(window: WebviewWindow) -> CmdResult<bool> {
    if window.label() != "recorder" { return Err("녹화 창에서만 사용할 수 있습니다".into()); }
    Ok(magnifier::primary_button_down())
}

/// 시작할 때 생긴 문제를 한 번 꺼내 간다 (녹화 틀이 뜰 때 알림으로 보여 준다)
#[tauri::command]
fn take_startup_notice(state: State<AppState>) -> Option<String> {
    state.startup_notice.lock().take()
}

/// 녹화 틀 크기 조절을 시작할 때 부른다. 주 마우스 버튼을 떼면 돋보기가 스스로 닫힌다.
#[tauri::command]
fn start_magnifier(window: WebviewWindow, scale: f64) -> CmdResult<()> {
    if window.label() != "recorder" { return Err("녹화 창에서만 사용할 수 있습니다".into()); }
    magnifier::start(scale);
    Ok(())
}

#[tauri::command]
fn get_full_capture(state: State<AppState>) -> Option<Rect> {
    *state.full_capture.lock()
}

#[tauri::command]
fn set_full_capture(app: AppHandle, window: WebviewWindow, state: State<AppState>, monitor: Option<Rect>) -> CmdResult<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowDisplayAffinity, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE};
    if window.label() != "recorder" { return Err("녹화 창에서만 전체 화면을 설정할 수 있습니다".into()); }
    let recording = state.recorder.lock();
    if recording.is_some() { return Err("녹화 중에는 영역을 변경할 수 없습니다".into()); }
    if let Some(r) = monitor {
        if !capture::monitors().map_err(err)?.contains(&r) { return Err("선택한 모니터를 찾을 수 없습니다".into()); }
    }
    let hwnd = HWND(recorder_window(&app)?.hwnd().map_err(err)?.0);
    unsafe {
        let previous = if state.full_capture.lock().is_some() { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
        let affinity = if monitor.is_some() { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
        SetWindowDisplayAffinity(hwnd, affinity).map_err(err)?;
        let mut actual = 0u32;
        if let Err(e) = GetWindowDisplayAffinity(hwnd, &mut actual) {
            let _ = SetWindowDisplayAffinity(hwnd, previous);
            return Err(format!("캡처 제외 상태 확인 실패: {e}"));
        }
        if actual != affinity.0 {
            let _ = SetWindowDisplayAffinity(hwnd, previous);
            return Err("이 Windows에서는 녹화 틀을 캡처에서 제외할 수 없습니다".into());
        }
    }
    *state.full_capture.lock() = monitor;
    Ok(())
}

#[tauri::command]
fn capture_windows() -> CmdResult<Vec<window_picker::CaptureWindow>> {
    window_picker::list()
}

#[tauri::command]
fn capture_window_bounds(id: String, pid: u32) -> CmdResult<window_picker::SelectedWindow> {
    window_picker::select(&id, pid)
}

fn virtual_desktop() -> CmdResult<Rect> {
    let monitors = capture::monitors().map_err(err)?;
    let first = monitors.first().ok_or("모니터를 찾을 수 없습니다")?;
    let (mut x1, mut y1) = (i64::from(first.x), i64::from(first.y));
    let (mut x2, mut y2) = (x1 + i64::from(first.w), y1 + i64::from(first.h));
    for m in &monitors[1..] {
        x1 = x1.min(i64::from(m.x));
        y1 = y1.min(i64::from(m.y));
        x2 = x2.max(i64::from(m.x) + i64::from(m.w));
        y2 = y2.max(i64::from(m.y) + i64::from(m.h));
    }
    let cv = |n: i64| i32::try_from(n).map_err(|_| "화면 크기가 너무 큽니다".to_string());
    Ok(Rect { x: cv(x1)?, y: cv(y1)?, w: cv(x2 - x1)?, h: cv(y2 - y1)? })
}

#[tauri::command]
fn begin_area_selection(app: AppHandle, window: WebviewWindow, state: State<AppState>) -> CmdResult<()> {
    if window.label() != "recorder" { return Err("녹화 창에서만 영역을 선택할 수 있습니다".into()); }
    if state.recorder.lock().is_some() { return Err("녹화 중에는 영역을 선택할 수 없습니다".into()); }
    let r = virtual_desktop()?;
    let selector = app.get_webview_window("selector").ok_or("영역 선택 창이 없습니다")?;
    let recorder = recorder_window(&app)?;
    let run = || -> CmdResult<()> {
        selector.set_position(tauri::PhysicalPosition::new(r.x, r.y)).map_err(err)?;
        selector.set_size(tauri::PhysicalSize::new(r.w as u32, r.h as u32)).map_err(err)?;
        let pos = selector.inner_position().map_err(err)?;
        let size = selector.inner_size().map_err(err)?;
        if pos.x != r.x || pos.y != r.y || size.width != r.w as u32 || size.height != r.h as u32 {
            return Err("화면 전체에 선택 창을 표시할 수 없습니다. 모니터 배치와 배율을 확인하세요".into());
        }
        recorder.hide().map_err(err)?;
        selector.show().map_err(err)?;
        selector.set_focus().map_err(err)?;
        Ok(())
    };
    if let Err(e) = run() {
        let _ = selector.hide();
        let _ = recorder.show();
        let _ = recorder.set_focus();
        return Err(e);
    }
    Ok(())
}

fn restore_after_area(app: &AppHandle, rect: Option<Rect>) {
    if let Some(selector) = app.get_webview_window("selector") { let _ = selector.hide(); }
    if let Ok(recorder) = recorder_window(app) {
        let _ = recorder.show();
        let _ = recorder.set_focus();
        if let Some(r) = rect { let _ = recorder.emit("area-selected", r); }
    }
}

fn area_inside_desktop(r: Rect, screen: Rect) -> bool {
    r.w >= 16 && r.h >= 16
        && i64::from(r.x) >= i64::from(screen.x) && i64::from(r.y) >= i64::from(screen.y)
        && i64::from(r.x) + i64::from(r.w) <= i64::from(screen.x) + i64::from(screen.w)
        && i64::from(r.y) + i64::from(r.h) <= i64::from(screen.y) + i64::from(screen.h)
}

/// 영역이 모니터들로 빈틈없이 덮이는지. 크기가 다른 모니터를 나란히 두면 전체 사각형 안에도
/// 화면이 없는 구석이 생기고, 그곳은 검게 녹화된다. 모니터끼리는 겹치지 않으므로(복제 화면은 같은 사각형)
/// 겹친 넓이의 합이 영역 넓이와 같으면 다 덮인 것이다.
fn area_on_monitors(r: Rect, monitors: &[Rect]) -> bool {
    let (rx1, ry1) = (i64::from(r.x), i64::from(r.y));
    let (rx2, ry2) = (rx1 + i64::from(r.w), ry1 + i64::from(r.h));
    let mut seen: Vec<Rect> = vec![];
    let mut covered = 0i64;
    for m in monitors {
        if seen.contains(m) { continue; }
        seen.push(*m);
        let w = (rx2.min(i64::from(m.x) + i64::from(m.w)) - rx1.max(i64::from(m.x))).max(0);
        let h = (ry2.min(i64::from(m.y) + i64::from(m.h)) - ry1.max(i64::from(m.y))).max(0);
        covered += w * h;
    }
    covered == (rx2 - rx1) * (ry2 - ry1)
}

#[tauri::command]
fn finish_area_selection(app: AppHandle, window: WebviewWindow, rect: Option<Rect>) -> CmdResult<()> {
    if window.label() != "selector" { return Err("영역 선택 창이 아닙니다".into()); }
    if !window.is_visible().map_err(err)? { return Err("영역 선택이 이미 종료되었습니다".into()); }
    let mut problem = "선택한 영역이 너무 작거나 화면 밖입니다";
    let valid = rect.filter(|r| {
        let (Ok(screen), Ok(monitors)) = (virtual_desktop(), capture::monitors()) else { return false; };
        if !area_inside_desktop(*r, screen) { return false; }
        if !area_on_monitors(*r, &monitors) {
            problem = "선택한 영역에 화면이 없는 부분(모니터 사이 빈 곳)이 있습니다";
            return false;
        }
        true
    });
    restore_after_area(&app, valid);
    if rect.is_some() && valid.is_none() {
        let _ = app.emit("notice", problem.to_string());
        return Err(problem.into());
    }
    Ok(())
}

fn start_recording(app: &AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut slot = state.recorder.lock();
    if slot.is_some() {
        return Ok(());
    }
    let region = capture_rect(app)?;
    if state.full_capture.lock().is_some() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
        if !capture::monitors().map_err(err)?.contains(&region) {
            return Err("전체 화면으로 선택한 모니터가 변경됐습니다. 다시 선택해 주세요".into());
        }
        let hwnd = HWND(recorder_window(app)?.hwnd().map_err(err)?.0);
        let mut affinity = 0u32;
        unsafe { GetWindowDisplayAffinity(hwnd, &mut affinity).map_err(err)?; }
        if affinity != WDA_EXCLUDEFROMCAPTURE.0 {
            return Err("녹화 틀의 캡처 제외가 해제됐습니다. 전체 화면을 다시 선택해 주세요".into());
        }
    }
    let s = state.settings.lock().clone();
    // MP4 바로 저장: 저장 폴더에 파일 이름 규칙대로
    let direct = if s.rec_direct_save {
        let name = export::file_name_from_rule(&s.file_name_rule);
        let path = export::unique_path(Path::new(&s.output_dir), &name, "mp4");
        Some(
            export::Mp4Stream::start(
                &path,
                region.w as u32,
                region.h as u32,
                s.fps.clamp(1, 100),
                s.rec_direct_save_quality,
            )
            .map_err(|e| format!("{e:#}"))?,
        )
    } else {
        None
    };
    let no_limit = s.rec_no_time_limit || (direct.is_some() && s.rec_direct_save_no_time_limit);
    let opts = RecordOptions {
        region,
        fps: s.fps.clamp(1, 100),
        skip_same_frame: s.skip_same_frame,
        max_ms: (!no_limit).then(|| s.max_rec_sec.clamp(1, 3600) as u64 * 1000),
        direct,
        drop_frames_after_limit: s.rec_direct_save_drop_frames,
        max_bytes: s.max_edit_memory_mb.max(64) as usize * 1024 * 1024,
        cursor: CursorOptions {
            draw: s.capture_cursor,
            highlight: s.cursor_highlight,
            highlight_color: s.cursor_highlight_color,
            click_fx: s.cursor_click_fx,
            left_color: s.left_click_color,
            right_color: s.right_click_color,
        },
    };
    let handle = app.clone();
    let rec = Recorder::start(opts, move |status: RecStatus| {
        let _ = handle.emit("rec-status", &status);
        if !status.recording {
            // 최대 시간·오류로 스스로 멈춘 경우. 녹화 스레드 안이라 join할 수 없어서 다른 스레드에서 정리한다.
            let h = handle.clone();
            std::thread::spawn(move || finish_recording(&h));
        }
    })
    .map_err(err)?;
    *slot = Some(rec);
    if s.rec_global_snapshot_hotkey {
        if let Err(e) = set_snapshot_hotkey(app, true) {
            let _ = app.emit("notice", format!("Ctrl+S 등록 실패: {e}. 화면 저장 버튼을 사용하세요."));
        }
    }
    drop(slot);
    let _ = recorder_window(app)?.set_resizable(false);
    Ok(())
}

/// 녹화를 끝내고 편집 창으로 넘어간다. 두 번 불려도 한 번만 처리된다.
fn finish_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut slot = state.recorder.lock();
    let Some(rec) = slot.take() else {
        return;
    };
    drop(slot);
    // 단축키 해제는 메인 스레드가 처리한다. 여기서 잠금을 쥔 채 기다리면, 같은 잠금을 기다리는
    // 메인 스레드(단축키·창 이벤트)와 서로 기다리며 멈춘다. 그사이 새 녹화가 시작됐으면 그대로 둔다.
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        if h.state::<AppState>().recorder.lock().is_none() {
            let _ = set_snapshot_hotkey(&h, false);
        }
    });
    let reason = rec.status().stop_reason;
    let result = rec.stop();
    let frames = result.frames;
    // MP4 바로 저장 결과 알리기 (녹화 틀에도 보이게 먼저)
    let mut direct_saved = false;
    match &result.direct {
        Some(Ok(path)) => {
            direct_saved = true;
            let _ = app.emit("notice", format!("MP4로 저장했습니다: {}", path.display()));
            if state.settings.lock().rec_direct_save_open_folder {
                let _ = tauri_plugin_opener::reveal_item_in_dir(path);
            }
        }
        Some(Err(e)) => {
            let _ = app.emit("notice", e.clone());
        }
        None => {}
    }
    if result.dropped {
        let _ =
            app.emit("notice", "메모리 한도에 닿아 편집 창에는 앞부분만 있습니다 (MP4에는 전부 저장됨)".to_string());
    }
    if let Ok(w) = recorder_window(app) {
        let _ = w.set_resizable(true);
    }
    let _ = app.emit(
        "rec-status",
        RecStatus {
            recording: false,
            paused: false,
            frames: frames.len(),
            elapsed_ms: 0,
            memory_mb: 0,
            stop_reason: reason.clone(),
        },
    );
    if frames.is_empty() {
        return;
    }
    {
        let mut p = state.project.lock();
        p.set_undo_enabled(state.settings.lock().enable_undo);
        p.load(frames);
        // 녹화는 아직 어디에도 저장되지 않았다 (버릴 때 묻기 위해). 바로 저장했으면 이미 저장됨.
        p.dirty = !direct_saved;
    }
    show_editor(app);
    if !direct_saved {
        let _ = app.emit("recorded", ());
    }
    if let Some(r) = reason {
        let _ = app.emit("notice", r);
    }
}

fn show_editor(app: &AppHandle) {
    if let Ok(r) = recorder_window(app) {
        let _ = r.hide();
    }
    if let Some(m) = app.get_webview_window("main") {
        let _ = m.emit("project-changed", ());
        let _ = m.show();
        let _ = m.unminimize();
        let _ = m.set_focus();
    }
}

#[tauri::command]
fn rec_start(app: AppHandle) -> CmdResult<()> {
    start_recording(&app)
}

#[tauri::command]
fn rec_stop(app: AppHandle) {
    std::thread::spawn(move || finish_recording(&app));
}

#[tauri::command]
fn rec_pause(state: State<AppState>) -> bool {
    match state.recorder.lock().as_ref() {
        Some(r) => {
            r.set_paused(!r.is_paused());
            r.is_paused()
        }
        None => false,
    }
}

#[tauri::command]
fn rec_status(state: State<AppState>) -> Option<RecStatus> {
    state.recorder.lock().as_ref().map(|r| r.status())
}

#[tauri::command]
fn rec_snapshot_global_active(state: State<AppState>) -> bool {
    *state.snapshot_hotkey.lock()
}

/// 녹화 중 마지막으로 캡처한 화면을 PNG로 저장한다.
#[tauri::command]
async fn rec_save_image(app: AppHandle) -> CmdResult<String> {
    let (image, settings) = {
        let state = app.state::<AppState>();
        let image = state.recorder.lock().as_ref().and_then(Recorder::latest_image)
            .ok_or("아직 저장할 녹화 화면이 없습니다")?;
        let settings = state.settings.lock().clone();
        (image, settings)
    };
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<String> {
        let dir = Path::new(&settings.output_dir);
        std::fs::create_dir_all(dir).map_err(err)?;
        let name = export::file_name_from_rule(&settings.file_name_rule);
        let path = export::unique_path(dir, &name, "png");
        // 기존 파일을 덮어쓰지 않는다. 이름 결정 후 다른 프로세스가 만들었으면 오류를 알린다.
        write_new_recording_png(&path, image.0, image.1, &image.2)?;
        Ok(path.to_string_lossy().into_owned())
    }).await.map_err(err)?
}

fn write_new_recording_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> CmdResult<()> {
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(err)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(err)?;
    writer.write_image_data(rgba).map_err(err)?;
    writer.finish().map_err(err)?;
    Ok(())
}

/// 프로그램 끝내기 (편집 창을 닫을 때)
#[tauri::command]
fn quit_app(app: AppHandle, state: State<AppState>) {
    // 정상 종료: 자동 저장해 둔 작업은 지운다 (남아 있으면 다음에 켤 때 비정상 종료로 본다)
    {
        state.recovery_stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut stopped = state.recovery_io.lock();
        *stopped = true;
        recovery::clear(&recovery::current(&recovery::root()));
    }
    app.exit(0);
}

/// 편집 창을 닫고 녹화 틀로 돌아간다 (편집 중인 프레임은 버린다)
#[tauri::command]
fn show_recorder(app: AppHandle, state: State<AppState>) -> CmdResult<()> {
    use std::sync::atomic::Ordering;
    state.recovery_stop.store(true, Ordering::SeqCst);
    state.project.lock().load(vec![]);
    {
        let _io = state.recovery_io.lock();
        recovery::clear(&recovery::current(&recovery::root()));
        state.recovery_stop.store(false, Ordering::SeqCst);
    }
    if let Some(m) = app.get_webview_window("main") {
        let _ = m.hide();
    }
    let r = recorder_window(&app)?;
    r.show().map_err(err)?;
    let _ = r.set_focus();
    Ok(())
}

// ---------- 설정 ----------

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
fn set_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> CmdResult<()> {
    settings::save(&settings).map_err(err)?;
    state.project.lock().set_undo_enabled(settings.enable_undo);
    let on_top = settings.always_on_top;
    *state.settings.lock() = settings;
    if let Ok(r) = recorder_window(&app) {
        let _ = r.set_always_on_top(on_top);
    }
    register_hotkeys(&app)?;
    let _ = app.emit("settings-changed", ());
    Ok(())
}

fn register_hotkeys(app: &AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    // 시작/정리와 단축키 재등록이 동시에 실행되어 등록 상태가 어긋나지 않게 한다.
    let recorder = state.recorder.lock();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    *state.snapshot_hotkey.lock() = false;
    let (rec, pause, ctrl_prtsc) = {
        let s = state.settings.lock();
        (s.hotkey_record.clone(), s.hotkey_pause.clone(), s.hotkey_ctrl_prtsc)
    };
    let mut problems = vec![];
    let mut parse = |key: &str| -> Option<Shortcut> {
        if key.trim().is_empty() {
            return None;
        }
        match key.parse::<Shortcut>() {
            Ok(sc) => match gs.register(sc) {
                Ok(()) => Some(sc),
                Err(e) => {
                    let why = if e.to_string().contains("already registered") {
                        "다른 프로그램이 이미 쓰고 있습니다".to_string()
                    } else {
                        e.to_string()
                    };
                    problems.push(format!("{key}: {why}"));
                    None
                }
            },
            Err(_) => {
                problems.push(format!("{key}: 알 수 없는 키 이름입니다"));
                None
            }
        }
    };
    let r = parse(&rec);
    let p = parse(&pause);
    let prtsc = if ctrl_prtsc {
        let shortcut: Shortcut = "Ctrl+PrintScreen".parse().map_err(err)?;
        if Some(shortcut) == r {
            Some(shortcut)
        } else if Some(shortcut) == p {
            problems.push("Ctrl+PrtSc: 일시정지 단축키와 겹칩니다".into());
            None
        } else {
            parse("Ctrl+PrintScreen")
        }
    } else { None };
    *state.hotkeys.lock() = (r, p);
    *state.ctrl_prtsc_hotkey.lock() = prtsc;
    if state.settings.lock().rec_global_snapshot_hotkey && recorder.is_some() {
        if let Err(e) = set_snapshot_hotkey(app, true) {
            problems.push(format!("Ctrl+S: {e}"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("단축키를 등록하지 못했습니다: {}", problems.join(", ")))
    }
}

fn set_snapshot_hotkey(app: &AppHandle, enabled: bool) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut active = state.snapshot_hotkey.lock();
    if *active == enabled { return Ok(()); }
    let shortcut: Shortcut = "Ctrl+S".parse().map_err(err)?;
    if enabled {
        let (rec, pause) = *state.hotkeys.lock();
        if Some(shortcut) == rec || Some(shortcut) == pause {
            return Err("녹화 또는 일시정지 단축키와 겹칩니다".into());
        }
        app.global_shortcut().register(shortcut).map_err(err)?;
        *active = true;
    } else {
        app.global_shortcut().unregister(shortcut).map_err(err)?;
        *active = false;
    }
    Ok(())
}

fn on_hotkey(app: &AppHandle, sc: &Shortcut) {
    let state = app.state::<AppState>();
    let (rec, pause) = *state.hotkeys.lock();
    // 녹화 틀이 보일 때만 반응한다
    let visible = recorder_window(app).ok().and_then(|w| w.is_visible().ok()).unwrap_or(false);
    let recording = state.recorder.lock().is_some();
    if !visible && !recording {
        return;
    }
    let snapshot: Shortcut = "Ctrl+S".parse().expect("valid shortcut");
    if *sc == snapshot && recording && *state.snapshot_hotkey.lock() {
        let h = app.clone();
        tauri::async_runtime::spawn(async move {
            let message = match rec_save_image(h.clone()).await {
                Ok(path) => format!("이미지를 저장했습니다: {path}"),
                Err(e) => e,
            };
            let _ = h.emit("notice", message);
        });
    } else if Some(*sc) == rec || Some(*sc) == *state.ctrl_prtsc_hotkey.lock() {
        if recording {
            let h = app.clone();
            std::thread::spawn(move || finish_recording(&h));
        } else if let Err(e) = start_recording(app) {
            let _ = app.emit("notice", e);
        }
    } else if Some(*sc) == pause
        && let Some(r) = state.recorder.lock().as_ref()
    {
        r.set_paused(!r.is_paused());
    }
}

// ---------- 편집 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectView {
    frames: Vec<FrameInfo>,
    can_undo: bool,
    can_redo: bool,
    dirty: bool,
    /// 편집 뒤 바뀐 선택 (앞뒤 이동 등)
    selection: Option<Vec<usize>>,
    message: Option<String>,
    /// 편집에 쓰는 메모리(되돌리기 기록 포함)
    memory_bytes: u64,
}

fn view(p: &Project, selection: Option<Vec<usize>>, message: Option<String>) -> ProjectView {
    ProjectView {
        frames: p.infos(),
        can_undo: p.can_undo(),
        can_redo: p.can_redo(),
        dirty: p.dirty,
        selection,
        message,
        memory_bytes: p.memory_bytes(),
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum EditOp {
    Delete { sel: Vec<usize> },
    DeleteUnselected { sel: Vec<usize> },
    CutRange { begin: usize, end: usize },
    Reverse { sel: Vec<usize> },
    Yoyo { sel: Vec<usize> },
    Merge { sel: Vec<usize> },
    SetDelay { sel: Vec<usize>, ms: u32 },
    ScaleSpeed { sel: Vec<usize>, factor: f64 },
    Move { sel: Vec<usize>, dir: i32 },
    MoveToEnd { sel: Vec<usize>, to_front: bool, copy: bool },
    MoveTo { sel: Vec<usize>, to: usize },
    Copy { sel: Vec<usize> },
    Cut { sel: Vec<usize> },
    Paste { at: usize },
    Reduce { sel: Vec<usize>, mode: ReduceMode, n: usize, keep_speed: bool },
    ReduceSimilar { sel: Vec<usize>, similarity: u32, tolerance: u32, time_limit_ms: Option<u32>, keep_speed: bool },
    Crop { x: u32, y: u32, w: u32, h: u32 },
    Resize { w: u32, h: u32, smooth: bool },
    Rotate { r: Rotate },
    Undo,
    Redo,
    RestoreInitial,
    SetInitial,
    Filters { sel: Vec<usize>, filters: Vec<Filter> },
    Transition { sel: Vec<usize>, kind: TransitionKind, ms: u32 },
    Zoom { start: usize, zoom: ZoomFx },
    Summarize { options: SummaryOptions },
    Upscale { factor: u32 },
}

#[tauri::command]
fn project_view(state: State<AppState>) -> ProjectView {
    view(&state.project.lock(), None, None)
}

#[tauri::command]
async fn edit(app: AppHandle, op: EditOp) -> CmdResult<ProjectView> {
    // 크기 바꾸기 같은 무거운 편집이 화면을 멈추지 않게 따로 돌린다
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut p = state.project.lock();
        let mut selection = None;
        let mut message = None;
        match op {
            EditOp::Delete { sel } => p.delete(&sel),
            EditOp::DeleteUnselected { sel } => p.delete_unselected(&sel),
            EditOp::CutRange { begin, end } => p.cut_range(begin, end),
            EditOp::Reverse { sel } => p.reverse(&sel),
            EditOp::Yoyo { sel } => p.yoyo(&sel),
            EditOp::Merge { sel } => p.merge(&sel),
            EditOp::SetDelay { sel, ms } => p.set_delay(&sel, ms),
            EditOp::ScaleSpeed { sel, factor } => p.scale_speed(&sel, factor),
            EditOp::Move { sel, dir } => selection = Some(p.move_by(&sel, dir)),
            EditOp::MoveToEnd { sel, to_front, copy } => p.move_to_end(&sel, to_front, copy),
            EditOp::MoveTo { sel, to } => selection = Some(p.move_to(&sel, to)),
            EditOp::Copy { sel } => p.copy(&sel),
            EditOp::Cut { sel } => p.cut(&sel),
            EditOp::Paste { at } => {
                if !p.paste(at) {
                    message = Some("붙여넣을 프레임이 없거나 크기가 다릅니다".into());
                }
            }
            EditOp::Reduce { sel, mode, n, keep_speed } => p.reduce(&sel, mode, n, keep_speed),
            EditOp::ReduceSimilar { sel, similarity, tolerance, time_limit_ms, keep_speed } => {
                let n = p.reduce_similar(&sel, similarity, tolerance, time_limit_ms, keep_speed);
                message = Some(format!("비슷한 프레임 {n}개를 지웠습니다"));
            }
            EditOp::Crop { x, y, w, h } => p.crop(x, y, w, h),
            EditOp::Resize { w, h, smooth } => p.resize(w, h, smooth),
            EditOp::Rotate { r } => p.rotate(r),
            EditOp::Undo => {
                if !p.undo() {
                    message = Some("되돌릴 작업이 없습니다".into());
                }
            }
            EditOp::Redo => {
                if !p.redo() {
                    message = Some("다시 실행할 작업이 없습니다".into());
                }
            }
            EditOp::RestoreInitial => p.restore_initial(),
            EditOp::SetInitial => p.set_initial(),
            EditOp::Filters { sel, filters } => p.apply_filters(&sel, &filters),
            EditOp::Transition { sel, kind, ms } => p.transition(&sel, kind, ms),
            EditOp::Zoom { start, zoom } => p.zoom_fx(start, &zoom),
            EditOp::Summarize { options } => p.summarize(options),
            EditOp::Upscale { factor } => p.upscale(factor),
        }
        view(&p, selection, message)
    })
    .await
    .map_err(err)
}

/// 바깥쪽 투명 테두리를 걷어낸 영역 (자르기의 "자동 맞춤")
#[tauri::command]
fn trim_bounds(state: State<AppState>) -> Option<(u32, u32, u32, u32)> {
    state.project.lock().trim_bounds()
}

// ---------- 저장 ----------

#[tauri::command]
fn default_save_path(state: State<AppState>, ext: String) -> String {
    let s = state.settings.lock();
    let name = export::file_name_from_rule(&s.file_name_rule);
    export::unique_path(Path::new(&s.output_dir), &name, &ext).to_string_lossy().into_owned()
}

#[tauri::command]
fn pick_save_path(app: AppHandle, current: String, ext: String) -> Option<String> {
    let cur = PathBuf::from(&current);
    let mut d = app.dialog().file().add_filter(ext.to_uppercase(), &[&ext]);
    if let Some(dir) = cur.parent() {
        d = d.set_directory(dir);
    }
    if let Some(name) = cur.file_name() {
        d = d.set_file_name(name.to_string_lossy());
    }
    d.blocking_save_file()?.into_path().ok().map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn pick_folder(app: AppHandle, current: String) -> Option<String> {
    app.dialog()
        .file()
        .set_directory(current)
        .blocking_pick_folder()?
        .into_path()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[derive(Serialize, Clone)]
struct ExportProgress {
    progress: f32,
    message: String,
}

#[derive(Serialize)]
struct ExportResult {
    paths: Vec<PathBuf>,
    size: u64,
}

/// 이번 저장으로 덮어쓰게 될 파일을 찾는다.
#[tauri::command]
async fn export_conflicts(app: AppHandle, opts: ExportOptions, path: String) -> CmdResult<Vec<PathBuf>> {
    let (frames, s) = {
        let state = app.state::<AppState>();
        let frames = state.project.lock().frames.clone();
        let s = state.settings.lock().clone();
        (frames, s)
    };
    tauri::async_runtime::spawn_blocking(move || {
        export::planned_paths(&frames, Path::new(&path), &opts, &s).into_iter().filter(|p| p.exists()).collect()
    })
    .await
    .map_err(err)
}

/// 저장한 파일 목록과 전체 크기를 돌려준다.
#[tauri::command]
async fn export_project(app: AppHandle, opts: ExportOptions, path: String) -> CmdResult<ExportResult> {
    let (frames, s) = {
        let state = app.state::<AppState>();
        let frames = state.project.lock().frames.clone();
        let s = state.settings.lock().clone();
        (frames, s)
    };
    let h = app.clone();
    let paths = tauri::async_runtime::spawn_blocking(move || {
        export::export(&frames, Path::new(&path), &opts, &s, &|progress, msg| {
            let _ = h.emit("export-progress", ExportProgress { progress, message: msg.into() });
        })
    })
    .await
    .map_err(err)?
    .map_err(|e| format!("{e:#}"))?;
    let size = paths.iter().map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)).sum();
    app.state::<AppState>().project.lock().dirty = false;
    Ok(ExportResult { paths, size })
}

#[tauri::command]
async fn estimate_size(app: AppHandle, opts: ExportOptions) -> CmdResult<u64> {
    let (frames, s) = {
        let state = app.state::<AppState>();
        let frames = state.project.lock().frames.clone();
        let s = state.settings.lock().clone();
        (frames, s)
    };
    tauri::async_runtime::spawn_blocking(move || export::estimate_size(&frames, &opts, &s))
        .await
        .map_err(err)?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn has_ffmpeg() -> bool {
    export::find_ffmpeg().is_some()
}

#[tauri::command]
fn reveal_file(path: String) -> CmdResult<()> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(err)
}

/// 저장한 파일 지우기 (설정에 따라 휴지통으로)
#[tauri::command]
fn delete_file(state: State<AppState>, path: String) -> CmdResult<()> {
    if state.settings.lock().delete_to_recycle_bin { recycle(&path) } else { std::fs::remove_file(&path).map_err(err) }
}

#[cfg(windows)]
fn recycle(path: &str) -> CmdResult<()> {
    use windows::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW,
    };
    use windows::core::PCWSTR;
    // 경로 목록은 \0\0으로 끝나야 한다
    let wide: Vec<u16> = path.encode_utf16().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(wide.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT).0 as u16,
        ..Default::default()
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r == 0 { Ok(()) } else { Err(format!("휴지통으로 옮기지 못했습니다 (코드 {r})")) }
}

/// 편집 창 미리보기용 프레임 이미지: frame://localhost/<id> (원본), /<id>/t (썸네일)
fn frame_png(app: &AppHandle, path: &str) -> Option<Vec<u8>> {
    let mut parts = path.trim_start_matches('/').split('/');
    let id: u64 = parts.next()?.parse().ok()?;
    let thumb = parts.next() == Some("t");
    let f = app.state::<AppState>().project.lock().find(id)?.clone();
    let (mut w, mut h, mut px) = (f.width, f.height, f.rgba());
    if thumb {
        let scale = (160.0 / w as f32).min(120.0 / h as f32).min(1.0);
        let (tw, th) = (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
        px = project::resize_rgba(w, h, &px, tw, th, true);
        (w, h) = (tw, th);
    }
    encode_png(w, h, &px)
}

/// 화면에 보여줄 용도의 빠른 PNG
fn encode_png(w: u32, h: u32, px: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![];
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fastest);
    let mut wr = enc.write_header().ok()?;
    wr.write_image_data(px).ok()?;
    wr.finish().ok()?;
    Some(out)
}

/// 필터 미리보기: 프레임 한 장에 필터를 적용한 PNG
#[tauri::command]
async fn filter_preview(app: AppHandle, id: u64, filters: Vec<Filter>) -> CmdResult<tauri::ipc::Response> {
    // 잔상 효과는 앞 프레임 몇 장이 있어야 보인다
    let (f, before) = {
        let state = app.state::<AppState>();
        let p = state.project.lock();
        let i = p.frames.iter().position(|x| x.id == id).ok_or("프레임이 없습니다")?;
        (p.frames[i].clone(), p.frames[i.saturating_sub(12)..i].to_vec())
    };
    tauri::async_runtime::spawn_blocking(move || {
        let run = |fr: &frame::Frame| {
            let mut px = fr.rgba();
            for flt in &filters {
                effects::apply_filter(&mut px, fr.width, fr.height, flt);
            }
            px
        };
        let mut px = run(&f);
        if let Some(strength) = filters.iter().find_map(|x| match x {
            Filter::Afterimage { strength } => Some(*strength),
            _ => None,
        }) {
            let mut prev: Option<Vec<u8>> = None;
            for b in before.iter().filter(|b| (b.width, b.height) == (f.width, f.height)) {
                let mut q = run(b);
                if let Some(p) = &prev {
                    filters::afterimage(&mut q, p, strength);
                }
                prev = Some(q);
            }
            if let Some(p) = &prev {
                filters::afterimage(&mut px, p, strength);
            }
        }
        encode_png(f.width, f.height, &px).map(tauri::ipc::Response::new).ok_or_else(|| "PNG 변환 실패".to_string())
    })
    .await
    .map_err(err)?
}

// ---------- 열기 ----------

#[tauri::command]
fn pick_open_files(app: AppHandle) -> Option<Vec<String>> {
    let all: Vec<&str> = import::IMAGE_EXTS.iter().chain(import::VIDEO_EXTS).copied().collect();
    let picked = app
        .dialog()
        .file()
        .add_filter("지원하는 파일", &all)
        .add_filter("움직이는 그림·이미지", import::IMAGE_EXTS)
        .add_filter("동영상", import::VIDEO_EXTS)
        .blocking_pick_files()?;
    Some(picked.into_iter().filter_map(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned()).collect())
}

#[tauri::command]
async fn probe_video(path: String) -> CmdResult<import::VideoInfo> {
    tauri::async_runtime::spawn_blocking(move || import::probe_video(&path))
        .await
        .map_err(err)?
        .map_err(|e| format!("{e:#}"))
}

// ---------- 새로 만들기·콜라주 ----------

fn memory_limit(app: &AppHandle) -> usize {
    app.state::<AppState>().settings.lock().max_edit_memory_mb.max(64) as usize * 1024 * 1024
}

/// 한 색(또는 투명) 배경의 빈 영상
#[tauri::command]
fn new_blank(
    app: AppHandle,
    width: u32,
    height: u32,
    fps: f32,
    sec: f32,
    color: [u8; 3],
    transparent: bool,
) -> CmdResult<ProjectView> {
    let [r, g, b] = color;
    let frames = collage::blank(width, height, fps, sec, [r, g, b, if transparent { 0 } else { 255 }]).map_err(err)?;
    Ok(start_editing(&app, import::Loaded { frames, warning: None }, true))
}

/// 콜라주 재료 하나 불러오기. 동영상은 앞 10초를 15FPS, 가로 640 이하로.
fn load_clip(path: &str, max: usize) -> anyhow::Result<Vec<frame::Frame>> {
    if import::is_video(std::path::Path::new(path)) {
        let info = import::probe_video(path)?;
        let s = (640.0 / info.width as f32).min(1.0);
        let (w, h) = (((info.width as f32 * s) as u32).max(1), ((info.height as f32 * s) as u32).max(1));
        Ok(import::load_video(path, 0, info.duration_ms.min(10_000), 15.0, w, h, max, &|_, _| {})?.frames)
    } else {
        Ok(import::load_files(&[path.to_string()], 100, import::Fit::Contain, max, &|_, _| {})?.frames)
    }
}

#[tauri::command]
async fn make_collage(
    app: AppHandle,
    paths: Vec<String>,
    use_current: bool,
    background_image: Option<String>,
    options: collage::CollageOptions,
) -> CmdResult<ProjectView> {
    let max = memory_limit(&app);
    let current = if use_current { app.state::<AppState>().project.lock().frames.clone() } else { vec![] };
    let progress = import_progress(&app);
    let frames = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Vec<frame::Frame>> {
        let mut clips = vec![];
        if !current.is_empty() {
            clips.push(current);
        }
        for (k, p) in paths.iter().enumerate() {
            progress(k as f32 / paths.len().max(1) as f32 * 0.5, "콜라주 재료 불러오는 중");
            clips.push(load_clip(p, max)?);
        }
        let bg = match background_image {
            Some(p) => {
                let img = image::open(&p)?.to_rgba8();
                Some((img.width(), img.height(), img.into_raw()))
            }
            None => None,
        };
        progress(0.6, "콜라주 만드는 중");
        collage::make(&clips, &options, bg)
    })
    .await
    .map_err(err)?
    .map_err(|e| format!("{e:#}"))?;
    Ok(start_editing(&app, import::Loaded { frames, warning: None }, true))
}

// ---------- 파일 관리 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    name: String,
    path: String,
    size: u64,
    modified_ms: u64,
}

/// 저장 폴더의 움짤·동영상·이미지 목록
#[tauri::command]
fn list_files(dir: String) -> CmdResult<Vec<FileEntry>> {
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        // 아직 한 번도 저장하지 않아 폴더가 없을 수 있다
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(err(e)),
    };
    let mut out = vec![];
    for e in rd.flatten() {
        let p = e.path();
        let ext = import::ext_of(&p);
        if !(import::IMAGE_EXTS.contains(&ext.as_str()) || import::VIDEO_EXTS.contains(&ext.as_str())) {
            continue;
        }
        let Ok(m) = e.metadata() else { continue };
        if !m.is_file() {
            continue;
        }
        let modified_ms = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64);
        out.push(FileEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            path: p.to_string_lossy().into_owned(),
            size: m.len(),
            modified_ms,
        });
    }
    Ok(out)
}

/// 같은 폴더 안에서 이름만 바꾼다. 움짤·동영상·이미지 확장자로 끝나지 않으면 원래 확장자를 붙인다
/// ("2026.10.29"의 ".29"는 확장자가 아니다).
#[tauri::command]
fn rename_file(path: String, new_name: String) -> CmdResult<String> {
    let new_name = new_name.trim();
    const BAD: &str = "\\/:*?\"<>|";
    if new_name.is_empty() || new_name.chars().any(|c| BAD.contains(c)) {
        return Err("파일 이름에 쓸 수 없는 글자가 있습니다 (\\ / : * ? \" < > |)".into());
    }
    let old = PathBuf::from(&path);
    let mut name = new_name.to_string();
    let typed = import::ext_of(std::path::Path::new(&name));
    if !(import::IMAGE_EXTS.contains(&typed.as_str()) || import::VIDEO_EXTS.contains(&typed.as_str()))
        && let Some(ext) = old.extension()
    {
        name = format!("{name}.{}", ext.to_string_lossy());
    }
    let new = old.with_file_name(name);
    if new.exists() {
        return Err("같은 이름의 파일이 이미 있습니다".into());
    }
    std::fs::rename(&old, &new).map_err(err)?;
    Ok(new.to_string_lossy().into_owned())
}

/// 관리 화면 미리보기용 파일 내용 (100MB 이하)
#[tauri::command]
async fn file_bytes(path: String) -> CmdResult<tauri::ipc::Response> {
    tauri::async_runtime::spawn_blocking(move || {
        let len = std::fs::metadata(&path).map_err(err)?.len();
        if len > 100 * 1024 * 1024 {
            return Err("파일이 너무 커서 미리 볼 수 없습니다".to_string());
        }
        std::fs::read(&path).map(tauri::ipc::Response::new).map_err(err)
    })
    .await
    .map_err(err)?
}

// ---------- 클립보드·이미지로 저장 ----------

/// i번 프레임을 클립보드에 그림으로 복사
#[tauri::command]
fn copy_frame_image(state: State<AppState>, index: usize) -> CmdResult<()> {
    let f = state.project.lock().frames.get(index).cloned().ok_or("프레임이 없습니다")?;
    let mut cb = arboard::Clipboard::new().map_err(err)?;
    cb.set_image(arboard::ImageData {
        width: f.width as usize,
        height: f.height as usize,
        bytes: std::borrow::Cow::Owned(f.rgba()),
    })
    .map_err(err)
}

fn clipboard_rgba() -> CmdResult<(u32, u32, Vec<u8>)> {
    let mut cb = arboard::Clipboard::new().map_err(err)?;
    let img = cb.get_image().map_err(|_| "클립보드에 이미지가 없습니다!".to_string())?;
    Ok((img.width as u32, img.height as u32, img.bytes.into_owned()))
}

/// 클립보드 그림을 i번 프레임에 넣는다. replace면 그 프레임을 바꾸고, 아니면 바로 뒤에 새 프레임으로.
#[tauri::command]
fn paste_clipboard_image(state: State<AppState>, index: usize, replace: bool) -> CmdResult<ProjectView> {
    let (w, h, px) = clipboard_rgba()?;
    let mut p = state.project.lock();
    let Some(f) = p.frames.get(index).cloned() else {
        return Err("프레임이 없습니다".into());
    };
    let fitted = import::fit_rgba(w, h, &px, f.width, f.height, import::Fit::Contain);
    let ok = if replace {
        p.replace_pixels(index, f.width, f.height, &fitted)
    } else {
        p.insert_frame(index + 1, frame::Frame::from_rgba(f.width, f.height, &fitted, f.delay_ms))
    };
    if !ok {
        return Err("클립보드 이미지를 넣지 못했습니다".into());
    }
    let sel = if replace { vec![index] } else { vec![index + 1] };
    Ok(view(&p, Some(sel), Some("클립보드 이미지를 넣었습니다".into())))
}

/// 클립보드 그림 PNG (삽입 화면에서 이미지로 쓰기)
#[tauri::command]
fn clipboard_image_png() -> CmdResult<tauri::ipc::Response> {
    let (w, h, px) = clipboard_rgba()?;
    encode_png(w, h, &px).map(tauri::ipc::Response::new).ok_or_else(|| "PNG 변환 실패".into())
}

/// 프레임 한 장을 이미지 파일로. 확장자로 형식을 고른다 (png·jpg·bmp·webp).
fn write_image(path: &std::path::Path, w: u32, h: u32, rgba: &[u8], jpg_quality: u8) -> anyhow::Result<()> {
    let ext = import::ext_of(path);
    // JPG·BMP는 투명을 못 쓰므로 흰 바탕에 합성
    let opaque = || -> Vec<u8> {
        rgba.chunks_exact(4)
            .flat_map(|p| {
                let a = p[3] as u32;
                [0, 1, 2].map(|k| ((p[k] as u32 * a + 255 * (255 - a)) / 255) as u8)
            })
            .collect()
    };
    match ext.as_str() {
        "jpg" | "jpeg" => {
            let file = std::io::BufWriter::new(std::fs::File::create(path)?);
            image::codecs::jpeg::JpegEncoder::new_with_quality(file, jpg_quality.clamp(1, 100)).encode(
                &opaque(),
                w,
                h,
                image::ExtendedColorType::Rgb8,
            )?;
        }
        "bmp" => image::save_buffer(path, &opaque(), w, h, image::ExtendedColorType::Rgb8)?,
        "webp" => image::save_buffer(path, rgba, w, h, image::ExtendedColorType::Rgba8)?,
        _ => export::write_png(path, w, h, rgba)?,
    }
    Ok(())
}

#[tauri::command]
fn save_frame_image(state: State<AppState>, index: usize, path: String) -> CmdResult<()> {
    let f = state.project.lock().frames.get(index).cloned().ok_or("프레임이 없습니다")?;
    let q = state.settings.lock().jpg_export_quality;
    write_image(std::path::Path::new(&path), f.width, f.height, &f.rgba(), q).map_err(|e| format!("{e:#}"))
}

/// 고른 프레임들을 폴더에 이미지로 (이름_0001.png ...). 만든 파일 수를 돌려준다.
#[tauri::command]
async fn export_frames(
    app: AppHandle,
    indices: Vec<usize>,
    dir: String,
    prefix: String,
    format: String,
) -> CmdResult<usize> {
    let (frames, q) = {
        let state = app.state::<AppState>();
        let p = state.project.lock();
        let frames: Vec<frame::Frame> = indices.iter().filter_map(|&i| p.frames.get(i).cloned()).collect();
        let q = state.settings.lock().jpg_export_quality;
        (frames, q)
    };
    let ext = if matches!(format.as_str(), "jpg" | "bmp" | "webp") { format } else { "png".into() };
    let prefix = export::file_name_from_rule(if prefix.trim().is_empty() { "frame" } else { prefix.trim() });
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<usize> {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        frames.par_iter().enumerate().try_for_each(|(k, f)| {
            write_image(&dir.join(format!("{prefix}_{:04}.{ext}", k + 1)), f.width, f.height, &f.rgba(), q)
        })?;
        Ok(frames.len())
    })
    .await
    .map_err(err)?
    .map_err(|e| format!("{e:#}"))
}

/// 저장 대화상자 (형식 목록을 받아서)
#[tauri::command]
fn pick_save_file(app: AppHandle, current: String, filters: Vec<(String, Vec<String>)>) -> Option<String> {
    let cur = PathBuf::from(&current);
    let mut d = app.dialog().file();
    for (name, exts) in &filters {
        let e: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
        d = d.add_filter(name, &e);
    }
    if let Some(dir) = cur.parent().filter(|p| !p.as_os_str().is_empty()) {
        d = d.set_directory(dir);
    }
    if let Some(name) = cur.file_name() {
        d = d.set_file_name(name.to_string_lossy());
    }
    d.blocking_save_file()?.into_path().ok().map(|p| p.to_string_lossy().into_owned())
}

/// 파일 하나 고르기
#[tauri::command]
fn pick_file(app: AppHandle, filters: Vec<(String, Vec<String>)>) -> Option<String> {
    let mut d = app.dialog().file();
    for (name, exts) in &filters {
        let e: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
        d = d.add_filter(name, &e);
    }
    d.blocking_pick_file()?.into_path().ok().map(|p| p.to_string_lossy().into_owned())
}

// ---------- 설정 내보내기·가져오기 ----------

/// 설정 창에서 고치고 있는 값(settings)을 파일로 쓴다. 지금 쓰는 설정은 바꾸지 않는다.
#[tauri::command]
fn export_settings(path: String, settings: Settings) -> CmdResult<()> {
    std::fs::write(&path, serde_json::to_string_pretty(&settings).map_err(err)?)
        .map_err(|e| format!("설정 정보를 내보내지 못했습니다!: {e}"))
}

/// 설정 파일을 읽어 지금 설정으로 쓴다. 없는 항목은 기본값.
#[tauri::command]
fn import_settings(app: AppHandle, state: State<AppState>, path: String) -> CmdResult<Settings> {
    let text = std::fs::read_to_string(&path).map_err(err)?;
    let s: Settings = serde_json::from_str(&text).map_err(|e| format!("OpenCam 설정 파일이 아닙니다: {e}"))?;
    set_settings(app, state, s.clone())?;
    Ok(s)
}

/// 동영상 구간 고르기용 장면 미리보기
#[tauri::command]
async fn video_frame(path: String, at_ms: u64) -> CmdResult<tauri::ipc::Response> {
    tauri::async_runtime::spawn_blocking(move || import::video_frame_png(&path, at_ms, 480))
        .await
        .map_err(err)?
        .map(tauri::ipc::Response::new)
        .map_err(|e| format!("{e:#}"))
}

/// 불러온 프레임으로 편집을 새로 시작하고 편집 창을 띄운다.
/// unsaved: 파일에서 연 게 아니라 새로 만든 것(빈 영상·콜라주)이라 버릴 때 물어야 하면 true
fn start_editing(app: &AppHandle, loaded: import::Loaded, unsaved: bool) -> ProjectView {
    let state = app.state::<AppState>();
    let v = {
        let mut p = state.project.lock();
        p.set_undo_enabled(state.settings.lock().enable_undo);
        p.load(loaded.frames);
        p.dirty = unsaved;
        view(&p, Some(vec![]), loaded.warning)
    };
    show_editor(app);
    v
}

// ---------- 작업 자동 저장·복구 ----------

/// 자동 저장한 뒤에도 디스크 공간이 이만큼은 남아야 한다 (모자라면 쓰지 않는다)
const AUTOSAVE_MIN_FREE: u64 = 1024 * 1024 * 1024;

/// 자동 저장(또는 비우기)할 때인가. key는 지금의 (프레임 목록 판, 작업 자동 저장 설정),
/// saved는 마지막으로 저장하거나 비운 때의 key, still은 지금 판이 더 바뀌지 않고 지난 시간.
/// 설정만 바뀌어도 다시 따진다 (켜면 바로 저장하고, 끄면 바로 지운다).
fn autosave_due(saved: Option<(u64, bool)>, key: (u64, bool), still: std::time::Duration) -> bool {
    saved != Some(key) && still >= std::time::Duration::from_secs(3)
}

/// 프레임 목록이 바뀌고 3초 동안 더 바뀌지 않으면 자동 저장한다
fn autosave_loop(app: AppHandle) {
    let state = app.state::<AppState>();
    let dir = recovery::current(&recovery::root());
    let mut saved = None; // 마지막으로 저장(또는 비움)한 (판, 설정)
    let mut seen = (0u64, std::time::Instant::now()); // 마지막으로 본 판과 처음 본 시각
    let mut warned = false;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let rev = state.project.lock().revision;
        if rev != seen.0 {
            seen = (rev, std::time::Instant::now());
            continue;
        }
        if !autosave_due(saved, (rev, state.settings.lock().auto_recovery), seen.1.elapsed()) {
            continue;
        }
        let stopped = state.recovery_io.lock();
        if *stopped {
            return;
        }
        let (rev, frames) = {
            let p = state.project.lock();
            (p.revision, p.frames.clone())
        };
        let enabled = state.settings.lock().auto_recovery;
        if frames.is_empty() || !enabled {
            recovery::clear(&dir);
        } else if !recovery::room_for(&dir, &frames, AUTOSAVE_MIN_FREE) {
            if !warned {
                warned = true;
                let msg = "디스크 공간이 모자라 작업 자동 저장을 멈췄습니다 (저장 뒤에도 1GB는 남아야 합니다)";
                let _ = app.emit("notice", msg.to_string());
            }
            continue;
        } else if let Err(e) = recovery::save(&dir, &frames, &state.recovery_stop) {
            if state.recovery_stop.load(std::sync::atomic::Ordering::SeqCst) {
                continue; // 끝내기·녹화 틀로 돌아가기가 멈추게 했다
            }
            eprintln!("자동 저장 실패: {e:#}");
            if !warned {
                warned = true;
                let _ = app.emit("notice", format!("작업을 자동 저장하지 못했습니다: {e}"));
            }
            // 같은 판을 1초마다 다시 쓰고 지우지 않게, 다음 편집 때 다시 해 본다
            saved = Some((rev, enabled));
            continue;
        }
        warned = false;
        saved = Some((rev, enabled));
    }
}

/// 비정상 종료로 남은 작업들 (최근 것부터)
#[tauri::command]
fn recovery_list() -> Vec<recovery::Entry> {
    recovery::list(&recovery::root())
}

/// 복구할 작업의 첫 프레임 썸네일 (PNG)
#[tauri::command]
async fn recovery_thumb(id: String) -> CmdResult<tauri::ipc::Response> {
    tauri::async_runtime::spawn_blocking(move || {
        let f = recovery::first_frame(&recovery::entry_dir(&recovery::root(), &id)?)?;
        let scale = (160.0 / f.width as f32).min(120.0 / f.height as f32).min(1.0);
        let (tw, th) = (((f.width as f32 * scale) as u32).max(1), ((f.height as f32 * scale) as u32).max(1));
        let px = project::resize_rgba(f.width, f.height, &f.rgba(), tw, th, true);
        encode_png(tw, th, &px).ok_or_else(|| anyhow::anyhow!("썸네일을 만들지 못했습니다"))
    })
    .await
    .map_err(err)?
    .map(tauri::ipc::Response::new)
    .map_err(|e| format!("{e:#}"))
}

/// 남은 작업을 편집 창으로 불러온다. 바로 자동 저장해 두고 복구 목록에서는 지운다.
#[tauri::command]
async fn recovery_open(app: AppHandle, id: String) -> CmdResult<ProjectView> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = recovery::root();
        let dir = recovery::entry_dir(&root, &id).map_err(|e| format!("{e:#}"))?;
        let frames = recovery::load(&dir).map_err(|e| format!("복구하지 못했습니다: {e:#}"))?;
        let state = app.state::<AppState>();
        let v = start_editing(
            &app,
            import::Loaded { frames, warning: Some("작업을 복구했습니다 (되돌리기 기록은 없습니다)".into()) },
            true,
        );
        // 새로 저장하지 못하면 복구 목록에 그대로 남겨 둔다
        let stopped = state.recovery_io.lock();
        if *stopped {
            return Ok(v); // 프로그램을 끝내는 중
        }
        let frames = state.project.lock().frames.clone();
        let cur = recovery::current(&root);
        if !state.settings.lock().auto_recovery
            || (recovery::room_for(&cur, &frames, AUTOSAVE_MIN_FREE)
                && recovery::save(&cur, &frames, &state.recovery_stop).is_ok())
        {
            recovery::clear(&dir);
        }
        Ok(v)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn recovery_discard(id: String) -> CmdResult<()> {
    let dir = recovery::entry_dir(&recovery::root(), &id).map_err(|e| format!("{e:#}"))?;
    std::fs::remove_dir_all(dir).map_err(|e| format!("지우지 못했습니다: {e}"))
}

fn import_progress(app: &AppHandle) -> impl Fn(f32, &str) + Sync + use<> {
    let h = app.clone();
    move |progress, msg| {
        let _ = h.emit("import-progress", ExportProgress { progress, message: msg.into() });
    }
}

/// GIF·WebP·APNG·이미지 여러 장 열기. 크기가 다르면 첫 그림에 맞춘다.
#[tauri::command]
async fn import_files(app: AppHandle, paths: Vec<String>, still_ms: u32, fit: import::Fit) -> CmdResult<ProjectView> {
    let max = app.state::<AppState>().settings.lock().max_edit_memory_mb.max(64) as usize * 1024 * 1024;
    let progress = import_progress(&app);
    let loaded =
        tauri::async_runtime::spawn_blocking(move || import::load_files(&paths, still_ms, fit, max, &progress))
            .await
            .map_err(err)?
            .map_err(|e| format!("{e:#}"))?;
    Ok(start_editing(&app, loaded, false))
}

/// 동영상의 구간을 프레임으로 불러온다
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn import_video(
    app: AppHandle,
    path: String,
    start_ms: u64,
    end_ms: u64,
    fps: f32,
    width: u32,
    height: u32,
) -> CmdResult<ProjectView> {
    let max = app.state::<AppState>().settings.lock().max_edit_memory_mb.max(64) as usize * 1024 * 1024;
    let progress = import_progress(&app);
    let loaded = tauri::async_runtime::spawn_blocking(move || {
        import::load_video(&path, start_ms, end_ms, fps, width, height, max, &progress)
    })
    .await
    .map_err(err)?
    .map_err(|e| format!("{e:#}"))?;
    Ok(start_editing(&app, loaded, false))
}

/// 붙이지 못하고 건너뛴 파일
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Skipped {
    name: String,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppendResult {
    view: ProjectView,
    skipped: Vec<Skipped>,
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

/// 오류를 알림 창에 보여줄 짧은 글로 (경로는 빼고, ffmpeg 출력처럼 긴 것은 자른다)
fn short_reason(e: &anyhow::Error, path: &str) -> String {
    let text = format!("{e}").replace(&format!(": {path}"), "").replace(path, "");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        "읽을 수 없는 파일입니다".into()
    } else if text.chars().count() > 120 {
        format!("{}…", text.chars().take(120).collect::<String>())
    } else {
        text
    }
}

/// 파일 하나를 (w, h) 크기로 불러온다. 비율을 지키고 남는 곳은 투명하게.
/// 정지 이미지는 한 장에 100ms, 동영상은 전체를 fps(원본보다 높지 않게)로.
fn load_for_append(
    path: &str,
    size: (u32, u32),
    fps: f32,
    max: usize,
    progress: export::Progress,
) -> anyhow::Result<import::Loaded> {
    let p = Path::new(path);
    if import::is_video(p) {
        import::load_video_contain(path, fps, size.0, size.1, max, progress)
    } else if import::IMAGE_EXTS.contains(&import::ext_of(p).as_str()) {
        import::load_files_sized(&[path.to_string()], 100, import::Fit::Contain, Some(size), max, progress)
    } else {
        anyhow::bail!("지원하지 않는 형식입니다")
    }
}

/// 끌어 놓은 파일들을 편집 중인 프레임 맨 뒤에 붙인다 (되돌리기 한 번). 순서는 화면이 정해서 보낸다.
/// 읽지 못한 파일은 건너뛰고 skipped로 알린다. 모두 못 읽었으면 아무것도 바꾸지 않는다.
#[tauri::command]
async fn append_files(app: AppHandle, paths: Vec<String>) -> CmdResult<AppendResult> {
    let state = app.state::<AppState>();
    let _permit = AppendPermit::acquire(&state.append_busy)?;
    let (size, used, generation) = {
        let p = state.project.lock();
        let f = p.frames.first().ok_or("편집 중인 프레임이 없습니다")?;
        ((f.width, f.height), p.memory_bytes() as usize, p.generation)
    };
    let fps = app.state::<AppState>().settings.lock().fps as f32;
    // 편집 메모리 한도에서 이미 쓰는 만큼을 뺀 만큼만 더 불러온다 (대략)
    let max = memory_limit(&app).saturating_sub(used);
    let progress = import_progress(&app);
    let (frames, skipped, warning) = tauri::async_runtime::spawn_blocking(move || {
        let n = paths.len().max(1);
        let mut frames = vec![];
        let mut skipped = vec![];
        let mut warning = None;
        let mut left = max;
        for (k, path) in paths.iter().enumerate() {
            let name = file_name(path);
            if left == 0 {
                skipped.push(Skipped { name, reason: "편집 메모리 한도에 닿았습니다".into() });
                continue;
            }
            let sub = |v: f32, m: &str| progress((k as f32 + v) / n as f32, m);
            match load_for_append(path, size, fps, left, &sub) {
                Ok(l) => {
                    left = left.saturating_sub(l.frames.iter().map(|f| f.packed_bytes()).sum::<usize>());
                    if let Some(w) = l.warning {
                        warning = Some(format!("{name}: {w}"));
                        left = 0;
                    }
                    frames.extend(l.frames);
                }
                Err(e) => skipped.push(Skipped { reason: short_reason(&e, path), name }),
            }
        }
        (frames, skipped, warning)
    })
    .await
    .map_err(err)?;
    let mut p = state.project.lock();
    check_append_generation(generation, p.generation)?;
    let selection = if frames.is_empty() {
        None
    } else {
        Some(p.append(frames).ok_or("편집 중인 프레임의 크기가 바뀌어 붙이지 못했습니다")?)
    };
    Ok(AppendResult { view: view(&p, selection, warning), skipped })
}

/// 줌 미리보기: 가장 크게 확대된 순간의 모습
#[tauri::command]
async fn zoom_preview(app: AppHandle, id: u64, zoom: ZoomFx) -> CmdResult<tauri::ipc::Response> {
    let f = app.state::<AppState>().project.lock().find(id).cloned().ok_or("프레임이 없습니다")?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut px = f.rgba();
        effects::draw_zoom(&mut px, f.width, f.height, &zoom, 1.0);
        encode_png(f.width, f.height, &px).map(tauri::ipc::Response::new).ok_or_else(|| "PNG 변환 실패".to_string())
    })
    .await
    .map_err(err)?
}

/// 영상 요약 미리보기: 남는 프레임 수와 총 시간
#[tauri::command]
async fn summary_preview(app: AppHandle, options: SummaryOptions) -> CmdResult<SummaryPreview> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().project.lock().summary_preview(options))
        .await
        .map_err(err)
}

/// 덧씌우기 정보. 그림 픽셀(RGBA)은 요청 본문으로 따로 온다.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverlayMeta {
    sel: Vec<usize>,
    w: u32,
    h: u32,
    x: f32,
    y: f32,
    #[serde(default)]
    anim_in: Anim,
    #[serde(default)]
    anim_out: Anim,
    #[serde(default)]
    in_ms: f32,
    #[serde(default)]
    out_ms: f32,
}

/// 그리기·텍스트·이미지 덧씌우기. 큰 픽셀 데이터를 JSON 없이 그대로 받는다.
#[tauri::command]
async fn apply_overlay(app: AppHandle, request: tauri::ipc::Request<'_>) -> CmdResult<ProjectView> {
    let tauri::ipc::InvokeBody::Raw(rgba) = request.body() else {
        return Err("그림 데이터가 없습니다".into());
    };
    let raw = request.headers().get("x-overlay").and_then(|v| v.to_str().ok()).ok_or("덧씌우기 정보가 없습니다")?;
    // 하나면 객체, 여러 개(레이어)면 배열. 픽셀은 순서대로 이어 붙어 온다.
    let metas: Vec<OverlayMeta> = match serde_json::from_str::<Vec<OverlayMeta>>(raw) {
        Ok(v) => v,
        Err(_) => vec![serde_json::from_str::<OverlayMeta>(raw).map_err(err)?],
    };
    let need: usize = metas.iter().map(|m| (m.w * m.h * 4) as usize).sum();
    if metas.is_empty() || rgba.len() != need {
        return Err("그림 크기가 맞지 않습니다".into());
    }
    let sel = metas[0].sel.clone();
    let mut at = 0;
    let ovs: Vec<Overlay> = metas
        .into_iter()
        .map(|m| {
            let len = (m.w * m.h * 4) as usize;
            let ov = Overlay {
                rgba: rgba[at..at + len].to_vec(),
                w: m.w,
                h: m.h,
                x: m.x,
                y: m.y,
                anim_in: m.anim_in,
                anim_out: m.anim_out,
                in_ms: m.in_ms,
                out_ms: m.out_ms,
            };
            at += len;
            ov
        })
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut p = state.project.lock();
        p.overlays(&sel, &ovs);
        view(&p, None, None)
    })
    .await
    .map_err(err)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings = settings::load();
    let mut project = Project::default();
    project.undo_enabled = settings.enable_undo;
    tauri::Builder::default()
        // 이미 켜져 있으면 새로 띄우지 않고 지금 보이는 창을 앞으로 가져온다 (가장 먼저 등록해야 한다)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let target = app
                .get_webview_window("recorder")
                .filter(|r| r.is_visible().unwrap_or(false))
                .or_else(|| app.get_webview_window("main"));
            if let Some(w) = target {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, sc, ev| {
                    if ev.state == ShortcutState::Pressed {
                        on_hotkey(app, sc);
                    }
                })
                .build(),
        )
        .manage(AppState {
            project: Mutex::new(project),
            append_busy: AtomicBool::new(false),
            settings: Mutex::new(settings),
            recorder: Mutex::new(None),
            hole: Mutex::new(Rect { x: 0, y: 0, w: 0, h: 0 }),
            full_capture: Mutex::new(None),
            hotkeys: Mutex::new((None, None)),
            ctrl_prtsc_hotkey: Mutex::new(None),
            startup_notice: Mutex::new(None),
            snapshot_hotkey: Mutex::new(false),
            recovery_io: Mutex::new(false),
            recovery_stop: std::sync::atomic::AtomicBool::new(false),
        })
        .register_asynchronous_uri_scheme_protocol("frame", |ctx, req, responder| {
            let app = ctx.app_handle().clone();
            let path = req.uri().path().to_string();
            rayon::spawn(move || {
                let resp = match frame_png(&app, &path) {
                    Some(png) => tauri::http::Response::builder()
                        .header("Content-Type", "image/png")
                        // 픽셀이 바뀌면 id가 바뀌므로 오래 캐시해도 된다
                        .header("Cache-Control", "max-age=31536000, immutable")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(png),
                    None => tauri::http::Response::builder().status(404).body(vec![]),
                };
                responder.respond(resp.unwrap());
            });
        })
        .setup(|app| {
            let h = app.handle().clone();
            if let Err(e) = register_hotkeys(&h) {
                // 이 시점에는 화면이 아직 알림을 받을 준비가 안 됐으니 기억해 둔다
                eprintln!("{e}");
                *h.state::<AppState>().startup_notice.lock() = Some(e);
            }
            let on_top = h.state::<AppState>().settings.lock().always_on_top;
            if let Ok(r) = recorder_window(&h) {
                let _ = r.set_always_on_top(on_top);
            }
            recovery::adopt_leftover(&recovery::root());
            let a = h.clone();
            std::thread::Builder::new().name("opencam-autosave".into()).spawn(move || autosave_loop(a))?;
            Ok(())
        })
        .on_window_event(|window, event| {
            let app = window.app_handle();
            match (window.label(), event) {
                // 녹화 중에 틀을 옮기면 녹화 영역도 따라간다
                ("recorder", tauri::WindowEvent::Moved(_)) => {
                    let state = app.state::<AppState>();
                    if let Some(r) = state.recorder.lock().as_ref()
                        && let Ok(rect) = capture_rect(app)
                    {
                        r.move_region(rect);
                    }
                }
                // 녹화 틀을 닫으면 편집 창으로 돌아간다 (녹화 중에는 닫지 않는다)
                ("recorder", tauri::WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    if app.state::<AppState>().recorder.lock().is_none() {
                        show_editor(app);
                    }
                }
                ("selector", tauri::WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    restore_after_area(app, None);
                }
                // 편집 창을 닫으면 화면에 물어보고(저장 안 한 내용) 프로그램을 끝낸다
                ("main", tauri::WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    let _ = window.emit("close-requested", ());
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_hole,
            monitors,
            resize_mouse_down,
            start_magnifier,
            take_startup_notice,
            recovery_list,
            recovery_thumb,
            recovery_open,
            recovery_discard,
            get_full_capture,
            set_full_capture,
            capture_windows,
            capture_window_bounds,
            begin_area_selection,
            finish_area_selection,
            rec_start,
            rec_stop,
            rec_pause,
            rec_status,
            rec_snapshot_global_active,
            rec_save_image,
            show_recorder,
            quit_app,
            get_settings,
            set_settings,
            project_view,
            edit,
            trim_bounds,
            default_save_path,
            pick_save_path,
            pick_folder,
            export_conflicts,
            export_project,
            estimate_size,
            has_ffmpeg,
            reveal_file,
            delete_file,
            filter_preview,
            apply_overlay,
            zoom_preview,
            pick_open_files,
            probe_video,
            video_frame,
            import_files,
            import_video,
            append_files,
            new_blank,
            make_collage,
            list_files,
            rename_file,
            file_bytes,
            copy_frame_image,
            paste_clipboard_image,
            clipboard_image_png,
            save_frame_image,
            export_frames,
            pick_save_file,
            pick_file,
            export_settings,
            import_settings,
            summary_preview,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_rejects_results_from_another_project() {
        let mut p = Project::default();
        p.load(vec![]);
        let started = p.generation;
        assert!(check_append_generation(started, p.generation).is_ok());
        // 내용·크기가 같아도 새로 연 작업이면 지난 결과를 받지 않는다. 비우기도 같다.
        p.load(vec![]);
        assert!(check_append_generation(started, p.generation).is_err());
        let next = p.generation;
        p.load(vec![]);
        assert!(check_append_generation(next, p.generation).is_err());
    }

    #[test]
    fn append_rejects_overlapping_calls_and_releases_on_error() {
        let busy = AtomicBool::new(false);
        let permit = AppendPermit::acquire(&busy).unwrap();
        assert!(AppendPermit::acquire(&busy).is_err());
        drop(permit);
        let fail = || -> CmdResult<()> {
            let _permit = AppendPermit::acquire(&busy)?;
            Err("파일을 읽지 못했습니다".into())
        };
        assert!(fail().is_err());
        assert!(!busy.load(Ordering::SeqCst));
        assert!(AppendPermit::acquire(&busy).is_ok());
    }

    #[test]
    fn ctrl_prtsc_shortcut_parses_and_is_disabled_for_old_settings() {
        let shortcut: Shortcut = "Ctrl+PrintScreen".parse().unwrap();
        let alternate: Shortcut = "Control+PrintScreen".parse().unwrap();
        assert_eq!(shortcut, alternate);
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.hotkey_ctrl_prtsc);
    }

    #[test]
    fn selection_bounds_handle_negative_monitors_and_overflow() {
        let desktop = Rect { x: -1920, y: -200, w: 4480, h: 1640 };
        assert!(area_inside_desktop(Rect { x: -1800, y: -100, w: 400, h: 300 }, desktop));
        assert!(area_inside_desktop(Rect { x: -100, y: 0, w: 300, h: 200 }, desktop));
        assert!(!area_inside_desktop(Rect { x: -1921, y: 0, w: 20, h: 20 }, desktop));
        assert!(!area_inside_desktop(Rect { x: 2550, y: 0, w: 20, h: 20 }, desktop));
        assert!(!area_inside_desktop(Rect { x: 0, y: 0, w: 15, h: 20 }, desktop));
        assert!(!area_inside_desktop(Rect { x: i32::MAX, y: 0, w: i32::MAX, h: 20 }, desktop));
    }

    #[test]
    fn selection_must_not_reach_gaps_between_monitors() {
        // 왼쪽 1920×1080, 오른쪽에 더 큰 2560×1440: 왼쪽 모니터 아래(y 1080~1440)는 화면이 없다
        let monitors = [Rect { x: 0, y: 0, w: 1920, h: 1080 }, Rect { x: 1920, y: 0, w: 2560, h: 1440 }];
        assert!(area_on_monitors(Rect { x: 100, y: 100, w: 400, h: 300 }, &monitors));
        assert!(area_on_monitors(Rect { x: 1800, y: 100, w: 400, h: 300 }, &monitors), "두 모니터에 걸쳐도 다 덮임");
        assert!(!area_on_monitors(Rect { x: 100, y: 1000, w: 400, h: 300 }, &monitors), "빈 구석에 걸침");
        // 복제 화면(같은 사각형 두 번)은 한 번만 센다
        let mirrored = [monitors[0], monitors[0]];
        assert!(!area_on_monitors(Rect { x: 1800, y: 0, w: 400, h: 300 }, &mirrored));
    }

    #[test]
    fn recording_png_preserves_pixels_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("opencam_snapshot_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("snapshot.png");
        let rgba = [12, 34, 56, 78, 90, 123, 145, 255];
        write_new_recording_png(&path, 2, 1, &rgba).unwrap();
        assert_eq!(image::open(&path).unwrap().to_rgba8().as_raw(), &rgba);
        assert!(write_new_recording_png(&path, 2, 1, &[0; 8]).is_err());
        assert_eq!(image::open(&path).unwrap().to_rgba8().as_raw(), &rgba);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn autosave_follows_the_setting_without_waiting_for_an_edit() {
        use std::time::Duration;
        let long = Duration::from_secs(10);
        // 바뀐 판은 3초 동안 더 안 바뀌어야 저장
        assert!(!autosave_due(Some((4, true)), (5, true), Duration::from_secs(1)));
        assert!(autosave_due(Some((4, true)), (5, true), long));
        assert!(!autosave_due(Some((5, true)), (5, true), long));
        // 꺼 둔 채 편집하다가 켜면, 다음 편집을 기다리지 않고 지금 작업을 저장한다
        assert!(autosave_due(Some((5, false)), (5, true), long));
        // 끄면 바로 지운다
        assert!(autosave_due(Some((5, true)), (5, false), long));
    }

    #[test]
    fn export_settings_writes_the_given_values() {
        // 설정 창에서 저장하지 않고 고친 값을 내보낸다 (지금 쓰는 설정을 거치지 않는다)
        let path = std::env::temp_dir().join(format!("opencam_export_settings_{}.json", std::process::id()));
        let s = Settings { fps: 23, ..Settings::default() };
        export_settings(path.to_string_lossy().into_owned(), s).unwrap();
        let back: Settings = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(back.fps, 23);
    }

    #[test]
    fn rename_keeps_extension_and_refuses_bad_names() {
        let dir = std::env::temp_dir().join(format!("opencam_rename_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.gif");
        std::fs::write(&a, b"x").unwrap();
        // 확장자를 안 쓰면 원래 확장자를 붙인다
        let b = rename_file(a.to_string_lossy().into_owned(), "새 이름".into()).unwrap();
        assert!(b.ends_with("새 이름.gif"));
        assert!(!a.exists());
        // 이름에 점이 있어도 확장자가 아니면 원래 확장자를 붙인다 (예전에는 확장자 없는 파일이 되어 목록에서 사라졌다)
        let dated = rename_file(b.clone(), "2026.10.29".into()).unwrap();
        assert!(dated.ends_with("2026.10.29.gif"));
        let b = rename_file(dated, "새 이름".into()).unwrap();
        // 쓸 수 없는 글자, 이미 있는 이름은 거절
        assert!(rename_file(b.clone(), "a/b".into()).is_err());
        std::fs::write(dir.join("c.gif"), b"y").unwrap();
        assert!(rename_file(b.clone(), "c".into()).is_err());
        // 목록에는 지원하는 파일만
        std::fs::write(dir.join("memo.txt"), b"z").unwrap();
        let names: Vec<String> =
            list_files(dir.to_string_lossy().into_owned()).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names.len(), 2);
        assert!(names.iter().all(|n| n.ends_with(".gif")));
        // 없는 폴더는 빈 목록
        assert!(list_files(dir.join("없음").to_string_lossy().into_owned()).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

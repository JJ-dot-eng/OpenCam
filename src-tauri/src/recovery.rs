//! 작업 자동 저장·복구.
//! 편집 중인 프레임을 압축된 그대로 `%LOCALAPPDATA%\OpenCam\recovery\current`에 써 둔다.
//! 정상 종료하면 지우므로, 켤 때 남아 있으면 비정상 종료한 것이다. 그 폴더는 시각 이름으로 옮겨 복구 목록에 보여 준다.

use crate::frame::Frame;
use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MANIFEST: &str = "manifest.json";
const CURRENT: &str = "current";
/// 복구 목록에 남겨 두는 작업 수
const KEEP: usize = 3;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    app_version: String,
    /// 픽셀 파일 이름 앞의 실행 이름 (0.1.4에는 없음)
    #[serde(default)]
    session: String,
    /// 마지막으로 저장한 시각 (Unix ms)
    saved_at: i64,
    frames: Vec<FrameEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameEntry {
    /// `frames/<session>-<file>.lz4`
    file: u64,
    width: u32,
    height: u32,
    delay_ms: u32,
}

/// 복구할 수 있는 작업 하나 (화면에 보여 줄 정보)
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub saved_at: i64,
    pub frames: usize,
    pub width: u32,
    pub height: u32,
    pub total_ms: u64,
}

/// 이번 실행의 이름. 픽셀 파일 이름 앞에 붙인다. 프레임 번호는 켤 때마다 1부터라서,
/// 지난 실행에서 지우지 못한 같은 번호 파일을 이번 프레임으로 잘못 쓰지 않게 한다.
fn session() -> &'static str {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| format!("{}-{}", std::process::id(), chrono::Local::now().timestamp_millis()))
}

/// 픽셀 파일 이름(확장자 빼고). 0.1.4가 저장한 작업은 실행 이름 없이 프레임 번호뿐이다.
fn frame_stem(session: &str, id: u64) -> String {
    if session.is_empty() { id.to_string() } else { format!("{session}-{id}") }
}

pub fn root() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenCam").join("recovery")
}

pub fn current(root: &Path) -> PathBuf {
    root.join(CURRENT)
}

/// 프레임 목록을 dir에 저장한다. 픽셀 파일은 프레임 번호마다 한 번만 쓰고,
/// 목록 파일은 다 쓴 뒤 바꿔치기해서 쓰다가 끊겨도 이전 목록이 남는다. 그다음 안 쓰는 픽셀 파일을 지운다.
/// 쓰다 실패하면(디스크 가득 참 등) 이번에 새로 쓴 파일은 지운다. 이전 목록은 그 파일들을 쓰지 않는다.
/// stop이 켜지면 프레임 사이에서 그만두고 오류를 돌려준다 (실패와 같이 정리한다).
pub fn save(dir: &Path, frames: &[Frame], stop: &AtomicBool) -> Result<()> {
    let fdir = dir.join("frames");
    std::fs::create_dir_all(&fdir)?;
    let mut written = vec![];
    if let Err(e) = write_frames(dir, &fdir, frames, stop, &mut written) {
        for p in written {
            let _ = std::fs::remove_file(p);
        }
        return Err(e);
    }
    let used: HashSet<String> = frames.iter().map(|f| format!("{}.lz4", frame_stem(session(), f.id))).collect();
    for e in std::fs::read_dir(&fdir)?.flatten() {
        if !used.contains(&*e.file_name().to_string_lossy()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    Ok(())
}

/// 이번 저장에서 새로 쓸 양: 아직 dir에 없는 픽셀 파일 크기의 합 (같은 프레임 번호는 한 번만)
fn pending_bytes(dir: &Path, frames: &[Frame]) -> u64 {
    let fdir = dir.join("frames");
    let mut seen = HashSet::new();
    frames
        .iter()
        .filter(|f| seen.insert(f.id) && !fdir.join(format!("{}.lz4", frame_stem(session(), f.id))).exists())
        .map(|f| f.packed_bytes() as u64)
        .sum()
}

/// 남은 공간 free에 pending만큼 쓰고도 min_free가 남는가. 남은 공간을 모르면 쓴다.
fn has_room(free: Option<u64>, pending: u64, min_free: u64) -> bool {
    free.is_none_or(|f| f >= pending.saturating_add(min_free))
}

/// dir에 frames를 저장하고도 디스크 공간이 min_free 넘게 남는가
pub fn room_for(dir: &Path, frames: &[Frame], min_free: u64) -> bool {
    has_room(free_space(dir), pending_bytes(dir, frames), min_free)
}

/// 새 픽셀 파일과 목록 파일을 쓴다. 새로 쓴(또는 쓰다 만) 파일 경로는 written에 모은다.
fn write_frames(
    dir: &Path,
    fdir: &Path,
    frames: &[Frame],
    stop: &AtomicBool,
    written: &mut Vec<PathBuf>,
) -> Result<()> {
    for f in frames {
        if stop.load(Ordering::SeqCst) {
            bail!("자동 저장을 멈췄습니다");
        }
        let stem = frame_stem(session(), f.id);
        let path = fdir.join(format!("{stem}.lz4"));
        if !path.exists() {
            let tmp = fdir.join(format!("{stem}.tmp"));
            written.push(tmp.clone());
            std::fs::write(&tmp, f.packed())?;
            written.push(path.clone());
            std::fs::rename(&tmp, &path)?;
        }
    }
    let m = Manifest {
        app_version: env!("CARGO_PKG_VERSION").into(),
        session: session().into(),
        saved_at: chrono::Local::now().timestamp_millis(),
        frames: frames
            .iter()
            .map(|f| FrameEntry { file: f.id, width: f.width, height: f.height, delay_ms: f.delay_ms })
            .collect(),
    };
    let tmp = dir.join("manifest.tmp");
    written.push(tmp.clone());
    std::fs::write(&tmp, serde_json::to_vec(&m)?)?;
    std::fs::rename(&tmp, dir.join(MANIFEST))?;
    Ok(())
}

pub fn clear(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// 켤 때 부른다. 지난번에 지우지 못한 작업(비정상 종료)을 복구 목록으로 옮기고, 오래된 것은 지운다.
pub fn adopt_leftover(root: &Path) {
    let cur = current(root);
    match read_manifest(&cur) {
        Ok(m) if !m.frames.is_empty() => {
            let name = chrono::DateTime::from_timestamp_millis(m.saved_at)
                .map(|t| t.with_timezone(&chrono::Local).format("%Y%m%d-%H%M%S").to_string())
                .unwrap_or_else(|| "unknown".into());
            let mut to = root.join(&name);
            let mut n = 2;
            while to.exists() {
                to = root.join(format!("{name}-{n}"));
                n += 1;
            }
            // 옮기지 못하면(안의 파일을 다른 프로그램이 열고 있음 등) 복사해 둔다. 복구할 작업을 지우지 않는다.
            if std::fs::rename(&cur, &to).is_err() {
                if copy_entry(&cur, &to).is_ok() {
                    clear(&cur);
                } else {
                    clear(&to);
                }
            }
        }
        _ => clear(&cur),
    }
    let mut all = list(root);
    for old in all.split_off(KEEP.min(all.len())) {
        clear(&root.join(old.id));
    }
}

/// 작업 폴더를 복사한다. 목록 파일은 맨 나중에 복사해서, 도중에 실패한 폴더는 복구 목록에 나오지 않는다.
fn copy_entry(from: &Path, to: &Path) -> Result<()> {
    let fdir = to.join("frames");
    std::fs::create_dir_all(&fdir)?;
    for e in std::fs::read_dir(from.join("frames"))? {
        let e = e?;
        std::fs::copy(e.path(), fdir.join(e.file_name()))?;
    }
    std::fs::copy(from.join(MANIFEST), to.join(MANIFEST))?;
    Ok(())
}

/// 복구할 수 있는 작업들 (최근 것부터)
pub fn list(root: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(root) else { return vec![] };
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter(|e| e.path().is_dir() && e.file_name() != CURRENT)
        .filter_map(|e| {
            let m = read_manifest(&e.path()).ok()?;
            let first = m.frames.first()?;
            Some(Entry {
                id: e.file_name().to_string_lossy().into_owned(),
                saved_at: m.saved_at,
                frames: m.frames.len(),
                width: first.width,
                height: first.height,
                total_ms: m.frames.iter().map(|f| f.delay_ms as u64).sum(),
            })
        })
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.saved_at));
    out
}

/// 복구 목록의 작업 폴더. 이름에 경로가 섞여 있으면 거절한다.
pub fn entry_dir(root: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty() || id == CURRENT || id.contains(['/', '\\', ':']) || id.starts_with('.') {
        bail!("잘못된 복구 작업 이름입니다");
    }
    Ok(root.join(id))
}

/// 저장해 둔 프레임을 읽는다. 하나라도 깨졌으면 오류.
pub fn load(dir: &Path) -> Result<Vec<Frame>> {
    let m = read_manifest(dir)?;
    let fdir = dir.join("frames");
    m.frames
        .par_iter()
        .map(|e| {
            let data = std::fs::read(fdir.join(format!("{}.lz4", frame_stem(&m.session, e.file))))
                .with_context(|| format!("프레임 파일이 없습니다 ({})", e.file))?;
            Frame::from_packed(e.width, e.height, e.delay_ms, data)
                .with_context(|| format!("프레임 파일이 깨졌습니다 ({})", e.file))
        })
        .collect()
}

/// 첫 프레임 (미리보기용)
pub fn first_frame(dir: &Path) -> Result<Frame> {
    let m = read_manifest(dir)?;
    let e = m.frames.first().context("프레임이 없습니다")?;
    let data = std::fs::read(dir.join("frames").join(format!("{}.lz4", frame_stem(&m.session, e.file))))?;
    Frame::from_packed(e.width, e.height, e.delay_ms, data).context("프레임 파일이 깨졌습니다")
}

fn read_manifest(dir: &Path) -> Result<Manifest> {
    Ok(serde_json::from_slice(&std::fs::read(dir.join(MANIFEST))?)?)
}

/// 남은 디스크 공간 (바이트). 알 수 없으면 None
pub fn free_space(dir: &Path) -> Option<u64> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::HSTRING;
    // 아직 없는 폴더면 있는 상위 폴더로 잰다
    let mut p = dir;
    while !p.exists() {
        p = p.parent()?;
    }
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(p.as_os_str()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

#[cfg(test)]
mod tests {
    use super::*;

    static GO: AtomicBool = AtomicBool::new(false);

    fn frame(v: u8, delay: u32) -> Frame {
        Frame::from_rgba(2, 1, &[v, 0, 0, 255, v, 0, 0, 255], delay)
    }

    fn file_of(dir: &Path, f: &Frame) -> PathBuf {
        dir.join("frames").join(format!("{}.lz4", frame_stem(session(), f.id)))
    }

    fn temp_root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("opencam_recovery_{name}_{}", std::process::id()));
        clear(&d);
        d
    }

    #[test]
    fn save_and_load_round_trip_and_cleanup() {
        let root = temp_root("roundtrip");
        let cur = current(&root);
        let a = frame(1, 10);
        let b = frame(2, 20);
        save(&cur, &[a.clone(), b.clone(), a.with_delay(30)], &GO).unwrap();
        // 같은 픽셀(시간만 바꾼 복사본)은 파일 하나
        assert_eq!(std::fs::read_dir(cur.join("frames")).unwrap().count(), 2);
        let back = load(&cur).unwrap();
        assert_eq!(back.iter().map(|f| f.delay_ms).collect::<Vec<_>>(), vec![10, 20, 30]);
        assert_eq!(back[1].rgba(), b.rgba());
        // b를 빼면 b 파일도 지운다
        save(&cur, &[a.clone()], &GO).unwrap();
        assert_eq!(std::fs::read_dir(cur.join("frames")).unwrap().count(), 1);
        clear(&root);
    }

    #[test]
    fn leftover_becomes_entry_and_old_ones_are_pruned() {
        let root = temp_root("leftover");
        // 비정상 종료로 남은 작업 5개
        for i in 0..5 {
            let cur = current(&root);
            save(&cur, &[frame(i, 100), frame(i + 1, 50)], &GO).unwrap();
            adopt_leftover(&root);
            assert!(!cur.exists());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let entries = list(&root);
        assert_eq!(entries.len(), KEEP);
        assert_eq!((entries[0].frames, entries[0].total_ms, entries[0].width), (2, 150, 2));
        assert!(entries.windows(2).all(|w| w[0].saved_at >= w[1].saved_at));
        let dir = entry_dir(&root, &entries[0].id).unwrap();
        assert_eq!(load(&dir).unwrap().len(), 2);
        assert_eq!(first_frame(&dir).unwrap().rgba()[0], 4);
        // 빈 작업이나 목록 파일이 없는 폴더는 남기지 않는다
        std::fs::create_dir_all(current(&root)).unwrap();
        adopt_leftover(&root);
        assert!(!current(&root).exists());
        assert_eq!(list(&root).len(), KEEP);
        clear(&root);
    }

    #[test]
    fn files_left_by_an_earlier_run_are_not_taken_for_new_frames() {
        // 프레임 번호는 켤 때마다 1부터라, 지난 실행에서 지우지 못한 같은 번호 파일이 있을 수 있다.
        // 그 파일을 이번 프레임으로 잘못 쓰면 복구할 때 옛 그림이 나온다.
        let root = temp_root("stale");
        let cur = current(&root);
        let a = frame(10, 100);
        std::fs::create_dir_all(cur.join("frames")).unwrap();
        std::fs::write(cur.join("frames").join(format!("{}.lz4", a.id)), frame(200, 100).packed()).unwrap();
        save(&cur, std::slice::from_ref(&a), &GO).unwrap();
        assert_eq!(load(&cur).unwrap()[0].rgba()[0], 10);
        // 남은 옛 파일은 저장하면서 정리한다
        assert_eq!(std::fs::read_dir(cur.join("frames")).unwrap().count(), 1);
        clear(&root);
    }

    #[test]
    fn work_saved_by_0_1_4_still_loads() {
        // 0.1.4는 목록에 session이 없고 픽셀 파일 이름이 프레임 번호뿐이었다
        let root = temp_root("old");
        let cur = current(&root);
        let a = frame(7, 100);
        save(&cur, std::slice::from_ref(&a), &GO).unwrap();
        let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(cur.join(MANIFEST)).unwrap()).unwrap();
        m.as_object_mut().unwrap().remove("session");
        std::fs::write(cur.join(MANIFEST), serde_json::to_vec(&m).unwrap()).unwrap();
        std::fs::rename(file_of(&cur, &a), cur.join("frames").join(format!("{}.lz4", a.id))).unwrap();
        assert_eq!(load(&cur).unwrap()[0].rgba()[0], 7);
        assert_eq!(first_frame(&cur).unwrap().rgba()[0], 7);
        clear(&root);
    }

    #[test]
    fn leftover_is_kept_even_if_it_cannot_be_moved() {
        // 켤 때 남은 작업 폴더를 옮기지 못하면(안의 파일을 다른 프로그램이 열고 있음 등) 지우지 말고 복사해 둔다
        use std::os::windows::fs::OpenOptionsExt;
        let root = temp_root("locked");
        let cur = current(&root);
        let a = frame(3, 100);
        save(&cur, std::slice::from_ref(&a), &GO).unwrap();
        // 읽기만 함께 쓰게 열어 두면 폴더를 옮길 수 없다
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(file_of(&cur, &a)).unwrap();
        adopt_leftover(&root);
        let entries = list(&root);
        assert_eq!(entries.len(), 1);
        assert_eq!(load(&entry_dir(&root, &entries[0].id).unwrap()).unwrap()[0].rgba()[0], 3);
        drop(held);
        clear(&root);
    }

    #[test]
    fn corrupted_or_missing_frames_are_errors_not_crashes() {
        let root = temp_root("corrupt");
        let cur = current(&root);
        let a = frame(1, 10);
        save(&cur, &[a.clone()], &GO).unwrap();
        std::fs::write(file_of(&cur, &a), b"garbage").unwrap();
        assert!(load(&cur).is_err());
        std::fs::remove_file(file_of(&cur, &a)).unwrap();
        assert!(load(&cur).is_err());
        clear(&root);
    }

    #[test]
    fn failed_save_removes_only_the_files_it_just_wrote() {
        // 디스크가 차서 쓰다 실패하면, 이번에 새로 쓴 파일은 지워 공간을 돌려준다 (예전 저장본은 그대로)
        let root = temp_root("failed");
        let cur = current(&root);
        let (old, a, b) = (frame(1, 10), frame(2, 10), frame(3, 10));
        save(&cur, std::slice::from_ref(&old), &GO).unwrap();
        // b를 쓸 자리에 폴더를 만들어 두어 쓰기가 실패하게 한다
        std::fs::create_dir_all(cur.join("frames").join(format!("{}.tmp", frame_stem(session(), b.id)))).unwrap();
        assert!(save(&cur, &[old.clone(), a.clone(), b.clone()], &GO).is_err());
        assert!(!file_of(&cur, &a).exists());
        assert!(file_of(&cur, &old).exists());
        assert_eq!(load(&cur).unwrap().len(), 1);
        clear(&root);
    }

    #[test]
    fn cancelled_save_stops_without_leaving_new_files() {
        // 끝내기·녹화 틀로 돌아가기가 큰 자동 저장을 끝까지 기다리지 않게, 멈춤 신호를 보면 바로 그만둔다
        let root = temp_root("cancel");
        let cur = current(&root);
        let old = frame(1, 10);
        save(&cur, std::slice::from_ref(&old), &GO).unwrap();
        let new: Vec<Frame> = (2..50).map(|v| frame(v, 10)).collect();
        let stop = AtomicBool::new(true);
        assert!(save(&cur, &[vec![old.clone()], new.clone()].concat(), &stop).is_err());
        assert!(new.iter().all(|f| !file_of(&cur, f).exists()));
        assert_eq!(load(&cur).unwrap().len(), 1);
        clear(&root);
    }

    #[test]
    fn room_check_counts_what_is_still_to_be_written() {
        let root = temp_root("room");
        let cur = current(&root);
        let (a, b) = (frame(1, 10), frame(2, 10));
        save(&cur, std::slice::from_ref(&a), &GO).unwrap();
        // 이미 쓴 a와, 시간만 바꾼 b 복사본은 세지 않는다
        assert_eq!(pending_bytes(&cur, &[a.clone(), b.clone(), b.with_delay(50)]), b.packed_bytes() as u64);
        clear(&root);
        const GB: u64 = 1024 * 1024 * 1024;
        // 남은 공간이 1.5GB라도 1GB를 써야 하면 1GB 여유가 안 남으니 쓰지 않는다
        assert!(!has_room(Some(GB * 3 / 2), GB, GB));
        assert!(has_room(Some(GB * 3), GB, GB));
        assert!(has_room(None, GB, GB));
    }

    #[test]
    fn entry_names_cannot_escape_the_folder() {
        let root = PathBuf::from("r");
        for bad in ["", "current", "..", "../x", "a\\b", "c:x", ".hidden"] {
            assert!(entry_dir(&root, bad).is_err(), "{bad}");
        }
        assert!(entry_dir(&root, "20261030-101500").is_ok());
    }
}

//! 작업 자동 저장·복구.
//! 편집 중인 프레임을 압축된 그대로 `%LOCALAPPDATA%\OpenCam\recovery\current`에 써 둔다.
//! 정상 종료하면 지우므로, 켤 때 남아 있으면 비정상 종료한 것이다. 그 폴더는 시각 이름으로 옮겨 복구 목록에 보여 준다.

use crate::frame::Frame;
use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "manifest.json";
const CURRENT: &str = "current";
/// 복구 목록에 남겨 두는 작업 수
const KEEP: usize = 3;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    app_version: String,
    /// 마지막으로 저장한 시각 (Unix ms)
    saved_at: i64,
    frames: Vec<FrameEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameEntry {
    /// `frames/<file>.lz4`
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

pub fn root() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenCam").join("recovery")
}

pub fn current(root: &Path) -> PathBuf {
    root.join(CURRENT)
}

/// 프레임 목록을 dir에 저장한다. 픽셀 파일은 프레임 번호마다 한 번만 쓰고,
/// 목록 파일은 다 쓴 뒤 바꿔치기해서 쓰다가 끊겨도 이전 목록이 남는다. 그다음 안 쓰는 픽셀 파일을 지운다.
pub fn save(dir: &Path, frames: &[Frame]) -> Result<()> {
    let fdir = dir.join("frames");
    std::fs::create_dir_all(&fdir)?;
    for f in frames {
        let path = fdir.join(format!("{}.lz4", f.id));
        if !path.exists() {
            let tmp = fdir.join(format!("{}.tmp", f.id));
            std::fs::write(&tmp, f.packed())?;
            std::fs::rename(&tmp, &path)?;
        }
    }
    let m = Manifest {
        app_version: env!("CARGO_PKG_VERSION").into(),
        saved_at: chrono::Local::now().timestamp_millis(),
        frames: frames
            .iter()
            .map(|f| FrameEntry { file: f.id, width: f.width, height: f.height, delay_ms: f.delay_ms })
            .collect(),
    };
    let tmp = dir.join("manifest.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&m)?)?;
    std::fs::rename(&tmp, dir.join(MANIFEST))?;
    let used: HashSet<String> = frames.iter().map(|f| format!("{}.lz4", f.id)).collect();
    for e in std::fs::read_dir(&fdir)?.flatten() {
        if !used.contains(&*e.file_name().to_string_lossy()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
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
            if std::fs::rename(&cur, &to).is_err() {
                clear(&cur);
            }
        }
        _ => clear(&cur),
    }
    let mut all = list(root);
    for old in all.split_off(KEEP.min(all.len())) {
        clear(&root.join(old.id));
    }
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
            let data = std::fs::read(fdir.join(format!("{}.lz4", e.file)))
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
    let data = std::fs::read(dir.join("frames").join(format!("{}.lz4", e.file)))?;
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

    fn frame(v: u8, delay: u32) -> Frame {
        Frame::from_rgba(2, 1, &[v, 0, 0, 255, v, 0, 0, 255], delay)
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
        save(&cur, &[a.clone(), b.clone(), a.with_delay(30)]).unwrap();
        // 같은 픽셀(시간만 바꾼 복사본)은 파일 하나
        assert_eq!(std::fs::read_dir(cur.join("frames")).unwrap().count(), 2);
        let back = load(&cur).unwrap();
        assert_eq!(back.iter().map(|f| f.delay_ms).collect::<Vec<_>>(), vec![10, 20, 30]);
        assert_eq!(back[1].rgba(), b.rgba());
        // b를 빼면 b 파일도 지운다
        save(&cur, &[a.clone()]).unwrap();
        assert_eq!(std::fs::read_dir(cur.join("frames")).unwrap().count(), 1);
        clear(&root);
    }

    #[test]
    fn leftover_becomes_entry_and_old_ones_are_pruned() {
        let root = temp_root("leftover");
        // 비정상 종료로 남은 작업 5개
        for i in 0..5 {
            let cur = current(&root);
            save(&cur, &[frame(i, 100), frame(i + 1, 50)]).unwrap();
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
    fn corrupted_or_missing_frames_are_errors_not_crashes() {
        let root = temp_root("corrupt");
        let cur = current(&root);
        let a = frame(1, 10);
        save(&cur, &[a.clone()]).unwrap();
        std::fs::write(cur.join("frames").join(format!("{}.lz4", a.id)), b"garbage").unwrap();
        assert!(load(&cur).is_err());
        std::fs::remove_file(cur.join("frames").join(format!("{}.lz4", a.id))).unwrap();
        assert!(load(&cur).is_err());
        clear(&root);
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

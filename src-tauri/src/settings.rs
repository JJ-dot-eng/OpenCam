//! 설정.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    // 일반
    pub always_on_top: bool,
    pub output_dir: String,
    pub file_name_rule: String,
    pub do_not_ask_discard_recording: bool,
    pub do_not_ask_gif_50fps: bool,
    pub delete_to_recycle_bin: bool,

    // 녹화
    pub fps: u32,
    pub capture_cursor: bool,
    pub cursor_highlight: bool,
    pub cursor_highlight_color: u32,
    pub cursor_click_fx: bool,
    pub left_click_color: u32,
    pub right_click_color: u32,
    pub skip_same_frame: bool,
    pub max_rec_sec: u32,
    pub rec_no_time_limit: bool,
    pub rec_global_snapshot_hotkey: bool,
    pub use_magnifier: bool,
    pub use_semi_transparency: bool,
    pub rec_toolbar_bottom: bool,
    pub rec_arrow_key_accel: bool,
    pub ignore_arrow_keys_while_recording: bool,
    pub goto_edit_after_recording: bool,
    pub size_presets: Vec<[u32; 2]>,
    pub saved_positions: Vec<Option<[i32; 4]>>,
    pub hotkey_record: String,
    pub hotkey_pause: String,
    pub hotkey_ctrl_prtsc: bool,
    pub max_edit_memory_mb: u32,
    /// 녹화 중 MP4로 바로 저장 (소리 없음)
    pub rec_direct_save: bool,
    pub rec_direct_save_quality: u8,
    pub rec_direct_save_open_folder: bool,
    /// 메모리 한도에 닿으면 편집용 프레임은 버리고 MP4 저장은 계속
    pub rec_direct_save_drop_frames: bool,
    pub rec_direct_save_no_time_limit: bool,

    // 편집
    pub enable_undo: bool,
    pub frame_time_3digit: bool,
    pub jpg_export_quality: u8,

    // GIF
    pub gif_quality: u8,
    pub gif_dither: bool,
    pub gif_remove_similar_pixels: bool,
    pub gif_finite_loop: bool,
    pub gif_loop_count: u16,

    // WebP
    pub webp_quality: u8,
    pub webp_method: u8,
    pub webp_lossless: bool,
    pub webp_finite_loop: bool,
    pub webp_loop_count: u16,
    /// 아이폰 호환: 모든 프레임을 통째로(키프레임) 저장해 아이폰 사파리에서 깨지지 않게
    pub webp_iphone_compat: bool,

    // APNG
    pub apng_finite_loop: bool,
    pub apng_loop_count: u16,
    /// 최적화된 256색으로 저장 (끄면 24비트 전체 색)
    pub apng_256_colors: bool,

    // AVIF
    pub avif_quality: u8,

    // WebM / MP4
    pub webm_quality: u8,
    pub mp4_quality: u8,
    pub mp4_replace_transparent: bool,
    pub transparent_replace_color: u32,

    // 저장 시 파일 크기 줄이기
    pub limit_fps: bool,
    pub limit_fps_value: u32,
    pub skip_similar: bool,
    pub skip_similar_percent: u32,
    pub skip_similar_time_limit: bool,
    pub skip_similar_time_limit_ms: u32,
    pub resize_percent: u32,

    // 재생 진행 막대
    pub embed_progress: bool,
    pub embed_progress_color: u32,
    pub embed_progress_bg_color: u32,
    pub embed_progress_line_width: u32,
    pub embed_progress_min_sec: u32,

    // 프레임 줄이기 대화상자
    pub reduce_similarity: u32,
    pub reduce_tolerance: u32,
    pub reduce_time_limit: bool,
    pub reduce_time_limit_ms: u32,
    pub reduce_keep_speed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let pictures =
            dirs::picture_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")).join("OpenCam");
        Settings {
            always_on_top: true,
            output_dir: pictures.to_string_lossy().into_owned(),
            file_name_rule: "opencam_%Y%m%d_%H%M%S".into(),
            do_not_ask_discard_recording: false,
            do_not_ask_gif_50fps: false,
            delete_to_recycle_bin: true,

            fps: 60,
            capture_cursor: false,
            cursor_highlight: false,
            cursor_highlight_color: 0x00ffff, // BGR: 노랑
            cursor_click_fx: false,
            left_click_color: 0x0000ff,  // 빨강
            right_click_color: 0xff0000, // 파랑
            skip_same_frame: true,
            max_rec_sec: 60,
            rec_no_time_limit: false,
            rec_global_snapshot_hotkey: false,
            use_magnifier: true,
            use_semi_transparency: true,
            rec_toolbar_bottom: false,
            rec_arrow_key_accel: true,
            ignore_arrow_keys_while_recording: false,
            goto_edit_after_recording: false,
            size_presets: vec![
                [320, 240],
                [400, 300],
                [480, 360],
                [640, 360],
                [640, 480],
                [800, 450],
                [800, 600],
                [1024, 576],
                [1280, 720],
            ],
            saved_positions: vec![None; 4],
            hotkey_record: "F2".into(),
            hotkey_pause: "F3".into(),
            hotkey_ctrl_prtsc: false,
            max_edit_memory_mb: 4096,
            rec_direct_save: false,
            rec_direct_save_quality: 90,
            rec_direct_save_open_folder: true,
            rec_direct_save_drop_frames: true,
            rec_direct_save_no_time_limit: false,

            enable_undo: true,
            frame_time_3digit: false,
            jpg_export_quality: 90,

            gif_quality: 80,
            // 켜면 같은 화질에서 GIF가 커진다(가까운 색 고르기가 더 낫다, TODO.md).
            gif_dither: false,
            gif_remove_similar_pixels: true,
            gif_finite_loop: false,
            gif_loop_count: 1,

            webp_quality: 80,
            webp_method: 4,
            webp_lossless: false,
            webp_finite_loop: false,
            webp_loop_count: 1,
            webp_iphone_compat: true,

            apng_finite_loop: false,
            apng_loop_count: 1,
            apng_256_colors: true,

            avif_quality: 80,

            webm_quality: 80,
            mp4_quality: 80,
            mp4_replace_transparent: false,
            transparent_replace_color: 0xffffff,

            limit_fps: false,
            limit_fps_value: 20,
            skip_similar: false,
            skip_similar_percent: 20,
            skip_similar_time_limit: true,
            skip_similar_time_limit_ms: 100,
            resize_percent: 100,

            embed_progress: false,
            embed_progress_color: 0x0000ff,
            embed_progress_bg_color: 0x404040,
            embed_progress_line_width: 3,
            embed_progress_min_sec: 0,

            reduce_similarity: 20,
            reduce_tolerance: 20,
            reduce_time_limit: true,
            reduce_time_limit_ms: 100,
            reduce_keep_speed: true,
        }
    }
}

fn settings_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenCam").join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read_to_string(settings_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(s: &Settings) -> anyhow::Result<()> {
    let p = settings_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(s)?)?;
    Ok(())
}

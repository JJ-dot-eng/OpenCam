//! 프레임 한 장. 메모리를 아끼려고 RGBA 픽셀을 LZ4로 압축해서 들고 있다.
//! (1080p 프레임 하나가 8MB인데, 화면 녹화 내용은 보통 10~30배 줄어든다)

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// 화면에 보여줄 때 쓰는 고유 번호. 픽셀이 바뀌면 새 번호를 받는다(브라우저 캐시용).
pub fn new_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct Frame {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    /// 이 프레임을 보여주는 시간(ms)
    pub delay_ms: u32,
    packed: Arc<Vec<u8>>,
}

impl Frame {
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8], delay_ms: u32) -> Self {
        debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
        Frame { id: new_id(), width, height, delay_ms, packed: Arc::new(lz4_flex::compress_prepend_size(rgba)) }
    }

    pub fn rgba(&self) -> Vec<u8> {
        lz4_flex::decompress_size_prepended(&self.packed).expect("frame data is corrupted")
    }

    /// 픽셀은 그대로 두고 시간만 바꾼 복사본
    pub fn with_delay(&self, delay_ms: u32) -> Self {
        Frame { delay_ms, ..self.clone() }
    }

    /// 같은 픽셀을 다른 프레임으로 복제(붙여넣기·yoyo용). id만 새로 받는다.
    pub fn duplicate(&self) -> Self {
        Frame { id: new_id(), ..self.clone() }
    }

    pub fn packed_bytes(&self) -> usize {
        self.packed.len()
    }

    /// 픽셀 버퍼가 같은지 알아보는 값. 시간만 바꾼 복사본·되돌리기 기록은 같은 버퍼를 함께 쓴다.
    pub fn packed_key(&self) -> usize {
        Arc::as_ptr(&self.packed) as usize
    }

    /// 픽셀을 바꾼 새 프레임 (시간은 유지)
    pub fn map_pixels(&self, f: impl FnOnce(u32, u32, Vec<u8>) -> (u32, u32, Vec<u8>)) -> Self {
        let (w, h, px) = f(self.width, self.height, self.rgba());
        Frame::from_rgba(w, h, &px, self.delay_ms)
    }
}

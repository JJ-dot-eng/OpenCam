//! 프레임 한 장. 메모리를 아끼려고 RGBA 픽셀을 LZ4로 압축해서 들고 있다.
//! (1080p 프레임 하나가 8MB인데, 화면 녹화 내용은 보통 10~30배 줄어든다)

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

// 불러올 한 장은 image 크레이트의 기본 메모리 한도와 같은 512MiB까지 허용한다.
pub(crate) const MAX_FRAME_BYTES: usize = 512 * 1024 * 1024;

/// RGBA 버퍼 크기. 계산 중 정수 범위를 넘으면 None을 돌려준다.
pub fn rgba_len(width: u32, height: u32) -> Option<usize> {
    usize::try_from(width).ok()?.checked_mul(usize::try_from(height).ok()?)?.checked_mul(4)
}

/// 불러올 RGBA 버퍼 크기. 빈 그림이나 메모리 한도를 넘는 그림은 받지 않는다.
pub fn import_rgba_len(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    let len = rgba_len(width, height)?;
    (len <= MAX_FRAME_BYTES).then_some(len)
}

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
        debug_assert_eq!(rgba_len(width, height), Some(rgba.len()), "프레임 크기와 픽셀 데이터가 맞지 않습니다");
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

    /// 작업 자동 저장 파일에서 읽은 압축 픽셀로 만든다. 깨진 데이터면 None (나중에 풀다가 꺼지지 않게 미리 풀어 본다).
    pub fn from_packed(width: u32, height: u32, delay_ms: u32, packed: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let len = rgba_len(width, height)?;
        let stored_len = u32::from_le_bytes(packed.get(..4)?.try_into().ok()?) as usize;
        if stored_len != len {
            return None;
        }
        let px = lz4_flex::decompress_size_prepended(&packed).ok()?;
        (px.len() == len).then(|| Frame { id: new_id(), width, height, delay_ms, packed: Arc::new(packed) })
    }

    /// 압축된 픽셀 (작업 자동 저장용)
    pub fn packed(&self) -> &[u8] {
        &self.packed
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_size_is_checked_without_import_limits() {
        assert_eq!(rgba_len(1920, 1080), Some(1920 * 1080 * 4));
        assert_eq!(rgba_len(0, 1), Some(0));
        assert_eq!(rgba_len(1, 0), Some(0));
        assert_eq!(rgba_len(u32::MAX, u32::MAX), None);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(rgba_len(65535, 65535), Some(65535usize * 65535 * 4));
        #[cfg(target_pointer_width = "32")]
        assert_eq!(rgba_len(65535, 65535), None);
    }

    #[test]
    fn import_rgba_size_limits() {
        assert_eq!(import_rgba_len(1920, 1080), Some(1920 * 1080 * 4));
        assert_eq!(import_rgba_len(7680, 4320), Some(7680 * 4320 * 4));
        assert_eq!(import_rgba_len(16384, 8192), Some(MAX_FRAME_BYTES));
        assert_eq!(import_rgba_len(65535, 1), Some(65535 * 4));
        for (w, h) in [(16384, 8193), (32768, 32768), (65535, 65535), (u32::MAX, u32::MAX), (0, 1), (1, 0), (0, 0)] {
            assert_eq!(import_rgba_len(w, h), None, "{w}×{h}");
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "프레임 크기와 픽셀 데이터가 맞지 않습니다")]
    fn mismatched_rgba_is_rejected() {
        Frame::from_rgba(1, 1, &[0; 3], 100);
    }

    #[test]
    fn packed_size_is_checked_before_unpacking() {
        let frame = Frame::from_rgba(1, 1, &[1, 2, 3, 255], 100);
        assert_eq!(Frame::from_packed(1, 1, 100, frame.packed().to_vec()).unwrap().rgba(), [1, 2, 3, 255]);
        assert!(Frame::from_packed(65535, 65535, 100, vec![]).is_none());
        assert!(Frame::from_packed(1, 1, 100, u32::MAX.to_le_bytes().to_vec()).is_none());
        assert!(Frame::from_packed(0, 1, 100, lz4_flex::compress_prepend_size(&[])).is_none());
    }

    #[test]
    fn wide_frame_can_be_created_and_recovered() {
        let px = [1, 2, 3, 255].repeat(65535);
        let frame = Frame::from_rgba(65535, 1, &px, 100);
        let recovered = Frame::from_packed(65535, 1, 100, frame.packed().to_vec()).unwrap();
        assert_eq!(recovered.rgba(), px);
    }
}

//! 편집 중인 프레임 목록과 편집 명령, 되돌리기.

use crate::effects::{self, Filter, Overlay, TransitionKind, ZoomFx};
use crate::frame::Frame;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FrameInfo {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub delay_ms: u32,
}

#[derive(Default)]
pub struct Project {
    pub frames: Vec<Frame>,
    undo: Vec<Vec<Frame>>,
    /// 되돌린 상태들. 새 편집을 하면 비운다.
    redo: Vec<Vec<Frame>>,
    /// 처음 상태 (Ctrl+Alt+S로 바꿀 수 있음)
    initial: Vec<Frame>,
    clipboard: Vec<Frame>,
    pub undo_enabled: bool,
    pub dirty: bool,
    /// 프레임 목록이 바뀔 때마다 늘어나는 번호 (작업 자동 저장이 바뀐 것을 알아챈다)
    pub revision: u64,
    /// 영상 요약용 "앞 프레임과 얼마나 다른가" 점수. 프레임 id 목록이 같을 때만 다시 쓴다.
    change_scores: Option<(Vec<u64>, Vec<f32>)>,
}

/// 영상 요약 옵션. 기본값: 요약률 50%, 프레임당 최대 300ms, 프레임 시간 20%로
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SummaryOptions {
    /// 남길 프레임 비율(%)
    pub rate: u32,
    /// 프레임당 최대 시간(ms). 없으면 제한 안 함
    pub limit_ms: Option<u32>,
    /// 없으면 총 재생 시간 유지, 있으면 프레임 시간을 이 비율(%)로
    pub time_percent: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryPreview {
    pub frames: usize,
    pub total_ms: u64,
}

const MAX_UNDO: usize = 50;

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum ReduceMode {
    Even,
    Odd,
    Nth,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Rotate {
    Left,
    Right,
    Half,
    FlipH,
    FlipV,
}

impl Project {
    pub fn load(&mut self, frames: Vec<Frame>) {
        self.initial = frames.clone();
        self.frames = frames;
        self.undo.clear();
        self.redo.clear();
        self.dirty = false;
        self.revision += 1;
    }

    pub fn infos(&self) -> Vec<FrameInfo> {
        self.frames
            .iter()
            .map(|f| FrameInfo { id: f.id, width: f.width, height: f.height, delay_ms: f.delay_ms })
            .collect()
    }

    /// 편집에 쓰는 메모리(압축된 픽셀). 지금 프레임·되돌리기/다시 실행 기록·처음 상태·복사해 둔 것을 합치되,
    /// 같은 버퍼는 한 번만 센다.
    pub fn memory_bytes(&self) -> u64 {
        let mut seen = std::collections::HashSet::new();
        let history = self.undo.iter().chain(&self.redo).flatten();
        let all = self.frames.iter().chain(history).chain(&self.initial).chain(&self.clipboard);
        all.filter(|f| seen.insert(f.packed_key())).map(|f| f.packed_bytes() as u64).sum()
    }

    pub fn find(&self, id: u64) -> Option<&Frame> {
        self.frames.iter().find(|f| f.id == id)
    }

    /// 편집하기 직전에 부른다
    fn checkpoint(&mut self) {
        if self.undo_enabled {
            self.undo.push(self.frames.clone());
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.dirty = true;
        self.revision += 1;
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(std::mem::replace(&mut self.frames, prev));
                self.dirty = true;
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(&mut self.frames, next));
                self.dirty = true;
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn set_initial(&mut self) {
        self.initial = self.frames.clone();
    }

    pub fn restore_initial(&mut self) {
        self.checkpoint();
        self.frames = self.initial.clone();
    }

    fn sel(&self, sel: &[usize]) -> Vec<usize> {
        let mut s: Vec<usize> = sel.iter().copied().filter(|&i| i < self.frames.len()).collect();
        s.sort_unstable();
        s.dedup();
        s
    }

    fn sel_or_all(&self, sel: &[usize]) -> Vec<usize> {
        let s = self.sel(sel);
        if s.is_empty() { (0..self.frames.len()).collect() } else { s }
    }

    // ---------- 프레임 목록 편집 ----------

    pub fn delete(&mut self, sel: &[usize]) {
        let s = self.sel(sel);
        if s.is_empty() || s.len() == self.frames.len() {
            return; // 전부 지울 수는 없다
        }
        self.checkpoint();
        for &i in s.iter().rev() {
            self.frames.remove(i);
        }
    }

    pub fn delete_unselected(&mut self, sel: &[usize]) {
        let s = self.sel(sel);
        if s.is_empty() {
            return;
        }
        self.checkpoint();
        self.frames = s.iter().map(|&i| self.frames[i].clone()).collect();
    }

    /// 프레임 잘라내기: [begin, end] 구간만 남긴다
    pub fn cut_range(&mut self, begin: usize, end: usize) {
        let n = self.frames.len();
        if n == 0 || begin > end || end >= n {
            return;
        }
        self.checkpoint();
        self.frames = self.frames[begin..=end].to_vec();
    }

    /// 선택한 프레임들의 순서를 거꾸로 (선택이 없으면 전체)
    pub fn reverse(&mut self, sel: &[usize]) {
        let s = self.sel_or_all(sel);
        self.checkpoint();
        let picked: Vec<Frame> = s.iter().map(|&i| self.frames[i].clone()).collect();
        for (k, &i) in s.iter().enumerate() {
            self.frames[i] = picked[picked.len() - 1 - k].clone();
        }
    }

    /// 역재생 프레임을 선택 뒤에 붙인다 (반복 재생, yoyo)
    pub fn yoyo(&mut self, sel: &[usize]) {
        let s = self.sel_or_all(sel);
        if s.len() < 2 {
            return;
        }
        self.checkpoint();
        // 끝 프레임은 두 번 보이지 않게 빼고, 처음 프레임도 빼서 이어 붙였을 때 자연스럽게
        let back: Vec<Frame> = s[1..s.len() - 1].iter().rev().map(|&i| self.frames[i].duplicate()).collect();
        let at = s[s.len() - 1] + 1;
        self.frames.splice(at..at, back);
    }

    /// 선택한 프레임을 첫 프레임 하나로 합친다 (시간은 더해진다)
    pub fn merge(&mut self, sel: &[usize]) {
        let s = self.sel(sel);
        if s.len() < 2 {
            return;
        }
        self.checkpoint();
        let total: u32 = s.iter().map(|&i| self.frames[i].delay_ms).sum();
        self.frames[s[0]] = self.frames[s[0]].with_delay(total);
        for &i in s[1..].iter().rev() {
            self.frames.remove(i);
        }
    }

    pub fn set_delay(&mut self, sel: &[usize], ms: u32) {
        let s = self.sel_or_all(sel);
        self.checkpoint();
        for i in s {
            self.frames[i] = self.frames[i].with_delay(ms.max(10));
        }
    }

    /// factor 0.9 = 10% 빠르게, 1.1 = 10% 느리게
    pub fn scale_speed(&mut self, sel: &[usize], factor: f64) {
        let s = self.sel_or_all(sel);
        self.checkpoint();
        for i in s {
            let d = (self.frames[i].delay_ms as f64 * factor).round().max(10.0) as u32;
            self.frames[i] = self.frames[i].with_delay(d);
        }
    }

    /// 선택한 프레임을 한 칸 앞(-1) 또는 뒤(+1)로
    pub fn move_by(&mut self, sel: &[usize], dir: i32) -> Vec<usize> {
        let s = self.sel(sel);
        if s.is_empty() {
            return s;
        }
        let n = self.frames.len();
        if (dir < 0 && s[0] == 0) || (dir > 0 && *s.last().unwrap() == n - 1) {
            return s;
        }
        self.checkpoint();
        if dir < 0 {
            for &i in &s {
                self.frames.swap(i, i - 1);
            }
            s.iter().map(|i| i - 1).collect()
        } else {
            for &i in s.iter().rev() {
                self.frames.swap(i, i + 1);
            }
            s.iter().map(|i| i + 1).collect()
        }
    }

    /// 맨 앞/맨 뒤로 이동 또는 복사
    pub fn move_to_end(&mut self, sel: &[usize], to_front: bool, copy: bool) {
        let s = self.sel(sel);
        if s.is_empty() {
            return;
        }
        self.checkpoint();
        let picked: Vec<Frame> =
            s.iter().map(|&i| if copy { self.frames[i].duplicate() } else { self.frames[i].clone() }).collect();
        if !copy {
            for &i in s.iter().rev() {
                self.frames.remove(i);
            }
        }
        if to_front {
            self.frames.splice(0..0, picked);
        } else {
            self.frames.extend(picked);
        }
    }

    /// 끌어 놓기: 고른 프레임을 지금 순서의 to번 자리(그 프레임 앞, 맨 뒤는 프레임 수)로 옮긴다.
    /// 옮긴 뒤 그 프레임들의 위치를 돌려준다.
    pub fn move_to(&mut self, sel: &[usize], to: usize) -> Vec<usize> {
        let s = self.sel(sel);
        if s.is_empty() {
            return s;
        }
        let to = to.min(self.frames.len());
        let at = to - s.iter().filter(|&&i| i < to).count();
        // 이미 그 자리에 붙어 있으면 아무것도 하지 않는다 (되돌리기 기록도 남기지 않게)
        if s.windows(2).all(|w| w[1] == w[0] + 1) && at == s[0] {
            return s;
        }
        self.checkpoint();
        let picked: Vec<Frame> = s.iter().map(|&i| self.frames[i].clone()).collect();
        for &i in s.iter().rev() {
            self.frames.remove(i);
        }
        let k = picked.len();
        self.frames.splice(at..at, picked);
        (at..at + k).collect()
    }

    pub fn copy(&mut self, sel: &[usize]) {
        let s = self.sel(sel);
        self.clipboard = s.iter().map(|&i| self.frames[i].clone()).collect();
    }

    pub fn cut(&mut self, sel: &[usize]) {
        self.copy(sel);
        self.delete(sel);
    }

    /// 붙여넣기. 크기가 다르면 붙이지 않는다.
    pub fn paste(&mut self, at: usize) -> bool {
        if self.clipboard.is_empty() {
            return false;
        }
        if let Some(f) = self.frames.first()
            && self.clipboard.iter().any(|c| c.width != f.width || c.height != f.height)
        {
            return false;
        }
        self.checkpoint();
        let at = at.min(self.frames.len());
        let items: Vec<Frame> = self.clipboard.iter().map(|f| f.duplicate()).collect();
        self.frames.splice(at..at, items);
        true
    }

    /// 짝/홀/N번째 프레임 지우기. keep_speed면 지운 시간을 앞 프레임에 더한다.
    pub fn reduce(&mut self, sel: &[usize], mode: ReduceMode, n: usize, keep_speed: bool) {
        let s = self.sel_or_all(sel);
        let n = n.max(2);
        // 선택 안에서 몇 번째인지(1부터)로 판단
        let drop: Vec<usize> = s
            .iter()
            .enumerate()
            .filter(|(k, _)| {
                let k = k + 1;
                match mode {
                    ReduceMode::Even => k % 2 == 0,
                    ReduceMode::Odd => k % 2 == 1,
                    ReduceMode::Nth => k % n == 0,
                }
            })
            .map(|(_, &i)| i)
            .collect();
        self.drop_frames(&drop, keep_speed);
    }

    fn drop_frames(&mut self, drop: &[usize], keep_speed: bool) {
        if drop.is_empty() || drop.len() >= self.frames.len() {
            return;
        }
        self.checkpoint();
        let mut gone = vec![false; self.frames.len()];
        for &i in drop {
            gone[i] = true;
        }
        let mut out: Vec<Frame> = Vec::with_capacity(self.frames.len() - drop.len());
        let mut carry = 0u32; // 앞에 남길 프레임이 없을 때 첫 프레임에 넘길 시간
        for (i, f) in self.frames.iter().enumerate() {
            if gone[i] {
                if keep_speed {
                    match out.last_mut() {
                        Some(prev) => *prev = prev.with_delay(prev.delay_ms + f.delay_ms),
                        None => carry += f.delay_ms,
                    }
                }
            } else {
                out.push(if carry > 0 { f.with_delay(f.delay_ms + std::mem::take(&mut carry)) } else { f.clone() });
            }
        }
        self.frames = out;
    }

    /// 비슷한 프레임 지우기.
    /// - similarity_pct: 바뀐 픽셀이 전체의 몇 % 이하면 비슷 (0이면 완전히 같을 때만)
    /// - tolerance: 채널 차이 허용치 (0~30)
    /// - time_limit_ms: 합쳐진 시간이 이 값을 넘으면 새 기준 프레임으로 끊는다
    pub fn reduce_similar(
        &mut self,
        sel: &[usize],
        similarity_pct: u32,
        tolerance: u32,
        time_limit_ms: Option<u32>,
        keep_speed: bool,
    ) -> usize {
        let s = self.sel_or_all(sel);
        let drop = find_similar(&self.frames, &s, similarity_pct, tolerance, time_limit_ms);
        let n = drop.len();
        self.drop_frames(&drop, keep_speed);
        n
    }

    // ---------- 픽셀 편집 (전체 프레임) ----------

    /// 선택 구간(처음~끝을 잇는 범위). 선택이 없으면 전체.
    fn span(&self, sel: &[usize]) -> Vec<usize> {
        let s = self.sel(sel);
        match (s.first(), s.last()) {
            (Some(&a), Some(&b)) => (a..=b).collect(),
            _ => (0..self.frames.len()).collect(),
        }
    }

    /// 고른 프레임들의 픽셀을 제자리에서 바꾼다.
    /// f(픽셀, 가로, 세로, 구간 안에서 이 프레임 가운데 시각 ms, 구간 전체 시간 ms)
    fn map_timed(&mut self, idx: &[usize], f: impl Fn(&mut [u8], u32, u32, f32, f32) + Sync) {
        if idx.is_empty() {
            return;
        }
        self.checkpoint();
        let mut mids = Vec::with_capacity(idx.len());
        let mut t = 0.0f32;
        for &i in idx {
            let d = self.frames[i].delay_ms as f32;
            mids.push(t + d / 2.0);
            t += d;
        }
        let total = t;
        let changed: Vec<Frame> = idx
            .par_iter()
            .zip(mids.par_iter())
            .map(|(&i, &tm)| {
                self.frames[i].map_pixels(|w, h, mut px| {
                    f(&mut px, w, h, tm, total);
                    (w, h, px)
                })
            })
            .collect();
        for (&i, fr) in idx.iter().zip(changed) {
            self.frames[i] = fr;
        }
    }

    // ---------- 2단계 효과 ----------

    /// 필터를 차례로 적용 (선택한 프레임, 없으면 전체)
    pub fn apply_filters(&mut self, sel: &[usize], filters: &[Filter]) {
        if filters.is_empty() {
            return;
        }
        let idx = self.sel_or_all(sel);
        self.map_timed(&idx, |px, w, h, _, _| {
            for f in filters {
                effects::apply_filter(px, w, h, f);
            }
        });
        // 잔상 효과는 앞 프레임 결과가 필요해서 차례대로 (위에서 이미 되돌리기 지점을 만들었다)
        for f in filters {
            if let Filter::Afterimage { strength } = *f {
                let mut prev: Option<Vec<u8>> = None;
                for &i in &idx {
                    let fr = &self.frames[i];
                    let mut px = fr.rgba();
                    if let Some(p) = &prev {
                        crate::filters::afterimage(&mut px, p, strength);
                    }
                    self.frames[i] = Frame::from_rgba(fr.width, fr.height, &px, fr.delay_ms);
                    prev = Some(px);
                }
            }
        }
    }

    /// 화면 전환 (선택 구간, 없으면 전체)
    pub fn transition(&mut self, sel: &[usize], kind: TransitionKind, ms: u32) {
        let idx = self.span(sel);
        let Some(&first) = idx.first() else { return };
        let first_px = self.frames[first].rgba();
        self.map_timed(&idx, |px, _, _, tm, total| {
            effects::apply_transition(px, kind, tm, total, ms as f32, &first_px);
        });
    }

    /// 줌 애니메이션: start 프레임부터 효과 시간만큼의 프레임에 넣는다
    pub fn zoom_fx(&mut self, start: usize, z: &ZoomFx) {
        let mut idx = vec![];
        let mut t = 0u32;
        for i in start..self.frames.len() {
            if t >= z.total_ms {
                break;
            }
            idx.push(i);
            t += self.frames[i].delay_ms;
        }
        let limit = z.total_ms as f32;
        self.map_timed(&idx, |px, w, h, tm, total| {
            // 녹화가 효과보다 짧으면 남은 길이에 맞춘다
            let p = z.progress(tm, total.min(limit));
            effects::draw_zoom(px, w, h, z, p);
        });
    }

    /// 프레임마다 앞 프레임과 다른 정도(0 ~ 1). 첫 프레임은 1.
    fn scores(&mut self) -> Vec<f32> {
        let ids: Vec<u64> = self.frames.iter().map(|f| f.id).collect();
        if let Some((cached, s)) = &self.change_scores
            && *cached == ids
        {
            return s.clone();
        }
        let s: Vec<f32> = (0..self.frames.len())
            .into_par_iter()
            .map(|i| {
                if i == 0 {
                    return 1.0;
                }
                let (a, b) = (&self.frames[i - 1], &self.frames[i]);
                if (a.width, a.height) != (b.width, b.height) {
                    return 1.0;
                }
                change_ratio(&a.rgba(), &b.rgba(), b.width, b.height)
            })
            .collect();
        self.change_scores = Some((ids, s.clone()));
        s
    }

    /// 영상 요약 결과: (남길 프레임 번호, 새 시간) 목록
    fn summary_plan(&mut self, o: SummaryOptions) -> Vec<(usize, u32)> {
        let n = self.frames.len();
        if n == 0 {
            return vec![];
        }
        let scores = self.scores();
        let keep_n = ((n as f64 * o.rate.clamp(1, 100) as f64 / 100.0).ceil() as usize).clamp(1, n);
        // 첫 프레임은 항상 남기고, 나머지는 많이 바뀐 순서로 고른다
        let mut order: Vec<usize> = (1..n).collect();
        order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
        let mut keep: Vec<usize> = std::iter::once(0).chain(order.into_iter().take(keep_n - 1)).collect();
        keep.sort_unstable();
        keep.iter()
            .enumerate()
            .map(|(k, &i)| {
                // 지운 프레임들의 시간은 앞에 남은 프레임이 이어받는다
                let next = keep.get(k + 1).copied().unwrap_or(n);
                let acc: u32 = self.frames[i..next].iter().map(|f| f.delay_ms).sum();
                let mut d = o.time_percent.map_or(acc, |p| (acc as u64 * p.clamp(1, 100) as u64 / 100) as u32);
                if let Some(lim) = o.limit_ms {
                    d = d.min(lim);
                }
                (i, d.max(10))
            })
            .collect()
    }

    pub fn summary_preview(&mut self, o: SummaryOptions) -> SummaryPreview {
        let plan = self.summary_plan(o);
        SummaryPreview { frames: plan.len(), total_ms: plan.iter().map(|&(_, d)| d as u64).sum() }
    }

    /// 영상 요약 (전체 프레임)
    pub fn summarize(&mut self, o: SummaryOptions) {
        let plan = self.summary_plan(o);
        if plan.is_empty() {
            return;
        }
        self.checkpoint();
        self.frames = plan.iter().map(|&(i, d)| self.frames[i].with_delay(d)).collect();
    }

    /// 그림·텍스트·이미지 여러 개를 선택 구간(없으면 전체)에 순서대로(뒤의 것이 위에) 덧씌운다. 되돌리기는 한 번.
    pub fn overlays(&mut self, sel: &[usize], ovs: &[Overlay]) {
        let idx = self.span(sel);
        self.map_timed(&idx, |px, w, h, tm, total| {
            for ov in ovs {
                ov.draw(px, w, h, tm, total);
            }
        });
    }

    fn map_all(&mut self, f: impl Fn(u32, u32, Vec<u8>) -> (u32, u32, Vec<u8>) + Sync) {
        self.checkpoint();
        self.frames = self.frames.par_iter().map(|fr| fr.map_pixels(&f)).collect();
    }

    pub fn crop(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let Some(f0) = self.frames.first() else {
            return;
        };
        let (fw, fh) = (f0.width, f0.height);
        let x = x.min(fw.saturating_sub(1));
        let y = y.min(fh.saturating_sub(1));
        let w = w.clamp(1, fw - x);
        let h = h.clamp(1, fh - y);
        self.map_all(move |sw, _sh, px| {
            let mut out = vec![0u8; (w * h * 4) as usize];
            for row in 0..h {
                let src = (((y + row) * sw + x) * 4) as usize;
                out[(row * w * 4) as usize..][..(w * 4) as usize].copy_from_slice(&px[src..src + (w * 4) as usize]);
            }
            (w, h, out)
        });
    }

    /// 모든 프레임에서 바깥쪽 투명 픽셀을 걷어낸 영역
    pub fn trim_bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let f0 = self.frames.first()?;
        let (w, h) = (f0.width, f0.height);
        let b = self
            .frames
            .par_iter()
            .filter_map(|f| {
                let px = f.rgba();
                let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
                for y in 0..h {
                    for x in 0..w {
                        if px[((y * w + x) * 4 + 3) as usize] != 0 {
                            x0 = x0.min(x);
                            y0 = y0.min(y);
                            x1 = x1.max(x);
                            y1 = y1.max(y);
                        }
                    }
                }
                (x1 >= x0 && y1 >= y0).then_some((x0, y0, x1, y1))
            })
            .reduce(|| (w, h, 0, 0), |a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)));
        (b.2 >= b.0 && b.3 >= b.1).then(|| (b.0, b.1, b.2 - b.0 + 1, b.3 - b.1 + 1))
    }

    pub fn resize(&mut self, w: u32, h: u32, smooth: bool) {
        let (w, h) = (w.max(1), h.max(1));
        self.map_all(move |sw, sh, px| (w, h, resize_rgba(sw, sh, &px, w, h, smooth)));
    }

    /// 2·3·4배 초해상도 (학습된 ESPCN 모델, `superres.rs`)
    pub fn upscale(&mut self, factor: u32) {
        let k = factor.clamp(2, 4);
        self.map_all(move |w, h, px| crate::superres::upscale_rgba(w, h, &px, k));
    }

    /// i번 프레임의 그림을 바꾼다 (시간은 그대로)
    pub fn replace_pixels(&mut self, i: usize, w: u32, h: u32, rgba: &[u8]) -> bool {
        let Some(f) = self.frames.get(i) else { return false };
        if (f.width, f.height) != (w, h) {
            return false;
        }
        let d = f.delay_ms;
        self.checkpoint();
        self.frames[i] = Frame::from_rgba(w, h, rgba, d);
        true
    }

    /// at 자리에 새 프레임을 넣는다 (크기는 다른 프레임과 같아야 한다)
    pub fn insert_frame(&mut self, at: usize, f: Frame) -> bool {
        if let Some(first) = self.frames.first()
            && (first.width, first.height) != (f.width, f.height)
        {
            return false;
        }
        self.checkpoint();
        let at = at.min(self.frames.len());
        self.frames.insert(at, f);
        true
    }

    pub fn rotate(&mut self, r: Rotate) {
        self.map_all(move |w, h, px| rotate_rgba(w, h, &px, r));
    }
}

/// 두 이미지에서 바뀐 픽셀 비율 (0 ~ 1). 빠르게 하려고 약 4만 곳만 골라 본다.
pub fn change_ratio(a: &[u8], b: &[u8], w: u32, h: u32) -> f32 {
    if a.len() != b.len() {
        return 1.0;
    }
    let step = ((w as f64 * h as f64 / 40_000.0).sqrt() as u32).max(1);
    let (mut changed, mut total) = (0u32, 0u32);
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let i = ((y * w + x) * 4) as usize;
            total += 1;
            // 비슷한 프레임 판정과 같은 허용치(20)
            if (0..4).any(|k| (a[i + k] as i32 - b[i + k] as i32).abs() > 20) {
                changed += 1;
            }
        }
    }
    changed as f32 / total.max(1) as f32
}

/// 두 이미지가 비슷한지 판정
pub fn is_similar(a: &[u8], b: &[u8], w: u32, h: u32, ratio: f32, t: i32) -> bool {
    if a.len() != b.len() {
        return false;
    }
    if ratio <= 0.0 {
        return a == b;
    }
    let limit = ((w * h) as f32 * ratio) as u32;
    let mut changed = 0u32;
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let d0 = (pa[0] as i32 - pb[0] as i32).abs();
        let d1 = (pa[1] as i32 - pb[1] as i32).abs();
        let d2 = (pa[2] as i32 - pb[2] as i32).abs();
        let d3 = (pa[3] as i32 - pb[3] as i32).abs();
        if d0 > t || d1 > t || d2 > t || d3 > t || d0 + d1 + d2 + d3 > t * 2 {
            changed += 1;
            if changed > limit {
                return false;
            }
        }
    }
    true
}

/// 기준 프레임과 비교해 비슷한 프레임들의 번호를 돌려준다
pub fn find_similar(
    frames: &[Frame],
    sel: &[usize],
    similarity_pct: u32,
    tolerance: u32,
    time_limit_ms: Option<u32>,
) -> Vec<usize> {
    let ratio = similarity_pct.min(50) as f32 / 100.0;
    let t = tolerance.min(30) as i32;
    let mut drop = vec![];
    let Some(&first) = sel.first() else {
        return drop;
    };
    let mut anchor = frames[first].rgba();
    let mut anchor_size = (frames[first].width, frames[first].height);
    let mut merged = frames[first].delay_ms;
    for &i in &sel[1..] {
        let f = &frames[i];
        let px = f.rgba();
        let similar = (f.width, f.height) == anchor_size && is_similar(&anchor, &px, f.width, f.height, ratio, t);
        let within_time = time_limit_ms.is_none_or(|lim| merged + f.delay_ms <= lim);
        if similar && within_time {
            drop.push(i);
            merged += f.delay_ms;
        } else {
            anchor = px;
            anchor_size = (f.width, f.height);
            merged = f.delay_ms;
        }
    }
    drop
}

pub fn resize_rgba(sw: u32, sh: u32, px: &[u8], w: u32, h: u32, smooth: bool) -> Vec<u8> {
    use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image};
    if (sw, sh) == (w, h) {
        return px.to_vec();
    }
    let src = Image::from_vec_u8(sw, sh, px.to_vec(), PixelType::U8x4).expect("bad source image");
    let mut dst = Image::new(w, h, PixelType::U8x4);
    let alg = if smooth { ResizeAlg::Convolution(FilterType::Lanczos3) } else { ResizeAlg::Nearest };
    Resizer::new().resize(&src, &mut dst, &ResizeOptions::new().resize_alg(alg)).expect("resize failed");
    dst.into_vec()
}

pub fn rotate_rgba(w: u32, h: u32, px: &[u8], r: Rotate) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = match r {
        Rotate::Left | Rotate::Right => (h, w),
        _ => (w, h),
    };
    let mut out = vec![0u8; px.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match r {
                Rotate::Right => (h - 1 - y, x),
                Rotate::Left => (y, w - 1 - x),
                Rotate::Half => (w - 1 - x, h - 1 - y),
                Rotate::FlipH => (w - 1 - x, y),
                Rotate::FlipV => (x, h - 1 - y),
            };
            let s = ((y * w + x) * 4) as usize;
            let d = ((ny * nw + nx) * 4) as usize;
            out[d..d + 4].copy_from_slice(&px[s..s + 4]);
        }
    }
    (nw, nh, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8, delay: u32) -> Frame {
        Frame::from_rgba(w, h, &vec![v; (w * h * 4) as usize], delay)
    }

    #[test]
    fn similar_threshold() {
        let a = vec![100u8; 10 * 10 * 4];
        let mut b = a.clone();
        // 픽셀 10개(10%)만 크게 바꾼다
        for p in 0..10 {
            b[p * 4] = 200;
        }
        assert!(is_similar(&a, &b, 10, 10, 0.20, 20));
        assert!(!is_similar(&a, &b, 10, 10, 0.05, 20));
        // 허용치 안의 작은 차이는 바뀐 픽셀로 세지 않는다
        let c: Vec<u8> = a.iter().map(|v| v + 5).collect();
        assert!(is_similar(&a, &c, 10, 10, 0.01, 20));
        // 비율 0이면 완전히 같을 때만
        assert!(!is_similar(&a, &c, 10, 10, 0.0, 20));
    }

    #[test]
    fn move_to_drops_frames_before_target() {
        let mut p = Project::default();
        p.undo_enabled = true;
        // 프레임 시간으로 어떤 프레임인지 알아본다
        p.load((1..=5).map(|d| solid(2, 2, 0, d * 10)).collect());
        let order = |p: &Project| p.frames.iter().map(|f| f.delay_ms / 10).collect::<Vec<_>>();
        // 2·3번(0부터)을 맨 앞으로
        assert_eq!(p.move_to(&[1, 2], 0), vec![0, 1]);
        assert_eq!(order(&p), vec![2, 3, 1, 4, 5]);
        // 첫 프레임을 4번 자리 앞(= 지금 5번째 앞)으로: 뒤로 옮기면 빠진 만큼 당겨진다
        assert_eq!(p.move_to(&[0], 4), vec![3]);
        assert_eq!(order(&p), vec![3, 1, 4, 2, 5]);
        // 맨 뒤로
        assert_eq!(p.move_to(&[0], 5), vec![4]);
        assert_eq!(order(&p), vec![1, 4, 2, 5, 3]);
        // 제자리면 되돌리기 기록이 늘지 않는다
        let undo = p.undo.len();
        assert_eq!(p.move_to(&[1], 1), vec![1]);
        assert_eq!(p.move_to(&[1], 2), vec![1]);
        assert_eq!(p.undo.len(), undo);
    }

    #[test]
    fn memory_counts_shared_buffers_once() {
        let mut p = Project::default();
        let a = solid(8, 8, 10, 100);
        let one = a.packed_bytes() as u64;
        // 같은 버퍼를 쓰는 시간만 바꾼 복사본
        p.load(vec![a.clone(), a.with_delay(50), solid(8, 8, 200, 100)]);
        let two = one + p.frames[2].packed_bytes() as u64;
        assert_eq!(p.memory_bytes(), two);
    }

    #[test]
    fn reduce_similar_keeps_speed() {
        let mut p = Project::default();
        p.load(vec![solid(4, 4, 0, 100), solid(4, 4, 0, 100), solid(4, 4, 255, 100), solid(4, 4, 255, 100)]);
        let n = p.reduce_similar(&[], 0, 20, None, true);
        assert_eq!(n, 2);
        assert_eq!(p.frames.len(), 2);
        assert_eq!(p.frames[0].delay_ms, 200);
        assert_eq!(p.frames[1].delay_ms, 200);
    }

    #[test]
    fn similar_time_limit_splits() {
        let mut p = Project::default();
        p.load((0..5).map(|_| solid(2, 2, 7, 60)).collect());
        // 100ms 제한: 60 + 60 > 100 이라 합치지 않고 계속 새 기준이 된다
        assert_eq!(p.reduce_similar(&[], 20, 20, Some(100), true), 0);
        assert_eq!(p.reduce_similar(&[], 20, 20, Some(130), true), 2);
    }

    #[test]
    fn redo_after_undo_and_cleared_by_new_edit() {
        let mut p = Project { undo_enabled: true, ..Default::default() };
        p.load((0..4).map(|i| solid(1, 1, i, 10)).collect());
        assert!(!p.redo());
        p.delete(&[0]);
        p.delete(&[0]);
        assert_eq!(p.frames.len(), 2);
        assert!(p.undo());
        assert!(p.undo());
        assert_eq!(p.frames.len(), 4);
        assert!(p.redo());
        assert_eq!(p.frames.len(), 3);
        assert!(p.can_undo() && p.can_redo());
        // 되돌린 뒤 새로 편집하면 다시 실행할 것이 없어진다
        p.set_delay(&[], 50);
        assert!(!p.can_redo());
        assert!(!p.redo());
        assert!(p.undo());
        assert_eq!(p.frames.len(), 3);
        assert!(p.frames.iter().all(|f| f.delay_ms == 10));
        // 처음 상태로도 되돌리고 다시 실행할 수 있다
        p.restore_initial();
        assert_eq!(p.frames.len(), 4);
        assert!(p.undo());
        assert_eq!(p.frames.len(), 3);
        assert!(p.redo());
        assert_eq!(p.frames.len(), 4);
        // 새로 불러오면 두 기록 모두 비운다
        assert!(p.undo());
        p.load(vec![solid(1, 1, 0, 10)]);
        assert!(!p.can_undo() && !p.can_redo());
    }

    #[test]
    fn undo_and_redo_after_saving_are_unsaved_changes() {
        // 저장한 뒤 되돌리거나 다시 실행하면 저장한 파일과 달라지므로, 닫을 때 물어야 한다
        let mut p = Project { undo_enabled: true, ..Default::default() };
        p.load((0..3).map(|i| solid(1, 1, i, 10)).collect());
        p.delete(&[0]);
        p.dirty = false;
        assert!(p.undo());
        assert!(p.dirty);
        p.dirty = false;
        assert!(p.redo());
        assert!(p.dirty);
        // 되돌릴 것이 없으면 그대로
        p.dirty = false;
        assert!(!p.redo());
        assert!(!p.dirty);
    }

    #[test]
    fn even_odd_and_yoyo() {
        let mut p = Project { undo_enabled: true, ..Default::default() };
        p.load((0..6).map(|i| solid(1, 1, i, 10)).collect());
        p.reduce(&[], ReduceMode::Even, 2, true);
        assert_eq!(p.frames.len(), 3);
        assert!(p.frames.iter().all(|f| f.delay_ms == 20));
        p.yoyo(&[]);
        // 0 1 2 + 1 = 4장
        assert_eq!(p.frames.len(), 4);
        assert!(p.undo());
        assert_eq!(p.frames.len(), 3);
    }

    #[test]
    fn video_summary() {
        let mut p = Project { undo_enabled: true, ..Default::default() };
        // 0,0,0,9,9,9 → 바뀐 곳은 3번(0→9) 하나뿐
        p.load([0u8, 0, 0, 200, 200, 200].iter().map(|&v| solid(4, 4, v, 100)).collect());
        let o = SummaryOptions { rate: 34, limit_ms: None, time_percent: None };
        // 6장의 34% → 3장: 첫 프레임, 바뀐 3번, 그리고 점수가 같은 것 중 앞쪽(1번)
        assert_eq!(p.summary_preview(o).frames, 3);
        p.summarize(o);
        let delays: Vec<u32> = p.frames.iter().map(|f| f.delay_ms).collect();
        assert_eq!(delays, vec![100, 200, 300]); // 총 재생 시간 유지
        assert!(p.undo());
        let o = SummaryOptions { rate: 34, limit_ms: Some(150), time_percent: Some(50) };
        p.summarize(o);
        let delays: Vec<u32> = p.frames.iter().map(|f| f.delay_ms).collect();
        assert_eq!(delays, vec![50, 100, 150]); // 50%로 줄이고 150ms에서 자른다
    }

    #[test]
    fn rotate_and_crop() {
        let px: Vec<u8> = (0..6u8).flat_map(|i| [i, 0, 0, 255]).collect(); // 3x2
        let (w, h, out) = rotate_rgba(3, 2, &px, Rotate::Right);
        assert_eq!((w, h), (2, 3));
        // 오른쪽으로 돌리면 왼쪽 아래(3)가 맨 위 왼쪽으로 온다
        assert_eq!(out[0], 3);
        let mut p = Project::default();
        p.load(vec![Frame::from_rgba(3, 2, &px, 10)]);
        p.crop(1, 0, 2, 2);
        assert_eq!((p.frames[0].width, p.frames[0].height), (2, 2));
        assert_eq!(p.frames[0].rgba()[0], 1);
    }
}

//! 학습된 모델로 하는 초해상도 (ESPCN, OpenCV dnn_superres 모델 원본).
//! 가중치는 `models/`에 있고(Apache 2.0, models/README.md), 계산은 여기서 직접 한다.
//! 밝기(Y)만 모델로 키우고 색(Cr·Cb)은 부드럽게 늘려서 합친다 (원래 구현과 같은 방식).
use crate::project::resize_rgba;
use rayon::prelude::*;

const X2: &[u8] = include_bytes!("../models/espcn_x2.bin");
const X3: &[u8] = include_bytes!("../models/espcn_x3.bin");
const X4: &[u8] = include_bytes!("../models/espcn_x4.bin");

struct Conv {
    k: usize,
    cin: usize,
    cout: usize,
    /// [높이][너비][입력][출력]
    w: Vec<f32>,
    b: Vec<f32>,
}

struct Espcn {
    r: usize,
    layers: [Conv; 3],
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

impl Espcn {
    fn new(r: usize) -> Espcn {
        let all = floats(match r {
            2 => X2,
            3 => X3,
            _ => X4,
        });
        let shapes = [(5, 1, 64), (3, 64, 32), (3, 32, r * r)];
        let mut at = 0;
        let mut take = |n: usize| {
            let v = all[at..at + n].to_vec();
            at += n;
            v
        };
        let layers = shapes.map(|(k, cin, cout)| {
            let w = take(k * k * cin * cout);
            let b = take(cout);
            Conv { k, cin, cout, w, b }
        });
        assert_eq!(at, all.len(), "모델 파일 크기가 맞지 않습니다");
        Espcn { r, layers }
    }

    /// 밝기(h×w, 0~1)를 r배로 키운다. 결과는 (h·r)×(w·r).
    fn run(&self, y: &[f32], h: usize, w: usize) -> Vec<f32> {
        let l1 = conv(y, h, w, &self.layers[0], true);
        let l2 = conv(&l1, h, w, &self.layers[1], true);
        let l3 = conv(&l2, h, w, &self.layers[2], false);
        // 픽셀 재배치(depth_to_space): 출력 (y·r+dy, x·r+dx) = 채널 dy·r+dx
        let r = self.r;
        let (ow, oh) = (w * r, h * r);
        let mut out = vec![0f32; ow * oh];
        out.par_chunks_mut(ow).enumerate().for_each(|(oy, row)| {
            let (sy, dy) = (oy / r, oy % r);
            for (ox, o) in row.iter_mut().enumerate() {
                let (sx, dx) = (ox / r, ox % r);
                *o = l3[(sy * w + sx) * r * r + dy * r + dx].tanh();
            }
        });
        out
    }
}

/// 같은 크기 출력의 합성곱 (모자란 가장자리는 0, TF의 SAME). 자료는 [y][x][채널].
fn conv(input: &[f32], h: usize, w: usize, l: &Conv, relu: bool) -> Vec<f32> {
    let p = l.k / 2;
    let mut out = vec![0f32; h * w * l.cout];
    out.par_chunks_mut(w * l.cout).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let acc = &mut row[x * l.cout..(x + 1) * l.cout];
            acc.copy_from_slice(&l.b);
            for ky in 0..l.k {
                let Some(sy) = (y + ky).checked_sub(p).filter(|&v| v < h) else { continue };
                for kx in 0..l.k {
                    let Some(sx) = (x + kx).checked_sub(p).filter(|&v| v < w) else { continue };
                    let src = &input[(sy * w + sx) * l.cin..][..l.cin];
                    let wk = &l.w[(ky * l.k + kx) * l.cin * l.cout..][..l.cin * l.cout];
                    for (ci, &v) in src.iter().enumerate() {
                        if v == 0.0 {
                            continue;
                        }
                        for (a, &wt) in acc.iter_mut().zip(&wk[ci * l.cout..(ci + 1) * l.cout]) {
                            *a += v * wt;
                        }
                    }
                }
            }
            if relu {
                for a in acc.iter_mut() {
                    *a = a.max(0.0);
                }
            }
        }
    });
    out
}

/// RGBA 그림을 r배(2~4)로 키운다. 밝기는 ESPCN, 색과 투명도는 Lanczos로 늘려 합친다.
pub fn upscale_rgba(w: u32, h: u32, px: &[u8], r: u32) -> (u32, u32, Vec<u8>) {
    let r = r.clamp(2, 4);
    let model = Espcn::new(r as usize);
    let (nw, nh) = (w * r, h * r);
    let y: Vec<f32> = px
        .chunks_exact(4)
        .map(|p| (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0)
        .collect();
    let big_y = model.run(&y, h as usize, w as usize);
    // 색과 투명도: 부드럽게 늘린 그림의 Cr·Cb·알파를 쓴다
    let mut out = resize_rgba(w, h, px, nw, nh, true);
    out.par_chunks_mut(4).zip(big_y.par_iter()).for_each(|(p, &ny)| {
        let (r0, g0, b0) = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
        let y0 = 0.299 * r0 + 0.587 * g0 + 0.114 * b0;
        // OpenCV YCrCb (0~1): Cr = (R-Y)·0.713 + 0.5, Cb = (B-Y)·0.564 + 0.5
        let cr = (r0 - y0) * 0.713;
        let cb = (b0 - y0) * 0.564;
        let ny = ny.clamp(0.0, 1.0);
        let rgb = [ny + 1.403 * cr, ny - 0.714 * cr - 0.344 * cb, ny + 1.773 * cb];
        for (c, v) in p.iter_mut().zip(rgb) {
            *c = (v * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    });
    (nw, nh, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 원래 모델(TensorFlow 그래프)과 같은 계산을 numpy로 한 값과 비교한다 (models/README.md의 층 구성).
    #[test]
    fn matches_reference_numpy_output() {
        let (h, w) = (7usize, 9usize);
        let y: Vec<f32> = (0..h).flat_map(|i| (0..w).map(move |j| ((i * 37 + j * 11) % 97) as f32 / 96.0)).collect();
        let expect: [(usize, [f32; 4]); 3] = [
            (2, [0.025979, 0.017335, 0.521954, 0.206529]),
            (3, [0.113764, 0.008185, 0.461037, 0.283127]),
            (4, [0.088754, 0.030802, 0.452884, 0.280223]),
        ];
        for (r, want) in expect {
            let out = Espcn::new(r).run(&y, h, w);
            let ow = w * r;
            let at = [(0, 0), (1, 1), (r * 3, r * 4 + 1), (h * r - 1, w * r - 1)];
            for ((yy, xx), e) in at.iter().zip(want) {
                let got = out[yy * ow + xx];
                assert!((got - e).abs() < 1e-4, "x{r} ({yy},{xx}): {got} != {e}");
            }
        }
    }

    #[test]
    fn upscale_keeps_flat_color_and_alpha() {
        let px = [200u8, 40, 90, 128].repeat(6 * 5);
        let (w, h, out) = upscale_rgba(6, 5, &px, 3);
        assert_eq!((w, h), (18, 15));
        // 한가운데(가장자리 0 채움 영향이 없는 곳)는 원래 색과 거의 같고, 알파는 그대로
        let c = &out[((7 * 18) + 9) * 4..][..4];
        assert!(c[..3].iter().zip([200u8, 40, 90]).all(|(&a, b)| a.abs_diff(b) <= 12), "{c:?}");
        assert_eq!(c[3], 128);
    }
}

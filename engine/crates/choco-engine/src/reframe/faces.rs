//! Face detection with YuNet (OpenCV Zoo, MIT, 233 KB) on the pure-Rust `tract` runtime.
//! Frames are letterboxed into the model's fixed 640x640 input.

use crate::render::yuv::Frame;
use ffmpeg_next as ff;
use tract_onnx::prelude::*;

const MODEL: &[u8] = include_bytes!("../../assets/models/face_detection_yunet_2023mar.onnx");
const SIZE: usize = 640;
const STRIDES: [usize; 3] = [8, 16, 32];

/// A detected face in normalised display coordinates (0..1 of frame width / height).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
    /// Right eye, left eye, nose tip, right mouth corner, left mouth corner.
    pub landmarks: [(f32, f32); 5],
}

impl Face {
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

pub fn iou(a: &Face, b: &Face) -> f32 {
    let ix = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let iy = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if ix <= 0.0 || iy <= 0.0 {
        return 0.0;
    }
    let inter = ix * iy;
    inter / (a.w * a.h + b.w * b.h - inter)
}

type Model = std::sync::Arc<TypedRunnableModel>;

pub struct FaceDetector {
    model: Model,
    scaler: Option<ff::software::scaling::Context>,
    scaler_key: (u32, u32),
    pub min_score: f32,
}

impl FaceDetector {
    pub fn new() -> TractResult<Self> {
        let model = tract_onnx::onnx()
            .model_for_read(&mut &MODEL[..])?
            .with_input_fact(0, f32::fact([1, 3, SIZE, SIZE]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self {
            model,
            scaler: None,
            scaler_key: (0, 0),
            min_score: 0.6,
        })
    }

    /// Faces in a YUV420P display-orientation frame, best first.
    pub fn detect(&mut self, f: &Frame) -> TractResult<Vec<Face>> {
        let (w, h) = (f.width(), f.height());
        let scale = SIZE as f32 / w.max(h) as f32;
        let (sw, sh) = (
            ((w as f32 * scale) as u32).max(2) & !1,
            ((h as f32 * scale) as u32).max(2) & !1,
        );
        if self.scaler.is_none() || self.scaler_key != (w, h) {
            self.scaler = Some(ff::software::scaling::Context::get(
                ff::format::Pixel::YUV420P,
                w,
                h,
                ff::format::Pixel::BGR24,
                sw,
                sh,
                ff::software::scaling::Flags::BILINEAR,
            )?);
            self.scaler_key = (w, h);
        }
        let mut bgr = Frame::new(ff::format::Pixel::BGR24, sw, sh);
        self.scaler.as_mut().expect("set above").run(f, &mut bgr)?;
        let stride = bgr.stride(0);
        let data = bgr.data(0);
        let mut input = tract_ndarray::Array4::<f32>::zeros((1, 3, SIZE, SIZE));
        for y in 0..sh as usize {
            for x in 0..sw as usize {
                let p = y * stride + x * 3;
                for c in 0..3 {
                    input[[0, c, y, x]] = data[p + c] as f32;
                }
            }
        }
        let out = self.model.run(tvec!(Tensor::from(input).into()))?;
        let mut faces = Vec::new();
        for (si, &s) in STRIDES.iter().enumerate() {
            let cols = SIZE / s;
            let cls = out[si].to_plain_array_view::<f32>()?;
            let obj = out[3 + si].to_plain_array_view::<f32>()?;
            let bbox = out[6 + si].to_plain_array_view::<f32>()?;
            let kps = out[9 + si].to_plain_array_view::<f32>()?;
            let n = cls.shape()[1];
            for i in 0..n {
                let score =
                    (cls[[0, i, 0]].clamp(0.0, 1.0) * obj[[0, i, 0]].clamp(0.0, 1.0)).sqrt();
                if score < self.min_score {
                    continue;
                }
                let (r, c) = ((i / cols) as f32, (i % cols) as f32);
                let s = s as f32;
                let cx = (c + bbox[[0, i, 0]]) * s;
                let cy = (r + bbox[[0, i, 1]]) * s;
                let bw = bbox[[0, i, 2]].exp() * s;
                let bh = bbox[[0, i, 3]].exp() * s;
                let mut lm = [(0.0f32, 0.0f32); 5];
                for (k, p) in lm.iter_mut().enumerate() {
                    *p = (
                        ((kps[[0, i, 2 * k]] + c) * s) / scale / w as f32,
                        ((kps[[0, i, 2 * k + 1]] + r) * s) / scale / h as f32,
                    );
                }
                faces.push(Face {
                    x: (cx - bw / 2.0) / scale / w as f32,
                    y: (cy - bh / 2.0) / scale / h as f32,
                    w: bw / scale / w as f32,
                    h: bh / scale / h as f32,
                    score,
                    landmarks: lm,
                });
            }
        }
        faces.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<Face> = Vec::new();
        for fc in faces {
            if kept.iter().all(|k| iou(k, &fc) < 0.3) {
                kept.push(fc);
            }
            if kept.len() == 8 {
                break;
            }
        }
        Ok(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::yuv;

    #[test]
    fn model_loads_and_finds_nothing_in_a_flat_frame() {
        crate::init();
        let mut d = FaceDetector::new().unwrap();
        let mut f = yuv::new_frame(640, 360);
        yuv::fill(&mut f, 120, 128, 128);
        assert!(d.detect(&f).unwrap().is_empty());
    }

    #[test]
    fn iou_basics() {
        let a = Face {
            x: 0.0,
            y: 0.0,
            w: 0.2,
            h: 0.2,
            score: 1.0,
            landmarks: [(0.0, 0.0); 5],
        };
        let b = Face { x: 0.1, ..a };
        assert!((iou(&a, &a) - 1.0).abs() < 1e-6);
        assert!(iou(&a, &b) > 0.3 && iou(&a, &b) < 0.4);
    }
}

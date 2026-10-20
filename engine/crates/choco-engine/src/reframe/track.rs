//! Link per-sample face detections into tracks, end tracks at scene cuts, and measure mouth
//! motion (mean absolute luma change in the mouth region between samples).

use super::faces::{Face, iou};
use crate::render::yuv::Frame;

#[derive(Debug, Clone)]
pub struct Sample {
    pub t_ms: u64,
    pub faces: Vec<Face>,
    /// Mouth-region motion per face, same order as `faces`.
    pub motion: Vec<f32>,
    pub cut: bool,
}

#[derive(Debug, Clone)]
pub struct TrackPoint {
    pub t_ms: u64,
    pub face: Face,
    pub motion: f32,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: usize,
    pub points: Vec<TrackPoint>,
}

impl Track {
    pub fn start(&self) -> u64 {
        self.points.first().map(|p| p.t_ms).unwrap_or(0)
    }
    pub fn end(&self) -> u64 {
        self.points.last().map(|p| p.t_ms).unwrap_or(0)
    }
    pub fn duration(&self) -> u64 {
        self.end() - self.start()
    }
    pub fn at(&self, t_ms: u64) -> Option<&TrackPoint> {
        self.points.iter().find(|p| p.t_ms == t_ms)
    }
    pub fn mean_cx(&self) -> f32 {
        self.points.iter().map(|p| p.face.cx()).sum::<f32>() / self.points.len().max(1) as f32
    }
}

/// 32-bin luma histogram on a subsampled grid, normalised.
pub fn histogram(f: &Frame) -> [f32; 32] {
    let (w, h, s) = (f.width() as usize, f.height() as usize, f.stride(0));
    let y = f.data(0);
    let mut hist = [0f32; 32];
    let mut n = 0f32;
    for yy in (0..h).step_by(4) {
        for xx in (0..w).step_by(4) {
            hist[(y[yy * s + xx] >> 3) as usize] += 1.0;
            n += 1.0;
        }
    }
    hist.iter_mut().for_each(|v| *v /= n.max(1.0));
    hist
}

/// A hard cut when half the histogram mass moved.
pub fn is_cut(a: &[f32; 32], b: &[f32; 32]) -> bool {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() > 1.0
}

/// Mean absolute luma difference inside the face's mouth region.
pub fn mouth_motion(face: &Face, cur: &Frame, prev: &Frame) -> f32 {
    if cur.width() != prev.width() || cur.height() != prev.height() {
        return 0.0;
    }
    let (w, h, s) = (cur.width() as f32, cur.height() as f32, cur.stride(0));
    let (rm, lm) = (face.landmarks[3], face.landmarks[4]);
    let mx = (rm.0 + lm.0) / 2.0;
    let my = (rm.1 + lm.1) / 2.0;
    let half_w = ((lm.0 - rm.0).abs() * 0.75).max(face.w * 0.2);
    let (x0, x1) = (
        ((mx - half_w) * w).max(0.0) as usize,
        ((mx + half_w) * w).min(w - 1.0) as usize,
    );
    let (y0, y1) = (
        ((my - face.h * 0.15) * h).max(0.0) as usize,
        ((my + face.h * 0.2) * h).min(h - 1.0) as usize,
    );
    let (a, b) = (cur.data(0), prev.data(0));
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in y0..=y1 {
        for x in x0..=x1 {
            sum += a[y * s + x].abs_diff(b[y * s + x]) as u64;
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { sum as f32 / n as f32 }
}

/// Greedy IoU association between consecutive samples. Tracks end at cuts or after
/// `max_gap_ms` without a match; tracks shorter than `min_ms` are dropped.
pub fn track(samples: &[Sample], max_gap_ms: u64, min_ms: u64) -> Vec<Track> {
    let mut done: Vec<Track> = Vec::new();
    let mut live: Vec<Track> = Vec::new();
    let mut next_id = 0;
    for s in samples {
        if s.cut {
            done.append(&mut live);
        }
        let mut used = vec![false; s.faces.len()];
        let mut keep: Vec<Track> = Vec::new();
        for mut t in live.drain(..) {
            let last = t.points.last().expect("tracks are never empty").face;
            let best = s
                .faces
                .iter()
                .enumerate()
                .filter(|(i, _)| !used[*i])
                .map(|(i, f)| (i, iou(&last, f), (last.cx() - f.cx()).abs()))
                .filter(|(_, o, d)| *o > 0.2 || *d < last.w * 0.6)
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.2.total_cmp(&a.2)));
            if let Some((i, _, _)) = best {
                used[i] = true;
                t.points.push(TrackPoint {
                    t_ms: s.t_ms,
                    face: s.faces[i],
                    motion: s.motion.get(i).copied().unwrap_or(0.0),
                });
                keep.push(t);
            } else if s.t_ms.saturating_sub(t.end()) <= max_gap_ms {
                keep.push(t);
            } else {
                done.push(t);
            }
        }
        for (i, f) in s.faces.iter().enumerate() {
            if !used[i] {
                keep.push(Track {
                    id: next_id,
                    points: vec![TrackPoint {
                        t_ms: s.t_ms,
                        face: *f,
                        motion: s.motion.get(i).copied().unwrap_or(0.0),
                    }],
                });
                next_id += 1;
            }
        }
        live = keep;
    }
    done.append(&mut live);
    done.retain(|t| t.duration() >= min_ms);
    done.sort_by_key(|t| t.id);
    done
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn face(cx: f32, cy: f32) -> Face {
        Face {
            x: cx - 0.05,
            y: cy - 0.1,
            w: 0.1,
            h: 0.2,
            score: 0.9,
            landmarks: [
                (cx - 0.02, cy - 0.03),
                (cx + 0.02, cy - 0.03),
                (cx, cy),
                (cx - 0.02, cy + 0.05),
                (cx + 0.02, cy + 0.05),
            ],
        }
    }

    #[test]
    fn two_faces_two_tracks_and_cuts_split_tracks() {
        let mut samples = Vec::new();
        for k in 0..20u64 {
            samples.push(Sample {
                t_ms: k * 200,
                faces: vec![face(0.3 + k as f32 * 0.002, 0.4), face(0.7, 0.4)],
                motion: vec![1.0, 0.0],
                cut: k == 10,
            });
        }
        let tr = track(&samples, 600, 1000);
        // Two people, split in two at the cut: four tracks.
        assert_eq!(
            tr.len(),
            4,
            "{:?}",
            tr.iter()
                .map(|t| (t.start(), t.end(), t.mean_cx()))
                .collect::<Vec<_>>()
        );
        assert!(tr.iter().all(|t| t.duration() >= 1000));
    }

    #[test]
    fn short_blips_are_dropped() {
        let mut samples: Vec<Sample> = (0..10u64)
            .map(|k| Sample {
                t_ms: k * 200,
                faces: vec![face(0.5, 0.4)],
                motion: vec![0.0],
                cut: false,
            })
            .collect();
        samples[4].faces.push(face(0.1, 0.1));
        samples[4].motion.push(0.0);
        assert_eq!(track(&samples, 600, 1000).len(), 1);
    }
}

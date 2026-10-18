//! Turn face tracks and word timings into a `CropPlan`.
//!
//! Two people on screen together for most of the clip get a stacked split. Otherwise the crop
//! follows the active speaker (most mouth motion while words are spoken, switching only after a
//! one-second lead), with a dead zone so small head movements do not move the camera, eased
//! moves for real ones, and hard cuts on scene changes and speaker switches.

use super::track::Track;
use crate::model::{CropKeyframe, CropPlan, Easing, Layout, Rect};
use crate::render::center_crop_plan;

#[derive(Debug, Clone, PartialEq)]
pub struct PlanOptions {
    /// Output size; the crop keeps its aspect.
    pub target: (u32, u32),
    /// The face may drift this far (share of frame width) from the crop centre before it moves.
    pub deadzone: f32,
    pub ease_ms: u64,
    /// Both faces present in at least this share of samples to use a split.
    pub split_presence: f32,
    /// A different speaker must lead for this long before the crop switches.
    pub switch_lead_ms: u64,
    /// A track must cover this share of the clip to count as a main subject.
    pub min_track_share: f32,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self { target: (1080, 1920), deadzone: 0.12, ease_ms: 400, split_presence: 0.7, switch_lead_ms: 1000, min_track_share: 0.3 }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PlanReport {
    pub layout: String,
    pub tracks: usize,
    pub subjects: usize,
    pub switches: usize,
    pub moves: usize,
}

/// Single-subject crop rect (normalised) centred horizontally on `cx`, full height when the
/// source is wider than the target.
fn single_rect(cx: f32, display: (u32, u32), target: (u32, u32)) -> Rect {
    let (sw, sh) = (display.0 as f32, display.1 as f32);
    let ta = target.0 as f32 / target.1 as f32;
    let (w, h) = if sw / sh > ta { (sh * ta / sw, 1.0) } else { (1.0, sw / ta / sh) };
    Rect { x: (cx - w / 2.0).clamp(0.0, 1.0 - w), y: (1.0 - h) / 2.0, w, h }
}

/// Slot rect for a split: aspect = output width : half output height, face about 40% from top.
fn slot_rect(t: &Track, display: (u32, u32), target: (u32, u32)) -> Rect {
    let (sw, sh) = (display.0 as f32, display.1 as f32);
    let slot_aspect = target.0 as f32 / (target.1 as f32 / 2.0);
    let n = t.points.len().max(1) as f32;
    let (cx, cy, fh) = t.points.iter().fold((0.0, 0.0, 0.0), |a, p| (a.0 + p.face.cx() / n, a.1 + p.face.cy() / n, a.2 + p.face.h / n));
    let mut h_px = (fh * sh * 3.2).min(sh);
    let mut w_px = h_px * slot_aspect;
    if w_px > sw {
        w_px = sw;
        h_px = w_px / slot_aspect;
    }
    let (w, h) = (w_px / sw, h_px / sh);
    Rect { x: (cx - w / 2.0).clamp(0.0, 1.0 - w), y: (cy - 0.4 * h).clamp(0.0, 1.0 - h), w, h }
}

fn speaking(words: &[(u64, u64)], t: u64) -> bool {
    words.iter().any(|&(a, b)| a <= t + 250 && b + 250 >= t)
}

/// `sample_times` are the times faces were sampled; `cuts` the sample times that start a new shot;
/// `words` the clip's word intervals (source ms).
#[allow(clippy::too_many_arguments)]
pub fn plan(
    tracks: &[Track],
    sample_times: &[u64],
    cuts: &[u64],
    words: &[(u64, u64)],
    display: (u32, u32),
    range: (u64, u64),
    opts: &PlanOptions,
) -> (CropPlan, PlanReport) {
    let (start, end) = range;
    let clip_ms = (end - start).max(1) as f32;
    // Main subjects (for the split decision) must span a good share of the clip. For following,
    // any track counts: edited multi-camera shows cut every few seconds, so each shot's track is
    // short, and the crop must still follow whoever is on screen in that shot.
    let stable: Vec<&Track> = tracks.iter().filter(|t| t.duration() as f32 >= opts.min_track_share * clip_ms).collect();
    let subjects: Vec<&Track> = tracks.iter().collect();
    let mut report = PlanReport { layout: "static".into(), tracks: tracks.len(), subjects: stable.len(), switches: 0, moves: 0 };
    if subjects.is_empty() || sample_times.is_empty() {
        return (center_crop_plan(display, opts.target, start), report);
    }

    // Split: the two most present subjects appear together most of the time.
    if stable.len() >= 2 {
        let mut by_presence = stable.clone();
        by_presence.sort_by_key(|t| std::cmp::Reverse(t.points.len()));
        let (a, b) = (by_presence[0], by_presence[1]);
        let together = sample_times.iter().filter(|&&t| a.at(t).is_some() && b.at(t).is_some()).count();
        if together as f32 >= opts.split_presence * sample_times.len() as f32 {
            let (left, right) = if a.mean_cx() <= b.mean_cx() { (a, b) } else { (b, a) };
            report.layout = "split".into();
            let plan = CropPlan {
                layout: Layout::Split,
                keyframes: vec![CropKeyframe { t_ms: start, rects: vec![slot_rect(left, display, opts.target), slot_rect(right, display, opts.target)], easing: Easing::Hold }],
                source_size: display,
                target_size: opts.target,
            };
            return (plan, report);
        }
    }

    // Single: choose the active speaker per sample, with hysteresis.
    report.layout = "single".into();
    let motion_near = |t: &Track, at: u64| -> f32 {
        t.points.iter().filter(|p| p.t_ms + 500 >= at && p.t_ms <= at + 500).map(|p| p.motion).sum::<f32>()
    };
    let mut current: Option<usize> = None;
    let mut challenger: Option<(usize, u64)> = None;
    let mut keyframes: Vec<CropKeyframe> = Vec::new();
    let mut crop_cx: Option<f32> = None;
    let mut last_move_end = 0u64;
    for &t in sample_times {
        let present: Vec<(usize, &Track)> = subjects.iter().enumerate().filter(|(_, tr)| tr.at(t).is_some()).map(|(i, tr)| (i, *tr)).collect();
        if present.is_empty() {
            continue;
        }
        let leader = if speaking(words, t) && present.len() > 1 {
            present.iter().max_by(|a, b| motion_near(a.1, t).total_cmp(&motion_near(b.1, t))).map(|p| p.0)
        } else {
            None
        };
        let cut_here = cuts.contains(&t);
        match (current, leader) {
            (None, _) => current = Some(leader.unwrap_or(present[0].0)),
            (Some(c), Some(l)) if l != c => match challenger {
                Some((who, since)) if who == l && t.saturating_sub(since) >= opts.switch_lead_ms => {
                    current = Some(l);
                    challenger = None;
                    crop_cx = None; // force a cut to the new speaker
                    report.switches += 1;
                }
                Some((who, _)) if who == l => {}
                _ => challenger = Some((l, t)),
            },
            _ => challenger = None,
        }
        // Current subject left the frame: fall back to whoever is present.
        if let Some(c) = current
            && subjects[c].at(t).is_none()
        {
            current = Some(present[0].0);
            crop_cx = None;
        }
        let subj = subjects[current.expect("set above")];
        let face = subj.at(t).expect("present").face;
        // Median of the face centre over +-500 ms, so detection noise does not move the camera.
        let mut near: Vec<f32> = subj.points.iter().filter(|p| p.t_ms + 500 >= t && p.t_ms <= t + 500).map(|p| p.face.cx()).collect();
        near.sort_by(|a, b| a.total_cmp(b));
        let fcx = near.get(near.len() / 2).copied().unwrap_or(face.cx());
        match crop_cx {
            None => {
                // First frame, a cut or a switch: jump.
                keyframes.push(CropKeyframe { t_ms: t, rects: vec![single_rect(fcx, display, opts.target)], easing: Easing::Hold });
                crop_cx = Some(fcx);
            }
            Some(cx) if cut_here => {
                keyframes.push(CropKeyframe { t_ms: t, rects: vec![single_rect(fcx, display, opts.target)], easing: Easing::Hold });
                crop_cx = Some(fcx);
                let _ = cx;
            }
            Some(cx) if (fcx - cx).abs() > opts.deadzone && t >= last_move_end => {
                // Ease from the current position to the face over ease_ms.
                keyframes.push(CropKeyframe { t_ms: t, rects: vec![single_rect(cx, display, opts.target)], easing: Easing::EaseInOut });
                keyframes.push(CropKeyframe { t_ms: t + opts.ease_ms, rects: vec![single_rect(fcx, display, opts.target)], easing: Easing::Hold });
                crop_cx = Some(fcx);
                last_move_end = t + opts.ease_ms;
                report.moves += 1;
            }
            _ => {}
        }
    }
    if keyframes.is_empty() {
        return (center_crop_plan(display, opts.target, start), report);
    }
    keyframes.sort_by_key(|k| k.t_ms);
    // The clip starts at `start`: make the first keyframe cover it.
    keyframes[0].t_ms = keyframes[0].t_ms.min(start);
    (CropPlan { layout: Layout::Single, keyframes, source_size: display, target_size: opts.target }, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reframe::track::{Track, TrackPoint, tests::face};

    fn track_at(id: usize, times: &[u64], cx: impl Fn(u64) -> f32, motion: impl Fn(u64) -> f32) -> Track {
        Track { id, points: times.iter().map(|&t| TrackPoint { t_ms: t, face: face(cx(t), 0.4), motion: motion(t) }).collect() }
    }

    fn times(n: u64) -> Vec<u64> {
        (0..n).map(|k| k * 200).collect()
    }

    #[test]
    fn no_faces_gives_a_centre_crop() {
        let (p, r) = plan(&[], &times(50), &[], &[], (1920, 1080), (0, 10_000), &PlanOptions::default());
        assert_eq!(p.layout, Layout::Static);
        assert_eq!(r.layout, "static");
    }

    #[test]
    fn single_subject_off_centre_is_followed_without_jitter() {
        let ts = times(50);
        // Face at 0.3 with small noise: the crop centres on it once and never moves.
        let t = track_at(0, &ts, |t| 0.3 + if t % 400 == 0 { 0.02 } else { -0.02 }, |_| 1.0);
        let (p, r) = plan(&[t], &ts, &[], &[(0, 10_000)], (1920, 1080), (0, 10_000), &PlanOptions::default());
        assert_eq!(p.layout, Layout::Single);
        assert_eq!(r.moves, 0, "noise must stay inside the dead zone");
        let rect = &p.keyframes[0].rects[0];
        assert!((rect.x + rect.w / 2.0 - 0.3).abs() < 0.05, "{rect:?}");
    }

    #[test]
    fn a_real_move_is_eased_not_jumped() {
        let ts = times(50);
        let t = track_at(0, &ts, |t| if t < 5_000 { 0.3 } else { 0.7 }, |_| 1.0);
        let (p, r) = plan(&[t], &ts, &[], &[], (1920, 1080), (0, 10_000), &PlanOptions::default());
        assert_eq!(r.moves, 1);
        assert!(p.keyframes.iter().any(|k| k.easing == Easing::EaseInOut));
    }

    #[test]
    fn two_people_together_get_a_split() {
        let ts = times(50);
        let a = track_at(0, &ts, |_| 0.25, |_| 1.0);
        let b = track_at(1, &ts, |_| 0.75, |_| 0.0);
        let (p, r) = plan(&[b, a], &ts, &[], &[], (1920, 1080), (0, 10_000), &PlanOptions::default());
        assert_eq!(p.layout, Layout::Split);
        assert_eq!(r.layout, "split");
        // Left person on top.
        assert!(p.keyframes[0].rects[0].x < p.keyframes[0].rects[1].x);
    }

    #[test]
    fn multi_camera_shots_are_each_followed() {
        // Four 2.5 s shots, one face each, alternating sides: no track spans 30% of the clip.
        let ts = times(50);
        let shots: Vec<Track> = (0..4)
            .map(|k| {
                let range: Vec<u64> = ts.iter().copied().filter(|t| *t >= k * 2_500 && *t < (k + 1) * 2_500).collect();
                track_at(k as usize, &range, move |_| if k % 2 == 0 { 0.8 } else { 0.2 }, |_| 1.0)
            })
            .collect();
        let cuts: Vec<u64> = vec![2_500, 5_000, 7_500];
        let (p, _) = plan(&shots, &ts, &cuts, &[], (1920, 1080), (0, 10_000), &PlanOptions::default());
        assert_eq!(p.layout, Layout::Single);
        let at = |t| crate::render::rects_at(&p, t)[0];
        let centre = |r: crate::model::Rect| r.x + r.w / 2.0;
        assert!(centre(at(1_000)) > 0.6, "shot 1 face on the right: {:?}", at(1_000));
        assert!(centre(at(3_500)) < 0.4, "shot 2 face on the left: {:?}", at(3_500));
    }

    #[test]
    fn speaker_switch_needs_a_sustained_lead() {
        // Two people but rarely together (each present 60%), A talks first, then B.
        let ts = times(100);
        let a = track_at(0, &ts[..60], |_| 0.25, |t| if t < 8_000 { 5.0 } else { 0.0 });
        let b = track_at(1, &ts[40..], |_| 0.75, |t| if t >= 8_000 { 5.0 } else { 0.0 });
        let words = vec![(0u64, 20_000u64)];
        let (p, r) = plan(&[a, b], &ts, &[], &words, (1920, 1080), (0, 20_000), &PlanOptions::default());
        assert_eq!(p.layout, Layout::Single);
        assert!(r.switches >= 1, "{r:?}");
        let last = p.keyframes.last().unwrap().rects[0];
        assert!(last.x + last.w / 2.0 > 0.6, "ends on B: {last:?}");
    }
}

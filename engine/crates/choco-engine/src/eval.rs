//! Golden-set evaluation of the picker. A pick "hits" a golden clip when they overlap by at
//! least half of the shorter of the two. Pure functions; the CLI does the I/O.

use crate::model::Candidate;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenClip {
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default)]
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenFixture {
    pub id: String,
    pub media: PathBuf,
    pub transcript: PathBuf,
    pub license: String,
    pub attribution: String,
    /// Who drafted the ranges, and whether a human has reviewed them.
    pub drafted_by: String,
    pub reviewed_by_human: bool,
    pub clips: Vec<GoldenClip>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenSet {
    pub version: u32,
    pub fixtures: Vec<GoldenFixture>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FixtureScore {
    pub id: String,
    pub picks: usize,
    pub precision_at_5: f32,
    pub recall_at_10: f32,
    /// Mean intersection-over-union of each matched golden clip with its best pick.
    pub mean_iou: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EvalScore {
    pub fixtures: Vec<FixtureScore>,
    pub precision_at_5: f32,
    pub recall_at_10: f32,
    pub mean_iou: f32,
    /// True only when every fixture's golden clips were reviewed by a human.
    pub human_reviewed: bool,
}

fn overlap(a: (u64, u64), b: (u64, u64)) -> u64 {
    a.1.min(b.1).saturating_sub(a.0.max(b.0))
}

pub fn hits(pick: (u64, u64), gold: (u64, u64)) -> bool {
    let shorter = (pick.1 - pick.0).min(gold.1 - gold.0).max(1);
    overlap(pick, gold) * 2 >= shorter
}

fn iou(a: (u64, u64), b: (u64, u64)) -> f32 {
    let inter = overlap(a, b) as f32;
    let union = (a.1.max(b.1) - a.0.min(b.0)) as f32;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// `picks` must be in ranked order, best first.
pub fn score_fixture(id: &str, picks: &[Candidate], golden: &[GoldenClip]) -> FixtureScore {
    let p: Vec<(u64, u64)> = picks.iter().map(|c| (c.start_ms, c.end_ms)).collect();
    let g: Vec<(u64, u64)> = golden.iter().map(|c| (c.start_ms, c.end_ms)).collect();
    let top5 = &p[..p.len().min(5)];
    let top10 = &p[..p.len().min(10)];
    let precision_at_5 = if top5.is_empty() {
        0.0
    } else {
        top5.iter().filter(|&&x| g.iter().any(|&y| hits(x, y))).count() as f32 / top5.len() as f32
    };
    let matched: Vec<f32> = g
        .iter()
        .filter_map(|&y| top10.iter().filter(|&&x| hits(x, y)).map(|&x| iou(x, y)).reduce(f32::max))
        .collect();
    FixtureScore {
        id: id.to_string(),
        picks: p.len(),
        precision_at_5,
        recall_at_10: if g.is_empty() { 0.0 } else { matched.len() as f32 / g.len() as f32 },
        mean_iou: if matched.is_empty() { 0.0 } else { matched.iter().sum::<f32>() / matched.len() as f32 },
    }
}

pub fn aggregate(fixtures: Vec<FixtureScore>, set: &GoldenSet) -> EvalScore {
    let n = fixtures.len().max(1) as f32;
    EvalScore {
        precision_at_5: fixtures.iter().map(|f| f.precision_at_5).sum::<f32>() / n,
        recall_at_10: fixtures.iter().map(|f| f.recall_at_10).sum::<f32>() / n,
        mean_iou: fixtures.iter().map(|f| f.mean_iou).sum::<f32>() / n,
        human_reviewed: set.fixtures.iter().all(|f| f.reviewed_by_human),
        fixtures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PickerProvenance;

    fn c(s: u64, e: u64) -> Candidate {
        Candidate {
            id: format!("{s}"),
            start_ms: s,
            end_ms: e,
            score: 80,
            reasons: vec![],
            title: String::new(),
            description: String::new(),
            hashtags: vec![],
            provenance: PickerProvenance { tier: "t".into(), model: "m".into(), prompt_hash: "h".into() },
        }
    }
    fn g(s: u64, e: u64) -> GoldenClip {
        GoldenClip { start_ms: s, end_ms: e, why: String::new() }
    }

    #[test]
    fn half_overlap_of_the_shorter_counts() {
        assert!(hits((0, 30_000), (15_000, 60_000)));
        assert!(!hits((0, 30_000), (16_000, 60_000)));
        // A short pick fully inside a long golden clip is a hit.
        assert!(hits((40_000, 55_000), (30_000, 90_000)));
    }

    #[test]
    fn precision_and_recall() {
        let picks = vec![c(0, 30_000), c(100_000, 130_000), c(200_000, 230_000), c(300_000, 330_000), c(400_000, 430_000), c(500_000, 530_000)];
        let gold = vec![g(0, 30_000), g(205_000, 240_000), g(900_000, 930_000), g(500_000, 520_000)];
        let s = score_fixture("x", &picks, &gold);
        assert!((s.precision_at_5 - 0.4).abs() < 1e-6, "{s:?}"); // 2 of top 5
        assert!((s.recall_at_10 - 0.75).abs() < 1e-6, "{s:?}"); // 3 of 4 golden found in top 10
        assert!(s.mean_iou > 0.6);
    }

    #[test]
    fn fewer_than_five_picks_are_not_rewarded() {
        // One perfect pick out of one is precision 1.0 over the picks actually made.
        let s = score_fixture("x", &[c(0, 30_000)], &[g(0, 30_000)]);
        assert_eq!(s.precision_at_5, 1.0);
    }
}

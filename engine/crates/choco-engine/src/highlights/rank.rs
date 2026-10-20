//! Final ranking after the model has scored the shortlist: no overlapping clips, no two clips
//! saying the same thing, a quality floor, and at most `n`. Content decides the count.

use crate::model::Candidate;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RankOptions {
    pub max_clips: usize,
    pub min_score: u8,
    /// Two clips overlapping by more than this share of the shorter one cannot both be kept.
    pub max_overlap_pct: u8,
}

impl Default for RankOptions {
    fn default() -> Self {
        Self {
            max_clips: 10,
            min_score: 55,
            max_overlap_pct: 40,
        }
    }
}

pub fn rank(mut scored: Vec<(Candidate, String)>, opts: &RankOptions) -> Vec<Candidate> {
    scored.sort_by(|a, b| {
        b.0.score
            .cmp(&a.0.score)
            .then(a.0.start_ms.cmp(&b.0.start_ms))
    });
    let mut out: Vec<(Candidate, String)> = Vec::new();
    for (c, text) in scored {
        if c.score < opts.min_score || out.len() == opts.max_clips {
            break;
        }
        let clash = out.iter().any(|(o, otext)| {
            let inter = c
                .end_ms
                .min(o.end_ms)
                .saturating_sub(c.start_ms.max(o.start_ms));
            let shorter = (c.end_ms - c.start_ms).min(o.end_ms - o.start_ms).max(1);
            inter * 100 > shorter * opts.max_overlap_pct as u64
                || super::search::jaccard(&text, otext) > 0.5
        });
        if !clash {
            out.push((c, text));
        }
    }
    out.into_iter().map(|(c, _)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PickerProvenance;

    fn cand(id: &str, s: u64, e: u64, score: u8) -> (Candidate, String) {
        (
            Candidate {
                id: id.into(),
                start_ms: s,
                end_ms: e,
                score,
                reasons: vec![],
                title: String::new(),
                description: String::new(),
                hashtags: vec![],
                provenance: PickerProvenance {
                    tier: "t".into(),
                    model: "m".into(),
                    prompt_hash: "h".into(),
                },
            },
            [
                "alpha bravo charlie delta",
                "echo foxtrot golf hotel",
                "india juliet kilo lima",
                "mike november oscar papa",
                "quebec romeo sierra tango",
            ][(id.as_bytes()[0] - b'a') as usize % 5]
                .to_string(),
        )
    }

    #[test]
    fn suppresses_overlap_applies_floor_and_cap() {
        let v = vec![
            cand("a", 0, 30_000, 90),
            cand("b", 5_000, 40_000, 85), // overlaps a heavily
            cand("c", 50_000, 80_000, 70),
            cand("d", 100_000, 130_000, 50), // under the floor
            cand("e", 140_000, 170_000, 60),
        ];
        let ids: Vec<_> = rank(v.clone(), &RankOptions::default())
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec!["a", "c", "e"]);
        let ids: Vec<_> = rank(
            v,
            &RankOptions {
                max_clips: 1,
                ..Default::default()
            },
        )
        .into_iter()
        .map(|c| c.id)
        .collect();
        assert_eq!(ids, vec!["a"]);
    }
}

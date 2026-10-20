//! Audio features for the shortlist prior: loudness relative to the episode, energy spikes,
//! and long pauses. Computed once from the 16 kHz mono WAV the transcriber already uses.

use crate::ingest::{SPEECH_SAMPLE_RATE, read_wav_f32};
use std::path::Path;

/// 50 ms frames.
pub const FRAME_MS: u64 = 50;

#[derive(Debug, Clone, PartialEq)]
pub struct AudioFeatures {
    /// RMS in dBFS per 50 ms frame.
    pub db: Vec<f32>,
    pub mean_db: f32,
    pub std_db: f32,
    /// Frames quieter than this count as silence.
    pub silence_db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowAudio {
    /// Mean loudness of voiced frames, in episode standard deviations.
    pub loudness_z: f32,
    /// Voiced frames more than two standard deviations above the episode mean, per minute.
    pub spikes_per_min: f32,
    /// Fraction of frames that are silent.
    pub pause_ratio: f32,
    /// Longest silent stretch, ms.
    pub longest_pause_ms: u64,
}

pub fn analyze(wav: &Path) -> std::io::Result<AudioFeatures> {
    Ok(from_samples(&read_wav_f32(wav)?))
}

pub fn from_samples(samples: &[f32]) -> AudioFeatures {
    let frame = (SPEECH_SAMPLE_RATE as u64 * FRAME_MS / 1000) as usize;
    let db: Vec<f32> = samples
        .chunks(frame)
        .map(|c| {
            let rms = (c.iter().map(|s| s * s).sum::<f32>() / c.len().max(1) as f32).sqrt();
            20.0 * rms.max(1e-6).log10()
        })
        .collect();
    // Voiced statistics only, so long silences do not drag the mean down.
    let mut sorted = db.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    // Silence sits near the noise floor and well under typical speech. A percentile alone fails
    // when silence is rare (most podcasts), so cap it relative to the median level.
    let pct = |p: usize| sorted.get(sorted.len() * p / 100).copied().unwrap_or(-90.0);
    let silence_db = (pct(10) + 6.0).max(-50.0).min(pct(50) - 15.0);
    let voiced: Vec<f32> = db.iter().copied().filter(|&d| d > silence_db).collect();
    let mean_db = if voiced.is_empty() {
        -90.0
    } else {
        voiced.iter().sum::<f32>() / voiced.len() as f32
    };
    let std_db = if voiced.len() < 2 {
        1.0
    } else {
        (voiced.iter().map(|d| (d - mean_db).powi(2)).sum::<f32>() / voiced.len() as f32)
            .sqrt()
            .max(0.5)
    };
    AudioFeatures {
        db,
        mean_db,
        std_db,
        silence_db,
    }
}

impl AudioFeatures {
    pub fn window(&self, start_ms: u64, end_ms: u64) -> WindowAudio {
        let a = (start_ms / FRAME_MS) as usize;
        let b = ((end_ms / FRAME_MS) as usize).min(self.db.len());
        if b <= a {
            return WindowAudio::default();
        }
        let frames = &self.db[a..b];
        let voiced: Vec<f32> = frames
            .iter()
            .copied()
            .filter(|&d| d > self.silence_db)
            .collect();
        let loudness_z = if voiced.is_empty() {
            -3.0
        } else {
            (voiced.iter().sum::<f32>() / voiced.len() as f32 - self.mean_db) / self.std_db
        };
        let spikes = voiced
            .iter()
            .filter(|&&d| d > self.mean_db + 2.0 * self.std_db)
            .count();
        let minutes = (end_ms - start_ms) as f32 / 60_000.0;
        let silent = frames.len() - voiced.len();
        let mut longest = 0u64;
        let mut run = 0u64;
        for &d in frames {
            if d <= self.silence_db {
                run += FRAME_MS;
                longest = longest.max(run);
            } else {
                run = 0;
            }
        }
        WindowAudio {
            loudness_z,
            spikes_per_min: spikes as f32 / minutes.max(0.01),
            pause_ratio: silent as f32 / frames.len() as f32,
            longest_pause_ms: longest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * SPEECH_SAMPLE_RATE as f32) as usize)
            .map(|i| amp * (i as f32 * 0.07).sin())
            .collect()
    }

    #[test]
    fn louder_window_scores_higher_and_pauses_are_found() {
        let mut s = tone(20.0, 0.05);
        s.extend(vec![0.0; 3 * SPEECH_SAMPLE_RATE as usize]);
        s.extend(tone(10.0, 0.4));
        let f = from_samples(&s);
        let quiet = f.window(0, 20_000);
        let loud = f.window(23_000, 33_000);
        let pause = f.window(15_000, 25_000);
        assert!(
            loud.loudness_z > quiet.loudness_z + 1.0,
            "{loud:?} vs {quiet:?}"
        );
        assert!(pause.longest_pause_ms >= 2_900, "{pause:?}");
        assert!(quiet.pause_ratio < 0.05);
    }
}

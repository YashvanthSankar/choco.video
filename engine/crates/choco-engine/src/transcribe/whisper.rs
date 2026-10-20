//! whisper.cpp transcriber. Default on Windows and on Macs older than macOS 26.
//! whisper.cpp windows long audio internally (30 s with context carry-over), so we feed
//! the whole file and rebuild words from sub-word tokens.

use super::{Result, STAGE, TranscribeError, TranscribeOptions};
use crate::ProgressFn;
use crate::backend::Device;
use crate::model::{Progress, Segment, Transcript, Word};
use crate::models_registry;
use std::path::Path;
use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext,
    WhisperContextParameters,
};

fn werr(e: impl std::fmt::Display) -> TranscribeError {
    TranscribeError::Whisper(e.to_string())
}

pub fn transcribe(
    wav: &Path,
    device: Device,
    opts: &TranscribeOptions,
    progress: ProgressFn,
) -> Result<Transcript> {
    let manifest = models_registry::find(&opts.whisper_model)
        .ok_or_else(|| TranscribeError::ModelMissing(opts.whisper_model.clone()))?;
    if !models_registry::is_installed(&opts.models_dir, &manifest) {
        return Err(TranscribeError::ModelMissing(opts.whisper_model.clone()));
    }
    let model_path = models_registry::path_for(&opts.models_dir, &manifest);
    let audio = crate::ingest::read_wav_f32(wav)?;

    whisper_rs::install_logging_hooks();
    progress(Progress::new(
        STAGE,
        0.0,
        format!("loading {}", opts.whisper_model),
    ));
    let mut cparams = WhisperContextParameters::default();
    cparams.use_gpu(device != Device::Cpu);
    // Dynamic-time-warping token timestamps from the cross-attention heads: far tighter word
    // onsets than the default energy heuristic (measured in fixtures/timing.py). Needs the
    // preset that matches the model; unknown models keep the heuristic times.
    if let Some(preset) = dtw_preset(&opts.whisper_model) {
        cparams.dtw_parameters(DtwParameters {
            mode: DtwMode::ModelPreset {
                model_preset: preset,
            },
            ..Default::default()
        });
    }
    let ctx = WhisperContext::new_with_params(&model_path, cparams).map_err(werr)?;
    let mut state = ctx.create_state().map_err(werr)?;

    // whisper.cpp wants a two-letter code or "auto".
    let lang = opts
        .language
        .as_deref()
        .map(|l| l.split(['-', '_']).next().unwrap_or(l).to_lowercase());

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8) as i32;

    // whisper.cpp falls into hallucination loops on long conversation (one sentence repeated
    // 174 times on a 72-min podcast) and the speech under a loop is lost. Decoding chunks of at
    // most 30 s, cut at the quietest moment, bounds a loop to one chunk; a chunk that still loops
    // is decoded again with sampling, then beam search, and the cleanest decode is kept.
    let chunks = chunk_bounds(&audio);
    let total = audio.len().max(1) as f32;
    let mut language: Option<String> = lang.clone();
    let mut segments = Vec::new();
    let mut retried = 0usize;
    for &(a, b) in &chunks {
        progress(Progress::new(STAGE, a as f32 / total, "transcribing"));
        let offset_ms = (a as u64 * 1000) / SAMPLE_RATE as u64;
        let secs = (b - a) as f32 / SAMPLE_RATE as f32;
        let mut best: Option<(i64, Vec<Segment>)> = None;
        for (i, attempt) in ATTEMPTS.iter().enumerate() {
            let lang_ref = language.as_deref().unwrap_or("auto");
            let params = full_params(attempt, lang_ref, threads);
            state.full(params, &audio[a..b]).map_err(werr)?;
            if language.is_none() {
                // Detect once, on the first chunk, so later chunks cannot flip languages.
                language =
                    whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_string);
            }
            let segs = read_segments(&state, offset_ms);
            let words: Vec<Word> = segs.iter().flat_map(|s| s.words.iter().cloned()).collect();
            let n = words.len();
            let kept = super::cleanup::drop_repeated_ngrams(words, 16, 3).len();
            let looped = n - kept >= 6 || (secs > 10.0 && (kept as f32) / secs < 0.6 && n > kept);
            let score = kept as i64 - 3 * (n - kept) as i64;
            if best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((score, segs));
            }
            if !looped {
                break;
            }
            if i + 1 < ATTEMPTS.len() {
                retried += 1;
            }
        }
        segments.extend(best.map(|(_, s)| s).unwrap_or_default());
    }
    if retried > 0 {
        progress(Progress::new(
            STAGE,
            1.0,
            format!("re-decoded {retried} looping chunks"),
        ));
    }
    let language = language.unwrap_or_else(|| "auto".into());
    progress(Progress::new(STAGE, 1.0, "transcribed"));
    Ok(Transcript {
        language,
        engine: format!("whisper:{}", manifest.id.trim_start_matches("whisper-")),
        segments,
    })
}

const SAMPLE_RATE: usize = 16_000;

/// One decode setting: greedy first, then sampling and beam search for chunks that loop.
enum Attempt {
    Greedy,
    Sample(f32),
    Beam,
}
const ATTEMPTS: [Attempt; 3] = [Attempt::Greedy, Attempt::Sample(0.5), Attempt::Beam];

fn full_params<'a>(attempt: &Attempt, lang: &'a str, threads: i32) -> FullParams<'a, 'a> {
    let strategy = match attempt {
        Attempt::Beam => SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: -1.0,
        },
        _ => SamplingStrategy::Greedy { best_of: 1 },
    };
    let mut params = FullParams::new(strategy);
    params.set_n_threads(threads);
    params.set_language(Some(lang));
    params.set_token_timestamps(true);
    params.set_no_context(true); // fewer runaway repeats across windows
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    if let Attempt::Sample(t) = attempt {
        params.set_temperature(*t);
    }
    params
}

/// Chunk boundaries (sample ranges) of at most 30 s, each cut at the quietest 200 ms between
/// 20 s and 30 s so a cut rarely lands inside a word. Near-silent chunks are left out.
fn chunk_bounds(audio: &[f32]) -> Vec<(usize, usize)> {
    const FRAME: usize = SAMPLE_RATE / 50; // 20 ms
    const MAX: usize = 30 * SAMPLE_RATE;
    const MIN: usize = 20 * SAMPLE_RATE;
    let energy: Vec<f32> = audio
        .chunks(FRAME)
        .map(|f| f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32)
        .collect();
    let mut out = Vec::new();
    let mut a = 0;
    while a < audio.len() {
        let b = if audio.len() - a <= MAX {
            audio.len()
        } else {
            // Quietest 10-frame run whose centre falls in [a + 20 s, a + 30 s).
            let (lo, hi) = ((a + MIN) / FRAME, (a + MAX) / FRAME - 10);
            let mut run: f32 = energy[lo..lo + 10].iter().sum();
            let (mut best, mut at) = (run, lo);
            for f in lo + 1..=hi {
                run += energy[f + 9] - energy[f - 1];
                if run < best {
                    (best, at) = (run, f);
                }
            }
            (at + 5) * FRAME
        };
        let peak = energy[a / FRAME..b.div_ceil(FRAME).min(energy.len())]
            .iter()
            .fold(0.0f32, |m, &e| m.max(e));
        // -60 dBFS: nothing to transcribe, and whisper invents "Thank you." on silence.
        if peak > 1e-6 {
            out.push((a, b));
        }
        a = b;
    }
    out
}

/// Words and segments of the last decode, timed from the start of the whole file.
fn read_segments(state: &whisper_rs::WhisperState, offset_ms: u64) -> Vec<Segment> {
    let mut segments = Vec::new();
    for seg in state.as_iter() {
        let mut words: Vec<Word> = Vec::new();
        // Whisper tokens are byte pieces: one Devanagari or CJK character can span several
        // tokens, so decode each word's bytes once, not each token (per-token lossy decoding
        // turned every non-Latin script into replacement characters).
        let mut cur: Vec<u8> = Vec::new();
        let (mut cur_start, mut cur_end, mut cur_conf) = (0u64, 0u64, 1.0f32);
        let flush = |cur: &mut Vec<u8>, start: u64, end: u64, conf: f32, words: &mut Vec<Word>| {
            let text = String::from_utf8_lossy(cur).trim().to_string();
            if !text.is_empty() {
                words.push(Word {
                    start_ms: start,
                    end_ms: end.max(start + 1),
                    text,
                    confidence: conf,
                });
            }
            cur.clear();
        };
        for t in 0..seg.n_tokens() {
            let Some(tok) = seg.get_token(t) else {
                continue;
            };
            let Ok(bytes) = tok.to_bytes() else { continue };
            if bytes.starts_with(b"[_") || bytes.starts_with(b"<|") {
                continue; // special token
            }
            let data = tok.token_data();
            let (mut t0, t1) = (
                offset_ms + (data.t0.max(0) as u64) * 10,
                offset_ms + (data.t1.max(0) as u64) * 10,
            );
            if data.t_dtw >= 0 {
                t0 = offset_ms + data.t_dtw as u64 * 10;
            }
            let starts_word = bytes.starts_with(b" ") || cur.is_empty();
            if starts_word && !cur.is_empty() {
                flush(&mut cur, cur_start, cur_end, cur_conf, &mut words);
            }
            if cur.is_empty() {
                cur_start = t0;
                cur_end = t1;
                cur_conf = data.p;
            } else {
                cur_end = t1.max(cur_end);
                cur_conf = cur_conf.min(data.p);
            }
            cur.extend_from_slice(bytes);
        }
        flush(&mut cur, cur_start, cur_end, cur_conf, &mut words);
        let text = seg
            .to_str_lossy()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if words.is_empty() && text.is_empty() {
            continue;
        }
        segments.push(Segment {
            start_ms: offset_ms + (seg.start_timestamp().max(0) as u64) * 10,
            end_ms: offset_ms + (seg.end_timestamp().max(0) as u64) * 10,
            text,
            words,
        });
    }
    segments
}

/// The whisper.cpp DTW alignment-head preset for a registry model id, if it is a standard model.
fn dtw_preset(model_id: &str) -> Option<DtwModelPreset> {
    let id = model_id.to_lowercase();
    Some(if id.contains("large-v3-turbo") {
        DtwModelPreset::LargeV3Turbo
    } else if id.contains("large-v3") {
        DtwModelPreset::LargeV3
    } else if id.contains("large-v2") {
        DtwModelPreset::LargeV2
    } else if id.contains("medium.en") {
        DtwModelPreset::MediumEn
    } else if id.contains("medium") {
        DtwModelPreset::Medium
    } else if id.contains("small.en") {
        DtwModelPreset::SmallEn
    } else if id.contains("small") {
        DtwModelPreset::Small
    } else if id.contains("base.en") {
        DtwModelPreset::BaseEn
    } else if id.contains("base") {
        DtwModelPreset::Base
    } else if id.contains("tiny.en") {
        DtwModelPreset::TinyEn
    } else if id.contains("tiny") {
        DtwModelPreset::Tiny
    } else {
        return None;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone with a short pause at `gap_s`, then more tone.
    fn speech_with_gap(total_s: f32, gap_s: f32) -> Vec<f32> {
        let n = (total_s * SAMPLE_RATE as f32) as usize;
        let gap = (gap_s * SAMPLE_RATE as f32) as usize;
        (0..n)
            .map(|i| {
                if (gap..gap + SAMPLE_RATE / 2).contains(&i) {
                    0.0
                } else {
                    0.3 * (i as f32 * 0.07).sin()
                }
            })
            .collect()
    }

    #[test]
    fn cuts_at_the_pause_and_never_exceeds_30_s() {
        let audio = speech_with_gap(75.0, 24.0);
        let chunks = chunk_bounds(&audio);
        assert_eq!(chunks[0].0, 0);
        let cut = chunks[0].1 as f32 / SAMPLE_RATE as f32;
        assert!((24.0..24.5).contains(&cut), "cut at {cut} s");
        for w in chunks.windows(2) {
            assert_eq!(w[0].1, w[1].0, "chunks are contiguous");
        }
        assert!(chunks.iter().all(|(a, b)| b - a <= 30 * SAMPLE_RATE));
        assert_eq!(chunks.last().unwrap().1, audio.len());
    }

    #[test]
    fn skips_silence() {
        let mut audio = vec![0.0f32; 40 * SAMPLE_RATE];
        audio.extend(speech_with_gap(10.0, 100.0));
        let chunks = chunk_bounds(&audio);
        assert_eq!(chunks.len(), 1, "{chunks:?}");
        assert_eq!(chunks[0].1, audio.len());
    }
}

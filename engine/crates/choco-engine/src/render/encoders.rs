//! H.264 encoder selection. Each backend names its encoders in preference order
//! (`Backend::hw_encoders`); this table maps each to FFmpeg encoder names, the input pixel format
//! the compositor must deliver, and options. The render opens the first candidate that works and
//! reports which one it used.

use crate::backend::HwCodec;
use ffmpeg_next as ff;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// FFmpeg encoder name, e.g. `h264_videotoolbox`.
    pub name: &'static str,
    /// Pixel format the encoder is fed. The compositor produces planar YUV420P today.
    pub pixel: ff::format::Pixel,
    pub options: &'static [(&'static str, &'static str)],
}

const fn c(name: &'static str, options: &'static [(&'static str, &'static str)]) -> Candidate {
    Candidate {
        name,
        pixel: ff::format::Pixel::YUV420P,
        options,
    }
}

/// The FFmpeg candidates for one backend codec, best first.
pub fn candidates_for(codec: HwCodec) -> Vec<Candidate> {
    match codec {
        // Apple's encoder; `allow_sw` lets it fall back to Apple's software path.
        HwCodec::VideoToolbox => vec![c(
            "h264_videotoolbox",
            &[("allow_sw", "1"), ("profile", "high"), ("realtime", "0")],
        )],
        HwCodec::Nvenc => vec![c(
            "h264_nvenc",
            &[("preset", "p5"), ("profile", "high"), ("rc", "vbr")],
        )],
        HwCodec::Amf => vec![c(
            "h264_amf",
            &[
                ("quality", "quality"),
                ("profile", "high"),
                ("rc", "vbr_peak"),
            ],
        )],
        // QSV wants NV12 input; it joins the table when the compositor can emit NV12.
        HwCodec::Qsv => vec![],
        // Media Foundation: the GPU path first, then Microsoft's software encoder (always present).
        HwCodec::MediaFoundation => vec![
            c(
                "h264_mf",
                &[
                    ("hw_encoding", "1"),
                    ("rate_control", "u_vbr"),
                    ("scenario", "archive"),
                ],
            ),
            c(
                "h264_mf",
                &[
                    ("hw_encoding", "0"),
                    ("rate_control", "u_vbr"),
                    ("scenario", "archive"),
                ],
            ),
        ],
        HwCodec::Software if cfg!(target_os = "windows") => vec![c(
            "h264_mf",
            &[("hw_encoding", "0"), ("rate_control", "u_vbr")],
        )],
        HwCodec::Software if cfg!(target_os = "macos") => candidates_for(HwCodec::VideoToolbox),
        HwCodec::Software | HwCodec::D3d11va => vec![],
    }
}

/// This machine's ordered encoder candidates, detected once.
///
/// `CHOCO_ENCODERS` (comma-separated FFmpeg names, e.g. `h264_mf`) narrows the list, so CI can
/// force a path and a user can work around a broken driver.
pub fn platform_candidates() -> &'static [Candidate] {
    static LIST: OnceLock<Vec<Candidate>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut list: Vec<Candidate> = Vec::new();
        for codec in crate::backend::detect()
            .hw_encoders()
            .into_iter()
            .chain([HwCodec::Software])
        {
            for cand in candidates_for(codec) {
                if !list.contains(&cand) {
                    list.push(cand);
                }
            }
        }
        if let Ok(only) = std::env::var("CHOCO_ENCODERS") {
            let names: Vec<&str> = only
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            list.retain(|c| names.contains(&c.name));
        }
        list
    })
}

/// Why every encoder may have failed, when the cause is the machine rather than the clip.
/// Windows N editions ship without Media Foundation (`mfplat.dll`), which the Media Foundation
/// encoders need; the fix is Microsoft's Media Feature Pack, not anything in choco.video.
pub fn missing_runtime_hint() -> Option<&'static str> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let root = std::env::var_os("SystemRoot")?;
    let mf = std::path::Path::new(&root)
        .join("System32")
        .join("mfplat.dll");
    (!mf.is_file()).then_some(
        ". This looks like a Windows N edition without Media Foundation: install the Media \
         Feature Pack (Settings > System > Optional features > Add a feature), then try again",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_backend_codec_maps_to_yuv420p_encoders() {
        for codec in [
            HwCodec::VideoToolbox,
            HwCodec::Nvenc,
            HwCodec::Amf,
            HwCodec::Qsv,
            HwCodec::MediaFoundation,
            HwCodec::Software,
            HwCodec::D3d11va,
        ] {
            for cand in candidates_for(codec) {
                assert_eq!(
                    cand.pixel,
                    ff::format::Pixel::YUV420P,
                    "{} must take what the compositor produces",
                    cand.name
                );
                assert!(cand.name.starts_with("h264_"));
            }
        }
    }

    #[test]
    fn media_foundation_falls_back_to_software() {
        let mf = candidates_for(HwCodec::MediaFoundation);
        assert_eq!(mf.len(), 2);
        assert!(mf[0].options.contains(&("hw_encoding", "1")));
        assert!(mf[1].options.contains(&("hw_encoding", "0")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_prefers_videotoolbox_with_the_same_options_as_before() {
        let first = &platform_candidates()[0];
        assert_eq!(first.name, "h264_videotoolbox");
        assert_eq!(
            first.options,
            &[("allow_sw", "1"), ("profile", "high"), ("realtime", "0")]
        );
    }
}

//! Final Cut Pro XML (FCPXML 1.10) for a finished job: one event holding the source video, one
//! project per clip, cut to the clip's in and out points on the original media so an editor can
//! keep refining the cut at full quality. DaVinci Resolve imports the same file. Times are whole
//! frames of the source rate, which Final Cut requires to place an edit without warnings.

use crate::model::{Candidate, MediaAsset};
use std::fmt::Write;

/// The document, or None for a source without video.
pub fn fcpxml(asset: &MediaAsset, clips: &[(usize, &Candidate)]) -> Option<String> {
    let v = asset.video.as_ref()?;
    let (num, den) = match (v.fps_num, v.fps_den) {
        (n, d) if n > 0 && d > 0 => (u64::from(n), u64::from(d)),
        _ => (30, 1),
    };
    // Whole frames covering `ms`, rounded down for in points and to the nearest for lengths.
    let floor = |ms: u64| ms * num / (den * 1000);
    let round = |ms: u64| (ms * num + den * 500) / (den * 1000);
    let t = |frames: u64| {
        if frames == 0 {
            "0s".to_string()
        } else {
            format!("{}/{}s", frames * den, num)
        }
    };
    let (rate, layout, channels) = match &asset.audio {
        Some(a) => (
            a.sample_rate,
            if a.channels == 1 { "mono" } else { "stereo" },
            a.channels.max(1),
        ),
        None => (48_000, "stereo", 2),
    };
    let seq_rate = match rate {
        32_000 => "32k",
        44_100 => "44.1k",
        88_200 => "88.2k",
        96_000 => "96k",
        _ => "48k",
    };
    let name = asset
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".into());

    let mut x = String::new();
    let _ = writeln!(x, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(x, "<!DOCTYPE fcpxml>");
    let _ = writeln!(x, r#"<fcpxml version="1.10">"#);
    let _ = writeln!(x, "  <resources>");
    let _ = writeln!(
        x,
        r#"    <format id="r1" frameDuration="{}" width="{}" height="{}"/>"#,
        t(1),
        v.display_width,
        v.display_height
    );
    let _ = writeln!(
        x,
        r#"    <asset id="r2" name="{}" start="0s" duration="{}" hasVideo="1" format="r1"{}>"#,
        esc(&name),
        t(round(asset.duration_ms).max(1)),
        if asset.audio.is_some() {
            format!(
                r#" hasAudio="1" audioSources="1" audioChannels="{channels}" audioRate="{rate}""#
            )
        } else {
            String::new()
        }
    );
    let _ = writeln!(
        x,
        r#"      <media-rep kind="original-media" src="{}"/>"#,
        esc(&file_url(&asset.path))
    );
    let _ = writeln!(x, "    </asset>");
    let _ = writeln!(x, "  </resources>");
    let _ = writeln!(x, "  <library>");
    let _ = writeln!(x, r#"    <event name="choco.video: {}">"#, esc(&name));
    for (rank, c) in clips {
        let start = floor(c.start_ms);
        let len = round(c.end_ms.saturating_sub(c.start_ms)).max(1);
        let title = format!("{rank:02} {}", c.title);
        let _ = writeln!(x, r#"      <project name="{}">"#, esc(&title));
        let _ = writeln!(
            x,
            r#"        <sequence format="r1" duration="{}" tcStart="0s" tcFormat="NDF" audioLayout="{layout}" audioRate="{seq_rate}">"#,
            t(len)
        );
        let _ = writeln!(x, "          <spine>");
        let _ = writeln!(
            x,
            r#"            <asset-clip ref="r2" offset="0s" name="{}" start="{}" duration="{}" format="r1" tcFormat="NDF">"#,
            esc(&c.title),
            t(start),
            t(len)
        );
        let note = format!("Score {}. {}", c.score, c.reasons.join(" · "));
        let _ = writeln!(x, "              <note>{}</note>", esc(note.trim()));
        let _ = writeln!(x, "            </asset-clip>");
        let _ = writeln!(x, "          </spine>");
        let _ = writeln!(x, "        </sequence>");
        let _ = writeln!(x, "      </project>");
    }
    let _ = writeln!(x, "    </event>");
    let _ = writeln!(x, "  </library>");
    let _ = writeln!(x, "</fcpxml>");
    Some(x)
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            // Control characters are not allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

/// `file:///` URL for an absolute path, percent-encoded byte by byte; Windows drive paths become
/// `file:///C:/…`.
fn file_url(path: &std::path::Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let p = abs.to_string_lossy().replace('\\', "/");
    let p = p.strip_prefix("//?/").unwrap_or(&p);
    let mut out = String::from("file://");
    if !p.starts_with('/') {
        out.push('/');
    }
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(b as char)
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioStream, PickerProvenance, VideoStream};

    fn asset(path: &str) -> MediaAsset {
        MediaAsset {
            path: path.into(),
            content_hash: String::new(),
            duration_ms: 4_312_000,
            container: "mov".into(),
            video: Some(VideoStream {
                index: 0,
                codec: "h264".into(),
                width: 1920,
                height: 1080,
                display_width: 1920,
                display_height: 1080,
                fps_num: 30_000,
                fps_den: 1_001,
                rotation_deg: 0,
            }),
            audio: Some(AudioStream {
                index: 1,
                codec: "aac".into(),
                sample_rate: 48_000,
                channels: 2,
            }),
        }
    }

    fn clip(start_ms: u64, end_ms: u64, title: &str) -> Candidate {
        Candidate {
            id: "w1".into(),
            start_ms,
            end_ms,
            score: 82,
            reasons: vec!["strong hook".into(), "clean payoff".into()],
            title: title.into(),
            description: String::new(),
            hashtags: vec![],
            provenance: PickerProvenance {
                tier: "provider".into(),
                model: "m".into(),
                prompt_hash: String::new(),
            },
        }
    }

    #[test]
    fn cuts_each_clip_on_whole_source_frames() {
        let a = asset("/Users/me/Talks/vidéo one.mov");
        let (c1, c2) = (
            clip(2_404_440, 2_493_060, "Money & <power>"),
            clip(0, 33_000, "Opener"),
        );
        let x = fcpxml(&a, &[(1, &c1), (2, &c2)]).unwrap();
        assert!(x.contains(r#"frameDuration="1001/30000s""#));
        assert!(
            x.contains(r#"src="file:///Users/me/Talks/vid%C3%A9o%20one.mov""#),
            "{x}"
        );
        assert!(x.contains(r#"<project name="01 Money &amp; &lt;power&gt;">"#));
        // 2,404,440 ms is frame 72,061.1 → in point 72,061; 88.62 s → 2,656 frames.
        assert!(x.contains(&format!(r#"start="{}/30000s""#, 72_061 * 1_001)));
        assert!(x.contains(&format!(
            r#"duration="{}/30000s" format="r1""#,
            2_656 * 1_001
        )));
        assert!(x.contains(r#"<project name="02 Opener">"#));
        assert!(x.contains(r#"offset="0s" name="Opener" start="0s""#));
        assert!(x.contains("<note>Score 82. strong hook · clean payoff</note>"));
        assert!(x.contains(r#"audioLayout="stereo" audioRate="48k""#));
        assert_eq!(x.matches("<project ").count(), 2);
        assert_eq!(x.matches("</project>").count(), 2);
    }

    #[test]
    fn no_video_no_document() {
        let mut a = asset("/tmp/a.m4a");
        a.video = None;
        assert!(fcpxml(&a, &[]).is_none());
    }

    #[test]
    fn windows_paths_become_drive_urls() {
        let u = file_url(std::path::Path::new(r"C:\Users\me\clip 1.mp4"));
        if cfg!(windows) {
            assert_eq!(u, "file:///C:/Users/me/clip%201.mp4");
        } else {
            assert!(
                u.starts_with("file:///") && u.ends_with("clip%201.mp4"),
                "{u}"
            );
        }
    }
}

//! Final Cut Pro 7 XML (xmeml version 5) for a finished job: one sequence per clip, cut to the
//! clip's in and out points on the original media, for Premiere Pro and DaVinci Resolve. Every
//! clip item points at one shared `<file>`, defined in full the first time. Times are whole
//! frames; NTSC rates are written as the rounded timebase with `<ntsc>TRUE</ntsc>`.

use super::fcpxml::{esc, file_url};
use crate::model::{Candidate, MediaAsset};
use std::fmt::Write;

/// The document, or None for a source without video.
pub fn xmeml(asset: &MediaAsset, clips: &[(usize, &Candidate)]) -> Option<String> {
    let v = asset.video.as_ref()?;
    let (tb, ntsc) = match (v.fps_num, v.fps_den) {
        (n, d) if n > 0 && d > 0 => timebase(u64::from(n), u64::from(d)),
        _ => (30, false),
    };
    // Frames are counted at the true rate: 30000/1001 for NTSC 30, the timebase otherwise.
    let (num, den) = if ntsc { (tb * 1000, 1001) } else { (tb, 1) };
    let floor = |ms: u64| ms * num / (den * 1000);
    let round = |ms: u64| (ms * num + den * 500) / (den * 1000);
    let rate = format!(
        "<rate><timebase>{tb}</timebase><ntsc>{}</ntsc></rate>",
        if ntsc { "TRUE" } else { "FALSE" }
    );
    let channels = asset.audio.as_ref().map_or(0, |a| a.channels.max(1));
    let sample_rate = asset.audio.as_ref().map_or(48_000, |a| a.sample_rate);
    let total = round(asset.duration_ms).max(1);
    let name = |p: Option<&std::ffi::OsStr>| {
        p.map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "video".into())
    };
    let (stem, file_name) = (name(asset.path.file_stem()), name(asset.path.file_name()));

    // Written once, inside the first clip item; every later item refers back to it by id.
    let mut file = String::new();
    let _ = writeln!(file, r#"                <file id="file-1">"#);
    let _ = writeln!(file, "                  <name>{}</name>", esc(&file_name));
    let _ = writeln!(
        file,
        "                  <pathurl>{}</pathurl>",
        esc(&file_url(&asset.path))
    );
    let _ = writeln!(file, "                  {rate}");
    let _ = writeln!(file, "                  <duration>{total}</duration>");
    let _ = writeln!(file, "                  <media>");
    let _ = writeln!(
        file,
        "                    <video><samplecharacteristics>{rate}<width>{}</width><height>{}</height></samplecharacteristics></video>",
        v.display_width, v.display_height
    );
    if channels > 0 {
        let _ = writeln!(
            file,
            "                    <audio><samplecharacteristics><depth>16</depth><samplerate>{sample_rate}</samplerate></samplecharacteristics><channelcount>{channels}</channelcount></audio>"
        );
    }
    let _ = writeln!(file, "                  </media>");
    let _ = writeln!(file, "                </file>");
    let mut file = Some(file);

    let mut x = String::new();
    let _ = writeln!(x, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(x, "<!DOCTYPE xmeml>");
    let _ = writeln!(x, r#"<xmeml version="5">"#);
    let _ = writeln!(x, "  <project>");
    let _ = writeln!(x, "    <name>choco.video: {}</name>", esc(&stem));
    let _ = writeln!(x, "    <children>");
    for (i, (rank, c)) in clips.iter().enumerate() {
        let n = i + 1;
        let start = floor(c.start_ms);
        let len = round(c.end_ms.saturating_sub(c.start_ms)).max(1);
        let note = format!("Score {}. {}", c.score, c.reasons.join(" · "));
        // Video and its audio channels move together in the timeline.
        let mut links = String::new();
        if channels > 0 {
            let _ = writeln!(
                links,
                "                <link><linkclipref>clipitem-{n}-v</linkclipref><mediatype>video</mediatype><trackindex>1</trackindex><clipindex>1</clipindex></link>"
            );
        }
        for ch in 1..=channels {
            let _ = writeln!(
                links,
                "                <link><linkclipref>clipitem-{n}-a{ch}</linkclipref><mediatype>audio</mediatype><trackindex>{ch}</trackindex><clipindex>1</clipindex><groupindex>1</groupindex></link>"
            );
        }
        // The fields every clip item shares, video or audio.
        let item = |id: &str| {
            format!(
                "            <track>\n              <clipitem id=\"{id}\">\n                <name>{}</name>\n                <enabled>TRUE</enabled>\n                <duration>{total}</duration>\n                {rate}\n                <start>0</start>\n                <end>{len}</end>\n                <in>{start}</in>\n                <out>{}</out>\n",
                esc(&c.title),
                start + len
            )
        };

        let _ = writeln!(x, r#"      <sequence id="sequence-{n}">"#);
        let _ = writeln!(
            x,
            "        <name>{}</name>",
            esc(&format!("{rank:02} {}", c.title))
        );
        let _ = writeln!(x, "        <duration>{len}</duration>");
        let _ = writeln!(x, "        {rate}");
        let _ = writeln!(
            x,
            "        <timecode>{rate}<string>00:00:00:00</string><frame>0</frame><displayformat>NDF</displayformat></timecode>"
        );
        let _ = writeln!(x, "        <media>");
        let _ = writeln!(x, "          <video>");
        let _ = writeln!(
            x,
            "            <format><samplecharacteristics>{rate}<width>{}</width><height>{}</height><anamorphic>FALSE</anamorphic><pixelaspectratio>square</pixelaspectratio><fielddominance>none</fielddominance></samplecharacteristics></format>",
            v.display_width, v.display_height
        );
        x.push_str(&item(&format!("clipitem-{n}-v")));
        match file.take() {
            Some(f) => x.push_str(&f),
            None => {
                let _ = writeln!(x, r#"                <file id="file-1"/>"#);
            }
        }
        x.push_str(&links);
        let _ = writeln!(
            x,
            "                <logginginfo><description>{}</description></logginginfo>",
            esc(note.trim())
        );
        let _ = writeln!(x, "              </clipitem>");
        let _ = writeln!(x, "            </track>");
        let _ = writeln!(x, "          </video>");
        if channels > 0 {
            let _ = writeln!(x, "          <audio>");
            let _ = writeln!(x, "            <numOutputChannels>2</numOutputChannels>");
            let _ = writeln!(
                x,
                "            <format><samplecharacteristics><depth>16</depth><samplerate>{sample_rate}</samplerate></samplecharacteristics></format>"
            );
            // One mono track per source channel, as Final Cut Pro 7 laid them out.
            for ch in 1..=channels {
                x.push_str(&item(&format!("clipitem-{n}-a{ch}")));
                let _ = writeln!(x, r#"                <file id="file-1"/>"#);
                let _ = writeln!(
                    x,
                    "                <sourcetrack><mediatype>audio</mediatype><trackindex>{ch}</trackindex></sourcetrack>"
                );
                x.push_str(&links);
                let _ = writeln!(x, "              </clipitem>");
                let _ = writeln!(x, "            </track>");
            }
            let _ = writeln!(x, "          </audio>");
        }
        let _ = writeln!(x, "        </media>");
        let _ = writeln!(x, "      </sequence>");
    }
    let _ = writeln!(x, "    </children>");
    let _ = writeln!(x, "  </project>");
    let _ = writeln!(x, "</xmeml>");
    Some(x)
}

/// Whole frames per second for a rate, and whether the rate is that times 1000/1001 (23.976,
/// 29.97, 59.94), which xmeml writes as the rounded timebase flagged NTSC.
fn timebase(num: u64, den: u64) -> (u64, bool) {
    let tb = ((num + den / 2) / den).max(1);
    // Within 0.01% of the NTSC rate, well inside the 0.1% that separates it from the whole one,
    // so a probe's 2997/100 counts too.
    let ntsc_rate = tb * 1000 * den;
    (tb, (num * 1001).abs_diff(ntsc_rate) * 10_000 <= ntsc_rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioStream, PickerProvenance, VideoStream};

    fn asset(path: &str, fps_num: u32, fps_den: u32) -> MediaAsset {
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
                fps_num,
                fps_den,
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
            reasons: vec!["strong \"hook\"".into(), "clean payoff".into()],
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
    fn ntsc_rates_round_up_and_are_flagged() {
        assert_eq!(timebase(24_000, 1_001), (24, true));
        assert_eq!(timebase(30_000, 1_001), (30, true));
        assert_eq!(timebase(60_000, 1_001), (60, true));
        assert_eq!(timebase(2_997, 100), (30, true));
        assert_eq!(timebase(25, 1), (25, false));
        assert_eq!(timebase(30, 1), (30, false));
        assert_eq!(timebase(60, 1), (60, false));
    }

    #[test]
    fn cuts_on_whole_frames_at_29_97() {
        let a = asset("/Users/me/Talks/vidéo one.mov", 30_000, 1_001);
        let c = clip(2_404_440, 2_493_060, "Money & <power>");
        let x = xmeml(&a, &[(1, &c)]).unwrap();
        assert!(x.contains("<rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>"));
        // 2,404,440 ms is frame 72,061.1 → in point 72,061; 88.62 s → 2,656 frames.
        assert!(x.contains("<start>0</start>"));
        assert!(x.contains("<end>2656</end>"));
        assert!(x.contains("<in>72061</in>"));
        assert!(x.contains(&format!("<out>{}</out>", 72_061 + 2_656)));
        // 4,312 s of source → 129,231 frames.
        assert!(x.contains("<duration>129231</duration>"));
        assert!(
            x.contains("<pathurl>file:///Users/me/Talks/vid%C3%A9o%20one.mov</pathurl>"),
            "{x}"
        );
        assert!(x.contains("<width>1920</width><height>1080</height>"));
        assert!(x.contains("<channelcount>2</channelcount>"));
        assert!(x.contains(r#"<clipitem id="clipitem-1-a2">"#));
    }

    #[test]
    fn cuts_on_whole_frames_at_25() {
        let a = asset("/tmp/a.mp4", 25, 1);
        let c = clip(2_404_440, 2_493_060, "Opener");
        let x = xmeml(&a, &[(1, &c)]).unwrap();
        assert!(x.contains("<rate><timebase>25</timebase><ntsc>FALSE</ntsc></rate>"));
        // Frame 60,111.0 in; 88.62 s is 2,215.5 frames, rounded to 2,216.
        assert!(x.contains("<in>60111</in>"));
        assert!(x.contains("<end>2216</end>"));
        assert!(x.contains("<out>62327</out>"));
    }

    #[test]
    fn escapes_names_and_notes() {
        let a = asset("/tmp/a&b.mp4", 25, 1);
        let c = clip(0, 1_000, "Money & <power>");
        let x = xmeml(&a, &[(3, &c)]).unwrap();
        assert!(x.contains("<name>03 Money &amp; &lt;power&gt;</name>"));
        assert!(x.contains("<name>Money &amp; &lt;power&gt;</name>"));
        assert!(x.contains("<name>a&amp;b.mp4</name>"));
        assert!(x.contains(
            "<description>Score 82. strong &quot;hook&quot; · clean payoff</description>"
        ));
        assert!(!x.contains("<power>"));
    }

    #[test]
    fn every_sequence_shares_one_file() {
        let a = asset("/tmp/a.mp4", 30_000, 1_001);
        let (c1, c2) = (clip(0, 30_000, "One"), clip(60_000, 90_000, "Two"));
        let x = xmeml(&a, &[(1, &c1), (2, &c2)]).unwrap();
        assert_eq!(x.matches("<sequence ").count(), 2);
        assert_eq!(x.matches(r#"<file id="file-1">"#).count(), 1);
        // Two clips × (one video + two audio items), less the full definition.
        assert_eq!(x.matches(r#"<file id="file-1"/>"#).count(), 5);
        assert!(x.find(r#"<file id="file-1">"#) < x.find(r#"<file id="file-1"/>"#));
        assert!(x.contains(r#"<sequence id="sequence-2">"#));
        assert!(x.contains("<name>02 Two</name>"));
    }

    #[test]
    fn silent_source_has_no_audio_items() {
        let mut a = asset("/tmp/a.mp4", 25, 1);
        a.audio = None;
        let x = xmeml(&a, &[(1, &clip(0, 1_000, "One"))]).unwrap();
        assert!(!x.contains("<audio>") && !x.contains("<link>"));
    }

    #[test]
    fn no_video_no_document() {
        let mut a = asset("/tmp/a.m4a", 25, 1);
        a.video = None;
        assert!(xmeml(&a, &[]).is_none());
    }

    /// Needs `xmllint` on PATH.
    #[test]
    #[ignore]
    fn well_formed_per_xmllint() {
        let mut silent = asset("/tmp/a b.mp4", 24_000, 1_001);
        silent.audio = None;
        let (c1, c2) = (clip(0, 30_000, "A & \"B\" <C>"), clip(1_000, 9_000, "Two"));
        for a in [silent, asset("/tmp/vidéo.mov", 30_000, 1_001)] {
            let x = xmeml(&a, &[(1, &c1), (2, &c2)]).unwrap();
            let p = std::env::temp_dir().join("choco-xmeml-check.xml");
            std::fs::write(&p, x).unwrap();
            let out = std::process::Command::new("xmllint")
                .arg("--noout")
                .arg(&p)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

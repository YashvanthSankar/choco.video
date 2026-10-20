//! Caption rasterizer. Bundled fonts only, so the same chunk produces the same pixels on every
//! machine and OS. Output is a premultiplied RGBA sprite placed in frame coordinates.

use crate::model::{CaptionChunk, CaptionStyle};
use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap,
    fontdb,
};

const FONTS: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Inter-ExtraBold.ttf"),
    include_bytes!("../../assets/fonts/Montserrat-ExtraBold.ttf"),
    include_bytes!("../../assets/fonts/Montserrat-Black.ttf"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    /// Top-left position in the frame. May be negative near edges; the compositor clips.
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// Premultiplied RGBA, row-major, `w * h * 4` bytes.
    pub rgba: Vec<u8>,
}

pub struct Rasterizer {
    fs: FontSystem,
    cache: SwashCache,
}

impl Default for Rasterizer {
    fn default() -> Self {
        Self::new()
    }
}

/// "#RRGGBB" or "#RRGGBBAA"; anything unparsable is opaque white.
pub fn parse_color(s: &str) -> [u8; 4] {
    let h = s.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255);
    match h.len() {
        6 => [byte(0), byte(2), byte(4), 255],
        8 => [byte(0), byte(2), byte(4), byte(6)],
        _ => [255, 255, 255, 255],
    }
}

fn case(s: &str, style: &CaptionStyle) -> String {
    if style.uppercase {
        s.to_uppercase()
    } else {
        s.to_string()
    }
}

/// Source-over blend of a solid colour with `coverage` (0..255) into a premultiplied pixel.
#[inline]
fn blend(dst: &mut [u8], rgb: [u8; 3], alpha: u32) {
    if alpha == 0 {
        return;
    }
    let inv = 255 - alpha;
    for c in 0..3 {
        dst[c] = ((rgb[c] as u32 * alpha + dst[c] as u32 * inv + 127) / 255) as u8;
    }
    dst[3] = ((alpha * 255 + dst[3] as u32 * inv + 127) / 255) as u8;
}

impl Rasterizer {
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        for f in FONTS {
            db.load_font_data(f.to_vec());
        }
        Self {
            fs: FontSystem::new_with_locale_and_db("en-US".into(), db),
            cache: SwashCache::new(),
        }
    }

    fn px(style: &CaptionStyle, frame_h: u32) -> f32 {
        (style.size_pct / 100.0 * frame_h as f32).max(8.0)
    }

    fn attrs(style: &CaptionStyle) -> Attrs<'_> {
        Attrs::new()
            .family(Family::Name(&style.font_family))
            .weight(Weight(style.font_weight))
    }

    /// Width in pixels of `text` on one line in `style` (active-word scaling included).
    pub fn measure(&mut self, text: &str, style: &CaptionStyle, frame_h: u32) -> f32 {
        let px = Self::px(style, frame_h);
        let mut b = Buffer::new(&mut self.fs, Metrics::new(px, px * 1.15));
        b.set_wrap(Wrap::None);
        b.set_size(None, None);
        b.set_text(
            &case(text, style),
            &Self::attrs(style),
            Shaping::Advanced,
            None,
        );
        b.shape_until_scroll(&mut self.fs, false);
        let w = b.layout_runs().map(|r| r.line_w).fold(0.0, f32::max);
        // Leave room for the active word growing.
        w * style.active_scale.max(1.0)
    }

    /// Rasterize `chunk` with `active` highlighted (per the style's highlight mode).
    pub fn render(
        &mut self,
        chunk: &CaptionChunk,
        active: Option<usize>,
        style: &CaptionStyle,
        frame_w: u32,
        frame_h: u32,
    ) -> Sprite {
        let px = Self::px(style, frame_h);
        let max_w = frame_w as f32 * 0.86;
        let fill = parse_color(&style.fill);
        let active_fill = style.active_fill.as_deref().map(parse_color);
        let highlighted = |i: usize| match (active, style.highlight.as_str()) {
            (Some(a), "progressive") => i <= a,
            (Some(a), _) => i == a,
            (None, _) => false,
        };

        let base = Self::attrs(style).color(Color::rgba(fill[0], fill[1], fill[2], fill[3]));
        let words: Vec<String> = chunk
            .words
            .iter()
            .map(|w| case(w.text.trim(), style))
            .collect();
        let mut spans: Vec<(&str, Attrs)> = Vec::new();
        for (i, w) in words.iter().enumerate() {
            let mut a = base.clone().metadata(i + 1);
            if highlighted(i) {
                if let Some(c) = active_fill {
                    a = a.color(Color::rgba(c[0], c[1], c[2], c[3]));
                }
                if Some(i) == active && (style.active_scale - 1.0).abs() > 0.001 {
                    let s = px * style.active_scale;
                    a = a.metrics(Metrics::new(s, s * 1.15));
                }
            }
            spans.push((w.as_str(), a));
            if i + 1 < words.len() {
                spans.push((" ", base.clone()));
            }
        }
        let mut buf = Buffer::new(&mut self.fs, Metrics::new(px, px * 1.15));
        buf.set_wrap(Wrap::Word);
        buf.set_size(Some(max_w), None);
        buf.set_rich_text(spans, &base, Shaping::Advanced, Some(Align::Center));
        buf.shape_until_scroll(&mut self.fs, false);

        // Bounds of the laid-out glyphs, and of the active word for a pill.
        let (mut x0, mut x1, mut y1) = (f32::MAX, f32::MIN, 0.0f32);
        let mut word_box: Option<(f32, f32, f32, f32)> = None;
        for run in buf.layout_runs() {
            y1 = y1.max(run.line_top + run.line_height);
            for g in run.glyphs {
                x0 = x0.min(g.x);
                x1 = x1.max(g.x + g.w);
                if active.is_some_and(|a| g.metadata == a + 1) {
                    let b = word_box.get_or_insert((
                        g.x,
                        run.line_top,
                        g.x + g.w,
                        run.line_top + run.line_height,
                    ));
                    b.0 = b.0.min(g.x);
                    b.2 = b.2.max(g.x + g.w);
                }
            }
        }
        if x0 > x1 {
            return Sprite {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
                rgba: vec![],
            };
        }

        let stroke_r = if style.stroke.is_some() {
            (px * 0.09).ceil()
        } else {
            0.0
        };
        let shadow_d = if style.shadow {
            (px * 0.06).ceil()
        } else {
            0.0
        };
        let pill_pad = if style.active_pill.is_some() {
            (px * 0.22).ceil()
        } else {
            0.0
        };
        let pad = stroke_r + shadow_d + pill_pad + 2.0;
        let w = (x1 - x0 + 2.0 * pad).ceil() as u32;
        let h = (y1 + 2.0 * pad).ceil() as u32;
        let (ox, oy) = (pad - x0, pad);
        let mut canvas = vec![0u8; (w * h * 4) as usize];

        // Pill: behind the active word, or behind the whole block when there is no active colour.
        if let Some(pc) = style.active_pill.as_deref().map(parse_color) {
            let rect = match (active_fill, word_box) {
                (Some(_), Some(b)) => Some(b),
                (None, _) => Some((x0, 0.0, x1, y1)),
                _ => None,
            };
            if let Some((a, b, c, d)) = rect {
                let r = px * 0.25;
                let (rx0, ry0, rx1, ry1) = (
                    a + ox - pill_pad,
                    b + oy - pill_pad * 0.4,
                    c + ox + pill_pad,
                    d + oy + pill_pad * 0.4,
                );
                for y in ry0.max(0.0) as u32..(ry1.ceil() as u32).min(h) {
                    for x in rx0.max(0.0) as u32..(rx1.ceil() as u32).min(w) {
                        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                        let dx = (rx0 + r - fx).max(fx - (rx1 - r)).max(0.0);
                        let dy = (ry0 + r - fy).max(fy - (ry1 - r)).max(0.0);
                        let cov = (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
                        let i = ((y * w + x) * 4) as usize;
                        blend(
                            &mut canvas[i..i + 4],
                            [pc[0], pc[1], pc[2]],
                            (cov * pc[3] as f32) as u32,
                        );
                    }
                }
            }
        }

        let mut pass = |canvas: &mut Vec<u8>,
                        dx: f32,
                        dy: f32,
                        solid: Option<[u8; 4]>,
                        fs: &mut FontSystem,
                        cache: &mut SwashCache| {
            buf.draw(
                fs,
                cache,
                Color::rgba(fill[0], fill[1], fill[2], fill[3]),
                |x, y, gw, gh, c| {
                    let (rgb, a) = match solid {
                        Some(s) => ([s[0], s[1], s[2]], c.a() as u32 * s[3] as u32 / 255),
                        None => ([c.r(), c.g(), c.b()], c.a() as u32),
                    };
                    for yy in 0..gh as i32 {
                        for xx in 0..gw as i32 {
                            let px_ = (x + xx) as f32 + ox + dx;
                            let py_ = (y + yy) as f32 + oy + dy;
                            if px_ < 0.0 || py_ < 0.0 || px_ >= w as f32 || py_ >= h as f32 {
                                continue;
                            }
                            let i = ((py_ as u32 * w + px_ as u32) * 4) as usize;
                            blend(&mut canvas[i..i + 4], rgb, a);
                        }
                    }
                },
            );
        };
        if style.shadow {
            pass(
                &mut canvas,
                shadow_d * 0.5,
                shadow_d,
                Some([0, 0, 0, 150]),
                &mut self.fs,
                &mut self.cache,
            );
        }
        if let Some(sc) = style.stroke.as_deref().map(parse_color) {
            for k in 0..16 {
                let t = k as f32 * std::f32::consts::TAU / 16.0;
                pass(
                    &mut canvas,
                    (stroke_r * t.cos()).round(),
                    (stroke_r * t.sin()).round(),
                    Some(sc),
                    &mut self.fs,
                    &mut self.cache,
                );
            }
        }
        pass(&mut canvas, 0.0, 0.0, None, &mut self.fs, &mut self.cache);

        let bottom = frame_h as f32 * (1.0 - style.position_pct / 100.0);
        Sprite {
            x: ((frame_w as f32 - w as f32) / 2.0).round() as i32,
            y: (bottom - h as f32 + pad).round() as i32,
            w,
            h,
            rgba: canvas,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::captions::preset;
    use crate::model::Word;

    fn chunk(words: &[&str]) -> CaptionChunk {
        CaptionChunk {
            start_ms: 0,
            end_ms: 1000,
            words: words
                .iter()
                .enumerate()
                .map(|(i, t)| Word {
                    start_ms: i as u64 * 200,
                    end_ms: i as u64 * 200 + 180,
                    text: t.to_string(),
                    confidence: 1.0,
                })
                .collect(),
        }
    }

    fn colored(s: &Sprite, rgb: [u8; 3]) -> usize {
        s.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 200 && (0..3).all(|c| (p[c] as i32 - rgb[c] as i32).abs() < 40))
            .count()
    }

    #[test]
    fn renders_visible_centered_text_deterministically() {
        let mut r = Rasterizer::new();
        let st = preset("bold").unwrap();
        let a = r.render(&chunk(&["Charge", "more"]), Some(0), &st, 1080, 1920);
        let b = r.render(&chunk(&["Charge", "more"]), Some(0), &st, 1080, 1920);
        assert_eq!(a, b, "same input must give identical pixels");
        assert!(a.w > 100 && a.h > 50, "{}x{}", a.w, a.h);
        let centre = a.x + a.w as i32 / 2;
        assert!((centre - 540).abs() <= 2, "centre {centre}");
        // Bottom of the block sits about 24% up from the bottom edge.
        assert!(a.y + (a.h as i32) < 1920 - 400 && a.y > 1000, "y {}", a.y);
        assert!(colored(&a, [255, 225, 77]) > 200, "active word in yellow");
        assert!(colored(&a, [255, 255, 255]) > 200, "inactive word in white");
        assert!(colored(&a, [0, 0, 0]) > 200, "outline present");
    }

    #[test]
    fn active_word_changes_pixels_and_uppercase_applies() {
        let mut r = Rasterizer::new();
        let st = preset("bold").unwrap();
        let a = r.render(&chunk(&["one", "two"]), Some(0), &st, 1080, 1920);
        let b = r.render(&chunk(&["one", "two"]), Some(1), &st, 1080, 1920);
        assert_ne!(a.rgba, b.rgba);
        assert!(
            r.measure("abc", &st, 1920) > r.measure("abc", &preset("clean").unwrap(), 1920) * 0.9
        );
    }

    #[test]
    fn minimal_style_draws_a_background_box() {
        let mut r = Rasterizer::new();
        let s = r.render(
            &chunk(&["quiet", "words"]),
            None,
            &preset("minimal").unwrap(),
            1080,
            1920,
        );
        let translucent = s
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 120 && p[3] < 220)
            .count();
        assert!(translucent > 1000, "{translucent}");
    }

    /// Pixel hashes of every preset, recorded on macOS arm64. The Windows x64 runner must produce
    /// the same bytes: captions use only the bundled fonts, so nothing may differ by OS. After a
    /// deliberate style change, copy the table from the failure message.
    const RASTER_HASHES: &[(&str, &str)] = &[
        ("clean", "5281d2e8f490e05c"),
        ("bold", "1bf4c80b1a306d85"),
        ("karaoke", "cc85fac3925513de"),
        ("minimal", "77c9cbba15611abe"),
    ];

    #[test]
    fn every_preset_rasterizes_identically_on_every_os() {
        let mut r = Rasterizer::new();
        let got: Vec<(String, String)> = crate::captions::presets()
            .iter()
            .map(|st| {
                let s = r.render(&chunk(&["Charge", "more", "now"]), Some(1), st, 1080, 1920);
                let mut h = blake3::Hasher::new();
                for v in [s.x as i64, s.y as i64, s.w as i64, s.h as i64] {
                    h.update(&v.to_le_bytes());
                }
                h.update(&s.rgba);
                (st.name.clone(), h.finalize().to_hex()[..16].to_string())
            })
            .collect();
        let want: Vec<(String, String)> = RASTER_HASHES
            .iter()
            .map(|(n, h)| (n.to_string(), h.to_string()))
            .collect();
        let table: String = got
            .iter()
            .map(|(n, h)| format!("        (\"{n}\", \"{h}\"),\n"))
            .collect();
        assert_eq!(got, want, "caption pixels changed; new table:\n{table}");
    }

    #[test]
    fn parses_colours() {
        assert_eq!(parse_color("#FFE14D"), [255, 225, 77, 255]);
        assert_eq!(parse_color("#000000B3"), [0, 0, 0, 179]);
        assert_eq!(parse_color("nope"), [255, 255, 255, 255]);
    }
}

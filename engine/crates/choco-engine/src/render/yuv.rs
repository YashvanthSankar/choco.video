//! 8-bit 4:2:0 frame operations for the CPU compositor: normalise, rotate, crop-scale, blur,
//! and blend caption sprites. BT.709 limited range throughout.

use crate::captions::Sprite;
use ff::format::Pixel;
use ff::software::scaling::{Context as Scaler, Flags};
use ffmpeg_next as ff;
use std::collections::HashMap;

pub type Frame = ff::frame::Video;

pub fn new_frame(w: u32, h: u32) -> Frame {
    let mut f = Frame::new(Pixel::YUV420P, w, h);
    f.set_color_space(ff::color::Space::BT709);
    f.set_color_range(ff::color::Range::MPEG);
    f
}

/// Scaler cache keyed by (src and dst pixel format codes, src w, src h, dst w, dst h).
#[derive(Default)]
pub struct Scalers {
    map: HashMap<(i32, u32, u32, u32, u32), Scaler>,
}

impl Scalers {
    fn get(
        &mut self,
        src: Pixel,
        sw: u32,
        sh: u32,
        dst: Pixel,
        dw: u32,
        dh: u32,
    ) -> Result<&mut Scaler, ff::Error> {
        let key = (
            ff::ffi::AVPixelFormat::from(src) as i32 * 1000
                + ff::ffi::AVPixelFormat::from(dst) as i32,
            sw,
            sh,
            dw,
            dh,
        );
        if !self.map.contains_key(&key) {
            // Keep the cache bounded when crop sizes vary frame to frame.
            if self.map.len() > 32 {
                self.map.clear();
            }
            self.map
                .insert(key, Scaler::get(src, sw, sh, dst, dw, dh, Flags::BICUBIC)?);
        }
        Ok(self.map.get_mut(&key).expect("just inserted"))
    }

    /// Any decoded frame (10-bit, 4:2:2, NV12, ...) to a same-size YUV420P frame.
    pub fn normalise(&mut self, src: &Frame) -> Result<Frame, ff::Error> {
        let mut out = new_frame(src.width(), src.height());
        if src.format() == Pixel::YUV420P {
            for p in 0..3 {
                let (h, sw, dw) = (out.plane_height(p) as usize, src.stride(p), out.stride(p));
                let w = out.plane_width(p) as usize;
                let (s, d) = (src.data(p), out.data_mut(p));
                for y in 0..h {
                    d[y * dw..y * dw + w].copy_from_slice(&s[y * sw..y * sw + w]);
                }
            }
            return Ok(out);
        }
        self.get(
            src.format(),
            src.width(),
            src.height(),
            Pixel::YUV420P,
            src.width(),
            src.height(),
        )?
        .run(src, &mut out)?;
        Ok(out)
    }

    /// Scale the even-aligned rectangle `(x, y, w, h)` of `src` into the rectangle `(dx, dy, dw, dh)` of `dst`.
    #[allow(clippy::too_many_arguments)]
    pub fn crop_scale_into(
        &mut self,
        src: &Frame,
        rect: (u32, u32, u32, u32),
        dst: &mut Frame,
        dst_rect: (u32, u32, u32, u32),
    ) -> Result<(), ff::Error> {
        let (x, y, w, h) = rect;
        let (dx, dy, dw, dh) = dst_rect;
        let (x, y, w, h) = (x & !1, y & !1, (w & !1).max(2), (h & !1).max(2));
        let (dx, dy, dw, dh) = (dx & !1, dy & !1, (dw & !1).max(2), (dh & !1).max(2));
        let scaler = self.get(Pixel::YUV420P, w, h, Pixel::YUV420P, dw, dh)?;
        // SAFETY: rectangles are clamped by callers to lie inside both frames; offsets are even so
        // chroma offsets are exact; pointers are valid for the lifetimes of the borrowed frames.
        unsafe {
            let s = src.as_ptr();
            let d = dst.as_mut_ptr();
            let src_ptrs: [*const u8; 4] = [
                (*s).data[0].add(y as usize * (*s).linesize[0] as usize + x as usize),
                (*s).data[1].add((y / 2) as usize * (*s).linesize[1] as usize + (x / 2) as usize),
                (*s).data[2].add((y / 2) as usize * (*s).linesize[2] as usize + (x / 2) as usize),
                std::ptr::null(),
            ];
            let dst_ptrs: [*mut u8; 4] = [
                (*d).data[0].add(dy as usize * (*d).linesize[0] as usize + dx as usize),
                (*d).data[1].add((dy / 2) as usize * (*d).linesize[1] as usize + (dx / 2) as usize),
                (*d).data[2].add((dy / 2) as usize * (*d).linesize[2] as usize + (dx / 2) as usize),
                std::ptr::null_mut(),
            ];
            let ss: [i32; 4] = [(*s).linesize[0], (*s).linesize[1], (*s).linesize[2], 0];
            let ds: [i32; 4] = [(*d).linesize[0], (*d).linesize[1], (*d).linesize[2], 0];
            let r = ff::ffi::sws_scale(
                scaler.as_mut_ptr(),
                src_ptrs.as_ptr(),
                ss.as_ptr(),
                0,
                h as i32,
                dst_ptrs.as_ptr(),
                ds.as_ptr(),
            );
            if r <= 0 {
                return Err(ff::Error::InvalidData);
            }
        }
        Ok(())
    }
}

/// Rotate a YUV420P frame clockwise by 90, 180 or 270 degrees.
pub fn rotate(src: &Frame, deg: u16) -> Frame {
    if deg.is_multiple_of(360) {
        return src.clone();
    }
    let (w, h) = (src.width(), src.height());
    let (ow, oh) = if deg % 180 == 90 { (h, w) } else { (w, h) };
    let mut out = new_frame(ow, oh);
    for p in 0..3 {
        let (pw, ph) = (src.plane_width(p) as usize, src.plane_height(p) as usize);
        let (ss, ds) = (src.stride(p), out.stride(p));
        let s = src.data(p).to_vec();
        let d = out.data_mut(p);
        for y in 0..ph {
            for x in 0..pw {
                let v = s[y * ss + x];
                let (nx, ny) = match deg {
                    90 => (ph - 1 - y, x),
                    180 => (pw - 1 - x, ph - 1 - y),
                    _ => (y, pw - 1 - x), // 270
                };
                d[ny * ds + nx] = v;
            }
        }
    }
    out
}

/// Three-pass box blur of every plane, in place. Cheap enough on a downscaled frame.
pub fn blur(f: &mut Frame, radius: usize) {
    for p in 0..3 {
        let (w, h, s) = (
            f.plane_width(p) as usize,
            f.plane_height(p) as usize,
            f.stride(p),
        );
        let r = if p == 0 { radius } else { (radius / 2).max(1) };
        let data = f.data_mut(p);
        for _ in 0..3 {
            let mut tmp = vec![0u8; w];
            for y in 0..h {
                let row = &data[y * s..y * s + w];
                for (x, t) in tmp.iter_mut().enumerate() {
                    let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
                    *t = (row[a..=b].iter().map(|&v| v as u32).sum::<u32>() / (b - a + 1) as u32)
                        as u8;
                }
                data[y * s..y * s + w].copy_from_slice(&tmp);
            }
            let mut col = vec![0u8; h];
            for x in 0..w {
                for (y, c) in col.iter_mut().enumerate() {
                    let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
                    *c = ((a..=b).map(|yy| data[yy * s + x] as u32).sum::<u32>()
                        / (b - a + 1) as u32) as u8;
                }
                for y in 0..h {
                    data[y * s + x] = col[y];
                }
            }
        }
    }
}

/// A caption sprite converted once to premultiplied Y, U, V plus alpha at luma and chroma resolution.
pub struct YuvSprite {
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    py: Vec<u16>,
    a: Vec<u16>,
    pu: Vec<u16>,
    pv: Vec<u16>,
    ac: Vec<u16>,
}

impl YuvSprite {
    pub fn from_sprite(s: &Sprite) -> Self {
        // Align to even coordinates so chroma lines up.
        let (x, y) = (s.x & !1, s.y & !1);
        let (w, h) = ((s.w as usize + 1) & !1, (s.h as usize + 1) & !1);
        let px = |xx: usize, yy: usize| -> [u32; 4] {
            if xx >= s.w as usize || yy >= s.h as usize {
                return [0; 4];
            }
            let i = (yy * s.w as usize + xx) * 4;
            [
                s.rgba[i] as u32,
                s.rgba[i + 1] as u32,
                s.rgba[i + 2] as u32,
                s.rgba[i + 3] as u32,
            ]
        };
        // Premultiplied RGB -> premultiplied limited-range YUV (BT.709). Offsets scale with alpha.
        let to_yuv = |[r, g, b, a]: [u32; 4]| -> (f32, f32, f32, f32) {
            let (r, g, b, a) = (r as f32, g as f32, b as f32, a as f32);
            let y = 16.0 * a / 255.0 + 0.1826 * r + 0.6142 * g + 0.0620 * b;
            let u = 128.0 * a / 255.0 - 0.1006 * r - 0.3386 * g + 0.4392 * b;
            let v = 128.0 * a / 255.0 + 0.4392 * r - 0.3989 * g - 0.0403 * b;
            (y, u, v, a)
        };
        let mut py = vec![0u16; w * h];
        let mut a = vec![0u16; w * h];
        for yy in 0..h {
            for xx in 0..w {
                let (yv, _, _, av) = to_yuv(px(xx, yy));
                py[yy * w + xx] = yv.round().clamp(0.0, 255.0) as u16;
                a[yy * w + xx] = av as u16;
            }
        }
        let (cw, ch) = (w / 2, h / 2);
        let (mut pu, mut pv, mut ac) = (
            vec![0u16; cw * ch],
            vec![0u16; cw * ch],
            vec![0u16; cw * ch],
        );
        for cy in 0..ch {
            for cx in 0..cw {
                let mut acc = (0.0f32, 0.0f32, 0.0f32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (_, u, v, al) = to_yuv(px(cx * 2 + dx, cy * 2 + dy));
                    acc.0 += u;
                    acc.1 += v;
                    acc.2 += al;
                }
                pu[cy * cw + cx] = (acc.0 / 4.0).round().clamp(0.0, 255.0) as u16;
                pv[cy * cw + cx] = (acc.1 / 4.0).round().clamp(0.0, 255.0) as u16;
                ac[cy * cw + cx] = (acc.2 / 4.0).round() as u16;
            }
        }
        Self {
            x,
            y,
            w,
            h,
            py,
            a,
            pu,
            pv,
            ac,
        }
    }

    /// dst = src + dst * (1 - alpha), clipped to the frame.
    pub fn blend(&self, f: &mut Frame) {
        let (fw, fh) = (f.width() as i32, f.height() as i32);
        let s0 = f.stride(0);
        let y_plane = f.data_mut(0);
        for yy in 0..self.h as i32 {
            let ty = self.y + yy;
            if ty < 0 || ty >= fh {
                continue;
            }
            for xx in 0..self.w as i32 {
                let tx = self.x + xx;
                if tx < 0 || tx >= fw {
                    continue;
                }
                let i = yy as usize * self.w + xx as usize;
                let al = self.a[i] as u32;
                if al == 0 {
                    continue;
                }
                let d = &mut y_plane[ty as usize * s0 + tx as usize];
                *d = (self.py[i] as u32 + (*d as u32 * (255 - al) + 127) / 255).min(255) as u8;
            }
        }
        let (cw, ch) = (self.w / 2, self.h / 2);
        for p in 1..3 {
            let s = f.stride(p);
            let plane = f.data_mut(p);
            let src = if p == 1 { &self.pu } else { &self.pv };
            for cy in 0..ch as i32 {
                let ty = self.y / 2 + cy;
                if ty < 0 || ty >= fh / 2 {
                    continue;
                }
                for cx in 0..cw as i32 {
                    let tx = self.x / 2 + cx;
                    if tx < 0 || tx >= fw / 2 {
                        continue;
                    }
                    let i = cy as usize * cw + cx as usize;
                    let al = self.ac[i] as u32;
                    if al == 0 {
                        continue;
                    }
                    let d = &mut plane[ty as usize * s + tx as usize];
                    *d = (src[i] as u32 + (*d as u32 * (255 - al) + 127) / 255).min(255) as u8;
                }
            }
        }
    }
}

/// Fill a frame with one colour (limited-range black by default).
pub fn fill(f: &mut Frame, y: u8, u: u8, v: u8) {
    for (p, val) in [(0, y), (1, u), (2, v)] {
        let n = f.stride(p) * f.plane_height(p) as usize;
        f.data_mut(p)[..n].fill(val);
    }
}

/// YUV420P -> packed RGB24 for posters and snapshot tests.
pub fn to_rgb(scalers: &mut Scalers, f: &Frame) -> Result<(u32, u32, Vec<u8>), ff::Error> {
    let mut out = Frame::new(Pixel::RGB24, f.width(), f.height());
    scalers
        .get(
            Pixel::YUV420P,
            f.width(),
            f.height(),
            Pixel::RGB24,
            f.width(),
            f.height(),
        )?
        .run(f, &mut out)?;
    let (w, h, s) = (f.width() as usize, f.height() as usize, out.stride(0));
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        rgb.extend_from_slice(&out.data(0)[y * s..y * s + w * 3]);
    }
    Ok((w as u32, h as u32, rgb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, y: u8) -> Frame {
        let mut f = new_frame(w, h);
        fill(&mut f, y, 128, 128);
        f
    }

    #[test]
    fn rotate_moves_a_marked_corner() {
        crate::init();
        let mut f = solid(8, 4, 16);
        f.data_mut(0)[0] = 235; // top-left of an 8x4 frame
        let r = rotate(&f, 90);
        assert_eq!((r.width(), r.height()), (4, 8));
        // After 90° clockwise, the old top-left is the new top-right.
        assert_eq!(r.data(0)[3], 235);
        let r2 = rotate(&f, 270);
        assert_eq!(r2.data(0)[7 * r2.stride(0)], 235);
    }

    #[test]
    fn crop_scale_and_blend_white_sprite() {
        crate::init();
        let mut sc = Scalers::default();
        let src = solid(64, 64, 200);
        let mut dst = solid(32, 32, 16);
        sc.crop_scale_into(&src, (8, 8, 32, 32), &mut dst, (0, 0, 32, 32))
            .unwrap();
        assert!((dst.data(0)[16 * dst.stride(0) + 16] as i32 - 200).abs() <= 2);

        // An opaque white sprite becomes limited-range white (235) in luma, neutral chroma.
        let sprite = Sprite {
            x: 4,
            y: 4,
            w: 4,
            h: 4,
            rgba: vec![255; 64],
        };
        let mut f = solid(16, 16, 16);
        YuvSprite::from_sprite(&sprite).blend(&mut f);
        let s = f.stride(0);
        assert_eq!(f.data(0)[5 * s + 5], 235);
        assert_eq!(f.data(0)[0], 16);
        let c = f.data(1)[2 * f.stride(1) + 2];
        assert!((c as i32 - 128).abs() <= 1, "{c}");
    }

    #[test]
    fn blur_smooths_an_edge() {
        crate::init();
        let mut f = solid(32, 32, 16);
        for y in 0..32 {
            for x in 16..32 {
                let s = f.stride(0);
                f.data_mut(0)[y * s + x] = 235;
            }
        }
        blur(&mut f, 3);
        let s = f.stride(0);
        let mid = f.data(0)[16 * s + 16];
        assert!(mid > 40 && mid < 220, "{mid}");
    }
}

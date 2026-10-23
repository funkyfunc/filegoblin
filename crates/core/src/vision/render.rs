//! Page rasterization with `hayro` (pure Rust, embedded standard fonts).

use super::UserBox;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use image::RgbImage;
use std::sync::Arc;

/// A parsed document ready for rendering. Cheap to share across threads.
pub struct Renderer {
    pdf: Pdf,
}

/// A rendered page plus the mapping between its pixels and PDF user space.
pub struct PageImage {
    pub rgb: RgbImage,
    /// user space → pixel: `[a, b, c, d, e, f]` with
    /// `px = a*x + c*y + e`, `py = b*x + d*y + f`.
    fwd: [f64; 6],
    /// pixel → user space.
    inv: [f64; 6],
}

impl Renderer {
    pub fn open(data: Arc<Vec<u8>>) -> Option<Self> {
        Pdf::new(data).ok().map(|pdf| Self { pdf })
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Renders `page` so that its longer side is about `long_side` pixels
    /// (clamped to `max_scale` pixels per point).
    pub fn render(&self, page: usize, long_side: f32, max_scale: f32) -> Option<PageImage> {
        let pages = self.pdf.pages();
        let p = pages.get(page)?;
        let (w, h) = p.render_dimensions();
        if w <= 1.0 || h <= 1.0 {
            return None;
        }
        let scale = (long_side / w.max(h)).min(max_scale);
        let cache = RenderCache::new();
        let pixmap = PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        };
        let pix = render(
            p,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &pixmap,
        );
        let (pw, ph) = (u32::from(pix.width()), u32::from(pix.height()));
        let mut rgb = RgbImage::new(pw, ph);
        for (dst, src) in rgb.pixels_mut().zip(pix.data().iter()) {
            // Opaque white background, so premultiplied == straight.
            *dst = image::Rgb([src.r, src.g, src.b]);
        }
        let t = p.initial_transform(true).as_coeffs();
        let s = f64::from(scale);
        let fwd = [t[0] * s, t[1] * s, t[2] * s, t[3] * s, t[4] * s, t[5] * s];
        let inv = invert(fwd)?;
        Some(PageImage { rgb, fwd, inv })
    }
}

/// The frame a reader sees on a page with `/Rotate`: the page turned upright,
/// y up, at the media box's origin. pdf_oxide reports rotated text in this
/// frame, and the layout model reads the rendered (turned) page in it.
#[derive(Debug, Clone, Copy)]
pub struct DisplayFrame {
    /// Clockwise page rotation: 90, 180 or 270.
    pub rot: i32,
    /// Media box `(llx, lly, urx, ury)`.
    pub media: (f32, f32, f32, f32),
}

impl DisplayFrame {
    /// Raw page space → displayed frame, as `[a, b, c, d, e, f]`
    /// (`x' = a·x + c·y + e`, `y' = b·x + d·y + f`).
    fn matrix(&self) -> [f64; 6] {
        let (llx, lly, urx, ury) = self.media;
        let (llx, lly) = (f64::from(llx), f64::from(lly));
        let (w, h) = (f64::from(urx) - llx, f64::from(ury) - lly);
        match self.rot {
            90 => [0.0, -1.0, 1.0, 0.0, llx - lly, llx + lly + w],
            180 => [-1.0, 0.0, 0.0, -1.0, 2.0 * llx + w, 2.0 * lly + h],
            270 => [0.0, 1.0, -1.0, 0.0, llx + lly + h, lly - llx],
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    /// A box in raw page space, in the displayed frame.
    pub fn map(&self, b: &UserBox) -> UserBox {
        let m = self.matrix();
        let pts = [
            apply(&m, b.x0.into(), b.y0.into()),
            apply(&m, b.x1.into(), b.y1.into()),
        ];
        bounds(&pts)
    }

    /// The page box `(x0, y0, x1, y1)` as displayed.
    pub fn page_box(&self) -> (f32, f32, f32, f32) {
        let (llx, lly, urx, ury) = self.media;
        match self.rot {
            90 | 270 => (llx, lly, llx + (ury - lly), lly + (urx - llx)),
            _ => self.media,
        }
    }
}

impl PageImage {
    /// The same image, with its pixel ↔ user-space mapping in the displayed
    /// frame of a rotated page instead of raw page space.
    pub fn in_display_frame(mut self, frame: DisplayFrame) -> Option<Self> {
        let r = frame.matrix();
        let r_inv = invert(r)?;
        self.fwd = compose(self.fwd, r_inv);
        self.inv = compose(r, self.inv);
        Some(self)
    }
}

/// `m1 ∘ m2`: apply `m2`, then `m1`.
fn compose(m1: [f64; 6], m2: [f64; 6]) -> [f64; 6] {
    [
        m1[0] * m2[0] + m1[2] * m2[1],
        m1[1] * m2[0] + m1[3] * m2[1],
        m1[0] * m2[2] + m1[2] * m2[3],
        m1[1] * m2[2] + m1[3] * m2[3],
        m1[0] * m2[4] + m1[2] * m2[5] + m1[4],
        m1[1] * m2[4] + m1[3] * m2[5] + m1[5],
    ]
}

fn invert(m: [f64; 6]) -> Option<[f64; 6]> {
    let [a, b, c, d, e, f] = m;
    let det = a * d - b * c;
    if det.abs() < 1e-12 {
        return None;
    }
    let ia = d / det;
    let ib = -b / det;
    let ic = -c / det;
    let id = a / det;
    Some([ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)])
}

fn apply(m: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

impl PageImage {
    /// Pixel rectangle → user-space box.
    pub fn to_user(&self, px0: f32, py0: f32, px1: f32, py1: f32) -> UserBox {
        let pts = [
            apply(&self.inv, px0.into(), py0.into()),
            apply(&self.inv, px1.into(), py0.into()),
            apply(&self.inv, px0.into(), py1.into()),
            apply(&self.inv, px1.into(), py1.into()),
        ];
        bounds(&pts)
    }

    /// User-space box → pixel rectangle `(x0, y0, x1, y1)`, clamped to the image.
    pub fn to_px(&self, b: &UserBox) -> (u32, u32, u32, u32) {
        let pts = [
            apply(&self.fwd, b.x0.into(), b.y0.into()),
            apply(&self.fwd, b.x1.into(), b.y0.into()),
            apply(&self.fwd, b.x0.into(), b.y1.into()),
            apply(&self.fwd, b.x1.into(), b.y1.into()),
        ];
        let r = bounds(&pts);
        let (w, h) = (self.rgb.width() as f32, self.rgb.height() as f32);
        (
            r.x0.clamp(0.0, w) as u32,
            r.y0.clamp(0.0, h) as u32,
            r.x1.clamp(0.0, w).ceil() as u32,
            r.y1.clamp(0.0, h).ceil() as u32,
        )
    }

    /// Pixels per point.
    pub fn scale(&self) -> f32 {
        (self.fwd[0] * self.fwd[3] - self.fwd[1] * self.fwd[2])
            .abs()
            .sqrt() as f32
    }
}

fn bounds(pts: &[(f64, f64)]) -> UserBox {
    let mut b = UserBox {
        x0: f32::MAX,
        y0: f32::MAX,
        x1: f32::MIN,
        y1: f32::MIN,
    };
    for &(x, y) in pts {
        b.x0 = b.x0.min(x as f32);
        b.y0 = b.y0.min(y as f32);
        b.x1 = b.x1.max(x as f32);
        b.y1 = b.y1.max(y as f32);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_frame_matches_pdf_oxides_rotation() {
        // pdf_oxide maps (x, y) on a /Rotate 90 page of width W to (y, W - x),
        // on 270 to (H - y, x), and on 180 to (W - x, H - y).
        let media = (0.0, 0.0, 612.0, 792.0);
        let p = UserBox {
            x0: 100.0,
            y0: 200.0,
            x1: 100.0,
            y1: 200.0,
        };
        let at = |rot| {
            let b = DisplayFrame { rot, media }.map(&p);
            (b.x0, b.y0)
        };
        assert_eq!(at(90), (200.0, 512.0));
        assert_eq!(at(270), (592.0, 100.0));
        assert_eq!(at(180), (512.0, 592.0));
        assert_eq!(
            DisplayFrame { rot: 90, media }.page_box(),
            (0.0, 0.0, 792.0, 612.0)
        );
    }

    #[test]
    fn pixels_map_to_the_displayed_frame() {
        // A raw page rendered at 2 px/pt with the y axis flipped.
        let fwd = [2.0, 0.0, 0.0, -2.0, 0.0, 1584.0];
        let img = PageImage {
            rgb: RgbImage::new(4, 4),
            fwd,
            inv: invert(fwd).unwrap(),
        };
        let frame = DisplayFrame {
            rot: 90,
            media: (0.0, 0.0, 612.0, 792.0),
        };
        let raw = (100.0f64, 200.0f64);
        let (px, py) = apply(&fwd, raw.0, raw.1);
        let shown = img.in_display_frame(frame).unwrap();
        let b = shown.to_user(px as f32, py as f32, px as f32, py as f32);
        assert!(
            (b.x0 - 200.0).abs() < 1e-3 && (b.y0 - 512.0).abs() < 1e-3,
            "{b:?}"
        );
    }
}

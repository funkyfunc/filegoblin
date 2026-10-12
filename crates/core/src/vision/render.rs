//! Page rasterization with `hayro` (pure Rust, embedded standard fonts).

use super::UserBox;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{RenderCache, RenderSettings, render};
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
        let settings = RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            ..Default::default()
        };
        let pix = render(p, &cache, &InterpreterSettings::default(), &settings);
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

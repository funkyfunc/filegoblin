//! Table structure with Table Transformer v1.1 "all" (MIT, Microsoft).
//!
//! A DETR detector over the table image predicts rows, columns, column-header
//! rows and spanning cells. The grid is rows × columns; the words that fill
//! the cells come from the caller (the PDF text layer or OCR), so the model
//! never changes a value — it only decides which cell a value sits in.

use super::render::PageImage;
use super::{UserBox, models};
use rten::{Model, ValueOrView};
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;
use std::sync::OnceLock;

const LONG_SIDE: f32 = 1000.0;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// The detected grid, in user space. Rows top to bottom, columns left to right.
#[derive(Debug, Clone, Default)]
pub struct Structure {
    pub rows: Vec<UserBox>,
    pub cols: Vec<UserBox>,
    /// Indices of rows that are column headers.
    pub header_rows: Vec<usize>,
    pub spans: Vec<UserBox>,
}

pub fn model() -> Option<&'static Model> {
    static M: OnceLock<Option<Model>> = OnceLock::new();
    M.get_or_init(|| models::load(models::TABLE_STRUCTURE))
        .as_ref()
}

/// Recognizes the structure of the table inside `bbox`.
pub fn recognize(page: &PageImage, bbox: &UserBox) -> Option<Structure> {
    let model = model()?;
    let (x0, y0, x1, y1) = page.to_px(bbox);
    // The model was trained on crops with a margin around the table.
    let margin = 10u32;
    let (cx0, cy0) = (x0.saturating_sub(margin), y0.saturating_sub(margin));
    let cx1 = (x1 + margin).min(page.rgb.width());
    let cy1 = (y1 + margin).min(page.rgb.height());
    let (cw, ch) = (cx1.saturating_sub(cx0), cy1.saturating_sub(cy0));
    if cw < 16 || ch < 8 {
        return None;
    }
    let crop = image::imageops::crop_imm(&page.rgb, cx0, cy0, cw, ch).to_image();
    let scale = LONG_SIDE / cw.max(ch) as f32;
    let (iw, ih) = (
        ((cw as f32) * scale).round() as u32,
        ((ch as f32) * scale).round() as u32,
    );
    let img = image::imageops::resize(
        &crop,
        iw.max(1),
        ih.max(1),
        image::imageops::FilterType::Triangle,
    );
    let mut x = NdTensor::<f32, 4>::zeros([1, 3, ih as usize, iw as usize]);
    for (px, py, p) in img.enumerate_pixels() {
        for c in 0..3 {
            x[[0, c, py as usize, px as usize]] = (f32::from(p[c]) / 255.0 - MEAN[c]) / STD[c];
        }
    }
    let input = *model.input_ids().first()?;
    let outs = model
        .run(
            vec![(input, ValueOrView::from(x.view()))],
            model.output_ids(),
            None,
        )
        .ok()?;
    let logits: NdTensor<f32, 3> = outs.first()?.clone().try_into().ok()?;
    let boxes: NdTensor<f32, 3> = outs.get(1)?.clone().try_into().ok()?;

    let (fw, fh) = (cw as f32, ch as f32);
    let mut rows = Vec::new();
    let mut cols = Vec::new();
    let mut headers = Vec::new();
    let mut spans = Vec::new();
    let classes = logits.size(2);
    for q in 0..logits.size(1) {
        // softmax over classes (+ "no object" last)
        let mx = (0..classes)
            .map(|c| logits[[0, q, c]])
            .fold(f32::MIN, f32::max);
        let exps: Vec<f32> = (0..classes)
            .map(|c| (logits[[0, q, c]] - mx).exp())
            .collect();
        let sum: f32 = exps.iter().sum();
        let (best, p) = exps[..classes - 1]
            .iter()
            .enumerate()
            .map(|(c, e)| (c, e / sum))
            .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
        if p < 0.5 {
            continue;
        }
        let (bx, by, bw, bh) = (
            boxes[[0, q, 0]],
            boxes[[0, q, 1]],
            boxes[[0, q, 2]],
            boxes[[0, q, 3]],
        );
        let px0 = cx0 as f32 + (bx - bw / 2.0) * fw;
        let py0 = cy0 as f32 + (by - bh / 2.0) * fh;
        let px1 = cx0 as f32 + (bx + bw / 2.0) * fw;
        let py1 = cy0 as f32 + (by + bh / 2.0) * fh;
        let ub = page.to_user(px0, py0, px1, py1);
        match best {
            1 => cols.push((ub, p)),
            2 => rows.push((ub, p)),
            3 => headers.push((ub, p)),
            // A projected row header ("Skin and Subcutaneous Tissue" over a
            // group of rows) is a cell that spans the whole row.
            4 | 5 => spans.push((ub, p)),
            _ => {}
        }
    }
    // Clip to the table and drop near-duplicates.
    let clip = |b: UserBox| UserBox {
        x0: b.x0.max(bbox.x0 - 4.0),
        y0: b.y0.max(bbox.y0 - 4.0),
        x1: b.x1.min(bbox.x1 + 4.0),
        y1: b.y1.min(bbox.y1 + 4.0),
    };
    let mut rows = dedupe(rows.into_iter().map(|(b, p)| (clip(b), p)).collect(), true);
    let mut cols = dedupe(cols.into_iter().map(|(b, p)| (clip(b), p)).collect(), false);
    // Rows top to bottom (user space: larger y first); columns left to right.
    rows.sort_by(|a, b| b.y1.total_cmp(&a.y1));
    cols.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    if rows.len() < 2 || cols.len() < 2 {
        return None;
    }
    let header_rows = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            headers.iter().any(|(h, _)| {
                let ov = (r.y1.min(h.y1) - r.y0.max(h.y0)).max(0.0);
                ov >= 0.5 * r.height()
            })
        })
        .map(|(i, _)| i)
        .collect();
    Some(Structure {
        rows,
        cols,
        header_rows,
        spans: spans.into_iter().map(|(b, _)| clip(b)).collect(),
    })
}

/// Keeps the stronger of two boxes that overlap by more than half along the
/// axis that separates them (y for rows, x for columns).
fn dedupe(mut v: Vec<(UserBox, f32)>, rows: bool) -> Vec<UserBox> {
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut kept: Vec<UserBox> = Vec::new();
    for (b, _) in v {
        let clash = kept.iter().any(|k| {
            let (a0, a1, b0, b1) = if rows {
                (k.y0, k.y1, b.y0, b.y1)
            } else {
                (k.x0, k.x1, b.x0, b.x1)
            };
            let ov = (a1.min(b1) - a0.max(b0)).max(0.0);
            ov > 0.5 * (a1 - a0).min(b1 - b0).max(0.01)
        });
        if !clash {
            kept.push(b);
        }
    }
    kept
}

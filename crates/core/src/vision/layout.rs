//! Page layout with PP-DocLayoutV3 (Apache-2.0, PaddlePaddle).
//!
//! The network is an RT-DETR detector with a reading-order head: for each of
//! 300 queries it predicts a box, scores for 25 region classes and pairwise
//! "comes before" logits, which are turned into a reading order by vote
//! counting (the same decoding as the reference implementation).

use super::render::PageImage;
use super::{UserBox, models};
use rten::{Model, ValueOrView};
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;
use std::sync::OnceLock;

const INPUT: u32 = 800;
const QUERIES: usize = 300;

/// Region classes, in the model's label order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Label {
    Abstract,
    Algorithm,
    AsideText,
    Chart,
    Content,
    DisplayFormula,
    DocTitle,
    FigureTitle,
    Footer,
    FooterImage,
    Footnote,
    FormulaNumber,
    Header,
    HeaderImage,
    Image,
    InlineFormula,
    Number,
    ParagraphTitle,
    Reference,
    ReferenceContent,
    Seal,
    Table,
    Text,
    VerticalText,
    VisionFootnote,
}

const LABELS: [Label; 25] = [
    Label::Abstract,
    Label::Algorithm,
    Label::AsideText,
    Label::Chart,
    Label::Content,
    Label::DisplayFormula,
    Label::DocTitle,
    Label::FigureTitle,
    Label::Footer,
    Label::FooterImage,
    Label::Footnote,
    Label::FormulaNumber,
    Label::Header,
    Label::HeaderImage,
    Label::Image,
    Label::InlineFormula,
    Label::Number,
    Label::ParagraphTitle,
    Label::Reference,
    Label::ReferenceContent,
    Label::Seal,
    Label::Table,
    Label::Text,
    Label::VerticalText,
    Label::VisionFootnote,
];

impl Label {
    /// Page furniture: running heads, folios, decorative header/footer art.
    pub fn is_furniture(self) -> bool {
        matches!(
            self,
            Label::Header | Label::Footer | Label::HeaderImage | Label::FooterImage | Label::Number
        )
    }
    /// Pictures whose interior text is part of the graphic, not the prose.
    pub fn is_graphic(self) -> bool {
        matches!(self, Label::Image | Label::Chart | Label::Seal)
    }
    fn threshold(self) -> f32 {
        match self {
            // Inline formulas are small and plentiful; a miss costs little
            // (the text layer is still there), a false hit costs a word.
            Label::InlineFormula => 0.45,
            Label::Header | Label::Footer | Label::Number => 0.5,
            _ => 0.4,
        }
    }
}

/// One detected region, in the model's reading order.
#[derive(Debug, Clone)]
pub struct Region {
    pub label: Label,
    pub score: f32,
    pub bbox: UserBox,
    /// Position in the predicted reading order (lower reads first).
    pub order: usize,
}

pub fn model() -> Option<&'static Model> {
    static M: OnceLock<Option<Model>> = OnceLock::new();
    M.get_or_init(|| models::load(models::LAYOUT)).as_ref()
}

/// Detects the regions of a rendered page.
pub fn detect(page: &PageImage) -> Option<Vec<Region>> {
    let model = model()?;
    let small = image::imageops::resize(
        &page.rgb,
        INPUT,
        INPUT,
        image::imageops::FilterType::CatmullRom,
    );
    let n = INPUT as usize;
    let mut x = NdTensor::<f32, 4>::zeros([1, 3, n, n]);
    for (px, py, p) in small.enumerate_pixels() {
        for c in 0..3 {
            x[[0, c, py as usize, px as usize]] = f32::from(p[c]) / 255.0;
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
    let order_logits: NdTensor<f32, 3> = outs.get(2)?.clone().try_into().ok()?;
    let ranks = order_ranks(&order_logits);

    let (w, h) = (page.rgb.width() as f32, page.rgb.height() as f32);
    let mut regions: Vec<Region> = Vec::new();
    for q in 0..QUERIES {
        for (c, &label) in LABELS.iter().enumerate() {
            let score = sigmoid(logits[[0, q, c]]);
            if score < label.threshold() {
                continue;
            }
            let (cx, cy, bw, bh) = (
                boxes[[0, q, 0]],
                boxes[[0, q, 1]],
                boxes[[0, q, 2]],
                boxes[[0, q, 3]],
            );
            let px0 = ((cx - bw / 2.0) * w).clamp(0.0, w);
            let py0 = ((cy - bh / 2.0) * h).clamp(0.0, h);
            let px1 = ((cx + bw / 2.0) * w).clamp(0.0, w);
            let py1 = ((cy + bh / 2.0) * h).clamp(0.0, h);
            if px1 - px0 < 1.0 || py1 - py0 < 1.0 {
                continue;
            }
            regions.push(Region {
                label,
                score,
                bbox: page.to_user(px0, py0, px1, py1),
                order: ranks[q],
            });
        }
    }
    Some(suppress_duplicates(regions))
}

fn sigmoid(v: f32) -> f32 {
    1.0 / (1.0 + (-v).exp())
}

/// Reading-order rank of every query from the pairwise logits: query `j`'s
/// vote is how many queries are predicted to precede it.
fn order_ranks(order_logits: &NdTensor<f32, 3>) -> Vec<usize> {
    let n = order_logits.size(1);
    let mut votes = vec![0f32; n];
    for (j, vote) in votes.iter_mut().enumerate() {
        let mut v = 0f32;
        for i in 0..n {
            if i < j {
                v += sigmoid(order_logits[[0, i, j]]);
            } else if i > j {
                v += 1.0 - sigmoid(order_logits[[0, j, i]]);
            }
        }
        *vote = v;
    }
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| votes[a].total_cmp(&votes[b]));
    let mut ranks = vec![0usize; n];
    for (rank, &q) in idx.iter().enumerate() {
        ranks[q] = rank;
    }
    ranks
}

/// One box per object: when two detections overlap heavily, the weaker one
/// goes — unless one is an inline formula inside the other (that nesting is
/// the point) or both are formulas of different kinds.
fn suppress_duplicates(mut regions: Vec<Region>) -> Vec<Region> {
    regions.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Region> = Vec::new();
    for r in regions {
        let dup = kept.iter().any(|k| {
            let nested_formula =
                (k.label == Label::InlineFormula) != (r.label == Label::InlineFormula);
            if nested_formula {
                return false;
            }
            let inter = k.bbox.intersection(&r.bbox);
            let smaller = k.bbox.area().min(r.bbox.area()).max(1e-3);
            k.bbox.iou(&r.bbox) > 0.6 || inter / smaller > 0.9
        });
        if !dup {
            kept.push(r);
        }
    }
    kept.sort_by_key(|r| r.order);
    kept
}

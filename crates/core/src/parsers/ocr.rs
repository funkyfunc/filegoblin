use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use std::path::Path;

/// OCR for standalone image files. On macOS this always works (Apple Vision,
/// no weights, no feature flag); elsewhere it needs `--features ocr`.
pub struct OcrGobbler;

impl Gobble for OcrGobbler {
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> Result<String> {
        let bytes = std::fs::read(path).context("Failed to open image file for OCR")?;
        let text = ocr_image_bytes(&bytes)?;
        // An error, not a sentence that looks like document content: a caller
        // reading `--json` (or a model reading an MCP result) cannot tell the
        // string "No text could be extracted…" from real extracted text.
        if text.trim().is_empty() {
            anyhow::bail!(
                "No text could be extracted from this image (no readable text, or too low-resolution)"
            );
        }
        Ok(format!(
            "## Image Text: {}\n\n{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            text.trim()
        ))
    }
}

/// True when this build can actually OCR (used to pick error messages and to
/// decide whether a scanned PDF page gets OCR'd or a marker).
pub fn ocr_available() -> bool {
    cfg!(target_os = "macos") || cfg!(feature = "ocr")
}

/// OCRs encoded image bytes (PNG/JPEG/TIFF/…) and returns the recognized text
/// in reading order. The platform backend is chosen at compile time:
/// - macOS: Apple Vision (always available, hardware accelerated, no weights)
/// - Linux + `ocr` feature: pure-Rust `ocrs`/`rten` (weights via `just get-brains`)
/// - Windows + `ocr` feature: the Windows.Media.Ocr API
pub fn ocr_image_bytes(bytes: &[u8]) -> Result<String> {
    // OCR is content-addressed: identical image bytes always yield the same
    // text for a given backend, so results cache indefinitely.
    let key = crate::cache::key(bytes);
    if let Some(text) = crate::cache::get_blob("ocr", &key) {
        return Ok(text);
    }
    let text = ocr_image_bytes_uncached(bytes)?;
    crate::cache::put_blob("ocr", &key, &text);
    Ok(text)
}

#[allow(unreachable_code, unused_variables)]
fn ocr_image_bytes_uncached(bytes: &[u8]) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        return vision_ocr(bytes);
    }
    #[cfg(all(target_os = "windows", feature = "ocr"))]
    {
        return windows_ocr(bytes);
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows"), feature = "ocr"))]
    {
        return ocrs_ocr(bytes);
    }
    anyhow::bail!(
        "OCR is not available in this build. Recompile with --features ocr \
         (macOS builds have native Vision OCR without any feature)."
    )
}

/// Geometry of a recognized text box/line.
#[derive(Debug, Clone)]
pub struct OcrBox {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
    pub text: String,
}

impl OcrBox {
    pub fn new(top: f64, bottom: f64, left: f64, right: f64, text: String) -> Self {
        Self {
            top,
            bottom,
            left,
            right,
            text,
        }
    }

    pub fn width(&self) -> f64 {
        (self.right - self.left).max(0.0)
    }

    pub fn height(&self) -> f64 {
        (self.bottom - self.top).max(0.0)
    }

    pub fn mid_x(&self) -> f64 {
        (self.left + self.right) / 2.0
    }

    pub fn mid_y(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }
}

#[derive(Debug, Clone)]
pub struct AssembledLine {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
    pub text: String,
}

/// Detects if a page has a distinct two-column layout with a vertical gutter corridor.
/// Returns the x-coordinate of the gutter corridor if detected.
pub fn detect_two_column_gutter(boxes: &[OcrBox]) -> Option<f64> {
    if boxes.len() < 8 {
        return None;
    }

    // Consider boxes that carry meaningful text
    let valid_boxes: Vec<&OcrBox> = boxes
        .iter()
        .filter(|b| b.width() >= 0.08 && b.text.chars().any(|c| c.is_alphabetic()))
        .collect();

    if valid_boxes.len() < 8 {
        return None;
    }

    let mut best_gutter = None;
    let mut min_crossings = usize::MAX;
    let mut max_balance = 0usize;

    // Search for a candidate gutter in x = [0.35, 0.65] with step 0.01
    for step in 35..=65 {
        let x = step as f64 / 100.0;
        let mut left_count = 0usize;
        let mut right_count = 0usize;
        let mut crossing_count = 0usize;

        for b in &valid_boxes {
            if b.left < x - 0.03 && b.right > x + 0.03 {
                crossing_count += 1;
            } else if b.mid_x() < x {
                left_count += 1;
            } else {
                right_count += 1;
            }
        }

        // A valid 2-column page must have substantial text in both columns
        if left_count >= 4 && right_count >= 4 {
            let total_col = left_count + right_count;
            // Crossing spans (e.g. titles/banners) must be a small fraction of column text
            if crossing_count * 4 <= total_col {
                let balance = left_count.min(right_count);
                if crossing_count < min_crossings
                    || (crossing_count == min_crossings && balance > max_balance)
                {
                    min_crossings = crossing_count;
                    max_balance = balance;
                    best_gutter = Some(x);
                }
            }
        }
    }

    best_gutter
}

/// Reconstructs natural reading order for a page of OCR text boxes,
/// correctly partitioning multi-column layouts and merging same-line fragments.
pub fn assemble_page_reading_order(boxes: Vec<OcrBox>) -> String {
    if boxes.is_empty() {
        return String::new();
    }

    // Determine layout: two-column or single-column
    let gutter = detect_two_column_gutter(&boxes);

    let groups: Vec<Vec<OcrBox>> = if let Some(gutter_x) = gutter {
        partition_two_column_page(boxes, gutter_x)
    } else {
        vec![boxes]
    };

    let mut result_parts: Vec<String> = Vec::new();
    for group in groups {
        let lines = assemble_lines_within_group(group);
        let text = join_lines_with_dehyphenation(&lines);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            result_parts.push(trimmed.to_string());
        }
    }

    result_parts.join("\n\n")
}

/// Partitions a two-column page into reading-order groups:
/// full-width header blocks, column 1 (left), column 2 (right), and full-width footer blocks.
fn partition_two_column_page(boxes: Vec<OcrBox>, gutter_x: f64) -> Vec<Vec<OcrBox>> {
    let mut full_width = Vec::new();
    let mut col_boxes = Vec::new();

    for b in boxes {
        let is_fw = b.width() >= 0.58 || (b.left < gutter_x - 0.03 && b.right > gutter_x + 0.03);
        if is_fw {
            full_width.push(b);
        } else {
            col_boxes.push(b);
        }
    }

    if full_width.is_empty() {
        let mut col1 = Vec::new();
        let mut col2 = Vec::new();
        for b in col_boxes {
            if b.mid_x() < gutter_x {
                col1.push(b);
            } else {
                col2.push(b);
            }
        }
        return vec![col1, col2];
    }

    // Group full-width boxes into contiguous vertical intervals
    full_width.sort_by(|a, b| a.top.total_cmp(&b.top));
    let mut fw_intervals: Vec<(f64, f64, Vec<OcrBox>)> = Vec::new();
    for b in full_width {
        if let Some(last) = fw_intervals.last_mut() {
            if b.top <= last.1 + 0.04 {
                last.1 = last.1.max(b.bottom);
                last.2.push(b);
                continue;
            }
        }
        fw_intervals.push((b.top, b.bottom, vec![b]));
    }

    let mut groups: Vec<Vec<OcrBox>> = Vec::new();
    let mut current_y = 0.0;

    for (fw_top, fw_bot, fw_boxes) in fw_intervals {
        // Collect column boxes strictly above this full-width band
        let band_col: Vec<OcrBox> = col_boxes
            .iter()
            .filter(|b| b.mid_y() >= current_y && b.mid_y() < fw_top)
            .cloned()
            .collect();

        if !band_col.is_empty() {
            let mut col1 = Vec::new();
            let mut col2 = Vec::new();
            for b in band_col {
                if b.mid_x() < gutter_x {
                    col1.push(b);
                } else {
                    col2.push(b);
                }
            }
            if !col1.is_empty() {
                groups.push(col1);
            }
            if !col2.is_empty() {
                groups.push(col2);
            }
        }

        // Add the full-width band
        groups.push(fw_boxes);
        current_y = fw_bot;
    }

    // Collect any remaining column boxes below the last full-width band
    let rem_col: Vec<OcrBox> = col_boxes
        .iter()
        .filter(|b| b.mid_y() >= current_y)
        .cloned()
        .collect();

    if !rem_col.is_empty() {
        let mut col1 = Vec::new();
        let mut col2 = Vec::new();
        for b in rem_col {
            if b.mid_x() < gutter_x {
                col1.push(b);
            } else {
                col2.push(b);
            }
        }
        if !col1.is_empty() {
            groups.push(col1);
        }
        if !col2.is_empty() {
            groups.push(col2);
        }
    }

    groups
}

/// Assembles boxes within a single column or full-width band into lines.
/// Merges boxes on the same horizontal line while keeping vertically consecutive lines separate.
fn assemble_lines_within_group(mut boxes: Vec<OcrBox>) -> Vec<AssembledLine> {
    if boxes.is_empty() {
        return Vec::new();
    }

    boxes.sort_by(|a, b| a.top.total_cmp(&b.top));

    let mut lines: Vec<AssembledLine> = Vec::new();
    for b in boxes {
        let mut merged = false;
        if let Some(last_line) = lines.last_mut() {
            // Vertical overlap requirement
            let overlap_y = (b.bottom.min(last_line.bottom) - b.top.max(last_line.top)).max(0.0);
            let min_h = b.height().min(last_line.bottom - last_line.top);
            let has_v_overlap = min_h > 0.0 && overlap_y >= 0.40 * min_h;

            // Horizontal overlap prohibition (same visual line items must be side-by-side)
            let overlap_x = (b.right.min(last_line.right) - b.left.max(last_line.left)).max(0.0);
            let min_w = b.width().min(last_line.right - last_line.left);
            let has_h_overlap = min_w > 0.0 && overlap_x >= 0.15 * min_w;

            if has_v_overlap && !has_h_overlap {
                last_line.top = last_line.top.min(b.top);
                last_line.bottom = last_line.bottom.max(b.bottom);
                if b.left >= last_line.right {
                    last_line.text.push(' ');
                    last_line.text.push_str(b.text.trim());
                    last_line.right = b.right;
                } else if b.right <= last_line.left {
                    let mut new_text = b.text.trim().to_string();
                    new_text.push(' ');
                    new_text.push_str(&last_line.text);
                    last_line.text = new_text;
                    last_line.left = b.left;
                } else {
                    last_line.left = last_line.left.min(b.left);
                    last_line.right = last_line.right.max(b.right);
                    last_line.text.push(' ');
                    last_line.text.push_str(b.text.trim());
                }
                merged = true;
            }
        }

        if !merged {
            lines.push(AssembledLine {
                top: b.top,
                bottom: b.bottom,
                left: b.left,
                right: b.right,
                text: b.text.trim().to_string(),
            });
        }
    }

    lines
}

/// Joins lines into formatted text, repairing broken soft hyphens across line breaks.
fn join_lines_with_dehyphenation(lines: &[AssembledLine]) -> String {
    if lines.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let mut i = 0;
    while i < lines.len() {
        let curr = &lines[i];
        let curr_text = curr.text.trim();
        if curr_text.is_empty() {
            i += 1;
            continue;
        }

        if i + 1 < lines.len() {
            let next = &lines[i + 1];
            let next_text = next.text.trim();

            // Detect soft hyphen at line break: e.g. "elici-" followed by "tation. We make..."
            if let Some(prefix) = curr_text.strip_suffix('-')
                && !prefix.is_empty()
                && prefix.chars().last().is_some_and(|c| c.is_alphabetic())
                && next_text
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_lowercase())
            {
                let mut parts = next_text.splitn(2, |c: char| c.is_whitespace());
                let first_word = parts.next().unwrap_or("");
                let rest = parts.next();

                output.push_str(prefix);
                output.push_str(first_word);
                if let Some(rem) = rest {
                    output.push(' ');
                    output.push_str(rem);
                }
                output.push('\n');
                i += 2;
                continue;
            }
        }

        output.push_str(curr_text);
        output.push('\n');
        i += 1;
    }

    output
}

/// Apple Vision text recognition. Zero dependencies beyond the OS.
#[cfg(target_os = "macos")]
fn vision_ocr(bytes: &[u8]) -> Result<String> {
    use objc2::{AnyThread, rc::Retained};
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedText,
        VNRecognizedTextObservation,
    };

    let data = NSData::from_vec(bytes.to_vec());
    let handler = VNImageRequestHandler::initWithData_options(
        VNImageRequestHandler::alloc(),
        &data,
        &NSDictionary::new(),
    );

    // Defaults to Accurate recognition, which is hardware accelerated.
    let request = VNRecognizeTextRequest::new();
    // Disable slow dictionary spellchecker pass which adds ~1.5s per page latency.
    // Disabling language correction retains exact character fidelity (math symbols,
    // code tokens, author footnote asterisks) while accelerating OCR recognition ~7x.
    request.setUsesLanguageCorrection(false);
    let requests = NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(
        request.clone(),
    ))]);

    if let Err(e) = handler.performRequests_error(&requests) {
        anyhow::bail!(
            "macOS Vision Framework threw an error during OCR execution: {:?}",
            e
        );
    }

    // Collect OcrBox per observation. Vision's coordinate space is
    // normalized with the origin at the bottom-left, so flip Y to read top-down.
    let mut boxes: Vec<OcrBox> = Vec::new();
    if let Some(res) = request.results() {
        for i in 0..res.count() {
            let observation = res.objectAtIndex(i);
            let Ok(obs) = Retained::downcast::<VNRecognizedTextObservation>(observation) else {
                continue;
            };
            let candidates = obs.topCandidates(1);
            if let Some(first_candidate) = candidates.firstObject() {
                let first_candidate: Retained<VNRecognizedText> = first_candidate;
                // SAFETY: reading a plain CGRect property off a Vision
                // observation we own; no aliasing or lifetime hazards.
                let bbox = unsafe { obs.boundingBox() };
                let top = 1.0 - (bbox.origin.y + bbox.size.height);
                let bottom = 1.0 - bbox.origin.y;
                let left = bbox.origin.x;
                let right = bbox.origin.x + bbox.size.width;
                boxes.push(OcrBox::new(
                    top,
                    bottom,
                    left,
                    right,
                    first_candidate.string().to_string(),
                ));
            }
        }
    }

    Ok(assemble_page_reading_order(boxes))
}

/// Windows.Media.Ocr, driven through a temp file (the Storage API wants paths).
#[cfg(all(target_os = "windows", feature = "ocr"))]
fn windows_ocr(bytes: &[u8]) -> Result<String> {
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};
    use windows::core::HSTRING;

    let mut tmp = tempfile::Builder::new()
        .prefix("gobble_ocr_")
        .suffix(".img")
        .tempfile()
        .context("Failed to stage image for Windows OCR")?;
    {
        use std::io::Write;
        tmp.write_all(bytes)
            .context("Failed to stage image for Windows OCR")?;
    }
    let path_str = tmp
        .path()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid unicode path"))?
        .to_string();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create Tokio runtime for Windows OCR")?;

    rt.block_on(async {
        let h_path = HSTRING::from(path_str.as_str());
        let file = StorageFile::GetFileFromPathAsync(&h_path)?.await?;
        let stream = file.OpenAsync(FileAccessMode::Read)?.await?;
        let decoder = BitmapDecoder::CreateAsync(&stream)?.await?;
        let bitmap = decoder.GetSoftwareBitmapAsync()?.await?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let result = engine.RecognizeAsync(&bitmap)?.await?;
        Ok(result.Text()?.to_string())
    })
    .map_err(|e: windows::core::Error| anyhow::anyhow!("Windows OCR Error: {}", e))
}

/// Pure-Rust `ocrs` inference for Linux and other Unixes.
#[cfg(all(not(target_os = "macos"), not(target_os = "windows"), feature = "ocr"))]
fn ocrs_ocr(bytes: &[u8]) -> Result<String> {
    use image::GenericImageView;
    use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
    use rten::Model;

    let img = image::load_from_memory(bytes).context("Failed to decode image for OCR")?;
    let (width, height) = img.dimensions();

    // Weight search order: $FILEGOBLIN_OCR_DIR, next to the binary, then CWD —
    // a released binary must not depend on being run from the repo root.
    let model_dir = ocr_model_dir().ok_or_else(|| {
        anyhow::anyhow!(
            "OCR Brains missing! Download `text-detection.rten` and `text-recognition.rten` \
             (see `just get-brains`) into $FILEGOBLIN_OCR_DIR, next to the filegoblin binary, \
             or ./assets/."
        )
    })?;

    let detection_model = Model::load_file(model_dir.join("text-detection.rten"))
        .context("Failed to load text detection model")?;
    let recognition_model = Model::load_file(model_dir.join("text-recognition.rten"))
        .context("Failed to load text recognition model")?;

    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
    .context("Failed to initialize OCR engine")?;

    let img_luma = img.into_luma8();
    let img_source = ImageSource::from_bytes(img_luma.as_raw(), (width, height))
        .context("Failed to create ImageSource from image bytes")?;
    let ocr_input = engine
        .prepare_input(img_source)
        .context("Failed to prepare OCR input")?;
    let word_rects = engine
        .detect_words(&ocr_input)
        .context("Failed to detect words")?;
    let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
    let texts = engine
        .recognize_text(&ocr_input, &line_rects)
        .context("Failed to recognize text")?;

    let mut output = String::new();
    for line in texts.into_iter().flatten() {
        output.push_str(&line.to_string());
        output.push('\n');
    }
    Ok(output)
}

/// First directory that holds both `.rten` models.
#[cfg(all(not(target_os = "macos"), not(target_os = "windows"), feature = "ocr"))]
fn ocr_model_dir() -> Option<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("FILEGOBLIN_OCR_DIR") {
        candidates.push(dir.into());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("assets"));
        candidates.push(dir.to_path_buf());
    }
    candidates.push("assets".into());
    candidates.into_iter().find(|d| {
        d.join("text-detection.rten").exists() && d.join("text-recognition.rten").exists()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_two_column_reading_order() {
        let mut boxes = Vec::new();
        // 5 lines in Col 1 (left: 0.10 to 0.45)
        // 5 lines in Col 2 (left: 0.55 to 0.90)
        // Staggered or identical Y positions to simulate side-by-side lines
        for i in 0..5 {
            let y = 0.20 + (i as f64) * 0.05;
            boxes.push(OcrBox::new(
                y,
                y + 0.02,
                0.10,
                0.45,
                format!("Left line {}", i + 1),
            ));
            boxes.push(OcrBox::new(
                y,
                y + 0.02,
                0.55,
                0.90,
                format!("Right line {}", i + 1),
            ));
        }

        let result = assemble_page_reading_order(boxes);
        let left_1 = result.find("Left line 1").unwrap();
        let left_5 = result.find("Left line 5").unwrap();
        let right_1 = result.find("Right line 1").unwrap();
        let right_5 = result.find("Right line 5").unwrap();

        assert!(left_1 < left_5, "Col 1 should be read top to bottom");
        assert!(left_5 < right_1, "Col 1 must precede Col 2");
        assert!(right_1 < right_5, "Col 2 should be read top to bottom");
    }

    #[test]
    fn test_two_column_with_full_width_title() {
        let mut boxes = Vec::new();
        // Title banner across the top
        boxes.push(OcrBox::new(
            0.05,
            0.08,
            0.15,
            0.85,
            "Document Title Banner".to_string(),
        ));

        for i in 0..5 {
            let y = 0.15 + (i as f64) * 0.05;
            boxes.push(OcrBox::new(
                y,
                y + 0.02,
                0.10,
                0.45,
                format!("Column 1 line {}", i + 1),
            ));
            boxes.push(OcrBox::new(
                y,
                y + 0.02,
                0.55,
                0.90,
                format!("Column 2 line {}", i + 1),
            ));
        }

        let result = assemble_page_reading_order(boxes);
        let title_pos = result.find("Document Title Banner").unwrap();
        let col1_pos = result.find("Column 1 line 1").unwrap();
        let col2_pos = result.find("Column 2 line 1").unwrap();

        assert!(title_pos < col1_pos);
        assert!(col1_pos < col2_pos);
    }

    #[test]
    fn test_single_column_preserves_consecutive_lines() {
        let boxes = vec![
            OcrBox::new(
                0.356,
                0.369,
                0.151,
                0.856,
                "(3) a desire to assist a knowledge engineer".to_string(),
            ),
            OcrBox::new(
                0.371,
                0.384,
                0.151,
                0.856,
                "associated with the acquisition of complex concepts".to_string(),
            ),
        ];

        let result = assemble_page_reading_order(boxes);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2, "Consecutive vertical lines must not merge");
        assert_eq!(lines[0], "(3) a desire to assist a knowledge engineer");
        assert_eq!(
            lines[1],
            "associated with the acquisition of complex concepts"
        );
    }

    #[test]
    fn test_same_line_fragment_merging() {
        let boxes = vec![
            OcrBox::new(
                0.639,
                0.657,
                0.151,
                0.563,
                "acquisition as a methodological \"patchwork,".to_string(),
            ),
            OcrBox::new(
                0.639,
                0.657,
                0.570,
                0.856,
                ",\" resulting from the importation".to_string(),
            ),
        ];

        let result = assemble_page_reading_order(boxes);
        assert!(result.contains(
            "acquisition as a methodological \"patchwork, ,\" resulting from the importation"
        ));
    }

    #[test]
    fn test_dehyphenation() {
        let lines = vec![
            AssembledLine {
                top: 0.12,
                bottom: 0.14,
                left: 0.15,
                right: 0.85,
                text: "intercon-".to_string(),
            },
            AssembledLine {
                top: 0.15,
                bottom: 0.17,
                left: 0.15,
                right: 0.85,
                text: "nected domain knowledge".to_string(),
            },
        ];

        let joined = join_lines_with_dehyphenation(&lines);
        assert_eq!(joined.trim(), "interconnected domain knowledge");
    }
}

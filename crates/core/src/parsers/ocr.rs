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
        if text.trim().is_empty() {
            return Ok("No text could be extracted from this image.".to_string());
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
#[allow(unreachable_code, unused_variables)]
pub fn ocr_image_bytes(bytes: &[u8]) -> Result<String> {
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
    let requests = NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(
        request.clone(),
    ))]);

    if let Err(e) = handler.performRequests_error(&requests) {
        anyhow::bail!(
            "macOS Vision Framework threw an error during OCR execution: {:?}",
            e
        );
    }

    // Collect (top-y, x, text) per observation. Vision's coordinate space is
    // normalized with the origin at the bottom-left, so flip Y to read
    // top-down.
    let mut spans: Vec<(f64, f64, String)> = Vec::new();
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
                spans.push((top, bbox.origin.x, first_candidate.string().to_string()));
            }
        }
    }

    // Reading order: sort by top edge, then greedily cluster boxes whose tops
    // sit within ~1.5% of the line's first box into one visual line, ordered
    // left→right. (Sorting on the scalar key first keeps this a total order —
    // a pairwise |Δy| comparator is intransitive and can abort the sort.)
    const Y_BAND: f64 = 0.015;
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut lines: Vec<Vec<(f64, f64, String)>> = Vec::new();
    for span in spans {
        match lines.last_mut() {
            Some(line) if span.0 - line[0].0 <= Y_BAND => line.push(span),
            _ => lines.push(vec![span]),
        }
    }

    let mut output = String::new();
    for mut line in lines {
        line.sort_by(|a, b| a.1.total_cmp(&b.1));
        let joined = line
            .iter()
            .map(|(_, _, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        output.push_str(&joined);
        output.push('\n');
    }
    Ok(output)
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

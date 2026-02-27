use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;

pub struct OcrGobbler;

#[cfg(target_os = "macos")]
impl Gobble for OcrGobbler {
    fn gobble(&self, path: &Path) -> Result<String> {
        use objc2::{rc::Retained, AnyThread};
        use objc2_foundation::{NSArray, NSData, NSDictionary};
        use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRecognizedText};
        use std::fs;

        // 1. Load the raw image bytes natively bypassing the `image` crate decoder
        let img_bytes = fs::read(path).context("Failed to open image file for OCR")?;
        let data = NSData::from_vec(img_bytes);
        
        // 2. Create the Vision Request Handler
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &data,
            &NSDictionary::new(),
        );

        // 3. Create the Text Recognition Request (defaults to Accurate processing which is hardware accelerated)
        let request = VNRecognizeTextRequest::new();

        // 4. Fire the synchronized request
        let requests = NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(request.clone()))]);
        
        let success = handler.performRequests_error(&requests);
        if let Err(e) = success {
             anyhow::bail!("macOS Vision Framework threw an error during OCR execution: {:?}", e);
        }

        // 5. Gather and parse the results
        let mut output = String::new();
        output.push_str(&format!("## Image Text: {}\n\n", path.file_name().unwrap_or_default().to_string_lossy()));

        let results = request.results();
        if let Some(res) = results {
            for i in 0..res.count() {
                let observation = res.objectAtIndex(i);
                
                // Downcast raw NSObject to VNRecognizedTextObservation
                let obs = Retained::downcast::<VNRecognizedTextObservation>(observation).unwrap();
                
                // We ask for the top 1 most confident candidate
                let candidates = obs.topCandidates(1);
                if let Some(first_candidate) = candidates.firstObject() {
                    let first_candidate: Retained<VNRecognizedText> = first_candidate;
                    let text = first_candidate.string();
                    output.push_str(&text.to_string());
                    output.push('\n');
                }
            }
        }

        if output.trim().is_empty() {
             return Ok("No text could be extracted from this image.".to_string());
        }

        Ok(output)
    }
}

#[cfg(target_os = "windows")]
impl Gobble for OcrGobbler {
    fn gobble(&self, path: &Path) -> Result<String> {
        use windows::core::HSTRING;
        use windows::Graphics::Imaging::BitmapDecoder;
        use windows::Media::Ocr::OcrEngine;
        use windows::Storage::{FileAccessMode, StorageFile};

        // We must use a separate block for the async Windows APIs since Gobble::gobble is synchronous
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("Failed to create Tokio runtime for Windows OCR")?;

        rt.block_on(async {
            let path_str = path.to_str().ok_or_else(|| anyhow::anyhow!("Invalid unicode path"))?;
            let h_path = HSTRING::from(path_str);
            
            // 1. Load the image using Windows Storage API
            let file = StorageFile::GetFileFromPathAsync(&h_path)?.await?;
            let stream = file.OpenAsync(FileAccessMode::Read)?.await?;

            // 2. Decode the image to a SoftwareBitmap
            let decoder = BitmapDecoder::CreateAsync(&stream)?.await?;
            let bitmap = decoder.GetSoftwareBitmapAsync()?.await?;

            // 3. Initialize the OCR Engine using user profile languages
            let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;

            // 4. Perform the recognition
            let result = engine.RecognizeAsync(&bitmap)?.await?;

            // 5. Output the result
            let text = result.Text()?.to_string(); // converts HSTRING to rust String
            
            let mut output = String::new();
            output.push_str(&format!("## Image Text: {}\n\n", path.file_name().unwrap_or_default().to_string_lossy()));
            output.push_str(&text);

            if output.trim().is_empty() {
                 return Ok("No text could be extracted from this image.".to_string());
            }

            Ok(output)
        }).map_err(|e: windows::core::Error| anyhow::anyhow!("Windows OCR Error: {}", e))
    }
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
impl Gobble for OcrGobbler {
    fn gobble(&self, path: &Path) -> Result<String> {
        use image::GenericImageView;
        use ocrs::{OcrEngine, OcrEngineParams, ImageSource};
        use rten::Model;

        // filegoblin strictly prefers zero dependencies. For Linux/Windows OCR, we use the `ocrs` crate 
        // powered by `rten` tensors.
        
        // 1. Load the image using the `image` crate
        let img = image::open(path).context("Failed to open image file for OCR")?;
        let (width, height) = img.dimensions();

        // 2. Load the pre-trained RTEN models.
        let detect_model_path = "assets/text-detection.rten";
        let rec_model_path = "assets/text-recognition.rten";

        if !Path::new(detect_model_path).exists() || !Path::new(rec_model_path).exists() {
             anyhow::bail!(
                 "OCR Brains missing! To chew on images, please download the `text-detection.rten` and `text-recognition.rten` models into the `./assets/` directory."
             );
        }

        let detection_model = Model::load_file(detect_model_path)
            .context("Failed to load text detection model")?;
        let recognition_model = Model::load_file(rec_model_path)
            .context("Failed to load text recognition model")?;

        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        }).context("Failed to initialize OCR engine")?;

        // 3. Convert image to the format expected by `ocrs`
        let img_luma = img.into_luma8();
        let img_source = ImageSource::from_bytes(img_luma.as_raw(), (width, height))
            .context("Failed to create ImageSource from image bytes")?;

        // 4. Run detection and recognition
        let ocr_input = engine.prepare_input(img_source)
            .context("Failed to prepare OCR input")?;
        
        let word_rects = engine.detect_words(&ocr_input)
            .context("Failed to detect words")?;
            
        let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
        let texts = engine.recognize_text(&ocr_input, &line_rects)
            .context("Failed to recognize text")?;

        let mut output = String::new();
        output.push_str(&format!("## Image Text: {}\n\n", path.file_name().unwrap_or_default().to_string_lossy()));
        
        for line in texts.into_iter().flatten() {
             output.push_str(&line.to_string());
             output.push('\n');
        }

        if output.trim().is_empty() {
             return Ok("No text could be extracted from this image.".to_string());
        }

        Ok(output)
    }
}

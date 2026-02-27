use windows::core::{HSTRING, Result};
use windows::Foundation::Uri;
use windows::Graphics::Imaging::{BitmapDecoder, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};

#[tokio::main]
async fn main() -> Result<()> {
    println!("Testing Windows.Media.Ocr Native Framework...");

    let file_path = std::env::current_dir()
        .unwrap()
        .join("assets")
        .join("goblin.png");
    
    // 1. Load the image using Windows Storage API
    let h_path = HSTRING::from(file_path.to_str().unwrap());
    let file = StorageFile::GetFileFromPathAsync(&h_path)?.await?;
    let stream = file.OpenAsync(FileAccessMode::Read)?.await?;

    // 2. Decode the image to a SoftwareBitmap
    let decoder = BitmapDecoder::CreateAsync(&stream)?.await?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.await?;

    // 3. Initialize the OCR Engine
    // Check if OCR is supported and English is available (as a fallback/test)
    let lang = windows::Globalization::Language::CreateLanguage(&HSTRING::from("en-US"))?;
    
    let engine = if OcrEngine::IsLanguageSupported(&lang)? {
        OcrEngine::TryCreateFromLanguage(&lang)?
    } else {
        println!("Falling back to User Profile Language");
        OcrEngine::TryCreateFromUserProfileLanguages()?
    };

    // 4. Perform the recognition
    let result = engine.RecognizeAsync(&bitmap)?.await?;

    // 5. Output the result
    let text = result.Text()?;
    println!("Extracted Text:\n{}", text);

    Ok(())
}

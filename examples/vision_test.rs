use objc2::{rc::Retained, ClassType, AnyThread};
use objc2_foundation::{NSArray, NSData, NSDictionary};
use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRecognizedText};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing macOS Vision Framework OCR...");

    // 1. Path to our test image
    let img_bytes = fs::read("assets/goblin.png")?;
    let data = NSData::from_vec(img_bytes);
    
    // 2. Create the Request Handler
    let handler = VNImageRequestHandler::initWithData_options(
        VNImageRequestHandler::alloc(),
        &data,
        &NSDictionary::new(),
    );

    // 3. Create the Text Recognition Request
    let request = VNRecognizeTextRequest::new();
    
    // Optional: configuration
    // request.setRecognitionLevel(VNRequestTextRecognitionLevelAccurate);
    // request.setUsesLanguageCorrection(true);

    // 4. Perform the request
    let requests = NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(request.clone()))]);
    
    let success = handler.performRequests_error(&requests);
    if let Err(e) = success {
        println!("Error: {:?}", e);
        return Ok(());
    }

    // 5. Get results
    let results = request.results();
    if let Some(res) = results {
        for i in 0..res.count() {
            let observation = res.objectAtIndex(i);
            // Downcast to VNRecognizedTextObservation
            let obs = Retained::downcast::<VNRecognizedTextObservation>(observation).unwrap();
            
            // Get top candidate (maximum 1)
            let candidates = obs.topCandidates(1);
            if let Some(first_candidate) = candidates.firstObject() {
                // Ensure first_candidate is known
                let first_candidate: Retained<VNRecognizedText> = first_candidate;
                let text = first_candidate.string();
                println!("{}", text);
            }
        }
    }

    Ok(())
}

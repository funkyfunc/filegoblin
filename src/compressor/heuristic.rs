pub fn estimate_tokens(content: &str, file_path: &str) -> usize {
    let extension = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let ratio = match extension.as_str() {
        "md" | "txt" | "pdf" | "docx" => 4.2, // English prose
        "rs" | "c" | "cpp" | "java" | "go" | "ts" | "js" => 3.2, // C-Style code has heavy braces and keywords
        "py" | "yaml" | "yml" => 3.8, // Structural whitespace dense
        "json" | "html" | "xml" => 2.5, // High bracket tag density
        "log" | "hash" | "" => 1.5, // High entropy noise
        _ => 3.5, // Generous fallback
    };

    (content.len() as f64 / ratio).ceil() as usize
}

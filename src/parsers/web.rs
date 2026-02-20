use crate::parsers::gobble::Gobble;
use anyhow::Result;
use std::path::Path;

pub struct WebGobbler;

impl WebGobbler {
    fn strip_tags(&self, mut html: String, tags: &[&str]) -> String {
        for tag in tags {
            let open = format!("<{}", tag);
            let close = format!("</{}>", tag);
            
            while let Some(start) = html.find(&open) {
                // Find closing tag strictly after start
                if let Some(end) = html[start..].find(&close) {
                    let full_end = start + end + close.len();
                    html.replace_range(start..full_end, "");
                } else {
                    // broken HTML, just break
                    break;
                }
            }
        }
        html
    }

    fn extract_tag_content(&self, html: &str, tag: &str) -> Option<String> {
        let open = format!("<{}", tag);
        let close = format!("</{}>", tag);
        
        if let Some(start_idx) = html.find(&open) {
            // Find end of the opening tag '>'
            if let Some(tag_end_idx) = html[start_idx..].find('>') {
                let content_start = start_idx + tag_end_idx + 1;
                if let Some(end_idx) = html[content_start..].find(&close) {
                    return Some(html[content_start..content_start + end_idx].to_string());
                }
            }
        }
        None
    }
}

impl Gobble for WebGobbler {
    fn gobble(&self, path: &Path) -> Result<String> {
        let content = std::fs::read_to_string(path)?;
        self.gobble_str(&content)
    }

    fn gobble_str(&self, content: &str) -> Result<String> {
        // Strip out noisy tags
        let mut clean = self.strip_tags(content.to_string(), &["script", "style", "nav", "svg"]);
        
        // Try to find <article> or <main>
        if let Some(article) = self.extract_tag_content(&clean, "article") {
            clean = article;
        } else if let Some(main) = self.extract_tag_content(&clean, "main") {
            clean = main;
        }

        // Just quick-strip all HTML tags to get pure text content roughly
        // MOCK: proper regex regex replace `<[^>]*>` would be nicer but string 0-dependency
        let mut text = String::new();
        let mut in_tag = false;
        for c in clean.chars() {
            if c == '<' {
                in_tag = true;
            } else if c == '>' {
                in_tag = false;
            } else if !in_tag {
                text.push(c);
            }
        }
        
        // Clean up excessive newlines/spaces
        let reduced = text.split('\n')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
            
        Ok(reduced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_heuristic_extraction() {
        let gobbler = WebGobbler;
        let html = r#"
            <html><body>
                <nav>Ignore me</nav>
                <article>
                    <h1>The Title</h1>
                    <p>The core content</p>
                </article>
                <footer>footer</footer>
            </body></html>
        "#;
        
        let result = gobbler.gobble_str(html).unwrap();
        
        // Assert we stripped navigation and kept main article
        assert!(result.contains("The core content"));
        assert!(!result.contains("Ignore me"));
        assert!(!result.contains("footer"));
    }
}

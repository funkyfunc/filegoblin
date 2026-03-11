use crate::parsers::gobble::Gobble;
use anyhow::Result;
use std::path::Path;

pub struct WebGobbler {
    pub extract_full: bool,
}

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

    /// Recursively flattens `<a href="...">Text</a>` into just `Text`
    fn flatten_links(&self, mut html: String) -> String {
        while let Some(start) = html.find("<a ") {
            if let Some(tag_end) = html[start..].find('>') {
                let tag_end_idx = start + tag_end + 1;
                // Delete the `<a href="...">`
                html.replace_range(start..tag_end_idx, "");

                // Now find the next `</a>` and delete it
                // We use replace instead of replace_range on the first occurrence
                // so we don't have to perfectly track the shift in indices
                if let Some(close_start) = html.find("</a>") {
                    html.replace_range(close_start..close_start + 4, "");
                }
            } else {
                break; // Broken tag
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
    fn gobble(&self, path: &Path, flags: &crate::cli::Cli) -> Result<String> {
        let content = std::fs::read_to_string(path)?;
        self.gobble_str(&content, flags)
    }

    fn gobble_str(&self, content: &str, _flags: &crate::cli::Cli) -> Result<String> {
        let mut clean = content.to_string();

        // --- Auth wall / JS-gated page detection ---
        // Detect pages that require sign-in or JavaScript rendering before bothering to parse
        let lower = clean.to_lowercase();
        let is_google_signin = lower.contains("accounts.google.com")
            || (lower.contains("sign in")
                && (lower.contains("google") || lower.contains("gemini")));
        let has_no_body_content = {
            // Strip all tags and check if meaningful text remains
            // Only flag if nearly empty AND has script tags (classic JS-only SPA/auth wall pattern)
            let text_only: String = clean
                .chars()
                .scan(false, |in_tag, c| {
                    if c == '<' {
                        *in_tag = true;
                    }
                    let emit = !*in_tag;
                    if c == '>' {
                        *in_tag = false;
                    }
                    Some(if emit { c } else { ' ' })
                })
                .collect::<String>();
            let word_count = text_only.split_whitespace().count();
            let has_scripts = lower.contains("<script");
            // Must be nearly empty (<10 meaningful words) AND script-driven
            // to avoid false positives on minimal test HTML
            word_count < 10 && has_scripts
        };

        if is_google_signin {
            anyhow::bail!(
                "This page requires a Google account sign-in (JavaScript auth wall).\n\
                 filegoblin cannot access authenticated Google/Gemini content.\n\
                 💡 Try: copy the content manually, save as a .md or .txt file, then: gobble <file>"
            );
        }

        if has_no_body_content {
            anyhow::bail!(
                "This page appears to require JavaScript rendering or authentication.\n\
                 filegoblin uses a static HTTP fetcher and cannot execute JavaScript.\n\
                 💡 Try: save the page as HTML from your browser, then: gobble <saved-file.html>"
            );
        }

        if self.extract_full {
            // In full extraction, just remove scripts/styles that aren't content
            clean = self.strip_tags(clean, &["script", "style", "svg", "noscript", "iframe"]);
        } else {
            // Strip out noisy tags natively
            clean = self.strip_tags(
                clean,
                &[
                    "script", "style", "nav", "svg", "noscript", "iframe", "header",
                ],
            );

            // Try to find <article> or <main>
            if let Some(main) = self.extract_tag_content(&clean, "main") {
                clean = main;
            } else if let Some(article) = self.extract_tag_content(&clean, "article") {
                clean = article;
            }
        }

        // Repair malformed nested lists so `html2md` correctly indents them
        // Many sites put <ol> after </li> instead of inside the <li>
        clean = clean.replace("</li>\n<ol", "<ol");
        clean = clean.replace("</li>\n<ul", "<ul");
        clean = clean.replace("</li><ol", "<ol");
        clean = clean.replace("</li><ul", "<ul");

        clean = self.flatten_links(clean);

        let markdown = html2md::parse_html(&clean);
        Ok(markdown.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_web_heuristic_extraction() {
        let gobbler = WebGobbler {
            extract_full: false,
        };
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

        let args = crate::cli::Cli::parse_from(["filegoblin"]);
        let result = gobbler.gobble_str(html, &args).unwrap();

        // Assert we stripped navigation and kept main article
        assert!(result.contains("The core content"));
        assert!(!result.contains("Ignore me"));
        assert!(!result.contains("footer"));
    }
}

use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use mail_parser::{Address, Message, MessageParser, MimeHeaders};
use std::path::Path;

/// Parses email into clean, LLM-friendly markdown: a header block (From/To/Cc/
/// Date/Subject) followed by the plain-text body. Handles single messages
/// (`.eml`) and Unix mailboxes (`.mbox`, many messages concatenated).
pub struct EmailGobbler;

impl Gobble for EmailGobbler {
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> Result<String> {
        let bytes = std::fs::read(path).context("Failed to read email file")?;

        let is_mbox = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mbox"))
            .unwrap_or(false);

        if is_mbox {
            return Ok(parse_mbox(&bytes));
        }

        let message = MessageParser::default()
            .parse(&bytes)
            .ok_or_else(|| anyhow::anyhow!("Not a valid RFC822/MIME email"))?;
        Ok(format_message(&message))
    }
}

/// Renders a parsed message as a markdown block.
fn format_message(message: &Message) -> String {
    let mut out = String::new();
    let subject = message.subject().unwrap_or("(no subject)");
    out.push_str(&format!("# Email: {}\n\n", subject));

    let from = format_address(message.from());
    if !from.is_empty() {
        out.push_str(&format!("**From:** {}\n", from));
    }
    let to = format_address(message.to());
    if !to.is_empty() {
        out.push_str(&format!("**To:** {}\n", to));
    }
    let cc = format_address(message.cc());
    if !cc.is_empty() {
        out.push_str(&format!("**Cc:** {}\n", cc));
    }
    if let Some(date) = message.date() {
        out.push_str(&format!("**Date:** {}\n", date.to_rfc3339()));
    }
    out.push_str(&format!("**Subject:** {}\n\n", subject));

    // Prefer a genuine text/plain part; otherwise convert the HTML body to
    // markdown through the DOM extractor. (mail-parser's `body_text` silently
    // down-converts HTML-only messages itself — we want our converter, not its.)
    let has_real_text_part = message
        .text_part(0)
        .is_some_and(|p| matches!(&p.body, mail_parser::PartType::Text(_)));
    let body = if has_real_text_part {
        message.body_text(0).map(|c| c.into_owned())
    } else {
        None
    }
    .or_else(|| message.body_html(0).map(|h| html_body_to_md(&h)))
    .or_else(|| message.body_text(0).map(|c| c.into_owned()))
    .unwrap_or_default();
    out.push_str(body.trim());

    // List attachments by name so they're accounted for, even though their
    // bytes aren't extracted.
    let attachments: Vec<&str> = message
        .attachments()
        .filter_map(|a| a.attachment_name())
        .collect();
    if !attachments.is_empty() {
        out.push_str(&format!(
            "\n\n**Attachments:** {}\n",
            attachments.join(", ")
        ));
    }

    out.trim_end().to_string()
}

/// Converts an HTML email body through the real DOM extractor (full mode: an
/// email body IS the content, no readability scoring wanted), falling back to
/// plain text extraction if that fails.
fn html_body_to_md(html: &str) -> String {
    let gobbler = crate::parsers::html::HtmlGobbler {
        extract_full: true,
        skip_front_matter: true,
        ..Default::default()
    };
    gobbler
        .gobble_str(html, &ParseOptions::default())
        .unwrap_or_else(|_| strip_html(html))
}

/// Formats an address header (possibly a list) as `Name <email>` entries.
fn format_address(address: Option<&Address>) -> String {
    let Some(address) = address else {
        return String::new();
    };
    address
        .iter()
        .filter_map(|addr| match (addr.name(), addr.address()) {
            (Some(name), Some(email)) => Some(format!("{} <{}>", name, email)),
            (_, Some(email)) => Some(email.to_string()),
            (Some(name), None) => Some(name.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Last-resort tag stripper, used only if the DOM extractor errors on a body.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Splits an mbox into individual RFC822 messages (each starts with a `From `
/// envelope line) and renders them in order.
fn parse_mbox(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut count = 0;
    let mut current = String::new();

    for line in text.split_inclusive('\n') {
        if line.starts_with("From ") && !current.is_empty() {
            append_mbox_message(&mut out, &current, &mut count);
            current.clear();
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        append_mbox_message(&mut out, &current, &mut count);
    }

    if count == 0 {
        return "No messages could be extracted from this mbox.".to_string();
    }
    out
}

fn append_mbox_message(out: &mut String, chunk: &str, count: &mut usize) {
    // Drop the leading mbox "From " envelope line before RFC822 parsing.
    let body = if chunk.starts_with("From ") {
        chunk.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        chunk
    };
    if let Some(message) = MessageParser::default().parse(body.as_bytes()) {
        *count += 1;
        if *count > 1 {
            out.push_str("\n\n---\n\n");
        }
        out.push_str(&format_message(&message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;
    use std::io::Write;

    const SAMPLE_EML: &str = "From: Alice <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Subject: Lunch\r\n\
Date: Mon, 14 Jul 2026 10:00:00 +0000\r\n\
Content-Type: text/plain\r\n\
\r\n\
Are we still on for lunch?\r\n";

    #[test]
    fn test_eml_parsing() {
        let mut f = tempfile::Builder::new().suffix(".eml").tempfile().unwrap();
        f.write_all(SAMPLE_EML.as_bytes()).unwrap();

        let gobbler = EmailGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(f.path(), &(&args).into()).unwrap();

        assert!(result.contains("# Email: Lunch"));
        assert!(result.contains("**From:** Alice <alice@example.com>"));
        assert!(result.contains("**To:** Bob <bob@example.com>"));
        assert!(result.contains("Are we still on for lunch?"));
    }

    #[test]
    fn test_mbox_multiple_messages() {
        let mbox = format!(
            "From alice@example.com Mon Jul 14 10:00:00 2026\n{}\nFrom carol@example.com Mon Jul 14 11:00:00 2026\n{}",
            SAMPLE_EML,
            SAMPLE_EML.replace("Lunch", "Dinner")
        );
        let mut f = tempfile::Builder::new().suffix(".mbox").tempfile().unwrap();
        f.write_all(mbox.as_bytes()).unwrap();

        let gobbler = EmailGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(f.path(), &(&args).into()).unwrap();

        assert!(result.contains("# Email: Lunch"));
        assert!(result.contains("# Email: Dinner"));
        assert!(result.contains("---")); // separator between messages
    }

    #[test]
    fn test_html_only_body_is_stripped() {
        let eml = "From: a@x.com\r\nSubject: HTML\r\nContent-Type: text/html\r\n\r\n<p>Hello <b>world</b></p>\r\n";
        let mut f = tempfile::Builder::new().suffix(".eml").tempfile().unwrap();
        f.write_all(eml.as_bytes()).unwrap();

        let gobbler = EmailGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(f.path(), &(&args).into()).unwrap();
        // The HTML body goes through the real DOM extractor now, so inline
        // formatting survives as markdown.
        assert!(result.contains("Hello **world**"));
        assert!(!result.contains("<b>"));
    }

    #[test]
    fn test_html_body_styles_do_not_leak() {
        let eml = "From: a@x.com\r\nSubject: Styled\r\nContent-Type: text/html\r\n\r\n\
            <html><head><style>.x { color: red; }</style></head>\
            <body><p>Visible &amp; important</p></body></html>\r\n";
        let mut f = tempfile::Builder::new().suffix(".eml").tempfile().unwrap();
        f.write_all(eml.as_bytes()).unwrap();

        let gobbler = EmailGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(f.path(), &(&args).into()).unwrap();
        assert!(result.contains("Visible & important"));
        assert!(!result.contains("color: red"));
    }
}

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
            return parse_mbox(&bytes);
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
    // No `**Subject:**` line — the `# Email:` heading above already carries it,
    // and repeating it just spent tokens saying the same thing twice.
    out.push('\n');

    // Prefer a genuine text/plain part; otherwise convert the HTML body to
    // markdown through the DOM extractor. (mail-parser's `body_text` silently
    // down-converts HTML-only messages itself — we want our converter, not its.)
    // The text part must be genuine *and non-empty*: marketing senders very
    // commonly ship an empty (or "View this email in your browser") plain part
    // alongside the real HTML, and merely checking that the part existed meant
    // the message rendered as headers with no body at all.
    let has_real_text_part = message
        .text_part(0)
        .is_some_and(|p| matches!(&p.body, mail_parser::PartType::Text(_)));
    let plain = if has_real_text_part {
        message
            .body_text(0)
            .map(|c| c.into_owned())
            .filter(|t| !t.trim().is_empty())
    } else {
        None
    };
    let body = plain
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
fn parse_mbox(bytes: &[u8]) -> Result<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut count = 0;
    let mut current = String::new();

    // A bare `starts_with("From ")` splits on any body line beginning "From "
    // ("From the beginning, we…"), cutting a message in half and feeding the
    // tail to the parser as a bogus extra message. A real envelope line follows
    // a blank line and looks like `From <addr> <date>`.
    let mut previous_blank = true;
    for line in text.split_inclusive('\n') {
        if previous_blank && is_mbox_envelope(line) && !current.is_empty() {
            append_mbox_message(&mut out, &current, &mut count);
            current.clear();
        }
        previous_blank = line.trim().is_empty();
        current.push_str(line);
    }
    if !current.is_empty() {
        append_mbox_message(&mut out, &current, &mut count);
    }

    // An error, not a sentence dressed as document content.
    if count == 0 {
        anyhow::bail!("No messages could be extracted from this mbox (is it really an mbox?)");
    }
    Ok(out)
}

/// True for an mbox envelope line: `From <sender> <date>`, where the sender
/// carries no spaces. Deliberately strict — a false positive splits a message
/// mid-body, and the cost of a false negative is only a missed boundary.
fn is_mbox_envelope(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("From ") else {
        return false;
    };
    let mut parts = rest.split_whitespace();
    let Some(sender) = parts.next() else {
        return false;
    };
    // `From ` at the start of a sentence is followed by an ordinary word; an
    // envelope sender is an address or `MAILER-DAEMON`, and a date follows.
    parts.next().is_some() && (sender.contains('@') || sender.chars().all(|c| !c.is_lowercase()))
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
mod regression_tests {
    use super::*;

    /// Regression: an empty text/plain part shadowed the real HTML body, so a
    /// very common marketing email rendered as headers with no content.
    #[test]
    fn empty_plain_part_falls_through_to_html() {
        let raw = "From: a@example.com\r\n\
To: b@example.com\r\n\
Subject: Newsletter\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"XX\"\r\n\
\r\n\
--XX\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
   \r\n\
--XX\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<html><body><p>The actual newsletter body.</p></body></html>\r\n\
--XX--\r\n";
        let msg = mail_parser::MessageParser::default()
            .parse(raw.as_bytes())
            .unwrap();
        let out = format_message(&msg);
        assert!(
            out.contains("The actual newsletter body."),
            "HTML body must be used when the plain part is blank:\n{out}"
        );
    }

    /// Regression: any body line starting "From " split the message.
    #[test]
    fn prose_starting_with_from_does_not_split_a_message() {
        let mbox = "From alice@example.com Mon Jan  1 00:00:00 2024\r\n\
From: alice@example.com\r\n\
Subject: A story\r\n\
\r\n\
It begins here.\r\n\
\r\n\
From the beginning, we knew it would work.\r\n\
\r\n\
The end.\r\n";
        let out = parse_mbox(mbox.as_bytes()).unwrap();
        assert_eq!(
            out.matches("# Email:").count(),
            1,
            "one message, not two:\n{out}"
        );
        assert!(out.contains("From the beginning"), "{out}");
    }

    #[test]
    fn real_envelope_lines_still_split() {
        let mbox = "From alice@example.com Mon Jan  1 00:00:00 2024\r\n\
From: alice@example.com\r\n\
Subject: One\r\n\
\r\n\
First body.\r\n\
\r\n\
From bob@example.com Tue Jan  2 00:00:00 2024\r\n\
From: bob@example.com\r\n\
Subject: Two\r\n\
\r\n\
Second body.\r\n";
        let out = parse_mbox(mbox.as_bytes()).unwrap();
        assert_eq!(out.matches("# Email:").count(), 2, "{out}");
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

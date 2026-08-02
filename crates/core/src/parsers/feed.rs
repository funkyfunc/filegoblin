//! RSS 2.0 / Atom / RDF feeds → a markdown list of entries.
//!
//! One event-based walker covers all three dialects: they differ mainly in
//! element names (`item`/`entry`, `description`/`summary`, `pubDate`/`updated`)
//! and in how links are carried (RSS text content vs Atom `href` attribute).
//! Entry bodies are usually CDATA-wrapped HTML, so they go through the same DOM
//! extractor email bodies use.

use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::Result;
use std::path::Path;

pub struct FeedGobbler;

#[derive(Default)]
struct Entry {
    title: String,
    link: String,
    date: String,
    author: String,
    summary: String,
    /// `content:encoded` (RSS) / `content` (Atom) — the full article body when
    /// the publisher includes one.
    content: String,
}

#[derive(Default)]
struct Feed {
    title: String,
    description: String,
    link: String,
    entries: Vec<Entry>,
}

impl Gobble for FeedGobbler {
    fn gobble(&self, path: &Path, opts: &ParseOptions) -> Result<String> {
        let content = std::fs::read_to_string(path)?;
        self.gobble_str(&content, opts)
    }

    fn gobble_str(&self, content: &str, opts: &ParseOptions) -> Result<String> {
        let feed = parse_feed(content)?;
        if feed.entries.is_empty() && feed.title.is_empty() {
            anyhow::bail!("No feed entries found (is this really an RSS/Atom document?)");
        }
        Ok(render(&feed, opts.full))
    }
}

/// True when the document looks like a syndication feed (used for both magic
/// sniffing and content-type routing of fetched URLs).
pub fn looks_like_feed(content: &str) -> bool {
    let head = &content[..content.len().min(1024)].to_lowercase();
    head.contains("<rss")
        || head.contains("<feed")
        || (head.contains("<rdf:rdf") && head.contains("rss"))
}

fn parse_feed(xml: &str) -> Result<Feed> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    // NOT trim_text(true): quick-xml trims every text event individually, and
    // entity refs split a run of text into several — so "Tom &amp; Jerry"
    // would lose the spaces around the `&`. Fields are trimmed on completion.
    reader.config_mut().trim_text(false);

    let mut feed = Feed::default();
    let mut entry: Option<Entry> = None;
    let mut tag = String::new();
    let mut buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "item" | "entry" => entry = Some(Entry::default()),
                    // Atom carries the link in an href attribute; prefer
                    // rel="alternate" (the human-readable page) and never
                    // rel="self"/"edit".
                    "link" => {
                        let mut href = None;
                        let mut rel = String::new();
                        for attr in e.attributes().flatten() {
                            match local_name(attr.key.as_ref()).as_str() {
                                "href" => href = attr.unescape_value().ok().map(|v| v.into_owned()),
                                "rel" => {
                                    rel = attr
                                        .unescape_value()
                                        .map(|v| v.into_owned())
                                        .unwrap_or_default()
                                }
                                _ => {}
                            }
                        }
                        if let Some(href) = href
                            && rel != "self"
                            && rel != "edit"
                            && rel != "hub"
                        {
                            let target = match &mut entry {
                                Some(e) => &mut e.link,
                                None => &mut feed.link,
                            };
                            if target.is_empty() {
                                *target = href;
                            }
                        }
                    }
                    _ => {}
                }
                tag = name;
                buf.clear();
            }
            Ok(Event::Text(e)) => {
                if let Ok(t) = std::str::from_utf8(e.as_ref())
                    && let Ok(decoded) = quick_xml::escape::unescape(t)
                {
                    buf.push_str(&decoded);
                }
            }
            Ok(Event::CData(e)) => {
                buf.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            // Escaped entities arrive as their own events in quick-xml 0.39;
            // without this every `<`/`&` in an escaped HTML body vanishes.
            Ok(Event::GeneralRef(e)) => {
                if let Some(s) = crate::parsers::resolve_xml_entity(&e) {
                    buf.push_str(&s);
                }
            }
            Ok(Event::End(ref e)) => {
                let name = local_name(e.name().as_ref());
                let text = std::mem::take(&mut buf).trim().to_string();
                if name == "item" || name == "entry" {
                    if let Some(done) = entry.take() {
                        feed.entries.push(done);
                    }
                } else if name == tag && !text.is_empty() {
                    assign(&mut feed, &mut entry, &name, text);
                }
                tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break, // tolerate malformed tails; emit what we parsed
            _ => {}
        }
    }
    Ok(feed)
}

/// Routes a completed element's text to the feed or the current entry.
fn assign(feed: &mut Feed, entry: &mut Option<Entry>, name: &str, text: String) {
    match entry {
        Some(e) => match name {
            "title" if e.title.is_empty() => e.title = text,
            "link" | "id" if e.link.is_empty() && text.starts_with("http") => e.link = text,
            "pubdate" | "published" | "updated" | "date" if e.date.is_empty() => e.date = text,
            "creator" | "author" | "name" if e.author.is_empty() => e.author = text,
            "description" | "summary" if e.summary.is_empty() => e.summary = text,
            "encoded" | "content" if e.content.is_empty() => e.content = text,
            _ => {}
        },
        None => match name {
            "title" if feed.title.is_empty() => feed.title = text,
            "description" | "subtitle" if feed.description.is_empty() => feed.description = text,
            "link" if feed.link.is_empty() && text.starts_with("http") => feed.link = text,
            _ => {}
        },
    }
}

/// Strips any namespace prefix: `dc:creator` → `creator`, lower-cased.
fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    s.rsplit(':').next().unwrap_or(&s).to_lowercase()
}

/// Feed bodies are usually HTML; reuse the DOM extractor (full mode — an entry
/// body IS the content, no readability scoring wanted).
fn body_to_markdown(html: &str) -> String {
    if !html.contains('<') {
        return html.to_string();
    }
    let gobbler = crate::parsers::html::HtmlGobbler {
        extract_full: true,
        skip_front_matter: true,
        ..Default::default()
    };
    gobbler
        .gobble_str(html, &ParseOptions::default())
        .unwrap_or_else(|_| html.to_string())
}

fn render(feed: &Feed, full: bool) -> String {
    let mut out = String::new();
    if !feed.title.is_empty() {
        out.push_str(&format!("# {}\n\n", feed.title.trim()));
    }
    if !feed.description.is_empty() {
        out.push_str(&format!(
            "> {}\n\n",
            body_to_markdown(&feed.description).trim()
        ));
    }
    if !feed.link.is_empty() {
        out.push_str(&format!("**Source:** {}\n", feed.link.trim()));
    }
    out.push_str(&format!("**Entries:** {}\n\n---\n\n", feed.entries.len()));

    for e in &feed.entries {
        let title = if e.title.is_empty() {
            "(untitled)"
        } else {
            e.title.trim()
        };
        match e.link.trim() {
            "" => out.push_str(&format!("## {}\n\n", title)),
            link => out.push_str(&format!("## [{}]({})\n\n", title, link)),
        }
        let meta: Vec<&str> = [e.date.trim(), e.author.trim()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        if !meta.is_empty() {
            out.push_str(&format!("*{}*\n\n", meta.join(" · ")));
        }
        // `--full` prefers the complete article body when the feed carries one.
        let body = match (full, e.content.is_empty()) {
            (true, false) => &e.content,
            _ if !e.summary.is_empty() => &e.summary,
            _ => &e.content,
        };
        let body = body_to_markdown(body);
        if !body.trim().is_empty() {
            out.push_str(body.trim());
            out.push_str("\n\n");
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
<channel>
  <title>Goblin Weekly</title>
  <link>https://goblin.example/</link>
  <description>Hoard news</description>
  <item>
    <title>Sorting the hoard</title>
    <link>https://goblin.example/sorting</link>
    <pubDate>Sun, 02 Aug 2026 10:00:00 +0000</pubDate>
    <dc:creator>The Archivist</dc:creator>
    <description><![CDATA[<p>A <b>catalogued</b> pile is wealth.</p>]]></description>
  </item>
  <item>
    <title>Cave lighting</title>
    <link>https://goblin.example/lighting</link>
    <description>Torches versus lanterns.</description>
  </item>
</channel>
</rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Goblin Atom</title>
  <subtitle>Feeds for goblins</subtitle>
  <link href="https://goblin.example/feed" rel="self"/>
  <link href="https://goblin.example/" rel="alternate"/>
  <entry>
    <title>Atom entry one</title>
    <link href="https://goblin.example/atom-1" rel="alternate"/>
    <updated>2026-08-02T10:00:00Z</updated>
    <author><name>Scribe</name></author>
    <summary>Short summary here.</summary>
  </entry>
</feed>"#;

    #[test]
    fn parses_rss_items_with_html_bodies() {
        let out = FeedGobbler
            .gobble_str(RSS, &ParseOptions::default())
            .unwrap();
        assert!(out.contains("# Goblin Weekly"));
        assert!(out.contains("**Entries:** 2"));
        assert!(out.contains("## [Sorting the hoard](https://goblin.example/sorting)"));
        assert!(out.contains("The Archivist"));
        // CDATA HTML becomes markdown, not raw tags.
        assert!(out.contains("**catalogued**"), "got: {out}");
        assert!(!out.contains("<b>"));
        assert!(out.contains("## [Cave lighting](https://goblin.example/lighting)"));
    }

    #[test]
    fn parses_atom_and_prefers_alternate_link() {
        let out = FeedGobbler
            .gobble_str(ATOM, &ParseOptions::default())
            .unwrap();
        assert!(out.contains("# Goblin Atom"));
        assert!(out.contains("## [Atom entry one](https://goblin.example/atom-1)"));
        assert!(out.contains("Scribe"));
        assert!(out.contains("Short summary here."));
        // rel="self" must not become the feed's source link.
        assert!(!out.contains("**Source:** https://goblin.example/feed"));
    }

    /// Regression: quick-xml 0.39 emits entity refs as separate events, so a
    /// Text-only walker silently dropped every `<`/`&` — an Atom `<content>`
    /// of escaped HTML came out as `pHello/p`.
    #[test]
    fn escaped_html_content_survives_entity_events() {
        let atom = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>T</title>
<entry><title>E</title>
<content type="html">&lt;p&gt;Tom &amp; Jerry say &lt;b&gt;hi&lt;/b&gt;.&lt;/p&gt;</content>
</entry></feed>"#;
        let out = FeedGobbler
            .gobble_str(atom, &ParseOptions::default())
            .unwrap();
        assert!(out.contains("Tom & Jerry"), "ampersand lost: {out}");
        assert!(out.contains("**hi**"), "escaped HTML not converted: {out}");
        assert!(!out.contains("pTom"), "angle brackets stripped: {out}");
    }

    #[test]
    fn detects_feed_shapes() {
        assert!(looks_like_feed(RSS));
        assert!(looks_like_feed(ATOM));
        assert!(!looks_like_feed("<html><body>not a feed</body></html>"));
    }

    #[test]
    fn non_feed_xml_errors_honestly() {
        let err = FeedGobbler
            .gobble_str("<root><thing>x</thing></root>", &ParseOptions::default())
            .unwrap_err();
        assert!(err.to_string().contains("No feed entries"));
    }
}

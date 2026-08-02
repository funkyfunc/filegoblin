//! SubRip (`.srt`) and WebVTT (`.vtt`) subtitle files → transcript markdown.
//!
//! One cue walker covers both: they differ mainly in the millisecond separator
//! (`,` vs `.`), VTT's header/NOTE/STYLE blocks, and VTT's cue settings and
//! inline tags. Output goes through the shared transcript renderer, so a
//! subtitle file and a whisper transcription of the same media look the same.

use crate::parsers::gobble::{Gobble, ParseOptions};
use crate::parsers::transcript::{Segment, render};
use anyhow::Result;
use std::path::Path;

pub struct SubtitleGobbler;

impl Gobble for SubtitleGobbler {
    fn gobble(&self, path: &Path, opts: &ParseOptions) -> Result<String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read subtitle file: {e}"))?;
        let title = path.file_name().unwrap_or_default().to_string_lossy();
        self.render_with_title(&content, &title, opts)
    }

    fn gobble_str(&self, content: &str, opts: &ParseOptions) -> Result<String> {
        self.render_with_title(content, "subtitles", opts)
    }
}

impl SubtitleGobbler {
    fn render_with_title(&self, content: &str, title: &str, opts: &ParseOptions) -> Result<String> {
        let is_vtt = looks_like_vtt(content);
        let segments = parse_cues(content);
        if segments.is_empty() {
            anyhow::bail!("No subtitle cues found (expected SubRip or WebVTT timing lines)");
        }
        let meta = vec![
            (
                "Format",
                if is_vtt { "WebVTT" } else { "SubRip" }.to_string(),
            ),
            ("Cues", segments.len().to_string()),
        ];
        Ok(render(title, &meta, &segments, opts.full))
    }
}

/// True for a WebVTT document (the spec requires the magic first line).
pub fn looks_like_vtt(content: &str) -> bool {
    content.trim_start().starts_with("WEBVTT")
}

/// True when the text carries subtitle cue timings — used for magic sniffing of
/// renamed / extension-less files.
pub fn looks_like_subtitles(content: &str) -> bool {
    if looks_like_vtt(content) {
        return true;
    }
    // SubRip has no magic header, so look for an actual timing line.
    content
        .lines()
        .take(40)
        .any(|l| l.contains("-->") && parse_timing_line(l).is_some())
}

/// Parses `00:00:01,000 --> 00:00:04,000` (plus optional VTT cue settings)
/// into a start time in centiseconds.
fn parse_timing_line(line: &str) -> Option<i64> {
    let (start, _rest) = line.split_once("-->")?;
    parse_timestamp(start)
}

/// `HH:MM:SS,mmm` / `HH:MM:SS.mmm` / `MM:SS.mmm` → centiseconds.
fn parse_timestamp(raw: &str) -> Option<i64> {
    let cleaned = raw.trim().replace(',', ".");
    let (hms, frac) = match cleaned.split_once('.') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (cleaned, "0".to_string()),
    };
    let parts: Vec<&str> = hms.split(':').collect();
    let (h, m, s): (i64, i64, i64) = match parts.as_slice() {
        [h, m, s] => (h.trim().parse().ok()?, m.parse().ok()?, s.parse().ok()?),
        [m, s] => (0, m.trim().parse().ok()?, s.parse().ok()?),
        _ => return None,
    };
    // Fractional part is milliseconds; keep centisecond precision.
    let millis: i64 = frac
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(3)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    Some((h * 3600 + m * 60 + s) * 100 + millis / 10)
}

/// Walks cues in either dialect. Non-cue blocks (WEBVTT header, NOTE/STYLE/
/// REGION, SubRip index numbers) are skipped; consecutive duplicate lines are
/// collapsed, because rolling auto-captions repeat the previous line in every
/// cue and would otherwise trible the token count.
fn parse_cues(content: &str) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut lines = content.lines().peekable();
    let mut last_line = String::new();

    while let Some(line) = lines.next() {
        // Skip VTT metadata blocks wholesale.
        let head = line.trim_start();
        if head.starts_with("NOTE") || head.starts_with("STYLE") || head.starts_with("REGION") {
            for l in lines.by_ref() {
                if l.trim().is_empty() {
                    break;
                }
            }
            continue;
        }
        if !line.contains("-->") {
            continue;
        }
        let Some(start_cs) = parse_timing_line(line) else {
            continue;
        };

        // Cue payload: everything up to the next blank line.
        let mut text = String::new();
        for l in lines.by_ref() {
            if l.trim().is_empty() {
                break;
            }
            let clean = strip_cue_tags(l).trim().to_string();
            if clean.is_empty() || clean == last_line {
                continue; // rolling-caption repeat
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&clean);
            last_line = clean;
        }

        if !text.trim().is_empty() {
            segments.push(Segment {
                start_cs,
                text: text.trim().to_string(),
            });
        }
    }
    segments
}

/// Removes inline markup: VTT `<v Speaker>` (kept as a `Speaker:` prefix),
/// styling tags (`<b>`, `<i>`, `<c.loud>`), karaoke timestamps
/// (`<00:00:01.000>`), and SubRip's `{\an8}` position overrides.
fn strip_cue_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            out.push_str(&rest[open..]);
            return finish_cue_text(&out);
        };
        let tag = &rest[open + 1..open + close];
        // Voice spans name the speaker: <v Alice> or <v.loud Alice>
        if let Some(name) = tag.strip_prefix('v')
            && (name.is_empty() || name.starts_with([' ', '.']))
        {
            let speaker = name.trim_start_matches('.').trim();
            let speaker = speaker.split_whitespace().next().unwrap_or("");
            if !speaker.is_empty() {
                out.push_str(&format!("{}: ", speaker));
            }
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    finish_cue_text(&out)
}

/// Drops SubRip `{...}` overrides and decodes the handful of HTML entities that
/// show up in caption files.
fn finish_cue_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        match rest[open..].find('}') {
            Some(close) => rest = &rest[open + close + 1..],
            None => {
                out.push_str(&rest[open..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\n\
00:00:01,000 --> 00:00:04,000\n\
The goblin devours documents.\n\
\n\
2\n\
00:00:04,500 --> 00:00:08,000\n\
<i>And produces</i> clean markdown.\n\
\n\
3\n\
00:01:30,000 --> 00:01:33,000\n\
{\\an8}Much later in the film.\n";

    const VTT: &str = "WEBVTT\n\
\n\
NOTE This is a comment block\n\
that spans lines.\n\
\n\
00:00:01.000 --> 00:00:04.000 align:start position:10%\n\
<v Alice>Hello from the hoard.\n\
\n\
00:00:04.500 --> 00:00:06.000\n\
<v Bob>Tom &amp; Jerry approve.\n";

    #[test]
    fn parses_srt_with_tags_and_overrides() {
        let out = SubtitleGobbler
            .gobble_str(SRT, &ParseOptions::default())
            .unwrap();
        assert!(out.contains("**Format:** SubRip"));
        assert!(out.contains("**Cues:** 3"));
        assert!(out.contains("The goblin devours documents."));
        assert!(out.contains("And produces clean markdown."), "got: {out}");
        assert!(!out.contains("<i>"));
        assert!(!out.contains("{\\an8}"), "position override leaked: {out}");
        // 90s gap starts a new timestamped paragraph.
        assert!(
            out.contains("**[00:01:30]** Much later in the film."),
            "got: {out}"
        );
    }

    #[test]
    fn parses_vtt_with_speakers_and_skips_notes() {
        let out = SubtitleGobbler
            .gobble_str(VTT, &ParseOptions::default())
            .unwrap();
        assert!(out.contains("**Format:** WebVTT"));
        assert!(out.contains("Alice: Hello from the hoard."), "got: {out}");
        assert!(out.contains("Bob: Tom & Jerry approve."), "got: {out}");
        assert!(
            !out.contains("This is a comment block"),
            "NOTE leaked: {out}"
        );
        assert!(!out.contains("align:start"), "cue settings leaked: {out}");
    }

    #[test]
    fn rolling_captions_are_deduped() {
        // YouTube-style auto-captions repeat the previous line every cue.
        let rolling = "WEBVTT\n\n\
00:00:01.000 --> 00:00:03.000\nhello there\n\n\
00:00:03.000 --> 00:00:05.000\nhello there\nand welcome\n\n\
00:00:05.000 --> 00:00:07.000\nand welcome\nto the show\n";
        let out = SubtitleGobbler
            .gobble_str(rolling, &ParseOptions::default())
            .unwrap();
        assert_eq!(out.matches("hello there").count(), 1, "got: {out}");
        assert_eq!(out.matches("and welcome").count(), 1, "got: {out}");
        assert!(out.contains("to the show"));
    }

    #[test]
    fn timestamp_forms_parse() {
        assert_eq!(parse_timestamp("00:00:01,500"), Some(150));
        assert_eq!(parse_timestamp("00:00:01.500"), Some(150));
        assert_eq!(parse_timestamp("01:02:03.000"), Some(372_300));
        assert_eq!(parse_timestamp("02:03.000"), Some(12_300)); // MM:SS short form
        assert_eq!(parse_timestamp("not a time"), None);
    }

    #[test]
    fn sniffing_distinguishes_subtitles() {
        assert!(looks_like_subtitles(VTT));
        assert!(looks_like_subtitles(SRT));
        assert!(!looks_like_subtitles("just some prose\nwith no timings\n"));
    }

    #[test]
    fn empty_or_untimed_file_errors_honestly() {
        let err = SubtitleGobbler
            .gobble_str("no cues here", &ParseOptions::default())
            .unwrap_err();
        assert!(err.to_string().contains("No subtitle cues"));
    }
}

//! Shared transcript rendering, so a whisper transcription and a subtitle file
//! come out looking identical — the source of the timing shouldn't change the
//! shape of the markdown an agent reads.

/// One timed chunk of speech. `start_cs` is centiseconds from the start of the
/// media (whisper's native unit).
pub struct Segment {
    pub start_cs: i64,
    pub text: String,
}

/// Centiseconds → `HH:MM:SS`.
pub fn fmt_ts(cs: i64) -> String {
    let s = cs / 100;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Renders segments as markdown.
///
/// Default is sparse-timestamped paragraphs — a `**[HH:MM:SS]**` marker only
/// when the speech gap exceeds 2s, the paragraph passes ~700 chars, or a minute
/// has elapsed. Stamping every segment would roughly double the token count for
/// no gain in an LLM context; `per_segment` (`--full`) does that anyway for
/// callers who want alignment data.
pub fn render(
    title: &str,
    meta: &[(&str, String)],
    segments: &[Segment],
    per_segment: bool,
) -> String {
    let mut out = format!("# Transcript: {}\n\n", title);
    if !meta.is_empty() {
        let line: Vec<String> = meta
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("**{}:** {}", k, v))
            .collect();
        if !line.is_empty() {
            out.push_str(&line.join(" · "));
            out.push_str("\n\n");
        }
    }

    if per_segment {
        for s in segments {
            out.push_str(&format!("**[{}]** {}\n", fmt_ts(s.start_cs), s.text));
        }
        return out.trim_end().to_string();
    }

    let mut para = String::new();
    let mut para_start = 0i64;
    let mut last_start = 0i64;
    for s in segments {
        let gap = s.start_cs - last_start;
        let elapsed = s.start_cs - para_start;
        if !para.is_empty() && (gap > 200 || para.len() > 700 || elapsed > 6_000) {
            out.push_str(&format!("**[{}]** {}\n\n", fmt_ts(para_start), para.trim()));
            para.clear();
        }
        if para.is_empty() {
            para_start = s.start_cs;
        }
        para.push_str(&s.text);
        para.push(' ');
        last_start = s.start_cs;
    }
    if !para.trim().is_empty() {
        out.push_str(&format!("**[{}]** {}\n", fmt_ts(para_start), para.trim()));
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs() -> Vec<Segment> {
        vec![
            Segment {
                start_cs: 0,
                text: "Hello there.".into(),
            },
            Segment {
                start_cs: 150,
                text: "Second sentence.".into(),
            },
            // 5.5s gap → new paragraph
            Segment {
                start_cs: 700,
                text: "After a pause.".into(),
            },
        ]
    }

    #[test]
    fn sparse_paragraphs_break_on_gaps() {
        let out = render("meeting.m4a", &[("Language", "en".into())], &segs(), false);
        assert!(out.contains("# Transcript: meeting.m4a"));
        assert!(out.contains("**Language:** en"));
        assert!(out.contains("**[00:00:00]** Hello there. Second sentence."));
        assert!(out.contains("**[00:00:07]** After a pause."));
    }

    #[test]
    fn per_segment_stamps_everything() {
        let out = render("a.srt", &[], &segs(), true);
        assert!(out.contains("**[00:00:01]** Second sentence."));
        assert!(out.contains("**[00:00:07]** After a pause."));
    }

    #[test]
    fn timestamps_roll_over_correctly() {
        assert_eq!(fmt_ts(0), "00:00:00");
        assert_eq!(fmt_ts(6_000), "00:01:00");
        assert_eq!(fmt_ts(360_000), "01:00:00");
    }
}

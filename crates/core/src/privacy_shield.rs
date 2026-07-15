//! The Privacy Shield: a 100%-local, zero-telemetry PII / secret scrubber.
//!
//! Detection has three layers, all running without any model weights:
//!   1. **Typed rules** — regexes for well-known secret and PII shapes, several
//!      gated by a validator (Luhn for cards, range checks for SSN, mod-97 for IBAN)
//!      so we redact real values, not any digit run that looks vaguely similar.
//!   2. **Entropy catch-all** — a sliding Shannon-entropy window that flags unbroken
//!      high-entropy tokens (keys/hashes) no fixed regex anticipated.
//!   3. **Allowlist / custom patterns** — user configuration layered on top.
//!
//! Every match carries its [`PiiKind`], so output can be typed (`[EMAIL]`, `[AWS_KEY]`)
//! rather than an opaque blob, and a per-run audit tally is available via [`PrivacyShield::counts`].

use crate::cli::RedactStyle;
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

/// Snap a byte index down to the nearest UTF-8 char boundary (never panics).
fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Snap a byte index up to the nearest UTF-8 char boundary (never panics).
fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// The category of a detected value. `label()` is what appears in typed output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PiiKind {
    Email,
    Ssn,
    CreditCard,
    PhoneNumber,
    IpAddress,
    Iban,
    AwsKey,
    GithubToken,
    OpenAiKey,
    GoogleApiKey,
    SlackToken,
    StripeKey,
    Jwt,
    PrivateKeyBlock,
    ConnectionString,
    /// High-entropy token from the catch-all tier, or a generic secret assignment.
    Secret,
    /// A user-defined pattern; the string is its (upper-cased) label.
    Custom(String),
}

impl PiiKind {
    pub fn label(&self) -> String {
        match self {
            PiiKind::Email => "EMAIL".into(),
            PiiKind::Ssn => "SSN".into(),
            PiiKind::CreditCard => "CREDIT_CARD".into(),
            PiiKind::PhoneNumber => "PHONE".into(),
            PiiKind::IpAddress => "IP_ADDRESS".into(),
            PiiKind::Iban => "IBAN".into(),
            PiiKind::AwsKey => "AWS_KEY".into(),
            PiiKind::GithubToken => "GITHUB_TOKEN".into(),
            PiiKind::OpenAiKey => "OPENAI_KEY".into(),
            PiiKind::GoogleApiKey => "GOOGLE_API_KEY".into(),
            PiiKind::SlackToken => "SLACK_TOKEN".into(),
            PiiKind::StripeKey => "STRIPE_KEY".into(),
            PiiKind::Jwt => "JWT".into(),
            PiiKind::PrivateKeyBlock => "PRIVATE_KEY".into(),
            PiiKind::ConnectionString => "CONNECTION_STRING".into(),
            PiiKind::Secret => "SECRET".into(),
            PiiKind::Custom(name) => name.clone(),
        }
    }
}

/// A detected span within the input, in byte offsets.
#[derive(Debug, Clone)]
pub struct Match {
    pub start: usize,
    pub end: usize,
    pub kind: PiiKind,
}

// ---------------------------------------------------------------------------
// Validators (reduce false positives on the numeric patterns)
// ---------------------------------------------------------------------------

/// Luhn checksum over the digits in `s` (13–19 digits). Filters random number runs.
fn luhn_valid(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let mut double = false;
    for &d in digits.iter().rev() {
        let mut v = d;
        if double {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
        double = !double;
    }
    sum.is_multiple_of(10)
}

/// US SSN structural validity: area != 000/666/900–999, group != 00, serial != 0000.
fn ssn_valid(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() != 9 {
        return false;
    }
    let area = digits[0] * 100 + digits[1] * 10 + digits[2];
    let group = digits[3] * 10 + digits[4];
    let serial = digits[5] * 1000 + digits[6] * 100 + digits[7] * 10 + digits[8];
    area != 0 && area != 666 && area < 900 && group != 0 && serial != 0
}

/// IPv4 dotted-quad with every octet <= 255.
fn ipv4_valid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok())
}

/// IBAN mod-97 checksum (ISO 13616): rearrange, map letters to numbers, mod 97 == 1.
fn iban_valid(s: &str) -> bool {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() < 15 || cleaned.len() > 34 {
        return false;
    }
    let (head, tail) = cleaned.split_at(4);
    let rearranged = format!("{}{}", tail, head);
    let mut remainder = 0u32;
    for c in rearranged.chars() {
        let value = if c.is_ascii_digit() {
            c as u32 - '0' as u32
        } else if c.is_ascii_alphabetic() {
            c.to_ascii_uppercase() as u32 - 'A' as u32 + 10
        } else {
            return false;
        };
        // Fold digit-by-digit to avoid overflow (value is 1 or 2 decimal digits).
        if value >= 10 {
            remainder = (remainder * 100 + value) % 97;
        } else {
            remainder = (remainder * 10 + value) % 97;
        }
    }
    remainder == 1
}

// ---------------------------------------------------------------------------
// Entropy trigger (catch-all tier)
// ---------------------------------------------------------------------------

/// Flags whitespace-free windows whose Shannon entropy exceeds a threshold.
pub struct PiiTrigger {
    window_size: usize,
    entropy_threshold: f64,
}

impl PiiTrigger {
    pub fn new(window_size: usize, threshold: f64) -> Self {
        Self {
            window_size,
            entropy_threshold: threshold,
        }
    }

    fn calculate_entropy(&self, data: &[u8]) -> f64 {
        let mut counts = HashMap::new();
        for &byte in data {
            *counts.entry(byte).or_insert(0u32) += 1;
        }
        let len = data.len() as f64;
        let mut entropy = 0.0;
        for &count in counts.values() {
            let p = count as f64 / len;
            entropy -= p * p.log2();
        }
        entropy
    }

    /// Returns byte windows that are whitespace-free and exceed the threshold.
    pub fn scan(&self, text: &str) -> Vec<(usize, usize)> {
        let bytes = text.as_bytes();
        let mut points = Vec::new();
        if bytes.len() < self.window_size {
            return points;
        }
        for i in 0..=(bytes.len() - self.window_size) {
            let window = &bytes[i..i + self.window_size];
            // Secrets are unbroken tokens; prose has whitespace within 24 chars.
            if window.iter().any(|b| b.is_ascii_whitespace()) {
                continue;
            }
            if self.calculate_entropy(window) > self.entropy_threshold {
                points.push((i, i + self.window_size));
            }
        }
        points
    }
}

/// Merges overlapping / near-adjacent `(start, end)` spans.
pub struct IndexMerger;

impl IndexMerger {
    pub fn merge(windows: Vec<(usize, usize)>, look_back: usize) -> Vec<(usize, usize)> {
        if windows.is_empty() {
            return vec![];
        }
        let mut sorted = windows;
        sorted.sort_by_key(|&(s, _)| s);
        let mut merged = Vec::new();
        let (mut cs, mut ce) = sorted[0];
        for &(s, e) in sorted.iter().skip(1) {
            if s <= ce + look_back {
                ce = ce.max(e);
            } else {
                merged.push((cs, ce));
                cs = s;
                ce = e;
            }
        }
        merged.push((cs, ce));
        merged
    }
}

// ---------------------------------------------------------------------------
// Rule set
// ---------------------------------------------------------------------------

struct RegexRule {
    kind: PiiKind,
    regex: Regex,
    validate: Option<fn(&str) -> bool>,
}

fn default_rules() -> Result<Vec<RegexRule>> {
    // Helper to cut down on boilerplate. The `regex` crate is linear-time, so none
    // of these can trigger catastrophic backtracking on hostile input.
    fn rule(kind: PiiKind, pat: &str, validate: Option<fn(&str) -> bool>) -> Result<RegexRule> {
        Ok(RegexRule {
            kind,
            regex: Regex::new(pat).with_context(|| format!("bad builtin pattern: {pat}"))?,
            validate,
        })
    }

    Ok(vec![
        // Structured PII
        rule(
            PiiKind::Email,
            r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b",
            None,
        )?,
        rule(
            PiiKind::Ssn,
            r"\b\d{3}[- ]\d{2}[- ]\d{4}\b",
            Some(ssn_valid),
        )?,
        rule(
            PiiKind::CreditCard,
            r"\b\d(?:[ -]?\d){12,18}\b",
            Some(luhn_valid),
        )?,
        rule(
            PiiKind::PhoneNumber,
            r"\b\+?[0-9]{1,3}[-. ]?\(?[0-9]{3}\)?[-. ]?[0-9]{3}[-. ]?[0-9]{4}\b",
            None,
        )?,
        rule(
            PiiKind::IpAddress,
            r"\b(?:\d{1,3}\.){3}\d{1,3}\b",
            Some(ipv4_valid),
        )?,
        rule(
            PiiKind::Iban,
            r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b",
            Some(iban_valid),
        )?,
        // Secrets / tokens
        rule(PiiKind::AwsKey, r"\bAKIA[0-9A-Z]{16}\b", None)?,
        rule(
            PiiKind::GithubToken,
            r"\bgh[pousr]_[A-Za-z0-9]{20,}\b",
            None,
        )?,
        rule(PiiKind::GoogleApiKey, r"\bAIza[0-9A-Za-z_\-]{35}\b", None)?,
        rule(
            PiiKind::SlackToken,
            r"\bxox[baprs]-[0-9A-Za-z-]{10,}\b",
            None,
        )?,
        rule(
            PiiKind::StripeKey,
            r"\b[a-z]{2}_(?:live|test)_[0-9A-Za-z]{16,}\b",
            None,
        )?,
        rule(PiiKind::OpenAiKey, r"\bsk-[A-Za-z0-9]{20,}\b", None)?,
        rule(PiiKind::Jwt, r"\beyJ[A-Za-z0-9_/+=-]{10,}\b", None)?,
        rule(
            PiiKind::PrivateKeyBlock,
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            None,
        )?,
        rule(
            PiiKind::ConnectionString,
            r"\b[a-zA-Z][a-zA-Z0-9+.\-]*://[^\s:@/]+:[^\s:@/]+@[^\s/]+",
            None,
        )?,
        // Generic "api_key = <token>" assignment
        rule(
            PiiKind::Secret,
            r"(?i)\bapi[_-]?key.{1,20}?[a-zA-Z0-9]{20,}\b",
            None,
        )?,
    ])
}

// ---------------------------------------------------------------------------
// The shield
// ---------------------------------------------------------------------------

/// Assigns stable, indexed placeholders per (label, value) for `Pseudonym` mode.
/// `EMAIL`'s first distinct value becomes `[EMAIL_1]`, the next `[EMAIL_2]`, and any
/// later occurrence of a seen value reuses its index.
#[derive(Default)]
struct PseudoInterner {
    assigned: HashMap<(String, String), usize>,
    next_index: HashMap<String, usize>,
}

impl PseudoInterner {
    fn placeholder(&mut self, label: &str, value: &str) -> String {
        let key = (label.to_string(), value.to_string());
        let idx = *self.assigned.entry(key).or_insert_with(|| {
            let counter = self.next_index.entry(label.to_string()).or_insert(0);
            *counter += 1;
            *counter
        });
        format!("[{}_{}]", label, idx)
    }
}

pub struct PrivacyShield {
    rules: Vec<RegexRule>,
    trigger: PiiTrigger,
    allowlist: HashSet<String>,
    style: RedactStyle,
    /// Per-run tally by label. Interior-mutable + `Sync` so one shared shield can
    /// accumulate across rayon workers and the streaming pipeline.
    counters: Mutex<BTreeMap<String, usize>>,
    /// Stable placeholder assignments for `Pseudonym` mode (shared across the run).
    interner: Mutex<PseudoInterner>,
}

impl PrivacyShield {
    /// A shield with the default rule set and typed output (used by tests/simple callers).
    pub fn init() -> Result<Self> {
        Self::new(RedactStyle::Typed)
    }

    pub fn new(style: RedactStyle) -> Result<Self> {
        Ok(Self {
            rules: default_rules()?,
            // 24-byte window @ 4.0 bits: prose sits below, unbroken secret tokens above.
            trigger: PiiTrigger::new(24, 4.0),
            allowlist: HashSet::new(),
            style,
            counters: Mutex::new(BTreeMap::new()),
            interner: Mutex::new(PseudoInterner::default()),
        })
    }

    /// Builds a shield with an allowlist and user-defined `(label, regex)` patterns.
    /// Invalid custom regexes are reported to stderr and skipped, not fatal.
    pub fn with_config(
        style: RedactStyle,
        allowlist: Vec<String>,
        custom: &[(String, String)],
    ) -> Result<Self> {
        let mut shield = Self::new(style)?;
        shield.allowlist = allowlist.into_iter().collect();
        for (name, pat) in custom {
            match Regex::new(pat) {
                Ok(regex) => shield.rules.push(RegexRule {
                    kind: PiiKind::Custom(name.to_uppercase()),
                    regex,
                    validate: None,
                }),
                Err(e) => eprintln!("⚠️ Ignoring invalid custom scrub pattern '{}': {}", name, e),
            }
        }
        Ok(shield)
    }

    /// Finds every non-overlapping match, most-specific rules winning over the
    /// entropy catch-all, with allowlisted values removed.
    pub fn scan(&self, input: &str) -> Vec<Match> {
        let mut matches: Vec<Match> = Vec::new();

        // 1. Rule matches (validated).
        for rule in &self.rules {
            for m in rule.regex.find_iter(input) {
                let text = &input[m.start()..m.end()];
                if let Some(v) = rule.validate
                    && !v(text)
                {
                    continue;
                }
                matches.push(Match {
                    start: m.start(),
                    end: m.end(),
                    kind: rule.kind.clone(),
                });
            }
        }

        // 2. Entropy catch-all, but only where no rule already fired.
        let entropy_spans = IndexMerger::merge(self.trigger.scan(input), 8);
        for (s, e) in entropy_spans {
            let overlaps_rule = matches.iter().any(|m| s < m.end && m.start < e);
            if !overlaps_rule {
                matches.push(Match {
                    start: s,
                    end: e,
                    kind: PiiKind::Secret,
                });
            }
        }

        // 3. Drop allowlisted values.
        if !self.allowlist.is_empty() {
            matches.retain(|m| !self.allowlist.contains(&input[m.start..m.end]));
        }

        // 4. Resolve overlaps: earliest start, then longest, wins.
        matches.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        let mut kept: Vec<Match> = Vec::new();
        let mut last_end = 0usize;
        for m in matches {
            if m.start >= last_end {
                last_end = m.end;
                kept.push(m);
            }
        }
        kept
    }

    /// Redacts `input`, tallying counts by kind as a side effect.
    pub fn redact(&self, input: &str) -> String {
        let kept = self.scan(input); // already sorted ascending by start
        if kept.is_empty() {
            return input.to_string();
        }

        // Tally.
        {
            let mut counters = self.counters.lock().unwrap();
            for m in &kept {
                *counters.entry(m.kind.label()).or_insert(0) += 1;
            }
        }

        // Compute replacements in reading order so pseudonym indices are assigned
        // first-seen-first (`[EMAIL_1]` before `[EMAIL_2]`), independent of the
        // back-to-front application below.
        let replacements: Vec<(usize, usize, String)> = kept
            .iter()
            .filter_map(|m| {
                let s = floor_char_boundary(input, m.start);
                let e = ceil_char_boundary(input, m.end);
                if s >= e {
                    return None;
                }
                let replacement = match self.style {
                    RedactStyle::Typed => format!("[{}]", m.kind.label()),
                    RedactStyle::Plain => "[REDACTED]".to_string(),
                    RedactStyle::Pseudonym => self
                        .interner
                        .lock()
                        .unwrap()
                        .placeholder(&m.kind.label(), &input[s..e]),
                };
                Some((s, e, replacement))
            })
            .collect();

        // Apply from the end so earlier byte offsets stay valid.
        let mut output = input.to_string();
        for (s, e, replacement) in replacements.into_iter().rev() {
            output.replace_range(s..e, &replacement);
        }
        output
    }

    /// The per-run tally, keyed by label (e.g. `"EMAIL" -> 3`).
    pub fn counts(&self) -> BTreeMap<String, usize> {
        self.counters.lock().unwrap().clone()
    }

    /// Total number of redactions so far.
    pub fn total(&self) -> usize {
        self.counters.lock().unwrap().values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shield() -> PrivacyShield {
        PrivacyShield::init().unwrap()
    }

    #[test]
    fn char_boundary_helpers() {
        let s = "a😀b";
        assert_eq!(floor_char_boundary(s, 3), 1);
        assert_eq!(ceil_char_boundary(s, 3), 5);
    }

    #[test]
    fn typed_labels_for_structured_pii() {
        let s = shield();
        assert_eq!(
            s.redact("mail me at dino@example.com now"),
            "mail me at [EMAIL] now"
        );
        assert_eq!(s.redact("ssn 123-45-6789 ok"), "ssn [SSN] ok");
    }

    #[test]
    fn luhn_filters_false_positives() {
        let s = shield();
        // Valid Visa test number (passes Luhn) → redacted.
        assert!(
            s.redact("card 4111 1111 1111 1111 end")
                .contains("[CREDIT_CARD]")
        );
        // A 16-digit run that fails Luhn → left alone.
        assert_eq!(
            s.redact("order 1234567812345678 shipped"),
            "order 1234567812345678 shipped"
        );
    }

    #[test]
    fn ssn_range_check() {
        let s = shield();
        // Invalid area 000 → not an SSN.
        assert_eq!(s.redact("id 000-12-3456 here"), "id 000-12-3456 here");
    }

    #[test]
    fn secret_prefixes_typed() {
        let s = shield();
        assert!(s.redact("k AKIAIOSFODNN7EXAMPLE").contains("[AWS_KEY]"));
        assert!(
            s.redact("t ghp_xYz123Abc456DeF789GHi012JkL345MnO")
                .contains("[GITHUB_TOKEN]")
        );
        assert!(
            s.redact("jwt eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9 x")
                .contains("[JWT]")
        );
    }

    #[test]
    fn entropy_catch_all_typed_secret() {
        let s = shield();
        let out = s.redact("value=Zx9Kq2Vb7Lm4Np1Wr8Ts5Yc3Ad6Bf0Gh end");
        assert!(out.contains("[SECRET]"), "got: {out}");
        assert!(!out.contains("Zx9Kq2Vb7Lm4Np1Wr8Ts5Yc3"));
    }

    #[test]
    fn plain_style() {
        let s = PrivacyShield::new(RedactStyle::Plain).unwrap();
        assert_eq!(s.redact("mail dino@example.com x"), "mail [REDACTED] x");
    }

    #[test]
    fn pseudonym_style_is_consistent() {
        let s = PrivacyShield::new(RedactStyle::Pseudonym).unwrap();
        // Same value → same placeholder, even across separate redact() calls (shared run).
        assert_eq!(
            s.redact("from a@x.com to b@x.com"),
            "from [EMAIL_1] to [EMAIL_2]"
        );
        assert_eq!(s.redact("reply a@x.com again"), "reply [EMAIL_1] again");
        // Distinct kinds get independent counters.
        assert_eq!(s.redact("key AKIAIOSFODNN7EXAMPLE"), "key [AWS_KEY_1]");
    }

    #[test]
    fn allowlist_exempts_value() {
        let s = PrivacyShield::with_config(
            RedactStyle::Typed,
            vec!["dino@example.com".to_string()],
            &[],
        )
        .unwrap();
        assert_eq!(
            s.redact("mail dino@example.com x"),
            "mail dino@example.com x"
        );
    }

    #[test]
    fn custom_pattern_redacts() {
        let s = PrivacyShield::with_config(
            RedactStyle::Typed,
            vec![],
            &[("employee_id".to_string(), r"EMP-\d{6}".to_string())],
        )
        .unwrap();
        assert_eq!(s.redact("hi EMP-123456 bye"), "hi [EMPLOYEE_ID] bye");
    }

    #[test]
    fn counts_are_tallied() {
        let s = shield();
        s.redact("a@b.com and c@d.com and AKIAIOSFODNN7EXAMPLE");
        let counts = s.counts();
        assert_eq!(counts.get("EMAIL"), Some(&2));
        assert_eq!(counts.get("AWS_KEY"), Some(&1));
        assert_eq!(s.total(), 3);
    }

    #[test]
    fn normal_prose_survives() {
        let s = shield();
        let safe = "The office in Seattle opens next month.";
        assert_eq!(s.redact(safe), safe);
    }

    #[test]
    fn no_panic_on_high_entropy_multibyte() {
        let s = shield();
        let input = "秘密鍵日本語🔐éàçüößΩ≈ç√∫˜µ≤≥÷🎉🚀🔥한국어テスト中文字符";
        let _ = s.redact(input);
    }
}

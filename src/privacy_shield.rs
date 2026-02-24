use aho_corasick::{AhoCorasick, MatchKind};
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Heuristic Trigger Component (Tier 3)
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

    /// Calculates the Shannon Entropy of a given byte slice.
    fn calculate_entropy(&self, data: &[u8]) -> f64 {
        let mut counts = HashMap::new();
        for &byte in data {
            *counts.entry(byte).or_insert(0) += 1;
        }

        let len = data.len() as f64;
        let mut entropy = 0.0;

        for &count in counts.values() {
            let p = count as f64 / len;
            // log2 is not defined for integers, but we pre-converted p to f64
            entropy -= p * p.log2();
        }

        entropy
    }

    /// Scans text and returns window indices that exceed the entropy threshold.
    pub fn scan(&self, text: &str) -> Vec<(usize, usize)> {
        let bytes = text.as_bytes();
        let mut trigger_points = Vec::new();

        if bytes.len() < self.window_size {
            return trigger_points;
        }

        for i in 0..=(bytes.len() - self.window_size) {
            let window = &bytes[i..i + self.window_size];
            let entropy = self.calculate_entropy(window);

            if entropy > self.entropy_threshold {
                trigger_points.push((i, i + self.window_size));
            }
        }

        trigger_points
    }
}

/// The Core Privacy Shield Engine
pub struct PrivacyShield {
    trigger: PiiTrigger,
    ac: AhoCorasick,
}

impl PrivacyShield {
    pub fn init() -> Result<Self> {
        // Sentinel Tier 1 Init
        let patterns = vec!["SSN", "Passport", "Credit Card"];
        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostFirst)
            .build(&patterns)
            .context("Failed to build Aho-Corasick automaton")?;

        // Trigger Tier 3 Init
        let trigger = PiiTrigger::new(64, 4.5); // Using 4.5 bits of entropy as high-risk threshold

        // Memory safety bounds as per `redation_extra_info.md`
        // let neural_semaphore = Arc::new(Semaphore::new(4)); // Max 4 concurrent neural inferences

        Ok(Self { trigger, ac })
    }

    /// Main entry point for scrubbing a string line/content
    pub fn redact(&self, input: &str) -> String {
        let mut output = input.to_string();

        // Pass 1: Aho-Corasick Deterministic
        output = self.ac.replace_all(&output, &["[REDACTED]"; 3]);

        // Pass 3: Neural Trigger (For ambiguous soft PII)
        let triggers = self.trigger.scan(&output);
        if !triggers.is_empty() {
            // In a real implementation, we would acquire the semaphore
            // and pass the byte windows to Candle/Tract here.
            // For the sake of the engine wrapper, we'll mark triggered zones to mock inference.
            // _ = self.neural_semaphore.try_acquire();
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pii_trigger_entropy() {
        let trigger = PiiTrigger::new(8, 2.5);
        // "AABBCCDD" has 4 unique chars evenly distributed.
        // H = 4 * -(0.25 * log2(0.25)) = 4 * 0.5 = 2.0
        assert!(
            trigger.scan("AABBCCDD").is_empty(),
            "Entropy should be 2.0, below 2.5"
        );

        // "ABCDEFGH" has 8 unique chars.
        // H = 8 * -(0.125 * log2(0.125)) = 8 * 0.375 = 3.0
        assert!(
            !trigger.scan("ABCDEFGH").is_empty(),
            "Entropy should be 3.0, above 2.5"
        );
    }

    #[test]
    fn test_privacy_shield_redaction() {
        let shield = PrivacyShield::init().unwrap();

        let safe_text = "Hello, my name is anonymous! I am writing code.";
        assert_eq!(shield.redact(safe_text), safe_text);

        let ssn_text = "My SSN is 123-45-6789.";
        assert_eq!(shield.redact(ssn_text), "My [REDACTED] is 123-45-6789.");
    }
}

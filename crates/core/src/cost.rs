// A static lookup table of model input-token costs per 1,000,000 tokens (USD).
// Prices are approximate list prices and will drift; update as vendors change them.
pub const MODELS: &[(&str, f64)] = &[
    ("Claude Sonnet 4.5", 3.00),
    ("Claude Haiku 4.5", 1.00),
    ("Claude Opus 4.1", 15.00),
    ("GPT-5", 1.25),
    ("GPT-5 mini", 0.25),
    ("Gemini 2.5 Pro", 1.25),
    ("Gemini 2.5 Flash", 0.30),
];

pub fn estimate_costs(tokens: usize) -> Vec<(String, f64)> {
    let multiplier = (tokens as f64) / 1_000_000.0;
    MODELS
        .iter()
        .map(|(name, cost_per_m)| (name.to_string(), cost_per_m * multiplier))
        .collect()
}

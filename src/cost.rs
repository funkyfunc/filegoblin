// A static lookup table of model costs per 1,000,000 input tokens (USD)
pub const MODELS: &[(&str, f64)] = &[
    ("GPT-4o", 5.00),
    ("GPT-4o-mini", 0.15),
    ("Claude 3.5 Sonnet", 3.00),
    ("Claude 3 Haiku", 0.25),
    ("Gemini 1.5 Pro", 3.50),
    ("Gemini 1.5 Flash", 0.075),
];

pub fn estimate_costs(tokens: usize) -> Vec<(String, f64)> {
    let multiplier = (tokens as f64) / 1_000_000.0;
    MODELS
        .iter()
        .map(|(name, cost_per_m)| (name.to_string(), cost_per_m * multiplier))
        .collect()
}

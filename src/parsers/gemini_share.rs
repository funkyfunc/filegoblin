use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use serde_json::Value;

pub struct GeminiGobbler;

impl GeminiGobbler {
    fn extract_share_id(&self, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        let segments = parsed.path_segments()?;
        for segment in segments {
            if segment == "share" {
                return parsed
                    .path_segments()?
                    .nth(parsed.path_segments()?.position(|s| s == "share")? + 1)
                    .map(|s| s.to_string());
            }
        }
        let url_str = url.to_string();
        if let Some(id_start) = url_str.find("/share/") {
            let after = &url_str[id_start + 7..];
            if let Some(slash_idx) = after.find("/") {
                return Some(after[..slash_idx].to_string());
            }
            return Some(after.to_string());
        }
        None
    }
}

impl Gobble for GeminiGobbler {
    fn gobble(&self, path: &std::path::Path, flags: &crate::cli::Cli) -> Result<String> {
        let content = std::fs::read_to_string(path)?;
        self.gobble_str(&content, flags)
    }

    fn gobble_str(&self, url: &str, _flags: &crate::cli::Cli) -> Result<String> {
        let share_id = self
            .extract_share_id(url)
            .context("Could not extract Gemini Share ID from URL")?;

        let client = reqwest::blocking::Client::new();

        eprintln!("✨ Fetching initial Gemini payload...");
        let base_res = client
            .get(url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
            )
            .send()
            .context("Failed to fetch initial Gemini page")?;

        if !base_res.status().is_success() {
            anyhow::bail!(
                "Gemini returned status {} on initial fetch.",
                base_res.status()
            );
        }

        let html = base_res.text()?;

        // Extract required params from standard Google `WIZ_global_data` format within the HTML script tags.
        let f_sid = extract_wiz_value(&html, "FdrFJe").unwrap_or_else(|| "-123456789".to_string()); // Session ID
        let bl = extract_wiz_value(&html, "cfb2h")
            .context("Could not extract build label `bl` from WIZ_global_data")?; // Build label

        eprintln!("🚀 Executing batchexecute RPC (ujx1Bf)...");

        let req_param = format!(
            r#"[[["ujx1Bf","[null,\"{}\",[4]]",null,"generic"]]]"#,
            share_id
        );
        let post_url = format!(
            "https://gemini.google.com/_/BardChatUi/data/batchexecute?rpcids=ujx1Bf&source-path=/share/{}&f.sid={}&bl={}&hl=en-US&rt=c",
            share_id, f_sid, bl
        );

        let rpc_res = client
            .post(&post_url)
            .header(
                "Content-Type",
                "application/x-www-form-urlencoded;charset=utf-8",
            )
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
            )
            .body(format!("f.req={}", urlencoding::encode(&req_param)))
            .send()
            .context("Failed to execute ujx1Bf batchexecute RPC")?;

        if !rpc_res.status().is_success() {
            anyhow::bail!("Gemini RPC returned status {}", rpc_res.status());
        }

        let rpc_text = rpc_res.text()?;

        // The batchexecute response format is `)]}'` followed by lines, each being a length and then a JSON string
        let mut builder = format!("# Gemini Conversation Share\nSource URL: {}\n\n", url);

        let cleaned = rpc_text.trim_start_matches(")]}'\n");
        let parsed_chunks = parse_batchexecute_chunks(cleaned);

        if parsed_chunks.is_empty() {
            return Ok(builder + "Failed to parse conversation chunks, or empty answer.");
        }

        for chunk in &parsed_chunks {
            if let Some(inner_arr) = chunk[0][2].as_str()
                && let Ok(Value::Array(conv_data)) = serde_json::from_str(inner_arr)
            {
                if let Some(Value::Array(turns)) = conv_data.first() {
                    for turn in turns {
                        if let Some(Value::Array(items)) = turn.get(0) {
                            if let Some(Value::Array(user_parts)) = items.first()
                                && let Some(Value::Array(user_content)) = user_parts.first()
                                && let Some(text) = user_content.first().and_then(|v| v.as_str())
                            {
                                builder.push_str(&format!("**User:**\n{}\n\n", text));
                            }
                            if let Some(Value::Array(model_parts)) = items.get(1)
                                && let Some(Value::Array(model_content)) = model_parts.first()
                                && let Some(text) = model_content.first().and_then(|v| v.as_str())
                            {
                                builder.push_str(&format!("**Gemini:**\n{}\n\n", text));
                            }
                        } else {
                            // Simplified flat parser for varying proto structs
                            if let Some(user_q) = turn.get(2).and_then(|v| v.as_str()) {
                                builder.push_str(&format!("**User:**\n{}\n\n", user_q));
                            } else if let Some(Value::Array(sub_items)) = turn.get(1)
                                && let Some(text) = sub_items.first().and_then(|v| v.as_str())
                            {
                                builder.push_str(&format!("**Message:**\n{}\n\n", text));
                            }
                        }
                    }
                } else if let Some(title) = conv_data.get(2).and_then(|v| v.as_str()) {
                    builder = format!(
                        "# Gemini Conversation: {}\nSource URL: {}\n\n{}",
                        title, url, builder
                    );
                }
            }
        }

        // Just in case parsing totally failed to match the proto layout, but data exists
        if builder.matches("**").count() == 0 {
            builder.push_str("\n\n<!-- Raw parsed JSON data fell through strict parsing. Using heuristic extraction. -->\n\n");
            for chunk in &parsed_chunks {
                builder.push_str(&extract_text_recursive(chunk));
            }
        }

        Ok(builder)
    }
}

// Helper for heuristic extraction when strict proto layout parsing fails
fn extract_text_recursive(val: &Value) -> String {
    let mut result = String::new();
    match val {
        Value::String(s) => {
            let s_trimmed = s.trim();
            if (s_trimmed.starts_with('[') || s_trimmed.starts_with('{'))
                && let Ok(inner_val) = serde_json::from_str::<Value>(s)
            {
                result.push_str(&extract_text_recursive(&inner_val));
            } else if s.len() > 30
                && !s.starts_with("rc_")
                && !s.starts_with("c_")
                && !s.starts_with("r_")
                && !s.starts_with("http")
                && !s.starts_with("d7a5")
                && !s.starts_with("d108")
            {
                // Clean up any remaining escaped characters or minor garbage if needed,
                // but generally pure text strings should be clean now.
                result.push_str(s);
                result.push_str("\n\n");
            }
        }
        Value::Array(arr) => {
            for item in arr {
                result.push_str(&extract_text_recursive(item));
            }
        }
        Value::Object(map) => {
            for (_, v) in map {
                result.push_str(&extract_text_recursive(v));
            }
        }
        _ => {}
    }
    result
}

// Helper to extract values from `WIZ_global_data` in Google pages
fn extract_wiz_value(html: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\":\"", key);
    let start_idx = html.find(&search)? + search.len();
    let after = &html[start_idx..];
    let end_idx = after.find("\"")?;
    Some(after[..end_idx].to_string())
}

// Parses Google's nested chunk format used by `batchexecute`
fn parse_batchexecute_chunks(payload: &str) -> Vec<Value> {
    let mut chunks = Vec::new();
    let lines: Vec<&str> = payload.lines().collect();

    // Pattern: 1 line with length, next line is JSON array string
    let mut i = 0;
    while i < lines.len() {
        if lines[i].parse::<usize>().is_ok() && i + 1 < lines.len() {
            if let Ok(json) = serde_json::from_str::<Value>(lines[i + 1])
                && let Value::Array(arr) = json
            {
                chunks.push(Value::Array(arr));
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    chunks
}

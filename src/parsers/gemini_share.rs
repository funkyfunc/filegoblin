use crate::parsers::gobble::Gobble;
use crate::parsers::credentials::load_credentials;
use anyhow::{Context, Result};
use serde_json::Value;

pub struct GeminiGobbler;

impl GeminiGobbler {
    fn extract_share_id(&self, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        let mut segments = parsed.path_segments()?;
        for segment in segments {
            if segment == "share" {
                return parsed.path_segments()?.nth(parsed.path_segments()?.position(|s| s == "share")? + 1).map(|s| s.to_string());
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
        let share_id = self.extract_share_id(url).context("Could not extract Gemini Share ID from URL")?;
        
        let creds = load_credentials().context("No credentials found. You need a valid Google session. Run `filegoblin --google-login`")?;
        let cookie_1psid = creds.google_cookie_1psid.context("No Google __Secure-1PSID cookie found. Run `filegoblin --google-login` and paste your cookie when prompted.")?;
        
        let client = reqwest::blocking::Client::new();
        
        eprintln!("✨ Fetching initial Gemini payload to extract XSRF tokens...");
        let base_res = client.get(url)
             .header("Cookie", format!("__Secure-1PSID={};", cookie_1psid))
             .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
             .send()
             .context("Failed to fetch initial Gemini page")?;

        if !base_res.status().is_success() {
             anyhow::bail!("Gemini returned status {} on initial fetch.", base_res.status());
        }

        let html = base_res.text()?;
        
        // Extract required params from standard Google `WIZ_global_data` format within the HTML script tags.
        let f_sid = extract_wiz_value(&html, "FdrFJe").unwrap_or_else(|| "-123456789".to_string()); // Session ID
        let bl = extract_wiz_value(&html, "cfb2h").context("Could not extract build label `bl` from WIZ_global_data")?; // Build label
        let at = extract_wiz_value(&html, "SNlM0d").context("Could not extract XSRF token `at` / `SNlM0d`. The cookie might be expired or invalid.")?;

        eprintln!("🚀 Executing batchexecute RPC (ujx1Bf)...");
        
        let req_param = format!(r#"[[["ujx1Bf","[[\"{}\"]]",null,"generic"]]]"#, share_id);
        let post_url = format!("https://gemini.google.com/_/BardChatUi/data/batchexecute?rpcids=ujx1Bf&source-path=/share/{}&f.sid={}&bl={}", share_id, f_sid, bl);

        let rpc_res = client.post(&post_url)
             .header("Cookie", format!("__Secure-1PSID={};", cookie_1psid))
             .header("Content-Type", "application/x-www-form-urlencoded;charset=utf-8")
             .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
             .body(format!("f.req={}&at={}", urlencoding::encode(&req_param), at))
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

        for chunk in parsed_chunks {
            if let Some(inner_arr) = chunk[0][2].as_str() {
                if let Ok(Value::Array(conv_data)) = serde_json::from_str(inner_arr) {
                    if let Some(Value::Array(turns)) = conv_data.get(0) {
                         for turn in turns {
                              if let Some(Value::Array(items)) = turn.get(0) {
                                   if let Some(Value::Array(user_parts)) = items.get(0) {
                                        if let Some(Value::Array(user_content)) = user_parts.get(0) {
                                             if let Some(text) = user_content.get(0).and_then(|v| v.as_str()) {
                                                  builder.push_str(&format!("**User:**\n{}\n\n", text));
                                             }
                                        }
                                   }
                                   if let Some(Value::Array(model_parts)) = items.get(1) {
                                        if let Some(Value::Array(model_content)) = model_parts.get(0) {
                                             if let Some(text) = model_content.get(0).and_then(|v| v.as_str()) {
                                                  builder.push_str(&format!("**Gemini:**\n{}\n\n", text));
                                             }
                                        }
                                   }
                              } else {
                                  // Simplified flat parser for varying proto structs
                                  if let Some(user_q) = turn.get(2).and_then(|v| v.as_str()) {
                                       builder.push_str(&format!("**User:**\n{}\n\n", user_q));
                                  } else if let Some(Value::Array(sub_items)) = turn.get(1) {
                                       if let Some(text) = sub_items.get(0).and_then(|v| v.as_str()) {
                                            builder.push_str(&format!("**Message:**\n{}\n\n", text));
                                       }
                                  }
                              }
                         }
                    } else if let Some(title) = conv_data.get(2).and_then(|v| v.as_str()) {
                         builder = format!("# Gemini Conversation: {}\nSource URL: {}\n\n{}", title, url, builder);
                    }
                }
            }
        }

        // Just in case parsing totally failed to match the proto layout, but data exists
        if builder.matches("**").count() == 0 {
             builder.push_str("\n\n<!-- Raw parsed JSON data fell through strict parsing -->\n");
             builder.push_str("Please open an issue to update the proto-extractor layout for this specific link format.");
        }

        Ok(builder)
    }
}

// Helper to extract values from `WIZ_global_data` in Google pages
fn extract_wiz_value(html: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\",\"", key);
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
         if lines[i].parse::<usize>().is_ok() {
              if i + 1 < lines.len() {
                   if let Ok(json) = serde_json::from_str::<Value>(lines[i+1]) {
                        if let Value::Array(arr) = json {
                             chunks.push(Value::Array(arr));
                        }
                   }
                   i += 2;
                   continue;
              }
         }
         i += 1;
    }
    chunks
}

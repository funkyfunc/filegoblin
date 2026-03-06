use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;
use sha2::{Sha256, Digest};
use base64::{Engine as _, engine::general_purpose};
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::{json, Value};
use urlencoding::encode;

const BEARER_TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs=1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";

pub struct TwitterGobbler {
    pub flavor: crate::flavors::Flavor,
}

impl TwitterGobbler {
    fn extract_tweet_id(&self, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        let mut segments = parsed.path_segments()?;
        // Assuming format /username/status/123456789
        segments.next()?;
        if segments.next()? == "status" {
            Some(segments.next()?.to_string())
        } else {
            None
        }
    }

    fn generate_transaction_id(&self, path: &str) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // A simplified hash to bypass trivial timing verification
        let payload = format!("{}-{}", timestamp, path);
        let mut hasher = Sha256::new();
        hasher.update(payload.as_bytes());
        let result = hasher.finalize();

        general_purpose::STANDARD.encode(result)
    }

    fn fetch_guest_token(
        &self,
        client: &reqwest::blocking::Client,
    ) -> Result<String> {
        let res = client
            .post("https://api.twitter.com/1.1/guest/activate.json")
            .header("Authorization", format!("Bearer {}", BEARER_TOKEN))
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .header("Accept-Language", "en-US,en;q=0.5")
            .send()
            .context("Failed to request guest token")?;

        let json: Value = res.json().context("Failed to parse activation JSON")?;

        json["guest_token"]
            .as_str()
            .map(|s| s.to_string())
            .context("No guest_token returned")
    }

    fn try_graphql_extraction(
        &self,
        tweet_id: &str,
    ) -> Result<Value> {
        let client = reqwest::blocking::ClientBuilder::new()
            .cookie_store(true)
            .use_rustls_tls()
            .build()
            .context("Failed to initialize highly concurrent HTTP client")?;

        let guest_token = self.fetch_guest_token(&client)?;

        // Fetch dummy init to get ct0 CSRF cookie
        let res = client
            .get("https://twitter.com/i/api/1.1/account/settings.json")
            .header("Authorization", format!("Bearer {}", BEARER_TOKEN))
            .header("x-guest-token", &guest_token)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .send()?;

        // Extract ct0
        let mut ct0_token = "missing".to_string();
        for cookie_str in res.headers().get_all(reqwest::header::SET_COOKIE) {
            if let Ok(c_str) = cookie_str.to_str() {
                for part in c_str.split(';') {
                    let p = part.trim();
                    if p.starts_with("ct0=") {
                        ct0_token = p.trim_start_matches("ct0=").to_string();
                    }
                }
            }
        }

        // We use a known working query ID, or fallback
        let query_id = "miKSMGb2R1SewIJv2-ablQ";
        let path = format!("/i/api/graphql/{}/TweetDetail", query_id);

        let variables = json!({
            "focalTweetId": tweet_id,
            "with_rux_injections": false,
            "includePromotedContent": false,
            "withCommunity": true,
            "withQuickPromoteEligibilityTweetFields": false,
            "withBirdwatchNotes": true,
            "withVoice": true,
            "withV2Timeline": true
        });

        // Bare minimum features
        let features = json!({
            "responsive_web_graphql_exclude_directive_enabled": true,
            "verified_phone_label_enabled": false,
            "responsive_web_graphql_timeline_navigation_enabled": true,
            "responsive_web_graphql_skip_user_profile_image_extensions_enabled": false,
            "tweetypie_unmention_optimization_enabled": true,
            "vibe_api_enabled": true,
            "responsive_web_edit_tweet_api_enabled": true,
            "graphql_is_translatable_rweb_tweet_is_translatable_enabled": true,
            "view_counts_everywhere_api_enabled": true,
            "longform_notetweets_consumption_enabled": true,
            "tweet_awards_web_tipping_enabled": false,
            "freedom_of_speech_not_reach_fetch_enabled": true,
            "standardized_nudges_misinfo": true,
            "tweet_with_visibility_results_prefer_gql_limited_actions_policy_enabled": false,
            "interactive_text_enabled": true,
            "responsive_web_text_conversations_enabled": false,
            "responsive_web_enhance_cards_enabled": false
        });

        // The exact URL to query
        let url = format!(
            "https://twitter.com{}?variables={}&features={}",
            path,
            encode(&serde_json::to_string(&variables).unwrap()),
            encode(&serde_json::to_string(&features).unwrap())
        );

        let res = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", BEARER_TOKEN))
            .header("x-guest-token", guest_token)
            .header("x-csrf-token", ct0_token)
            .header("x-client-transaction-id", self.generate_transaction_id(&path))
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .send()
            .context("Failed to execute GraphQL query")?;

        if !res.status().is_success() {
            anyhow::bail!("GraphQL request failed: {}", res.status());
        }

        let body: Value = res.json()?;
        Ok(body)
    }

    fn fetch_syndication(&self, tweet_id: &str) -> Result<Value> {
        let url = format!(
            "https://cdn.syndication.twimg.com/tweet-result?id={}&token=!",
            tweet_id
        );
        let client = reqwest::blocking::ClientBuilder::new()
            .cookie_store(true)
            .use_rustls_tls()
            .build()
            .context("Failed to initialize Syndication HTTP client")?;
        let res = client.get(&url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .send()?;
        if !res.status().is_success() {
            anyhow::bail!("Syndication API failed with status {}", res.status());
        }
        let body: Value = res.json()?;
        Ok(body)
    }

    pub fn get_thread_nodes(&self, url: &str) -> Result<Vec<(String, String)>> {
        let tweet_id = self
            .extract_tweet_id(url)
            .context("Invalid Twitter URL format. Could not extract tweet ID.")?;

        match self.try_graphql_extraction(&tweet_id) {
            Ok(json_ast) => {
                let instructions = json_ast["data"]["threaded_conversation_with_injections_v2"]["instructions"].as_array();
                let mut nodes = Vec::new();
                if let Some(insts) = instructions {
                    for inst in insts {
                        if inst["type"] == "TimelineAddEntries" {
                            if let Some(entries) = inst["entries"].as_array() {
                                for entry in entries {
                                    if let Some(entry_id) = entry["entryId"].as_str() {
                                        if entry_id.starts_with("tweet-") {
                                            let text = entry["itemContent"]["tweet_results"]["result"]["legacy"]["full_text"].as_str().unwrap_or("").to_string();
                                            let tid = entry["itemContent"]["tweet_results"]["result"]["legacy"]["id_str"].as_str().unwrap_or("").to_string();
                                            if !tid.is_empty() {
                                                nodes.push((format!("https://x.com/i/status/{}", tid), text));
                                            }
                                        } else if entry_id.starts_with("conversationthread-") {
                                            if let Some(items) = entry["content"]["items"].as_array() {
                                                for item in items {
                                                    let node = &item["item"];
                                                    let text = node["itemContent"]["tweet_results"]["result"]["legacy"]["full_text"].as_str().unwrap_or("").to_string();
                                                    let tid = node["itemContent"]["tweet_results"]["result"]["legacy"]["id_str"].as_str().unwrap_or("").to_string();
                                                    if !tid.is_empty() {
                                                        nodes.push((format!("https://x.com/i/status/{}", tid), text));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(nodes)
            }
            Err(_) => {
                // Fallback to syndication
                let syn_ast = self.fetch_syndication(&tweet_id)?;
                let text = syn_ast["text"].as_str().unwrap_or("").to_string();
                let tid = syn_ast["id_str"].as_str().unwrap_or("").to_string();
                Ok(vec![(format!("https://x.com/i/status/{}", tid), text)])
            }
        }
    }

    fn parse_graphql_tree(&self, url: &str, root: &Value, focal_id: &str) -> String {
        let mut builder = String::new();

        let instructions = match root["data"]["threaded_conversation_with_injections_v2"]["instructions"].as_array() {
            Some(i) => i,
            None => return "Could not locate threading instructions in AST.\n".to_string(),
        };

        let mut tweet_nodes = Vec::new();
        for instruction in instructions {
            if instruction["type"] == "TimelineAddEntries" {
                if let Some(entries) = instruction["entries"].as_array() {
                    for entry in entries {
                        if let Some(entry_id) = entry["entryId"].as_str() {
                            if entry_id.starts_with("tweet-") {
                                tweet_nodes.push(entry);
                            } else if entry_id.starts_with("conversationthread-") {
                                if let Some(items) = entry["content"]["items"].as_array() {
                                    for item in items {
                                        tweet_nodes.push(&item["item"]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut focal_author = String::new();
        let mut focal_node_opt = None;
        let mut thread_nodes = Vec::new();

        for node in tweet_nodes {
            let legacy = &node["itemContent"]["tweet_results"]["result"]["legacy"];
            if legacy.is_null() { continue; }
            let id = legacy["id_str"].as_str().unwrap_or("");
            if id == focal_id {
                focal_node_opt = Some(node);
                let user_legacy = &node["itemContent"]["tweet_results"]["result"]["core"]["user_results"]["result"]["legacy"];
                focal_author = user_legacy["screen_name"].as_str().unwrap_or("unknown").to_string();
            } else {
                thread_nodes.push(node);
            }
        }

        match self.flavor {
            crate::flavors::Flavor::Anthropic => {
                builder.push_str(&format!("<twitter_thread source_url=\"{}\">\n", url));
                if let Some(focal_node) = focal_node_opt {
                    builder.push_str(&self.format_tweet_xml(focal_node, "focal_tweet", &focal_author, true));
                }
                if !thread_nodes.is_empty() {
                    builder.push_str("  <thread_context>\n");
                    for node in thread_nodes {
                        builder.push_str(&self.format_tweet_xml(node, "tweet", &focal_author, false));
                    }
                    builder.push_str("  </thread_context>\n");
                }
                builder.push_str("</twitter_thread>\n");
            },
            _ => {
                builder.push_str(&format!("Twitter Thread by @{}\nThread URL: {}\n\n", focal_author, url));
                let mut index = 1;
                if let Some(focal_node) = focal_node_opt {
                    builder.push_str(&self.format_tweet_md(focal_node, index));
                    index += 1;
                }
                for node in thread_nodes {
                    builder.push_str(&self.format_tweet_md(node, index));
                    index += 1;
                }
            }
        }
        builder
    }

    fn extract_media(&self, legacy: &Value) -> String {
        let mut media_block = String::new();
        if let Some(media_arr) = legacy["entities"]["media"].as_array() {
            for m in media_arr {
                if let Some(url) = m["media_url_https"].as_str() {
                    let url_orig = url.replace("?format=jpg&name=medium", "?format=jpg&name=orig")
                                      .replace("?format=png&name=medium", "?format=png&name=orig");
                    media_block.push_str(&format!("\nMedia Attached:\n*({})\n", url_orig));
                }
            }
        }
        media_block
    }

    fn extract_media_xml(&self, legacy: &Value) -> String {
        let mut media_block = String::new();
        if let Some(media_arr) = legacy["entities"]["media"].as_array() {
            media_block.push_str("      <media>\n");
            for m in media_arr {
                if let Some(url) = m["media_url_https"].as_str() {
                    let url_orig = url.replace("?format=jpg&name=medium", "?format=jpg&name=orig")
                                      .replace("?format=png&name=medium", "?format=png&name=orig");
                    media_block.push_str(&format!("        <image url=\"{}\"></image>\n", url_orig));
                }
            }
            media_block.push_str("      </media>\n");
        }
        media_block
    }

    fn format_tweet_xml(&self, node: &Value, tag: &str, focal_author: &str, is_focal: bool) -> String {
        let mut res = String::new();
        let legacy = &node["itemContent"]["tweet_results"]["result"]["legacy"];
        let id = legacy["id_str"].as_str().unwrap_or("");
        let parent_id = legacy["in_reply_to_status_id_str"].as_str().unwrap_or("");
        let text = legacy["full_text"].as_str().unwrap_or("...");
        
        let user_legacy = &node["itemContent"]["tweet_results"]["result"]["core"]["user_results"]["result"]["legacy"];
        let handle = user_legacy["screen_name"].as_str().unwrap_or("unknown");
        let name = user_legacy["name"].as_str().unwrap_or("unknown");
        let created_at = legacy["created_at"].as_str().unwrap_or("unknown");
        
        let likes = legacy["favorite_count"].as_i64().unwrap_or(0);
        let rts = legacy["retweet_count"].as_i64().unwrap_or(0);
        let replies = legacy["reply_count"].as_i64().unwrap_or(0);

        let media_block = self.extract_media_xml(legacy);
        let relationship = if handle == focal_author { "self_reply" } else { "external_reply" };

        let padding = if is_focal { "  " } else { "    " };

        if is_focal {
            res.push_str(&format!("{}<{} id=\"{}\">\n", padding, tag, id));
        } else {
            res.push_str(&format!("{}<{} id=\"{}\" relationship=\"{}\" parent_id=\"{}\">\n", padding, tag, id, relationship, parent_id));
        }

        res.push_str(&format!("{}  <metadata>\n", padding));
        res.push_str(&format!("{}    <author username=\"@{}\" display_name=\"{}\" />\n", padding, handle, name));
        res.push_str(&format!("{}    <timestamp>{}</timestamp>\n", padding, created_at));
        res.push_str(&format!("{}    <metrics likes=\"{}\" retweets=\"{}\" replies=\"{}\" />\n", padding, likes, rts, replies));
        res.push_str(&format!("{}  </metadata>\n", padding));
        res.push_str(&format!("{}  <content>\n", padding));
        res.push_str(&format!("{}    <text>{}</text>\n", padding, text.replace("\n", &format!("\n{}    ", padding))));
        if !media_block.is_empty() {
            let padded_media = media_block.replace("\n", &format!("\n{}", padding));
            res.push_str(&padded_media);
        }
        res.push_str(&format!("{}  </content>\n", padding));
        res.push_str(&format!("{}</{}>\n", padding, tag));
        
        res
    }

    fn format_tweet_md(&self, node: &Value, index: usize) -> String {
        let mut res = String::new();
        let legacy = &node["itemContent"]["tweet_results"]["result"]["legacy"];
        let text = legacy["full_text"].as_str().unwrap_or("...");
        let user_legacy = &node["itemContent"]["tweet_results"]["result"]["core"]["user_results"]["result"]["legacy"];
        let handle = user_legacy["screen_name"].as_str().unwrap_or("unknown");
        let name = user_legacy["name"].as_str().unwrap_or("unknown");
        let created_at = legacy["created_at"].as_str().unwrap_or("unknown");
        
        let likes = legacy["favorite_count"].as_i64().unwrap_or(0);
        let rts = legacy["retweet_count"].as_i64().unwrap_or(0);
        let replies = legacy["reply_count"].as_i64().unwrap_or(0);

        let media_block = self.extract_media(legacy);

        res.push_str(&format!("{}. @{} ({})\n", index, handle, name));
        res.push_str(&format!("Posted at: {} | Likes: {} | Retweets: {} | Replies: {}\n", created_at, likes, rts, replies));
        
        let parent_id = legacy["in_reply_to_status_id_str"].as_str().unwrap_or("");
        if !parent_id.is_empty() {
             let parent_author = legacy["in_reply_to_screen_name"].as_str().unwrap_or("unknown");
             res.push_str(&format!("Replying to @{}\n", parent_author));
        }

        res.push_str(&format!("> {}\n", text.replace("\n", "\n> ")));
        if !media_block.is_empty() {
            res.push_str(&media_block);
        }
        res.push_str("---\n\n");
        res
    }

    fn parse_syndication(&self, url: &str, root: &Value) -> String {
        let mut builder = String::new();

        let id = root["id_str"].as_str().unwrap_or("");
        let text = root["text"].as_str().unwrap_or("...");
        let created_at = root["created_at"].as_str().unwrap_or("unknown");
        let name = root["user"]["name"].as_str().unwrap_or("unknown");
        let handle = root["user"]["screen_name"].as_str().unwrap_or("unknown");
        let likes = root["favorite_count"].as_i64().unwrap_or(0);
        
        let mut media_block_md = String::new();
        let mut media_block_xml = String::new();

        if let Some(media_arr) = root["mediaDetails"].as_array() {
            media_block_xml.push_str("      <media>\n");
            for m in media_arr {
                if let Some(media_url) = m["media_url_https"].as_str() {
                    let url_orig = media_url.replace("?format=jpg&name=medium", "?format=jpg&name=orig");
                    media_block_md.push_str(&format!("\nMedia Attached:\n*({})\n", url_orig));
                    media_block_xml.push_str(&format!("        <image url=\"{}\"></image>\n", url_orig));
                }
            }
            media_block_xml.push_str("      </media>\n");
        }

        match self.flavor {
            crate::flavors::Flavor::Anthropic => {
                 builder.push_str(&format!("<twitter_thread source_url=\"{}\">\n", url));
                 builder.push_str(&format!("  <focal_tweet id=\"{}\">\n", id));
                 builder.push_str(&format!("    <metadata>\n"));
                 builder.push_str(&format!("      <author username=\"@{}\" display_name=\"{}\" />\n", handle, name));
                 builder.push_str(&format!("      <timestamp>{}</timestamp>\n", created_at));
                 builder.push_str(&format!("      <metrics likes=\"{}\" retweets=\"0\" replies=\"0\" />\n", likes));
                 builder.push_str(&format!("    </metadata>\n"));
                 builder.push_str(&format!("    <content>\n"));
                 builder.push_str(&format!("      <text>{}</text>\n", text.replace("\n", "\n      ")));
                 if !media_block_xml.is_empty() {
                      builder.push_str(&media_block_xml);
                 }
                 builder.push_str(&format!("    </content>\n"));
                 builder.push_str(&format!("  </focal_tweet>\n"));
                 builder.push_str(&format!("</twitter_thread>\n"));
            },
            _ => {
                 builder.push_str(&format!("Twitter Thread by @{}\nThread URL: {}\n\n", handle, url));
                 builder.push_str(&format!("1. @{} ({})\n", handle, name));
                 builder.push_str(&format!("Posted at: {} | Likes: {}\n", created_at, likes));
                 builder.push_str(&format!("> {}\n", text.replace("\n", "\n> ")));
                 if !media_block_md.is_empty() {
                      builder.push_str(&media_block_md);
                 }
                 builder.push_str("---\n\n");
            }
        }
        
        builder
    }
}

impl Gobble for TwitterGobbler {
    fn gobble(&self, path: &Path) -> Result<String> {
        let content = std::fs::read_to_string(path)?;
        self.gobble_str(&content)
    }

    fn gobble_str(&self, url: &str) -> Result<String> {
        let tweet_id = self
            .extract_tweet_id(url)
            .context("Invalid Twitter URL format. Could not extract tweet ID.")?;

        eprintln!("🐦 Extracted Tweet ID: {}", tweet_id);
        eprintln!("🕵️ Attempting Primary GraphQL Extraction Flow...");

        match self.try_graphql_extraction(&tweet_id) {
            Ok(json_ast) => {
                eprintln!("✅ GraphQL extraction succeeded! Parsing AST...");
                let parsed = self.parse_graphql_tree(url, &json_ast, &tweet_id);
                // Return exactly what was parsed.
                Ok(parsed)
            }
            Err(e) => {
                eprintln!("⚠️ GraphQL failed ({}). Cascading to Syndication API...", e);
                let syn_ast = self.fetch_syndication(&tweet_id).context(
                    "Both Primary GraphQL and Fallback Syndication APIs failed. Twitter has rate limited this agent.",
                )?;
                eprintln!("✅ Syndication API extraction succeeded!");
                let parsed = self.parse_syndication(url, &syn_ast);
                Ok(parsed)
            }
        }
    }
}

use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use sha2::{Sha256, Digest};
use base64::{Engine as _, engine::general_purpose};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use serde_json::{json, Value};
use urlencoding::encode;
use serde::{Deserialize, Serialize};

// oauth2 imports
use oauth2::{
    basic::BasicClient, reqwest::http_client, AuthUrl, AuthorizationCode, ClientId,
    CsrfToken, PkceCodeChallenge, RedirectUrl, TokenResponse, TokenUrl, Scope, RefreshToken
};
use tiny_http::{Server, Response};

const BEARER_TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs=1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";

/// Local storage format for ~/.config/filegoblin/credentials.json
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct LocalCredentials {
    pub github_token: Option<String>,
    pub twitter_access_token: Option<String>,
    pub twitter_refresh_token: Option<String>,
    pub twitter_token_expires_at: Option<u64>,
}

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

    fn try_oauth2_extraction(&self, tweet_id: &str, access_token: &str) -> Result<Value> {
        let client = reqwest::blocking::Client::new();
        
        let url = format!(
            "https://api.twitter.com/2/tweets/{}?tweet.fields=created_at,author_id,conversation_id,public_metrics,in_reply_to_user_id&expansions=author_id,attachments.media_keys,in_reply_to_user_id,referenced_tweets.id&media.fields=media_key,type,url,preview_image_url&user.fields=name,username",
            tweet_id
        );
        let focal_res = client.get(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .context("Failed to fetch focal tweet via OAuth2")?;

        if !focal_res.status().is_success() {
             anyhow::bail!("Focal tweet OAuth2 request failed: {}", focal_res.status());
        }

        let focal_json: Value = focal_res.json()?;
        
        let conversation_id = focal_json["data"]["conversation_id"].as_str().unwrap_or(tweet_id).to_string();
        let author_username = focal_json["includes"]["users"][0]["username"].as_str().unwrap_or("").to_string();

        let mut thread_url = "https://api.twitter.com/2/tweets/search/recent?".to_string();
        thread_url.push_str(&format!("query=conversation_id:{} from:{}", conversation_id, author_username));
        thread_url.push_str("&tweet.fields=created_at,author_id,conversation_id,public_metrics,in_reply_to_user_id");
        thread_url.push_str("&expansions=author_id,attachments.media_keys,in_reply_to_user_id,referenced_tweets.id");
        thread_url.push_str("&media.fields=media_key,type,url,preview_image_url");
        thread_url.push_str("&user.fields=name,username");
        thread_url.push_str("&max_results=100");

        let thread_res = client.get(&thread_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .context("Failed to fetch thread via OAuth2")?;

        if !thread_res.status().is_success() {
             anyhow::bail!("Thread search OAuth2 request failed: {}", thread_res.status());
        }

        let thread_json: Value = thread_res.json()?;

        // To make parsing simple, we reshape the v2 API response into a format that `parse_syndication` or a slightly modified AST parser can handle
        // Wait, instead of reshaping, let's just create a custom parse function for the standard OAuth 2.0 response format.
        // For simplicity we return a combined JSON blob that `gobble_str` will pass to a new `parse_oauth_tree`.
        Ok(json!({
            "focal": focal_json,
            "thread": thread_json
        }))
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
    fn parse_oauth_tree(&self, url: &str, root: &Value) -> String {
        let mut builder = String::new();
        
        // The root contains {"focal": {...}, "thread": {...}}
        let focal_json = &root["focal"];
        let thread_json = &root["thread"];
        
        let focal_id = focal_json["data"]["id"].as_str().unwrap_or("");
        let focal_text = focal_json["data"]["text"].as_str().unwrap_or("...");
        let focal_author_id = focal_json["data"]["author_id"].as_str().unwrap_or("");
        
        let mut username = "unknown".to_string();
        let mut display_name = "unknown".to_string();
        if let Some(users) = focal_json["includes"]["users"].as_array() {
             for u in users {
                  if u["id"].as_str() == Some(focal_author_id) {
                       username = u["username"].as_str().unwrap_or("unknown").to_string();
                       display_name = u["name"].as_str().unwrap_or("unknown").to_string();
                       break;
                  }
             }
        }

        let focal_timestamp = focal_json["data"]["created_at"].as_str().unwrap_or("unknown");
        let likes = focal_json["data"]["public_metrics"]["like_count"].as_i64().unwrap_or(0);

        match self.flavor {
            crate::flavors::Flavor::Anthropic => {
                 builder.push_str(&format!("<twitter_thread source_url=\"{}\">\n", url));
                 builder.push_str(&format!("  <focal_tweet id=\"{}\">\n", focal_id));
                 builder.push_str(&format!("    <metadata>\n"));
                 builder.push_str(&format!("      <author username=\"@{}\" display_name=\"{}\" />\n", username, display_name));
                 builder.push_str(&format!("      <timestamp>{}</timestamp>\n", focal_timestamp));
                 builder.push_str(&format!("      <metrics likes=\"{}\" retweets=\"0\" replies=\"0\" />\n", likes));
                 builder.push_str(&format!("    </metadata>\n"));
                 builder.push_str(&format!("    <content>\n"));
                 builder.push_str(&format!("      <text>{}</text>\n", focal_text.replace("\n", "\n      ")));
                 builder.push_str(&format!("    </content>\n"));
                 builder.push_str(&format!("  </focal_tweet>\n"));
                 
                 if let Some(data) = thread_json["data"].as_array() {
                      if !data.is_empty() {
                           builder.push_str("  <thread_context>\n");
                           for item in data.iter().rev() {
                                let id = item["id"].as_str().unwrap_or("");
                                let text = item["text"].as_str().unwrap_or("...");
                                let created_at = item["created_at"].as_str().unwrap_or("unknown");
                                let t_likes = item["public_metrics"]["like_count"].as_i64().unwrap_or(0);
                                builder.push_str(&format!("    <tweet id=\"{}\">\n", id));
                                builder.push_str(&format!("      <metadata>\n"));
                                builder.push_str(&format!("        <timestamp>{}</timestamp>\n", created_at));
                                builder.push_str(&format!("        <metrics likes=\"{}\" retweets=\"0\" replies=\"0\" />\n", t_likes));
                                builder.push_str(&format!("      </metadata>\n"));
                                builder.push_str(&format!("      <content>\n"));
                                builder.push_str(&format!("        <text>{}</text>\n", text.replace("\n", "\n        ")));
                                builder.push_str(&format!("      </content>\n"));
                                builder.push_str(&format!("    </tweet>\n"));
                           }
                           builder.push_str("  </thread_context>\n");
                      }
                 }
                 builder.push_str(&format!("</twitter_thread>\n"));
            },
            _ => {
                 builder.push_str(&format!("Twitter Thread by @{}\nThread URL: {}\n\n", username, url));
                 builder.push_str(&format!("1. @{} ({})\n", username, display_name));
                 builder.push_str(&format!("Posted at: {} | Likes: {}\n", focal_timestamp, likes));
                 builder.push_str(&format!("> {}\n", focal_text.replace("\n", "\n> ")));
                 builder.push_str("---\n\n");
                 
                 let mut index = 2;
                 if let Some(data) = thread_json["data"].as_array() {
                      for item in data.iter().rev() {
                           let text = item["text"].as_str().unwrap_or("...");
                           let created_at = item["created_at"].as_str().unwrap_or("unknown");
                           let t_likes = item["public_metrics"]["like_count"].as_i64().unwrap_or(0);
                           builder.push_str(&format!("{}. @{} ({})\n", index, username, display_name));
                           builder.push_str(&format!("Posted at: {} | Likes: {}\n", created_at, t_likes));
                           builder.push_str(&format!("> {}\n", text.replace("\n", "\n> ")));
                           builder.push_str("---\n\n");
                           index += 1;
                      }
                 }
            }
        }
        
        builder
    }
}

impl Gobble for TwitterGobbler {
    fn gobble(&self, path: &std::path::Path, flags: &crate::cli::Cli) -> anyhow::Result<String> {
        let content = std::fs::read_to_string(path)?;
        self.gobble_str(&content, flags)
    }

    fn gobble_str(&self, url: &str, _flags: &crate::cli::Cli) -> anyhow::Result<String> {
        let tweet_id = self
            .extract_tweet_id(url)
            .context("Invalid Twitter URL format. Could not extract tweet ID.")?;

        eprintln!("🐦 Extracted Tweet ID: {}", tweet_id);

        let mut access_token_opt = None;
        if let Some(mut creds) = load_credentials() {
             if let (Some(token), Some(expires_at), Some(refresh)) = (&creds.twitter_access_token, creds.twitter_token_expires_at, &creds.twitter_refresh_token) {
                  let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                  if now > expires_at {
                       eprintln!("🔄 OAuth2 token expired. Refreshing...");
                       let client = build_oauth_client();
                       if let Ok(token_result) = client.exchange_refresh_token(&RefreshToken::new(refresh.to_string())).request(http_client) {
                            creds.twitter_access_token = Some(token_result.access_token().secret().to_string());
                            if let Some(rt) = token_result.refresh_token() {
                                 creds.twitter_refresh_token = Some(rt.secret().to_string());
                            }
                            let exp_in = token_result.expires_in().unwrap_or(std::time::Duration::from_secs(7200));
                            creds.twitter_token_expires_at = Some(now + exp_in.as_secs());
                            let _ = save_credentials(&creds);
                            access_token_opt = creds.twitter_access_token.clone();
                       }
                  } else {
                       access_token_opt = Some(token.to_string());
                  }
             }
        }

        if let Some(token) = access_token_opt {
             eprintln!("🕵️ Attempting Primary OAuth2 Extraction Flow...");
             match self.try_oauth2_extraction(&tweet_id, &token) {
                  Ok(json_ast) => {
                       eprintln!("✅ OAuth2 extraction succeeded! Parsing Thread...");
                       let parsed = self.parse_oauth_tree(url, &json_ast);
                       return Ok(parsed);
                  }
                  Err(e) => {
                       eprintln!("⚠️ OAuth2 failed ({}). Cascading to Guest Token...", e);
                       // Fallthrough to Guest Token logic
                  }
             }
        }

        eprintln!("🕵️ Attempting Legacy GraphQL Extraction Flow...");

        match self.try_graphql_extraction(&tweet_id) {
            Ok(json_ast) => {
                eprintln!("✅ GraphQL extraction succeeded! Parsing AST...");
                let parsed = self.parse_graphql_tree(url, &json_ast, &tweet_id);
                // Return exactly what was parsed.
                Ok(parsed)
            }
            Err(_e) => {
                use colored::Colorize;
                eprintln!("\n{}", "⚠️  Twitter rate-limited or blocked the unauthenticated request.".truecolor(255, 99, 71).bold());
                eprintln!("{}  For reliable access, run: {}", "💡".bold(), "gobble --twitter-login".truecolor(0, 255, 128).bold());
                eprintln!("    {}\n", "This takes ~2 minutes and stores credentials locally.");
                
                anyhow::bail!("Unauthenticated Twitter GraphQL Extraction Failed. Please use --twitter-login.");
            }
        }
    }
}

fn get_credentials_path() -> std::path::PathBuf {
    home::home_dir()
        .map(|h| h.join(".config/filegoblin/credentials.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("credentials.json"))
}

pub fn load_credentials() -> Option<LocalCredentials> {
    let path = get_credentials_path();
    if !path.exists() {
        return None;
    }
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_credentials(creds: &LocalCredentials) -> Result<()> {
    let path = get_credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(creds)?;
    std::fs::write(path, data)?;
    Ok(())
}

fn build_oauth_client() -> BasicClient {
    let client_id = std::env::var("TWITTER_CLIENT_ID")
        .unwrap_or_else(|_| "Wm5uMmdDMEtzQ21xTzlMZTlVUGs6MTpjaQ".to_string());

    BasicClient::new(
        ClientId::new(client_id),
        None,
        AuthUrl::new("https://twitter.com/i/oauth2/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://api.twitter.com/2/oauth2/token".to_string()).unwrap()),
    )
    .set_redirect_uri(RedirectUrl::new("http://127.0.0.1:7890/callback".to_string()).unwrap())
}

pub fn handle_twitter_login() -> Result<()> {
    use colored::Colorize;
    eprintln!("{}", "🐦 Initiating Twitter OAuth 2.0 Login...".truecolor(0, 200, 255));

    let client = build_oauth_client();
    let (pkce_code_challenge, pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();

    let (authorize_url, _csrf_state) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("tweet.read".to_string()))
        .add_scope(Scope::new("users.read".to_string()))
        .add_scope(Scope::new("offline.access".to_string()))
        .set_pkce_challenge(pkce_code_challenge)
        .url();

    eprintln!("\n{}\n", "Press ENTER to open your browser and authorize 'filegoblin'.".bold());
    eprintln!("If the browser doesn't open automatically, navigate to this URL:\n{}\n", authorize_url.as_str().underline());
    
    let mut dummy = String::new();
    std::io::stdin().read_line(&mut dummy)?;

    if let Err(e) = open::that(authorize_url.as_str()) {
         eprintln!("⚠️ Could not open browser automatically: {}", e);
    }

    let server = Server::http("127.0.0.1:7890").map_err(|e| anyhow::anyhow!("Failed to bind to localhost:7890. Is the port in use? {}", e))?;
    eprintln!("Waiting for authorization callback on localhost:7890...");

    let mut auth_code = None;

    for request in server.incoming_requests() {
        let url = request.url().to_string();
        if url.starts_with("/callback") {
            if let Some(query) = url.split('?').nth(1) {
                for pair in query.split('&') {
                    let mut kv = pair.split('=');
                    if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
                        if k == "code" {
                            auth_code = Some(AuthorizationCode::new(v.to_string()));
                        }
                    }
                }
            }

            let response = Response::from_string("Success! You can now close this tab and return to the terminal.");
            let _ = request.respond(response);
            break;
        } else {
             let response = Response::from_string("Not Found").with_status_code(404);
             let _ = request.respond(response);
        }
    }

    let code = auth_code.context("No authorization code received from Twitter")?;

    eprintln!("Exchanging OAuth code for access tokens...");

    let token_result = client
        .exchange_code(code)
        .set_pkce_verifier(pkce_code_verifier)
        .request(http_client)
        .context("Failed to retrieve token from Twitter API")?;

    let expires_in = token_result.expires_in().unwrap_or(std::time::Duration::from_secs(7200));
    let expires_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + expires_in.as_secs();

    let mut creds = load_credentials().unwrap_or_default();
    creds.twitter_access_token = Some(token_result.access_token().secret().to_string());
    if let Some(rt) = token_result.refresh_token() {
         creds.twitter_refresh_token = Some(rt.secret().to_string());
    }
    creds.twitter_token_expires_at = Some(expires_at);

    save_credentials(&creds)?;

    let display_name = match reqwest::blocking::Client::new()
         .get("https://api.twitter.com/2/users/me")
         .bearer_auth(token_result.access_token().secret())
         .send() {
              Ok(res) => {
                   if let Ok(json) = res.json::<Value>() {
                        json["data"]["username"].as_str().unwrap_or("Unknown").to_string()
                   } else {
                        "Unknown".to_string()
                   }
              },
              Err(_) => "Unknown".to_string()
         };

    eprintln!("{} Connected as @{}! Credentials saved to ~/.config/filegoblin/credentials.json", "✅".green(), display_name.bold());

    Ok(())
}

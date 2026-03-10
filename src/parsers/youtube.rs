use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::path::Path;

pub struct YouTubeGobbler;

impl YouTubeGobbler {
    pub fn new() -> Self {
        Self
    }

    fn extract_id(&self, url: &str) -> Result<String> {
        // Supports:
        // https://www.youtube.com/watch?v=VIDEO_ID
        // https://youtu.be/VIDEO_ID
        // https://www.youtube.com/shorts/VIDEO_ID
        let url_obj = url::Url::parse(url).context("Failed to parse YouTube URL")?;
        
        if let Some(domain) = url_obj.domain() {
            if domain.ends_with("youtu.be") {
                if let Some(id) = url_obj.path_segments().and_then(|mut s| s.next()) {
                    return Ok(id.to_string());
                }
            } else if domain.ends_with("youtube.com") {
                if url_obj.path().starts_with("/shorts/") {
                    if let Some(id) = url_obj.path_segments().and_then(|mut s| s.nth(1)) {
                        return Ok(id.to_string());
                    }
                } else if let Some(query_pairs) = Some(url_obj.query_pairs()) {
                    for (k, v) in query_pairs {
                        if k == "v" {
                            return Ok(v.to_string());
                        }
                    }
                }
            }
        }
        
        anyhow::bail!("Could not extract video ID from YouTube URL: {}", url)
    }

    fn get_player_response(&self, client: &Client, video_id: &str) -> Result<Value> {
        let url = "https://www.youtube.com/youtubei/v1/player";
        let body = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "ANDROID",
                    "clientVersion": "20.10.38"
                }
            },
            "videoId": video_id
        });

        client.post(url)
            .json(&body)
            .send()
            .context("Failed to connect to YouTube InnerTube API")?
            .json::<Value>()
            .context("Failed to parse player response JSON")
    }

    fn select_best_track<'a>(&self, tracks: &'a Vec<Value>, preferred_lang: Option<&str>) -> Result<&'a Value> {
        // Priority:
        // 1. Manual English ("en")
        // 2. Manual requested language (if preferred_lang is some)
        // 3. Auto English ("a.en")
        // 4. Auto requested language
        // 5. First available fallback
        
        let mut manual_en = None;
        let mut manual_pref = None;
        let mut auto_en = None;
        let mut auto_pref = None;

        for track in tracks {
            if let Some(vss_id) = track.get("vssId").and_then(|v| v.as_str()) {
                if vss_id == ".en" || vss_id == "en" {
                    manual_en = Some(track);
                } else if let Some(lang) = preferred_lang {
                    if vss_id == format!(".{}", lang) || vss_id == lang {
                        manual_pref = Some(track);
                    } else if vss_id == format!("a.{}", lang) {
                        auto_pref = Some(track);
                    }
                }
                
                if vss_id == "a.en" {
                    auto_en = Some(track);
                }
            }
        }

        if let Some(track) = manual_en {
            return Ok(track);
        }
        if let Some(track) = manual_pref {
            return Ok(track);
        }
        if let Some(track) = auto_en {
            return Ok(track);
        }
        if let Some(track) = auto_pref {
            return Ok(track);
        }
        
        tracks.first().context("No tracks found even though array was non-empty")
    }

    fn parse_transcript_xml(&self, xml: &str) -> Result<String> {
        use quick_xml::events::Event;
        use quick_xml::reader::Reader;

        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut buf = Vec::new();
        let mut transcript = String::new();
        let mut in_text = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) if e.name().as_ref() == b"text" => {
                    in_text = true;
                }
                Ok(Event::End(ref e)) if e.name().as_ref() == b"text" => {
                    in_text = false;
                    transcript.push('\n');
                }
                Ok(Event::Text(e)) if in_text => {
                    let raw_str = std::str::from_utf8(e.as_ref()).unwrap_or_default();
                    let decoded = raw_str
                        // Full entity forms
                        .replace("&amp;", "&")
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&quot;", "\"")
                        .replace("&apos;", "'")
                        .replace("&#39;", "'")
                        .replace("&nbsp;", " ")
                        // Partial entity remnants (quick-xml consumes leading '&')
                        .replace("gt;gt;", ">>")
                        .replace("gt;", ">")
                        .replace("lt;", "<")
                        .replace("amp;", "&")
                        .replace("quot;", "\"")
                        .replace("apos;", "'")
                        .replace("#39;", "'")
                        .replace("nbsp;", " ");
                    let cleaned = decoded.replace('\n', " ");
                    transcript.push_str(&cleaned);
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(anyhow::anyhow!("Error at position {}: {:?}", reader.buffer_position(), e)),
                _ => (),
            }
            buf.clear();
        }

        Ok(transcript.trim().to_string())
    }

    fn attempt_yt_dlp_fallback(&self, video_url: &str) -> Result<String> {
        let output = std::process::Command::new("yt-dlp")
            .args([
                "--write-auto-sub",
                "--skip-download",
                "--sub-format", "srt",
                "--output", "-",
                video_url
            ])
            .output()
            .context("Failed to execute yt-dlp. Is it installed in PATH?")?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("yt-dlp failed: {}", err);
        }

        let srt_content = String::from_utf8(output.stdout).context("yt-dlp output was not valid UTF-8")?;
        
        // Simple SRT to text conversion: strip timestamps and index lines
        let mut transcript = String::new();
        for line in srt_content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.chars().all(|c| c.is_ascii_digit()) || trimmed.contains("-->") {
                continue;
            }
            // Filter some VTT-like tags
            let cleaned = trimmed.replace("<<", "").replace(">>", "");
            transcript.push_str(&cleaned);
            transcript.push('\n');
        }

        Ok(transcript.trim().to_string())
    }
}

impl Gobble for YouTubeGobbler {
    fn gobble(&self, _path: &Path, _flags: &crate::cli::Cli) -> Result<String> {
        anyhow::bail!("YouTubeGobbler only supports gobble_str with URL")
    }

    fn gobble_str(&self, url: &str, flags: &crate::cli::Cli) -> Result<String> {
        let client = Client::builder()
            .use_rustls_tls()
            .build()?;

        let video_id = self.extract_id(url)?;
        let response = self.get_player_response(&client, &video_id)?;

        let playability = response["playabilityStatus"]["status"].as_str().unwrap_or("OK");
        if playability != "OK" {
            anyhow::bail!("Video not playable (requires login or unavailable): {}", playability);
        }

        let title = response["videoDetails"]["title"].as_str().unwrap_or("Unknown Title").to_string();
        let author = response["videoDetails"]["author"].as_str().unwrap_or("Unknown Author").to_string();
        let duration = response["videoDetails"]["lengthSeconds"].as_str().unwrap_or("Unknown Duration").to_string();

        let captions = response["captions"]["playerCaptionsTracklistRenderer"]["captionTracks"]
            .as_array()
            .cloned() // Clone or extract earlier to avoid borrowing issues
            .unwrap_or_default();

        let transcript_content = if captions.is_empty() {
            // Attempt yt-dlp fallback if we get empty captions but playability is OK
            self.attempt_yt_dlp_fallback(url)
                .map_err(|e| anyhow::anyhow!("No native transcripts found and fallback failed: {}", e))?
        } else {
            let best_track = self.select_best_track(&captions, flags.lang.as_deref())?;
            let base_url = best_track["baseUrl"].as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing baseUrl in caption track"))?
                .to_string();

            let mut parsed_url = url::Url::parse(&base_url).map_err(|e| anyhow::anyhow!("Invalid baseUrl: {}", e))?;
            let mut query_pairs: Vec<(String, String)> = parsed_url.query_pairs().into_owned().collect();
            query_pairs.retain(|(k, _)| k != "fmt");
            query_pairs.push(("fmt".to_string(), "srv1".to_string()));
            
            // Server-side translation
            if let Some(lang) = &flags.lang {
                if best_track["vssId"].as_str().unwrap_or("") != format!(".{}", lang) &&
                   best_track["vssId"].as_str().unwrap_or("") != format!("a.{}", lang) {
                     query_pairs.push(("tlang".to_string(), lang.clone()));
                }
            }
            
            parsed_url.query_pairs_mut().clear().extend_pairs(query_pairs);
            let fetch_url = parsed_url.to_string();

            let xml_resp = client.get(&fetch_url).send()?.text()?;
            
            if xml_resp.trim().is_empty() {
                // Potentially blocked by POT - fallback to yt-dlp
                self.attempt_yt_dlp_fallback(url)
                    .map_err(|e| anyhow::anyhow!("Native InnerTube fetch returned empty payload (POT block likely) and fallback failed: {}", e))?
            } else {
                self.parse_transcript_xml(&xml_resp)?
            }
        };

        Ok(format!(
            "# {}\n\n**Channel:** {}\n**Duration:** {}s\n\n{}",
            title, author, duration, transcript_content
        ))
    }
}

# **Engineering Analysis for Unauthenticated YouTube Transcript Ingestion in Zero-Dependency Rust Environments**

The evolution of automated textual extraction from streaming video platforms has encountered a paradigm shift in the mid-2020s, as foundational platforms like YouTube have increasingly hardened their internal APIs against traditional scraping techniques. For specialized utilities such as filegoblin, which are engineered for deployment in air-gapped corporate environments and restricted systems, the requirement for zero-dependency, statically-linked binaries imposes a unique set of challenges. These constraints necessitate a deep understanding of the underlying network protocols, cryptographic handshakes, and data serialization formats that govern YouTube's transcription services. The transition from legacy endpoints like /get\_video\_info to the modern, more obscured "InnerTube" API framework has redefined the request patterns required to access video metadata and caption tracks.1

## **Architectural Constraints and the Rust Static Linking Paradigm**

The deployment of filegoblin within environments where users lack the authority to install system packages or shared libraries dictates a strict adherence to pure-Rust implementations or minimal subprocess interaction. In the context of 2025 and 2026, the most critical hurdle for a statically linked Rust binary is the handling of Transport Layer Security (TLS).3 Traditional Rust HTTP clients often default to native-tls, which serves as a thin wrapper around the operating system's native TLS implementation, such as OpenSSL on Linux, Security Framework on macOS, and Schannel on Windows.4 On Linux systems, this typically introduces a dynamic dependency on libssl.so, which violates the hard architectural constraint for filegoblin.5

To maintain a single, portable binary, the implementation must utilize rustls, a modern TLS library written entirely in Rust.6 Unlike OpenSSL, rustls does not require external shared libraries and can be compiled directly into the filegoblin binary. However, the move toward rustls in 2024 and 2025 has introduced new complexities, particularly with the introduction of the aws-lc-rs cryptographic provider as the default.5 While aws-lc-rs offers superior performance through optimized assembly code, it often requires cmake and other build-time dependencies that can complicate cross-compilation for various target architectures.5 For the highest level of portability across macOS, Linux, and Windows, the ring provider remains a viable alternative for rustls, as it simplifies the build pipeline while maintaining the required safety guarantees.5

The following table compares the implications of different TLS backends for the filegoblin static binary requirement:

| TLS Backend | Static Linking Capability | Shared Library Dependency | Build-time Requirements | Compatibility (Linux/Mac/Win) |
| :---- | :---- | :---- | :---- | :---- |
| native-tls | Partial (requires vendoring) | libssl.so (Linux) | System-level SSL headers | High, but varies by OS version |
| rustls (aws-lc) | Full | None | cmake, nasm, C compiler | High performance, complex cross-build |
| rustls (ring) | Full | None | Standard Rust toolchain | High portability, minimal build deps |
| native-tls-vendored | Full | None | C compiler, Perl (for OpenSSL) | Increases binary size significantly |

Source: 4

Given the goal of a zero-dependency binary, the recommended approach for filegoblin is the use of reqwest with the rustls-tls-native-roots feature. This configuration allows the binary to be entirely self-contained regarding its cryptographic logic while still utilizing the host system's root certificate store for authenticating YouTube's servers.6

## **Technical Investigation of the InnerTube API and Discovery Patterns**

The primary mechanism for discovering available transcript tracks in 2025 and 2026 is the InnerTube API, specifically the /v1/player endpoint.1 This internal API is the same backend used by official YouTube clients, including the Android and iOS applications, as well as modern smart TVs. Accessing this endpoint requires an API key, which is not obtained through the Google Cloud Console but is instead scraped directly from the HTML source of the video watch page.12

### **Discovery through Player Response Parsing**

When a user provides a YouTube URL, the first step is to perform a GET request to the watch page (e.g., https://www.youtube.com/watch?v=VIDEO\_ID). The response contains a large HTML document with embedded JSON structures.12 The most important of these is the ytInitialPlayerResponse object. This object contains the complete "tracklist" of available captions.1

The exact request pattern to discover these tracks without utilizing the HTML embedded data involves a POST request to the InnerTube API. This method is often more robust as it mimics the behavior of the YouTube Android application, which is less frequently subject to the draconian bot-detection measures applied to standard web browsers.1

The request body for the /v1/player endpoint must be structured as follows:

JSON

{  
  "context": {  
    "client": {  
      "clientName": "ANDROID",  
      "clientVersion": "20.10.38"  
    }  
  },  
  "videoId": "VIDEO\_ID"  
}

The clientVersion string must correspond to a contemporary version of the YouTube Android app to avoid rejection.1 The response from this POST request is a complex JSON object. To locate the transcript tracks, the parser must navigate the following path: captions.playerCaptionsTracklistRenderer.captionTracks. This array contains objects for each available language and type of transcript.1

### **Transcript Track Metadata and Formats**

Each object in the captionTracks array provides essential metadata that filegoblin must use to select the appropriate source for LLM ingestion. The critical fields include:

| Field Name | Description | Example Value |
| :---- | :---- | :---- |
| baseUrl | The signed URL to fetch the actual transcript data | https://www.youtube.com/api/timedtext?... |
| vssId | The internal ID, indicating if it is auto-generated | .en (Manual) or a.en (Auto) |
| languageCode | The ISO 639-1 code for the transcript language | en, es, zh-Hans |
| kind | The type of transcript, such as ASR (auto) | asr |
| name | The human-readable label for the track | English (auto-generated) |

Source: 1

A critical insight for the 2026 landscape is that these baseUrl values are pre-signed and often contain an expiration timestamp and a signature parameter.12 While historically these URLs were stable for long periods, they are now frequently tied to the session that generated the player response, emphasizing the need for filegoblin to perform the discovery and the fetch in a single, contiguous workflow.18

## **Analysis of the timedtext Endpoint and Proof of Origin Tokens**

The actual textual data of the transcript is retrieved from the https://www.youtube.com/api/timedtext endpoint using the baseUrl identified during the discovery phase.12 However, as of mid-2025, YouTube has significantly increased the security requirements for this endpoint. A major development is the requirement of the pot (Proof of Origin Token) parameter.18

### **The Proof of Origin (POT) Mechanism**

The pot parameter is a security token designed to verify that the request originated from a legitimate client that has passed a "Service Integrity" check.18 When this parameter is missing, the timedtext API may return a successful HTTP 200 status code but provide an empty body, which can be highly confusing for standard scrapers.18 This token is typically accompanied by a potc parameter, often set to 1\.18

Generating these tokens natively in Rust without a JavaScript engine is exceptionally difficult, as they are often generated by obscured client-side scripts. However, research into the yt-dlp ecosystem reveals that these tokens can sometimes be bypassed or simulated by providing the correct serviceIntegrityDimensions within the initial InnerTube POST request.18 For an unauthenticated CLI like filegoblin, the most stable path is to impersonate the ANDROID client, which currently has different token requirements compared to the WEB client.1

### **Data Formats: SRV1, JSON3, and WebVTT**

The timedtext endpoint supports multiple output formats, which can be requested by appending a fmt parameter to the URL.14 For the purposes of filegoblin, the choice of format is governed by the ease of parsing and the preservation of relevant metadata like speaker labels.

1. **SRV1 (XML)**: This is the traditional format. It returns a simple XML document with \<text\> tags. Each tag includes a start (start time in seconds) and dur (duration) attribute.17 This is highly compatible with the quick-xml crate already used in filegoblin.  
2. **JSON3 (JSON)**: A more modern, event-based format. It provides detailed segments and is natively supported by serde\_json. It is particularly useful if the transcript includes rich formatting or complex timing, although it is more verbose than SRV1.21  
3. **WebVTT / SRT**: These are standard subtitle formats. While useful for video players, they include significant formatting overhead (like arrow delimiters and line numbers) that must be stripped for LLM consumption.25

The SRV1 format is the recommended target for filegoblin due to its minimal parsing overhead and high reliability for unauthenticated access.17

The mathematical representation of a transcript segment in SRV1 follows a simple structure:

![][image1]  
Where the cumulative text for the LLM is the concatenation of all ![][image2] after entity decoding.17

## **Video Metadata Extraction without the Data API**

Extracting the title, channel name, and duration without using the official Google Data API (which requires an OAuth2 token or a paid-tier API key) requires parsing the page's initial state.29 The ytInitialPlayerResponse JSON blob mentioned previously is the most reliable source for this data.1

### **Minimal Parsing Approach**

The metadata is nested within the videoDetails object of the player response. The following JSON paths are current as of 2026:

* **Title**: videoDetails.title  
* **Channel Name**: videoDetails.author  
* **Duration (Seconds)**: videoDetails.lengthSeconds  
* **View Count**: videoDetails.viewCount  
* **Thumbnail URL**: videoDetails.thumbnail.thumbnails.url

Parsing this in Rust using serde\_json is highly efficient. The application can use a "loose" struct definition that only captures these fields, ignoring the megabytes of other data present in the response.1

Rust

\#  
struct YouTubeMetadata {  
    \#  
    details: VideoDetails,  
}

\#  
struct VideoDetails {  
    title: String,  
    author: String,  
    \#  
    duration: String,  
}

This approach allows filegoblin to extract all necessary metadata with a single regex match on the HTML source followed by a single JSON deserialization step, minimizing memory usage and processing time.1

## **Evaluation of the Rust Crate Landscape for YouTube Ingestion**

A survey of crates.io reveals a handful of libraries dedicated to YouTube transcript extraction, though few are maintained with the rigor required for the rapidly changing 2025/2026 environment.29

| Crate | Latest Version | Features | Dependencies | Static Linking Compatibility |
| :---- | :---- | :---- | :---- | :---- |
| ytt | 1.1.0 | InnerTube API, Markdown, JSON, SRT | quick-xml, tokio | High (Pure Rust) |
| yt-transcript-rs | 0.1.1 | Proxies, Cookie auth, Metadata | anyhow, serde, tokio | High (Pure Rust) |
| ytranscript | 0.1.0 | ID extraction, basic fetch | regex, reqwest | Moderate (Dependency on old regex) |

Source: 20

The ytt crate is the most promising for integration into filegoblin. It is a Rust implementation of the YouTube Transcript API that specifically targets the InnerTube player response.20 It already produces Markdown output, which aligns with the project's goals. However, as it is a standalone CLI tool, filegoblin may prefer to implement its own logic using the same principles to maintain total control over dependencies like tokio versus blocking reqwest.4

If filegoblin chooses to implement the logic natively, the minimum requirements are reqwest for the HTTP transport, serde\_json for discovery, and quick-xml for the final transcript parsing—all of which are already present in the project's dependency tree.4

## **Auto-Generated vs. Manual Transcripts: Priority and Quality**

YouTube provides two distinct types of transcripts: those manually uploaded by the creator (or through community contributions) and those generated by Google's Automated Speech Recognition (ASR) system.35 For LLM ingestion, manual transcripts are vastly superior as they contain proper punctuation, capitalization, and often preserve speaker labels.25

### **Priority Logic for Track Selection**

When multiple tracks are available, filegoblin should implement a priority-based selection algorithm. The vssId field in the InnerTube response is the key to identifying these types: manual tracks have a standard language code (e.g., en), while ASR tracks are prefixed with a. (e.g., a.en).14

The recommended priority order is:

1. **Manual English**: Standard English transcript (vssId: "en").  
2. **Manual Other Language**: If the video is non-English, but a manual transcript exists for that language.35  
3. **Auto-generated English**: The ASR version (vssId: "a.en").36  
4. **Auto-generated Other**: Fallback for non-English content without manual tracks.

Enumerating these tracks involves iterating through the captionTracks array and checking for the presence of the kind: "asr" field.14 If a user provides a \--lang flag, the logic should first search for a manual track with that code, then an auto-generated one, and finally resort to YouTube's server-side translation feature if the requested language is not natively available.14

## **Multi-Language Support and Server-Side Translation**

YouTube's timedtext API provides a powerful but often overlooked feature: automatic translation.14 By appending the tlang parameter to a transcript URL, the server will translate the source transcript into the requested language before delivering it.14

For example, a URL fetching a Spanish transcript:

https://www.youtube.com/api/timedtext?v=VIDEO\_ID\&lang=es

Can be converted to an English translation by appending:

\&tlang=en

This feature is invaluable for filegoblin, as it allows the tool to support 99+ languages without needing a local translation model or external API credits.14 The \--lang flag in filegoblin should map directly to this tlang parameter if the requested language is not available as a native track.

## **The yt-dlp Subprocess Fallback Strategy**

The volatile nature of YouTube's internal APIs means that any native implementation is subject to breaking when YouTube updates its security protocols.12 As a robust fallback, filegoblin can leverage yt-dlp if it is detected in the system's $PATH.

### **Fallback Evaluation and Decision Framework**

The use of yt-dlp as a fallback is highly recommended for non-air-gapped environments. It handles the complexities of signature deciphering, PO token generation, and rate limit mitigation through a globally maintained codebase.18

The exact command to extract a transcript to stdout without downloading the video is:

yt-dlp \--write-auto-sub \--skip-download \--sub-format srt \--output \- "VIDEO\_URL"

Parsing the SRT or VTT output from yt-dlp is more complex than SRV1 XML but can be handled by a dedicated parser if necessary.14

**Decision Framework for Ingestion**:

1. **Attempt Native Extraction**: Use the InnerTube /v1/player endpoint and timedtext (SRV1) method. This is the fastest and has zero external dependencies.  
2. **Verify Result**: If the response is an empty 200 or an error 403/429, proceed to fallback.18  
3. **Check for yt-dlp**: If yt-dlp exists in $PATH, execute the subprocess command.  
4. **Final Fail**: If yt-dlp is missing or fails, return a clear error message to the user indicating the need for authentication or a browser-based session.18

## **Rate Limits, Captchas, and Mitigation Tactics**

YouTube implements aggressive rate limiting to prevent bulk scraping.18 When a limit is hit, the server typically returns an HTTP 429 "Too Many Requests" error or, more aggressively, redirects the request to a captcha page.34

### **Mitigation Strategies**

For a CLI tool like filegoblin, which is typically used for single-video ingestion by a human, rate limits are rarely an issue.41 However, for recursive crawling or automated pipelines, the following tactics are necessary:

* **Exponential Backoff**: If a 429 error is received, the tool should wait and retry. A suggested sequence is 1s, 2s, 4s, with added random jitter.41  
* **User-Agent Randomization**: While InnerTube requests should use a consistent ANDROID client version, standard web requests for the watch page should use a realistic User-Agent string (e.g., a modern Chrome version).41  
* **Proxy Support**: filegoblin already uses reqwest, which supports system proxies. Users in heavily restricted environments can route their traffic through a proxy to avoid IP-based blocks.33

If a video is age-restricted or private, YouTube will return a "Video Unavailable" error or a redirect to a login page.14 The YouTubeGobbler should detect these specific JSON error messages (e.g., playabilityStatus.status \== "LOGIN\_REQUIRED") and inform the user accordingly rather than failing silently.14

## **Implementation Sketch and Logic Flow**

The integration of the YouTubeGobbler into the filegoblin project requires a multi-stage approach that respects the established Gobble trait and the existing dependency on blocking reqwest clients.

### **Logic Flow for YouTube Transcription**

1. **URL Validation**: Regular expression check for youtube.com/watch?v=..., youtu.be/..., or youtube.com/shorts/....20  
2. **Metadata and Discovery**:  
   * Perform an unauthenticated POST to /v1/player impersonating the ANDROID client.1  
   * Deserialize the response to find videoDetails and captionTracks.1  
3. **Track Selection**:  
   * Apply priority: Manual English \> Manual requested lang \> Auto English \> Auto requested lang.35  
   * Construct the timedtext URL, appending \&fmt=srv1 and, if necessary, \&tlang=... for translation.14  
4. **Transcript Retrieval**:  
   * Fetch the SRV1 XML.17  
   * If the result is empty or gated by a PO token, attempt the yt-dlp fallback.18  
5. **Markdown Transformation**:  
   * Header: Title, Channel, Duration.  
   * Body: Iterate through \<text\> tags, decode HTML entities, and concatenate into clean paragraphs.17  
   * Preserve speaker labels if found (common in manual SRT/VTT sources translated to SRV1).25

### **Implementation Pseudocode (Rust)**

Rust

use anyhow::{Result, Context};  
use serde\_json::Value;  
use reqwest::blocking::Client;  
use std::path::Path;

pub struct YouTubeGobbler;

impl YouTubeGobbler {  
    pub fn new() \-\> Self { Self }

    fn get\_player\_response(&self, client: \&Client, video\_id: &str) \-\> Result\<Value\> {  
        let url \= "https://www.youtube.com/youtubei/v1/player";  
        let body \= serde\_json::json\!({  
            "context": {  
                "client": {  
                    "clientName": "ANDROID",  
                    "clientVersion": "20.10.38"  
                }  
            },  
            "videoId": video\_id  
        });

        client.post(url)  
           .json(\&body)  
           .send()  
           .context("Failed to connect to YouTube InnerTube API")?  
           .json::\<Value\>()  
           .context("Failed to parse player response JSON")  
    }

    fn parse\_transcript\_xml(&self, xml: &str) \-\> Result\<String\> {  
        // Use quick-xml to find all \<text\> elements  
        // Concatenate their contents after entity decoding  
        // Logic similar to tt2srt.py or other SRV1 parsers  
        unimplemented\!()  
    }  
}

impl Gobble for YouTubeGobbler {  
    fn gobble(&self, url: \&Path, flags: \&Cli) \-\> Result\<String\> {  
        let client \= Client::builder()  
           .use\_rustls\_tls() // Ensure zero-dependency static linking  
           .build()?;

        let video\_id \= extract\_id(url)?;  
        let response \= self.get\_player\_response(\&client, \&video\_id)?;

        // Verify playability (handle private/age-restricted)  
        let status \= response\["status"\].as\_str().unwrap\_or("OK");  
        if status\!= "OK" {  
            return Err(anyhow::anyhow\!("Video not playable: {}", status));  
        }

        let metadata \= parse\_metadata(\&response)?;  
        let tracks \= response\["captions"\]  
           .as\_array()  
           .ok\_or\_else(|| anyhow::anyhow\!("No transcripts available"))?;

        let best\_track \= select\_best\_track(tracks, flags.lang)?;  
        let transcript\_url \= best\_track\["baseUrl"\].as\_str().unwrap();

        let xml \= client.get(transcript\_url).send()?.text()?;  
        let content \= self.parse\_transcript\_xml(\&xml)?;

        // Return clean Markdown  
        Ok(format\!(  
            "\# {}\\n\\n\*\*Channel:\*\* {}\\n\*\*Duration:\*\* {}\\n\\n{}",  
            metadata.title, metadata.author, metadata.duration, content  
        ))  
    }  
}

## **Stability Assessment and Future Risks**

The proposed approach is the most stable unauthenticated method available in 2026, as it leverages the same API used by millions of mobile devices.1 Because YouTube cannot easily break this endpoint without breaking legacy mobile applications, it provides a high degree of resilience. However, several emerging trends pose risks:

1. **Mandatory Proof of Origin (POT)**: If YouTube forces all ANDROID client requests to provide a hardware-attested PO token, unauthenticated CLI tools will be blocked.18 The only defense is a robust fallback to browser-based cookie extraction or authenticated sessions.18  
2. **Obfuscation of Metadata**: YouTube has a history of changing the JSON paths for fields like title and author.12 By using a flexible, loose parsing approach with serde\_json, filegoblin can easily update its logic if these paths drift.  
3. **ASR Quality Volatility**: While ASR continues to improve, changes in the model can affect how timestamps and word-grouping work in the SRV1 output.36 This is largely transparent to filegoblin but may result in less readable text for the LLM.

The probability of this method remaining functional for 6–12 months without major maintenance is moderate-to-high, provided the implementation is flexible enough to handle slight variations in the JSON structure.12

## **Conclusion and Strategic Recommendations**

The implementation of YouTube transcript ingestion for filegoblin must prioritize the project's air-gapped, zero-dependency requirements while acknowledging the hardening security of the YouTube ecosystem. A dual-track strategy is the most effective:

* **Primary Path**: A native Rust implementation using rustls for static linking, interfacing with the InnerTube /v1/player API for discovery and the timedtext SRV1 endpoint for data retrieval. This approach utilizes existing project dependencies (reqwest, serde\_json, quick-xml) and avoids external system libraries.6  
* **Fallback Path**: An optional subprocess call to yt-dlp for environments where it is available, providing a battle-tested safety net for videos that require complex signature deciphering or PO token handling.18

By prioritizing manual transcripts over auto-generated ones and leveraging YouTube's server-side translation, filegoblin can provide high-quality, multi-language textual data to LLMs with minimal latency and maximal portability.14 This strategy ensures that filegoblin remains a robust component of the modern AI toolchain, capable of ingesting diverse video content even in the most restricted technical environments.

#### **Works cited**

1. Reverse-Engineering YouTube: Revisited \- Oleksii Holub, accessed March 9, 2026, [https://tyrrrz.me/blog/reverse-engineering-youtube-revisited](https://tyrrrz.me/blog/reverse-engineering-youtube-revisited)  
2. wslyyy/youtube-go: Go Client for Google's Private InnerTube API. Works with YouTube, YouTube Music and more\! \- GitHub, accessed March 9, 2026, [https://github.com/wslyyy/youtube-go](https://github.com/wslyyy/youtube-go)  
3. Why exactly are Rust binaries generally larger than a C binaries? \- Stack Overflow, accessed March 9, 2026, [https://stackoverflow.com/questions/79904038/why-exactly-are-rust-binaries-generally-larger-than-a-c-binaries](https://stackoverflow.com/questions/79904038/why-exactly-are-rust-binaries-generally-larger-than-a-c-binaries)  
4. reqwest \- crates.io: Rust Package Registry, accessed March 9, 2026, [https://crates.io/crates/reqwest/0.13.1](https://crates.io/crates/reqwest/0.13.1)  
5. reqwest v0.13 \- rustls by default : r/rust \- Reddit, accessed March 9, 2026, [https://www.reddit.com/r/rust/comments/1pzlrqx/reqwest\_v013\_rustls\_by\_default/](https://www.reddit.com/r/rust/comments/1pzlrqx/reqwest_v013_rustls_by_default/)  
6. reqwest v0.13 \- rustls by default \- seanmonstar, accessed March 9, 2026, [https://seanmonstar.com/blog/reqwest-v013-rustls-default/](https://seanmonstar.com/blog/reqwest-v013-rustls-default/)  
7. Why rust is failing to build command for openssl-sys v0.9.60 even after local installation?, accessed March 9, 2026, [https://stackoverflow.com/questions/65553557/why-rust-is-failing-to-build-command-for-openssl-sys-v0-9-60-even-after-local-in](https://stackoverflow.com/questions/65553557/why-rust-is-failing-to-build-command-for-openssl-sys-v0-9-60-even-after-local-in)  
8. Securing the Web: Rustls on track to outperform OpenSSL : r/rust \- Reddit, accessed March 9, 2026, [https://www.reddit.com/r/rust/comments/18ygzjo/securing\_the\_web\_rustls\_on\_track\_to\_outperform/](https://www.reddit.com/r/rust/comments/18ygzjo/securing_the_web_rustls_on_track_to_outperform/)  
9. rustls \- Rust \- Docs.rs, accessed March 9, 2026, [https://docs.rs/rustls/latest/rustls/](https://docs.rs/rustls/latest/rustls/)  
10. Rustls Outperforms OpenSSL and BoringSSL \- Prossimo \- Memory Safety, accessed March 9, 2026, [https://www.memorysafety.org/blog/rustls-performance-outperforms/](https://www.memorysafety.org/blog/rustls-performance-outperforms/)  
11. Rustls vs openssl 2024 \- community \- The Rust Programming Language Forum, accessed March 9, 2026, [https://users.rust-lang.org/t/rustls-vs-openssl-2024/111754](https://users.rust-lang.org/t/rustls-vs-openssl-2024/111754)  
12. Extract YouTube Transcripts Using Innertube API (2025 JavaScript Guide) | by Mohammed Aqib | Medium, accessed March 9, 2026, [https://medium.com/@aqib-2/extract-youtube-transcripts-using-innertube-api-2025-javascript-guide-dc417b762f49](https://medium.com/@aqib-2/extract-youtube-transcripts-using-innertube-api-2025-javascript-guide-dc417b762f49)  
13. How to Get YouTube Video Transcripts in Seconds, accessed March 9, 2026, [https://www.youtube.com/watch?v=i\_hThUuNeLs](https://www.youtube.com/watch?v=i_hThUuNeLs)  
14. How to Get YouTube Transcripts: A Complete Developer's Guide | by Taimur Khan \- Medium, accessed March 9, 2026, [https://medium.com/@volods/how-to-get-youtube-transcripts-a-complete-developers-guide-b3f092eb0a96](https://medium.com/@volods/how-to-get-youtube-transcripts-a-complete-developers-guide-b3f092eb0a96)  
15. Best practice question: extracting structured metadata from YouTube watch pages (JSON / JSON-LD) \- Reddit, accessed March 9, 2026, [https://www.reddit.com/r/youtube/comments/1q103jc/best\_practice\_question\_extracting\_structured/](https://www.reddit.com/r/youtube/comments/1q103jc/best_practice_question_extracting_structured/)  
16. YouTube Video Transcript API \- SerpApi, accessed March 9, 2026, [https://serpapi.com/youtube-video-transcript](https://serpapi.com/youtube-video-transcript)  
17. Fetch Youtube captions from browser on 2026 : r/webdev \- Reddit, accessed March 9, 2026, [https://www.reddit.com/r/webdev/comments/1qyadf1/fetch\_youtube\_captions\_from\_browser\_on\_2026/](https://www.reddit.com/r/webdev/comments/1qyadf1/fetch_youtube_captions_from_browser_on_2026/)  
18. Youtube caption extract, timedtext api \&pot parameter \- Stack Overflow, accessed March 9, 2026, [https://stackoverflow.com/questions/79668836/youtube-caption-extract-timedtext-api-pot-parameter](https://stackoverflow.com/questions/79668836/youtube-caption-extract-timedtext-api-pot-parameter)  
19. YouTube timedtext for Captions · Issue \#251 \- GitHub, accessed March 9, 2026, [https://github.com/ableplayer/ableplayer/issues/251](https://github.com/ableplayer/ableplayer/issues/251)  
20. ytt \- crates.io: Rust Package Registry, accessed March 9, 2026, [https://crates.io/crates/ytt](https://crates.io/crates/ytt)  
21. Download YouTube Subtitles with yt-dlp | PDF \- Scribd, accessed March 9, 2026, [https://www.scribd.com/document/650927202/Download-a-YouTube-Video-Subtitles-File-With-Yt-dlp](https://www.scribd.com/document/650927202/Download-a-YouTube-Video-Subtitles-File-With-Yt-dlp)  
22. Convert youtube timedtext XML fromat to SRT subtitles \- gists · GitHub, accessed March 9, 2026, [https://gist.github.com/25253548ae8929ee7664](https://gist.github.com/25253548ae8929ee7664)  
23. How to use JSON subtitles : r/youtubedl \- Reddit, accessed March 9, 2026, [https://www.reddit.com/r/youtubedl/comments/1hyoreb/how\_to\_use\_json\_subtitles/](https://www.reddit.com/r/youtubedl/comments/1hyoreb/how_to_use_json_subtitles/)  
24. \[Subtitle, JSON\] Improve the format of the subtitles related json fields · Issue \#7896 · yt-dlp/yt-dlp \- GitHub, accessed March 9, 2026, [https://github.com/yt-dlp/yt-dlp/issues/7896](https://github.com/yt-dlp/yt-dlp/issues/7896)  
25. Multi-Speaker Transcripts: SRT, VTT, JSON Formats \- BrassTranscripts, accessed March 9, 2026, [https://brasstranscripts.com/blog/multi-speaker-transcript-formats-srt-vtt-json](https://brasstranscripts.com/blog/multi-speaker-transcript-formats-srt-vtt-json)  
26. AI Meeting Transcription & Speaker Identification \- Taption, accessed March 9, 2026, [https://www.taption.com/speaker](https://www.taption.com/speaker)  
27. vtt \- crates.io: Rust Package Registry, accessed March 9, 2026, [https://crates.io/crates/vtt](https://crates.io/crates/vtt)  
28. \[Youtube\] \_UnsafeExtensionError when downloading \`--sub-format\` json3, srv1, srv2, srv3 · Issue \#10360 \- GitHub, accessed March 9, 2026, [https://github.com/yt-dlp/yt-dlp/issues/10360](https://github.com/yt-dlp/yt-dlp/issues/10360)  
29. yt\_transcript\_rs \- Rust \- Docs.rs, accessed March 9, 2026, [https://docs.rs/yt-transcript-rs](https://docs.rs/yt-transcript-rs)  
30. Videos | YouTube Data API \- Google for Developers, accessed March 9, 2026, [https://developers.google.com/youtube/v3/docs/videos](https://developers.google.com/youtube/v3/docs/videos)  
31. Extract Titles and Duration from Column of YouTube Links : r/sheets \- Reddit, accessed March 9, 2026, [https://www.reddit.com/r/sheets/comments/1dyuou5/extract\_titles\_and\_duration\_from\_column\_of/](https://www.reddit.com/r/sheets/comments/1dyuou5/extract_titles_and_duration_from_column_of/)  
32. akinsella/yt-transcript-rs: 🎬️ A Rust library for accessing YouTube Video Infos & Transcripts, accessed March 9, 2026, [https://github.com/akinsella/yt-transcript-rs](https://github.com/akinsella/yt-transcript-rs)  
33. yt-transcript-rs \- crates.io: Rust Package Registry, accessed March 9, 2026, [https://crates.io/crates/yt-transcript-rs/0.1.1](https://crates.io/crates/yt-transcript-rs/0.1.1)  
34. ytranscript \- crates.io: Rust Package Registry, accessed March 9, 2026, [https://crates.io/crates/ytranscript](https://crates.io/crates/ytranscript)  
35. youtube-transcript-api \- PyPI, accessed March 9, 2026, [https://pypi.org/project/youtube-transcript-api/](https://pypi.org/project/youtube-transcript-api/)  
36. How To Get YouTube Transcript Instantly in 2025 \- DumplingAI, accessed March 9, 2026, [https://www.dumplingai.com/blog/how-to-get-youtube-transcript-instantly-in-2025](https://www.dumplingai.com/blog/how-to-get-youtube-transcript-instantly-in-2025)  
37. YouTube Transcript Downloader | Agent Skills \- Awesome MCP Servers, accessed March 9, 2026, [https://mcpservers.org/agent-skills/michalparkola/youtube-transcript](https://mcpservers.org/agent-skills/michalparkola/youtube-transcript)  
38. YouTube Transcript Generator – Fast, Accurate Video to Text \- ElevenLabs, accessed March 9, 2026, [https://elevenlabs.io/youtube-transcript-generator](https://elevenlabs.io/youtube-transcript-generator)  
39. Speaker Labels Explained: How to Obtain Speaker Names and Use Them for Accurate Transcription \- Recall.ai, accessed March 9, 2026, [https://www.recall.ai/blog/speaker-labels-and-names-explained](https://www.recall.ai/blog/speaker-labels-and-names-explained)  
40. Youtube — CATLISM | Online Compendium, accessed March 9, 2026, [https://catlism.github.io/data\_collection/social\_media\_platforms/youtube/yt-dlp\_ytcd.html](https://catlism.github.io/data_collection/social_media_platforms/youtube/yt-dlp_ytcd.html)  
41. YouTube Transcript API Not Working? Your Ultimate Fix Guide, accessed March 9, 2026, [https://transcriptapi.com/blog/youtube-transcript-api-not-working](https://transcriptapi.com/blog/youtube-transcript-api-not-working)  
42. Crawler YouTube subtitle For making an auto-response AI bot | by Mobin Shaterian, accessed March 9, 2026, [https://blog.stackademic.com/crawler-youtube-subtitle-for-making-an-auto-response-ai-bot-59ea68f3fdbb](https://blog.stackademic.com/crawler-youtube-subtitle-for-making-an-auto-response-ai-bot-59ea68f3fdbb)  
43. YouTube Transcript Generator | Extract & Download Video Transcripts, accessed March 9, 2026, [https://www.youtube-transcript.io/](https://www.youtube-transcript.io/)  
44. Best YouTube Transcript API in 2026 \- Supadata, accessed March 9, 2026, [https://supadata.ai/blog/best-youtube-transcript-api](https://supadata.ai/blog/best-youtube-transcript-api)

[image1]: <data:image/png;base64,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>

[image2]: <data:image/png;base64,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>
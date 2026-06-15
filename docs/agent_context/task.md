# Google Docs & Gemini Integration Tasks

- [x] Phase 1: Gemini Shared Conversation Links
  - [x] Identify `https://gemini.google.com/share/` URLs.
  - [x] Route to a new `GeminiGobbler`.
  - [x] Reverse-engineer the Gemini response payload (`WIZ_global_data` or similar `batchexecute` RPCs).
  - [x] Extract the human prompt and the assistant response text.
  - [x] Provide a clean Markdown layout.
  - [x] Verify extraction locally.

- [x] Phase 2: Private Google Docs 
  - [x] Abandon cookie extraction (due to Google's App-Bound Encryption and TLS fingerprinting).
  - [x] Implement the "Download Hijack" strategy.
  - [x] Attempt public fetch first to avoid unnecessary browser tabs.
  - [x] Trigger `open` (macOS), `start` (Windows), or `xdg-open` (Linux) to the `export?format=txt` URL.
  - [x] Add a polling file-watcher to `~/Downloads`.
  - [x] Detect newly downloaded `.txt` files, read their content, and auto-delete the file to clean up.
  - [x] Ensure 100% pure-Rust implementation with zero dependencies (removed `rookie` and `headless_chrome`).

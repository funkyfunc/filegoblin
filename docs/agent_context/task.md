# filegoblin Feature Checklist

## Phase I: MVP & Initialization (Completed)
- [x] Contextual Review (PRD, UX, AGENTS, README)
- [x] Project Initialization (`cargo init`, dependencies)
- [x] Configuration (`horde-check` alias)
- [x] Architecture & Documentation (`src/main.rs`, `src/lib.rs`, `README.md`)
- [x] MVP Entry Point (CLI with `clap`, ASCII mascot)
- [x] Verification (`cargo horde-check`)

## Phase II: Core Parsers (The "Stomach")
- [x] Implement `Gobble` trait for PDF (`oxidize-pdf` / mocks)
- [x] Implement `Gobble` trait for Office Docs (`docx-parser` / mocks)
- [x] Implement `Gobble` trait for Web/HTML (Heuristics for `<article>`, `/llms.txt`)
- [x] Implement `Gobble` trait for Source Code (Pure Rust minifier pending)
- [x] Implement Data Normalization (Sequence of Records for tables)
- [x] Add OCR support (WASM Asset Pipeline Integrated / Mocks)

## Phase II.b: Actual Parser Integration
- [x] Implement Central Routing (`src/lib.rs`)
- [x] Implement PDF Engine (`oxidize-pdf`)
- [x] Implement Office Engine (`docx-rs`)
- [x] Implement HTML Web Engine 
- [x] Implement Code Minification Engine (`tree-sitter-rust`)

## Phase III: Output Flavors (The Answer Key) (Completed)
- [x] Implement `human` flavor (Standard Markdown)
- [x] Implement `anthropic` flavor (XML Anchored)
- [x] Implement `gpt` flavor (Reasoning-First Markdown)
- [x] Implement `gemini` flavor (Hierarchical with ASCII map)

## Phase IV: Privacy & Security
- [x] Implement Regex-based PII/Secret scrubbing
- [x] Integrate Local SLM (ONNX Runtime, Distil-PII-1B)
- [x] Connect scrubbing to `--scrub` flag

## Phase V: CLI Flags & UX Utilities
- [x] Implement URL input fetching and ingestion
- [x] Implement recursive directory ingestion (`--horde`)
- [x] Implement recursive website crawling/ingestion
- [x] Implement token count estimation (`--tokens`)
- [x] Add clipboard support (`--copy`)
- [x] Add OS open support (`--open`)
- [x] Add target directory watch support (`--watch`)

## Phase V.5: Output Organization
- [x] Implement `--split` flag for local directory splitting
- [x] Implement `--split` flag for web crawler splitting

## Phase VI: The Interactive TUI & Vibe
- [x] Review `docs/UX.md` for interactive design guidelines and goals
- [x] Initialize `ratatui` dashboard (`-i` flag)
- [x] Implement "Hoard" Selector (Navigation, Prefix glowing, Preview Pane)
- [x] Implement The Progress Bar ("Teeth" jitter animation)
- [x] Add Error Personalities & Empty States
- [x] Add "Full" Belch Summary table

## Phase VI.5: Interactive Mode Improvements
- [x] Refactor `gobble_app` to accept `targets: &[String]` array
- [x] Add TUI Bottom Bar with Toggleable Flags (`c`, `o`, `s`, `t`)
- [x] Enable TUI Multi-File Execution returning dynamic arrays into pipeline

## Phase VI.75: Stdin Pipeline Support
- [x] Make `path` optional in the CLI
- [x] Implement `std::io::IsTerminal` pipe detection
- [x] Support `"-"` target inside `gobble_app`

## Phase VI.80: Output File Flag
- [x] Add `--write` target file flag in `cli.rs`
- [x] Pipe serialized combined outputs to the file path in `lib.rs`

## Phase VI.90: UI/UX Polish and "Delight"
- [x] Implement dynamic, multi-frame animated mascot in `main.rs`
- [x] Add color gradients to the ratatui interface in `ui.rs`
- [x] Improve the progress "teeth" animation in `ui.rs`
- [x] Update `main.rs` to print the mascot dynamically instead of a static string

## Phase VII: Extensibility
- [ ] Implement WASM Plugin System for custom parsers via `wasmtime`

## Phase VIII: Expanded Core Parsers
- [x] Presentation Gobbler (`.pptx`) via `quick-xml` or `oxml`
- [x] Image OCR Gobbler (`.png`, `.jpg`) via `ocrs`
- [x] Excel / Spreadsheet Gobbler (`.xlsx`, `.csv`)

## Phase IX: Advanced Output Structures
- [ ] Implement standard XML Output Flavor (optimized for Anthropic models)
- [ ] Implement YAML Output Flavor (optimized for strict data-schema models)

## Phase X: Token Compression & Stripping
- [x] Implement `--compress <safe|contextual|aggressive>` flag with descriptive help docs (`src/cli.rs`)
- [x] Display total LLM tokens saved in the Full Belch terminal summary

## Phase XI: Remote Data Hooks
- [ ] Native Git Cloning via `git2` (e.g., `fg https://github.com/user/repo`)
- [ ] Remote Cloud Storage Fetching (AWS S3 / GCS URIs)
- [ ] Local Database Connection String Ingestion (SQLite `.db`, Postgres)

## Phase XII: Curation & Intelligence
- [x] Intelligent Auto-Pruning (`--max-tokens 100k`) based on relevance heuristics
- [x] Semantic Search / RAG Lite (`--search "auth flow"`) using zero-dependency local inverted index
- [x] Targeted ingestion masking via glob filtering (`--include "*.rs"`)
- [x] Structural Code Skeletonization (`--extract symbols`)
- [x] Git-Aware Context Diffing (`--git-diff`)

## Phase XIII: Evaluation Feedback & UX Polish (v1.7.0)
- [x] Fix Gemini flavor duplicated `FILE_START` header
- [x] `--tokens` output labeled `tokens: N` on stderr
- [x] `--tokens-only` mode for scripting (stdout, no content)
- [x] `--exclude` glob blacklist for horde filtering
- [x] `--depth` recursion limit for horde crawling
- [x] `--manifest` table-of-contents with per-file token counts
- [x] `--diff-format` unified diff output for `--git-diff`
- [x] BM25 relevance score annotations in `--search` results
- [x] Compiler warning cleanup across all parsers

## Phase XIV: New Ingestion Sources
> 📝 Research needed before implementation: **YouTube transcripts only** (API approach TBD)
- [ ] GitHub URL ingestion (`gobble https://github.com/user/repo`) — shallow clone to tempdir via `git2`, horde-ingest, cleanup
- [ ] Jupyter Notebook gobbler (`.ipynb`) — parse JSON cells by type (code/markdown/output)
- [ ] SQLite gobbler (`.db`) — dump schema + sample rows per table via `rusqlite`
- [ ] Slack / Discord export gobbler (JSON → structured markdown)
- [ ] HTTP POST ingestion (`--post <url> -d '<body>'`) — extend existing reqwest fetcher
- [ ] YouTube transcript ingestion — **needs research** (no public API; options: `yt-dlp` subprocess, timedtext scraping, or data API)
- [ ] Google OAuth flow (`--google-login`) — access authenticated Google content (Docs, Drive, Gemini shares); **needs research** (OAuth 2.0 scopes, which APIs are needed, credential store pattern same as Twitter)

## Phase XV: Wow-Factor UX Features
- [ ] `--cost` flag — estimate API cost per model (lookup table of price/M tokens for GPT-4o, Claude, Gemini etc.)
- [ ] `--summary` flag — heuristic context preamble (scan manifest files, count module types, grab README intro)
- [ ] `--watch` mode — filesystem watcher that auto-regenerates output on save (`notify` crate)

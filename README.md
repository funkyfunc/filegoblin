# (o_o) filegoblin (filegoblin)

> **"Ingesting the messy world, spitting out clean context."**

**filegoblin** is a single-binary CLI that "gobbles" almost any file or website — PDF, Word, Excel, PowerPoint, email, EPUB, HTML, whole repos — and converts it into clean, LLM-ready markdown.

## Philosophy

- **It just works.** `filegoblin document.pdf > context.md` needs no manual. Power flags exist, but the default path is the product.
- **Format invisibility.** You shouldn't have to think about extensions; detection is extension-first with magic-byte fallback, so renamed and extension-less files route correctly.
- **Fail loudly, never silently.** An empty result exits non-zero, parse failures are reported in-band, and invalid flags are hard errors. An agent can trust every exit code.
- **Local and private.** Parsing and PII scrubbing happen entirely on your machine. No cloud APIs, no runtimes, no `pip install`.
- **Not just a concatenator.** `cat * > out.txt` exists; filegoblin adds structure (real markdown from Word, one consistent table dialect, readability-grade web extraction), token budgeting, and redaction.

---

## 🏗 Current Project Structure
*Note to Agent: Maintain this section as you add modules.*

```text
.
├── Cargo.toml        (Virtual Workspace Root)
├── crates/
│   ├── core/         (filegoblin-core: The Engine & Parsers)
│   │   ├── Cargo.toml
│   │   ├── build.rs  (Man page & shell completions)
│   │   └── src/
│   │       ├── lib.rs        (Core Pipeline Logic)
│   │       ├── cli.rs        (Clap Flag Definitions)
│   │       ├── formatters.rs (Output Assembly & Splitting)
│   │       ├── curation/     (BM25 Search & Auto-Pruning)
│   │       ├── privacy_shield.rs (PII Redaction Engine)
│   │       ├── compressor/   (Token Compression Pipeline)
│   │       ├── parsers/      (File parsing engines)
│   │       └── pipeline/     (Pipeline execution stages)
│   ├── cli/          (filegoblin: The Headless Binary)
│   │   ├── Cargo.toml
│   │   └── src/main.rs       (CLI Entry & Mascot)
│   └── tui/          (filegoblin-tui: The Interactive Dashboard)
│       ├── Cargo.toml
│       └── src/lib.rs
└── .cargo/
    └── config.toml   (Aliases)
```

---

## ⚡ Current Capabilities (v1.11.0)

Some capabilities are gated behind opt-in Cargo features (built off by default to
keep the binary lean). Feature-gated items are marked below.

- [x] Core Parsers (PDF, Word, Excel/CSV, PowerPoint, OpenDocument, Jupyter, Code, Email, RTF, HTML, EPUB) — always on
- [x] PDF → markdown via `pdf_oxide`: headings, pipe tables, reading order, hyphenation — with per-page scanned-vs-text classification
- [x] **Scanned PDFs and images OCR automatically on macOS** (Apple Vision, zero setup); Linux/Windows via `--features ocr`
- [x] Word → real markdown: heading levels, nested lists, bold/italic, hyperlinks with URLs
- [x] DOM-based HTML → markdown (readability-style content selection, link-farm pruning, preserved link URLs, title/source front-matter, ATX headings)
- [x] One tabular dialect everywhere: standard markdown pipe tables across PDF, docx, pptx, OpenDocument, and CSV/XLSX
- [x] PowerPoint: manifest-true slide order + speaker notes
- [x] Magic-byte format detection — extension-first, falling back to content signatures so renamed / extension-less files (including archives) still route correctly; legacy `.doc`/`.ppt` detected with an actionable error
- [x] URL fetching routes by Content-Type: HTML → extractor, raw markdown/JSON/text → passthrough, binary documents (a PDF URL!) → the normal parser pipeline
- [x] `--browser`: render JavaScript-built pages (SPAs) with your installed Chrome/Edge/Brave — headless, throwaway profile, no bundled browser
- [x] Recursive crawling (`--horde`) + GitHub repo cloning — **on by default** in the CLI binary (`web` feature; disable with `--no-default-features`)
- [x] Fail-loud contract for agents: exit `4` whenever nothing could be extracted, hard errors on invalid flags, quiet exit on broken pipes, explicit paths take precedence over piped stdin
- [x] MCP server (`--mcp`): stdio Model Context Protocol server exposing a `convert` tool — plug filegoblin into Claude Desktop/Code with one config line
- [x] Subtitles (`.srt`/`.vtt`): cue files → the same timestamped transcript markdown whisper produces, with speaker labels and rolling-caption dedup
- [x] RSS/Atom feeds: a feed URL or saved feed file → a clean markdown list of entries
- [x] Conversion cache: repeat URL fetches and OCR/transcription results come back instantly (`~/.filegoblin/cache`, `--no-cache` to bypass)
- [x] Audio/video transcription (`--features transcribe-metal`): mp3/m4a/wav/flac/ogg/opus/mp4/mov/webm/mkv (audio track) → timestamped transcript markdown via whisper.cpp, on-device, model auto-downloads once
- [x] Golden-file test suite: end-to-end snapshots of real fixture output per format (`cargo insta review` to inspect changes)
- [x] WASM Plugin Extensibility (`--plugin`) — requires `--features wasm`
- [x] Semantic BM25 Search with Relevance Scores (`--search`) — requires `--features rag`
- [x] Interactive Terminal Dashboard (`-i`) with ratatui — requires `--features tui` (in the CLI crate)
- [x] Glob Filtering (`--include`, `--exclude`) & Depth Control (`--depth`)
- [x] Token Estimation (`--tokens`), Token-Only Mode (`--tokens-only`)
- [x] Token Compression (`--compress safe|contextual`)
- [x] Auto-Pruning to Token Budgets (`--max-tokens`)
- [x] Manifest Table of Contents (`--manifest`)
- [x] Git-Aware Diffing (`--git-diff`) with Unified Diff Output (`--diff-format`)
- [x] Rust Symbol Extraction (`--extract symbols`) via tree-sitter
- [x] Local PII / Secret Redaction (`--scrub`) — deterministic + entropy heuristics
- [x] Output Splitting (`--split`), Chunking (`--chunk`), JSON (`--json`)
- [x] Clipboard Ingestion (`--clipboard`), Clipboard Output (`--copy`), OS Open (`--open`), File Write (`--write`)
- [x] Heuristic Project Summaries (`--summary`)
- [x] Out-of-core streaming for the default dump path (flat RAM footprint)
- [x] Pipeline-Safe Streams (`-q` for clean stdout)

---

## ✨ Key Features
- **Parsers:** `pdf_oxide`, `docx-rs`, `tree-sitter`, `quick-xml`, `calamine`, `scraper` (Statically Linked).
- **OCR built in on macOS:** Apple Vision (`objc2`) with no weights or flags — scanned PDFs and images just work. Linux gets pure-Rust `ocrs`/`rten` inference behind `--features ocr`.
- **WASM Extensibility:** `wasmtime` for pure-rust statically-linked component execution.
- **Privacy First:** 100% local PII/secret scrubbing — deterministic patterns (SSN, email, cloud/token prefixes) plus a Shannon-entropy tier that redacts high-entropy secrets. No model weights, no data egress.
- **Structural Minification:** Tree-sitter powered `--extract symbols` mode for Rust source.

| Source | Path Example | Parser Used | Output Format |
|---|---|---|---|
| **PDF** | `~/receipt.pdf` | `PdfGobbler` | Markdown (headings, pipe tables, OCR for scans) |
| **Word** | `~/notes.docx` | `OfficeGobbler` | Markdown (headings, lists, emphasis, links) |
| **Excel/CSV** | `~/data.xlsx` | `SheetGobbler` | Markdown pipe tables (header row named) |
| **PowerPoint** | `~/deck.pptx` | `PptxGobbler` | Per-slide markdown + speaker notes |
| **OpenDocument** | `~/doc.odt`, `~/deck.odp` | `OpenDocGobbler` | Markdown (headings, lists, tables) |
| **Source Code** | `~/project/src` | `CodeGobbler` | Full content (or `--extract symbols`) |
| **Email** | `~/thread.eml`, `~/inbox.mbox` | `EmailGobbler` | Header block + markdown body + attachment list |
| **RTF** | `~/notes.rtf` | `RtfGobbler` | Plain text |
| **Images** | `~/screenshot.png`, `~/IMG_0042.HEIC` | `OcrGobbler` (built-in on macOS) | OCR Text |
| **Subtitles** | `~/movie.srt`, `~/talk.vtt` | `SubtitleGobbler` | Timestamped transcript |
| **RSS/Atom** | `https://hnrss.org/frontpage` | `FeedGobbler` | Entry list (title, link, date, body) |
| **Webpages** | `https://news.ycombinator.com` | `HtmlGobbler` (default build) | DOM extraction + front-matter |

- **Semantic Search:** BM25 relevance scoring via `tantivy` with zero model weights.
- **Manifest TOC:** Auto-generated table of contents with per-file token counts.

## ⚙️ Development & Asset Pipeline

`filegoblin` maintains a strict zero-dependency profile for the end user. However, building the project requires assembling several logic assets. 

`build.rs` generates the manpage and shell completions into `target/generated/`. The `.rten` tensor weights for pure-Rust OCR fallback on Linux/Windows are **not** downloaded automatically — fetch them with the provided `justfile` when building with `--features ocr`:
```bash
just get-brains
```
(On macOS, OCR uses Apple's Vision framework natively; no weights needed.)

---

## 🚀 Quick Start

**The Goblin Horde Check:**
All code must pass the ritual before being merged:
```bash
cargo horde-check
```

**Basic Gobble (Files):**
```bash
# Output a single file to stdout
filegoblin my_notes.pdf > context.md

# Merge multiple files natively
filegoblin src/main.rs src/lib.rs README.md --write context.md
```

**URL Ingestion (Web):**
```bash
filegoblin https://example.com/api-docs > context.md
```

**Web Horde (Split into mapping directory):**
```bash
filegoblin https://bettercli.org/ --horde --split
# Translates to -> ./bettercli.org_gobbled/
```

**Privacy Shield (Redact PII & Secrets locally):**
```bash
# Typed redaction (default): values become [EMAIL], [AWS_KEY], [CREDIT_CARD], …
filegoblin ./src/api_keys.ts --scrub > safe_context.md

# Pipe a log through it as a pre-flight gate before any cloud LLM
cat prod.log | filegoblin - --scrub --scrub-style plain

# CI / pre-commit guard: non-zero exit if anything sensitive is found
git ls-files -z | xargs -0 cat | filegoblin - --scrub --scrub-fail -q || echo "secrets detected!"

# Exempt a known-public value, or add your own pattern via ./.filegoblin-scrub.json
filegoblin ./config --horde --scrub --scrub-allow "support@example.com"

# Consistent pseudonyms: the same value maps to [EMAIL_1] everywhere, so an LLM can
# still tell that an entity recurs — without ever seeing it.
cat ticket.txt | filegoblin - --scrub --scrub-style pseudonym
```
Detection is 100% local with **zero model weights**: validated rules (Luhn for cards, range
checks for SSN, mod-97 for IBAN) plus cloud/token prefixes (AWS, GitHub, Stripe, Slack, Google,
OpenAI, JWT, private-key blocks, connection strings) and a Shannon-entropy catch-all for unknown
secrets. A value-free audit tally is printed to stderr (`🛡️ Privacy Shield redacted 4 item(s): …`).
An optional `.filegoblin-scrub.json` supplies an `allowlist` and custom `patterns`.

> **What filegoblin does NOT do:** maintain a secrets blocklist. It converts exactly what you
> point it at — including a `.env` file if you name one. Deciding what's sensitive is yours:
> the directory walker respects `.gitignore` (where secrets usually already live), `--exclude`
> filters by glob, and `--scrub` redacts what does get through.

**Machine Contract (`--json`) — extract any file to clean text for agents/scripts:**
```bash
# One weird binary format → clean text + metadata, in a single call
filegoblin report.pdf --json | jq '.[0] | {format, tokens}'
# → { "format": "pdf", "tokens": 1234 }

filegoblin ./src/ --horde -q --json | jq '.[].path'
```
`--json` emits a **deterministic, alphabetically-sorted** array. A successful record is
`{ "path", "format", "tokens", "content" }` (`format` is a coarse type like `pdf`,
`spreadsheet`, `code`, `image`, `text`; `tokens` is an LLM token estimate). A file that
**fails to parse** appears in-band as `{ "path", "format", "error" }` rather than silently
vanishing — so an agent can trust that every input is accounted for. Exit codes: `0` success,
`4` when nothing could be extracted (empty result vs. total failure), `3` when
`--scrub --scrub-fail` redacted something.

**Token Compression (Reduce LLM context sizes):**
```bash
# Strip code comments, minify JSON/HTML, collapse whitespace
filegoblin ./src/main.rs --compress contextual > context.md
```

**Multi-Part Splitting (Chunk output by tokens):**
```bash
# Automatically break massive repositories into `partN` files at a 50k token threshold
filegoblin ./src/ --horde --chunk 50k
```

**Pipe from stdin:**
```bash
curl -s "https://api.github.com/users/octocat" | filegoblin -q --json
```

**Clipboard Ingestion:**
```bash
# Read contents directly from your OS clipboard
filegoblin --clipboard > context.md
```



**Development & Intelligence:**
```bash
# Get a heuristic overview of the project with token counts
filegoblin ./src/ --summary --tokens
```

**Filtering & Exclusion:**
```bash
# Only Rust files, excluding tests and generated code
filegoblin ./src/ --horde --include "*.rs" --exclude "*test*" --exclude "*generated*"

# Limit crawl depth to top-level files only
filegoblin ./src/ --horde --depth 1
```

**Manifest / Table of Contents:**
```bash
# Prepend a TOC with file paths and token counts
filegoblin ./src/ --horde --include "*.rs" --manifest
```

**Token-Only Mode (Scripting):**
```bash
# Print just the token count, no content — ideal for CI/scripts
filegoblin ./src/ --horde --tokens-only
```

**Semantic Search with Relevance Scores:**
```bash
# Search across a codebase and see BM25 relevance scores
filegoblin ./src/ --horde --search "authentication"
```

**Git-Diff Mode (Only Changed Files):**
```bash
# Ingest only files changed since last commit
filegoblin . --horde --git-diff HEAD~1

# Show unified diffs instead of full file contents
filegoblin . --horde --git-diff HEAD~1 --diff-format
```

**Interactive Hoard Selector (TUI):**
A full, snappy `ratatui` dashboard wrapper around the engine.
- Navigate with `j / k` and page with `u / d`.
- Toggle multiple files for ingestion using `Space`.
- Toggle pipeline flags directly from the bottom bar (`c` for copy, `s` for scrub secrets, etc.).
- Hit `Enter` to belch out the results!
```bash
filegoblin ./src/ -i
```

---

## 🎨 Vibe & Aesthetics
`filegoblin` embraces the **Mischievous Librarian** archetype. We don't "process" data; we **feast** on it.
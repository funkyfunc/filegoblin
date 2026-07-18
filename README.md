# (o_o) filegoblin (filegoblin)

> **"Ingesting the messy world, spitting out clean context."**

**filegoblin** is a zero-dependency, high-performance CLI tool designed to "gobble" messy file formats and convert them into high-fidelity, LLM-optimized Markdown/XML/YAML.

---

## 🏗 Current Project Structure
*Note to Agent: Maintain this section as you add modules.*

```text
.
├── Cargo.toml        (Virtual Workspace Root)
├── crates/
│   ├── core/         (filegoblin-core: The Engine & Parsers)
│   │   ├── Cargo.toml
│   │   ├── build.rs  (Man page, completions & asset fetcher)
│   │   └── src/
│   │       ├── lib.rs        (Core Pipeline Logic)
│   │       ├── cli.rs        (Clap Flag Definitions)
│   │       ├── flavors.rs    (LLM Output Flavors)
│   │       ├── curation.rs   (BM25 Search & Auto-Pruning)
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

- [x] Core Parsers (PDF, Word, Excel/CSV, PowerPoint, Jupyter, Code, Email, RTF) — always on
- [x] EPUB / Web / URL ingestion & recursive crawling (`--horde`) — requires `--features web`
- [x] Image OCR — requires `--features ocr` (macOS Vision / pure-Rust `ocrs`)
- [x] WASM Plugin Extensibility (`--plugin`) — requires `--features wasm`
- [x] Semantic BM25 Search with Relevance Scores (`--search`) — requires `--features rag`
- [x] Interactive Terminal Dashboard (`-i`) with ratatui — requires `--features tui` (in the CLI crate)
- [x] Glob Filtering (`--include`, `--exclude`) & Depth Control (`--depth`)
- [x] 4 LLM Flavors (`human`, `anthropic`, `gpt`, `gemini`)
- [x] Token Estimation (`--tokens`), Token-Only Mode (`--tokens-only`)
- [x] Token Compression (`--compress safe|contextual`)
- [x] Auto-Pruning to Token Budgets (`--max-tokens`)
- [x] Manifest Table of Contents (`--manifest`)
- [x] Git-Aware Diffing (`--git-diff`) with Unified Diff Output (`--diff-format`)
- [x] Rust Symbol Extraction (`--extract symbols`) via tree-sitter
- [x] Local PII / Secret Redaction (`--scrub`) — deterministic + entropy heuristics
- [x] Output Splitting (`--split`), Chunking (`--chunk`), JSON (`--json`)
- [x] Clipboard Ingestion (`--clipboard`), Clipboard Output (`--copy`), OS Open (`--open`), File Write (`--write`)
- [x] Heuristic Project Summaries (`--summary`) & LLM API Cost Estimation (`--cost`)
- [x] Live Auto-Regeneration Watch Mode (`--watch`)
- [x] Out-of-core streaming for the default dump path (flat RAM footprint)
- [x] Pipeline-Safe Streams (`-q` for clean stdout)

---

## ✨ Key Features
- **Parsers:** `oxidize-pdf`, `docx-rs`, `tree-sitter`, `quick-xml`, `calamine`, `ocrs` (Statically Linked).
- **Hybrid OCR:** Instantaneous macOS Vision Framework integration (`objc2`) with pure-Rust `rten` inference as fallback.
- **WASM Extensibility:** `wasmtime` for pure-rust statically-linked component execution.
- **LLM-Native:** Specific structural anchors (XML/YAML) to prevent "Attention Drift."
- **Privacy First:** 100% local PII/secret scrubbing — deterministic patterns (SSN, email, cloud/token prefixes) plus a Shannon-entropy tier that redacts high-entropy secrets. No model weights, no data egress.
- **Structural Minification:** Tree-sitter powered `--extract symbols` mode for Rust source.

| Source | Path Example | Parser Used | Output Format |
|---|---|---|---|
| **PDF** | `~/receipt.pdf` | `PdfGobbler` | Sequence of Records |
| **Word** | `~/notes.docx` | `OfficeGobbler` | Sequence of Records |
| **Excel/CSV** | `~/data.xlsx` | `SheetGobbler` | Sequence of Records |
| **Source Code** | `~/project/src` | `CodeGobbler` | Full content (or `--extract symbols`) |
| **Email** | `~/thread.eml`, `~/inbox.mbox` | `EmailGobbler` | Header block + body text |
| **RTF** | `~/notes.rtf` | `RtfGobbler` | Plain text |
| **Images** | `~/screenshot.png` | `OcrGobbler` (`--features ocr`) | OCR Text |
| **Webpages** | `https://news.ycombinator.com` | `WebGobbler` (`--features web`) | Heuristic Extraction |

- **Semantic Search:** BM25 relevance scoring via `tantivy` with zero model weights.
- **Manifest TOC:** Auto-generated table of contents with per-file token counts.

## ⚙️ Development & Asset Pipeline

`filegoblin` maintains a strict zero-dependency profile for the end user. However, building the project requires assembling several logic assets. 

This process is automated via `build.rs`. During `cargo build`, the project downloads the required `.rten` tensor arrays from the web (e.g., `text-detection.rten`) and places them cleanly in the `/assets` directory for pure-Rust OCR fallback inference on Linux/Windows. (Note: On macOS, the tool inherently uses Apple's Vision hardware, making inference weights largely optional for average use).

If you prefer to manually fetch or override these assets, a `justfile` is provided:
```bash
just get-brains
```

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

**Machine Contract (`--json`) — extract any file to clean text for agents/scripts:**
```bash
# One weird binary format → clean text + metadata, in a single call
filegoblin report.pdf --json | jq '.[0] | {format, tokens}'
# → { "format": "pdf", "tokens": 1234 }

filegoblin ./src/ --horde -q --json | jq '.[].path'
```
`--json` emits a **deterministic, alphabetically-sorted** array; each record is
`{ "path", "format", "tokens", "content" }` (`format` is a coarse type like `pdf`,
`spreadsheet`, `code`, `image`, `text`; `tokens` is an LLM token estimate). Exit codes:
`0` success, `4` when nothing could be extracted (so a caller can tell an empty result
from a total failure), `3` when `--scrub --scrub-fail` redacted something.

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
# Get a heuristic overview of the project and estimate LLM API ingest costs
filegoblin ./src/ --summary --cost --tokens

# Watch mode: continuously monitor files and dump fresh context to output file on save
filegoblin ./src/ --write context.md --watch
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
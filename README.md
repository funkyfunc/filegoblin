# (o_o) filegoblin (filegoblin)

> **"Ingesting the messy world, spitting out clean context."**

**filegoblin** is a zero-dependency, high-performance CLI tool designed to "gobble" messy file formats and convert them into high-fidelity, LLM-optimized Markdown/XML/YAML.

---

## 🏗 Current Project Structure
*Note to Agent: Maintain this section as you add modules.*

```text
.
├── Cargo.toml
├── build.rs          (Man page & shell completions generator)
├── src/
│   ├── main.rs       (CLI Entry & Mascot)
│   ├── lib.rs        (Core Pipeline Logic)
│   ├── cli.rs        (Clap Flag Definitions)
│   ├── flavors.rs    (LLM Output Flavors)
│   ├── curation.rs   (BM25 Search & Auto-Pruning)
│   ├── privacy_shield.rs (PII Redaction Engine)
│   ├── ui.rs         (Ratatui Interactive TUI)
│   ├── compressor/   (Token Compression Pipeline)
│   │   ├── mod.rs
│   │   ├── level1.rs (Safe: whitespace folding)
│   │   ├── level2.rs (Contextual: comment stripping)
│   │   ├── level3.rs (Aggressive: stopword removal)
│   │   └── heuristic.rs (Token estimation)
│   └── parsers/      (File parsing engines)
│       ├── mod.rs
│       ├── gobble.rs  (Gobble Trait)
│       ├── code.rs    (tree-sitter AST)
│       ├── pdf.rs     (oxidize-pdf)
│       ├── office.rs  (docx-rs)
│       ├── web.rs     (HTML heuristics)
│       ├── sheet.rs   (calamine xlsx/csv)
│       ├── powerpoint.rs (quick-xml pptx)
│       ├── ocr.rs     (Vision/rten hybrid)
│       ├── twitter.rs (X/Twitter GraphQL)
│       ├── crawler.rs (Recursive web crawler)
│       └── wasm.rs    (WASM plugin host)
└── .cargo/
    └── config.toml   (Aliases)
```

---

## ⚡ Current Capabilities (v1.7.0)

- [x] 10 Core Parsers (PDF, Office, Web, Code, Excel, PowerPoint, Images, Twitter, WASM Plugins)
- [x] URL Ingestion & Recursive Crawling (`--horde`)
- [x] Glob Filtering (`--include`, `--exclude`) & Depth Control (`--depth`)
- [x] 4 LLM Flavors (`human`, `anthropic`, `gpt`, `gemini`)
- [x] Token Estimation (`--tokens`), Token-Only Mode (`--tokens-only`)
- [x] Token Compression & Stripping (`--compress safe|contextual|aggressive`)
- [x] Auto-Pruning to Token Budgets (`--max-tokens`)
- [x] Semantic BM25 Search with Relevance Scores (`--search`)
- [x] Manifest Table of Contents (`--manifest`)
- [x] Git-Aware Diffing (`--git-diff`) with Unified Diff Output (`--diff-format`)
- [x] Code Skeletonization (`--extract symbols`) via tree-sitter
- [x] PII Redaction (`--scrub`)
- [x] Output Splitting (`--split`), Chunking (`--chunk`), JSON (`--json`)
- [x] Clipboard Ingestion (`--clipboard`), Clipboard Output (`--copy`), OS Open (`--open`), File Write (`--write`)
- [x] Heuristic Project Summaries (`--summary`) & LLM Core API Cost Estimation (`--cost`)
- [x] Live Auto-Regeneration Watch Mode (`--watch`)
- [x] Interactive Terminal Dashboard (`-i`) with ratatui
- [x] Pipeline-Safe Streams (`-q` for clean stdout)
- [x] WASM Plugin Extensibility (`--plugin`)
- [x] Cloud Resource Ingestion (Google Docs, Drive, Gemini, Twitter URL parsing + Authentication)

---

## ✨ Key Features
- **Parsers:** `oxidize-pdf`, `docx-rs`, `tree-sitter`, `quick-xml`, `calamine`, `ocrs` (Statically Linked).
- **Hybrid OCR:** Instantaneous macOS Vision Framework integration (`objc2`) with pure-Rust `rten` inference as fallback.
- **WASM Extensibility:** `wasmtime` for pure-rust statically-linked component execution.
- **LLM-Native:** Specific structural anchors (XML/YAML) to prevent "Attention Drift."
- **Privacy First:** Local-only PII/Secret scrubbing using Distil-PII-1B.
- **Structural Minification:** Tree-sitter powered `--extract symbols` mode for source code.
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
filegoblin ./src/api_keys.ts --scrub > safe_context.md
```

**Scripting Pipeline (JSON & Quiet):**
```bash
filegoblin ./src/ --horde -q --json | jq '.[].path'
```

**Token Compression (Reduce LLM context sizes):**
```bash
# Strip comments from code, remove stop words, collapse whitespace
filegoblin ./src/main.rs --compress aggressive > context.md
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

**Cloud Resources (Google Docs/Drive/Gemini):**
```bash
# Authenticate once to securely cache PKCE token and provide session cookies
filegoblin --google-login

# Ingest Google Docs/Drive items or Gemini share links directly
filegoblin https://docs.google.com/document/d/1X...
filegoblin https://gemini.google.com/share/...
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
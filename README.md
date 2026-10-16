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

## 📦 Installation

Every release comes in two flavours:

- **with the PDF vision models** for layout and reading order, formulas as LaTeX, tables, and OCR on scanned pages (~510 MB);
- **lite**, without them: the fast geometric converter, plus Apple Vision OCR on macOS.

Everything else is the same.

| Channel | Lite | With the models |
|---|---|---|
| GitHub release ([latest](https://github.com/funkyfunc/filegoblin/releases/latest)) | `filegoblin-<target>` archive | `filegoblin-full-<target>` archive: one binary with the models compiled in |
| PyPI | `pip install filegoblin` | `pip install "filegoblin[models]"` |
| npm | `npm i -g filegoblin` | `npm i -g filegoblin @filegoblin/models` |
| Cargo | `cargo install --git https://github.com/funkyfunc/filegoblin filegoblin` | then `filegoblin --install-models` |

Builds are published for macOS (Apple Silicon and Intel), Linux (x86_64 and aarch64), and Windows (x86_64).

**Getting the models later.**
- `filegoblin --install-models` downloads this version's model archive from the GitHub release, checks its SHA-256 and unpacks it into `~/.filegoblin/models`. You can pass a directory instead, and set `FILEGOBLIN_MODELS_URL` to point at an internal mirror. filegoblin only downloads when you ask it to.
- `filegoblin --models-status` shows where each model was found.

**Behind a corporate network.** The PyPI and npm model packages carry the weights themselves, split into pieces under the registries' 100 MB per-file limit. They install from a company mirror (Artifactory, Nexus, CodeArtifact) like any other package, with no access to github.com.

**Air-gapped machines.** Copy the full download. Alternatively, put the `filegoblin-vision-models.tar.gz` archive on a shared read-only path and name it in `FILEGOBLIN_MODELS`. That variable takes a `PATH`-style list. Licenses are in `LICENSES.md` inside the archive.

**From source** — needs a Rust toolchain (1.87+):

```bash
cargo install --git https://github.com/funkyfunc/filegoblin filegoblin
```

That gives you everything in the "always on" list below, including web fetching and
crawling. The optional engines are opt-in because each one pulls in a heavy
dependency:

| Want | Add | Build needs |
|---|---|---|
| OCR on Linux/Windows | `--features ocr` | `just get-brains` for the weights (macOS uses Apple Vision, no flag) |
| PDF vision models (layout, reading order, formulas → LaTeX, tables, scan OCR) | `--features vision` (on in release builds) | The weights: `filegoblin --install-models` (the release's `filegoblin-vision-models.tar.gz`, SHA-256 checked) or `just build-vision-models`. filegoblin finds them in `models/` next to the binary, in `~/.filegoblin/models`, or in `$FILEGOBLIN_MODELS`. Add `bundled-models` with `FILEGOBLIN_MODELS_DIR` set to compile them into one binary |
| Audio/video transcription | `--features transcribe` | `cmake` + a C/C++ compiler; downloads a ~574 MB model on first use |
| Metal-accelerated transcription (macOS only) | `--features transcribe-metal` | Xcode Command Line Tools. **Does not build on Linux/Windows** |
| WASM plugins | `--features wasm` | — |
| BM25 search (`--search`) | `--features rag` | — |

```bash
# Example: a macOS build with GPU-accelerated transcription
cargo install --git https://github.com/funkyfunc/filegoblin filegoblin --features transcribe-metal
```

Without the models, `--vision smart` (the default) behaves exactly like the fast converter, with one note per run on stderr; `--vision off` silences it. With the models, it uses them only on the pages that need them. Anyone building from source can make a single self-contained binary with `--features vision,bundled-models` and `FILEGOBLIN_MODELS_DIR` set.

---

**Measuring quality.** `just gauntlet` runs the 29-case adversarial gauntlet
(`just gauntlet off` for the fast path). Each case gets pass/fail canaries and
a 0–100 quality score graded against hand-checked gold excerpts in
`test_assets/gauntlet/gold/`, plus checks that `--format plain` and
`--chunk-size` lose nothing. `just gauntlet-compare A B` shows what a change
did case by case. Results are in `GAUNTLET_SCORECARD.md`.

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
│   │       ├── formatters/   (Plain text & specialized formatters)
│   │       ├── chunking/     (Layout-aware semantic chunking engine)
│   │       ├── curation/     (BM25 Search & Auto-Pruning)
│   │       ├── privacy_shield.rs (PII Redaction Engine)
│   │       ├── compressor/   (Token Compression Pipeline)
│   │       ├── parsers/      (File parsing engines; pdf_vision.rs = layout-guided PDF assembly)
│   │       ├── vision/       (Bundled ONNX models on rten: layout, formula, table, OCR; hayro rendering)
│   │       └── pipeline/     (Pipeline execution stages)
│   └── cli/          (filegoblin: The Headless Binary)
│       ├── Cargo.toml
│       └── src/main.rs       (CLI Entry & Mascot)
│           └── walk.rs       (--walk interactive picker)
├── npm/              (npx filegoblin Zero-Install Launcher)
│   ├── package.json
│   └── bin/filegoblin.js
└── .cargo/
    └── config.toml   (Aliases)
```

---

## ⚡ Current Capabilities (v1.15.0)

Some capabilities are gated behind opt-in Cargo features (built off by default to
keep the binary lean). Feature-gated items are marked below.

- [x] **Multi-Format Output Engine (`--format markdown|plain|json` / `-f`)**: GFM markdown (default), syntax-stripped plain text with space-aligned tables for vector embeddings (e.g. OpenAI `text-embedding-3`, BGE) and BM25 search, and structured JSON documents
- [x] **Layout-Aware Semantic Chunking (`--chunk-size`, `--chunk-overlap`, `--chunk-strategy layout|tokens`)**: splits along structural headings (H1 > H2 > H3), protects tables and code blocks from mid-block slicing, automatically replicates table headers across chunk splits, and embeds hierarchical outline breadcrumbs
- [x] Core Parsers (PDF, Word, Excel/CSV, PowerPoint, OpenDocument, Jupyter, Code, Email, RTF, HTML, EPUB) — always on
- [x] PDF → markdown via `pdf_oxide`: headings, pipe tables, reading order, hyphenation — with per-page scanned-vs-text classification
- [x] **Scanned PDFs and images OCR automatically on macOS** (Apple Vision, zero setup; parallelized page conversion, 30+ pages/s); Linux/Windows via `--features ocr`
- [x] **PDF vision pipeline** (`--features vision` + model weights; `--vision smart|all|off`, default `smart`): pages are rendered in pure Rust (`hayro`) and read by bundled, permissively licensed models on `rten` — no Python, system libraries or LLM at runtime. PP-DocLayoutV3 places every text-layer word in its region (title, heading, paragraph, caption, footnote, table, figure, running header/footer) in a learned reading order; display and inline formulas are re-read from the image as LaTeX (pix2text MFR); tables get rows/columns/spanning cells from Table Transformer and their cell values from the text layer; scanned pages, text regions the text layer leaves empty, and scans whose own OCR layer is too damaged to keep are read with PP-OCRv6 (medium). `--vision smart` decides per page from cheap text-layer facts — scans, math fonts/symbols, column-aligned tables and forms, multi-column text, pictures, one- or two-page documents go through the models; plain single-column prose stays on the fast path. On the full olmOCR-bench (1,403 PDFs, official scorer) the models take filegoblin from 33.2% to 72.3% (arXiv math 0% → 84%, tables 26% → 75%, multi-column 46% → 71%, headers/footers 42% → 92%)
- [x] **Geometric PDF page furniture removal**: running headers/footers, page numbers, and line-number gutters (pleading paper, patents) are identified from position, cross-page recurrence, font size and alignment — before conversion — so prose, numbers and headings are never rewritten by text heuristics. Line-break hyphenation and words run together across visible gaps are repaired from page geometry plus the document's own vocabulary
- [x] **Semantic Chapter Boundaries**: Automated detection and injection of thematic dividers and XML section markers (`<!-- filegoblin:section ... -->`)
- [x] Word → real markdown: heading levels, nested lists, bold/italic, hyperlinks with URLs, and equations
- [x] **Word Resilient Fallback**: Streaming OpenXML reader handling `<w:gridSpan>` merged tables and `<mc:AlternateContent>` floating text boxes
- [x] **Word Office Math (OMML)**: `<m:oMath>` and `<m:oMathPara>` converted directly to standard LaTeX (`$ ... $` and `$$ ... $$`) with radicals, delimiters, matrices, and functions
- [x] **Embedded Office Charts**: PowerPoint (`.pptx`) and Word (`.docx`) OpenXML DrawingML charts (`c:chartSpace`) deconstructed into clean Markdown pipe tables, Mermaid `xychart-beta`, and native Mermaid `pie showData` diagrams
- [x] **DrawingML Connectors & SmartArt**: PowerPoint and Word shape connections, flowcharts, and SmartArt process hierarchies compiled into Mermaid `flowchart` diagrams
- [x] **Zero-install npm distribution**: Execute via `npx filegoblin` in environments without Rust toolchains
- [x] DOM-based HTML → markdown (readability-style content selection, link-farm pruning, preserved link URLs, title/source front-matter, ATX headings)
- [x] One tabular dialect everywhere: standard markdown pipe tables across PDF, docx, pptx, OpenDocument, and CSV/XLSX
- [x] **Pure Rust Ruling-Line Table Extraction**: Native port of `pdfplumber.table` vector ruling-line and intersection table algorithm, supporting collinear edge snapping, bounding box cycle reconstruction, multi-column merged headers, and fallback gutter detection
- [x] **Trafilatura DOM Content Extraction**: Native port of `trafilatura.core` text/link-density heuristics, boilerplate token pruning, syntax-highlighted code block preservation, and citation-link dampening
- [x] PowerPoint: manifest-true slide order + speaker notes + embedded chart tables
- [x] Magic-byte format detection — extension-first, falling back to content signatures so renamed / extension-less files (including archives) still route correctly; legacy `.doc`/`.ppt` detected with an actionable error
- [x] URL fetching routes by Content-Type: HTML → extractor, raw markdown/JSON/text → passthrough, binary documents (a PDF URL!) → the normal parser pipeline
- [x] `--browser`: render JavaScript-built pages (SPAs) with your installed Chrome/Edge/Brave — headless, throwaway profile, no bundled browser
- [x] Recursive crawling (`--horde`) + GitHub repo cloning — **on by default** in the CLI binary (`web` feature; disable with `--no-default-features`)
- [x] Fail-loud contract for agents: exit `4` whenever nothing could be extracted, hard errors on invalid flags, quiet exit on broken pipes, explicit paths take precedence over piped stdin
- [x] MCP server (`--mcp`): stdio Model Context Protocol server exposing a `convert` tool — plug filegoblin into Claude Desktop/Code with one config line
- [x] Subtitles (`.srt`/`.vtt`): cue files → the same timestamped transcript markdown whisper produces, with speaker labels and rolling-caption dedup
- [x] RSS/Atom feeds: a feed URL or saved feed file → a clean markdown list of entries
- [x] Conversion cache: repeat URL fetches and OCR/transcription results come back instantly (`~/.filegoblin/cache`, `--no-cache` to bypass)
- [x] Audio/video transcription (`--features transcribe`; add `transcribe-metal` on macOS for GPU): mp3/m4a/wav/flac/ogg/opus/mp4/mov/webm/mkv (audio track) → timestamped transcript markdown via whisper.cpp, on-device, model auto-downloads once
- [x] Golden-file test suite: end-to-end snapshots of real fixture output per format (`cargo insta review` to inspect changes)
- [x] WASM Plugin Extensibility (`--plugin`) — requires `--features wasm`
- [x] Semantic BM25 Search with Relevance Scores (`--search`) — requires `--features rag`
- [x] Conversion cache management: `--no-cache` to bypass, `--cache-clear` to empty it
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
| **Word** | `~/notes.docx` | `OfficeGobbler` | Markdown (headings, lists, emphasis, links, equations, charts) |
| **Excel/CSV** | `~/data.xlsx` | `SheetGobbler` | Markdown pipe tables (header row named) |
| **PowerPoint** | `~/deck.pptx` | `PptxGobbler` | Per-slide markdown + speaker notes + embedded charts |
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

**Multi-Format Output (`--format markdown|plain|json` / `-f`):**
```bash
# Default GFM markdown with tables, math, and code fences
filegoblin document.pdf -f markdown > doc.md

# Strip markdown punctuation for dense vector embeddings & BM25 search
# (Pipe tables become space-aligned text columns; headings & math stripped of tokens)
filegoblin document.pdf -f plain > embedding_input.txt

# Structured JSON document array for agent pipelines
filegoblin document.pdf -f json > document.json
```

**Layout-Aware Semantic Chunking (`--chunk-size`, `--chunk-overlap`, `--chunk-strategy`):**
```bash
# Split along structural boundaries (H1/H2/H3, tables, code blocks) with 512 token budget
# Replicates table headers across chunk boundaries and embeds hierarchical breadcrumbs
filegoblin document.pdf --chunk-size 512 > chunks.md

# Structured JSON chunks with breadcrumbs and token estimates for RAG vectors
filegoblin document.pdf -f json --chunk-size 512 --chunk-overlap 64 > rag_chunks.json
```

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

**Scoping a crawl (`--depth`, `--include`, `--exclude`):**

These apply to both halves of `--horde` — local file paths, and the URL *path*
when crawling. Out-of-scope pages are never fetched, so scoping saves requests
rather than just trimming the output.

```bash
# Only the guide section, and never the changelog
filegoblin https://docs.example.com --horde --include "/guide/*" --exclude "*changelog*"

# The seed page and whatever it links to directly — nothing further
filegoblin https://docs.example.com --horde --depth 1

# Shorthand: a bare word matches anywhere in the path
filegoblin https://docs.example.com --horde --include guide
```

The seed URL is always fetched, so `--include "/guide/*"` starting at `/` still
works. `--depth 0` is the seed alone.

**Walk and pick (`--walk`) — for when you don't want the whole site:**

Pointing `--horde` at a website converts everything it can reach: the changelog,
the 2019 blog archive, every language variant. `--walk` opens a picker instead —
read each page, follow its links, mark what you want, convert only that.

```bash
filegoblin --walk https://docs.example.com > context.md
filegoblin --walk ./my-repo > context.md
```

```
 https://docs.example.com/guide/     8.4k tok  ↳1
 # Getting Started

 Install the CLI, then point it at a file. Every
 conversion happens locally on your machine.

 ─ pick ──────────────────────────────────────────
 ❯ ◉ this page — Getting Started
   ○ Next: Configuration
   ○ API Reference

  1 selected   ↑↓ move · space keep · ⏎ follow · b back · d done · q quit
```

The preview is the actual converted markdown, so you're judging what you'd
get, not a guess. Links are scoped to the same origin (host *and* port), and
selecting a directory includes everything beneath it. Pages you revisit come
from the conversion cache, so backing up is instant.

The picker draws on stderr and the markdown goes to stdout, so redirecting works
normally. It needs a terminal — piping without one is a clear error rather than
a hang, and every other invocation is unchanged, so scripts, agents and the MCP
server are unaffected.

**MCP Server (Claude Desktop / Claude Code):**

`filegoblin --mcp` speaks the Model Context Protocol over stdio, exposing a single
`convert` tool that takes a file path, directory, or URL.

For **Claude Code**:
```bash
claude mcp add filegoblin -- filegoblin --mcp
```

For **Claude Desktop**, add this to `claude_desktop_config.json`
(macOS: `~/Library/Application Support/Claude/`, Windows: `%APPDATA%\Claude\`):
```json
{
  "mcpServers": {
    "filegoblin": {
      "command": "filegoblin",
      "args": ["--mcp"]
    }
  }
}
```

Use an absolute path to the binary if it isn't on the PATH your client sees. To let
the tool reach services on your own machine, add
`"env": { "FILEGOBLIN_ALLOW_LOCAL_URLS": "1" }` — see the security note below for
why that is off by default.

---

## 🗺️ Roadmap

- [ ] **Unified Output Formats (`--format`):**
  - `--format markdown` / `--format md` (default): Clean, token-optimized Markdown for LLM context windows.
  - `--format plain` (or `--plain`): Syntax-stripped prose for text-embedding models (OpenAI, Cohere, Voyage) and token counters.
  - `--format json` (or `--json`): Structured AST array of typed semantic blocks (`heading`, `paragraph`, `table`, `code`, `math`, `callout`) with bounding box and page metadata for RAG ingestors.
- [ ] **Semantic Section Chunking (`--chunk-tokens`, `--chunk-by`):**
  - Chunk documents into LLM context-ready token budgets (`--chunk-tokens 512`) without severing tables, code blocks, or mathematical equations across chunk boundaries.
  - Boundary-aware splitting (`--chunk-by section|chapter|page`).
- [ ] **Multi-Channel Distribution:**
  - **PyPI (`pip install filegoblin`):** Python wheels built via Maturin for instant drop-in use in LangChain, LlamaIndex, and PyTorch pipelines.
  - **Crates.io (`cargo install filegoblin`):** Publish CLI binary and `filegoblin-core` library crate.
  - **Homebrew Tap (`brew install funkyfunc/tap/filegoblin`):** Zero-dependency native macOS installation.
  - **Precompiled Binary Releases:** Automated cross-platform release archives (Linux x64/arm64, macOS Apple Silicon/Intel, Windows x64) via `cargo-dist`.

---

## 🔒 Security notes

- **URLs pointing at private addresses are refused.** Loopback, private ranges,
  link-local (including the cloud metadata endpoint `169.254.169.254`), and any
  hostname that *resolves* to one are blocked — on the initial URL, on every
  redirect hop, and inside the DNS resolver itself, so a name that resolves
  differently between the check and the connection (DNS rebinding) is refused
  too. Set `FILEGOBLIN_ALLOW_LOCAL_URLS=1` to allow them — reasonable for
  converting your own dev server, and worth thinking about before enabling it
  for the MCP server, where the URL comes from a model.
- **`--browser` is a wider surface.** Chrome does not honour those redirect
  guards, so a page it renders can redirect somewhere filegoblin would refuse to
  fetch directly. The initial URL is still validated, the profile is a throwaway
  temp dir, and the mode is strictly opt-in.
- **Archives and Office files have decompression caps.** Zip, tar.gz, and the
  zip-container formats (`.docx`/`.pptx`/`.odt`/`.epub`) bound what they inflate
  and refuse entries whose paths escape the extraction directory.

---

## 🛠 Contributing

All code must pass the ritual before being merged:
```bash
cargo horde-check   # clippy -D warnings + fmt --check + test, via .cargo/config.toml
```

Golden-file tests live in `crates/cli/tests/` and use `insta` snapshots; run
`cargo insta review` to inspect a diff before accepting it.

---

## 🎨 Vibe & Aesthetics
`filegoblin` embraces the **Mischievous Librarian** archetype. We don't "process" data; we **feast** on it.
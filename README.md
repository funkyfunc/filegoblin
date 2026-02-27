# (o_o) filegoblin (fg)

> **"Ingesting the messy world, spitting out clean context."**

**filegoblin** is a zero-dependency, high-performance CLI tool designed to "gobble" messy file formats and convert them into high-fidelity, LLM-optimized Markdown/XML/YAML.

---

## 🏗 Current Project Structure
*Note to Agent: Maintain this section as you add modules.*

```text
.
├── Cargo.toml
├── src/
│   ├── main.rs       (CLI Entry)
│   ├── lib.rs        (Core Logic)
│   └── parsers/      (File parsing engines)
│       ├── mod.rs
│       └── gobble.rs (Gobble Trait)
└── .cargo/
    └── config.toml   (Aliases)
```

---

## ⚡ Current Capabilities
*Note to Agent: Document new flags and parsers here.*

- [x] Basic Rust Project Initialization
- [x] `cargo horde-check` Alias Configuration
- [x] MVP CLI Setup with `clap`
- [x] Core Library scaffolding & `Gobble` trait
- [x] Initial Core Parsers (PDF, Office, Web, Code) with TDD Mocks
- [x] Add Image OCR support (Apple Vision Native Hooks & `ocrs` fallback)
- [x] URL Ingestion Support & Recursive Crawling (`--horde`)
- [x] Output Splitting & Auto-Directory Mapping (`--split`)
- [x] Pipeline Isolation (`--quiet`) & Structured Data (`--json`)
- [x] Token Estimation (`--tokens`)
- [x] PII Redaction (`--scrub`)
- [x] Token Compression & Stripping (`--compress`)
- [x] Interactive Terminal Dashboard (`-i`)
- [x] The "Full Belch" Output Summary Table

---

## ✨ Key Features
- **Parsers:** `oxidize-pdf`, `docx-rs`, `tree-sitter`, `quick-xml`, `calamine` (Statically Linked).
- **Hybrid OCR:** Instantaneous macOS Vision Framework integration (`objc2`) with pure-Rust `rten` inference as fallback.
- **WASM Extensibility:** `wasmtime` for pure-rust statically-linked component execution.
- **LLM-Native:** Specific structural anchors (XML/YAML) to prevent "Attention Drift."
- **Privacy First:** Local-only PII/Secret scrubbing using Distil-PII-1B.
- **Structural Minification:** Tree-sitter powered `--skeleton` mode for source code.

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
fg my_notes.pdf > context.md

# Merge multiple files natively
fg src/main.rs src/lib.rs README.md --write context.md
```

**URL Ingestion (Web):**
```bash
fg https://example.com/api-docs > context.md
```

**Web Horde (Split into mapping directory):**
```bash
fg https://bettercli.org/ --horde --split
# Translates to -> ./bettercli.org_gobbled/
```

**Privacy Shield (Redact PII & Secrets locally):**
```bash
fg ./src/api_keys.ts --scrub > safe_context.md
```

**Scripting Pipeline (JSON & Quiet):**
```bash
fg ./src/ --horde -q --json | jq '.[].path'
```

**Token Compression (Reduce LLM context sizes):**
```bash
# Strip comments from code, remove stop words, collapse whitespace
fg ./src/main.rs --compress aggressive > context.md
```

**Multi-Part Splitting (Chunk output by tokens):**
```bash
# Automatically break massive repositories into `partN` files at a 50k token threshold
fg ./src/ --horde --chunk 50k
```

# Pipe directly from other programs using stdin
curl -s "https://api.github.com/users/octocat" | fg -q --json

**Interactive Hoard Selector (TUI):**
A full, snappy `ratatui` dashboard wrapper around the engine.
- Navigate with `j / k` and page with `u / d`.
- Toggle multiple files for ingestion using `Space`.
- Toggle pipeline flags directly from the bottom bar (`c` for copy, `s` for scrub secrets, etc.).
- Hit `Enter` to belch out the results!
```bash
fg ./src/ -i
```

---

## 🎨 Vibe & Aesthetics
`filegoblin` embraces the **Mischievous Librarian** archetype. We don't "process" data; we **feast** on it.
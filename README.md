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
- [x] Core Parsers (PDF, Office, Web, Code) with TDD Mocks
- [x] URL Ingestion Support & Recursive Crawling (`--horde`)
- [x] Token Estimation (`--tokens`)
- [ ] Output Flavors (Human, GPT, Claude, Gemini)
- [ ] PII Redaction (Local SLM)

---

## ✨ Key Features
- **Parsers:** `oxidize-pdf`, `docx-rs`, `tree-sitter` (Statically Linked).
- **WASM Extensibility:** `wasmtime` for pure-rust statically-linked component execution.
- **LLM-Native:** Specific structural anchors (XML/YAML) to prevent "Attention Drift."
- **Privacy First:** Local-only PII/Secret scrubbing using Distil-PII-1B.
- **Structural Minification:** Tree-sitter powered `--skeleton` mode for source code.

## ⚙️ Development & Asset Pipeline

`filegoblin` maintains a strict zero-dependency profile for the end user. However, building the project requires assembling several WASM engines. 

This process is completely automated via `build.rs`. During `cargo build`, the project will automatically fetch the pinned `tesseract-core-simd` WASM module (v6.1.2) from the web and place it in the `/assets` directory so that `include_bytes!` can statically link it into the final binary. 

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

**Basic Gobble (File):**
```bash
fg my_notes.pdf > context.md
```

**URL Ingestion (Web):**
```bash
fg https://example.com/api-docs > context.md
```

---

## 🎨 Vibe & Aesthetics
`filegoblin` embraces the **Mischievous Librarian** archetype. We don't "process" data; we **feast** on it.
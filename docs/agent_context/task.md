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
- [x] Add `--out` target file flag in `cli.rs`
- [x] Pipe serialized combined outputs to the file path in `lib.rs`

## Phase VII: Extensibility
- [ ] Implement WASM Plugin System for custom parsers via `wasmtime`

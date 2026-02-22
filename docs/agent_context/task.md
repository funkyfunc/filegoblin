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
- [ ] Implement Regex-based PII/Secret scrubbing
- [ ] Integrate Local SLM (ONNX Runtime, Distil-PII-1B)
- [ ] Connect scrubbing to `--scrub` flag

## Phase V: CLI Flags & UX Utilities
- [x] Implement URL input fetching and ingestion
- [ ] Implement recursive directory ingestion (`--horde`)
- [ ] Implement token count estimation (`--tokens`)
- [ ] Add clipboard support (`--copy`)
- [ ] Add OS open support (`--open`)
- [ ] Add target directory watch support (`--watch`)

## Phase VI: The Interactive TUI & Vibe
- [ ] Review `docs/UX.md` for interactive design guidelines and goals
- [ ] Initialize `ratatui` dashboard (`-i` flag)
- [ ] Implement "Hoard" Selector (Navigation, Prefix glowing, Preview Pane)
- [ ] Implement The Progress Bar ("Teeth" jitter animation)
- [ ] Add Error Personalities & Empty States
- [ ] Add "Full" Belch Summary table

## Phase VII: Extensibility
- [ ] Implement WASM Plugin System for custom parsers via `wasmtime`

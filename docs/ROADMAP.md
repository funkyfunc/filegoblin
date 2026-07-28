# Filegoblin Roadmap & Strategic Enhancements

This document tracks the strategic trajectory and planned enhancements for `filegoblin`. These capabilities ensure we maintain absolute market dominance as the premier, zero-dependency context ingestion tool for LLMs.

---

## ✅ Recently Completed Enhancements

### 1. Geometric Heuristic PDF Table Extraction
**Status: COMPLETED**
**Implementation:** Implemented via `oxidize-pdf`'s native Rust text/graphics extraction (`crates/core/src/parsers/pdf.rs`). By analyzing the bounding boxes and text coordinates within the PDF vector stream, we heuristically reconstruct tables and output them as a perfect "Sequence of Records", completely bypassing heavy ML vision models.

### 2. Local Regex/Entropy PII Redaction
**Status: COMPLETED (honest, non-neural)**
**Implementation:** `crates/core/src/privacy_shield.rs`. A two-tier engine: deterministic Aho-Corasick keywords + regexes (SSN, email, credit card, and cloud/token prefixes such as AWS/GitHub/OpenAI/JWT), followed by a Shannon-entropy tier that redacts unbroken high-entropy tokens (keys, hashes). No model weights, no data egress. NOTE: an earlier "neural"/SLM tier was a hardcoded mock and has been removed; this engine makes no ML claims.

### 3. Out-of-Core Streaming (default dump path)
**Status: PARTIAL.** The default output path (`stream_and_output`) streams documents one at a time to stdout/file with a flat RAM footprint. Whole-output operations (`--search`, `--max-tokens`, `--copy`, `--open`, `--cost`, `--tokens-only`, `--summary`, `--manifest`) intentionally buffer the full corpus because they need it in memory.

---

## 🚀 Upcoming Enhancements (To-Do)

### 1. Full Out-of-Core Streaming for Budgeting
**Status:** `--max-tokens` currently buffers the whole corpus to rank documents (`crate::curation::enforce_budget`). A streaming/approximate budgeter could restore a flat footprint for budgeted runs.

### 2. (Removed) Dependency-Graph PageRank Pruning
**Status: REMOVED.** A PageRank pruner (`curation/pagerank.rs`, `petgraph`) was built but its "File A contains File B's stem name ⇒ dependency" heuristic produced garbage edges (e.g. `error.rs` matched every file mentioning "error") and it never wired into `enforce_budget`. It was deleted in favor of the single extension/size-ranked `enforce_budget`. Any future importance-based pruning should use real AST import resolution, not substring matching.

---

## 🛠 Active Capabilities (Do Not Lose)
*If refactoring or rewriting the engine, the following core features must be preserved:*

- **Zero-Dependency Native Execution:** Pure Rust, statically linked binary.
- **Structural Minification:** Tree-sitter powered `--extract symbols` for codebase skeletonization.
- **Semantic Pruning & Search:** BM25 relevance scoring (`--search`) via Tantivy.
- **WASM Plugin Extensibility:** Sandboxed third-party parsers via `wasmtime`.
- **Horde Mode:** Recursive web crawling (`--horde`) and output chunking (`--chunk`).
- **Data Normalization:** "Sequence of Records" generation for spreadsheet and PDF tabular data.

# Filegoblin Roadmap & Strategic Enhancements

This document tracks the strategic trajectory and planned enhancements for `filegoblin`. These capabilities ensure we maintain absolute market dominance as the premier, zero-dependency context ingestion tool for LLMs.

---

## ✅ Recently Completed Enhancements

### 1. Geometric Heuristic PDF Table Extraction
**Status: COMPLETED**
**Implementation:** Implemented via `oxidize-pdf`'s native Rust text/graphics extraction (`crates/core/src/parsers/pdf.rs`). By analyzing the bounding boxes and text coordinates within the PDF vector stream, we heuristically reconstruct tables and output them as a perfect "Sequence of Records", completely bypassing heavy ML vision models.

### 2. Zero-Latency Regex/Entropy PII Redaction
**Status: COMPLETED**
**Implementation:** Implemented in `crates/core/src/privacy_shield.rs`. A highly parallelized engine utilizing `rayon` alongside a `tokio::sync::Semaphore`. It applies deterministic matching (Aho-Corasick) followed by heuristic Shannon entropy scans to redact API keys and PII with near-zero latency overhead.

---

## 🚀 Upcoming Enhancements (To-Do)

Based on competitive analysis against Python/Node.js alternatives (Repomix, Aider, Docling), the following major architectural enhancements are targeted to solve the industry's most painful context-generation bottlenecks.

### 1. True "Out-of-Core" Stream Processing
**The Problem:** Competitors (like V8/Node-based tools) crash with Out-Of-Memory (OOM) errors on enterprise monorepos because they load entire parsed ASTs into RAM simultaneously. Our current orchestrator (`crates/core/src/routing.rs`) also accumulates all outputs into memory before formatting.
**The Solution:** Refactor the core ingestion engine to utilize strict out-of-core streaming (lazy iterators and asynchronous pipelines). filegoblin must stream file paths, parse via `rayon`, compress, and flush output to disk sequentially, guaranteeing a flat, minimal RAM footprint even on repositories with 100,000+ files.

### 2. Dependency Graph "PageRank" for Heuristic Pruning
**The Problem:** When massive repositories exceed LLM token limits, naive truncation randomly drops critical architectural context.
**The Solution:** Adapt a Rayon-parallelized PageRank algorithm over our Tree-sitter ASTs. By building a structural dependency graph (tracking which files import/reference which symbols) and running PageRank natively in Rust, we can mathematically determine the most "important" files. When the `--max-tokens` limit is hit, filegoblin intelligently prunes the least-referenced files first, preserving the core architecture.

---

## 🛠 Active Capabilities (Do Not Lose)
*If refactoring or rewriting the engine, the following core features must be preserved:*

- **Zero-Dependency Native Execution:** Pure Rust, statically linked binary.
- **Model-Specific Flavors:** Support for `human`, `anthropic` (XML), `gpt`, and `gemini` (Breadcrumbs/ASCII map).
- **Structural Minification:** Tree-sitter powered `--extract symbols` for codebase skeletonization.
- **Semantic Pruning & Search:** BM25 relevance scoring (`--search`) via Tantivy.
- **WASM Plugin Extensibility:** Sandboxed third-party parsers via `wasmtime`.
- **Horde Mode:** Recursive web crawling (`--horde`) and output chunking (`--chunk`).
- **Data Normalization:** "Sequence of Records" generation for spreadsheet and PDF tabular data.

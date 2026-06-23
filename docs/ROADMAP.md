# Filegoblin Roadmap & Strategic Enhancements

This document tracks the strategic trajectory and planned enhancements for `filegoblin`. These capabilities ensure we maintain absolute market dominance as the premier, zero-dependency context ingestion tool for LLMs.

---

## 🚀 Strategic Horizon (Upcoming Enhancements)

Based on competitive analysis against Python/Node.js alternatives (Repomix, Aider, Docling), the following major architectural enhancements are targeted to solve the industry's most painful context-generation bottlenecks.

### 1. Geometric Heuristic PDF Table Extraction
**The Problem:** Standard local OCR and PDF parsers destroy tabular structures (merging columns, misaligning rows), heavily impacting financial and legal context ingestion.
**The Solution:** Implement a native Rust geometric parsing algorithm. By extracting text bounding boxes directly from the PDF vector stream, we can heuristically reconstruct grids (identifying overlapping coordinates and whitespace density) to output perfect Markdown/CSV Sequence-of-Records, completely bypassing the need for heavy ML vision models.

### 2. Dependency Graph "PageRank" for Heuristic Pruning
**The Problem:** When massive repositories exceed LLM token limits, naive truncation randomly drops critical architectural context.
**The Solution:** Adapt a Rayon-parallelized PageRank algorithm over our Tree-sitter ASTs. By building a structural dependency graph (tracking which files import/reference which symbols) and running PageRank natively in Rust, we can mathematically determine the most "important" files. When the `--max-tokens` limit is hit, filegoblin intelligently prunes the least-referenced files first, preserving the core architecture.

### 3. True "Out-of-Core" Stream Processing
**The Problem:** Competitors (like V8/Node-based tools) crash with Out-Of-Memory (OOM) errors on enterprise monorepos because they load entire parsed ASTs into RAM simultaneously.
**The Solution:** Refactor/ensure the core ingestion engine utilizes strict out-of-core streaming (lazy iterators). filegoblin must be able to read, parse, compress, and stream out to disk sequentially, guaranteeing a flat, minimal RAM footprint even on repositories with 100,000+ files.

### 4. Zero-Latency Regex/Entropy PII Redaction
**The Problem:** Data privacy is paramount, but executing ML models (even local SLMs like `Distil-PII-1B`) introduces compute overhead.
**The Solution:** Introduce a high-speed Regex and Shannon Entropy scanning engine running concurrently via `rayon`. This acts as an instantaneous "Level 1" privacy shield for API keys, IP addresses, and standard PII. It is computationally cheaper and orders of magnitude faster than spinning up tensor models for massive monorepos, acting as the primary engine for the `--scrub` flag.

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

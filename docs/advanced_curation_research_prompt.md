# Research Prompt: Advanced Token Curation & Intelligence

**Target Area:** `filegoblin` CLI Curation Feeds
**Objective:** Investigate zero-dependency architectures for two advanced intelligent curation features designed to prevent LLM context-window exhaustion: "Auto-Pruning" and "Semantic Search".

## Mandatory Architecture Constraints
- **Language:** Rust (Stable).
- **Dependencies:** Strictly "Pure Rust" or statically linked via `cc`. No user-facing system dependencies (like `apt` or `brew` packages) and no external runtime services (like Redis, Postgres, or Elasticsearch).
- **Offline First:** All operations must run locally on the user's machine. Calling out to external NLP or embedding APIs (e.g., OpenAI) is strictly forbidden for standard operations, as `filegoblin` is an air-gapped utility.

## Area 1: Token-Aware Budgeting & Auto-Pruning
The user executes `filegoblin ./src --horde --max-tokens 100k`. If `./src` natively equates to 500k tokens, the CLI must intelligently "prune" 400k tokens of data.

**Your R&D Task:** Design the heuristic engine to determine "least relevant" files. 
- *Investigation Areas:* 
  - Standard `.gitignore` and `.filegoblinignore` traversal logic (using the `ignore` crate).
  - Extension ranking (e.g., dropping `.json` test mocks before `.rs` business logic).
  - Using `tree-sitter` (already integrated) to prune function bodies dynamically as a fallback limit.
  - Sizing algorithms (dropping the largest files first vs. small files).

## Area 2: Zero-Dependency Semantic Search (RAG Lite)
The user executes `filegoblin https://docs.rs/hyper --horde --search "connection pooling"`. The tool must scrape the entire domain, index it locally in RAM, and ONLY output the top 3-5 markdown files that most closely match the semantic meaning of the query.

**Your R&D Task:** Determine the most performant, lightweight approach to local text ranking or embeddings in Rust.
- *Investigation Areas:*
  - **TF-IDF / BM25:** Can we use existing crates like `tantivy` or `fst` to build a fast inverted index in memory without bloating the binary by 50MB? Which algorithms offer the best semantic relevance without native AI embeddings?
  - **Local Embeddings (ONNX):** We currently use `candle-core` and an ONNX model for PII Redaction. Could we statically compile a highly quantized embedding model (like `all-MiniLM-L6-v2`) to generate vector representations of chunks, and perform a local cosine-similarity search? Investigate the binary size and memory footprint implications of embedding models.

## Deliverables Required
1. A technical specification (`docs/advanced_curation_spec.md`) evaluating the trade-offs of the approaches above.
2. An architectural design for Area 1 (Pruning).
3. A finalized recommendation for Area 2 (Semantic Search: BM25 vs ONNX Embeddings).
4. Update `docs/ADR.md` once a clear path forward is decided.

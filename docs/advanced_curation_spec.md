# Advanced Token Curation and Intelligence Specification

**Target Area:** `filegoblin` CLI Curation Feeds
**Objective:** Architecture definition for intelligent auto-pruning and semantic search features, operating under a strict zero-dependency, local-only mandate.

## 1. Heuristic Auto-Pruning Engine
The auto-pruning engine (`--max-tokens`) follows a multi-tiered filtration strategy to dynamically reduce large repositories to fit a specified LLM token budget.

### Core Components
- **Tokenizer:** Integration of `tiktoken-rs` or `miktik` to accurately quantify the token footprint of files prior to pruning, supporting models like `cl100k_base` and `o200k_base`.
- **Discovery Layer:** Leverages the `ignore` crate (`WalkBuilder`) with a strict precedence model (CLI flags -> `.filegoblinignore` -> `.ignore` -> `.gitignore` -> Global Configs -> Hidden Files).
- **Relevance Scoring:** Weighting map assigning priority to business logic/interfaces (`.rs`, `.go`, `.h`) over static data (`.json`, `.csv`) and trivial files (`.log`).
- **Sizing Algorithm:** Implementation of a Priority-Weighted Knapsack approach (or simple Greedy reduction) to discard low-priority, high-volume files first until the budget is met.
- **Structural Compression (Skeletonization):** Final fallback mechanism utilizing `tree-sitter` to parse high-priority source files and replace function bodies with placeholders (e.g., `// [implementation pruned]`), preserving only signatures and docstrings.

## 2. Zero-Dependency Semantic Search (RAG Lite)
The semantic search functionality (`--search "query"`) provides high-fidelity retrieval of relevant context from large directories or web domains entirely in RAM, without relying on dense neural embeddings.

### Core Implementation
- **Architecture:** Hybrid-Lexical Search using **BM25** ranking.
- **Engine:** `tantivy` (a fast, full-text search library in pure Rust).
- **Justification over ONNX Embeddings:** 
  - **Speed:** `tantivy` builds ephemeral in-memory indices (`Index::create_in_ram`) and executes searches in microseconds, maintaining the snappy UX of a CLI tool. Dense retrieval (even quantized INT8 models via `candle` or `tract`) adds 50-200ms latency.
  - **Binary Size:** Avoids the 20-90MB overhead of static embedding models (e.g., `all-MiniLM-L6-v2`), keeping the `filegoblin` executable lightweight.
  - **Accuracy:** Technical searches (e.g., code identifiers like `TcpStream`) perform better with exact keyword matching (BM25) than semantic approximations.
- **Enhancements:** "Identifier Tokenization" (breaking `camelCase` and `snake_case`) and a "Technical Synonym Map" (e.g., expanding "pool" to "connection pool") to approximate semantic behavior with zero runtime cost.

## 3. Resource Management Restrictions
- **Static Linking:** `tantivy`, `tree-sitter`, and tokenizers must be statically linked.
- **Memory Footprint:** The `tantivy` indexer must be capped at a 50MB RAM budget during execution to prevent OOM errors on large repositories.

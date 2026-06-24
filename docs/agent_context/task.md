# Execution Tasks: Out-of-Core Streaming & PageRank

- `[/]` **Phase 1: Dependency Graph PageRank**
  - `[x]` Add `petgraph` dependency to `crates/core/Cargo.toml`.
  - `[x]` Create `crates/core/src/curation/pagerank.rs` to parse AST imports and build the graph.
  - `[ ]` Refactor `crates/core/src/curation.rs` to use PageRank scoring for `enforce_budget`.
- `[ ]` **Phase 2: Out-of-Core Stream Processing**
  - `[ ]` Refactor `crates/core/src/routing.rs` and `lib.rs` to use a 2-pass crawl.
  - `[ ]` Update `crates/core/src/pipeline/stages.rs` to process files lazily or via channel.
  - `[ ]` Refactor `crates/core/src/formatters.rs` to stream incoming documents to disk/stdout.
- `[x]` **Verification**
  - `[x]` Fix all compilation errors.
  - `[x]` Run `cargo horde-check` to ensure tests and linting pass.

# Execution Tasks: Phase-6 Audit Cleanup

- `[x]` **A: Make all features compile** (web, tui)
- `[x]` **C: Remove dead code & deps** (12 deps, credentials.rs, level3, --lang, scratch/, prod mocks)
- `[x]` **D1: Delete PageRank + petgraph** (route --max-tokens through enforce_budget)
- `[x]` **D3/D4/D5: Simplify** (CodeGobbler full-content default; drop aggressive compression; real globset)
- `[x]` **Privacy Shield: delete mock, honest tiers** (fix char-boundary panic; secret prefixes; entropy redaction)
- `[x]` **B: Security fixes** (SSRF/size/timeout, archive bombs, WASM limits, git ref guard)
- `[x]` **G: Restructure** (walk.rs + git.rs + parse_files; unify formatters; fix flavor regression)
- `[x]` **E: Stale strings/docs** (mascot/UA versions, cost table, README/PRD/ROADMAP/HANDOFF, ADR)
- `[x]` **F: Verification** (workspace tests 30/0; clippy clean; feature compiles default/web/wasm/rag/tui)
  - `[ ]` `cargo horde-check` full run + `ocr` build on a suitable machine (mkl not available here)

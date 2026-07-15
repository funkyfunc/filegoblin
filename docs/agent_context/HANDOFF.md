# Handoff Report: Phase-6 Audit Cleanup

Branch: `audit/phase6-cleanup` (off `main` @ `caa54a8`). Not yet merged/tagged.

## Correction to the previous handoff
The prior v1.11.0 handoff over-claimed. For the record, on audit:
- **PageRank was never wired into `enforce_budget`.** `curation/pagerank.rs` existed but
  `enforce_budget` used extension ranking; the PageRank code ran only as a separate,
  divergent budget path inside the directory walk. It has now been **deleted**.
- **Streaming was a half-migration.** `stream_and_output`'s default path wrote raw blocks
  with **no flavor formatting at all** (flavors were silently dropped), and `gobble_local`
  still buffered. Fixed this session.
- Two feature builds (`web`, `tui`) **did not compile**; releases build `all-features`, so
  release binaries were broken. Fixed.

## What this session did (fix / remove / simplify / restructure — no new features)

**Compile fixes**
- `web`: fixed `raw_pairs` → `tx.send(...)` in `routing.rs`; added `use url::Url` in `lib.rs`.
- `tui`: made `args` mutable in `cli/main.rs`.

**Security**
- Privacy shield: fixed a `--scrub` char-boundary panic on multibyte input (byte offsets now
  snapped to char boundaries); **deleted the mocked "neural" Tier2 refiner + tokio semaphore**;
  the entropy tier now actually redacts unbroken high-entropy tokens; added deterministic
  detection for AWS/GitHub/OpenAI/JWT secret prefixes.
- Web: new `src/net.rs` — hardened reqwest clients with timeouts, a **redirect SSRF guard**
  (blocks loopback/private/link-local/metadata hosts on redirects), and a **10 MiB response cap**;
  correct User-Agent (`filegoblin/<version>`). Used by `fetch_url` and the crawler.
- Crawler: `println!` → `eprintln!` (was polluting stdout payload); added a 500-page cap.
- Archives (`archive.rs`): uncompressed-size cap (2 GiB), entry cap, zip-slip guard, and a
  **nested-archive guard** (`no_nested_archives`) to stop zip-quine recursion.
- WASM (`wasm.rs`): fuel + memory (`StoreLimits`) limits, cached `Engine`; implicit
  extension dispatch no longer auto-runs `./plugins/*.wasm` from the CWD (home dir only).
- `git`: `--git-diff` refs starting with `-` are rejected (option-injection guard), via new `src/git.rs`.

**Removed (dead)**
- Deps: `fst`, `sliding_features`, `safetensors`, `nojson`, `memmap2`, `nonzero_ext`,
  `urlencoding`, `base64`, `sha2`, `wasmtime-wasi`, `petgraph`, `compression-prompt`,
  `stop-words`; `tokio` is now feature-gated (`web`/`ocr` only). Build-deps trimmed.
- Modules/features: `curation/pagerank.rs`, `compressor/level3.rs`,
  `compressor/statistical_filter.rs`, the `aggressive` compression level, the dead
  `parsers/credentials.rs`, the unimplemented `--lang` flag, the `scratch/` Python scripts.
- Production mock fallbacks in `pdf.rs` / `office.rs` / `code.rs` that fabricated content on
  read errors — now return errors.

**Simplified / restructured**
- `CodeGobbler`: full file content by default; `--extract symbols` is opt-in and Rust-only
  (other languages return full source instead of being mangled by the Rust grammar).
- `--include`/`--exclude`: real globbing via `globset` (were substring matchers).
- `lib.rs`: `gobble_local` decomposed into `walk.rs` (tree/filters/globs) + `git.rs` +
  a `parse_files` helper.
- Formatters: streaming default path now applies the flavor wrapper (fixes the regression);
  whole-output flags route through the buffered pipeline.
- Stale strings: mascot + User-Agent now derive from `CARGO_PKG_VERSION`; `cost.rs` model
  table refreshed; README/PRD/ROADMAP reconciled with reality.

## State
- `cargo test --workspace` (default): **30 passed, 0 failed.**
- `cargo clippy --workspace`: clean.
- Feature compiles verified: default, `web`, `wasm`, `rag`, `tui`. `ocr` not built here (mkl).

## Immediate next steps
1. Run `cargo horde-check` and the `ocr` build on a machine set up for it.
2. Decide on merge + version bump/tag (this branch is unmerged).
3. Phase-7 feature work (separate approval): honest local Privacy Shield as a product;
   agent-native per-file extraction primitive. See `implementation-notes.md` and the audit report.

# Implementation Notes — Phase 6 Audit Cleanup

Branch: `audit/phase6-cleanup`. Started from `caa54a8` on `main`.

This log records every change made during the post-audit cleanup and the reasoning
behind it. Scope is **fix / remove / simplify / restructure** — no new features.
See `/Users/stompinggrounds/.claude/plans/filegoblin-deep-linked-quail.md` for the full plan.

## Locked decisions (from interview)
- Scope: everything + restructure (decompose `lib.rs`, unify formatters).
- Privacy Shield: delete the mock neural tier; keep honest deterministic + entropy tiers.
- CodeGobbler: full file content by default; skeletonization opt-in; non-Rust → plain read.
- No scry code-sharing (duplication is fine); techniques may still be ported.

---

## A. Make everything compile

### A1. `web` feature broken — GitHub clone path (routing.rs)
`route_url_target` was left half-refactored during the streaming migration: it referenced
a `raw_pairs` local that no longer exists (the function now sends over a `SyncSender<GobbleDocument>`
instead of accumulating a Vec). Fixed by sending each cloned-repo pair over `tx`, mirroring
`route_local_target`.

### A2. `web` feature broken — `fetch_url` missing `Url` import (lib.rs)
`fetch_url` used `&Url` but `use url::Url;` was never added. Added a `#[cfg(feature = "web")]`
import (the only user of `Url` in lib.rs is web-gated).

### A3. `tui` feature broken — immutable `args` (cli/main.rs)
`run_tui(&mut args)` requires a mutable binding but `main` had `let args`. Made the binding
`mut`. Because non-tui builds don't take `&mut`, added `#[cfg_attr(not(feature = "tui"), allow(unused_mut))]`.

Verified compiles: default, `web`, `wasm`, `rag`, `tui`. `ocr` NOT built here (candle-core
pulls Intel MKL, not set up on this machine) — flagged for follow-up.

## D1. Delete PageRank
Deleted `curation/pagerank.rs`, the `petgraph` dep, and the second budget path that ran inside
`gobble_local`'s walk. `--max-tokens` now routes through the single `curation::enforce_budget`
by extending `gobble_app`'s buffering condition (it already handled `--search`). Rationale:
the `content.contains(module_stem)` heuristic produced garbage edges and never touched the real
`enforce_budget`; a second divergent enforcer was pure liability.

## C. Dead code / deps removed
- Deps (Cargo.toml): `fst`, `sliding_features`, `safetensors`, `nojson`, `memmap2`,
  `nonzero_ext`, `urlencoding`, `base64`, `sha2`, `wasmtime-wasi`, `petgraph`,
  `compression-prompt`, `stop-words`. Verified zero source references first. `tokio` made
  optional (gated to `web`/`ocr`; the only always-on user was the deleted PII semaphore).
  Trimmed build-deps to the clap/mangen/complete set actually used by `build.rs`.
- `parsers/credentials.rs`: entire module unused (no callers). Deleted + removed `mod`.
- `--lang` CLI flag: defined but never read anywhere. Deleted.
- `compressor/level3.rs` (`StopwordPruner`): declared + tested but never added to
  `CompressionPipeline`. Deleted with the `aggressive` level.
- `scratch/` Python refactor scripts: fragility smell (string-patching Rust via `git show`).
  Deleted.
- Production mock fallbacks: `pdf.rs`/`office.rs` returned fake rows for missing "dummy.*"
  paths; `code.rs` substituted a dummy fn on read error. All removed — they now return errors.
  Tests that only exercised the mocks were replaced with real-fixture / error-path tests.

## Privacy Shield (delete mock, honest tiers)
Rewrote `privacy_shield.rs`:
- Removed `Tier2Refiner` (hardcoded word list) and the `tokio::sync::Semaphore` (permit was
  acquired and dropped immediately — pure ceremony).
- Fixed the char-boundary panic: entropy scan works on bytes but redaction spans are now
  snapped with `floor_char_boundary`/`ceil_char_boundary` before `replace_range`.
- Entropy tier now *actually redacts* the high-entropy spans it finds. Discovered the original
  tier was effectively dead: window=16 caps max entropy at log2(16)=4.0 so `> 4.0` never fired
  (only the mock ever redacted). New params: window=24, threshold=4.0, plus a **whitespace-free**
  requirement so unbroken secret tokens trip it but varied prose (which has spaces within 24
  chars) does not.
- Deterministic tier gained AWS/GitHub/OpenAI/JWT secret-prefix regexes and spaced-SSN support.

## D3/D4/D5. Parser/compression/glob simplification
- `code.rs`: full file content by default; `--extract symbols` opt-in and **Rust-only** (other
  languages return full source rather than being parsed with the wrong grammar). Removed the
  body-elision-by-default skeleton path.
- Compression: dropped the `aggressive` level (and `statistical_filter.rs`); `CompressionLevel`
  is now `Safe | Contextual`. Updated the TUI toggle (`ui.rs`) which cycled through `Aggressive`.
- Globs: `--include`/`--exclude` now use `globset` (`walk.rs::build_globset`) with a friendly
  normalizer (`.rs`→`*.rs`, bare word→`*word*`, explicit globs verbatim). Were substring matchers.

## B. Security
- `net.rs` (new): hardened blocking/async reqwest clients (30s timeout, SSRF redirect guard via
  `is_blocked_host`, UA from `CARGO_PKG_VERSION`); `MAX_RESPONSE_BYTES` = 10 MiB.
- `fetch_url`: uses the hardened client; rejects over-cap `Content-Length` and caps chunked
  bodies via `take(MAX+1)`.
- `crawler.rs`: uses `net::async_client`; `println!`→`eprintln!` (was corrupting stdout payload);
  content-length size guard; `MAX_PAGES` = 500 frontier cap.
- `archive.rs`: manual zip iteration with per-entry size accounting + `enclosed_name` zip-slip
  guard; tar via `unpack_in` with size/entry caps; `no_nested_archives` guard (new hidden Cli
  field, `#[arg(skip)]`) set when recursing so archives-inside-archives are refused.
- `git.rs` (new): `validate_ref` rejects `-`-prefixed refs (option-injection guard); `changed_files`
  / `file_diff` wrappers.
- `wasm.rs`: cached `Engine` (OnceLock), `consume_fuel` + `StoreLimits` memory cap; `sniff` gained
  an `allow_cwd` param — implicit dispatch passes `false` (home dir only), explicit `--plugin`
  passes `true`.

## G. Restructure
- `walk.rs` (new): `walk_tree`, `is_radioactive`/`filter_radioactive`, `build_globset`/
  `matches_globset`/`apply_glob_filters`. `gobble_local` shrank from ~270 lines to an orchestrator
  + a `parse_files` helper.
- Formatters: streaming default path now writes `flavors::format_header` once before the loop and
  `format_footer` after (matches the buffered path; fixes the dropped-flavor regression). Whole-
  output flags (`--copy/--open/--cost/--tokens-only/--summary/--manifest`) route through the
  buffered pipeline via `needs_buffer` in `gobble_app`.

## E. Stale strings
- Mascot (`cli/main.rs`) + User-Agent (`net.rs`) derive from `CARGO_PKG_VERSION`.
- `cost.rs`: refreshed model/price table (Claude 4.5 / GPT-5 / Gemini 2.5).
- README capabilities reconciled with feature gates; "Images N/A" → OCR; Distil-PII claims
  replaced with honest description. PRD/ROADMAP/HANDOFF/task.md corrected. ADR entry appended.

## Verification
- `cargo test --workspace` (default): 30 passed, 0 failed.
- `cargo clippy --workspace -- -D warnings`: clean.
- Feature compiles: default/web/wasm/rag/tui OK; ocr not attempted (mkl).

---

# Phase 7 — Local Privacy Shield as a Product

Turns `--scrub` into a real, scriptable, zero-weight PII/secret scrubber. Same branch.

## Engine rewrite (`privacy_shield.rs`)
- Typed rule engine: `PiiKind` (with `label()`), `Match{start,end,kind}`, `RegexRule{kind,regex,
  validate: Option<fn(&str)->bool>}`. `scan()` runs rules (validated) → adds entropy matches that
  don't overlap a rule → drops allowlisted values → resolves overlaps (earliest/longest wins).
  `redact()` applies from end to start with char-boundary snapping and tallies typed counters.
- Detection expanded: AWS/GitHub/OpenAI/Google/Slack/Stripe/JWT/private-key-block/connection-string,
  plus phone/IPv4/IBAN, plus the prior email/SSN. Validators: `luhn_valid` (cards — kills the old
  over-broad digit-run FP), `ssn_valid` (area/group/serial ranges), `ipv4_valid` (octet ≤255),
  `iban_valid` (mod-97). Dropped the keyword-word redaction (redacting "Passport" was noise).
- Entropy tier kept (window 24, threshold 4.0, whitespace-free) → typed `SECRET`, yields to rules.
- Counters are `Mutex<BTreeMap<String,usize>>` (Sync) so one shared shield aggregates across the
  rayon buffered path and the streaming closure. `counts()` / `total()` expose the tally.
- Credit-card regex anchored to start/end on a digit (`\b\d(?:[ -]?\d){12,18}\b`) so it doesn't
  eat a trailing separator.

## Config (`scrub_config.rs`, new — serde_json, already a dep)
- `ScrubConfig{allowlist, patterns:[{name,regex}]}` from `--scrub-config <path>` or auto-discovered
  `./.filegoblin-scrub.json`. Explicit-but-missing = hard error; missing default = silently none.
  Custom patterns become `PiiKind::Custom(NAME)`; invalid regexes are logged and skipped.

## CLI (`cli.rs`)
- `RedactStyle{Typed(default),Plain}` (clap `ValueEnum`; referenced unqualified so `build.rs`'s
  `include!("src/cli.rs")` still compiles). New flags (all `requires = "scrub"` except style):
  `--scrub-style`, `--scrub-fail`, `--scrub-config`, repeatable `--scrub-allow`.

## Wiring (`lib.rs`, `stages.rs`, `cli/main.rs`)
- `gobble_app` builds one shield via `build_shield(args)`, shares it (`&` buffered / `Arc` streamed),
  prints `print_scrub_report` (value-free, to stderr unless `-q`), and returns the redaction total
  (`Result<usize>`). `run_privacy_stage` now takes `Option<&PrivacyShield>`.
- `main.rs`: `if args.scrub_fail && redacted > 0 { process::exit(3) }` on the normal + tui paths
  (before entering watch).

## Verification (Phase 7)
- `cargo test --workspace`: 40 passed (core), 0 failed. New tests: typed labels, Luhn FP-reject +
  accept, SSN range, secret prefixes, entropy `SECRET`, plain style, allowlist, custom pattern,
  count tally, prose-survives, non-ASCII no-panic; plus `scrub_config` parse/discover tests.
- `cargo clippy --workspace -- -D warnings`: clean; feature clippy (web/wasm/rag/tui): clean.
- Smoke: typed report end-to-end; `--scrub-style plain`; `--scrub-fail` exits 3 (dirty) / 0 (clean);
  `.filegoblin-scrub.json` allowlist + custom pattern honored.

---

# Phase 7b — Consistent Pseudonymization + JSON Extraction Contract

## Item 2 — pseudonymization (`privacy_shield.rs`, `cli.rs`)
- `RedactStyle::Pseudonym` (third clap variant). `PseudoInterner` (a `Mutex` field on the shared
  shield) maps `(label, value) → stable index`; `[EMAIL_1]`, `[EMAIL_2]`, reused everywhere so the
  same value is consistent across all documents in a run.
- Bug found + fixed by the unit test: indices must be assigned in *reading* order, but redaction is
  applied back-to-front (to keep byte offsets valid). Reworked `redact` to compute replacements
  front-to-back (assigning pseudonym indices then) into a `Vec<(s,e,rep)>`, then apply `.rev()`.

## Item 3 — JSON machine contract (`formatters.rs`, `lib.rs`, `cli/main.rs`)
- JSON records are now `{path, format, tokens, content}`. `detect_format(path)` maps to a coarse
  type (pdf/spreadsheet/code/image/text/…); `tokens` reuses `heuristic::estimate_tokens`.
- `--json` added to `needs_buffer` so it always uses the buffered `format_and_output`, which sorts
  alphabetically → deterministic output (verified: two horde runs → identical md5).
- `gobble_app` now returns `GobbleOutcome { redactions, produced }` (produced = non-empty content
  docs, excl. `_tree.md`). `main.rs` exit-code contract: `3` for `--scrub-fail` matches, `4` when
  `--json` produced nothing, else `0`. The streaming `stream_and_output` json branch is now
  unreachable via the app (kept intact for direct callers).

## Verification (Phase 7b)
- `cargo test --workspace`: 41 core tests pass (added `pseudonym_style_is_consistent`), 0 failed.
- clippy `-D warnings` clean (default + web/wasm/rag/tui); fmt clean.
- Smoke: pseudonym consistency (`[EMAIL_1]`/`[EMAIL_2]` reused), JSON `{format,tokens}`, empty
  stdin `--json` exits 4, horde `--json` deterministic across runs.

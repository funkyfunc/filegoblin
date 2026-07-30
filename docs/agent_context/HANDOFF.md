# Handoff Report

Branch: `audit/phase6-cleanup` (7 sessions of work sitting on top of `v1.11.0`;
unmerged, untagged).

## Where things stand (2026-07-29)

The trust-layer / quality / simplification pass is complete. In order:

1. **Trust layer** (`51f013f`): producer errors propagate; exit 4 whenever
   nothing is extracted (all modes); `web` is a default feature of the cli
   crate; `--extract` is a ValueEnum; stdin precedence fixed (explicit paths
   win, bare pipe works, no phantom `-`); broken stdout pipes exit 0.
2. **DOM-based HTML extractor** (`f3eef90`): readability-lite on `scraper`,
   preserved link URLs, title/source front-matter, link-farm pruning, charset
   handling, panic-hardened html2md.
3. **Conversion quality** (`2b658ca`): docx → real markdown (headings, lists,
   emphasis, resolved hyperlinks); sheets use the header row (one Sequence-of-
   Records dialect everywhere); PDF cell text assigned from fragment geometry
   with column-boundary splitting (fixes the merged-header bug); empty PDFs
   error instead of Ok("").
4. **Golden suite** (`773f753`): `crates/cli/tests/golden.rs` — snapshots per
   format over real fixtures + behavioral contract tests; deterministic walk.
5. **Simplification** (`6ed76cf`): flavors, `--watch`, `--cost` removed (one
   markdown output format); TUI and clipboard kept.
6. **Docs diet** (this session): docs reduced to README + ADR + this dir +
   `docs/research/` archive; PRD/NORTH_STAR/ARCHITECTURE/implementation-notes/
   UX/ROADMAP/instructions/.agents deleted; 15.7 MB of dead OCR/tesseract
   assets removed from git; `just get-brains` now fetches the correct ocrs
   models; GEMINI.md rewritten to match reality.

## State
- `cargo test --workspace`: 65 core unit + 14 e2e golden, all green.
- `cargo clippy --workspace --all-features -- -D warnings`: clean.
- Snapshots live in `crates/cli/tests/snapshots/` — review diffs with
  `cargo insta review`.

## Immediate next steps
1. Decide on merging this branch to `main` and tagging (minor bump — several
   features and breaking flag removals since v1.11.0).
2. Candidate gaps (owner reviewing): legacy `.doc`, `.odt`/`.odp`, PPTX speaker
   notes/slide order from the manifest, email HTML bodies through the real
   HTML extractor, scanned-PDF → OCR fallback story, CI feature matrix,
   `--scrub` not protecting directly-named secret files, installers in
   cargo-dist.

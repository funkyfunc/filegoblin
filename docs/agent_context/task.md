# Task State: PDF Conversion Fidelity Engine

## Current Status: Completed (Phase 1, Phase 2, Phase 3)

### Completed Items
- [x] **Phase 1: Benchmark Harness & Targeted Slices**
  - Extracted 5-page targeted benchmark slices into `test_assets/benchmarks/`:
    - `slice_a_twocolumn.pdf` (*Structuring Interviews with Experts*)
    - `slice_b_tables_charts.pdf` (*The Logic of Failure*)
    - `slice_c_callouts_headers.pdf` (*Thinking in Systems*)
    - `slice_d_unseen_frameworks.pdf` (*Bulletproof Problem Solving*)
  - Created automated benchmark scoring harness `scripts/benchmark_fidelity.py` (measuring header leakage, sentence interruptions, line interleaving, n-gram recall/precision, broken hyphens, and table syntax).
  - Created integration test `crates/core/tests/benchmark_fidelity.rs`.
  - Recorded initial baseline scorecard in `BENCHMARK_REPORT.md` (commit `f1bad26`).

- [x] **Phase 2: Systematic Diagnostic & Iterative Fixes**
  - **Iteration 1: Running Header & Footer Suppression** (commit `05f16b8`)
    - Implemented `filter_running_headers_and_footers` in `crates/core/src/parsers/pdf.rs`.
    - Added `is_isolated_folio` for digits, Roman numerals, bracketed folios, and OCR glyph drift (`l` <-> `1`).
    - Added `is_publisher_or_production_slug` for JSTOR, journal markers, and copyright lines.
    - Added `headers_match` with Levenshtein edit distance <= 2 for alternating headers across odd/even pages.
    - Preserved document titles and author blocks on Page 0 via `is_document_title_block`.
  - **Iteration 2: Layout / Reading Order & Two-Column Continuity** (commit `fe84280`)
    - Introduced `OcrBox` geometry struct in `crates/core/src/parsers/ocr.rs`.
    - Implemented deterministic two-column gutter detection (`detect_two_column_gutter`) along corridor x in [0.35, 0.65].
    - Implemented band-based page partitioning (`partition_two_column_page`) to preserve full-width banner titles before sequencing Column 1 then Column 2.
    - Overhauled line assembly (`assemble_lines_within_group`) with vertical overlap (>= 40%) and horizontal non-overlap (<= 15%) rules, eliminating line interleaving.
    - Implemented soft-hyphen dehyphenation (`join_lines_with_dehyphenation`), dropping broken hyphens from 16 to 1.
  - **Iteration 3: Table and Block Separation** (commit `fc981ab`)
    - Implemented `format_table_and_block_boundaries` in `crates/core/src/parsers/pdf.rs`.
    - Enforced mandatory blank line margins before and after markdown tables and blockquotes.
    - Synchronized table separator row column counts (`|---|...|`) with header row column counts.
    - Protected code fences (```) from line formatting and collapsed redundant blank lines.

- [x] **Phase 3: Regression & Validation Gate**
  - Validated full test suite: 190 core unit tests + 21 golden tests passing with 0 warnings.
  - Validated on unseen Slice D (`slice_d_unseen_frameworks.pdf`): 0 header leaks, 0 interruptions, 0 broken hyphens, 91.6% 8-gram recall.
  - Finalized `BENCHMARK_REPORT.md` (commit `545f707`).
  - Added ADR entry `2026-10-10: Deterministic PDF Layout Ordering, Header/Footer Suppression, and Table Boundary Spacing` to `docs/ADR.md`.

### Pending Items
- None for the current PDF conversion fidelity milestone. All verification gates and horde checks pass.

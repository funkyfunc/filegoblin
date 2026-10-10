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

- [x] **Full-Book Conversion Stress Testing & Telemetry (Milestone 1)**
  - Implemented `scripts/profile_throughput.py` measuring pages/second, memory RSS, tables, and unrecovered figure markers.
  - Profiled full books streaming to disk (`target/bench_out/`):
    - *Structuring Interviews with Experts* (21 pages scanned): 9.15 s, 2.3 pages/s, 334 MB RSS, 9,161 words.
    - *Thinking in Systems* (235 pages): 67.18 s, 3.5 pages/s, 94.4 MB RSS, 29,927 words, 3 tables.
    - *Bulletproof Problem Solving* (296 pages): 44.26 s, 6.7 pages/s, 417.4 MB RSS, 57,168 words, 70 figure OCR blocks.
  - Documented results in `BENCHMARK_REPORT.md`.

- [x] **Semantic Chapter & Section Boundaries (Milestone 2)**
  - Implemented `insert_semantic_section_boundaries` in `crates/core/src/parsers/pdf.rs`.
  - Automatically identifies top-level chapters, parts, and appendices.
  - Synthesizes clean thematic divider `---` and XML section comments (`<!-- filegoblin:section type="chapter" title="..." -->`).
  - Merges split chapter label and subtitle lines into unified section titles while protecting code fences.
  - Verified with 5 comprehensive unit tests (all passing).

### Pending Items
- Office structure enhancements (embedded charts, equations, and tables).


# Session Handoff: PDF Conversion Fidelity Engine

## 1. What Was Just Completed
We have overhauled `filegoblin`'s PDF conversion engine across three comprehensive phases to achieve state-of-the-art fidelity, correct reading order, running header/footer suppression, and table boundary spacing with **zero hallucination** and **pure CPU execution**:

1. **Benchmark Suite & Harness (`f1bad26`):**
   - Created targeted slices in `test_assets/benchmarks/`: `slice_a_twocolumn.pdf`, `slice_b_tables_charts.pdf`, `slice_c_callouts_headers.pdf`, and `slice_d_unseen_frameworks.pdf`.
   - Created automated evaluation harness `scripts/benchmark_fidelity.py` and integration test `crates/core/tests/benchmark_fidelity.rs`.
   - Established baseline scorecard in `BENCHMARK_REPORT.md`.

2. **Running Header & Footer Suppression (`05f16b8`):**
   - Implemented `filter_running_headers_and_footers` in `crates/core/src/parsers/pdf.rs`.
   - Strips isolated folios (page numbers, OCR glyph variations `l` <-> `1`, Roman numerals), publisher slugs (JSTOR, copyright, journal metadata), and repeated headers across distinct pages via Levenshtein distance <= 2.
   - Preserves Page 0 document title and author blocks.
   - Reduced Slice C header/footer leakage from 10 to 0 and eliminated page-boundary sentence interruptions.

3. **Layout / Reading Order & Two-Column Continuity (`fe84280`):**
   - Implemented `OcrBox` geometry struct in `crates/core/src/parsers/ocr.rs`.
   - Built deterministic gutter corridor detection (`detect_two_column_gutter`) and vertical band-based page partitioning (`partition_two_column_page`).
   - Replaced scalar line-bucketing with geometric line collision prevention (`assemble_lines_within_group`): requires >= 40% vertical overlap and <= 15% horizontal overlap, preventing adjacent vertical lines from merging.
   - Added automated soft-hyphen dehyphenation (`join_lines_with_dehyphenation`), dropping broken hyphens from 16 to 1 in Slice A.
   - Reduced Slice A interleaved line violations from 1 to 0, restoring true top-to-bottom column reading order.

4. **Table & Block Separation (`fc981ab`):**
   - Implemented `format_table_and_block_boundaries` in `crates/core/src/parsers/pdf.rs`.
   - Enforces mandatory empty line padding before and after tables and blockquotes.
   - Repairs table separator rows (`|---|...|`) so column counts strictly match header rows.
   - Preserves code fences (```) and normalizes whitespace.

5. **Validation Gate & Documentation (`545f707` and current):**
   - Validated against unseen Slice D (`slice_d_unseen_frameworks.pdf`): 0 header leaks, 0 interruptions, 0 broken hyphens, 91.6% 8-gram recall.
   - Finalized `BENCHMARK_REPORT.md` with complete Before vs After scorecard.
   - Appended `2026-10-10` ADR entry to `docs/ADR.md`.

## 2. Broken Tests or Pending Bugs
- **Zero broken tests:** 190 core unit tests, 1 integration test, and 21 golden snapshot tests pass with 0 warnings.
- **Zero clippy warnings:** `cargo clippy --workspace -- -D warnings` clean.
- **Zero formatting issues:** `cargo fmt --check` clean.
- **Zero pending bugs:** All identified failure modes from the baseline have been resolved or documented under architectural limitations.

## 3. Immediate Next Steps
- Push the local commits to remote (`origin/main`) if desired.
- If cutting a release, bump crate versions (e.g. `1.13.0` for features or `1.12.1` for fixes) and tag `vX.Y.Z`.

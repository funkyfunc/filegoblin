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

- [x] **OpenXML Embedded Chart Deconstruction (Milestone 3)**
  - Implemented `crates/core/src/parsers/openxml_chart.rs` to parse ECMA-376 DrawingML charts (`c:chartSpace`).
  - Extracts title, categories (`c:cat`/`c:xVal`), and series values (`c:ser`/`c:val`/`c:yVal`) into standard Markdown pipe tables.
  - PowerPoint integration (`crates/core/src/parsers/powerpoint.rs`): maps slide `.rels` (`ppt/slides/_rels/slideN.xml.rels`) and injects tables beneath slide content.
  - Word integration (`crates/core/src/parsers/office.rs`): maps document `.rels` (`word/_rels/document.xml.rels`) and injects tables at paragraph anchor positions.
  - Recovered alt-text descriptions from `<p:cNvPr descr="...">` (e.g. `[Image: Red square SVG]`).

- [x] **Word Complex Layouts & Office Math (OMML) Extraction (Milestone 4)**
  - Implemented `crates/core/src/parsers/omml.rs` converting Office Math Markup Language (`<m:oMath>` / `<m:oMathPara>`) to LaTeX syntax.
  - Supports fractions (`\frac`), superscripts (`^`), subscripts (`_`), sub-superscripts, radicals (`\sqrt`), delimiters (`(...)`), and n-ary sums (`\sum`).
  - Augmented `crates/core/src/parsers/office.rs` to substitute math formulas into paragraphs and table cells while preserving headings and lists.
  - Verified on `test_assets/benchmarks/equations.docx`.

- [x] **Zero-Install npm / npx Distribution Package (Milestone 5)**
  - Created `npm/package.json` with `filegoblin` binary mapping.
  - Created `npm/bin/filegoblin.js` cross-platform launcher resolving Cargo release/debug builds, optional platform packages, or system PATH.
  - Transparent execution: spawns binary with `{ stdio: 'inherit' }` and propagates exit codes.
  - Tested with `node npm/bin/filegoblin.js test_assets/benchmarks/chart_sample.pptx -q`.

- [x] **PDF Fidelity & Performance Iterative Optimization (Hill-Climbing Loop)**
  - **Cycle 1 (Recall Recovery on Slice C - Thinking in Systems):**
    - Diagnosed dropped text: trailing production slugs caused paragraph loss; margin pull-quotes were intertwined.
    - Implemented `clean_trailing_production_slug` and `disentangle_margin_callouts`.
    - **Result:** Recall increased from 70.0% to 86.6% (Target: $\ge 85\%$), Precision rose to 90.5%. (Commit `1df636d`)
  - **Cycle 2 (Phantom OCR Elimination on Slice D - Bulletproof Problem Solving):**
    - Diagnosed phantom text: digital pages with small diagrams triggered duplicate full-page OCR.
    - Gated secondary OCR on `PageKind::ImageText` behind `md.split_whitespace().count() < 40`.
    - **Result:** Precision increased from 76.9% to 93.6% (Target: $\ge 88\%$), Recall held at 93.1%. (Commit `243faa4`)
  - **Cycle 3 (Throughput Acceleration on Scanned OCR - Structuring Interviews with Experts):**
    - Multi-threaded page conversion via Rayon `(0..page_count).into_par_iter()`.
    - Zero-copy direct pass-through of JPEG byte streams from PDFs to OCR engine.
    - Disabled slow Apple Vision NLP dictionary spellchecking (`setUsesLanguageCorrection(false)`).
    - **Result:** Throughput accelerated from 2.3 pages/s to 31.4 pages/s (Target: $\ge 5.0$ pages/s), digital throughput reached 1032 pages/s. (Commit `c31d56d`)

- [x] **Deterministic Office & PDF Structural Deconstruction Loops**
  - **Loop 1: Embedded Charts -> GFM Tables & Mermaid xychart-beta (Commit `baec99b`):**
    - Extracted OpenXML DrawingML charts (`c:chartSpace`) in `crates/core/src/parsers/openxml_chart.rs`.
    - Computes automatic axis limits (`nice_limits`) and dual-emits GFM tables and Mermaid `xychart-beta` (with title, x-axis categories, y-axis series name & range, and bar/line values).
    - Verified on `test_assets/benchmarks/chart_sample.pptx`.
  - **Loop 2: DrawingML Connectors -> Mermaid Flowcharts (Commit `ee25dd0`):**
    - Created `crates/core/src/parsers/drawingml.rs` with `SlideDiagram::parse` and `render_mermaid`.
    - Integrated with `powerpoint.rs`: connected shapes via `p:cxnSp` (`stCxn`, `endCxn`) are compiled into clean `flowchart TD` blocks, while isolated prose shapes remain as bullet points.
    - Verified on `test_assets/benchmarks/connector_flowchart.pptx`.
  - **Loop 3: SmartArt dataModel -> Mermaid Graphs (Commit `ad20a6b`):**
    - Created `crates/core/src/parsers/smartart.rs` with `SmartArtDiagram::parse` and `render_mermaid`.
    - Traverses `dgm:dataModel` (`ppt/diagrams/data*.xml`), filters non-semantic transitional nodes (`sibTrans`/`parTrans`), and compiles sequence chains into `flowchart LR` (e.g. `N_1["Phase 1: Research"] --> N_2["Phase 2: Prototype"] --> N_3["Phase 3: Production"]`).
    - Verified on `test_assets/benchmarks/smartart_process.pptx`.
  - **Loop 4: Word OMML Equations -> KaTeX LaTeX Math (Commit `aada1f7`):**
    - Overhauled `crates/core/src/parsers/omml.rs` with full XML attribute support (`OmmlNode::attrs`).
    - Compiles fractions, radicals with `degHide`, sub/superscripts, configurable delimiters (`dPr` with `begChr`/`endChr`), n-ary operators (`\int`, `\sum`, `\prod`), matrices (`m`/`mr`/`e`), and accents (`acc`).
    - Strips invisible unicode characters (`\u{2061}`-`\u{2064}`), maps trigonometric/logarithmic functions to `\sin`, `\cos`, etc., and captures embedded images in Word documents.
    - Verified on `test_assets/benchmarks/equations.docx` and `docx_complex_layout.docx`.
  - **Loop 5: PDF TeX Math Font Recovery via CMap Normalization (Commit `f8d9aa4`):**
    - Implemented `normalize_pdf_glyph` in `crates/core/src/parsers/pdf.rs` covering `CMSY*`, `CMMI*`, `MSAM*`, and `MSBM*` font glyph codes.
    - Implemented `normalize_pdf_math_notation` repairing legacy publisher CMap substitutions (e.g. `=I-` $\to$ `\neq`, `::;` / `:S` $\to$ `\le`, `2':` $\to$ `\ge`, `q(x) 2 0` $\to$ `q(x) \ge 0`, `= O` $\to$ `= 0`).
    - Verified on `test_assets/benchmarks/slice_e_math_zeitz.pdf`.

### Pending Items
- None (All 5 loops verified with 215 tests passing, 0 warnings, and 21 golden tests matching).



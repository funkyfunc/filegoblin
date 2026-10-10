# Benchmark Report: PDF Conversion Fidelity & Zero-Hallucination

This report documents automated fidelity, layout continuity, and quality benchmark metrics for `filegoblin`'s PDF conversion engine on real-world complex documents (two-column layouts, tables, sidebars, and running headers/footers).

## Benchmark Slices (test_assets/benchmarks/)

- **Slice A (`slice_a_twocolumn.pdf`)**: 5 pages from *Structuring Interviews with Experts During Knowledge Elicitation* (Wood & Ford 1993). Pure two-column layout with abstracts, sections, callouts, and running headers/footers.
- **Slice B (`slice_b_tables_charts.pdf`)**: 5 pages from *The Logic of Failure* (Dörner 1990, Phil. Trans. R. Soc. Lond. B). Academic paper containing figures/diagrams, tables, and multi-line running footers/folios.
- **Slice C (`slice_c_callouts_headers.pdf`)**: 5 pages from *Thinking in Systems* (Meadows 2008). Complex non-fiction book layout featuring alternating running chapter headers, publisher production footers, and margin callout boxes.
- **Slice D (`slice_d_unseen_frameworks.pdf`)**: 5 pages from *Bulletproof Problem Solving* (Conn & McLean). Dense business frameworks, logic trees, and exhibits (reserved for validation gate).

---

## Baseline Scorecard (Phase 1)

| Benchmark Slice | Header/Footer Leakage | Sentence Interruptions | Interleaved Lines / Columns | Shingle Recall (8-gram) | Shingle Precision (8-gram) | Broken Hyphens / Ligatures |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Slice A (Two-Column)** | 6 | 0 | 1 (Entire doc merged) | N/A (Scanned page) | N/A (Scanned page) | 16 hyphens |
| **Slice B (Tables & Charts)** | 14 | 0 | 0 | 87.7% | 87.7% | 0 |
| **Slice C (Callouts & Headers)** | 10 | 1 | 2 | 77.4% | 78.4% | 0 |

---

## Detailed Failure Mode Diagnosis (Phase 2 Diagnostic)

### 1. Running Header, Footer, and Page Folio Ingestion
- **Observed Symptoms:**
  - Running headers (`WOOD AND FORD`, `STRUCTURING INTERVIEWS WITH EXPERTS`, `THE LOGIC OF FAILURE`, `PART ONE: SYSTEM STRUCTURE AND BEHAVIOR`, `CHAPTER ONE: THE BASICS`) are emitted directly into markdown body text.
  - Page folios (e.g. `**14**`, `15`, `**l7** 1`, `I **18** I`, `31-2`) interrupt paragraphs and break sentences.
  - Running publisher production marks (`TIS final pgs 14 5/2/09 10:37:35`) and journal slugs (`INTERNATIONAL JOURNAL OF INTELLIGENT SYSTEMS... CCC 0884-8173/93/010071-20`, `*Phil. Trans. R. Soc. Lond. B* **327, 463473** *(1990)*`) are injected into the text stream.
  - In Slice C, sentence continuity is broken right across page boundaries:
    `...supply of hope that the world can be better` $\rightarrow$ `TIS final pgs 17 5/2/09 10:37:35` $\rightarrow$ `**18**` $\rightarrow$ `**PART ONE: SYSTEM STRUCTURE AND BEHAVIOR**` $\rightarrow$ `are both stocks.`
- **Root Cause:**
  - `ConversionOptions::default()` in `pdf_oxide` has `strip_running_headers_footers: false` and `include_artifacts: true`.
  - Even with `strip_running_headers_footers: true`, `pdf_oxide`'s `repeated_running_head_foot` requires a 0.6 majority across all pages, which fails on alternating headers (even/odd chapter titles never exceed 50%), and ignores strings $\le 3$ characters (isolated page numbers/folios are completely skipped).
  - Furthermore, on scanned pages, OCR raw line clustering groups header and footer text with no margin bounding or boundary suppression.

### 2. Multi-Column Reading Order & Sidebar Interleaving
- **Observed Symptoms:**
  - In Slice A, column 1 and column 2 are horizontally merged row-by-row into nonsensical interleaved text (e.g. `We have developed interviewing techniques... (1) relevant research... (2) the nature of expertise, / associated with the acquisition of highly complex concepts, and (4) the desirability of (3) a desire to assist...`).
  - In Slice C, callout sidebars on the left margin (`**Many of the interconnec-**`, `**The least obvious part of**`) are merged line-by-line with the primary text paragraph on the right (`...students progressing through a university...`).
- **Root Cause:**
  - In `crates/core/src/parsers/ocr.rs` (`vision_ocr`), recognized spans are sorted solely by vertical position (`top`), grouped into horizontal rows within `Y_BAND = 0.015`, and joined left-to-right. Multi-column documents have left and right spans at the same `top`, causing immediate row-wise interleaving.
  - In digital extraction, callouts located in margin bands lack boundary separation and get absorbed into adjacent body text lines.

### 3. Hyphenation, Ligature Gaps, and Table Syntax
- **Observed Symptoms:**
  - Soft hyphens and end-of-line hyphens remain split across lines (16 broken hyphenations in Slice A).
  - Broken ligature spacing (`fl ow`, `confl ict`, `fi ring`, `difﬁ cult`).
  - Table and diagram separation lacks clean blank line boundary spacing.

# What the free PDF converters do where they beat us (2026-10-31)

filegoblin was benchmarked against MinerU, marker and docling as black boxes
(`docs/COMPARISON.md`). This pass read their source to find the technique
behind each place they win, and tried the ones that fit filegoblin's geometry:
pdf_oxide words, lines and vector edges, plus PP-DocLayoutV3 regions in smart
mode. The numbers for each attempt are in the 2026-10-31 ADR entry.

**Versions read** (installed under `~/Library/Caches/fgv/alts/<tool>/lib/python3.12/site-packages/`):
- MinerU 4.1.2, with its geometry dependency docvortex 0.6.8;
- marker 2.1.0, with surya 0.23.0 and pdftext 0.7.1;
- docling 2.145.0, with docling-core 2.105 and docling-ibm-models 4.0.4.

**Licences.** Ideas only. No code or data file was copied, and every Rust
change is written fresh.
- MinerU: Apache-2.0 with extra terms (online services must credit it; a
  commercial licence is needed above a size threshold).
- docvortex: MIT AND Apache-2.0 AND BSD-3-Clause.
- docling: MIT.
- marker: GPL-3.0/Apache code (this build's headers say Apache-2.0); its model
  weights carry their own terms.

**Two surprises in the installed packages.**
- MinerU 4.1's own `mineru/` package holds the model pipeline. Nearly all of
  its geometry (reading order, furniture, paragraphs, unruled tables) lives in
  `docvortex`, partly compiled (`docvortex/_native.abi3.so`), so for some of
  it only the docstrings can be read.
- The installed marker is not stock upstream. It has fast and balanced modes,
  an rf-detr layout model and a surya VLM, and its comments cite olmOCR-bench
  scores.

**What "where they beat us" turned out to be.** On the dev split, filegoblin
smart now fails 24 multi-column tests. Of those, 22 are a sentence that is
missing or misread, mostly on scanned pages where the OCR misreads letters.
Only 2 are true order errors, and MinerU and docling make the same number of
order errors. Long tiny text is no longer a gap: since the 4 GB-worker fix,
filegoblin scores 89.3 against MinerU's 85.7.

## Findings

"Carries over" says whether the idea fits our geometry. "Result" is what
happened when it was tried. All test counts are on the olmOCR-bench dev split.

| Area | Technique | Where upstream | Carries over? | Result |
|---|---|---|---|---|
| 1 Headers/footers | **Position-only marginalia.** A block wholly inside the top 8% or bottom 13% of the page counts as furniture when it is no taller than 3.5% of the page, has at most 150 characters, and lies outside the body. | marker `processors/marginalia.py`, `MarginaliaProcessor.__call__` | Yes. We have no blocks on the fast path, so a row counts as its own block when a gap at least its own height separates it from the next row inward (1.5× for the second row in). Guards: no rows set larger than body text, no list items, no captions, no footnotes, no full-width prose that ends mid-sentence. | **Kept** (`pdf_furniture::marginal_lines`). Off: headers/footers 69.2 → 75.8 (+6/−0); test split 68.3 → 74.0; gauntlet R2 +0.2. Smart: no change on dev or the gauntlet. |
| 1 | **Repeated first/last blocks.** A text is "common" when it appears more than 3 times and on ≥20% of pages or 3 pages running; digits are stripped first. | marker `processors/ignoretext.py` | Already present: our masked signatures, global or chapter-run recurrence. | Nothing to take. |
| 1 | **Marginal candidates.** Top 15%/bottom 10% bands; page numbers whose values differ by the page gap; text matched with digits masked and fuzzy ratio ≥0.92; odd/even pages mirrored. | docvortex `analyzers/native/pdf/auxiliary_text.py` | Mostly already present: signatures, folio advance. Two parts are new: the fuzzy match and odd/even mirroring. | Not tried: no dev or gauntlet failure needs them. |
| 1 | **Rule-delimited headers.** The topmost rule in the top 15% that spans ≥60% of the width marks the lines above it as header. | docvortex `auxiliary_text.py`, `_classify_rule_delimited_headers` | Yes. | Not tried: the remaining smart failures (`ESG`, `ANEC`, "Exposé No XXV") have no such rule. |
| 1 | **Header/footer relabelling.** The layout model's header/footer labels are relabelled by position: everything above the lowest header, or above a top page number, is header. | MinerU `model/layout/pp_doclayout_v2_base.py`, `_relabel_header_footer_boundary_blocks` | Yes, for smart mode (PP-DocLayoutV3 has the same labels). | Not tried. A candidate for smart headers. |
| 1 | **Layout labels only.** docling keeps no header/footer rule of its own: page header and page footer labels go to a furniture layer that the markdown leaves out. | docling `models/stages/reading_order/readingorder_model.py`; `docling_core` content layers | Already what smart mode does with PP-DocLayoutV3. | Nothing to take. |
| 2 Reading order | **Rule-based predecessor graph.** Same-row links follow the content stream. Each block's predecessors are the blocks above it in the same column. A block widens to a sole neighbour, by at most 15% of page width. Chains are walked depth-first, and a block is read only after its predecessors. | docling `models/postprocessing/reading_order_rb.py`, `ReadingOrderPredictor` | Yes, on layout regions. | **Rejected.** Ordering smart regions with it changed 65 dev pages but no test. Three dense OCR pages timed out (the link pass is cubic in the region count). PP-DocLayoutV3's order head is as good on dev. |
| 2 | **Content-stream order.** Blocks are sorted by the smallest character index they contain; the docstring claims 75% on olmOCR order tests against 56% for a learned order. | marker `builders/line.py`, `order_blocks_by_reading_order` | Yes: word indices from glyph order. | **Rejected.** Multi-column 77.4 → 63.2 in smart mode. |
| 2 | **XY-cut++.** Spanning blocks are pulled out first, then the page is cut at the largest projection gap (horizontal first when blocks cover >90% of the region), and spanning blocks are put back. | docvortex `analyzers/native/_shared/xycut.py`, `sort_entries` | Partly. Tried on the fast path without the spanning-block step: words are XY-cut into column regions, each converted by pdf_oxide on its own. A strip counts as a gutter only when both sides hold rows of several words and fewer than three rules cross it (docling's separators). | **Rejected** for now. Off: +11/−7 (long tiny text +12.5, multi-column −0.9). Gauntlet: FORM_1 −5.7, SCI_2 −0.8. Converting regions separately also loses pdf_oxide's heading levels. The lead: the spanning-block step (a pull quote across the gutter split one page's columns into bands). |
| 2 | **Paragraphs across floats and columns.** A paragraph that stops mid-sentence continues in the next text block past captions, figures, tables and footnotes, when that block starts in lowercase. | docling `reading_order_rb.py` `predict_merges`; docvortex `postprocess/paragraphs.py` `merge_para_text_blocks`; marker `processors/text.py` | Yes: smart mode already joins adjacent blocks; skipping floats is a small change in `pdf_vision::assemble`. | **Rejected.** It moved the continuation on 8 dev pages without changing a test, and on the gauntlet it also moved a patent's stray line numbers (PATENT_1 −0.02). It gained nothing anywhere. |
| 3 Long tiny text | **Hybrid lines.** DBNet line boxes on each layout block are filled with the PDF's characters. Also: tight word-space insertion at 0.25 em, and de-hyphenation that keeps the hyphen after compound prefixes (non-, self-, …). | docvortex `document/pdf/text/spacing.py`, `foundation/_text.py`; MinerU `backend/analysis/pdf/text/content.py` | Partly: our glyph words already break at gaps relative to each line's letter spacing. | Not tried. We lead this category (89.3 vs 85.7) since the 4 GB-worker fix, which recovered MinerU's whole lead. |
| 4 Tables without rules | **Unruled numeric columns.** At least 3 rows of 3–8 numeric fragments, aligned and at a steady pitch, plus the header line above. The grid comes from the median gaps of rows with the modal cell count. | docvortex `_table_recovery/table_detection.py`, `_detect_unruled_numeric_column_tables`; `text_grid.py` | Yes. Rows with ≥3 cells split at gutters, mostly numbers, at a pitch ≤2.5 em; up to 3 short header lines. The table is given the rules it does not draw and goes through the booktabs reader, so every guard there applies. | **Kept** (`pdf_table::unruled_tables`). Off: tables 36.0 → 36.9 (+1/−0); gauntlet FIN_1 +2.2 (off), +0.2 (smart fallback pages). On the test split it first lost 6 tests on two pages (a table beside prose took in the prose column; a paragraph's last line became the header). Both were fixed (table kept to its own columns; header measured in the table's type size, no captions): dev and gauntlet unchanged, test split off tables 32.5 → 35.2 (+63/−0 overall). |
| 4 | **Sweep and score.** Candidate grids come from the modal token count and from ink projections at three thresholds. Each grid is scored on column type purity, compound cells, fill and header agreement, and accepted at ≥0.5 (fast) or ≥0.75 (balanced). | marker `processors/table_recon.py` | Needs a table box from a layout model, which the fast path does not have. Our booktabs gutter logic covers the column part. | Not tried. A candidate for smart mode when TATR fails. |
| 4 | **Continuation rows.** A row joins the row above when its filled columns are a strict subset of that row's and the gap is ≤0.4 glyph height. | docvortex `text_grid.py` (row grouping); marker `table_recon.py` `_attach_wrapped_lines` | Needs row geometry. pdf_oxide's own tables give text only. | Not tried. Would fix wrapped cells such as `tables/a70c1fc…` on the fast path. |
| 4 | **Cell snapping.** Model cells are snapped to median column alignment, and orphan words go to the nearest row and column band. | docling-ibm-models `tableformer/data_management/matching_post_processor.py` | Model-bound (TableFormer cells). | Nothing to take. |
| 5 Scans/handwriting | **Repetition-loop guard.** A tail of length L repeated more than 4(1 + 3/L) times is cut. | surya `recognition/__init__.py`, `_detect_repeat_loop` | Not needed. PP-OCR is CTC and cannot loop, and no formula on dev loops (the 7 hits were table cells between `$`). | Nothing to fix. |
| 5 | **Broken text layer.** Text is judged by whitespace/alnum ratios, replacement characters, vowel share and Latin-extended share. | marker `providers/pdf.py` `detect_bad_ocr`; docling `utils/text_quality.py` `is_broken_text_layer` | Already present as `pdf_textq` (single letters, garbage tokens, unmapped glyphs). | Nothing to take. |
| 5 | **OCR clean-up.** Lines with confidence <0.5 are dropped; crops taller than 1.5× their width are turned; LaTeX braces and `\left`/`\right` are balanced. | MinerU `backend/analysis/pdf/ocr.py`; `mineru_vl_utils/post_process/` | Yes, but they are small. | Not tried. Our scan losses are misread handwriting and letters; none would change that. |
| 5 | **Handwriting and scanned math.** A 1.2B VLM (MinerU2.5-Pro) or surya reads whole blocks. | MinerU VLM backend; surya | No. Neither fits the ONNX runtime, the size budget or the licence terms. | Out of scope, as briefed. |

## Where the remaining leads come from

- **Smart multi-column (77.4 vs MinerU 79.2):** OCR misreads on scanned pages
  ("two sraff members"), a drop cap merged into the next line ("TSeadisplayed",
  `multi_column/0b045ab…`), and the IRS pages whose glyph widths pdf_oxide
  gets wrong. None is an ordering rule; the order experiments above confirm it.
- **Smart headers/footers (95.6 vs 98.9):** three pages where a short top or
  bottom line (`ESG`, `ANEC`, "Exposé No XXV 26 Mai 1978") is not labelled
  furniture. MinerU's position relabelling of model labels is the matching
  idea, not yet tried.
- **Fast-path multi-column (50.9 vs pymupdf4llm 77.4):** pdf_oxide's converter
  interleaves lines of neighbouring columns. Column regions converted one at a
  time fix the dictionary pages but cost a form and a paper. The missing
  pieces are XY-cut++'s spanning-block step and a way to keep heading levels
  when regions are converted separately.
- **Fast-path tables (36.9 vs 55.9):** most remaining misses are two-column
  text tables (date lists, key–value) and wrapped cells. Neither has the
  numeric signal the unruled detector relies on.

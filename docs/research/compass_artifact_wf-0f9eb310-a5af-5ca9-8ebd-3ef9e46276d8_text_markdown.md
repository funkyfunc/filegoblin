# filegoblin Gauntlet: A Verified Corpus of Hard Conversion Files, Licenses, and Generation Recipes

The strongest foundation for the gauntlet is olmOCR-bench (ODC-BY-1.0, 1,403 PDFs with 7,010 unit tests) for PDF, pandoc and Apache POI/Tika/docling/MarkItDown test fixtures plus specific GitHub bug reports for Office formats, and locally generated files for everything else. Most famous layout benchmarks (OmniDocBench in particular) are research-only and must not be committed to a public repo.\[1\]

## TL;DR
- **Use as the redistributable core:** olmOCR-bench (ODC-BY-1.0), DocLayNet (CDLA-Permissive-1.0), PubLayNet/PubTabNet annotations (CDLA-Permissive), SciTSR (MIT), Govdocs1 ("freely redistributable"), US-government PDFs, and Apache-2.0/MIT parser fixtures. OmniDocBench is "for research purposes only and not for commercial use" (CC-BY-NC-4.0), so fetch it at test time rather than vendoring it.
- **Office-format hard cases are best sourced from bug trackers and fixtures,** because no benchmark covers them. MarkItDown has open or recent issues on grouped PPTX shapes, chart `None` values, shape reading order, and OMML math. Docling has issues on merged cells, multi-column flow, duplicated text boxes, and CJK tables. Pandoc has issues on tracked changes and comments.
- **Thin categories need local generation:** XLSX (pivots, cached formula values, named ranges, 100k+ rows), CSV encodings, PPTX SmartArt, hidden slides and alt text, broken-ToUnicode PDFs, XFA, and RTL PDFs. Author the clean target first, then render the messy file. I could confirm about 60 manifest rows (several of them datasets of thousands of pages); the recipes below add about 55 generated files, bringing the total to roughly 115.

## Key Findings

1. **License is the main constraint, not availability.** The OmniDocBench repo's evaluation code is Apache-2.0, but its dataset is "for research purposes only and not for commercial use", and the Hugging Face card was amended to cc-by-nc-4.0.\[1\]\[2\] The derivative Real5-OmniDocBench is labelled Apache-2.0 but says use of inherited materials "remains subject to the applicable OmniDocBench and source-document terms".\[3\] Treat both as download-only.
2. **olmOCR-bench is the best single PDF asset for regression tracking.** It ships unit-test-style ground truth rather than reference Markdown, so a converter can't fail just by formatting differently.\[4\] Its splits map directly onto your categories: arxiv_math, headers_footers, long_tiny_text, multi_column, old_scans, old_scans_math, and table_tests.\[5\] The 1.5-preview adds synthetic tests, bringing the total to 28,770 tests across 3,401 PDFs and 15 categories. Those tests were made by re-rendering PDFs into HTML with Claude, so their ground truth is model-derived.\[6\]
3. **The "FinTabNet license" question resolves to CDLA-Permissive,** according to IBM's DAX page (89,646 pages, 112,887 tables, 16 GB). The underlying documents, however, are S&P 500 earnings reports whose copyright belongs to the companies.\[7\] Redistribute the annotations freely, but treat the PDFs with caution.
4. **Known weak spots in the tools you'll be compared against are well documented:**
   - **MarkItDown:** text inside grouped PPTX shapes was dropped (issue #332, fixed by PR #331).\[8\] Missing chart data points render as the literal "None" (#2526).\[9\] Same-row shapes are read top-first instead of left-to-right (#2571). One unrecognized shape type aborts the whole deck (#2569). A DOCX math delimiter with several arguments keeps only the last one (#2573). Heading 7–9 styles lose heading structure (#2530).\[10\] Text inside PPTX images isn't extracted without an LLM (#1125).\[11\] Recent CSV fixes covered the widest-row column count (#2430) and CR-only line endings (#2412).\[12\]
   - **Docling:** fails on merged cells and densely spaced columns (#2756).\[13\] Text flows across columns in 3-column financial layouts (#2067).\[14\] Merged DOCX table cells repeat in HTML output (#791).\[15\] Table cells in a DOCX go missing (#1105).\[16\] A text box anchored in a table cell is emitted twice, from both the DrawingML and VML fallback copies (#4569).\[17\] Rows misalign on CJK PDFs with a partial text layer, a regression from v2.115.0 to v2.116.0 (#4083).\[18\]
   - **Pandoc:** comments on tracked insertions are dropped (#9833).\[19\] Comments spanning multiple lines are mangled (#4270).\[20\] Comments on equations are lost (#3842).\[21\] Leading and trailing spaces at insertion edges were mishandled (#4427, fixed in PR #11904 in Sept–Oct 2026).\[19\]\[20\]\[21\]\[22\]\[23\]

## Manifest

Verification note: every URL below was returned by a search or fetch during this research, either mine or my sub-researcher's. GitHub blocked direct page fetches, so some individual fixture pages are "path attested" (seen in search snippets, or cited at exact paths in other issues) rather than byte-verified. Run `git clone --depth 1` or the GitHub contents API to confirm sizes before you freeze the manifest.

### A. Benchmark datasets (PDF/images)

| Name | URL | Format | Size / pages | Tags | Why it's hard | Ground truth | License | Redistributable? |
|---|---|---|---|---|---|---|---|---|
| olmOCR-bench | https://huggingface.co/datasets/allenai/olmOCR-bench | PDF + JSONL tests | 1,403 PDFs, 7,010 tests | multi-column, equations, tables, scans, headers/footers, tiny text | Real arXiv math, old scans, dense tables | Yes: unit tests for text presence/absence, reading order, table cells, and math | ODC-BY-1.0 | Yes, with attribution (check the source-PDF terms of any PDF you vendor)\[4\] |
| olmOCR-bench-1.5-preview | https://huggingface.co/datasets/allenai/olmOCR-bench-1.5-preview | PDF + JSONL | 3,401 PDFs, 28,770 tests, 15 categories | as above + synthetic hard cases | Adds harder synthetic scenarios | Yes, but the synthetic tests are LLM-derived | ODC-BY | Yes, with attribution\[6\] |
| OmniDocBench (v1.0–v1.6) | https://github.com/opendatalab/OmniDocBench ; https://huggingface.co/datasets/opendatalab/OmniDocBench | page images + JSON | 981 pages (v1.0); v1.6 adds 296 harder pages | multi-column, equations, nested tables, newspapers, handwriting, CJK | 9 document sources, 19 layout categories, attribute labels | Yes: text, LaTeX formulas, HTML tables, reading order | Code Apache-2.0; data research-only / CC-BY-NC-4.0 | **No** — fetch at runtime\[1\]\[24\] |
| Real5-OmniDocBench | https://huggingface.co/datasets/PaddlePaddle/Real5-OmniDocBench | photos/scans | 1K–10K images | scans, skew, low-quality capture | OmniDocBench pages physically re-captured | Yes (inherited) | Apache-2.0 label, but bound by OmniDocBench terms | **No (unclear)**\[3\] |
| DocLayNet | https://huggingface.co/datasets/ds4sd/DocLayNet ; license https://github.com/DS4SD/DocLayNet/blob/main/LICENSE | PNG + PDF + COCO JSON | 80,863 pages, 11 classes | layout, headers/footers, tables, finance/patents/law/manuals | Human-annotated, diverse non-academic layouts | Layout boxes only (no reading-order text) | CDLA-Permissive-1.0 | Yes (include the license text)\[25\] |
| ICDAR 2023 DocLayNet competition | https://huggingface.co/datasets/ds4sd/icdar2023-doclaynet | images + COCO | <1K | layout, corporate docs | Held-out robust-layout test set | Layout boxes | Apache-2.0 (HF label) / CDLA | Yes\[26\] |
| PubLayNet | https://github.com/ibm-aur-nlp/PubLayNet | images + JSON | 360K+ document images built from 1M+ PMC articles (arXiv:1908.07836) | multi-column scientific | Auto-annotated from PDF↔XML matching | Layout boxes and polygons | Annotations CDLA-Permissive-1.0; images from the PMC OA commercial-use subset | Yes (annotations) |
| PubTabNet | https://github.com/ibm-aur-nlp/PubTabNet | PNG + HTML | 568k table images with structured HTML (arXiv:1911.10683) | merged headers, spanning cells | Complex scientific tables | Yes: HTML table structure and content (origin of TEDS) | LICENSE.md in repo (CDLA-Permissive per IBM; verify) | Yes (annotations) |
| FinTabNet | https://dax-cdn.cdn.appdomain.cloud/dax-fintabnet/1.0.0/data_preview/index.html | PDF + JSON | 89,646 pages, 112,887 tables, 16 GB | financial tables, borderless, multi-page | Dense S&P 500 earnings tables | Cell structure | CDLA-Permissive | Annotations yes; **PDFs are company-copyrighted — flag**\[7\] |
| FinTabNet.c | https://huggingface.co/datasets/bsmock/FinTabNet.c | images + annotations | subset of FinTabNet | financial tables | Canonicalized and corrected FinTabNet | Cell structure | CDLA-Permissive-2.0 | Annotations yes\[27\]\[28\] |
| SciTSR (+ SciTSR-COMP) | https://github.com/Academic-Hammer/SciTSR | PDF + JSON | 15,000 tables (12k train / 3k test) | merged cells, complicated tables | Structure labels taken from LaTeX source | Yes: cell row/column spans and adjacency relations | MIT (repo) | Labels yes; the arXiv-derived PDFs depend on each paper's license — flag\[29\] |

### B. Real-world file corpora

| Name | URL | Format | Size | Tags | Why it's hard | Ground truth | License | Redistributable? |
|---|---|---|---|---|---|---|---|---|
| Govdocs1 | https://digitalcorpora.org/corpora/file-corpora/files/ (zips: https://downloads.digitalcorpora.org/corpora/files/govdocs1/zipfiles/) | mixed: PDF, DOC, XLS, PPT, CSV, HTML… | 1M files minus ~13,722 removed for California over-collection (Digital Corpora); 231,000+ PDFs, 1,000 ZIPs ≈308 GB (PDF Association index) | all legacy formats, real producers | Unbiased .gov crawl with odd producers and old formats | No | "freely redistributable" "to the best of our knowledge" | Yes (best effort) — good for crash/hang testing |
| SafeDocs CC-MAIN-2021-31-PDF-UNTRUNCATED | https://digitalcorpora.org/corpora/file-corpora/cc-main-2021-31-pdf-untruncated/ | PDF | 7.9M unique PDFs (8.3M URLs) in 7,933 ZIPs, nearly 8 TB uncompressed (Digital Corpora) | everything PDF, including broken files | The web as it is; untruncated refetches | No (metadata CSVs only) | Common Crawl terms plus Digital Corpora ToU | **No** — files are third-party copyrighted web content |
| UNSAFE-DOCS (incl. `issue_trackers` subset) | https://digitalcorpora.org/corpora/file-corpora/unsafe-docs-cc-main-2021-31-unsafe/ | PDF + other | 5.3M PDFs + 180K other files | malformed PDFs, parser-bug files | Includes PDFs crawled from parser issue trackers and synthetic malforms | No | Digital Corpora ToU | **No**; may contain malicious files — sandbox it\[30\]\[31\] |
| PDF Association corpora index | https://github.com/pdf-association/pdf-corpora | index | — | — | Map of further PDF corpora | — | — | n/a\[32\] |

### C. Parser test fixtures (specific files)

| Name | URL | Format | Size | Tags | Why it's hard | Ground truth | License | Redistributable? |
|---|---|---|---|---|---|---|---|---|
| Tika footnotes.docx | https://github.com/apache/tika/blob/master/tika-parsers/src/test/resources/test-documents/footnotes.docx | DOCX | 12.5 KB | footnotes | Footnote text lives in a separate part | Partial (Tika unit-test assertions) | Apache-2.0 | Yes\[33\] |
| Tika test-outlook.msg | https://github.com/apache/tika/blob/master/tika-parsers/src/test/resources/test-documents/test-outlook.msg | MSG | 19.5 KB | email | OLE2 compound email | Partial (unit tests) | Apache-2.0 | Yes\[34\] |
| Tika testCSV.csv | https://github.com/apache/tika/blob/master/tika-parsers/src/test/resources/test-documents/testCSV.csv | CSV | small | CSV baseline | Baseline for detection | Trivial | Apache-2.0 | Yes\[35\] |
| POI TwoSheetsOneHidden.xls | https://github.com/apache/poi/blob/master/test-data/spreadsheet/TwoSheetsOneHidden.xls | XLS | small | hidden sheets | Should the hidden sheet be emitted or flagged? | Yes: Sheet1!A1="Sheet1A1", Sheet2!A1="Sheet2A1" | Apache-2.0 | Yes\[36\] |
| POI HeaderFooterUnicode.doc | https://raw.githubusercontent.com/apache/poi/942d95d85b15d0dfdb3bc9ba1b4f273f277757c8/test-data/document/HeaderFooterUnicode.doc | DOC | small | headers/footers, Unicode | Legacy binary Word, non-ASCII headers | Partial | Apache-2.0 | Yes\[37\] |
| POI footnote.doc | https://raw.githubusercontent.com/apache/poi/942d95d85b15d0dfdb3bc9ba1b4f273f277757c8/test-data/document/footnote.doc | DOC | small | footnotes | Binary-format footnotes | Partial | Apache-2.0 | Yes\[37\] |
| POI Bug46817.doc | apache/poi `test-data/document/Bug46817.doc` (path attested) | DOC | small | text boxes | 661-character text box | Partial | Apache-2.0 | Yes\[37\] |
| POI ThreeColHeadFoot.doc | apache/poi `test-data/document/ThreeColHeadFoot.doc` (path attested) | DOC | small | headers/footers, columns | Three-column header/footer layout | Partial | Apache-2.0 | Yes\[37\] |
| POI EmbeddedDocument.docx | apache/poi `test-data/document/EmbeddedDocument.docx` (path attested) | DOCX | small | embedded objects | Excel workbook embedded in Word | Partial | Apache-2.0 | Yes\[38\] |
| pandoc test/docx suite | https://github.com/jgm/pandoc/tree/main/test/docx | DOCX + `.native` | ~100+ small files | tracked changes, comments, fields, lists, notes, VML images, diagrams | Each file isolates one Word feature | **Yes**: paired pandoc `.native` AST golden files | GPL-2.0-or-later | Yes, but copyleft — keep in a GPL-licensed subfolder\[39\] |
| ↳ track_changes_insertion.docx / track_changes_deletion.docx | in the folder above | DOCX | small | tracked changes | Accept/reject/all semantics; the insertion golden file was regenerated Oct 5, 2026 (PR #11904) | `.native` | GPL | Yes (GPL)\[22\] |
| ↳ comments.docx, comments_warning.docx | in the folder above | DOCX | small | comments | Comment ranges cross blocks | `.native` | GPL | Yes (GPL) |
| ↳ empty_field.docx, cross_reference.docx, instrText_hyperlink.docx | in the folder above | DOCX | small | fields, TOC-like fields | Field codes vs. cached results | `.native` | GPL | Yes (GPL) |
| ↳ lists.docx, lists_continuing.docx, lists-compact.docx, enumerated_headings.docx | in the folder above | DOCX | small | multi-level lists | numbering.xml restarts and continuation | `.native` | GPL | Yes (GPL) |
| ↳ link_in_notes.docx, diagram.docx, image_vml.docx, drop_cap.docx, i18n_blocks.docx, normalize.docx | in the folder above | DOCX | small | footnotes, diagrams/shapes, VML, RTL/i18n, malformed XML | normalize.docx contains invalid XML (`<w:i/ >`) | `.native` | GPL | Yes (GPL)\[40\] |
| docling fixtures | `tests/data/docx/word_sample.docx`, `lorem_ipsum.docx`, `unit_test_headers_numbered.docx`; `tests/data/pptx/powerpoint_sample.pptx`; `tests/data/html/wiki_duck.html`; `tests/data/pdf/2206.01062.pdf` in https://github.com/docling-project/docling | DOCX/PPTX/HTML/PDF | small | numbered headings, boilerplate HTML, multi-column paper | Docling's own regression set | Yes: docling ground-truth JSON/MD in repo | MIT | Yes (the PDF is the DocLayNet paper — check the arXiv license)\[41\]\[42\]\[43\] |
| docling powerpoint_unrecognized_shape.pptx | docling `tests/data` (cited in MarkItDown PR https://github.com/microsoft/markitdown/pull/2570) | PPTX | small | unusual shapes | Shape type python-pptx can't map | Yes: "Text in an unrecognized shape" | MIT | Yes\[44\] |
| MarkItDown test_files | https://github.com/microsoft/markitdown (packages/markitdown/tests/test_files: test.docx 136 KB, test.xlsx 12 KB, test.pptx) | DOCX/XLSX/PPTX | small | tables, multi-sheet | Seeded content for loss detection | Yes (test assertions) | MIT | Yes\[45\] |
| MarkItDown PPTX missing-text fixtures | https://github.com/microsoft/markitdown/pull/2594 (pptx_empty_title_run, pptx_empty_body_run, pptx_notes_empty_run, pptx_notes_missing_body_frame) | PPTX | small | speaker notes, empty runs | Absent notes frame, empty `<a:t/>` | Yes | MIT | Yes (PR — confirm it's merged)\[46\] |
| pdf.js f1040.pdf.link | https://github.com/mozilla/pdf.js/blob/master/test/pdfs/f1040.pdf.link | link → IRS Form 1040 (archive.org) | 84-byte link | fillable forms | Real AcroForm-style government form | No | US government work | Yes (download the target PDF; it's public domain)\[47\] |
| pdf.js 160F-2019.pdf | https://github.com/mozilla/pdf.js/blob/master/test/pdfs/160F-2019.pdf | PDF | 321 KB | forms (likely; not inspected) | Real-world form regression file | Rendering refs only | Apache-2.0 repo; content provenance unclear | Probably — verify\[48\] |
| pdf.js transparent.pdf, helloworld-bad.pdf | https://github.com/mozilla/pdf.js/blob/master/test/pdfs/transparent.pdf ; https://github.com/mozilla/pdf.js/blob/master/test/pdfs/helloworld-bad.pdf | PDF | 55.3 KB; small | transparency, malformed | Broken-structure robustness | Trivial text | Apache-2.0 | Yes\[49\]\[50\] |
| pdf.js bug1811668_reduced.pdf, checkbox-bad-appearance.pdf, issue15910.pdf | `test/pdfs/` in mozilla/pdf.js (path attested) | PDF | small | encoding (named-encoding font), forms | Text-selection encoding bug; bad widget appearance | Partial | Apache-2.0 | Yes\[51\] |
| pdf.js pdf.pdf.link (PDF Reference 1.7) / ocs.pdf.link (Cyrillic) | `test/pdfs/` (path attested) | link | 1,300+ pages (unverified) | very large; non-Latin | Huge real spec; Cyrillic text | No | Adobe copyright / unknown | **No** — link only\[52\]\[53\] |

### D. Bug-tracker items (files that broke converters)

| Issue | URL | Format | Tags | Why it's hard | Ground truth | License of attachment | Redistributable? |
|---|---|---|---|---|---|---|---|
| docling.rs merged.xlsx | https://github.com/docling-project/docling.rs/issues/410 (attachment merged.xlsx, ~4.7 KB) | XLSX | merged cells | A1:C2 merge loses row_span/col_span | Yes: 1 cell, row_span 2, col_span 3 | Self-authored minimal file, no stated license | Unclear — regenerate it (recipe X3)\[54\] |
| docling textbox in cell | https://github.com/docling-project/docling/issues/4569 (python-02-textbox-in-cell.zip) | DOCX | text boxes, tables | DrawingML + VML fallback means duplicated text | Yes: `MARKW` should appear once | Word-authored repro, no license | Unclear — regenerate it\[17\] |
| docling.rs .doc textbox/header/footnote | https://github.com/docling-project/docling.rs/issues/535 | DOC/DOCX pair | text boxes, headers, footnotes | Same content gives different output by format | Yes: MARKFN/MARKTB1/MARKHDR markers | No license | Unclear\[55\] |
| docling missing table data | https://github.com/docling-project/docling/issues/1105 (test.docx) | DOCX | tables | Table rows output as "[ MISSING ]" | Visible in the source | Appears to be an Australian MBS government document | Unclear — flag\[16\] |
| docling merged cells / dense columns | https://github.com/docling-project/docling/issues/2756 | PDF | merged headers | rowspan/colspan fragmentation | Manual | Unknown | No\[13\] |
| docling 3-column financial flow | https://github.com/docling-project/docling/issues/2067 | PDF | multi-column | Text flows across columns | Manual | Unknown | No\[14\] |
| docling CJK partial text layer | https://github.com/docling-project/docling/issues/4083 | PDF | CJK, tables | Regression between v2.115.0 and v2.116.0 | Manual | Unknown | No\[18\] |
| docling DOCX merged-cell repetition | https://github.com/docling-project/docling/issues/791 | DOCX | merged cells | Repeated columns in HTML | Manual | Unknown | No\[15\] |
| pandoc comments on insertions | https://github.com/jgm/pandoc/issues/9833 (comments_test.docx) | DOCX | comments + tracked changes | Comment on an unaccepted insertion is dropped | Yes: 2 comments expected | Minimal user file | Unclear — regenerate\[19\] |
| pandoc multiline comments | https://github.com/jgm/pandoc/issues/4270 | DOCX | comments | Comments spanning paragraphs | Described in issue | Unknown | No\[20\] |
| MarkItDown grouped shapes | https://github.com/microsoft/markitdown/issues/332 | PPTX | grouped shapes | Text inside groups dropped | Regenerate (recipe P2) | — | —\[8\] |
| MarkItDown chart None | https://github.com/microsoft/markitdown/issues/2526 | PPTX | native charts | Missing points printed as "None" | Yes: code repro in issue | Code (MIT context) | Yes (regenerate from repro)\[9\] |
| MarkItDown shape order / unrecognized shape / OMML / H7–9 | https://github.com/microsoft/markitdown/issues (#2571, #2569, #2573, #2530) | PPTX/DOCX | reading order, robustness, equations, headings | See Key Findings | Repro descriptions | — | Regenerate\[10\] |

### E. Large and government documents (public domain)

| Name | URL | Format | Size / pages | Tags | Why it's hard | Ground truth | License | Redistributable? |
|---|---|---|---|---|---|---|---|---|
| OMB Budget Appendix (current, single PDF) | https://whitehouse.gov/omb/information-resources/budget/appendix/ | PDF | 1,340 pages, 16 MB (probably FY2027 — check) | very large, tables spanning pages, repeated headers/footers | Dense account tables on every page | Partial: budget data is also published as spreadsheets/XML on govinfo | US government work | Yes\[56\] |
| FY2026 Budget Appendix | https://www.govinfo.gov/content/pkg/BUDGET-2026-APP/pdf/BUDGET-2026-APP-1.pdf | PDF | 13.1 MB | very large, tables | As above | Partial | US government work | Yes\[57\] |
| GAO High-Risk Series GAO-25-107743 | https://www.gao.gov/products/gao-25-107743 ; HTML: https://files.gao.gov/reports/GAO-25-107743/index.html | PDF + HTML | 323 pages | headers/footers, vector charts, sidebars | Two-column report with callouts | **Yes: the official HTML rendering can serve as derived ground truth** | US government work | Yes\[58\] |
| arXiv papers with source (e.g., 2412.07626 OmniDocBench, 1908.04729 SciTSR, 2502.18443 olmOCR, 2501.17887 Docling) | https://arxiv.org/abs/2412.07626 ; https://arxiv.org/pdf/1908.04729 | PDF + LaTeX e-print | 10–30 pages | multi-column, equations, tables, footnotes | Real typeset science | **Yes: LaTeX source** | Per-paper; the default arXiv license is non-exclusive distribution | Only papers marked CC-BY/CC0 — filter on license metadata |

No 500+-page SEC 10-K or NASA technical report was confirmed with a URL and page count in this research. Use the OMB Appendix for the 500+-page row, and add an EDGAR 10-K after checking it yourself.

## Coverage Summary

| Category | Coverage | Primary sources |
|---|---|---|
| PDF multi-column / equations / footnotes | **Strong** | olmOCR-bench (multi_column, arxiv_math), arXiv CC-BY papers with LaTeX, DocLayNet |
| PDF complex tables | **Strong** | PubTabNet, FinTabNet(.c), SciTSR, olmOCR table_tests, OMB Appendix |
| PDF scans / OCR | **Good** | olmOCR old_scans; Real5 (non-redistributable); recipe R1 |
| PDF headers/footers | **Good** | olmOCR headers_footers, GAO, OMB |
| PDF broken encoding / ligatures | **Thin** | pdf.js bug1811668_reduced.pdf only → recipe R2 |
| PDF forms (AcroForm/XFA) | **Thin** (no XFA) | f1040 link, 160F-2019 → recipe R3 |
| PDF non-Latin / RTL | **Thin** | OmniDocBench (NC), docling #4083 (not redistributable) → recipe R4 |
| PDF 500+ pages | **Adequate** | OMB Appendix (1,340 pages) |
| PDF vector charts | **Thin** | GAO report → recipe R5 |
| DOCX tracked changes / comments / fields / lists / notes | **Strong** | pandoc test/docx (GPL) |
| DOCX text boxes / merged / nested tables | **Moderate** | POI, docling issues → recipes D1–D3 |
| DOCX OMML / SmartArt / embedded charts | **Thin** | POI EmbeddedDocument.docx → recipes D4–D6 |
| PPTX (all rows) | **Thin** | docling/MarkItDown fixtures → recipes P1–P7 |
| XLSX (all rows) | **Thin** | POI hidden sheet, MarkItDown test.xlsx → recipes X1–X9 |
| CSV | **Very thin** | Tika testCSV.csv → recipe C1 |
| Other (HTML, EPUB, ODF, RTF, EML/MSG, ipynb, images) | **Thin** | Tika MSG, docling wiki_duck.html → recipe O1 |

## Generation Recipes (clean target first, then the messy file)

General rule: write `case_X/truth.md` (plus `truth.json` for cell-level data), generate `case_X/input.ext`, and commit both under CC0. Plant unique canary tokens (UUIDs) in every text location (body, notes, text boxes, hidden sheets) so silent drops are detectable. Thunderbit's MarkItDown review used the same approach: it ran MarkItDown's own test files "seeded with UUIDs so I could detect silent content loss" and found that "none of the Office files lost a single UUID sentinel".

**PDF**
- **R1 Scans (10 files):** compile CC-BY arXiv LaTeX or your own Markdown with pandoc. Run `pdftoppm -r 150/300`, then degrade with ImageMagick (`-rotate 1.5`, `-attenuate 0.5 +noise Gaussian`, `-blur 0x0.6`, JPEG quality 30), then rebuild the PDF with `img2pdf`. Use one file per degradation level, plus a mixed native/scanned document. Truth = the source text.
- **R2 Broken encoding (6):** take a clean PDF and use pikepdf to delete `/ToUnicode` from font dictionaries. Add a variant with `fi`/`fl` ligatures, a Type 3 font variant, and a variant whose font subset uses custom glyph names. Truth = the original source.
- **R3 Forms (5):** create AcroForm fields with reportlab or pypdf (text, checkbox, radio, dropdown) and fill some of them. Truth = a field-name→value JSON. XFA can't be generated with open tools, so record it as a known gap and use real government forms only if their licenses are clear.
- **R4 RTL/CJK/mixed (6):** use xelatex with Noto fonts on Arabic, Hebrew, Chinese, Japanese, and mixed English–Arabic UTF-8 source, including bidi numbers. Truth = the source in logical order.
- **R5 Vector charts/flowcharts (5):** export matplotlib charts with `savefig("x.pdf")` (paths, not images; truth = the data CSV) and graphviz `dot -Tpdf` diagrams (truth = the DOT edge list).
- **R6 Headers/footers and long docs (3):** LaTeX `fancyhdr` with running titles and page numbers. Generate a 600-page report from templated Markdown. Truth = the body without the furniture.

**DOCX** (python-docx, raw OOXML where needed)
- **D1 Merged and nested tables:** `cell.merge()` horizontally and vertically, and `cell.add_table()` for nesting.
- **D2 Tracked changes and comments:** inject `<w:ins>`/`<w:del>` runs (python-docx has no API for them), and use python-docx 1.2's comments support. Truth = accepted text plus a change list.
- **D3 Text boxes:** write `wps:txbx` inside `mc:AlternateContent` with a VML fallback; this reproduces docling #4569.\[17\]
- **D4 OMML:** write `$...$` LaTeX in Markdown and run `pandoc -o x.docx`, which produces native OMML. Truth = the LaTeX.
- **D5 Footnotes, multi-level lists, TOC/fields:** use pandoc Markdown footnotes and nested lists, then `pandoc --toc` and LibreOffice headless `--convert-to docx` to refresh the fields.
- **D6 Embedded charts/sheets:** create a chart in python-pptx or openpyxl, then embed it through LibreOffice. SmartArt can't be produced by LibreOffice, so author 3 SmartArt files once in Word and release them as CC0, with truth = the node text hierarchy.

**PPTX** (python-pptx)
- **P1** Flowchart from autoshapes plus `add_connector(...).begin_connect/end_connect`. Truth = the node and edge list.
- **P2** Grouped shapes (nested groups), the case behind MarkItDown #332.\[8\]
- **P3** `add_chart` with `None` points (the MarkItDown #2526 repro).\[9\]
- **P4** Tables with merged cells.
- **P5** Speaker notes, hidden slides (`show="0"` on the slide element), and layout/master placeholder text.
- **P6** Pictures with and without `descr` alt text.
- **P7** A 40-slide dense "consulting" deck with same-row shapes to test reading order (MarkItDown #2571).\[10\]

**XLSX** (openpyxl, then LibreOffice to recalculate)
- **X1** Hidden sheets (`sheet_state="hidden"` / `"veryHidden"`) and hidden rows and columns.
- **X2** Several tables on one sheet with blank gutters.
- **X3** Merged ranges (re-create the docling.rs merged.xlsx: A1:C2).\[54\]
- **X4** Header starting at row 7 under a title block.
- **X5** Formulas. openpyxl writes no cached values, so run `soffice --headless --convert-to xlsx` to compute them, and keep one file *without* cached values to check whether the converter emits formulas, values, or nothing.
- **X6** Pivot tables. openpyxl can't author them, so use Apache POI's `XSSFPivotTable` (documented in POI's quick guide) or LibreOffice UNO.\[59\]\[60\]
- **X7** openpyxl charts.
- **X8** Date serials, including the 1904 epoch, and named ranges via `DefinedName`.
- **X9** 200k-row `write_only` sheet. Truth = a CSV per sheet plus a JSON of hidden/merged metadata.

**CSV (C1, 10 files):** write the same truth table in each variant:
- semicolon, tab, and pipe delimiters
- cp1252/Latin-1, UTF-16LE with BOM, and UTF-8-sig encodings
- quoted fields containing embedded CRLF
- ragged rows
- CR-only line endings (MarkItDown #2412)\[12\]
- a 5,000-column wide file and a 1M-row long file

**Other (O1):**
- boilerplate HTML: wrap a known article in nav, cookie-banner, and footer templates
- EPUB, RTF, and ODT/ODS/ODP from pandoc or `soffice --convert-to`
- EML with attachments from Python's `email` package
- notebooks from `nbformat`
- images of text from PIL-rendered strings

## Scoring Notes

- **Text:** normalized edit distance (OmniDocBench's main metric) and CER/WER on whitespace- and Markdown-normalized output.\[24\]
- **Tables:** TEDS (Tree-Edit-Distance-based Similarity, proposed in the PubTabNet paper, arXiv:1911.10683) plus TEDS-Struct, which ignores content. For XLSX/CSV, use exact cell-grid match against the truth JSON.
- **Formulas:** CDM (OmniDocBench) or normalized LaTeX edit distance.\[24\]
- **Layout:** mAP on DocLayNet boxes, only if you expose layout.
- **Reading order and presence:** olmOCR-bench's unit tests cover text present, text absent (headers/footers), before/after ordering, table cell relations, and math. This is the best model for regression tracking because tests are binary and independent of formatting. Install the harness with `olmocr[bench]`; a third-party harness pins `olmocr[bench]==0.4.27` for reproducibility.\[61\]
- **Structure relations:** SciTSR's adjacency-relation precision and recall for tables.\[62\]
- **Recommendation:** use olmOCR-style binary tests for every gauntlet file (canary presence, forbidden-text absence, order pairs, table-cell pairs). Add TEDS for tables and edit distance as a trend metric. Pin dataset revisions, and store per-file pass/fail so regressions point to specific files.

## Caveats
- GitHub blocked direct page fetches, so several fixture paths are attested through search snippets or other issues rather than byte-verified. Pandoc's GPL and pdf.js's Apache-2.0 licenses are stated from well-known project terms, not re-read from LICENSE files.
- Docling and MarkItDown issue numbers above the ~#2500 / ~#4000 range come from recent (Sept–Oct 2026) reports. Some are open, some have pending fixes, and fixes will move the "known weak spot" baseline.
- An open dataset license (ODC-BY, CDLA) covers the annotations or database, not necessarily the copyright in each underlying PDF.\[63\] Only vendor PDFs whose own license (CC-BY, CC0, US government) is confirmed.
- The confirmed manifest falls short of the 80–150 target in standalone files; the generation recipes are what make up the difference.

## Sources

1. [GitHub - opendatalab/OmniDocBench: \[CVPR 2025\] A Comprehensive Benchmark for Document Parsing and Evaluation · GitHub](https://github.com/opendatalab/OmniDocBench)
2. [opendatalab/OmniDocBench · Add dataset card and link to paper](https://huggingface.co/datasets/opendatalab/OmniDocBench/discussions/2)
3. [PaddlePaddle/Real5-OmniDocBench · Datasets at Hugging Face](https://huggingface.co/datasets/PaddlePaddle/Real5-OmniDocBench)
4. [allenai/olmOCR-bench · Datasets at Hugging Face](https://huggingface.co/datasets/allenai/olmOCR-bench)
5. [README.md · allenai/olmOCR-bench at main](https://huggingface.co/datasets/allenai/olmOCR-bench/blob/main/README.md)
6. [allenai/olmOCR-bench-1.5-preview · Datasets at Hugging Face](https://huggingface.co/datasets/allenai/olmOCR-bench-1.5-preview)
7. [DAX Dataset Preview - FinTabNet](https://dax-cdn.cdn.appdomain.cloud/dax-fintabnet/1.0.0/data_preview/index.html)
8. [PR SUBMMITED 331: PPTX Shape Groups Are not accounted for and any and all text in these shapes is not being included in markdown · Issue #332 · microsoft/markitdown](https://github.com/microsoft/markitdown/issues/332)
9. [pptx: missing chart data points render as the literal string "None" · Issue #2526 · microsoft/markitdown](https://github.com/microsoft/markitdown/issues/2526)
10. [Issues · microsoft/markitdown](https://github.com/microsoft/markitdown/issues)
11. [while i try to convert a .pptx into a markdown it is not extracting the text from the images. · Issue #1125 · microsoft/markitdown](https://github.com/microsoft/markitdown/issues/1125)
12. [Releases · microsoft/markitdown](https://github.com/microsoft/markitdown/releases)
13. [Issues with Table Extraction on Densely Aggregated Columns, Irregular Row Heights, and Merged Cells · Issue #2756 · docling-project/docling](https://github.com/docling-project/docling/issues/2756)
14. [Multi-column layout extraction fails - text flows incorrectly across columns despite table structure recognition enabled · Issue #2067 · docling-project/docling](https://github.com/docling-project/docling/issues/2067)
15. [Incorrect Table Formatting in Converting Word Documents To HTML · Issue #791 · docling-project/docling](https://github.com/docling-project/docling/issues/791)
16. [missing data in single page docx · Issue #1105 · docling-project/docling](https://github.com/docling-project/docling/issues/1105)
17. [Text box anchored in a table cell is duplicated in the output (and emitted before the table) · Issue #4569 · docling-project/docling](https://github.com/docling-project/docling/issues/4569)
18. [Table rows/cells get misaligned on CJK PDFs with a partial/unreliable native text layer since the layout-driven OCR pipeline (v2.116.0, #3710) · Issue #4083 · docling-project/docling](https://github.com/docling-project/docling/issues/4083)
19. [docx comments on tracked-changes insertions not handled properly · Issue #9833 · jgm/pandoc](https://github.com/jgm/pandoc/issues/9833)
20. [Docx Reader with Track Changes does not convert comments than span multiple lines properly · Issue #4270 · jgm/pandoc](https://github.com/jgm/pandoc/issues/4270)
21. [pandoc not extracting comments from equations in a word docx · Issue #3842 · jgm/pandoc](https://github.com/jgm/pandoc/issues/3842)
22. [Docx reader: keep spaces at the edges of tracked changes in the span by raffaelemancuso · Pull Request #11904 · jgm/pandoc](https://github.com/jgm/pandoc/pull/11904)
23. [insertions in docx with leading or trailing spaces are not correctly converted · Issue #4427 · jgm/pandoc](https://github.com/jgm/pandoc/issues/4427)
24. [OmniDocBench v1.0 Benchmark](https://www.emergentmind.com/topics/omnidocbench-v1-0)
25. [DocLayNet / README.md](https://huggingface.co/datasets/docling-project/DocLayNet/blob/refs%2Fpr%2F5/README.md)
26. [ds4sd/icdar2023-doclaynet · Datasets at Hugging Face](https://huggingface.co/datasets/ds4sd/icdar2023-doclaynet)
27. [Hugging Face](https://huggingface.co/datasets/bsmock/FinTabNet.c/discussions)
28. [Dataset Viewer](https://huggingface.co/datasets/bsmock/FinTabNet.c)
29. [GitHub - Academic-Hammer/SciTSR: Table structure recognition dataset of the paper: Complicated Table Structure Recognition · GitHub](https://github.com/Academic-Hammer/SciTSR)
30. [UNSAFE-DOCS (CC-MAIN-2021-31-UNSAFE)](https://digitalcorpora.org/corpora/file-corpora/unsafe-docs-cc-main-2021-31-unsafe/)
31. [File Corpora](https://digitalcorpora.org/corpora/file-corpora/)
32. [GitHub - pdf-association/pdf-corpora: An index of PDF-centric corpora · GitHub](https://github.com/pdf-association/pdf-corpora)
33. [tika/tika-parsers/src/test/resources/test-documents/footnotes.docx at master · apache/tika](https://github.com/apache/tika/blob/master/tika-parsers/src/test/resources/test-documents/footnotes.docx)
34. [tika/tika-parsers/src/test/resources/test-documents/test-outlook.msg at master · apache/tika](https://github.com/apache/tika/blob/master/tika-parsers/src/test/resources/test-documents/test-outlook.msg)
35. [tika/tika-parsers/src/test/resources/test-documents/testCSV.csv at master · apache/tika](https://github.com/apache/tika/blob/master/tika-parsers/src/test/resources/test-documents/testCSV.csv)
36. [XLS: hidden/very-hidden worksheets are dropped entirely from to\_ir() instead of kept-and-flagged like XLSX does · Issue #231 · yfedoseev/office\_oxide](https://github.com/yfedoseev/office_oxide/issues/231)
37. [bug(doc): header, footer, footnote, comment and text-box text is dropped whenever the document has a body · Issue #2054 · xberg-io/xberg](https://github.com/xberg-io/xberg/issues/2054)
38. [PoiSamples/src/main/java/org/apache/poi/xwpf/usermodel/examples/UpdateEmbeddedDoc.java at master · stolbovd/PoiSamples](https://github.com/stolbovd/PoiSamples/blob/master/src/main/java/org/apache/poi/xwpf/usermodel/examples/UpdateEmbeddedDoc.java)
39. [pandoc/test/docx at main · jgm/pandoc](https://github.com/jgm/pandoc/tree/main/test/docx)
40. [\`test/docx/normalize.docx\` contains invalid xml · Issue #10242 · jgm/pandoc](https://github.com/jgm/pandoc/issues/10242)
41. [docling/docs/v2.md at main · docling-project/docling](https://github.com/docling-project/docling/blob/main/docs/v2.md)
42. [docx: inherit heading numId through basedOn and honor numFmt in list markers (docling 2.126 drift) · Issue #363 · docling-project/docling.rs](https://github.com/docling-project/docling.rs/issues/363)
43. [GitHub - ali-workshop/Docling-CheatSheet-and-TestCases: A comprehensive cheat sheet and collection of test cases for Docling, covering document conversion, serialization & chunking, and RAG with various AI development frameworks. Includes detailed examples and code snippets for quick reference. ✨](https://github.com/ali-workshop/Docling-CheatSheet-and-TestCases)
44. [fix(pptx): keep converting when a shape type is unrecognized by aImErYbArRaUlT · Pull Request #2570 · microsoft/markitdown](https://github.com/microsoft/markitdown/pull/2570)
45. [MarkItDown Review: The File-to-Markdown Converter That Isn't](https://thunderbit.com/blog/markitdown-review)
46. [Test PowerPoint missing text with real fixtures and shared vectors by afourney · Pull Request #2594 · microsoft/markitdown](https://github.com/microsoft/markitdown/pull/2594)
47. [pdf.js/test/pdfs/f1040.pdf.link at master · mozilla/pdf.js](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/f1040.pdf.link)
48. [pdf.js/test/pdfs/160F-2019.pdf at master · mozilla/pdf.js](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/160F-2019.pdf)
49. [pdf.js/test/pdfs/transparent.pdf at master · mozilla/pdf.js](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/transparent.pdf)
50. [pdf.js/test/pdfs/helloworld-bad.pdf at master · mozilla/pdf.js](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/helloworld-bad.pdf)
51. [1811712 - Update PDF.js to new version edfdb693e5b11057252f8cef9388d063678d95cd from 2023-01-21 13:02:40](https://bugzilla.mozilla.org/show_bug.cgi?id=1811712)
52. [pdf.js/test/pdfs/pdf.pdf.link at master · mozilla/pdf.js](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/pdf.pdf.link)
53. [pdf.js/test/pdfs/ocs.pdf.link at master · mozilla/pdf.js](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/ocs.pdf.link)
54. [xlsx: merged cells lose row\_span/col\_span in the JSON (text copied into covered cells) · Issue #410 · docling-project/docling.rs](https://github.com/docling-project/docling.rs/issues/410)
55. [docling.rs: \`.doc\` (Word 97-2004) — text of text boxes, headers/footers and footnotes is not extracted · Issue #535 · docling-project/docling.rs](https://github.com/docling-project/docling.rs/issues/535)
56. [Budget Appendix](https://www.whitehouse.gov/omb/information-resources/budget/appendix/)
57. [Budget FY 2026 - Technical Supplement to the 2026 Budget: Appendix, Budget of the United States Government, Fiscal Year 2026 - BUDGET-2026-APP](https://www.govinfo.gov/app/details/BUDGET-2026-APP/summary)
58. [U.S. GAO - High-Risk Series: Heightened Attention Could Save Billions More and Improve Government Efficiency and Effectiveness](https://www.gao.gov/products/gao-25-107743)
59. [Samples/src/main/java/org/devocative/samples/poi/CreatePivotTable.java at master · mbizhani/Samples](https://github.com/mbizhani/Samples/blob/master/src/main/java/org/devocative/samples/poi/CreatePivotTable.java)
60. [Busy Developers' Guide to HSSF and XSSF Features](https://poi.apache.org/components/spreadsheet/quick-guide.html)
61. [GitHub - Unsiloed-AI/unsiloed-olmocr-benchmark: Reproducible olmOCR-Bench harness for OCR/document-parsing services + thin client for Unsiloed Parser's public v3 API · GitHub](https://github.com/Unsiloed-AI/unsiloed-olmocr-benchmark)
62. [Complicated Table Structure Recognition](https://arxiv.org/pdf/1908.04729)
63. [FinQA: A Dataset of Numerical Reasoning over Financial ...](https://aclanthology.org/2021.emnlp-main.300v1.pdf)

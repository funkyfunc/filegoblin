# filegoblin against free PDF-to-Markdown tools

Measured 2026-10-30 on one Apple M4 Pro (14 cores, 24 GB, macOS), filegoblin
v1.16.0 plus the memory ceiling of `4283928` (the build these numbers call
"filegoblin"). Every tool ran in its own virtual environment under
`~/Library/Caches/fgv/alts/`, in the configuration its own documentation gives
for this machine, and was graded by the same two harnesses as filegoblin:

- **olmOCR-bench, dev split** (180 one-page PDFs, 7 categories, 1,121 tests,
  allenai's official scorer): `scripts/olmocr_bench.py run NAME --split dev
  --engine TOOL`, then `score`/`table`.
- **The gauntlet** (29 cases, 17 of them PDFs, graded against hand-checked gold
  excerpts by `scripts/gauntlet_grade.py`): `scripts/run_gauntlet.py --engine
  TOOL`, then `--table`.

`scripts/engines.py` describes each engine (install line, formats, licence);
`scripts/alt_engine_worker.py` runs inside the tool's environment, loads it
once and converts files one at a time. To repeat everything:

```bash
python3 scripts/olmocr_bench.py run cmp-docling --split dev --jobs 1 --engine docling
python3 scripts/run_gauntlet.py --engine docling
```

## The tools

| tool | version | configuration (its default here) | device | licence |
|---|---|---|---|---|
| **filegoblin** `--vision smart` | 1.16.0 + `4283928` | layout (PP-DocLayoutV3), tables (Table Transformer), formulas (pix2text MFR), OCR (PP-OCRv6) on pages that need them | CPU (rten) | MIT OR Apache-2.0 |
| **filegoblin** `--vision off` | same | text layer only, no models | CPU | same |
| MinerU | 4.1.2 | `mineru-kit parse`, tier **standard** (the default): layout and OCR models plus the MinerU2.5-Pro 1.2B VLM through llama.cpp | MPS + Metal | Apache-2.0 with extra terms: a commercial licence above 100 M monthly users or US$20 M monthly revenue; online services must credit MinerU |
| marker | 2.1.0 (surya 0.23.0) | `PdfConverter` defaults: mode **fast** (marker's default on CPU/MPS), the surya VLM through llama.cpp's `llama-server` (b11985) for OCR and equations | MPS + Metal | code Apache-2.0; model weights a modified OpenRAIL-M, free for research, personal use and companies under US$5 M funding/revenue |
| docling | 2.145.0 | `DocumentConverter()` defaults: Heron layout model, TableFormer, OCR chosen automatically (RapidOCR); formula enrichment off | MPS / CPU | MIT |
| pymupdf4llm | 2.0 (PyMuPDF 2.0, pymupdf-layout 2.0) | `to_markdown()` with PyMuPDF Layout (its default when installed) and RapidOCR for pages that need OCR (Tesseract is a system package, so not installed) | CPU (ONNX) | **AGPL-3.0** or Artifex commercial licence |
| markitdown | 0.1.9 | `MarkItDown().convert()` (pdfminer.six for PDFs) | CPU | MIT |
| olmOCR | — | not run: its pipeline needs an NVIDIA GPU with ≥ 12 GB (README); the numbers below are **published, not measured** | — | Apache-2.0 |

AGPL matters to anyone shipping a product: linking pymupdf4llm into a network
service obliges its source to be offered. MinerU's and marker's model terms
are free for most users but not unconditional. filegoblin and its bundled
models are permissively licensed.

## olmOCR-bench (dev split, 180 PDFs)

Pass rates per category; overall is the mean of the eight, the official
definition.

| category | filegoblin smart | filegoblin off | MinerU | marker | docling | pymupdf4llm | markitdown |
|---|---:|---:|---:|---:|---:|---:|---:|
| arXiv math | **87.5** | 0.0 | 75.5 | 26.6 | 0.0 | 0.0 | 0.0 |
| headers/footers | 95.6 | 69.2 | **98.9** | 97.8 | 92.3 | 25.3 | 33.0 |
| long tiny text | 80.4 | 41.1 | **85.7** | 80.4 | 71.4 | 42.9 | 39.3 |
| multi-column | 77.4 | 50.9 | **79.2** | 73.6 | 78.3 | 77.4 | 46.2 |
| old scans | 28.8 | 21.2 | 38.8 | **42.5** | 30.0 | 25.0 | 13.8 |
| old scans math | 65.0 | 0.0 | 76.2 | **82.5** | 0.0 | 0.0 | 0.0 |
| tables | **74.8** | 16.2 | 70.3 | 63.1 | 66.7 | 55.9 | 25.2 |
| baseline | 98.9 | 98.3 | 98.9 | **100.0** | 99.4 | 97.8 | 85.0 |
| **overall** | 76.03 | 37.13 | **77.93** | 70.80 | 54.77 | 40.52 | 30.31 |
| median s / PDF | 2.40 | 0.04 | 3.46 | 1.23 | 1.37 | 0.54 | 0.09 |
| mean s / PDF | 2.98 | 0.05 | 4.71 | 4.62 | 2.98 | 1.56 | 0.10 |

One of filegoblin's pages (`long_tiny_text/16b_pg1.pdf`, a scanned dictionary
page) went past the PDF worker's 4 GB memory ceiling and failed; without the
ceiling the same build scored 77.21.

Times are wall-clock seconds per one-page PDF, one file at a time, on a
machine another session was also using, so read them as ±30%. filegoblin's
include starting its process and loading its models for every file (it ran
one process per PDF); the other tools stay loaded across files, so theirs
leave out the one-time load (marker 8 s, docling 8 s, pymupdf4llm 2 s; MinerU
loads lazily inside its first file). MinerU, marker and docling use the GPU
through MPS or Metal; filegoblin, pymupdf4llm and markitdown use only the CPU.

### Published olmOCR-bench numbers (full benchmark, 1,403 PDFs; not measured here)

| system | overall | source |
|---|---:|---|
| olmOCR v0.4.0 (7B VLM, GPU) | 82.4 ± 1.1 | olmOCR README |
| Chandra OCR 0.1.0 | 83.1 ± 0.9 | olmOCR README |
| marker 1.10.1 (force OCR) | 76.1 ± 1.1 | olmOCR README |
| MinerU 2.5.4 | 75.2 ± 1.1 | olmOCR README |
| marker 2.x balanced (GPU) | 76.0 | marker README |
| marker 2.x fast (GPU) | 66.6 | marker README |
| docling (GPU) | 50.3 | marker README |
| filegoblin smart (measured, full bench, v1.16.0) | 74.68 | docs/ADR.md, 2026-10-23 |

The measured dev-split numbers agree with these: marker's fast mode lands
near its published 66.6 (70.8 on dev), docling near 50 without formula
enrichment (54.8), MinerU 4.1 a little above MinerU 2.5's 75.2 (77.9).

## The gauntlet

Quality 0–100 against the hand-checked gold excerpts (`scripts/gauntlet_grade.py`),
one case at a time. "fail" is a conversion that did not finish: an error, or a
memory limit (filegoblin's own: 6 GB for the process, 4 GB for its PDF worker;
the other tools: the harness stops any whose processes pass 8 GB). A failure
counts as 0 in the means, since the reader gets nothing; "n/a" is a format the
tool does not read; "—" is a case without gold. BOOK_1 is graded only on
furniture staying out (the book is copyrighted), so any output that converts
scores high there.

| case | kind | filegoblin smart | filegoblin off | MinerU | marker | docling | pymupdf4llm | markitdown |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| P1 | PPTX Diagrams | 100.0 | 100.0 | fail | n/a | fail | n/a | fail |
| P2 | PPTX Grouped Shapes | 100.0 | 100.0 | fail | n/a | fail | n/a | fail |
| P3 | PPTX Chart Gaps | 98.5 | 98.5 | 79.2 | n/a | fail | n/a | fail |
| D1 | DOCX Tables | 100.0 | 100.0 | 100.0 | n/a | 0.0 | n/a | 100.0 |
| D3 | DOCX Floating Text Boxes | 100.0 | 100.0 | 100.0 | n/a | 100.0 | n/a | 85.5 |
| D4 | DOCX Equations | 92.1 | 92.1 | 92.0 | n/a | 92.0 | n/a | 90.9 |
| R1 | PDF Layout | fail | 61.4 | 85.4 | 97.6 | 85.4 | 81.0 | 0.0 |
| R2 | PDF Math | fail | 70.3 | 93.6 | 91.1 | 72.1 | 74.0 | 57.7 |
| SCI_1 | Scientific Paper (arXiv) | 90.8 | 74.1 | 86.5 | 79.4 | 78.1 | 79.2 | 32.9 |
| SCI_2 | Scientific Paper (arXiv) | 96.6 | 64.8 | 89.6 | 76.5 | 90.5 | 86.2 | 54.1 |
| BOOK_1 | Full Book (300pp) | 75.0 | 75.0 | fail | 100.0 | 100.0 | 50.0 | 50.0 |
| BOOK_2 | Full Book (Scan) | fail | — | — | — | — | — | — |
| TBL_1 | PDF Tables | 100.0 | 98.7 | 100.0 | 74.6 | 86.7 | 93.8 | 56.7 |
| WEB_1 | HTML Web Articles | 99.6 | 99.6 | 99.9 | n/a | 29.9 | n/a | 0.0 |
| FIN_1 | Corporate Financial 10-K | fail | 77.8 | fail | 92.5 | 81.3 | 83.0 | 54.7 |
| FIN_2 | SEC EDGAR iXBRL HTML | 88.2 | 88.2 | 69.2 | n/a | 69.2 | n/a | 69.2 |
| FORM_1 | Government Tax Forms | 70.6 | 70.6 | 61.0 | 36.8 | 62.0 | 46.5 | 50.9 |
| TECH_1 | Hardware Datasheets | 89.9 | 63.8 | fail | 81.9 | 91.7 | 79.4 | 54.2 |
| SLIDE_1 | Corporate Investor Relations Deck | 86.7 | 72.6 | 90.0 | 74.9 | 91.2 | 91.6 | 76.7 |
| SEC_S1 | SEC S-1 Registration | 85.8 | 85.8 | 70.0 | n/a | 62.9 | n/a | 74.9 |
| SEC_CREDIT | Debt Credit Agreements | 85.1 | 85.1 | 74.9 | n/a | 79.9 | n/a | 79.9 |
| SEC_PROXY | SEC DEF 14A Proxy | 94.4 | 94.4 | 77.8 | n/a | 82.8 | n/a | 72.7 |
| LEGAL_VIE | Cross-Border VIE Contracts | 93.5 | 93.5 | 74.5 | n/a | 79.5 | n/a | 79.5 |
| SCOTUS_1 | Supreme Court Slip Opinion | 78.4 | 78.4 | 94.2 | 94.8 | 94.6 | 79.9 | 64.8 |
| PATENT_1 | USPTO Patents | 78.5 | 64.2 | 86.0 | 77.0 | 80.9 | 43.7 | 64.0 |
| CLIN_1 | Clinical Study Protocols | fail | 78.0 | fail | 62.5 | 80.3 | 89.1 | 64.9 |
| FDA_1 | FDA Drug Safety Labels | fail | 75.9 | fail | 76.2 | 70.2 | 81.1 | 71.2 |
| TI_REG | Hardware Register Maps | 94.4 | 88.6 | 94.6 | 76.7 | 93.2 | 75.9 | 57.4 |
| CSI_SPEC | CSI MasterFormat Specs | 76.2 | 76.2 | 95.0 | 94.6 | 96.2 | 63.9 | 61.1 |
| **mean, graded PDF cases** | | **58.6** (16) | **74.4** (16) | **61.0** (16) | **80.4** (16) | **84.7** (16) | **74.9** (16) | **54.5** (16) |
| **mean, graded cases it reads** | | **74.1** (28) | **83.1** (28) | **64.8** (28) | **80.4** (16) | **69.7** (28) | **74.9** (16) | **54.4** (28) |
| failed conversions | | 6 | 0 | 7 | 0 | 3 | 0 | 3 |

PDF wall time for the 17 PDF cases (1,118 pages), one case at a time: filegoblin
smart 205 s (with six cases stopped early), off 7 s, MinerU 421 s (five stopped;
a few minutes of it overlapped a compile), marker 660 s, docling 688 s,
pymupdf4llm 434 s, markitdown 71 s.

Before its memory ceiling existed, the same filegoblin smart build converted
all of these: 90.5 over the 28 graded cases on 2026-10-23 (`GAUNTLET_SCORECARD.md`),
with BOOK_2, FIN_1, R1, R2, CLIN_1 and FDA_1 peaking above 4 GB. With a 4 GB
worker (the default on an 8 GB machine) they stopped.

Two smaller harness facts: the gauntlet's grader now reads HTML tables (MinerU
and marker write them; olmOCR's scorer reads them too) and ignores inline
`<br>`/`<sup>` tags, which moved filegoblin's own HTML cases up by 0.3–1.2
(its `<br/>` inside table cells had counted as words) and nothing else; and
the canaries are filegoblin's own expectations, so only quality compares tools.

## Where each tool beats filegoblin, and why

**Short answer.** On olmOCR-bench, filegoblin smart beats every free tool
here except MinerU 4.1, which leads by about 1.9 points (0.7 without the one
page filegoblin's memory ceiling stopped). On the gauntlet it leads them all: its
PDF cases score 88.2 when they finish, against 84.7 for docling, the best of
the others. With `--vision off` it is close to pymupdf4llm on the gauntlet and
behind it on olmOCR-bench.

Where the others win, test by test (olmOCR-bench dev split: tests a tool passes
and filegoblin smart fails; MinerU 76, marker 63, docling 24, pymupdf4llm 15,
against 102, 284, 425 and 513 the other way):

- **Scanned pages** (old scans: MinerU 38.8, marker 42.5, filegoblin 28.8;
  old scans with math: 76.2, 82.5, 65.0). The old-scans category is mostly
  handwritten 19th- and early-20th-century letters. MinerU and marker read
  whole pages with a vision-language model (MinerU2.5-Pro 1.2B; surya), which
  reads handwriting; filegoblin's PP-OCRv6 line recognizer reads printed text
  well and handwriting badly (a Red Cross letter comes out as "Chif - Justier
  waili, of lwashinglin"). On scanned math the VLMs write whole formulas as
  LaTeX, where filegoblin's layout model misses some formula regions on a
  noisy scan and its formula model misreads others (`x^{m}` as `x^{n}`). A
  model problem, not one a rule can fix; see "Open" in the ADR.
- **Running headers and footers set once, or numbered by section.** MinerU
  (98.9), marker (97.8) and docling (92.3) label page headers and footers with
  a layout model on every page; filegoblin finds furniture by recurrence and
  by position, so a head that appears on one page only ("(Slip Opinion)
  OCTOBER TERM, 2025" on a slip opinion's first page), a head moved lower in
  some sections, and footers such as "01 1100 - 3" leaked. That is why
  docling, MinerU and marker score 94–96 on SCOTUS_1 and CSI_SPEC against
  78 and 76, and 100 on BOOK_1 against 75. The next ADR entry fixes the
  first three kinds (SCOTUS_1 88.4, CSI_SPEC 91.2).
- **Multi-column pages** (MinerU 79.2, docling 78.3, filegoblin 77.4): mostly
  single pages either way. One cause is outside filegoblin: pdf_oxide gives
  every glyph of a standard-14 font (Helvetica, Times) that has no `/Widths`
  and a `/Differences` encoding remapping letter codes a flat 0.55 em advance,
  because it looks the standard widths up by byte code, not glyph name. The
  glyph positions then drift into the next column; on three IRS pages that
  costs filegoblin 7 dev tests that MinerU, reading the page through PDFium,
  passes.
- **Long tiny text** (MinerU 85.7, filegoblin 80.4): filegoblin lost one
  dictionary page to its memory ceiling; without it, 89.3.
- **Gauntlet cases** beyond furniture: marker on FIN_1 (92.5 vs 90.2) and R1
  (97.6 vs 97.3); docling on TECH_1 (91.7 vs 89.9) and SLIDE_1 (91.2 vs 86.7,
  where filegoblin promotes few headings: 25); MinerU on PATENT_1 (86.0 vs
  78.5: filegoblin keeps figure labels and a sheet header); pymupdf4llm on
  SLIDE_1 (91.6).
- **`--vision off` against pymupdf4llm**, the fair CPU-only pairing:
  pymupdf4llm leads on olmOCR-bench (40.5 vs 37.1), on tables (55.9 vs 16.2)
  and multi-column order (77.4 vs 50.9). PyMuPDF Layout runs a small graph
  model to find tables and columns; filegoblin's fast path uses no model by
  design, and its tables came only from fully drawn grids and pdf_oxide's own
  detection, so booktabs-style tables (rules above and below, none down)
  scored 0 of 38 dev tests. The next ADR entry adds them (tables 16.2 → 36.0).
  On the gauntlet the two are level (PDF means 74.4 and 74.9).

Where filegoblin leads:

- **Math in born-digital papers**: arXiv math 87.5, against MinerU 75.5 and
  at most 27 for the rest (marker's fast mode and docling's defaults do not
  OCR formulas).
- **Tables**: 74.8 on olmOCR-bench (MinerU 70.3, docling 66.7) and the best
  gauntlet table scores (TBL_1 100, TI_REG 94.4).
- **Everything that is not a PDF**: the adversarial PPTX cases crash MinerU,
  docling and markitdown (`InvalidXmlError`); HTML articles keep their
  navigation and cookie banners in docling and markitdown (WEB_1 29.9 and 0,
  every leaked string costing 5 points); filegoblin reads all 12 non-PDF
  cases at 85–100.
- **Long documents within a memory budget**: MinerU's standard tier passed 8 GB
  on every gauntlet document over 50 pages (BOOK_1, FIN_1, TECH_1, CLIN_1,
  FDA_1) and was stopped. filegoblin's own smart mode needed more than its
  4 GB worker on six gauntlet PDFs; the next ADR entry fixes that.
- **Licence and hardware**: MIT/Apache-2.0 for the binary and its models; the
  CPU only, on any machine. MinerU (commercial-licence thresholds, attribution
  for services), marker's weights (OpenRAIL-M with a revenue cap) and
  pymupdf4llm (AGPL) all carry conditions, and MinerU and marker rely on a
  GPU or Metal for their speed.

## After this pass

The next ADR entry (2026-10-30) fixed what it could of the above: smart mode
inside a 4 GB worker, booktabs tables on the fast path, running heads that do
not recur. Same machine, same limits, same harness:

| | before | after | best other tool |
|---|---:|---:|---|
| olmOCR-bench dev, smart | 76.03 | **77.21** | MinerU 77.93 |
| olmOCR-bench dev, off | 37.13 | **39.61** | pymupdf4llm 40.52 (CPU) |
| olmOCR-bench test (1,223 PDFs), smart | 73.70 | **74.29** | not measured |
| olmOCR-bench test, off | 39.28 | **40.08** | not measured |
| gauntlet, graded PDF cases, smart | 58.6 (6 failed) | **89.0** (0 failed) | docling 84.7 |
| gauntlet, graded PDF cases, off | 74.4 | **76.4** | pymupdf4llm 74.9 |
| gauntlet, all graded cases, smart | 74.1 | **91.4** | |
| gauntlet, all graded cases, off | 83.1 | **84.3** | |

MinerU still leads olmOCR-bench's dev split by 0.7. It is 10 and 11 points
ahead on the two scanned categories, and 2–3 tests ahead on headers/footers
and multi-column; filegoblin leads arXiv math, long tiny text, tables and
baseline.

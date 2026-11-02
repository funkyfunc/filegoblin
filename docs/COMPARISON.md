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
- **New material, 2026-11-01** (see the last section): a fresh held-out set
  in olmOCR-bench's format (133 one-page PDFs, 1,009 hand-checked tests), and
  11 gauntlet cases in languages and document kinds not measured before.

`scripts/engines.py` describes each engine (install line, formats, licence);
`scripts/alt_engine_worker.py` runs inside the tool's environment, loads it
once and converts files one at a time. To repeat everything:

```bash
python3 scripts/olmocr_bench.py run cmp-docling --split dev --jobs 1 --engine docling
python3 scripts/run_gauntlet.py --engine docling
```

**Read this first.** An audit on 2026-11-01 (`docs/reviews/2026-11-01-claims-audit.md`) re-ran the other
tools on olmOCR-bench's 1,223-PDF test split and read two fresh sets written after the tuning.
- **Every number below reproduces.** But the dev split and the gauntlet are where filegoblin was tuned, and
  the other tools never saw them.
- **On documents nobody tuned on, the order changes.**
  - The test split put MinerU 4.7 points ahead of filegoblin smart. With the page reader (9db61d6) the gap
    is 0.7, inside the noise.
  - The fresh held-out pages put marker's fast mode narrowly ahead.
  - docling leads the fresh gauntlet cases.
  - The fast path trails pymupdf4llm everywhere but dev.

  See "olmOCR-bench (test split)" and "Off the tuning sets".
- **Every tool ran in its default configuration**: marker in fast mode, docling without formula enrichment.
  Read every comparison here as default against default.

## The tools

| tool | version | configuration (its default here) | device | licence |
|---|---|---|---|---|
| **filegoblin** `--vision smart` | 1.16.0 + `4283928` | layout (PP-DocLayoutV3), tables (Table Transformer), formulas (pix2text MFR), OCR (PP-OCRv6) on pages that need them | CPU (rten) | MIT OR Apache-2.0 |
| **filegoblin** `--vision off` | same | text layer only, no models | CPU | same |
| MinerU | 4.1.2 | `mineru-kit parse`, tier **standard** (the default): layout and OCR models plus the MinerU2.5-Pro 1.2B VLM through llama.cpp | MPS + Metal | Apache-2.0 with extra terms: a commercial licence above 100 M monthly users or US$20 M monthly revenue; online services must credit MinerU |
| marker | 2.1.0 (surya 0.23.0) | `PdfConverter` defaults: mode **fast** (marker picks it whenever the device is not CUDA), the surya VLM through llama.cpp's `llama-server` (b11985) for OCR and equations. Its **balanced** mode (VLM layout, inline-math OCR) is the one its README scores at 76.0 against fast's 66.6. Measured here on the dev split, balanced scores **77.00** (fast 70.80, filegoblin smart 77.21) at 10.6 s per page | MPS + Metal | code Apache-2.0; model weights a modified OpenRAIL-M, free for research, personal use and companies under US$5 M funding/revenue |
| docling | 2.145.0 | `DocumentConverter()` defaults: Heron layout model, TableFormer, OCR chosen automatically (RapidOCR); formula enrichment off (`do_formula_enrichment`, not its default), so its two math categories score 0 by configuration | MPS / CPU | MIT |
| pymupdf4llm | 2.0 (PyMuPDF 2.0, pymupdf-layout 2.0) | `to_markdown()` with PyMuPDF Layout (its default when installed) and RapidOCR for pages that need OCR (Tesseract is a system package, so not installed) | CPU (ONNX) | **AGPL-3.0** or Artifex commercial licence |
| markitdown | 0.1.9 | `MarkItDown().convert()` (pdfminer.six for PDFs) | CPU | MIT |
| olmOCR | — | not run: its pipeline needs an NVIDIA GPU with ≥ 12 GB (README); the numbers below are **published, not measured** | — | Apache-2.0 |

AGPL matters to anyone shipping a product: linking pymupdf4llm into a network
service obliges its source to be offered. MinerU's and marker's model terms
are free for most users but not unconditional. filegoblin and its bundled
models are permissively licensed.

## olmOCR-bench (dev split, 180 PDFs)

Pass rates per category; overall is the mean of the eight, the official
definition. 180 PDFs is a small sample: the 95 % interval on each overall is
about ±5 points, so this table does not rank tools that are within 5 points of
each other. The test split below does.

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

The filegoblin row mixes the 180 dev PDFs it was tuned on with the 1,223
test PDFs, and the other rows are other versions on other hardware, so this
table is context, not a ranking. On the test split measured here (next
section), marker's fast mode scores 65.6 (published 66.6) and docling 54.6;
MinerU 4.1 scores 79.0, above MinerU 2.5's published 75.2.

## olmOCR-bench (test split, 1,223 PDFs)

Measured 2026-11-01 (`docs/reviews/2026-11-01-claims-audit.md`), one tool at a
time, same harness and scorer. The other tools ran in the default
configurations of the table above. The filegoblin smart column is main at
`9db61d6`, with the page reader (LightOnOCR-2-1B) for handwritten and math
scans; `75355f5`, before it, scored 74.31. The off column is `75355f5`. filegoblin was never tuned on this split, but it is not fully held
out for filegoblin either: fixes found by reading failures here are worth
about 0.6 (smart) and 0.5 (off) of its scores. The other tools carry none of
that.

| category | filegoblin smart | filegoblin off | MinerU | marker (fast) | docling | pymupdf4llm |
|---|---:|---:|---:|---:|---:|---:|
| arXiv math | **87.1** | 0.0 | 77.7 | 23.1 | 0.0 | 0.0 |
| headers/footers | 92.7 | 74.0 | **95.7** | 93.9 | 90.1 | 32.4 |
| long tiny text | **87.8** | 54.1 | 83.9 | 63.2 | 77.7 | 59.6 |
| multi-column | 72.1 | 49.5 | **79.7** | 74.4 | 73.5 | 66.1 |
| old scans | 39.9 | 17.3 | 38.3 | **43.9** | 24.0 | 19.7 |
| old scans math | 67.2 | 0.0 | **76.7** | 60.8 | 0.0 | 0.0 |
| tables | 80.6 | 35.2 | **82.0** | 65.8 | 73.4 | 64.4 |
| baseline | 99.3 | 98.9 | 98.0 | **99.3** | 97.9 | 95.9 |
| **overall** | 78.33 | 41.13 | **79.01** | 65.55 | 54.58 | 42.27 |
| 95 % interval | 76.4–80.2 | 39.0–43.0 | 77.3–80.6 | 63.3–68.1 | 52.8–56.3 | 40.6–44.2 |

- **MinerU and filegoblin smart cannot be told apart.** MinerU is 0.7
  ahead (interval −1.2 to +2.5), about 1.3 once filegoblin's test-informed
  fixes are discounted. Before the page reader the gap was 4.7 (2.6–6.5); the
  reader took old scans from 28.9 to 39.9 and old scans math from 47.6 to
  67.2. MinerU still leads scanned math (+10), multi-column (+8),
  headers/footers and tables; filegoblin leads arXiv math, long tiny text
  and old scans.
- **The page reader is slow.** The run took 2 hours on 3 jobs (M4 Pro, CPU
  only), against about an hour on 6–7 jobs before it; a page it reads takes
  half a minute to two minutes.
- **filegoblin leads marker's fast mode by 12.8** (9.9–15.7), docling by 23.8
  and pymupdf4llm by 36.
- **The fast path trails pymupdf4llm by 1.1.** The interval is −1.5 to +3.9,
  so not significant.
- **The test split moves the tools differently from dev:** MinerU +1.1,
  pymupdf4llm +1.8, docling −0.2, marker −5.3, filegoblin smart −2.9 (the same
  drop as a filegoblin build from before the split existed).

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
| D4 | DOCX Equations | 92.8 | 92.8 | 92.6 | n/a | 92.6 | n/a | 91.6 |
| R1 | PDF Layout | fail | 61.8 | 87.3 | 97.4 | 85.2 | 80.6 | 0.0 |
| R2 | PDF Math | fail | 70.9 | 93.6 | 90.8 | 72.1 | 75.6 | 58.1 |
| SCI_1 | Scientific Paper (arXiv) | 92.0 | 74.4 | 88.9 | 79.7 | 78.4 | 79.8 | 37.6 |
| SCI_2 | Scientific Paper (arXiv) | 96.8 | 65.8 | 90.9 | 76.8 | 90.7 | 86.8 | 55.2 |
| BOOK_1 | Full Book (300pp) | 75.0 | 75.0 | fail | 100.0 | 100.0 | 50.0 | 50.0 |
| BOOK_2 | Full Book (Scan) | fail | — | — | — | — | — | — |
| TBL_1 | PDF Tables | 100.0 | 98.7 | 100.0 | 74.6 | 86.7 | 93.8 | 56.7 |
| WEB_1 | HTML Web Articles | 99.6 | 99.6 | 99.9 | n/a | 29.9 | n/a | 0.0 |
| FIN_1 | Corporate Financial 10-K | fail | 77.8 | fail | 92.5 | 85.8 | 87.5 | 54.7 |
| FIN_2 | SEC EDGAR iXBRL HTML | 88.2 | 88.2 | 69.2 | n/a | 69.2 | n/a | 69.2 |
| FORM_1 | Government Tax Forms | 71.3 | 71.3 | 61.7 | 37.0 | 62.2 | 46.5 | 50.8 |
| TECH_1 | Hardware Datasheets | 90.1 | 65.7 | fail | 84.4 | 91.7 | 79.4 | 54.3 |
| SLIDE_1 | Corporate Investor Relations Deck | 86.8 | 75.9 | 90.0 | 75.6 | 91.3 | 91.6 | 76.6 |
| SEC_S1 | SEC S-1 Registration | 85.8 | 85.8 | 70.0 | n/a | 62.9 | n/a | 74.9 |
| SEC_CREDIT | Debt Credit Agreements | 85.1 | 85.1 | 74.9 | n/a | 79.9 | n/a | 79.9 |
| SEC_PROXY | SEC DEF 14A Proxy | 94.4 | 94.4 | 77.8 | n/a | 82.8 | n/a | 72.7 |
| LEGAL_VIE | Cross-Border VIE Contracts | 93.5 | 93.5 | 74.5 | n/a | 79.5 | n/a | 79.5 |
| SCOTUS_1 | Supreme Court Slip Opinion | 77.2 | 77.2 | 94.9 | 95.6 | 93.9 | 79.5 | 64.9 |
| PATENT_1 | USPTO Patents | 79.5 | 65.5 | 87.3 | 77.4 | 82.1 | 54.2 | 64.0 |
| CLIN_1 | Clinical Study Protocols | fail | 80.3 | fail | 68.4 | 80.3 | 89.5 | 64.9 |
| FDA_1 | FDA Drug Safety Labels | fail | 77.6 | fail | 78.3 | 70.6 | 81.0 | 71.2 |
| TI_REG | Hardware Register Maps | 94.4 | 88.6 | 94.6 | 78.9 | 93.2 | 75.9 | 57.4 |
| CSI_SPEC | CSI MasterFormat Specs | 76.2 | 76.2 | 95.0 | 94.6 | 96.2 | 63.9 | 61.1 |
| **mean, graded PDF cases** | | **58.7** (16) | **75.2** (16) | **61.5** (16) | **81.4** (16) | **85.0** (16) | **76.0** (16) | **54.8** (16) |
| **mean, graded cases it reads** | | **74.2** (28) | **83.6** (28) | **65.1** (28) | **81.4** (16) | **69.9** (28) | **76.0** (16) | **54.7** (28) |
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

What the gauntlet can and cannot show against other tools:
- **It is filegoblin's own acceptance test.** Every change since 2026-10-14 had to keep every case, and the
  2026-10-30 pass fixed SCOTUS_1 and CSI_SPEC after this comparison showed the others winning there. The
  other tools ran once, untuned.
- **The "cases it reads" mean mixes formats.** 12 of the 28 graded cases are not PDFs, which marker and
  pymupdf4llm do not read.
- **P1–P3 are synthetic.** They are small PPTX files built to reproduce known markitdown bugs, and they lack
  a `<p:spPr>` element the schema requires. Their crashes count as 0 for three tools.
- **MinerU's five failures are the harness's 8 GB cap**, not a crash. It was not tested whether MinerU
  finishes them with more memory.
- **On the 11 PDF cases every tool finished**, filegoblin smart scores 89.8, MinerU 88.7, docling 84.6,
  marker 79.5 and pymupdf4llm 74.1.
- **On 11 fresh gauntlet cases written after the tuning**, docling leads; see "Off the tuning sets" below.

## Where each tool beats filegoblin, and why

**Short answer** (corrected 2026-11-01).
- **olmOCR-bench test split:** MinerU 4.1 and filegoblin smart cannot be
  told apart (79.01 against 78.33; MinerU led by 4.7 before the page reader).
  filegoblin is clearly ahead of marker's fast mode, docling and
  pymupdf4llm.
- **Dev split:** filegoblin and MinerU cannot be told apart (0.7 apart, ±5).
- **133 fresh pages:** marker's fast mode is ahead of filegoblin and MinerU
  is behind it, neither significantly.
- **The gauntlet:** it leads on the PDF cases each tool finished (90.1 vs
  MinerU 89.5), on its own tuning suite. On 11 fresh cases docling led
  (84.9), with marker (81.2), MinerU (81.0) and filegoblin (80.7) level
  behind it. The right-to-left fix of 2026-11-01 takes filegoblin to 87.9,
  but it was made for those two cases, so they are no longer fresh: without
  them filegoblin is first (89.1 against docling's 86.1 and MinerU's 85.8).
- **`--vision off`:** trails pymupdf4llm everywhere except the dev split.

The rest of this section was written from the dev split and the gauntlet on
2026-10-30.

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
  On the gauntlet the two are close (PDF means 75.2 and 76.0).
  After the 2026-10-30/31 passes the fast path matched pymupdf4llm on dev
  (40.54 vs 40.52), but about 1.5 points of that are specific to dev. On the
  test split pymupdf4llm leads 42.3 to 41.1, and on the held-out pages and the
  fresh gauntlet cases by about 10.

Where filegoblin leads:

- **Math in born-digital papers**: arXiv math 87.5, against MinerU 75.5 and
  at most 27 for the rest (marker's fast mode and docling's defaults do not
  OCR formulas).
- **Tables**: 74.8 on olmOCR-bench (MinerU 70.3, docling 66.7) and the best
  gauntlet table scores (TBL_1 100, TI_REG 94.4).
- **Everything that is not a PDF**: the adversarial PPTX cases crash MinerU,
  docling and markitdown (`InvalidXmlError`; these are synthetic files missing a
  required `<p:spPr>`, built to reproduce markitdown bugs); HTML articles keep their
  navigation and cookie banners in docling and markitdown (WEB_1 29.9 and 0,
  every leaked string costing 5 points); filegoblin reads all 12 non-PDF
  cases at 85–100.
- **Long documents within a memory budget**: MinerU's standard tier passed 8 GB
  on every gauntlet document over 50 pages (BOOK_1, FIN_1, TECH_1, CLIN_1,
  FDA_1) and was stopped by the harness's 8 GB cap. It was not tried with
  more, so this shows its memory use, not that it cannot finish them. filegoblin's own smart mode needed more than its
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
| olmOCR-bench test (1,223 PDFs), smart | 73.70 | **74.29** (78.33 with the page reader) | MinerU 79.01 (measured 2026-11-01) |
| olmOCR-bench test, off | 39.28 | **40.08** | pymupdf4llm 42.27 (CPU; measured 2026-11-01) |
| gauntlet, graded PDF cases, smart | 58.7 (6 failed) | **89.2** (0 failed) | docling 85.0 |
| gauntlet, graded PDF cases, off | 75.2 | **77.1** | pymupdf4llm 76.0 |
| gauntlet, all graded cases, smart | 74.2 | **91.6** | |
| gauntlet, all graded cases, off | 83.6 | **84.7** | |

The 2026-10-31 ADR entry read the other tools' source for the ideas behind
these leads (`docs/reviews/2026-10-31-competitor-source.md`). Two carried over
to the fast path: position-only furniture after marker, and unruled numeric
tables after MinerU. With them, `--vision off` scores 40.54 on dev (pymupdf4llm: 40.52) and
41.13 on test, and the gauntlet scores 84.8 (off) and 91.6 (smart). Smart mode's
olmOCR-bench score did not move (dev 77.21, test 74.31).

MinerU still leads olmOCR-bench's dev split by 0.7. It is 10 and 11 points
ahead on the two scanned categories, and 2–3 tests ahead on headers/footers
and multi-column; filegoblin leads arXiv math, long tiny text, tables and
baseline. On the test split MinerU's lead was 4.7; with the page reader it is 0.7 (above).

## Off the tuning sets

The dev split and the gauntlet are where filegoblin was tuned. Two sets were
written on 2026-10-31 by a separate session, which froze them before any
converter ran (`~/Library/Caches/fgv/heldout/`, outside the repo; only scores
are read from it).

**Held-out pages**: 133 one-page PDFs, 1,009 tests, olmOCR's seven categories
plus 18 multilingual pages. All nine categories are averaged; the intervals
are a 95 % bootstrap over PDFs.

| | overall | 95 % interval |
|---|---:|---|
| marker 2.1 (fast) | **87.1** | 83.3–90.9 |
| filegoblin smart | 85.9 (a second run: 84.9) | 82.7–90.2 |
| MinerU 4.1.2 | 82.1 | 77.8–86.2 |
| docling 2.145 | 69.7 | 66.4–72.8 |
| pymupdf4llm 2.0 | 57.8 | 53.9–61.4 |
| filegoblin off | 47.8 | 43.9–52.0 |
| markitdown 0.1.9 | 40.7 | 36.7–44.7 |

**New gauntlet cases**: 11 PDFs (CJK, Arabic, Hebrew, French, a receipt, an
invoice, a filled form, a 1922 newspaper scan, a newsletter, a slide deck).
Quality is graded as above.

| | mean (11) | without Arabic/Hebrew (9) |
|---|---:|---:|
| docling | 84.9 | 86.1 |
| marker | 81.2 | 83.8 |
| MinerU | 81.0 | 85.8 |
| filegoblin smart | 80.7 (87.9†) | **89.1** |
| pymupdf4llm | 79.1 | 79.7 |
| filegoblin off | 70.0 (75.7†) | 76.7 |
| markitdown | 49.1 | 54.4 |

† With right-to-left text in reading order (2026-11-01, see the ADR). That
fix was made for RTL_AR and RTL_HE, so the column without them is the fresh
comparison.

Both sets put filegoblin smart in the top group, not on its own at the top,
and both put the fast path well behind pymupdf4llm.

## Off the tuning sets, in detail

Measured 2026-11-01 on the same machine and harnesses, with filegoblin built at
`75355f5` (`--features vision`) and the same versions of the other tools.

The olmOCR-bench test split is no longer fully held out: the 2026-10-22
research read failures across the whole bench, and the unruled-table fix of
2026-10-31 was found on test pages. The dev split is tuned on by design. So
both of the numbers above have been fitted to some degree.

### The held-out set (133 one-page PDFs, 1,009 tests)

It is built in olmOCR-bench's exact format, so `scripts/olmocr_bench.py` and
allenai's scorer grade it unchanged. It lives outside the repository at
`~/Library/Caches/fgv/heldout/`, as the bench does, with a README, the rules the
tests were written to (`SPEC.md`), and a manifest.

**How the tests were made.** Every test was written by hand-checking its page:
- the rendered image for content and reading order;
- the page's own text layer, or the arXiv LaTeX source for equations, for exact
  characters;
- for scans, transcription by eye.

No converter, OCR engine or VLM ran on these pages until the tests were frozen.
`FROZEN` holds the sha256 of each test file at that moment.

**Sources:**
- **arXiv:** CC BY, CC BY-SA and CC0 papers submitted in October 2026.
- **US federal reports and statistics (2026):** GAO, CRS, NIST, CDC MMWR, BLS,
  Census, the Fed, EIA, the Federal Register and the Congressional Record.
- **Open-licence journals:** CC BY journals from 2026 (PLOS, BMC, eLife,
  Frontiers, Copernicus).
- **Europe:** EU acts and agency reports, plus national statistics under open
  licences.
- **Scans:** public-domain pages from the Library of Congress, the Internet
  Archive, HathiTrust and NACA, with printed, typewritten and handwritten pages.

**Licences:** 65 public domain, 52 CC BY or the EU's equivalent, 5 CC BY-SA,
and 1 each CC0, Etalab and OGL. None is NC or ND. No document is in
olmOCR-bench or the gauntlet.

**Categories:** olmOCR-bench's seven, plus `multilingual`: 18 pages in 18
Latin-, Greek- and Cyrillic-script languages. CJK is left out because allenai's
baseline test fails any output containing CJK characters.

**Use it for scores only.** Reading its failures and fixing filegoblin for them
would turn it into one more dev set.

```bash
OLMOCR_BENCH=~/Library/Caches/fgv/heldout/bench_data scripts/olmocr_bench.py run NAME --engine TOOL
OLMOCR_BENCH=~/Library/Caches/fgv/heldout/bench_data scripts/olmocr_bench.py table NAME …
```

| category (PDFs / tests) | filegoblin smart | filegoblin off | MinerU | marker | docling | pymupdf4llm | markitdown |
|---|---:|---:|---:|---:|---:|---:|---:|
| arXiv math (20 / 124) | **82.3** | 0.0 | 53.2 | 71.8 | 0.0 | 0.0 | 0.0 |
| headers/footers (20 / 117) | 90.6 | 78.6 | 94.0 | **96.6** | 93.2 | 59.8 | 56.4 |
| long tiny text (10 / 80) | **100.0** | 33.8 | 98.8 | 98.8 | 92.5 | 91.2 | 63.8 |
| multi-column (19 / 125) | 96.0 | 63.2 | 96.0 | **99.2** | 94.4 | 83.2 | 73.6 |
| old scans (15 / 139) | 85.6 | 62.6 | 91.4 | **96.4** | 71.2 | 53.2 | 7.9 |
| old scans math (6 / 27) | 44.4 | 0.0 | 51.9 | **70.4** | 0.0 | 0.0 | 0.0 |
| tables (25 / 224) | 84.8 | 26.8 | 70.1 | 75.4 | **85.3** | 66.5 | 19.6 |
| multilingual (18 / 173) | 89.6 | 65.3 | 84.4 | 75.7 | **90.8** | 68.8 | 60.7 |
| baseline | **100.0** | **100.0** | 99.2 | **100.0** | **100.0** | 97.7 | 84.2 |
| **overall** | 85.93 | 47.81 | 82.10 | **87.14** | 69.70 | 57.84 | 40.69 |

**Runs.** The other tools ran one file at a time. filegoblin ran with
`--jobs 3 --no-cache`, sharing the machine's three vision slots with two other
benchmarking sessions, so its median of 9.5 s per page is mostly waiting. Read
no speed into it.

**Two failed conversions in the first smart run.** Two of its 133 pages failed
with "no progress on this PDF for 120 s". Alone, the same pages convert in 41 s
and 9 s, so the table counts them re-run. As run, the score was 84.89.

The cause is a bug in `75355f5`. The PDF worker's stall watchdog counts finished
pages as progress, so a one-page PDF that waits more than 120 s for a vision
slot is stopped as if it were stuck. Waiting for a slot should not count as a
stall.

What this set says that the bench did not:

- **marker, not MinerU, leads**, by 1.2 points over filegoblin smart. marker
  scores 96–99 on scans, furniture and multi-column order. filegoblin leads
  only arXiv math and long tiny text, and is level on multi-column with MinerU.
- **MinerU drops to 53.2 on arXiv math** (75.5 on bench dev). filegoblin's
  82.3 holds up.
- **filegoblin's weakest categories are scanned math (44.4) and old scans
  (85.6).** Both trail marker and MinerU, the same model gap the bench showed.
  Next comes headers/footers (90.6), behind all three model-based tools: a
  furniture gap the bench's 95.6 understated.
- **The fast path's parity with pymupdf4llm does not carry over: 47.8 against
  57.8.** On the bench's dev split the two were level (40.54 and 40.52). Here
  `--vision off` trails on:
  - long tiny text, 33.8 against 91.2;
  - multi-column order, 63.2 against 83.2;
  - tables, 26.8 against 66.5.

  This is the clearest sign that the fast path's recent gains were fitted to
  the bench.

### New gauntlet cases (11)

These cover document kinds and languages not measured before. Gold excerpts are
hand-checked from the page images, and each case lists its source and licence
in `gauntlet_manifest.json`:
- **CJK_ZH:** Taiwan DGBAS press release (Open Government Data License).
- **CJK_JA:** Statistics Bureau of Japan (terms compatible with CC BY 4.0).
- **RTL_AR and RTL_HE:** Wikipedia renders (CC BY-SA 4.0).
- **EU_ACCENT:** an EUR-Lex implementing regulation in French (CC BY 4.0).
- **RECEIPT_1:** a dot-matrix till receipt from Wikimedia Commons (CC0).
- **INVOICE_1:** an invoice generated for the gauntlet, since no openly licensed
  real one was found.
- **FORM_FILLED:** IRS W-4 (2026) with fictional values in its live AcroForm fields.
- **NEWS_1:** the Evening Star of 15 March 1922, an image-only scan from
  Chronicling America.
- **NEWS_2:** NASA Stennis's *Lagniappe* newsletter.
- **SLIDES_2:** a NASA NEPP slide deck exported to PDF.

NEWS_1's gold was widened on 2026-11-01 from 2 of the page's articles to 16
(the method is in `test_assets/gauntlet/gold/README.md`). The two first
excerpts had favoured docling, which reads those two articles well and much
of the rest of the page badly. The table uses the 16-article gold; with the
first two alone the row read filegoblin smart 71.5, off 22.5, MinerU 57.0,
marker 34.8, docling 67.6, pymupdf4llm 34.4.

### The gold audit (2026-11-01)

Every PDF gold page except NEWS_1's (55 pages in 25 cases) was transcribed independently by
two models, Gemini 4 Argon (thinking High) and Claude Opus 5.5 (effort High),
with the gold conventions in the prompt. Each place where they disagreed
with the gold was then settled on the page: on born-digital pages the words
against the text layer, on scans against a crop of the image, and the
structure (tables, headings, order) against a 200–300 dpi render.

- **Words held up.** Across about 1,000 places checked, no gold word was
  misread on any page, scans included. Where a model differed, the page sided
  with the gold: a space a receipt doesn't print, a footnote moved, a
  contact block reordered.
- **Structure and markup took 15 changes, one per file:** three headings
  that weren't (two letterheads, a court caption); three real ones that
  weren't marked (two labels, a table title); two multi-line titles split
  into several headings (EU_ACCENT, CJK_JA); two form footer lines kept; a
  readable receipt line left out; a table title split across a page break;
  one Arabic mark order; seven footnote markers detached from their words;
  and one marker in superscript parentheses. `test_assets/gauntlet/gold/README.md` now answers
  each judgement call behind them.
- **The grader had four bugs, now fixed.** It split Hebrew and Arabic words
  at every vowel mark. It glued a citation marker to its word ("woodward1").
  It scored LaTeX font commands (`\mathbf{A}` against `A`). And it stretched
  an excerpt's span to the last repeated phrase anywhere in the window:
  dropping one gold line from FORM_1 moved MinerU's precision from 0.92 to
  0.63 with its output unchanged. The span is now the run of matches that
  best fits the excerpt.

The tables above use the corrected gold and grader. The same saved outputs
moved by under a point on the old cases' means; on the 11 new cases the
grader fixes moved marker by +2.6 and MinerU by +0.9, and the gold fixes by
under a point. With the committed gold and grader of `5236b7a`, the means
were: old PDF cases filegoblin smart 89.0, off 76.6, MinerU 61.0, marker
80.4, docling 84.7, pymupdf4llm 74.9; new cases smart 80.8, off 68.6,
MinerU 79.4, marker 79.0, docling 84.7, pymupdf4llm 78.1. No tool changed
places on the old cases; on the new ones marker and MinerU moved level with
filegoblin.

The grader now counts each Chinese or Japanese ideograph or kana as a word. A
clause written without spaces was otherwise one token, so a single line break
cost the whole clause. No earlier case contains CJK, so no earlier score moved.

| case | filegoblin smart | filegoblin off | MinerU | marker | docling | pymupdf4llm | markitdown |
|---|---:|---:|---:|---:|---:|---:|---:|
| CJK_ZH | 89.4 | 70.3 | 87.4 | 88.0 | **91.5** | 87.8 | 70.7 |
| CJK_JA | 90.8 | 82.1 | 89.9 | 85.8 | 87.4 | **91.6** | 78.5 |
| RTL_AR | 35.5 → **81.5**† | 52.8 → 68.3† | 55.6 | 73.4 | 68.8 | 74.3 | 31.9 |
| RTL_HE | 50.5 → 84.2† | 26.4 → 73.8† | 62.7 | 65.2 | **89.7** | 79.2 | 18.5 |
| EU_ACCENT | 80.8 | 84.2 | **93.4** | 71.9 | 82.6 | 83.3 | 68.3 |
| RECEIPT_1 | **94.7** | 82.4 | 94.1 | 92.5 | 85.6 | 93.7 | 0.0 |
| INVOICE_1 | 92.0 | 92.5 | 94.5 | **94.8** | 93.4 | 84.2 | 68.6 |
| FORM_FILLED | 81.1 | 81.2 | 78.4 | 85.1 | **91.6** | 78.7 | 71.0 |
| NEWS_1 | **89.5** | 28.9 | 87.8 | 59.6 | 49.3 | 34.0 | 0.0 |
| NEWS_2 | 95.7 | 81.5 | 73.4 | 94.2 | **96.5** | 75.4 | 71.2 |
| SLIDES_2 | 87.6 | 87.3 | 73.3 | 82.3 | **97.0** | 88.1 | 61.4 |
| **mean** | 80.7 → **87.9**† | 70.0 → 75.7† | 81.0 | 81.2 | 84.9 | 79.1 | 49.1 |

† After the right-to-left fix (2026-11-01), which was made for these two
cases; no other case moved.

No tool failed a conversion. On macOS, filegoblin's off mode OCRs image-only
pages with Apple Vision, which is why it reads the receipt.

Where filegoblin is weakest here:

- **Right-to-left text was reversed** (fixed 2026-11-01). Arabic came out in
  visual order, character by character backwards (`نادوسلا تايلاو` for
  `ولايات السودان`), in both modes. Lines are now put in logical order
  (UAX #9), and RTL tables list their right-hand column first. What is left:
  the layout model does not mark the Hebrew and Arabic section titles as
  headings (smart mode's headings score 0 on both cases), some Arabic words
  still lose a letter or run together, and the fast path keeps pdf_oxide's
  own text for lines it cannot match to the page's glyphs.
- **Long multi-line table rows split into many rows.** EU_ACCENT's annex has one
  logical row whose cells run to several paragraphs. filegoblin smart makes each
  visual line a table row and folds the "(1) (2) (3)" column-number row into the
  first cell (tables 33). MinerU keeps the row whole (tables 100, 90.9 overall).
- **A filled form's first page collapses into one paragraph.** On the W-4, the
  fields' values are all present, including the name, address, employer and
  amounts. But page 1's labels and values run together in reading order, and
  "166.15" comes out as "166 15" across its dollar and cent boxes. docling keeps
  the structure (91.9 against 84.1).
- **Slides lose headings and keep furniture.** SLIDES_2 keeps "To be presented
  by" and the "Unclassified / Open Access" banner, and promotes fewer slide
  titles to headings. docling scores 98.9.
- **filegoblin leads on the 1922 newspaper scan:** 87.1, against MinerU's 85.9
  and 48 or less for the rest. It is level with the leaders on CJK, receipts and
  invoices.

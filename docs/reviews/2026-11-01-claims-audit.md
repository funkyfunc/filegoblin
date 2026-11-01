# Are filegoblin's quality claims honest? An audit (2026-11-01)

The claims are in `docs/COMPARISON.md`, `README.md` (capabilities), `GAUNTLET_SCORECARD.md` and the
2026-10-23/30/31 entries of `docs/ADR.md`. This audit checks whether they reproduce, whether they are an
artifact of weeks of tuning on the same documents, and whether each tool ran in a fair configuration. It
starts from the assumption that the authors fooled themselves somewhere.

**Method.**
- Every number was recomputed from the saved outputs: `~/Library/Caches/fgv/bench/bench_data/<run>` (allenai's
  scorer through `scripts/olmocr_bench.py`) and the gauntlet output folders (`run_gauntlet.py --regrade` on
  copies, then `--table`).
- MinerU, marker, docling and pymupdf4llm were run on olmOCR-bench's 1,223-PDF test split, one tool at a time,
  `--jobs 1`, into `aud-<tool>-test`. Every tool converted all 1,223 PDFs with no failures.
- The fresh documents come from the held-out session (worktree `funny-kalam-ca66ec`): 11 new gauntlet cases
  and a frozen 133-page olmOCR-format set in `~/Library/Caches/fgv/heldout/`. Both were written before any
  converter ran on them. Only their scores were read, never their failures.
- Intervals are 95 % bootstraps over PDFs.
- The build is main at `75355f5`, whose PDF conversion matches the saved filegoblin runs.

## Verdicts

| claim | verdict |
|---|---|
| olmOCR-bench dev: filegoblin smart 77.21, MinerU 77.93, marker 70.8, docling 54.8, pymupdf4llm 40.5, markitdown 30.3 | **holds with a qualifier.** Every number reproduces exactly. They are dev-split numbers: 180 PDFs, on the split filegoblin was tuned on, with ±5-point intervals, and each tool in its default configuration (marker fast, docling without formula enrichment). |
| "within a point of MinerU, CPU-only" | **doesn't hold.** On the 1,223-PDF test split MinerU scores **79.01** against filegoblin's 74.31: **4.7 points ahead** (interval +2.6 to +6.5). On dev the two cannot be told apart (MinerU − filegoblin: −4.0 to +5.7). "CPU-only" holds: filegoblin used no GPU, while MinerU and marker used Metal/MPS. |
| olmOCR-bench test split: smart 74.31, off 41.13 | **holds with a qualifier.** Reproduces. About 0.6 (smart) and 0.5 (off) of it came from fixes found by reading test-split failures (1c). |
| the dev → test drop (77.2 → 74.3) is overfitting | **it is not, for smart mode.** A build from before the split existed drops just as much (1b). The fast path is somewhat overfit to dev. |
| "every competitor is 3–4 points higher on dev" | **doesn't hold.** dev → test: MinerU +1.1, pymupdf4llm +1.8, docling −0.2, marker −5.3. The 180-PDF dev split is simply too small to rank tools within about 5 points. |
| marker 70.8 | **holds with a qualifier**: fast mode, its default without CUDA. On the test split marker fast scores 65.55, and filegoblin leads it by 8.8 (5.9 to 11.4). On the frozen held-out set marker leads filegoblin (87.1 vs 85.9, not significant). In balanced mode marker scores **77.00 on dev**, level with filegoblin's 77.21 (4a). |
| gauntlet: filegoblin first at 91.5 | **holds only with heavy qualifiers.** It reproduces, but four things inflate it (3c). On the 11 PDF cases every tool finished, filegoblin smart scores 89.8 and MinerU 88.7. On 11 fresh cases docling leads (86.8) and filegoblin smart is second (79.4). |
| fast path level with pymupdf4llm (dev 40.54 vs 40.52) | **doesn't hold off the dev split.** Test: pymupdf4llm 42.27, filegoblin off 41.13 (the gap is not significant). Held-out set: 57.8 vs 47.8. Fresh gauntlet cases: 78.7 vs 68.1. |
| full bench "39.0 % → 74.7 %" (README) | **holds with a qualifier.** It reproduces for the 2026-10-23 build. The current build scores 41.06 → 74.79. The 1,403 PDFs include the 180 dev PDFs it was tuned on. |
| no hard-coding to benchmark documents | **holds** (section 2). |
| gauntlet gold not written from filegoblin's output | **holds** (3a). |
| grader changes neutral | **holds**; the one change since the gold favours the other tools' HTML output (3b). |

## 1. Tuning exposure

### 1a. The other tools on the test split

olmOCR-bench test split: 1,223 PDFs, 7,335 tests, official scorer. Overall is the mean of the 8 category
rates. Each tool is in its default configuration (section 4).

| category | filegoblin smart | filegoblin off | MinerU 4.1.2 | marker 2.1 (fast) | docling 2.145 | pymupdf4llm 2.0 |
|---|---:|---:|---:|---:|---:|---:|
| arXiv math | **87.1** | 0.0 | 77.7 | 23.1 | 0.0 | 0.0 |
| headers/footers | 91.6 | 74.0 | **95.7** | 93.9 | 90.1 | 32.4 |
| long tiny text | **88.1** | 54.1 | 83.9 | 63.2 | 77.7 | 59.6 |
| multi-column | 72.0 | 49.5 | **79.7** | 74.4 | 73.5 | 66.1 |
| old scans | 28.9 | 17.3 | 38.3 | **43.9** | 24.0 | 19.7 |
| old scans math | 47.6 | 0.0 | **76.7** | 60.8 | 0.0 | 0.0 |
| tables | 80.0 | 35.2 | **82.0** | 65.8 | 73.4 | 64.4 |
| baseline | 99.2 | 98.9 | 98.0 | **99.3** | 97.9 | 95.9 |
| **overall** | 74.31 | 41.13 | **79.01** | 65.55 | 54.58 | 42.27 |
| 95 % interval | 72.3–76.3 | 39.0–43.0 | 77.3–80.6 | 63.3–68.1 | 52.8–56.3 | 40.6–44.2 |
| minus filegoblin smart | — | | **+2.6 to +6.5** | −11.4 to −5.9 | −22.0 to −17.6 | |

pymupdf4llm minus filegoblin off: −1.5 to +3.9.

MinerU leads on scanned math (+29), scans (+9), multi-column (+8), headers/footers and tables; filegoblin
leads on arXiv math and long tiny text. Times are not comparable: the runs shared the machine with other
sessions' benchmarks and the vision slots.

### 1b. Is dev easier, and is filegoblin overfit to it?

| run | dev | test | test − dev |
|---|---:|---:|---:|
| filegoblin smart, `vision_final` (2026-10-12, before the split existed) | 74.68 | 71.84 | −2.84 |
| filegoblin smart, `base` (2026-10-22) | 75.66 | 72.24 | −3.42 |
| filegoblin smart, `final1` (10-23, tuned on dev) | 77.39 | 73.54 | −3.85 |
| filegoblin smart, `final` (10-23, after the test-informed fixes) | 77.21 | 74.18 | −3.03 |
| filegoblin smart, now | 77.21 | 74.31 | **−2.90** |
| filegoblin off, `base-off` (2026-10-22) | 33.13 | 35.48 | +2.35 |
| filegoblin off, `final-off` (10-23) | 37.13 | 39.27 | +2.14 |
| filegoblin off, now | 40.54 | 41.13 | **+0.59** |
| MinerU (never tuned here) | 77.93 | 79.01 | +1.08 |
| marker fast | 70.80 | 65.55 | −5.25 |
| docling | 54.77 | 54.58 | −0.19 |
| pymupdf4llm | 40.52 | 42.27 | +1.76 |

- **Smart mode is not overfit to dev.** A build from ten days before the split drops 2.84; the current
  build drops 2.90. Smart has gained the same on both splits since 2026-10-12 (dev +2.53, test +2.47). Dev
  tuning in the 10-23 pass widened the gap by 0.4, and the test-informed fixes (1c) took it back.
- **The fast path is.** It gained 3.41 on dev against 1.86 on test since `final-off`, nearly all in tables
  (booktabs and unruled tables: dev +20.7, test +8.6). Its gap fell from about +2.2 untuned to +0.59.
  About 1.5 points of its 40.54 dev score are specific to dev, which is the whole margin of "level with
  pymupdf4llm".
- **The hint is wrong.** Competitors do not uniformly score higher on dev; their test − dev ranges from
  −5.3 to +1.8. The dev split is 180 PDFs, and its intervals are ±5 points (MinerU − filegoblin on dev:
  −4.0 to +5.7; marker − filegoblin: −11.1 to −1.6). A ranking within 5 points needs the test split. The
  "dev agrees with published numbers" paragraph in COMPARISON.md was reading noise.
- **What changed between dev and test is MinerU, not filegoblin.** The relative change against MinerU, from
  0.7 behind to 4.7 behind, is not explained by filegoblin overfitting; filegoblin's own gap is its untuned
  gap. On dev's few scanned-math pages filegoblin happens to do well (65.0, against 47.6 on test; the
  pre-split build shows the same 61.2 vs 44.2). MinerU does not depend on that page sample (76.2 vs 76.7).

### 1c. How held out is the test split?

Changes chosen by reading test-split failures, with what each could be worth:

| when | what | measured effect on test |
|---|---|---|
| 2026-10-22/23 | R5 (table taxonomy, 237 failing table tests) and R6 (single-page furniture, thresholds "fitted and checked on 479 single pages") read failures across the whole bench | not separable; tables and headers/footers are where it would show |
| 2026-10-23 | five bugs found by reading the first full run's losses on test pages (ADR "Found in the final run") | `final1` → `final`: **test +0.64, dev −0.18** (smart); off +0.09 |
| 2026-10-30 | prose-columns guard, never-replace rule, grid fallback, from 16 test tests lost in off mode | ≤ 16 tests ≈ **+0.24** (off) |
| 2026-10-31 | unruled-table fixes, found on two test pages (`9317418`) | off 40.98 → 41.13, **+0.15**; smart +1 test |

So filegoblin's test-split numbers are optimistic by about **0.6–0.7 (smart)** and **0.5 (off)** against a
clean split, plus whatever R5/R6 contributed. The other tools carry none of this. Corrected for it, MinerU's
test lead is about 5.3 points.

### 1d. Fresh documents

**Held-out set.** 133 one-page PDFs and 1,009 tests in olmOCR's seven categories plus `multilingual`, frozen
before any converter ran. The 9-category overall uses all of them:

| | overall (9 categories) | 95 % interval | 8 olmOCR categories |
|---|---:|---|---:|
| marker 2.1 (fast) | **87.14** | 83.3–90.9 | **88.57** |
| filegoblin smart (`ho-fg-smart`) | 85.93 | 82.7–90.2 | 85.47 |
| filegoblin smart (`ho-fg-smart-asrun`, a second run) | 84.89 | 81.2–89.4 | 84.30 |
| MinerU 4.1.2 | 82.10 | 77.8–86.2 | 81.82 |
| docling 2.145 | 69.70 | 66.4–72.8 | 67.07 |
| pymupdf4llm 2.0 | 57.84 | 53.9–61.4 | 56.47 |
| filegoblin off | 47.81 | 43.9–52.0 | 45.62 |
| markitdown 0.1.9 | 40.69 | 36.7–44.7 | 38.19 |

- marker minus filegoblin smart: −2.3 to +4.5. filegoblin smart minus MinerU: +0.6 to +7.7.
- This set and the test split disagree about MinerU and marker. That is what 133 pages and a different mix
  of sources do (2026 arXiv papers, government reports, public-domain scans), and the intervals overlap with
  the test split's ranking only partly.
- The robust conclusions are the ones both agree on:
  - filegoblin smart is in the top group with MinerU and marker;
  - docling and pymupdf4llm are well behind;
  - the fast path trails pymupdf4llm or ties it.
- The held-out session should say which of its two smart runs is canonical.

**Fresh gauntlet cases** (11 PDFs; uncommitted in `funny-kalam-ca66ec`): see 3c. docling leads (86.8) and
filegoblin smart is second (79.4).

## 2. Hard-coding

Searched every string literal outside `#[cfg(test)]` in `crates/core/src/parsers/pdf*.rs`, `headings.rs`,
`ocr.rs` and `crates/core/src/vision/*.rs`, and grepped for the gauntlet's and olmOCR-bench's document names
(KEYTRUDA, ESP32, Tesla, Berk v. Choy, Attention/Zeitz/Meadows).

- **No document-specific string drives a decision.** Every case-like string ("01 1100 - 3", "(Slip Opinion)
  OCTOBER TERM", "Skin and Subcutaneous Tissue", "KE 10 mg/kg 3 YTRUDA", "borderforfifteen", "Table 3-13. R11
  Register Field Descriptions") is a comment example or a unit-test fixture. The word lists that exist
  (math-font names in `pdf_route.rs`/`pdf_words.rs`, operator names in `pdf_math.rs`, garbage shapes in
  `pdf_textq.rs`) are generic.
- **History:** `2bdcbcd` (2026-10-11) deleted the earlier document-specific literals ("Attention/Zeitz/Meadows
  answer keys") and word-rewriting regexes. None has come back.
- **No file-name checks.** Nothing in the converter reads the input's name for PDF decisions.
- **Thresholds.** The rules added where competitors won (the section folios and folio-row rule in
  `pdf_furniture.rs`, prompted by CSI_SPEC and SCOTUS_1) are general: a folio needs a fixed prefix whose
  number advances with the page index on two or more pages, and the folio-row rule has table-of-contents and
  table-edge guards. The single-page furniture thresholds of 2026-10-23 (top 8 % / bottom 13 %, and 10 % /
  15 % for lexical lines) were "fitted and checked on 479 single pages" of the benchmark, not limited to dev
  (1c).
- **What the fixtures do show** is which rules came from gauntlet cases: headings rules from FDA_1, TI_REG,
  CSI_SPEC and the SEC filings; furniture rules from SCOTUS_1 and CSI_SPEC. That is tuning exposure (3c),
  not hard-coding.

## 3. The gauntlet

### 3a. The gold is not written in filegoblin's style

`test_assets/gauntlet/gold/` has one commit (`27e4366`, 2026-10-14) and has not changed since.

- **Every text-layer gold page against its own text layer** (`pdftotext -f N -l N`, 35 pages in 15 PDF
  cases). 99–100 % of gold words are in the page's text layer. The rest are LaTeX command names in math gold
  (R2, SCI_1) and joined hyphenations. No gold page carries words that only filegoblin produced.
- **Five cases read against the page:**
  - SLIDE_1 p4: the table is the page's numbers, row for row.
  - FORM_1 p1: the gold reads the 1040 top to bottom as a person fills it. filegoblin's order differs
    (order 84).
  - TECH_1 p28: merged cells are written once with the covered cells empty. That is the opposite of
    filegoblin's convention, which repeats a spanning cell in every column; the grader collapses repeats for
    every tool.
  - SCOTUS_1 p1: the headings are what a reader sees. filegoblin scores 49 on them; docling and MinerU 82.
  - CLIN_1 p19: the protocol's numbered sections.
- **Component means over the 11 PDF cases every tool finished:**

  | | completeness | precision | order | tables | headings | math |
  |---|---:|---:|---:|---:|---:|---:|
  | filegoblin smart | 98.9 | 88.4 | 97.6 | 86.4 | 76.8 | 84.0 |
  | MinerU | 96.8 | 87.8 | 96.7 | **87.8** | 74.9 | 52.2 |
  | marker | 94.8 | 82.1 | 86.8 | 44.5 | 68.6 | 30.7 |
  | docling | 96.1 | 86.2 | 94.2 | 68.4 | **86.1** | 0.0 |
  | pymupdf4llm | 94.4 | 82.6 | 94.5 | 74.1 | 68.7 | 0.0 |

  The two components where the gold makes judgment calls (headings, tables) are led by other tools.
- **One convention favours LaTeX writers.** Math is graded only inside `$…$`/`$$…$$` (gold rule 6). MinerU
  wrote SCI_2's "1 × 10<sup>−3</sup>" as text and scored math 0 there. olmOCR-bench expects the same, and
  math weighs 10 %.

### 3b. Grader changes since the gold

There has been one change to `scripts/gauntlet_grade.py` since 2026-10-14 (`14816d2`): HTML tables are read
as pipe tables, and inline `<br>`, `<sup>`, `<span>` and similar tags are dropped.

- It helps the tools that write HTML (MinerU, marker); filegoblin's own HTML cases moved +0.3 to +1.2.
- Its tag list misses `<small>`, which MinerU wraps its footnotes in (52 times), so "small" counts as junk in
  MinerU's precision. The cost is tiny but one-sided; worth adding.
- `354ab0e` (math ignores dollar amounts) is from the gold's own day and applies to everyone.
- The held-out session's uncommitted per-character CJK tokens are neutral.

Re-grading every saved run with the current grader reproduces the published tables exactly.

### 3c. Tuning exposure and the "first at 91.5" claim

The gauntlet was the acceptance test for every change since 2026-10-14 ("kept only if … no gauntlet case loses
ground"). The 2026-10-30 pass then set out to fix the cases where the other tools had won: SCOTUS_1 78.4 →
88.4, CSI_SPEC 76.2 → 91.2. The others were measured once, untuned.

What inflates the 91.5:
- **It is the mean over 28 cases, 12 of them not PDFs.** marker and pymupdf4llm read only the 16 PDFs.
- **Three of the 12 are the synthetic PPTX files P1–P3.** They were built to reproduce known markitdown bugs
  (#332, #2526) and lack a `<p:spPr>` element the schema requires. MinerU, docling and markitdown raise on
  them, and each crash counts as 0.
- **MinerU's 61.0 counts five long documents as 0.** They were stopped by the harness's 8 GB cap (3d).
- **filegoblin wins 6 of the 16 PDF cases outright.** It loses 10 to the best other tool, three of them by
  under a point (R1, R2, TI_REG).

| PDF cases | n | filegoblin smart | MinerU | marker | docling | pymupdf4llm | fg off |
|---|---:|---:|---:|---:|---:|---:|---:|
| all 16 (failures = 0) | 16 | **89.0** | 61.0 | 80.4 | 84.7 | 74.9 | 76.6 |
| every tool finished | 11 | **89.8** | 88.7 | 79.5 | 84.6 | 74.1 | 77.5 |
| … minus SCOTUS_1, CSI_SPEC (tuned after the comparison) | 9 | **89.8** | 87.4 | 76.1 | 82.3 | 74.6 | 74.8 |

**On new documents the lead does not hold.** These are the held-out session's 11 PDF cases, all seven
configurations, build `75355f5`; the results are uncommitted and provisional:

| fresh PDF cases | n | filegoblin smart | fg off | MinerU | marker | docling | pymupdf4llm | markitdown |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| all | 11 | 79.4 | 68.1 | 76.7 | 77.7 | **86.8** | 78.7 | 48.2 |
| without Arabic/Hebrew | 9 | 87.3 | 75.7 | 80.5 | 80.4 | **89.1** | 79.2 | 53.5 |

### 3d. The other tools' failures

- **MinerU: BOOK_1, FIN_1, TECH_1, CLIN_1 and FDA_1** (51–259 pages) were killed by the harness's 8 GB RSS
  watchdog.
  - filegoblin runs under its own 6 GB CLI / 4 GB worker ceilings, and was re-engineered on 2026-10-30 to fit
    them. Before that, six of its own gauntlet PDFs failed the same way.
  - Whether MinerU finishes these with more memory was not tested. It was not tried here either, because of
    the machine's memory budget.
  - Established: "MinerU's standard tier passes 8 GB on documents over 50 pages". Not established: "MinerU
    cannot convert long documents".
- **MinerU, docling and markitdown on P1–P3:** real crashes, on schema-invalid synthetic files.
- **No competitor failed an olmOCR-bench page,** on dev or test: 0 non-zero exits and 0 empty outputs across
  MinerU, marker, docling and pymupdf4llm.

## 4. Configuration fairness

From `scripts/alt_engine_worker.py` and each tool's installed source:

| tool | ran as | better configuration it documents | effect |
|---|---|---|---|
| marker 2.1 | `PdfConverter()`, no mode set. marker picks `mode = "balanced" if TORCH_DEVICE_MODEL == "cuda" else "fast"` (`marker/converters/pdf.py`), so **fast** here | `--mode balanced`: the surya VLM for layout, inline-math OCR, full-page re-OCR of bad pages | marker's README: balanced 76.0 vs fast 66.6 on the full bench (GPU). See 4a. |
| MinerU 4.1.2 | `parse(tier="standard", ocr_mode="auto")`, what `mineru-kit parse` does | standard is the default | fair |
| docling 2.145 | `DocumentConverter()`: formula enrichment **off** | `do_formula_enrichment=True` | 2 of its 8 categories (arXiv math, old scans math) are 0.0 by configuration. Not measured with it on. |
| pymupdf4llm 2.0 | `to_markdown()` with PyMuPDF Layout and RapidOCR | Tesseract (a system package, not installed) | small; OCR pages only |
| markitdown 0.1.9 | `MarkItDown().convert()` | — | fair |

filegoblin ran `--vision smart`, its default, so every comparison is default against default. Claims that
need the qualifier:
- **"marker 70.8 / 65.6"** needs "fast mode, its default without CUDA".
- **"docling 54.8"** needs "formula enrichment off".
- **Any "beats marker"** needs "in fast mode".

### 4a. marker in balanced mode

Run on the dev split with `PdfConverter(config={"mode": "balanced"})`, the same worker otherwise. A scratch
copy of the harness was used; repo code is unchanged. The surya VLM ran through llama.cpp on Metal. All 180
PDFs converted.

| category | marker fast | marker balanced | filegoblin smart | MinerU |
|---|---:|---:|---:|---:|
| arXiv math | 26.6 | 74.4 | **87.5** | 75.5 |
| headers/footers | 97.8 | 97.8 | 95.6 | **98.9** |
| long tiny text | 80.4 | 76.8 | **89.3** | 85.7 |
| multi-column | 73.6 | 71.7 | 77.4 | **79.2** |
| old scans | **42.5** | **42.5** | 28.8 | 38.8 |
| old scans math | **82.5** | **82.5** | 65.0 | 76.2 |
| tables | 63.1 | 70.3 | **74.8** | 70.3 |
| baseline | **100.0** | **100.0** | 99.4 | 98.9 |
| **overall** | 70.80 | **77.00** | 77.21 | 77.93 |
| median s / PDF | 1.23 | 10.55 | 2.40 | 3.46 |

- **Balanced mode gains 6.2 points**, almost all from inline-math OCR (arXiv math +47.8) and tables (+7.2).
  That puts marker level with filegoblin and MinerU on dev (all within the ±5 interval).
- **filegoblin's dev lead over marker (6.4) is a property of marker's default fast mode**, not of marker.
- The log shows the layout VLM sometimes emitting malformed boxes under llama.cpp ("Skipping layout block with
  bad bbox"), so on a CUDA machine with vLLM, balanced may do better still.
- Balanced was not run on the test split; fast mode drops 5.3 points there.
- **Any claim of beating marker must say "fast mode".**

## 5. Every number we'd publish, recomputed

| number | source run | recomputed | |
|---|---|---:|---|
| dev, filegoblin smart 77.21 / off 40.54 | `cs-final`, `cs-final-off` | 77.21 / 40.54 | ✓ |
| dev, MinerU 77.93 / marker 70.80 / docling 54.77 / pymupdf4llm 40.52 / markitdown 30.31 | `cmp-*` | same | ✓ |
| dev before the 10-30 pass: smart 76.03, off 37.13 | `cmp-fg-smart`, `cmp-fg-off` | 76.03, 37.13 | ✓ |
| test, smart 74.31 / off 41.13 | `cs-final-test`, `cs-fix-test-off` | 74.31 / 41.13 | ✓ |
| test, 10-30: 73.70 / 39.28 → 74.29 / 40.08 | `cmp-fg-*`, `exp15-*` | same | ✓ |
| 10-23 test split 72.24 → 74.18 | `base`, `final` | same | ✓ |
| full bench 74.68 / 39.03 (README "74.7 %", "39.0 %") | `final`, `final-off` | same | ✓ for the 2026-10-23 build; the current build scores 74.79 / 41.06 |
| gauntlet smart 91.5 (28) / 89.0 (16 PDFs); off 84.4 / 76.6 | `cs-final-smart`, `cs-final-off`, regraded | same | ✓ |
| gauntlet MinerU 61.0 / marker 80.4 / docling 84.7 / pymupdf4llm 74.9 / markitdown 54.5 (PDF means) | `pdf-parsing-7bb3d5/target/gauntlet/*`, regraded | same | ✓ |
| COMPARISON "after" table: gauntlet off, PDF cases 76.4 | — | 76.6 now (the 10-31 pass moved FIN_1) | stale by 0.2 |
| GAUNTLET_SCORECARD header 91.4 | — | 91.5 since `2837c04` | stale by 0.1 |

Nothing failed to reproduce. What fails is the reading of them: dev-split differences under 5 points were
read as rankings.

## 6. What the docs should say

Corrected in this commit:
- `docs/COMPARISON.md`:
  - a warning at the top;
  - the test-split table of all tools, and the held-out results;
  - "default configuration" qualifiers;
  - the gauntlet caveats;
  - a corrected short answer.
- `README.md`: the full-bench figure updated to the current build, with a note that it includes the dev pages
  and with the test-split comparison.
- `GAUNTLET_SCORECARD.md`: a note on what the gauntlet can and cannot show against other tools.

Sentences the README rewrite can use as they stand:
- "On olmOCR-bench's 1,223-PDF test split, filegoblin `--vision smart` scores 74.3 on the CPU alone. In their
  default configurations, MinerU scores 79.0 (with the GPU), marker 65.6, docling 54.6 and pymupdf4llm 42.3."
- "filegoblin leads arXiv math (87 vs MinerU's 78) and long tiny text, and is behind MinerU on scans,
  scanned math and multi-column pages."
- "On 133 fresh pages written after the tuning: marker (fast) 87.1, filegoblin 85.9, MinerU 82.1, docling
  69.7, pymupdf4llm 57.8."

Sentences it should not use:
- "within a point of MinerU". On the larger split it is 4.7 behind, and on dev the two cannot be told apart.
- "first on the gauntlet" without "on our own 28-case suite, including non-PDF formats".
- "the fast path matches pymupdf4llm".

## Follow-ups (not done here)

- Add `<small>` (and similar inline tags) to the grader's tag list (3b).
- Measure MinerU's five long gauntlet documents with a higher cap, on a quiet machine (3d).
- Measure docling with formula enrichment on (4).
- The held-out session: commit the 11 gauntlet cases and say which held-out smart run is canonical.

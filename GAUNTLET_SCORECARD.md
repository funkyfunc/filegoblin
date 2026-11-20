# Filegoblin Adversarial Gauntlet Scorecard

Mode: `--vision smart`

**Canaries:** 37/40 (92.5%) · **Quality:** 90.7 / 100 (mean over 39 cases with gold excerpts)

<!-- notes -->
Measured 2026-11-19 on an Apple M4 Pro at `4816489` (`--features vision`: the page reader reads scans OCR is less than 0.99 sure of; see docs/ADR.md). The gauntlet now has 40 cases: the 29 below the first table plus the 11 added on 2026-11-01. Every run since the memory ceiling of `4283928` holds the CLI to 6 GB and its PDF worker to 4 GB (FILEGOBLIN_MEMORY_MB / FILEGOBLIN_PDF_MEMORY_MB, set by the harness), and converts one case at a time; latencies below overlapped other tools' runs on the same machine. With `--vision off` the same build scores 36/40 canaries and 82.3 quality. Other tools on the same cases: docs/COMPARISON.md (`run_gauntlet.py --engine`).

| run | canaries | quality |
|---|---|---|
| `--vision smart`, before the scan work (`19f7ee3`) | 29/29 | 87.9 |
| `--vision smart`, after the scan work (`46a1d55`) | 29/29 | 88.7 |
| `--vision smart`, after the PDF accuracy pass (no memory ceiling) | 29/29 | 90.5 |
| `--vision smart`, `4283928` with a 4 GB worker: six PDFs stopped | 23/29 | 74.1 |
| `--vision smart`, after the 2026-10-30 pass | 29/29 | 91.4 |
| `--vision off`, before the PDF accuracy pass | 28/29 | 82.1 |
| `--vision off`, after the PDF accuracy pass | 28/29 | 83.1 |
| `--vision off`, after the 2026-10-30 pass | 28/29 | 84.3 |
| `--vision smart`, `4816489`, the original 29 cases (current grader) | 29/29 | 91.7 |
| `--vision smart`, `4816489`, all 40 cases | 37/40 | 90.7 |
| `--vision off`, `4816489`, the original 29 cases (current grader) | 28/29 | 84.8 |
| `--vision off`, `4816489`, all 40 cases | 36/40 | 82.3 |

The 2026-10-30 pass: R1, R2, BOOK_2, FIN_1, CLIN_1 and FDA_1 convert again inside the 4 GB worker; CSI_SPEC 76.2 → 91.2 and SCOTUS_1 78.4 → 88.4 in both modes (running heads); SCI_2 64.8 → 71.6 with `--vision off` (booktabs tables); no case lost ground in either mode. The 83.0 → 83.1 off-mode step is the grader now ignoring inline HTML tags (`<br/>` in table cells), not a change in output.

The gauntlet is filegoblin's own acceptance suite: every change since 2026-10-14 was kept only if no case lost ground, so its scores measure regressions, not standing against other tools. On the 11 cases added 2026-11-01, filegoblin smart scored 80.7 against docling's 84.9 when they were fresh (docs/reviews/2026-11-01-claims-audit.md). It now scores 88.3 on them, but they are no longer fresh: the right-to-left fix was made for RTL_AR and RTL_HE, and RECEIPT_1 and NEWS_1 checked the page reader's routing.

Three canaries miss at `4816489`. CJK_ZH and FORM_FILLED missed theirs on 2026-11-01 as well. On RECEIPT_1, the page reader writes "DOMINO CONECT." for "DOMINO CONFECT"; an earlier run of the same routing read it right, so its reading differs a little from run to run.

Regraded 2026-11-01 after the gold audit (`docs/COMPARISON.md`, "The gold audit"): the same saved outputs score 91.6 (smart) and 84.7 (off); the 2026-10-31 rows of the table above keep their original grading. The per-case rows below are the 2026-11-19 run.

This block is kept when a run rewrites the scorecard; edit it by hand.
<!-- /notes -->

Quality is graded against hand-checked gold excerpts in `test_assets/gauntlet/gold/`; components are completeness, precision, order, tables, headings and math (see `scripts/gauntlet_grade.py`).

| ID | Category | Canaries | Quality | Weakest | Latency | First failure |
| :--- | :--- | :--- | ---: | :--- | ---: | :--- |
| **P1** | PPTX Diagrams | ✅ PASS | 100.0 | completeness 100 | 0.7 s |  |
| **P2** | PPTX Grouped Shapes | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **P3** | PPTX Chart Gaps | ✅ PASS | 98.5 | precision 93 | 0.0 s |  |
| **D1** | DOCX Tables | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **D3** | DOCX Floating Text Boxes | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **D4** | DOCX Equations | ✅ PASS | 92.8 | precision 85 | 0.0 s |  |
| **R1** | PDF Layout | ✅ PASS | 97.2 | precision 92 | 28.0 s |  |
| **R2** | PDF Math | ✅ PASS | 96.0 | precision 92 | 182.6 s |  |
| **SCI_1** | Scientific Paper (arXiv) | ✅ PASS | 92.0 | tables 74 | 18.7 s |  |
| **SCI_2** | Scientific Paper (arXiv) | ✅ PASS | 96.8 | precision 91 | 18.7 s |  |
| **BOOK_1** | Full Book (300pp) | ✅ PASS | 75.0 |  | 15.9 s |  |
| **BOOK_2** | Full Book (Scan) | ✅ PASS | — |  | 113.1 s |  |
| **TBL_1** | PDF Tables | ✅ PASS | 100.0 | completeness 100 | 0.6 s |  |
| **WEB_1** | HTML Web Articles | ✅ PASS | 99.6 | precision 98 | 0.0 s |  |
| **FIN_1** | Corporate Financial 10-K | ✅ PASS | 90.4 | headings 67 | 218.8 s |  |
| **FIN_2** | SEC EDGAR iXBRL HTML | ✅ PASS | 88.2 | precision 83; leaks: Apple Inc. | 2025 Form 10-K | 5 | 0.2 s |  |
| **FORM_1** | Government Tax Forms | ✅ PASS | 71.3 | tables 49; leaks: Created 9/5/25 | 0.2 s |  |
| **TECH_1** | Hardware Datasheets | ✅ PASS | 90.1 | tables 74 | 68.9 s |  |
| **SLIDE_1** | Corporate Investor Relations Deck | ✅ PASS | 86.8 | headings 25 | 49.7 s |  |
| **SEC_S1** | SEC S-1 Registration | ✅ PASS | 85.8 | tables 65 | 0.5 s |  |
| **SEC_CREDIT** | Debt Credit Agreements | ✅ PASS | 85.1 | precision 86; leaks: ACTIVE/119835323.10, 750894493.17 | 0.3 s |  |
| **SEC_PROXY** | SEC DEF 14A Proxy | ✅ PASS | 94.4 | precision 85 | 0.3 s |  |
| **LEGAL_VIE** | Cross-Border VIE Contracts | ✅ PASS | 93.5 | precision 85 | 0.1 s |  |
| **SCOTUS_1** | Supreme Court Slip Opinion | ✅ PASS | 87.2 | headings 41 | 1.1 s |  |
| **PATENT_1** | USPTO Patents | ✅ PASS | 79.5 | precision 70; leaks: Sheet 2 of 3 | 6.8 s |  |
| **CLIN_1** | Clinical Study Protocols | ✅ PASS | 93.6 | headings 83 | 53.9 s |  |
| **FDA_1** | FDA Drug Safety Labels | ✅ PASS | 88.2 | headings 61 | 138.7 s |  |
| **TI_REG** | Hardware Register Maps | ✅ PASS | 94.4 | precision 85 | 55.9 s |  |
| **CSI_SPEC** | CSI MasterFormat Specs | ✅ PASS | 91.2 | headings 66 | 0.2 s |  |
| **CJK_ZH** | Multilingual CJK (Traditional Chinese) | ❌ FAIL | 89.3 | headings 40 | 2.4 s | Missing canary: '就業失業重要統計指標' |
| **CJK_JA** | Multilingual CJK (Japanese) | ✅ PASS | 90.8 | headings 65 | 2.0 s |  |
| **RTL_AR** | Multilingual RTL (Arabic) | ✅ PASS | 81.5 | headings 0 | 1.3 s |  |
| **RTL_HE** | Multilingual RTL (Hebrew) | ✅ PASS | 84.2 | headings 0 | 1.3 s |  |
| **EU_ACCENT** | Multilingual Accented European (French) | ✅ PASS | 80.8 | tables 33 | 0.6 s |  |
| **RECEIPT_1** | Scanned Receipts | ❌ FAIL | 97.4 | precision 96 | 13.3 s | Missing canary: 'DOMINO CONFECT' |
| **INVOICE_1** | Invoices | ✅ PASS | 92.7 | headings 50 | 4.4 s |  |
| **FORM_FILLED** | Filled PDF Forms | ❌ FAIL | 81.1 | headings 50 | 0.5 s | Missing canary: '166.15' |
| **NEWS_1** | Historic Newspaper Scans | ✅ PASS | 89.9 | headings 75 | 48.2 s |  |
| **NEWS_2** | Newsletters (born-digital) | ✅ PASS | 95.7 | order 88 | 1.6 s |  |
| **SLIDES_2** | Slide Decks (PDF) | ✅ PASS | 87.6 | precision 94; leaks: To be presented by, Unclassified / Open Access | 9.7 s |  |

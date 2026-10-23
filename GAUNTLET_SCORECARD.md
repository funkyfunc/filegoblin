# Filegoblin Adversarial Gauntlet Scorecard

Mode: `--vision smart`

**Canaries:** 29/29 (100.0%) · **Quality:** 90.5 / 100 (mean over 28 cases with gold excerpts)

<!-- notes -->
Measured 2026-10-23 on an Apple M4 Pro, after the PDF accuracy pass (glyph-level words, rotated pages, ruled tables, inline math, single-page furniture, hardening; see docs/ADR.md). With `--vision off` the same cases score 28/29 canaries and 83.0 quality; R2 fails there because the fast path writes no LaTeX. Latencies below were taken with three cases running at once.

| run | canaries | quality |
|---|---|---|
| `--vision smart`, before the scan work (`19f7ee3`) | 29/29 | 87.9 |
| `--vision smart`, after the scan work (`46a1d55`) | 29/29 | 88.7 |
| `--vision smart`, after the PDF accuracy pass | 29/29 | 90.5 |
| `--vision off`, before the PDF accuracy pass | 28/29 | 82.1 |
| `--vision off`, after the PDF accuracy pass | 28/29 | 83.0 |

The largest moves in the accuracy pass: BOOK_1 50.0 → 75.0 (both modes), TECH_1 74.8 → 89.9, FDA_1 83.6 → 87.0, CLIN_1 90.8 → 93.6, R2 91.4 → 93.2; no case lost more than 0.2.

This block is kept when a run rewrites the scorecard; edit it by hand.
<!-- /notes -->

Quality is graded against hand-checked gold excerpts in `test_assets/gauntlet/gold/`; components are completeness, precision, order, tables, headings and math (see `scripts/gauntlet_grade.py`).

| ID | Category | Canaries | Quality | Weakest | Latency | First failure |
| :--- | :--- | :--- | ---: | :--- | ---: | :--- |
| **P1** | PPTX Diagrams | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **P2** | PPTX Grouped Shapes | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **P3** | PPTX Chart Gaps | ✅ PASS | 98.5 | precision 93 | 0.0 s |  |
| **D1** | DOCX Tables | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **D3** | DOCX Floating Text Boxes | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **D4** | DOCX Equations | ✅ PASS | 92.1 | precision 85 | 0.0 s |  |
| **R1** | PDF Layout | ✅ PASS | 97.2 | precision 92 | 60.8 s |  |
| **R2** | PDF Math | ✅ PASS | 93.2 | math 84 | 84.9 s |  |
| **SCI_1** | Scientific Paper (arXiv) | ✅ PASS | 90.8 | math 68 | 30.9 s |  |
| **SCI_2** | Scientific Paper (arXiv) | ✅ PASS | 96.6 | precision 91 | 35.7 s |  |
| **BOOK_1** | Full Book (300pp) | ✅ PASS | 75.0 |  | 14.7 s |  |
| **BOOK_2** | Full Book (Scan) | ✅ PASS | — |  | 225.5 s |  |
| **TBL_1** | PDF Tables | ✅ PASS | 100.0 | completeness 100 | 1.3 s |  |
| **WEB_1** | HTML Web Articles | ✅ PASS | 99.6 | precision 98 | 0.0 s |  |
| **FIN_1** | Corporate Financial 10-K | ✅ PASS | 90.2 | headings 67 | 445.1 s |  |
| **FIN_2** | SEC EDGAR iXBRL HTML | ✅ PASS | 87.9 | tables 80; leaks: Apple Inc. | 2025 Form 10-K | 5 | 0.2 s |  |
| **FORM_1** | Government Tax Forms | ✅ PASS | 70.6 | tables 49; leaks: Created 9/5/25 | 0.2 s |  |
| **TECH_1** | Hardware Datasheets | ✅ PASS | 89.9 | tables 74 | 118.1 s |  |
| **SLIDE_1** | Corporate Investor Relations Deck | ✅ PASS | 86.7 | headings 25 | 45.7 s |  |
| **SEC_S1** | SEC S-1 Registration | ✅ PASS | 85.1 | tables 58 | 0.6 s |  |
| **SEC_CREDIT** | Debt Credit Agreements | ✅ PASS | 85.1 | precision 86; leaks: ACTIVE/119835323.10, 750894493.17 | 0.1 s |  |
| **SEC_PROXY** | SEC DEF 14A Proxy | ✅ PASS | 93.3 | precision 83 | 0.4 s |  |
| **LEGAL_VIE** | Cross-Border VIE Contracts | ✅ PASS | 93.5 | precision 85 | 0.0 s |  |
| **SCOTUS_1** | Supreme Court Slip Opinion | ✅ PASS | 78.4 | headings 49; leaks: (Slip Opinion), OCTOBER TERM, 2025 | 2.0 s |  |
| **PATENT_1** | USPTO Patents | ✅ PASS | 78.5 | headings 67; leaks: Sheet 2 of 3 | 13.0 s |  |
| **CLIN_1** | Clinical Study Protocols | ✅ PASS | 93.6 | headings 83 | 104.1 s |  |
| **FDA_1** | FDA Drug Safety Labels | ✅ PASS | 87.0 | headings 50 | 195.1 s |  |
| **TI_REG** | Hardware Register Maps | ✅ PASS | 94.4 | precision 85 | 119.9 s |  |
| **CSI_SPEC** | CSI MasterFormat Specs | ✅ PASS | 76.2 | headings 66; leaks: JOHNSON DEAN PRESERVE, 01 1100 - 1 | 0.3 s |  |

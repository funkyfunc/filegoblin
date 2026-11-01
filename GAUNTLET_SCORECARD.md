# Filegoblin Adversarial Gauntlet Scorecard

Mode: `--vision smart`

**Canaries:** 29/29 (100.0%) · **Quality:** 91.4 / 100 (mean over 28 cases with gold excerpts)

<!-- notes -->
Measured 2026-10-31 on an Apple M4 Pro, after the 2026-10-30 pass (smart mode inside a 4 GB PDF worker, booktabs tables on the fast path, running heads that do not recur; see docs/ADR.md). Every run since the memory ceiling of `4283928` holds the CLI to 6 GB and its PDF worker to 4 GB (FILEGOBLIN_MEMORY_MB / FILEGOBLIN_PDF_MEMORY_MB, set by the harness), and converts one case at a time. With `--vision off` the same cases score 28/29 canaries and 84.3 quality; R2 fails there because the fast path writes no LaTeX. Other tools on the same cases: docs/COMPARISON.md (`run_gauntlet.py --engine`).

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

The 2026-10-30 pass: R1, R2, BOOK_2, FIN_1, CLIN_1 and FDA_1 convert again inside the 4 GB worker; CSI_SPEC 76.2 → 91.2 and SCOTUS_1 78.4 → 88.4 in both modes (running heads); SCI_2 64.8 → 71.6 with `--vision off` (booktabs tables); no case lost ground in either mode. The 83.0 → 83.1 off-mode step is the grader now ignoring inline HTML tags (`<br/>` in table cells), not a change in output.

The gauntlet is filegoblin's own acceptance suite: every change since 2026-10-14 was kept only if no case lost ground, so its scores measure regressions, not standing against other tools. On 11 fresh cases written after the tuning, filegoblin smart scored 80.8 against docling's 84.7 (docs/reviews/2026-11-01-claims-audit.md).

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
| **R1** | PDF Layout | ✅ PASS | 97.3 | precision 92 | 24.0 s |  |
| **R2** | PDF Math | ✅ PASS | 93.2 | math 84 | 38.2 s |  |
| **SCI_1** | Scientific Paper (arXiv) | ✅ PASS | 90.8 | math 68 | 12.5 s |  |
| **SCI_2** | Scientific Paper (arXiv) | ✅ PASS | 96.6 | precision 91 | 13.1 s |  |
| **BOOK_1** | Full Book (300pp) | ✅ PASS | 75.0 |  | 5.9 s |  |
| **BOOK_2** | Full Book (Scan) | ✅ PASS | — |  | 103.3 s |  |
| **TBL_1** | PDF Tables | ✅ PASS | 100.0 | completeness 100 | 0.5 s |  |
| **WEB_1** | HTML Web Articles | ✅ PASS | 99.6 | precision 98 | 0.0 s |  |
| **FIN_1** | Corporate Financial 10-K | ✅ PASS | 90.2 | headings 67 | 182.9 s |  |
| **FIN_2** | SEC EDGAR iXBRL HTML | ✅ PASS | 88.2 | precision 83; leaks: Apple Inc. | 2025 Form 10-K | 5 | 0.1 s |  |
| **FORM_1** | Government Tax Forms | ✅ PASS | 70.6 | tables 49; leaks: Created 9/5/25 | 0.1 s |  |
| **TECH_1** | Hardware Datasheets | ✅ PASS | 89.9 | tables 74 | 40.0 s |  |
| **SLIDE_1** | Corporate Investor Relations Deck | ✅ PASS | 86.7 | headings 25 | 19.0 s |  |
| **SEC_S1** | SEC S-1 Registration | ✅ PASS | 85.8 | tables 65 | 0.4 s |  |
| **SEC_CREDIT** | Debt Credit Agreements | ✅ PASS | 85.1 | precision 86; leaks: ACTIVE/119835323.10, 750894493.17 | 0.2 s |  |
| **SEC_PROXY** | SEC DEF 14A Proxy | ✅ PASS | 94.4 | precision 85 | 0.3 s |  |
| **LEGAL_VIE** | Cross-Border VIE Contracts | ✅ PASS | 93.5 | precision 85 | 0.0 s |  |
| **SCOTUS_1** | Supreme Court Slip Opinion | ✅ PASS | 88.4 | headings 49 | 0.9 s |  |
| **PATENT_1** | USPTO Patents | ✅ PASS | 78.5 | headings 67; leaks: Sheet 2 of 3 | 5.4 s |  |
| **CLIN_1** | Clinical Study Protocols | ✅ PASS | 93.6 | headings 83 | 38.1 s |  |
| **FDA_1** | FDA Drug Safety Labels | ✅ PASS | 87.0 | headings 50 | 85.1 s |  |
| **TI_REG** | Hardware Register Maps | ✅ PASS | 94.4 | precision 85 | 56.4 s |  |
| **CSI_SPEC** | CSI MasterFormat Specs | ✅ PASS | 91.2 | headings 66 | 0.2 s |  |

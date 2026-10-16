# Filegoblin Adversarial Gauntlet Scorecard

Mode: `--vision smart`

**Canaries:** 29/29 (100.0%) · **Quality:** 88.7 / 100 (mean over 28 cases with gold excerpts)

Measured 2026-10-15 on an Apple M4 Pro, after the scan-accuracy work and its ablations (PP-OCRv6 medium, text-layer quality gate, scan furniture; see docs/ADR.md). With `--vision off` the same cases score 28/29 canaries and 82.1 quality, unchanged; R2 fails there because the fast path writes no LaTeX. Latencies below were taken with other benchmark jobs running on the same machine.

| run | canaries | quality |
|---|---|---|
| `--vision smart`, before the scan work (`19f7ee3`) | 29/29 | 87.9 |
| `--vision smart`, first push (`b764d36`) | 29/29 | 88.6 |
| `--vision smart`, now | 29/29 | 88.7 |
| `--vision off`, all three | 28/29 | 82.1 |

Quality is graded against hand-checked gold excerpts in `test_assets/gauntlet/gold/`; components are completeness, precision, order, tables, headings and math (see `scripts/gauntlet_grade.py`).

| ID | Category | Canaries | Quality | Weakest | Latency | First failure |
| :--- | :--- | :--- | ---: | :--- | ---: | :--- |
| **P1** | PPTX Diagrams | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **P2** | PPTX Grouped Shapes | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **P3** | PPTX Chart Gaps | ✅ PASS | 98.5 | precision 93 | 0.0 s |  |
| **D1** | DOCX Tables | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **D3** | DOCX Floating Text Boxes | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **D4** | DOCX Equations | ✅ PASS | 92.1 | precision 85 | 0.0 s |  |
| **R1** | PDF Layout | ✅ PASS | 97.1 | precision 92 | 21.6 s |  |
| **R2** | PDF Math | ✅ PASS | 91.4 | math 73 | 36.1 s |  |
| **SCI_1** | Scientific Paper (arXiv) | ✅ PASS | 90.9 | math 68 | 11.3 s |  |
| **SCI_2** | Scientific Paper (arXiv) | ✅ PASS | 96.1 | precision 90 | 11.7 s |  |
| **BOOK_1** | Full Book (300pp) | ✅ PASS | 50.0 |  | 15.6 s |  |
| **BOOK_2** | Full Book (Scan) | ✅ PASS | — |  | 95.8 s |  |
| **TBL_1** | PDF Tables | ✅ PASS | 100.0 | completeness 100 | 0.5 s |  |
| **WEB_1** | HTML Web Articles | ✅ PASS | 99.6 | precision 98 | 0.0 s |  |
| **FIN_1** | Corporate Financial 10-K | ✅ PASS | 89.8 | headings 67 | 187.5 s |  |
| **FIN_2** | SEC EDGAR iXBRL HTML | ✅ PASS | 87.9 | tables 80; leaks: Apple Inc. | 2025 Form 10-K | 5 | 0.1 s |  |
| **FORM_1** | Government Tax Forms | ✅ PASS | 70.6 | tables 49; leaks: Created 9/5/25 | 0.1 s |  |
| **TECH_1** | Hardware Datasheets | ✅ PASS | 74.8 | tables 66 | 41.7 s |  |
| **SLIDE_1** | Corporate Investor Relations Deck | ✅ PASS | 86.7 | headings 25 | 17.5 s |  |
| **SEC_S1** | SEC S-1 Registration | ✅ PASS | 85.1 | tables 58 | 0.4 s |  |
| **SEC_CREDIT** | Debt Credit Agreements | ✅ PASS | 85.1 | precision 86; leaks: ACTIVE/119835323.10, 750894493.17 | 0.1 s |  |
| **SEC_PROXY** | SEC DEF 14A Proxy | ✅ PASS | 93.3 | precision 83 | 0.3 s |  |
| **LEGAL_VIE** | Cross-Border VIE Contracts | ✅ PASS | 93.5 | precision 85 | 0.0 s |  |
| **SCOTUS_1** | Supreme Court Slip Opinion | ✅ PASS | 77.9 | headings 49; leaks: (Slip Opinion), OCTOBER TERM, 2025 | 0.8 s |  |
| **PATENT_1** | USPTO Patents | ✅ PASS | 78.7 | headings 67; leaks: Sheet 2 of 3 | 4.6 s |  |
| **CLIN_1** | Clinical Study Protocols | ✅ PASS | 90.8 | tables 79 | 35.0 s |  |
| **FDA_1** | FDA Drug Safety Labels | ✅ PASS | 83.6 | headings 28 | 86.7 s |  |
| **TI_REG** | Hardware Register Maps | ✅ PASS | 92.8 | precision 84 | 58.9 s |  |
| **CSI_SPEC** | CSI MasterFormat Specs | ✅ PASS | 76.2 | headings 66; leaks: JOHNSON DEAN PRESERVE, 01 1100 - 1 | 0.2 s |  |

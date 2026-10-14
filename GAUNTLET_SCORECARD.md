# Filegoblin Adversarial Gauntlet Scorecard

Mode: `--vision smart`

**Canaries:** 28/29 (96.6%) · **Quality:** 81.8 / 100 (mean over 28 cases with gold excerpts)

Quality is graded against hand-checked gold excerpts in `test_assets/gauntlet/gold/`; components are completeness, precision, order, tables, headings and math (see `scripts/gauntlet_grade.py`).

| ID | Category | Canaries | Quality | Weakest | Latency | First failure |
| :--- | :--- | :--- | ---: | :--- | ---: | :--- |
| **P1** | PPTX Diagrams | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **P2** | PPTX Grouped Shapes | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **P3** | PPTX Chart Gaps | ✅ PASS | 98.5 | precision 93 | 0.0 s |  |
| **D1** | DOCX Tables | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **D3** | DOCX Floating Text Boxes | ✅ PASS | 100.0 | completeness 100 | 0.0 s |  |
| **D4** | DOCX Equations | ✅ PASS | 92.1 | precision 85 | 0.0 s |  |
| **R1** | PDF Layout | ✅ PASS | 87.0 | precision 91; leaks: INTERNATIONAL JOURNAL OF INTELLIGENT SYSTEMS, CCC 0884-8173/93/010071-20 | 5.9 s |  |
| **R2** | PDF Math | ✅ PASS | 78.0 | math 29 | 1.6 s |  |
| **SCI_1** | Scientific Paper (arXiv) | ✅ PASS | 90.9 | math 68 | 7.0 s |  |
| **SCI_2** | Scientific Paper (arXiv) | ✅ PASS | 96.1 | precision 90 | 7.2 s |  |
| **BOOK_1** | Full Book (300pp) | ✅ PASS | 0.0 |  | 7.5 s |  |
| **BOOK_2** | Full Book (Scan) | ✅ PASS | — |  | 24.9 s |  |
| **TBL_1** | PDF Tables | ✅ PASS | 100.0 | completeness 100 | 0.5 s |  |
| **WEB_1** | HTML Web Articles | ✅ PASS | 99.6 | precision 98 | 0.0 s |  |
| **FIN_1** | Corporate Financial 10-K | ✅ PASS | 79.9 | headings 0 | 54.7 s |  |
| **FIN_2** | SEC EDGAR iXBRL HTML | ✅ PASS | 69.6 | headings 0; leaks: Apple Inc. | 2025 Form 10-K | 5, Apple Inc. | 2025 Form 10-K | 29 | 0.1 s |  |
| **FORM_1** | Government Tax Forms | ✅ PASS | 70.5 | tables 49; leaks: Created 9/5/25 | 0.1 s |  |
| **TECH_1** | Hardware Datasheets | ✅ PASS | 74.3 | headings 33 | 29.2 s |  |
| **SLIDE_1** | Corporate Investor Relations Deck | ✅ PASS | 87.5 | headings 25 | 7.6 s |  |
| **SEC_S1** | SEC S-1 Registration | ✅ PASS | 69.8 | headings 0; leaks: d752207ds1a.htm | 0.3 s |  |
| **SEC_CREDIT** | Debt Credit Agreements | ✅ PASS | 74.9 | headings 0; leaks: dynatrace-creditagreementc.htm, ACTIVE/119835323.10 | 0.1 s |  |
| **SEC_PROXY** | SEC DEF 14A Proxy | ✅ PASS | 66.7 | headings 0; leaks: Merck & Co., Inc., Rahway, NJ, U.S.A., 2026 Proxy Statement, 0000310158 | 0.2 s |  |
| **LEGAL_VIE** | Cross-Border VIE Contracts | ✅ PASS | 74.2 | headings 0; leaks: d118108dex1016.htm | 0.0 s |  |
| **SCOTUS_1** | Supreme Court Slip Opinion | ✅ PASS | 72.9 | headings 49; leaks: (Slip Opinion), OCTOBER TERM, 2025 | 0.1 s |  |
| **PATENT_1** | USPTO Patents | ❌ FAIL | 78.6 | headings 67; leaks: Sheet 2 of 3 | 4.9 s | Missing canary: 'limitations on the scope of any invention' |
| **CLIN_1** | Clinical Study Protocols | ✅ PASS | 90.8 | tables 79 | 13.1 s |  |
| **FDA_1** | FDA Drug Safety Labels | ✅ PASS | 81.5 | headings 15 | 64.4 s |  |
| **TI_REG** | Hardware Register Maps | ✅ PASS | 89.1 | headings 56 | 61.6 s |  |
| **CSI_SPEC** | CSI MasterFormat Specs | ✅ PASS | 67.9 | headings 12; leaks: JOHNSON DEAN PRESERVE, 01 1100 - 1 | 0.1 s |  |

Measured 2026-10-14 on an Apple M4 Pro. With `--vision off` the same cases score 28/29 canaries and 74.7 quality (R2 fails: the fast path writes no LaTeX). With `--formats`, all 29 cases keep 100% of the markdown's content in `--format plain` and in `--chunk-size 512 --format json`.

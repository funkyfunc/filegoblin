# Filegoblin Adversarial Gauntlet Scorecard

**Overall Score:** 19/19 (100.0%)

| ID | Category | Description | Status | Latency | Primary Failure Reason |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **P1** | PPTX Diagrams | Flowchart with glued connectors (<p:cxnSp>) | ✅ PASS | 11.1 ms | None |
| **P2** | PPTX Grouped Shapes | Text inside nested <p:grpSp> groups (MarkItDown #332) | ✅ PASS | 9.4 ms | None |
| **P3** | PPTX Chart Gaps | Chart with missing data points, must not print literal 'None' (MarkItDown #2526) | ✅ PASS | 9.1 ms | None |
| **D1** | DOCX Tables | Word table with gridSpan merged headers | ✅ PASS | 8.9 ms | None |
| **D3** | DOCX Floating Text Boxes | Floating text box inside mc:AlternateContent (Docling #4569) | ✅ PASS | 8.7 ms | None |
| **D4** | DOCX Equations | Word equations in OMML format compiled to LaTeX | ✅ PASS | 31.1 ms | None |
| **R1** | PDF Layout | Two-column scanned layout without line interleaving | ✅ PASS | 72.8 ms | None |
| **R2** | PDF Math | Heavy mathematical notation and inequalities (CMSY/CMMI) | ✅ PASS | 295.1 ms | None |
| **SCI_1** | Scientific Paper (arXiv) | Attention Is All You Need: 2-column, Transformer diagram, multi-head attention math | ✅ PASS | 896.2 ms | None |
| **SCI_2** | Scientific Paper (arXiv) | Dense 2-column mathematics and transformer architectures (arXiv:2401.14373) | ✅ PASS | 572.1 ms | None |
| **BOOK_1** | Full Book (300pp) | Thinking in Systems (Meadows, 235pp): flat RAM, chapters, callouts | ✅ PASS | 4923.4 ms | None |
| **BOOK_2** | Full Book (Scan) | Structuring Interviews with Experts (21pp scan): high-throughput Apple Vision OCR | ✅ PASS | 484.6 ms | None |
| **TBL_1** | PDF Tables | Complex ruling-line PDF table with colspans and multi-line cells | ✅ PASS | 53.4 ms | None |
| **WEB_1** | HTML Web Articles | Noisy technical blog: boilerplate and ad purge with code/table preservation | ✅ PASS | 20.4 ms | None |
| **FIN_1** | Corporate Financial 10-K | Berkshire Hathaway Annual Report (140pp): multi-page balance sheets and operating earnings | ✅ PASS | 12159.6 ms | None |
| **FIN_2** | SEC EDGAR iXBRL HTML | Apple Inc. 10-K (iXBRL HTML): preserve dense financial statement tables without article boilerplate stripping | ✅ PASS | 986.5 ms | None |
| **FORM_1** | Government Tax Forms | IRS Form 1040: spatial form grids, label-value bounding boxes, and SSN field recovery | ✅ PASS | 469.5 ms | None |
| **TECH_1** | Hardware Datasheets | ESP32 SoC Datasheet: pinout matrices, peripheral tables, and register memory maps | ✅ PASS | 4349.6 ms | None |
| **SLIDE_1** | Corporate Investor Relations Deck | Tesla Q4 & FY 2023 Update: wide-format presentation slides, financial metric callouts, and vehicle deliveries | ✅ PASS | 1425.9 ms | None |

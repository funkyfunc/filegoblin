# Filegoblin Adversarial Gauntlet Scorecard

**Overall Score:** 28/29 (96.6%)

| ID | Category | Description | Status | Latency | Primary Failure Reason |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **P1** | PPTX Diagrams | Flowchart with glued connectors (<p:cxnSp>) | ✅ PASS | 13.3 ms | None |
| **P2** | PPTX Grouped Shapes | Text inside nested <p:grpSp> groups (MarkItDown #332) | ✅ PASS | 12.3 ms | None |
| **P3** | PPTX Chart Gaps | Chart with missing data points, must not print literal 'None' (MarkItDown #2526) | ✅ PASS | 11.0 ms | None |
| **D1** | DOCX Tables | Word table with gridSpan merged headers | ✅ PASS | 10.7 ms | None |
| **D3** | DOCX Floating Text Boxes | Floating text box inside mc:AlternateContent (Docling #4569) | ✅ PASS | 13.1 ms | None |
| **D4** | DOCX Equations | Word equations in OMML format compiled to LaTeX | ✅ PASS | 28.6 ms | None |
| **R1** | PDF Layout | Two-column scanned layout without line interleaving | ✅ PASS | 36.1 ms | None |
| **R2** | PDF Math | Heavy mathematical notation and inequalities (CMSY/CMMI) | ❌ FAIL | 203.6 ms | Missing canary: '\le' |
| **SCI_1** | Scientific Paper (arXiv) | Attention Is All You Need: 2-column, Transformer diagram, multi-head attention math | ✅ PASS | 658.3 ms | None |
| **SCI_2** | Scientific Paper (arXiv) | Dense 2-column mathematics and transformer architectures (arXiv:2401.14373) | ✅ PASS | 708.8 ms | None |
| **BOOK_1** | Full Book (300pp) | Thinking in Systems (Meadows, 235pp): flat RAM, chapters, callouts | ✅ PASS | 3774.5 ms | None |
| **BOOK_2** | Full Book (Scan) | Structuring Interviews with Experts (21pp scan): high-throughput Apple Vision OCR | ✅ PASS | 202.4 ms | None |
| **TBL_1** | PDF Tables | Complex ruling-line PDF table with colspans and multi-line cells | ✅ PASS | 34.2 ms | None |
| **WEB_1** | HTML Web Articles | Noisy technical blog: boilerplate and ad purge with code/table preservation | ✅ PASS | 18.8 ms | None |
| **FIN_1** | Corporate Financial 10-K | Berkshire Hathaway Annual Report (140pp): multi-page balance sheets and operating earnings | ✅ PASS | 7117.7 ms | None |
| **FIN_2** | SEC EDGAR iXBRL HTML | Apple Inc. 10-K (iXBRL HTML): preserve dense financial statement tables without article boilerplate stripping | ✅ PASS | 295.9 ms | None |
| **FORM_1** | Government Tax Forms | IRS Form 1040: spatial form grids, label-value bounding boxes, and SSN field recovery | ✅ PASS | 143.6 ms | None |
| **TECH_1** | Hardware Datasheets | ESP32 SoC Datasheet: pinout matrices, peripheral tables, and register memory maps | ✅ PASS | 1912.2 ms | None |
| **SLIDE_1** | Corporate Investor Relations Deck | Tesla Q4 & FY 2023 Update: wide-format presentation slides, financial metric callouts, and vehicle deliveries | ✅ PASS | 503.0 ms | None |
| **SEC_S1** | SEC S-1 Registration | DoorDash S-1/A: non-GAAP reconciliation and financial statement tables | ✅ PASS | 1216.3 ms | None |
| **SEC_CREDIT** | Debt Credit Agreements | Dynatrace Credit Agreement: pricing grid tables and deep nested definitions | ✅ PASS | 252.8 ms | None |
| **SEC_PROXY** | SEC DEF 14A Proxy | Merck DEF 14A Proxy: multi-year executive compensation and footnotes | ✅ PASS | 776.9 ms | None |
| **LEGAL_VIE** | Cross-Border VIE Contracts | VIE Call Option Agreement: multi-party covenants and outlines | ✅ PASS | 28.4 ms | None |
| **SCOTUS_1** | Supreme Court Slip Opinion | SCOTUS Berk v. Choy: 2-column syllabus transition and opinion | ✅ PASS | 456.5 ms | None |
| **PATENT_1** | USPTO Patents | Google Transformer Patent (US 10,452,978): multi-column claims and architecture | ✅ PASS | 736.9 ms | None |
| **CLIN_1** | Clinical Study Protocols | EMR700568-012 MS Study Protocol: Schedule of Assessments and multi-trial criteria | ✅ PASS | 1330.5 ms | None |
| **FDA_1** | FDA Drug Safety Labels | Keytruda FDA Label (SPL): multi-column boxed warnings and adverse reactions | ✅ PASS | 8042.1 ms | None |
| **TI_REG** | Hardware Register Maps | TI LMK3H2104 Register Map: 16-bit register diagrams and bitfield tables | ✅ PASS | 2150.5 ms | None |
| **CSI_SPEC** | CSI MasterFormat Specs | City of Norcross Specs (Division 01): 50-division MasterFormat hierarchy | ✅ PASS | 440.4 ms | None |

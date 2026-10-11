# Filegoblin Adversarial Gauntlet Scorecard

**Overall Score:** 14/14 (100.0%)

| ID | Category | Description | Status | Latency | Primary Failure Reason |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **P1** | PPTX Diagrams | Flowchart with glued connectors (<p:cxnSp>) | ✅ PASS | 11.9 ms | None |
| **P2** | PPTX Grouped Shapes | Text inside nested <p:grpSp> groups (MarkItDown #332) | ✅ PASS | 11.2 ms | None |
| **P3** | PPTX Chart Gaps | Chart with missing data points, must not print literal 'None' (MarkItDown #2526) | ✅ PASS | 11.4 ms | None |
| **D1** | DOCX Tables | Word table with gridSpan merged headers | ✅ PASS | 11.3 ms | None |
| **D3** | DOCX Floating Text Boxes | Floating text box inside mc:AlternateContent (Docling #4569) | ✅ PASS | 11.5 ms | None |
| **D4** | DOCX Equations | Word equations in OMML format compiled to LaTeX | ✅ PASS | 33.8 ms | None |
| **R1** | PDF Layout | Two-column scanned layout without line interleaving | ✅ PASS | 73.0 ms | None |
| **R2** | PDF Math | Heavy mathematical notation and inequalities (CMSY/CMMI) | ✅ PASS | 285.6 ms | None |
| **SCI_1** | Scientific Paper (arXiv) | Attention Is All You Need: 2-column, Transformer diagram, multi-head attention math | ✅ PASS | 888.3 ms | None |
| **SCI_2** | Scientific Paper (arXiv) | Dense 2-column mathematics and transformer architectures (arXiv:2401.14373) | ✅ PASS | 633.5 ms | None |
| **BOOK_1** | Full Book (300pp) | Thinking in Systems (Meadows, 235pp): flat RAM, chapters, callouts | ✅ PASS | 4921.1 ms | None |
| **BOOK_2** | Full Book (Scan) | Structuring Interviews with Experts (21pp scan): high-throughput Apple Vision OCR | ✅ PASS | 481.5 ms | None |
| **TBL_1** | PDF Tables | Complex ruling-line PDF table with colspans and multi-line cells | ✅ PASS | 54.4 ms | None |
| **WEB_1** | HTML Web Articles | Noisy technical blog: boilerplate and ad purge with code/table preservation | ✅ PASS | 23.6 ms | None |

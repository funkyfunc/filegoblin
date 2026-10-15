# Session Handoff: Phase 7 Multi-Format Output Engine & Layout-Aware Semantic Chunking

> **Update 2026-10-14:** the gauntlet now grades quality (0–100) against hand-checked gold in `test_assets/gauntlet/gold/` and checks `--format plain` and `--chunk-size`. Current numbers are in `GAUNTLET_SCORECARD.md`: with `--vision smart`, 29/29 canaries and 87.9 quality; with `--vision off`, 28/29 and 82.1. Open issues are listed in docs/ADR.md under "Headings, EDGAR furniture, layout tables, deterministic scans". The 29/29 figures below were measured before that.

## 1. What Was Just Completed
We successfully designed, implemented, and verified the native Multi-Format Output Engine and Layout-Aware Semantic Chunking architecture for RAG and LLM agent pipelines in `filegoblin`:

### A. Unified Output Format Flag (`--format <FORMAT>` / `-f`)
- Added `OutputFormat` enum: `Markdown` (default), `Plain`, and `Json`.
- Preserved full backwards compatibility with legacy `--json`.
- Implemented `crates/core/src/formatters/plain.rs` (`markdown_to_plain`):
  - Formats GFM pipe tables into clean space-aligned text columns.
  - Strips heading `#` prefixes while preserving heading text.
  - Strips link markdown `[text](url)` to plain anchor text `text`.
  - Strips inline (`$ ... $`) and display (`$$ ... $$`) LaTeX math delimiters.
  - Unwraps fenced code blocks while preserving code lines.
  - Preserves `<!-- Context: ... -->` outline breadcrumbs.

### B. Layout-Aware Semantic Chunking Engine (`crates/core/src/chunking/mod.rs`)
- Added CLI parameters:
  - `--chunk-size <TOKENS>`: parses bare numbers (`512`, `1024`) or metric units (`1k`, `2m`).
  - `--chunk-overlap <TOKENS>`: overlap token budget between narrative blocks (default: 50).
  - `--chunk-strategy <layout|tokens>`: defaults to `layout` (structural boundaries), with fallback `tokens` (hard sliding window).
- **Hierarchical Outline Breadcrumbs:**
  - `OutlineTracker` tracks document heading depth (H1 > H2 > H3).
  - Emits `<!-- Context: Section > Subsection -->` in markdown and plain formats, or populates `"breadcrumbs": [...]` in JSON schema.
- **Table Context Preservation (Mandatory):**
  - When multi-row tables exceed `--chunk-size` and split across chunks, the chunker automatically replicates the table's header row and separator line at the start of every continuation chunk.
  - No chunk ever contains orphan data rows without column headers.
- **Structural Integrity:**
  - Protects code blocks and short tables from mid-block cuts.
  - Seeds narrative continuation chunks with `--chunk-overlap` tokens from preceding prose.

### C. Structured JSON Output Schema
- When combining `--format json` with `--chunk-size`, emits a valid JSON array of chunk objects:
  ```json
  [
    {
      "file": "path/to/document.pdf",
      "chunk_index": 0,
      "total_chunks": 4,
      "breadcrumbs": ["Section 1", "Subsection A"],
      "content": "Chunk body content...",
      "tokens": 420
    }
  ]
  ```

---

## 2. Quantitative Fidelity Benchmark Scorecard

| Slice | Metric / Target Gate | Baseline State | Final Result | Status |
| :--- | :--- | :--- | :--- | :--- |
| **Slice A (Two-Column)** | H/F Leaks = 0 | 0 | **0** | ✅ PASS |
| | Sentence Interruptions = 0 | 0 | **0** | ✅ PASS |
| **Slice B (Tables & Charts)** | Recall $\ge 85.0\%$ / Precision $\ge 85.0\%$ | 86.9% / 87.7% | **86.9% / 87.7%** | ✅ PASS |
| | H/F Leaks = 0, Interruptions = 0 | 0 / 0 | **0 / 0** | ✅ PASS |
| **Slice C (Callouts & Headers)** | Recall $\ge 85.0\%$ / Precision $\ge 90.0\%$ | 88.5% / 93.3% | **88.5% / 93.3%** | ✅ PASS |
| | H/F Leaks = 0, Interruptions = 0 | 0 / 0 | **0 / 0** | ✅ PASS |
| **Slice D (Unseen Frameworks)** | Recall $\ge 85.0\%$ / Precision $\ge 85.0\%$ | 88.7% / 89.6% | **88.7% / 89.6%** | ✅ PASS |
| | H/F Leaks = 0, Interruptions = 0 | 0 / 0 | **0 / 0** | ✅ PASS |
| **Slice E (Zeitz Math)** | Recall $\ge 85.0\%$ / Precision $\ge 85.0\%$ | 85.3% / 85.8% | **85.3% / 85.8%** | ✅ PASS |
| | H/F Leaks = 0, Interruptions = 0 | 0 / 0 | **0 / 0** | ✅ PASS |
| **Attention Is All You Need** | Table Integrity (6/6 valid) | 6/6 | **6/6 (100.0%)** | ✅ PASS |
| | H/F Leaks = 0, Interruptions = 0 | 0 / 0 | **0 / 0** | ✅ PASS |

---

## 3. Adversarial Enterprise Gauntlet Scorecard (29/29 Passing - 100.0%)

| ID | Category | Description | Status | Latency |
| :--- | :--- | :--- | :--- | :--- |
| **P1** | PPTX Diagrams | Flowchart with glued connectors (`<p:cxnSp>`) | ✅ PASS | 12.5 ms |
| **P2** | PPTX Grouped Shapes | Text inside nested `<p:grpSp>` groups (MarkItDown #332) | ✅ PASS | 11.0 ms |
| **P3** | PPTX Chart Gaps | Chart with missing data points, must not print literal 'None' | ✅ PASS | 12.1 ms |
| **D1** | DOCX Tables | Word table with `gridSpan` merged headers | ✅ PASS | 11.0 ms |
| **D3** | DOCX Floating Text Boxes | Floating text box inside `mc:AlternateContent` (Docling #4569) | ✅ PASS | 12.6 ms |
| **D4** | DOCX Equations | Word equations in OMML format compiled to LaTeX | ✅ PASS | 36.7 ms |
| **R1** | PDF Layout | Two-column scanned layout without line interleaving | ✅ PASS | 104.8 ms |
| **R2** | PDF Math | Heavy mathematical notation and inequalities (CMSY/CMMI) | ✅ PASS | 340.9 ms |
| **SCI_1** | Scientific Paper (arXiv) | Attention Is All You Need: 2-column, Transformer diagram, multi-head attention math | ✅ PASS | 905.2 ms |
| **SCI_2** | Scientific Paper (arXiv) | Dense 2-column mathematics and transformer architectures (arXiv:2401.14373) | ✅ PASS | 867.6 ms |
| **BOOK_1** | Full Book (300pp) | Thinking in Systems (Meadows, 235pp): flat RAM, chapters, callouts | ✅ PASS | 6126.3 ms |
| **BOOK_2** | Full Book (Scan) | Structuring Interviews with Experts (21pp scan): high-throughput Apple Vision OCR | ✅ PASS | 701.6 ms |
| **TBL_1** | PDF Tables | Complex ruling-line PDF table with colspans and multi-line cells | ✅ PASS | 64.4 ms |
| **WEB_1** | HTML Web Articles | Noisy technical blog: boilerplate and ad purge with code/table preservation | ✅ PASS | 21.1 ms |
| **FIN_1** | Corporate Financial 10-K | Berkshire Hathaway Annual Report (140pp): multi-page balance sheets and operating earnings | ✅ PASS | 14658.2 ms |
| **FIN_2** | SEC EDGAR iXBRL HTML | Apple Inc. 10-K (iXBRL HTML): preserve dense financial statement tables | ✅ PASS | 998.8 ms |
| **FORM_1** | Government Tax Forms | IRS Form 1040: spatial form grids, label-value bounding boxes, and SSN field recovery | ✅ PASS | 525.1 ms |
| **TECH_1** | Hardware Datasheets | ESP32 SoC Datasheet: pinout matrices, peripheral tables, and register memory maps | ✅ PASS | 4795.1 ms |
| **SLIDE_1** | Corporate IR Deck | Tesla Q4 & FY 2023 Update: wide-format presentation slides, financial metric callouts | ✅ PASS | 1521.2 ms |
| **SEC_S1** | SEC S-1 Registration | DoorDash S-1/A: non-GAAP reconciliation and financial statement tables | ✅ PASS | 4140.0 ms |
| **SEC_CREDIT** | Debt Credit Agreements | Dynatrace Credit Agreement: pricing grid tables and deep nested definitions | ✅ PASS | 683.8 ms |
| **SEC_PROXY** | SEC DEF 14A Proxy | Merck DEF 14A Proxy: multi-year executive compensation and footnotes | ✅ PASS | 2440.7 ms |
| **LEGAL_VIE** | Cross-Border VIE Contracts | VIE Call Option Agreement: multi-party covenants and outlines | ✅ PASS | 69.0 ms |
| **SCOTUS_1** | Supreme Court Slip Opinion | SCOTUS Berk v. Choy: 2-column syllabus transition and opinion | ✅ PASS | 592.7 ms |
| **PATENT_1** | USPTO Patents | Google Transformer Patent (US 10,452,978): multi-column claims and architecture | ✅ PASS | 822.7 ms |
| **CLIN_1** | Clinical Study Protocols | EMR700568-012 MS Study Protocol: Schedule of Assessments and multi-trial criteria | ✅ PASS | 2750.0 ms |
| **FDA_1** | FDA Drug Safety Labels | Keytruda FDA Label (SPL): multi-column boxed warnings and adverse reactions | ✅ PASS | 24467.7 ms |
| **TI_REG** | Hardware Register Maps | TI LMK3H2104 Register Map: 16-bit register diagrams and bitfield tables | ✅ PASS | 8874.8 ms |
| **CSI_SPEC** | CSI MasterFormat Specs | City of Norcross Specs (Division 01): 50-division MasterFormat hierarchy | ✅ PASS | 557.0 ms |

---

## 4. Broken Tests or Pending Bugs
- **Zero broken tests:** All 240 workspace unit and integration tests (`cargo test --workspace`), 21 golden snapshot tests (`cargo test -p filegoblin --test golden`), 5 format & semantic chunking tests (`cargo test -p filegoblin --test format_and_chunking`), and 29/29 Adversarial Enterprise Gauntlet test cases pass with 0 errors.
- **Zero warnings:** `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` pass with 0 warnings.
- **Zero leaks:** Zero headers, footers, folios, or sentence interruptions across all slices and full books.
- **The Generalization Firewall intact:** Zero document-specific prose, titles, or numbers hardcoded in `crates/core/src/parsers/pdf.rs`.

---

## 5. Immediate Next Steps
- Prepare release packaging and multi-channel distribution (PyPI wheels, Crates.io publication, Homebrew formula).

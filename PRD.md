# Project filegoblin: Technical Product Requirements Document (PRD) v1.5

**Version:** 1.5.0 (The Comprehensive Hoard)
**Status:** Ready for Development
**Target Stack:** Rust (Stable)

---

## 1. Product Overview
### 1.1 Objective
**filegoblin** (`fg`) is a zero-dependency, high-performance CLI tool designed to "gobble" heterogeneous file formats (PDF, DOCX, XLSX, HTML, Source Code) and convert them into high-fidelity, LLM-optimized Markdown/XML/YAML. 

### 1.2 Core Value Proposition
- **High-Signal Ingestion:** Uses specific structural anchors to prevent "Attention Drift" in long-context LLMs.
- **Zero-Dependency:** Single, portable Rust binary with no system-level dependencies.
- **Privacy-First:** Local-only PII/Secret scrubbing using a hybrid Regex + SLM engine.

---

## 2. Technical Architecture & Stack
### 2.1 The "Stomach" (Core Ingestion)
- **PDF Engine:** `oxidize-pdf` (Must support XRef rebuilding and table style preservation).
- **Office Engine:** `docx-lite` (Word) and `ooxml` (Excel) using streaming XML parsing for low memory overhead (<5MB).
- **Web Engine:** Heuristic-based HTML cleaner. Must prioritize `<article>` and `<main>` tags and support the 2026 `/llms.txt` standard.
- **Code Engine:** `tree-sitter` for AST-based signature extraction.

### 2.2 OCR & Extensibility
- **OCR:** `tesseract-wasm` executed via `wasmtime` to avoid C-library dependencies.
- **Plugin System:** Implementation of the **WASM Component Model** (via `wasmtime`) to allow for third-party sandboxed `.wasm` parsers.

### 2.3 Privacy & Security
- **Local SLM:** ONNX Runtime executing a quantized **Distil-PII-1B** model.
- **Regex:** Pre-configured patterns for API Keys, JWTs, and structured PII (SSNs, Phones).

---

## 3. Data Specification & Output Flavors (The Answer Key)

### 3.1 Model-Specific Structuring (MANDATORY TEMPLATES)
The following structures must be strictly adhered to when the `--flavor` flag is used:

**Flavor: `human` (Standard Markdown)**
- **Requirement:** Plain, readable markdown without LLM-specific framing or metadata anchors. Useful for human review.

**Flavor: `anthropic` (Claude 3.5/4.5 - XML Anchored)**

```xml
<context_stream>
  <context_metadata>
    <project_name>filegoblin</project_name>
    <total_tokens>1250</total_tokens>
  </context_metadata>
  <file path="src/main.rs" type="rust">
    <content>
    fn main() { println!("Hello Goblin!"); }
    </content>
  </file>
</context_stream>
```

**Flavor: `gpt` (OpenAI o1/o3 - Reasoning-First)**
```markdown
---
file: src/main.rs
tokens: 42
mode: full
---
```rust
fn main() { println!("Hello Goblin!"); }
```
```

**Flavor: `gemini` (Gemini 1.5/3 Pro - Hierarchical)**
- **Requirement:** Must include a Global ASCII Map at the start and File Boundary Breadcrumbs (e.g., `// --- FILE_START: src/lib.rs ---`) to assist internal indexing.

### 3.2 Data Normalization: Sequence of Records
All wide-spatial tables (XLSX/PDF) MUST be normalized to a repeatable key-value record format to prevent hallucination.
**Example Output:**
> `Row 1: Date: 2026-01-01; Revenue: $10M; Growth: +5%;`
> `Row 2: Date: 2026-02-01; Revenue: $12M; Growth: +20%;`

### 3.3 Structural Minification (`--skeleton`)

Using Tree-sitter to preserve imports, signatures, and docstrings while eliding method bodies.
**Example Output:**
```rust
pub struct GoblinStomach { ... }

impl GoblinStomach {
    /// Consumes a file and returns markdown.
    pub fn gobble(&self, path: &Path) -> Result<String> { /* body elided */ }
}
```

---

## 4. Functional Requirements & CLI Flags
### 4.1 Ingestion & Processing
- **`fg <path>`**: Single file ingestion.
- **`fg <url>`**: URL ingestion (fetches web page/file into memory for processing).
- **`fg . --horde`**: Recursive directory ingestion with a top-level ASCII map.
- **`--split`**: Modifier for `--horde`. Suppresses stdout, auto-generates a `target_gobbled` directory, and splits output into discrete mapping files.
- **`-q, --quiet`**: Suppress auxiliary terminal output (progress/banners) for pristine pipeline execution.
- **`--json`**: Convert the output format from Markdown into strictly formatted structured JSON arrays.
- **`--scrub`**: Local PII/Secret redaction.
- **`--tokens`**: Print estimated token counts for the specific model flavor.
- **`--compress <safe|contextual|aggressive>`**: Token compression engine for reducing LLM overhead.

### 4.2 UX Utilities
- **`--copy`**: Headless "Direct-to-Clipboard" support for Wayland, X11, macOS, and Windows.
- **`--open`**: Automatically trigger the OS native file explorer (Finder/Explorer) to show the output.
- **`--watch`**: Monitor directory and re-gobble/re-copy on every file save.
- **`-i` (Interactive)**: Launch the `ratatui` TUI dashboard for file selection.

---

## 5. System Integration & Performance
### 5.1 Performance Targets
- **Parsing:** < 2s for 50-page business PDF.
- **Redaction:** < 100ms per text page.
- **Binary Size:** < 20MB (statically linked).

### 5.2 Concurrency
- Parallelize directory crawling and multi-file parsing using the `rayon` crate.
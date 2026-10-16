# (o_o) filegoblin

> **"Ingesting the messy world, spitting out clean context."**

[![npm version](https://img.shields.io/npm/v/filegoblin.svg)](https://www.npmjs.com/package/filegoblin)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![GitHub](https://img.shields.io/badge/GitHub-funkyfunc%2Ffilegoblin-blue.svg)](https://github.com/funkyfunc/filegoblin)

**filegoblin** is a high-performance CLI that converts almost any file or website — PDF, Word, Excel, PowerPoint, audio, subtitles, email, EPUB, HTML, or entire git repositories — into clean, structured Markdown ready for Large Language Models (LLMs) and context windows.

---

## 🚀 Quick Start with `npx`

Zero installation required. If you have Node.js installed, you can run `filegoblin` immediately:

```bash
# Convert a PDF to clean markdown
npx filegoblin document.pdf > context.md

# Convert Word documents with full LaTeX math equations
npx filegoblin paper.docx > paper.md

# Extract PowerPoint slides, embedded data charts, and diagrams
npx filegoblin deck.pptx > deck.md

# Convert any web page or article cleanly (strips ads & link-farms)
npx filegoblin https://example.com/article > article.md

# Pipe directly into clipboard or downstream CLI tools
npx filegoblin report.pdf -q | pbcopy
```

---

## 📦 Global Installation

To use `filegoblin` directly without the `npx` prefix:

```bash
npm install -g filegoblin                     # converter
npm install -g filegoblin @filegoblin/models   # plus the PDF vision models (~510 MB)
```

Now `filegoblin` is available anywhere in your terminal:

```bash
filegoblin document.pdf > context.md
```

**PDF vision models.** `@filegoblin/models` adds:
- layout and reading order;
- formulas written out as LaTeX;
- table structure;
- OCR for scanned pages.

All of them are models bundled with filegoblin that run locally. `--vision smart`, the default, uses them only on the pages that need them. The weights come from the npm registry, split into packages under 100 MB, so they install through a company npm mirror too. Without them, filegoblin uses its fast converter, plus Apple Vision OCR on macOS. You can add them later with `filegoblin --install-models` (downloads from the GitHub release) and check what it finds with `filegoblin --models-status`.

*(Precompiled binaries are fetched for macOS Apple Silicon/Intel, Linux x86_64/arm64, and Windows x64. On other platforms, install with `cargo install --git https://github.com/funkyfunc/filegoblin filegoblin`.)*

---

## ⚡ What Makes filegoblin Different?

### 1. Deterministic Structural Deconstruction (Zero Hallucination)
- **Embedded Office Charts $\to$ Data Tables & Mermaid `xychart-beta`:** OpenXML DrawingML charts (`c:chartSpace`) in `.pptx` and `.docx` are parsed directly from raw numeric caches into standard GitHub Flavored Markdown pipe tables and native Mermaid charts.
- **DrawingML Connectors & SmartArt $\to$ Mermaid Flowcharts:** Resolves process flows and shape connection topologies into `flowchart TD` / `flowchart LR` graphs.
- **Word OMML $\to$ KaTeX LaTeX Math:** Office Math Markup Language (`<m:oMath>`) is compiled recursively into LaTeX math (`$ ... $` and `$$ ... $$`), covering fractions, radicals, n-ary operators, delimiters, and matrices.
- **PDF TeX Font Recovery:** Maps Computer Modern and AMS Math font encodings (`CMSY*`, `CMMI*`, `MSAM*`, `MSBM*`) back to true Unicode mathematical symbols and repairs legacy publisher ASCII substitutions (`=I-` $\to$ `\neq`, `::;` $\to$ `\le`, `2':` $\to$ `\ge`).

### 2. High-Fidelity PDF Engine
- **Layout Ordering & Multi-Column Continuity:** Two-column page partitioning preserves full-width banner titles before reading column 1 then column 2. Eliminates line interleaving.
- **Running Header & Footer Suppression:** Strips page numbers (isolated folios), alternating running headers, and publisher slugs across page boundaries so prose paragraphs flow uninterrupted.
- **Parallel Scanned OCR:** Built-in hardware-accelerated Apple Vision OCR on macOS (and neural OCR on Linux/Windows) converts scanned books at 30+ pages/second with zero external cloud API dependencies.

### 3. One Consistent Markdown Dialect
- Every tabular source (Excel, CSV, Word, PowerPoint, OpenDocument, PDF) emits identical, well-formed Markdown pipe tables with aligned separator rows.
- Clear file delimiter boundaries (`// --- FILE_START: <path> ---`) and optional semantic chapter markers (`<!-- filegoblin:section ... -->`).

### 4. Built-in MCP Server (Model Context Protocol)
Connect `filegoblin` directly to Claude Desktop or Claude Code with a single flag:

```bash
# Add to Claude Code
claude mcp add filegoblin -- npx -y filegoblin --mcp
```

Or configure Claude Desktop (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "filegoblin": {
      "command": "npx",
      "args": ["-y", "filegoblin", "--mcp"]
    }
  }
}
```

---

## 🛠 Supported Formats

| Category | Formats | What filegoblin Extracts |
| :--- | :--- | :--- |
| **Documents** | `.pdf` | Multi-column reading order, headings, tables, soft-hyphen joining, TeX math symbols, OCR |
| **Word** | `.docx` | Headings, lists, tables, links, images, and OMML equations converted to LaTeX |
| **Presentations** | `.pptx`, `.odp` | Slide order, speaker notes, embedded data charts to tables + Mermaid `xychart-beta`, connectors to Mermaid flowcharts |
| **Spreadsheets** | `.xlsx`, `.csv`, `.tsv` | Standard GFM Markdown pipe tables with normalized date/float formatting |
| **Web** | URLs, `.html` | DOM readability extraction, link-farm pruning, SPA rendering via `--browser`, recursive crawling via `--horde` |
| **OpenDocument** | `.odt` | Headings, lists, pipe tables, footnotes emitted after citing paragraphs |
| **E-Books & Text** | `.epub`, `.rtf`, `.txt` | Chapters, structured typography, clean metadata |
| **Code & Data** | `.ipynb`, `.json`, repos | Jupyter Notebook cells (code + markdown + text outputs), tree-sitter symbol extraction (`--extract symbols`), git diffs (`--git-diff`) |
| **Media & Audio** | `.srt`, `.vtt`, `.mp3`, `.wav` | Subtitle cue deduplication, local whisper audio transcription |

---

## 🛡 Privacy & Token Optimization

- **Local PII & Secret Scrubbing (`--scrub`):** Redacts API keys, credentials, JWTs, emails, and sensitive patterns locally before passing text to an LLM.
- **Token Estimation & Compression (`--tokens`, `--compress`):** Measure exact token footprints and compress whitespace and repetitive structure safely.
- **Fail-Loud Contract:** Exits with code `4` if nothing could be extracted, so automated agent loops never hallucinate on empty context.

---

## 📖 Links & Documentation

- **GitHub Repository:** [https://github.com/funkyfunc/filegoblin](https://github.com/funkyfunc/filegoblin)
- **Issue Tracker:** [https://github.com/funkyfunc/filegoblin/issues](https://github.com/funkyfunc/filegoblin/issues)
- **Changelog & Architectural Decisions:** See [docs/ADR.md](https://github.com/funkyfunc/filegoblin/blob/main/docs/ADR.md) on GitHub.

---

## 📄 License

MIT © [filegoblin contributors](https://github.com/funkyfunc/filegoblin)

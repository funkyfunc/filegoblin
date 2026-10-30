# filegoblin

Converts PDFs (including scans and math), Word, PowerPoint, Excel, HTML and web pages, EPUB, email, notebooks, subtitles and audio into clean Markdown for LLMs. It's a single native binary with no Python dependencies. This package installs it and puts `filegoblin` on your PATH.

```bash
pip install filegoblin             # the converter
filegoblin --install-models        # optional: the PDF vision models (~510 MB)
```

The PDF vision models are:
- PP-DocLayoutV3 for layout and reading order
- pix2text MFR for formulas, written out as LaTeX
- Table Transformer for tables
- PP-OCRv6 for OCR on scanned pages

`--vision smart`, the default, sends the pages that need them through the models: scans, math, tables, columns and figures. Without the models, filegoblin uses its fast geometric converter, plus Apple Vision OCR on macOS.

`filegoblin --install-models` downloads them from the GitHub release and checks their SHA-256. They are not on PyPI yet. `filegoblin --models-status` shows which models it finds.

```bash
filegoblin report.pdf > report.md
filegoblin https://example.com/article
filegoblin --mcp        # MCP server for Claude and other agents
```

Documentation: https://github.com/funkyfunc/filegoblin

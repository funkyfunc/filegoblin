# Current Session Tasks (Completed)

- `[x]` Geometric PDF table extraction via `oxidize-pdf` bounding box heuristics.
- `[x]` `.docx` table extraction recursively resolving into "Sequence of Records".
- `[x]` Native `.ipynb` Jupyter parser built on `serde_json`, capturing output streams natively.
- `[x]` OpenXML state machine upgrade for `.pptx` table extraction.
- `[x]` `ArchiveGobbler`: In-memory extraction for `.zip`, `.tar.gz`, and `.tgz` routed through temp-directories.
- `[x]` `EpubGobbler`: E-book HTML flattening and Markdown formatting via zip extraction.
- `[x]` Version bumped to `1.10.0` in all `Cargo.toml` files.

# Pending Tasks (Next Session)

- `[ ]` Dependency Graph "PageRank" Heuristic Pruning (using Tree-sitter).
- `[ ]` Git Context & Diffs parsing (`--diff` flag or `.patch` ingestion).
- `[ ]` Database Schema Extraction (`.sqlite`, `.db`).

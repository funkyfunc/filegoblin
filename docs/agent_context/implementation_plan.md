# filegoblin Implementation Plan: Actual Parser Integration (Phase II.b)

The user has approved replacing the mock parsers in `src/parsers/` with actual "plumbing." This step will officially transition the CLI into a functioning ingestion tool by hooking up the engines.

## Proposed Changes

### 1. Central Routing (The Brain)
Presently, `src/lib.rs` just fetches URLs or mocks local files and passes them directly to the Output Flavors engine. It does not actually route files to the parsers!
#### [MODIFY] src/lib.rs
- Create a router function `route_and_gobble(path: &str, raw_content: Option<String>) -> Result<String>`.
- If the path ends in `.pdf`, instantiate `PdfGobbler` and call its `gobble` method.
- If it ends in `.docx` or `.xlsx`, instantiate `OfficeGobbler`.
- If it's `http://` or `.html`, instantiate `WebGobbler` (passing the pre-fetched text).
- If it's a code file (`.rs`, etc.), instantiate `CodeGobbler`.
- Update `gobble_app` to call this router instead of returning the raw text directly.

### 2. PDF Engine
#### [MODIFY] src/parsers/pdf.rs
- Implement `PdfGobbler`. We will utilize `oxidize-pdf` (the dependency we added in Phase II) to load the document at `path`, extract the text from each page, and apply rudimentary spatial heuristics to detect tables and enforce the "Sequence of Records" structure per PRD 3.2.

### 3. Office Docs Engine
#### [MODIFY] src/parsers/office.rs
- Implement `OfficeGobbler`. Use `docx-rs` and `zip`/XML tools if necessary to unpack the open xml structures. We will extract plaintext runs from `<w:t>` tags in Word documents to return raw text.

### 4. Web Engine
#### [MODIFY] src/parsers/web.rs
- Implement `WebGobbler`. The gobbler will take the raw fetched HTML string, identify `<article>` or `<main>` tags (using basic string manipulation/regex for zero-dependency purity where possible), and strip out `<script>`, `<style>`, and `<nav>` blocks to provide LLM-optimized high-signal text.

### 5. Code Structural Minification
#### [MODIFY] Cargo.toml
- Add the `tree-sitter-rust` language dependency so the `tree-sitter` engine has a grammar to parse.
#### [MODIFY] src/parsers/code.rs
- Implement `CodeGobbler`. Load the `tree-sitter` parser with the Rust language grammar. Traverse the AST to identify `function_item` nodes. Keep the signature and docstrings, but replace the `block` node with `/* body elided */` as per PRD 3.3.

## Verification Plan
1. **Routing Test**: Test that `filegoblin` correctly routes a `.pdf` file to the `PdfGobbler`.
2. **Engine Tests**: Update and verify the specific unit tests for each parser to ensure the actual implementations satisfy the established TDD mocks.
3. **Purity Test**: `cargo horde-check` to confirm the tree-sitter C bindings and other logic compile safely.

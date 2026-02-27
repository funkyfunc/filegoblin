# filegoblin Architecture Blueprint

This document details the core architectural pillars and technical decisions that guide the development of `filegoblin`. It serves as a historical record of *why* the system is built the way it is, ensuring that future contributions align with the project's foundational constraints: extreme portability, high-fidelity extraction, and strict privacy.

---

## 1. The Pure-Language Mandate (Rust)

**Decision:** `filegoblin` is implemented entirely in Rust (Stable), strictly avoiding C-bindings, dynamic system libraries (`.so`, `.dll`), and runtime environments (like Python or Node.js).

**Justification:**
- **Zero-Dependency Portability:** The tool is designed for locked-down corporate environments where users cannot run `apt install` or `pip install`. Rust compiles to a statically linked, standalone binary that can be simply copied and executed.
- **Binary Size vs. Go:** While Go also produces static binaries, its 20MB+ runtime (garbage collector, scheduler) bloats the executable. Rust allows us to package a full PDF/HTML/Office parser into a deeply optimized binary often half the size.
- **Deterministic Memory:** Traversing complex, malformed files (like 1,000-page broken PDFs) in "Horde Mode" requires granular memory control to avoid the sudden latency spikes and RAM bloat associated with garbage-collected languages.

## 2. Platform-Native Hooks & Extensibility

**Decision:** Heavy logic (like Optical Character Recognition) that traditionally require fragile system-level C libraries (e.g., Leptonica, Tesseract) are managed via a **Hybrid Architecture** prioritizing OS-Native Frameworks when universally available. Third-party extensibility utilizes WebAssembly (WASM).

**Justification:**
- **The Tesseract Problem:** Traditional OCR breaks the zero-dependency rule. We pivoted away from bloated C++ libraries and sluggish WASM equivalents to a native approach:
  - **macOS:** Pure-Rust `objc2` bindings connect directly into Apple's native Vision Framework. This harnesses the hardware Neural Engine for instantaneous (<50ms) zero-dependency OCR.
  - **Linux/Windows:** A pure-Rust fallback leverages the `ocrs` crate and `rten` (Rust Tensor) inference engine, executing locally purely via CPU math without external installations.
- **WASM Sandboxing:** For custom user parsers, the WASM Component Model (via WebAssembly Interface Types or WIT) allows third parties to write tools in any language that run in a highly restricted sandbox, with zero unauthorized access to the host machine.

## 3. LLM-Native Formatting Engine

**Decision:** The engine is not designed to produce "human-readable" outputs as a primary goal. All text extraction is optimized strictly for modern LLM Context Windows (100k - 2M tokens).

**Justification:**
- **Structural Minification:** Source code is skeletonized using `tree-sitter`. LLMs generally only need to understand signatures, types, and docstrings to map an architecture. Stripping function bodies (via the pure C11 runtime of tree-sitter) saves up to 70% of tokens while retaining 90% of contextual utility.
- **Sequence of Records (Tabular Data):** LLMs struggle with 2D spatial awareness for wide Excel/Word tables. `filegoblin` flattens tables into sequential key-value strings (`Row 1: Col A: Val; Col B: Val;`) to enforce pattern matching and dense positional embeddings.
- **Hierarchical Anchoring:** Multi-file repositories ("horde mode") are prepended with an ASCII Project Map (`tree` style), and every file boundary is strictly tagged (e.g., XML `<file path="...">` or Markdown breadcrumbs) to prevent "Attention Drifting" in the middle of massive context payloads.

## 4. Local-First Privacy Shield

**Decision:** All Data Loss Prevention (DLP), PII redaction, and secret scanning are executed 100% locally. The CLI will never phone home or utilize an external API for content scrubbing.

**Justification:**
- **Enterprise Air-Gaps:** The tool is intended for use in environments where proprietary source code cannot be sent to third-party endpoints.
- **Hybrid PII Engine:** Fast, deterministic secrets (API keys, SSNs) are caught via high-speed Regex. Contextual "soft" PII (names, organizations) is identified locally by running a quantized Small Language Model (SLM) like `Distil-PII` using the ONNX Runtime directly on the host machine.

## 5. Cross-Platform Ergonomics (Wayland Clipboard)

**Decision:** The `--copy` functionality must interact directly with the system clipboard protocol, bypassing legacy wrappers or execution calls (`xclip`).

**Justification:**
- **The Linux Transition:** Major distributions (Ubuntu, RHEL) have deprecated X11. `filegoblin` utilizes `wl-clipboard-rs` to implement the `wl-data-device` protocol natively in pure Rust. This ensures the tool can silently and correctly pipe millions of tokens into the clipboard on modern Wayland desktop environments without requiring the user to install `libwayland-client` shared objects.

## 6. Unix Philosophy & Pipeline Integration

**Decision:** The CLI must absolutely respect the sanctity of standard streams. Diagnostic outputs (ASCII mascots, proxy warnings, progress logs) MUST go to `stderr`. Only perfectly formatted, target-file content may go to `stdout`.

**Justification:**
- **The Piped Ecosystem:** `filegoblin` is not an isolated GUI; it's a pipe component meant to be combined with `jq`, `grep`, or direct `curl` payloads to LLM APIs.
- **Data Corruption:** Emitting a single byte of "Hello Goblin" or "Fetching URL..." into `stdout` instantly invalidates the resulting JSON array or Markdown stream for programmatic consumers.
- **Zero-Config Quiet:** The addition of the `-q, --quiet` flag instantly silences `stderr` entirely, guaranteeing a completely silent operational footprint when orchestrated by CI/CD pipelines or background bash scripts.

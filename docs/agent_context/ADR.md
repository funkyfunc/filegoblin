# Architecture Decision Records (ADR)

*A lightweight log of design decisions made during the development of filegoblin to capture intent, without being overly burdensome.*

---

## 2026-02-22: Stripping Web Links by Default
**Context:** When parsing HTML via `WebGobbler`, the extracted `href` attributes create significant context bloat and noise for LLM models. For human readers (e.g., using `--flavor human` or `--full`), the links are equally useless since the output is no longer an interactive website but static Markdown.
**Decision:** All HTML links `<a>` will be aggressively flattened into plain text (e.g., `[Link Text](path)` becomes simply `Link Text`) before the Markdown conversion step, regardless of the `--full` or flavor flags.
**Status:** Accepted

## 2026-02-22: System Proxy Ingestion
**Context:** The tool will be used in corporate environments where outbound network traffic must flow through HTTP/HTTPS proxies. For safety and predictability, we must ensure these are respected.
**Decision:** `reqwest::blocking::Client` inherently reads standard environment variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`) by default. We will simply rely on this robust behavior without bloating the codebase with manual proxy logic, but will add explicit `println!` logging into `route_and_gobble`'s URL fetcher so users have immediate visibility that their corporate proxy is being actively used.
**Status:** Accepted

## 2026-02-22: Extracting System Blueprint to ARCHITECTURE.md
**Context:** The founding research docs contain massive amounts of deep technical justifications (Rust vs Go, WASM OCR over Tesseract, Wayland Clipboard protocols). `PRD.md` is meant for *what* the tool does, and `README.md` is for *how* to use it, leaving the *why* undocumented.
**Decision:** Created a new root-level file `ARCHITECTURE.md` to permanently capture the 5 core pillars of the `filegoblin` system based on the initial R&D specs.
**Status:** Accepted

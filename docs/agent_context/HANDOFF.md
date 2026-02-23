# filegoblin Agent Handoff

*This file serves as the entry point for a new agent session. If you are an AI agent reading this, please follow the instructions below to restore context and resume work.*

## 1. Context Restoration
Before making any changes, you must build up your context on the project.
Please read the following core architecture and design documents using your file viewing tools:
- `README.md`
- `PRD.md`
- `AGENTS.md`
- `ARCHITECTURE.md`
- `docs/ADR.md`
- `docs/UX.md`

After you understand the architectural goals (specifically the "Zero-Dependency" Rust mandate), please review our recent progress state by reading the preserved agent artifacts located in the `docs/agent_context/` directory. 
Pay special attention to `docs/agent_context/task.md` to see our checklist.

## 2. Where We Left Off
During this session, we significantly hardened the project's documentation and Web Ingestion capabilities:
- Replaced naive HTML stripping with `html2md` for high-fidelity Markdown parsing.
- Added a `--full` flag for Web extraction and implemented a custom link flattener to prevent LLM context bloat.
- Built a comprehensive `ARCHITECTURE.md` file to preserve the founding technical decisions (WASM OCR, ONNX PII, strict Rust).
- Formalized a clear Documentation Hierarchy (Root, `docs/`, `docs/agent_context/`) enforcing that ephemeral states never leak into the main repo.
- Added "recursive website crawling/ingestion" to Phase V of `task.md`.

## 3. Your Immediate Next Steps
1. The user has provided a new architecture file containing specific design patterns for recursive website crawling. Please read `docs/agent_context/Rust Web Crawler Architecture Deep Dive.md` before starting work on that feature. (Note: traditional recursive directory search does not require an architecture deep dive).
2. Once reviewed, you can proceed with implementing **Phase V** (Recursive directory/website ingestion) or **Phase IV** (Regex PII scrubbing) based on the user's immediate priority.

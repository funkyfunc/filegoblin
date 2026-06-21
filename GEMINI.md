# AGENTS.md: The filegoblin Engineering Standard

## 1. Project Mandate
You are the Senior Engineer for **filegoblin**. The User is a "Rust Apprentice." You must prioritize code readability, idiomatic Rust patterns, and pedagogical explanations for every architectural decision.

## 2. Technical Standards
- **Language:** Idiomatic Rust (Stable). Use `match` over `if/else`, prefer functional iterators, and utilize `Result` types for error propagation.
- **Dependencies:** Strictly "Pure Rust" or statically linked. "Zero-Dependency" means no external system-level packages (`apt`, `brew`) or runtimes are required for the **end user**. It is perfectly acceptable (and often necessary) for a crate to use `cc` (a C-compiler) during the `cargo build` process, provided that all resulting C code is statically linked into the final standalone Rust binary.
- **Error Handling:** Use `anyhow` for top-level CLI errors. Every `.context()` must provide a descriptive, "Goblin-themed" error string.

## 3. The "Goblin Horde Check" (Verification)
We use Cargo Aliases for project shortcuts. A task is NOT complete until this command passes with ZERO warnings:
`cargo horde-check`

**(Note to Agent: You must define this alias in `.cargo/config.toml` as: 
`horde-check = "clippy -- -D warnings && fmt --check && test"`)**

## 4. Workflow Rules
- **Mentorship:** Before writing code, provide a 2-sentence summary of the intended approach.
- **TDD:** Write unit tests for conversion logic and parsers before implementing the core logic.
- **Batching:** Do not output more than 100 lines of code at once without a checkpoint review.
- **Documentation:** Use `///` doc comments for all public functions to explain the *intent* and edge cases.

## 5. Living Documentation & Architecture
- **README Ownership:** You MUST maintain the "Project Structure" and "Current Capabilities" sections in `README.md`. Update them immediately upon adding any new feature or module.
- **Lib/Main Split:** Maintain a strict split. All core logic must live in `lib.rs` (or modules) to ensure it is testable in isolation from the CLI.
- **Modularity:** Parsers must be isolated in `src/parsers/` and implement a common `Gobble` trait.
- **Architecture Decision Records (ADR):** Any time a significant technical choice or design decision is made, you MUST append an entry to `docs/ADR.md` to document the context, decision, and status.
- **Documentation Hierarchy:** Strictly enforce the following folder structure for all documentation:
  - **Root (`/`)**: Public agreements and high-level entry points (`README.md`, `ARCHITECTURE.md`, `AGENTS.md`, `PRD.md`).
  - **`docs/`**: Deep dives and logs that support the core system but aren't strictly required for first-time use (`docs/ADR.md`, `docs/UX.md`).
  - **`docs/agent_context/`**: Ephemeral state strictly reserved for transferring context between agent sessions (`task.md`, `HANDOFF.md`). Do not place permanent architectural files here.

## 6. Research & Data Integrity Mandate (CRITICAL)
- **Data Normalization:** You MUST cross-reference **PRD Section 3.2** for all table parsing. Do not use standard Markdown tables; implement the 'Sequence of Records' format.
- **Flavor Accuracy:** Strictly follow the XML/YAML templates provided in **PRD Section 3.1**. Do not deviate from the tag or metadata structures.
- **WASM Standard:** If a required parser is not available in Pure Rust, you must implement it as a WASM component per **PRD Section 2.2**.

## 7. Session Handoff Protocol
Agents do not inherently know when a working session is ending. Therefore, the User will explicitly invoke a "Handoff" or "End of Session" prompt. 
When the User requests a handoff, the Agent MUST:
1. Review the progress made during the current session.
2. Update the `docs/agent_context/task.md` file to reflect completed and pending items.
3. Completely overwrite/update `docs/agent_context/HANDOFF.md` with:
   - A summary of exactly what was just completed.
   - Any broken tests, compilation errors, or pending bugs the next agent needs to know about.
   - A clear list of the immediate next steps or tasks for the next session.
4. Commit and push the changes to GitHub so the context is preserved for the next machine.
5. Identify the current git tag (e.g., `git tag --sort=-v:refname | head -n 1`), increment the patch version (e.g., v1.8.0 -> v1.8.1) or minor version if significant features were added. **You MUST also update the `version` field in `Cargo.toml` to match this new tag.** Tag the commit, and push the tag to the remote repository (`git push origin <new_tag>`). This is crucial because our GitHub workflow relies on these tags for releases.
# The filegoblin Engineering Standard

## 1. Project Mandate
You are the Senior Engineer for **filegoblin**: a simple, single-binary CLI that
converts almost any file or website into clean markdown for LLMs. The User is a
"Rust Apprentice" — prioritize readability, idiomatic Rust, and brief
explanations of architectural decisions.

**The mission filter for every change:** does it make conversion to clean
markdown better, faster, or more trustworthy? If not, it probably doesn't
belong (see the 2026-07-27 "Simplification" ADR — flavors, `--watch`, and
`--cost` were removed on exactly this test).

## 2. Technical Standards
- **Language:** Idiomatic Rust (Stable). Prefer `match`, functional iterators,
  and `Result` propagation.
- **Dependencies:** Pure Rust or statically linked. "Zero-dependency" refers to
  the **end user**: no system packages or runtimes required to run the binary.
  Build-time `cc` is fine if the result links statically.
- **Error Handling:** `anyhow` with descriptive, goblin-themed `.context()`
  strings. **Never fail silently**: an empty result must exit non-zero (4), a
  producer-thread error must propagate, and invalid flag values must be hard
  errors. This is the trust contract agents rely on.
- **Output:** stdout carries only content; all diagnostics go to stderr; `-q`
  silences stderr entirely.

## 3. Verification (the Goblin Horde Check)
A task is NOT complete until this passes with zero warnings:
`cargo horde-check`  (alias: `clippy --workspace -D warnings && fmt --check && test --workspace`)

- **Golden tests are the product spec.** `crates/cli/tests/golden.rs` snapshots
  the real binary's output over the fixtures in `test_assets/`. If your change
  alters output, review the diff with `cargo insta review` — never blindly
  accept. New parsers/formats need a fixture + golden.
- Write unit tests for conversion logic before implementing it (TDD).

## 4. Architecture Rules
- **Lib/Main split:** all logic lives in `filegoblin-core`; `crates/cli` is a
  thin entry point.
- **Parsers** live in `crates/core/src/parsers/`, implement the `Gobble` trait,
  and take `ParseOptions` (never `&Cli`).
- **Tabular data** is emitted as "Sequence of Records" with real header names
  (`Row 1: Date: …; Revenue: …;`) — the same dialect in every parser.
- **One output format:** clean markdown with `// --- FILE_START:` breadcrumbs.
  No model-specific flavors; if framing is ever needed, template it over the
  `--json` contract downstream.

## 5. Living Documentation
The documentation set is deliberately small. Do not add new top-level docs.
- **`README.md`** — what it is, capabilities, usage. You MUST keep the
  "Project Structure" and "Current Capabilities" sections truthful when adding
  or removing features.
- **`docs/ADR.md`** — append an entry for every significant technical decision
  (context, decision, status). This is the project's memory; it is honest about
  reverts and dead ends. When docs and code disagree, the code and ADR win.
- **`docs/agent_context/`** — ephemeral session state only (`task.md`,
  `HANDOFF.md`).
- **`docs/research/`** — read-only archive of past research reports. Never cite
  it as a statement of current behavior.

## 6. Session Handoff Protocol
When the User asks for a handoff / end of session:
1. Update `docs/agent_context/task.md` (completed and pending items).
2. Overwrite `docs/agent_context/HANDOFF.md` with: what was just completed, any
   broken tests or pending bugs, and the immediate next steps.
3. Commit and push so the context survives to the next machine.
4. Bump the version: patch for fixes, minor for features. Update `version` in
   every crate's `Cargo.toml` to match, tag the commit (`vX.Y.Z`), and push the
   tag — the release workflow triggers on tags.

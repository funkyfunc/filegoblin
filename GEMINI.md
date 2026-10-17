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
- **Tabular data** is emitted as standard markdown pipe tables (row 1 of the
  source is the header row) — the same dialect in every parser.
- **One output format:** clean markdown with `// --- FILE_START:` breadcrumbs.
  No model-specific flavors; if framing is ever needed, template it over the
  `--json` contract downstream.

## 5. Living Documentation
The documentation set is deliberately small. Do not add new top-level docs.
- **`README.md`** — what it is, capabilities, usage. You MUST keep the
  "Project Structure" and "Current Capabilities" sections truthful when adding
  or removing features.
- **`docs/RELEASING.md`** — how a release is built, where it is published
  (GitHub Releases, PyPI, npm), the registry setup, and known release failures.
  Keep it truthful when you change packaging or the release workflows.
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
4. Do **not** bump the version or tag as part of a handoff. A `vX.Y.Z` tag
   publishes permanent versions to PyPI and npm, so release only when the User
   asks for one (see §7).

## 7. Releases and Publishing
- Pushing a `vX.Y.Z` tag builds and publishes everything: the GitHub release
  (lite and `filegoblin-full` archives, plus the model archive), PyPI
  (`filegoblin`) and npm (`filegoblin` plus the `@filegoblin/*` platform and
  model packages). Publishing is tokenless trusted publishing from
  `.github/workflows/publish.yml`.
- Release only when the User asks. Patch for fixes, minor for features.
  Follow `docs/RELEASING.md`: it lists every file that pins the version, the
  checks to run before tagging, every package name, and known failures.
- Creating a brand-new package name on PyPI or npm needs the User (a logged-in
  account with 2FA or a passkey). Prepare the exact command and hand it over;
  never handle their passwords, codes or tokens.
- If it is available, the `registry-release` skill (private repo
  `funkyfunc/agent-skills`) automates this procedure and has preflight and
  registry-status scripts.

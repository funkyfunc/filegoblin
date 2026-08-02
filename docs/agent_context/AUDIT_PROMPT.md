# Fresh-Eyes Audit Prompt

Paste the block below into a new session. It is written to orient you without
telling you what to conclude — the point is a genuinely independent read.

---

I want a fresh-eyes audit of the Rust workspace at
`/Users/stompinggrounds/Development/filegoblin`. Read the code directly; treat
`docs/ADR.md` as *one engineer's after-the-fact reasoning*, not as fact — if the
code disagrees with it, or if a decision recorded there looks wrong to you, say
so. Same for `README.md`: verify its claims rather than trusting them.

**What the tool is:** a single-binary CLI that converts almost any file, URL, or
media into clean markdown for LLMs/agents. Formats: PDF (incl. scanned → OCR),
Office, OpenDocument, EPUB, email, HTML/web (with an optional headless-Chrome
render path), images (OCR), audio/video (whisper transcription), subtitles,
RSS/Atom, archives, source trees. It also ships an MCP stdio server, a
conversion cache, a PII/secret scrubber, and a ratatui TUI. ~11k lines of Rust
across `crates/{core,cli,tui}`, ~120 test functions.

**Build/test:**
- `cargo horde-check` — the project's gate (clippy -D warnings, fmt --check, test)
- `cargo test --workspace` — default features
- `cargo test --workspace --features transcribe-metal` — includes whisper (needs
  cmake; first run may download a ~574MB model)
- Golden/e2e tests live in `crates/cli/tests/` and use `insta` snapshots
  (`cargo insta review` to inspect diffs)

**Context that matters:** the owner intends to **publish this** (likely npm
wrapper + GitHub releases via cargo-dist) shortly after this audit. So weight
findings by what would embarrass or break a public v1. The repo is at version
1.11.0 with ~23 untagged commits of features since that tag.

**What I want from you:**

1. **Code quality and craft.** Read the parsers, the pipeline, the routing, the
   CLI surface. Where is this code good, and where is it sloppy, duplicated,
   over-clever, or fragile? Name specific files and lines. Be blunt — flattery
   is useless here.

2. **Correctness risks.** Hunt for real bugs, not style. Especially: error
   paths, edge cases in the newer parsers, anything that could silently produce
   wrong or empty output. (This project's stated contract is "fail loudly,
   never silently succeed" — verify that actually holds.)

3. **Publish-readiness.** Assume a stranger runs `cargo install` or grabs a
   release binary on macOS/Linux/Windows. What breaks? Look hard at the feature
   flags, the build requirements they imply, and the release configuration.

4. **Testing.** Is the suite meaningful or is it theater? What's untested that
   matters? Are the golden snapshots asserting anything real?

5. **Feature gaps.** With fresh eyes on the mission ("point it at anything, get
   clean markdown"), what's obviously missing or obviously not worth keeping?

**Areas I specifically want scrutinized** (listed because they were built fast
or haven't been examined since, not because I know they're wrong):

- `crates/core/src/parsers/` — `feed.rs`, `subtitle.rs`, `transcribe.rs`,
  `opendocument.rs` are the newest and least battle-tested.
- `crates/core/src/browser.rs` — spawns the user's Chrome; the
  "dump-then-hang" detection is heuristic and the SSRF posture is a documented
  tradeoff. Is it sound?
- `crates/core/src/mcp.rs` — hand-rolled MCP protocol. It has never been run
  against a real client (only our own test harness). Would Claude Desktop /
  Claude Code actually work with it?
- `crates/core/src/cache.rs` — note which cache kinds get swept and which don't.
- `crates/core/src/formatters.rs` (847 lines) and `lib.rs` (739) — the two
  biggest files; `gobble_app` and `gobble_to_string` overlap.
- `crates/tui/` (~1100 lines, off by default, no tests) — does it still make
  sense to keep? Does it even know about the formats added recently?
- `Cargo.toml` `[workspace.metadata.dist] all-features = true` versus the
  `transcribe` feature's build requirements, and `.cargo/config.toml`'s `[env]`
  block. Think about what a release build on CI actually has to do.
- `.github/workflows/horde-check.yml` — what does CI actually cover?

**Deliverable:** a prioritized report — what to fix before publishing, what to
fix soon after, what to consider later, and what to cut. Include a short "what's
genuinely good here" section so I know what not to break. Cite `file:line`.
Don't change any code; I want the assessment first.

Use subagents to parallelize the reading if that helps.

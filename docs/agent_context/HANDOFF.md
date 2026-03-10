## Session Handoff: 2026-03-09 (Post v1.7.0)

### 1. What was just completed
- **v1.7.0 Evaluation Feedback (previous session):**
  - Gemini flavor fix, `--tokens` labels, `--tokens-only`, `--exclude`, `--depth`, `--manifest`, `--diff-format`, BM25 relevance scores, warning cleanup.
  - Full doc update (README, task.md, ADR, HANDOFF).
  - Tagged `v1.7.0`, pushed to `origin/main`.
- **Shell Setup:**
  - Added `fg-update` alias (rebuild + install + refresh zsh completions) and `zsh-add` helper to `~/.zshrc`.
  - Added `alias gobble="filegoblin"`.
- **Roadmap Planning:**
  - Added Phase XIV (new ingestion sources) and Phase XV (wow-factor UX) to `docs/agent_context/task.md`.
  - Wrote YouTube transcript research prompt (see below).

### 2. Next Steps for the Next Machine

#### Priority 1: GitHub URL Ingestion (Phase XIV)
One-shot implementation. Pattern:
- Detect `github.com` URL in `lib.rs` URL router (alongside existing `twitter.com` check)
- Use `git2` crate to shallow-clone (`--depth 1`) to `std::env::temp_dir()`
- Pass the tempdir path into existing `gobble_local()` horde pipeline
- Clean up tempdir on exit
- Private repos: check for `~/.config/filegoblin/credentials.json` and use stored token as `git2` credential callback

#### Priority 2: Twitter OAuth 2.0 Refactor (Phase XIV)
**Current approach is fragile** — the guest-token + `ct0` CSRF cookie hack breaks when Twitter rotates tokens (silently, no warning).

**Recommended approach: Graceful degradation with optional OAuth**
- **Default (no setup required):** Attempt existing guest-token flow as before
- **On failure:** Instead of a cryptic error, surface a clear, actionable message:
  ```
  ⚠️  Twitter rate-limited or blocked the unauthenticated request.
  💡  For reliable access, run: gobble --twitter-login
      This takes ~2 minutes and stores credentials locally.
  ```
- **`--twitter-login`:** Triggers a one-time OAuth 2.0 PKCE flow — opens browser, captures callback on `localhost:7890`, stores `access_token` + `refresh_token` in `~/.config/filegoblin/credentials.json`
- **Subsequent runs:** If credentials exist, use them silently (auto-refresh if expired). If not, fall back to guest-token and warn on failure.

This gives zero-friction for casual users while giving power users a reliable upgrade path. No research needed — `oauth2` crate handles the PKCE flow.

#### Priority 3: YouTube Transcript Ingestion (Phase XIV)
**Needs research first.** Research prompt generated at:
`docs/agent_context/youtube_research_prompt.md`

Hand this to a research agent before implementing. Key unknowns: stability of unauthenticated `timedtext` endpoint, whether a maintained Rust crate exists, and fallback strategy.

#### Priority 4: Jupyter Notebook Gobbler (Phase XIV)
One-shot. `.ipynb` is plain JSON. Parse `cells[]` array, emit by `cell_type`: markdown cells as-is, code cells in fenced blocks, output cells as blockquotes.

#### Other Phase XIV/XV items
See `docs/agent_context/task.md` — all Phase XIV items (SQLite, Slack/Discord, `--cost`, `--summary`, `--watch`) are one-shot implementations requiring no research.

### 3. Current State
- `cargo test` passes (21/21, 0 warnings) on `v1.7.0`
- `gobble` alias + `fg-update` + `zsh-add` active in `~/.zshrc`
- Shell completions at `~/.zfunc/_filegoblin`

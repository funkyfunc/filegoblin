## Session Handoff: 2026-03-09 (Post v1.8.0)

### 1. What was just completed
- **GitHub URL Ingestion (Phase XIV):**
  - Added `git2` crate. `src/parsers/github.rs` implements shallow clone (`depth=1`) to tempdir.
  - Routed `github.com` URLs in `lib.rs` alongside existing `twitter.com` check.
  - Private repos: reads `github_token` from `~/.config/filegoblin/credentials.json`.
  - Tempdir cleaned up on exit (success or error).

- **Twitter OAuth 2.0 PKCE (Phase XIV):**
  - New `--twitter-login` CLI flag triggers one-time PKCE browser flow.
  - Spawns `tiny_http` server on `127.0.0.1:7890` to capture callback code.
  - Stores `access_token` + `refresh_token` + expiry in `~/.config/filegoblin/credentials.json`.
  - Auto-refreshes expired tokens silently before each scrape.
  - Falls back gracefully to legacy GraphQL guest-token flow if no credentials present.
  - On guest-token failure: surfaces actionable UX warning pointing user to `--twitter-login`.
  - Custom Client ID supported via `TWITTER_CLIENT_ID` env var.
  - Authenticated path hits canonical `api.twitter.com/2/tweets` V2 endpoints.
  - Note: X API requires Basic tier ($200/mo) or PPU model to read data. Free tier is write-only.

- **Tagged `v1.8.0`** (not yet pushed).

### 2. Next Steps for the Next Machine

#### Priority 1: YouTube Transcript Ingestion (Phase XIV)
**Needs research first.** Research prompt at: `docs/agent_context/youtube_research_prompt.md`

Key unknowns to resolve before implementation:
- Stability of the unauthenticated `timedtext` / `get_transcript` endpoint
- Whether a maintained Rust crate exists (or if `yt-dlp` subprocess is the best path)
- Fallback strategy when auto-captions are unavailable

Hand to a research agent first, then implement.

#### Priority 2: Google OAuth 2.0 (Phase XIV)
**Needs research first.** Research prompt at: `docs/agent_context/google_oauth_research_prompt.md`

Pattern will closely mirror the Twitter PKCE flow:
- New `--google-login` flag using `oauth2` crate (same PKCE/tiny_http pattern)
- Scopes: `drive.readonly` + `docs.readonly` (minimum for reading Docs/Drive files)
- Callback on `127.0.0.1:7890/callback` (same port, reuse same pattern)
- Store credentials in existing `~/.config/filegoblin/credentials.json` under `google_token`
- Then wire up `drive.google.com` and `docs.google.com` URLs in the `lib.rs` router

#### Priority 3: Jupyter Notebook Gobbler (Phase XIV)
One-shot. `.ipynb` is plain JSON. Parse `cells[]` array, emit by `cell_type`:
- `markdown` → emit as-is
- `code` → fenced code block with language hint from kernel
- `outputs` → blockquote (stdout/result) or skip (images)

#### Other Phase XIV/XV items
See `docs/agent_context/task.md` — SQLite, Slack/Discord, `--cost`, `--summary`, `--watch` are all one-shot, no research needed.

### 3. Current State
- `cargo check` passes cleanly on `v1.8.0` (2 minor dead-code warnings, non-blocking)
- Tagged `v1.8.0` locally — run `git push && git push --tags` to publish
- `gobble` alias + `fg-update` + `zsh-add` active in `~/.zshrc`
- Shell completions at `~/.zfunc/_filegoblin`

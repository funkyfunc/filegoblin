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

- **YouTube Transcript Ingestion (Phase XIV, Priority 1) — ✅ COMPLETE:**
  - New `src/parsers/youtube.rs` — `YouTubeGobbler` struct implementing `Gobble`.
  - Zero-dependency: uses `reqwest` (rustls) + `serde_json` + `quick-xml`, all already in tree.
  - InnerTube `/v1/player` POST (ANDROID client) for metadata + caption track discovery.
  - Track priority: Manual EN → Manual `--lang` → Auto EN → Auto `--lang` → first available.
  - URL parameter rewriting via `url` crate to safely force `&fmt=srv1` (overrides YouTube's default `fmt=srv3`).
  - Server-side translation via `&tlang=XX` when `--lang` is specified and no native track exists.
  - `yt-dlp` subprocess fallback if native XML fetch returns empty (POT block).
  - Partial HTML entity cleanup pass (`gt;gt;` → `>>`, `quot;` → `"`, etc.) for multi-speaker captions.
  - New `--lang <code>` CLI flag added under "Crawling & Ingestion".
  - Routed `youtube.com` / `youtu.be` URLs in `lib.rs` (before the generic web fetcher).
  - `commands` shell function added to `~/.zshrc` (lists personal aliases, functions, cargo bins).

- **Tagged `v1.8.0`** (4 commits ahead of origin — not yet pushed).

### 2. Next Steps for the Next Machine

#### Priority 1: Google OAuth 2.0 + Gemini Share Links (Phase XIV)
**Research prompt at:** `docs/agent_context/google_oauth_research_prompt.md`

Pattern closely mirrors the Twitter PKCE flow — should be a relatively fast implementation:
- New `--google-login` flag using `oauth2` crate (same PKCE/tiny_http pattern already in tree)
- Scopes: `drive.readonly` + `docs.readonly` (minimum for reading Docs/Drive files)
- Callback on `127.0.0.1:7890/callback` (same port, reuse same `tiny_http` pattern)
- Store credentials in `~/.config/filegoblin/credentials.json` under `google_token` key
- Wire up `drive.google.com` and `docs.google.com` URLs in the `lib.rs` router
- Wire up `docs.google.com` export URL (`/export?format=md` or similar)
- **Also store the Google session cookies** (`SID`, `__Secure-1PSID`, `__Secure-3PSID`) returned by the OAuth callback flow — these are needed for Gemini share links (see below)

Read the research prompt first before implementing.

#### Priority 1b: Gemini Share Link Ingestion (Phase XIV)
**Depends on:** `--google-login` storing session cookies (above)

Reverse-engineered the exact API call Gemini uses for share links (from Network tab):
- **Endpoint:** `POST https://gemini.google.com/_/BardChatUi/data/batchexecute`
- **Key params:** `rpcids=ujx1Bf`, `source-path=/share/<id>`, `bl=<build_label>`, `f.sid=<session_id>`
- **POST body:** `f.req=[[["ujx1Bf","[[\"{share_id}\"]]",null,"generic"]]]&at=<xsrf_token>`

**Implementation flow for `GeminiGobbler`:**
1. Detect `gemini.google.com/share/<id>` URL pattern
2. Load stored Google session cookies from `credentials.json`
3. `GET gemini.google.com/share/<id>` with cookies → parse `WIZ_global_data` JSON blob from HTML
   - Extract `FdrFJe` → `f.sid` parameter
   - Extract `cfb2h` → `bl` (build label, changes with deployments)
   - Extract `SNlM0d` → `at` XSRF token (**only present in authenticated page loads**)
4. Fire the `batchexecute` POST with all three params + session cookies
5. Parse the response (Google's `)]}'` prefixed JSON + length-chunked streaming format)
6. Extract conversation turns from the nested array structure

**Important findings from testing:**
- Without auth cookies: page loads (511KB) but `SNlM0d` (at/XSRF token) is absent → RPC returns error 4
- With valid auth cookies: full `WIZ_global_data` including `SNlM0d` is injected → RPC succeeds
- The `bl` build label changes with each server deployment — must be scraped fresh each call, not hardcoded
- OAuth bearer tokens alone do NOT work — this is cookie-based (same as how the browser does it)
- Response format: `)]}'` header + length-prefixed chunks + nested JSON array — similar to YouTube's protobuf-lite

#### Priority 2: `--clipboard` Flag (Quick Win, ~30 min)
Read clipboard contents as filegoblin input — inverse of existing `--copy` (which outputs *to* clipboard):
- Add `--clipboard` / `-c` flag to `src/cli.rs`
- In `lib.rs`: if `--clipboard` set, spawn `pbpaste` (macOS) or `xclip -o` (Linux) and treat stdout as the input target
- Allows: copy text from Gemini/ChatGPT → `gobble --clipboard` instead of fussing with temp files

#### Priority 3: Jupyter Notebook Gobbler (Phase XIV)
One-shot. `.ipynb` is plain JSON. Parse `cells[]` array, emit by `cell_type`:
- `markdown` → emit as-is
- `code` → fenced code block with language hint from kernel
- `outputs` → blockquote (stdout/result) or skip (images)

#### Other Phase XIV/XV items
See `docs/agent_context/task.md` — SQLite, Slack/Discord, `--cost`, `--summary`, `--watch` are all one-shot, no research needed.

### 3. Current State
- `cargo check` passes cleanly (1 minor dead-code warning on `parse_syndication` in twitter.rs — non-blocking)
- Tagged `v1.8.0` locally — run `git push && git push --tags` to publish
- `gobble` alias + `fg-update` + `zsh-add` + `commands` active in `~/.zshrc`
- Shell completions at `~/.zfunc/_filegoblin`
- YouTube gobbler tested against live URLs — transcripts extract correctly including multi-speaker `>>` indicators

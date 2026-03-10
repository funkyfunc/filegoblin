# Research Prompt: Google OAuth 2.0 for filegoblin

## Context

`filegoblin` is a zero-dependency, statically-linked Rust CLI tool that ingests content into LLM-optimized Markdown. It already implements an optional OAuth 2.0 PKCE flow for Twitter/X (pattern: `--twitter-login` → browser auth → store tokens in `~/.config/filegoblin/credentials.json` → auto-refresh on subsequent runs).

We want to add `--google-login` with the same graceful degradation pattern:
1. **Default:** Attempt unauthenticated access (already works for public pages)
2. **On auth-wall failure:** Surface a clear message: "Try `gobble --google-login` for authenticated access"
3. **After login:** Use stored credentials silently for Google Docs, Drive files, and Gemini share URLs

---

## Please Fetch These Docs Directly (Do Not Rely on Training Data)

1. `https://developers.google.com/identity/protocols/oauth2/native-app` — OAuth for installed apps
2. `https://developers.google.com/drive/api/guides/about-sdk` — Drive API overview
3. `https://developers.google.com/docs/api/reference/rest` — Docs API reference
4. `https://developers.google.com/identity/protocols/oauth2/scopes` — Full scope list

---

## Research Questions

### 1. OAuth 2.0 PKCE for a CLI App
- What are the current authorization and token exchange endpoint URLs?
- Does Google support `http://localhost` as a redirect URI for native/CLI apps without app review?
- What is the exact PKCE flow for a CLI (no browser automation — just print the URL and wait for callback)?

### 2. Minimum Required Scopes
What exact scope strings are needed to:
- Read a Google Doc by URL (e.g. `docs.google.com/document/d/<id>`)
- Read a file from Google Drive by URL
- Access a Gemini conversation share link (e.g. `gemini.google.com/share/<id>`) — is this even possible via API, or is it always JS-only?
- Are these scopes available without OAuth app review, or do they require Google verification?

### 3. Free Tier & App Registration
- Can a developer create a free Google Cloud project and use these APIs without paying?
- What are the quota limits on the free tier for Docs/Drive reads?
- Does the app require Google's OAuth consent screen verification, or can it run in "Testing" mode for personal use?

### 4. Fetching Content
- For Google Docs: what is the API call to export a Doc as plain text or Markdown?
- For Google Drive: how do we detect file type and route to the right export format?
- For Gemini share URLs: is there any API access at all, or are these always browser/JS-only?

### 5. Token Storage & Refresh
- What is the access token TTL?
- Is `offline` scope / `access_type=offline` still the correct way to get a refresh token?
- How long do refresh tokens last before requiring re-auth?

---

## Deliverables

1. **Confirmed OAuth endpoint URLs** — authorization, token exchange, token refresh
2. **Minimum scope string** for Docs + Drive read access
3. **Gemini share verdict** — is API access possible at all? Yes/No + explanation
4. **Free tier viability** — can a developer use this without billing enabled?
5. **App review requirements** — testing mode sufficient for personal CLI use?
6. **Fetch recipe** — exact API calls (with example URLs) to export a Google Doc as plain text
7. **Any gotchas** — known issues with localhost redirect URIs, scope changes post-2023, etc.

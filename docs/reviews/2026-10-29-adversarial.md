# Adversarial review of v1.16.0 (2026-10-29)

A hostile, fresh-eyes pass over everything except PDF output quality: the CLI, the
non-PDF parsers, archives, the crawler, `--browser`, the MCP server, the walk and
split paths, packaging, the release workflows and the documentation's claims.
Every finding below was reproduced against a real binary (the v1.16.0 release
build, or a debug build of `0a42c28`; the parser findings against a release build
of this branch with `main`'s memory ceiling merged) unless it says otherwise.
Crafted inputs lived in a scratch directory, not the repo; the regression tests
that now cover them are named per finding.

Severity is about what an attacker (a hostile file, page, repository or model
prompt) or an ordinary mistake can do to a user who runs filegoblin as documented.

**Action needed from the maintainer (cannot be done from the repo):** register
`filegoblin-models` and the eight `filegoblin-model-*` names on PyPI now — see C3.

## Summary

| # | Severity | Finding | Status |
|---|---|---|---|
| C1 | Critical | A symlink in a tar (or a cloned repo, or any walked directory) reads any local file into the output | Fixed |
| C2 | Critical | `http://[::ffff:127.0.0.1]/` and other IPv6 spellings bypass the SSRF guard | Fixed |
| C3 | Critical | `pip install "filegoblin[models]"` depends on PyPI names nobody owns | Mitigated for future releases; v1.16.0 needs the names registered |
| H1 | High | `--browser`: one line of page JavaScript navigates Chrome to a private address and returns that page | Fixed |
| H2 | High | HTTP 404/500 pages are converted, exit 0, and cached | Fixed |
| H3 | High | npm launcher fork-bombs the machine when the platform package is missing | Fixed |
| H4 | High | Nothing stops a release from publishing permanently from a red commit | Fixed (publish gate) |
| H5 | High | One damaged file's parser panic ends the whole run (exit 1); 185 of 225 flagged mutants | Fixed |
| H6 | High | Damaged `.docx`, `.ods` and deeply nested HTML spin at full CPU with flat memory, past any memory ceiling | Fixed |
| H7 | High | Small hostile files drove the CLI to 6+ GB (the 60–100 GB processes during this review) | Ceiling (merged from `main`) now stops each with exit 5; `gridSpan` fixed at the source; the rest deferred |
| M1 | Medium | Crawler drops failed pages silently and converts error pages as content | Fixed |
| M2 | Medium | Crawler ignores the page charset (Latin-1 → U+FFFD) | Fixed |
| M3 | Medium | Archive members are not named under their archive; `--split` overwrites across archives | Fixed |
| M4 | Medium | `--split` silently overwrites documents whose names collide (`notes` / `notes.md`, case) | Fixed |
| M5 | Medium | A 7 MB `.log.gz` peaks at 3.2 GB resident: every streamed document was held twice | Fixed (halved) |
| M6 | Medium | `FILEGOBLIN_ALLOW_LOCAL_URLS=0` turns the SSRF guard off | Fixed |
| M7 | Medium | npm launcher orphans the binary on SIGTERM and reports signal deaths as exit 1 | Fixed |
| M8 | Medium | `publish.yml` dispatch input spliced into a script; any run's artifact publishable | Fixed |
| M9 | Medium | A missing platform archive only warns; the release silently drops that platform | Fixed |
| M10 | Medium | Flaky `browser::tests::discovery_and_js_render` on ubuntu | Diagnosed in part; diagnostics and flags fixed |
| M11 | Medium | Linux glibc floor (2.35) undocumented and unenforced on npm | Deferred |
| M12 | Medium | Release model weights and build deps are unpinned | Deferred |
| L1 | Low | `--chunk-size 0` accepted (one chunk per word) | Fixed |
| L2 | Low | Crawl output order depends on network timing | Fixed |
| L3 | Low | A gzip that starts with 1 KiB of NULs extracts as an empty tar | Fixed |
| L4 | Low | npm launcher prefers `node_modules/target/...` over the platform package | Fixed |
| L5 | Low | README / RELEASING claims that don't match the code | Fixed |
| L6 | Low | `-q` does not silence per-file error lines, while GEMINI.md says it silences stderr entirely | Not changed (decision needed) |
| L7 | Low | MCP: converting `/` or `~` is unbounded work; a 5th concurrent call blocks `ping` | Deferred |
| L8 | Low | `--chunk-overlap` silently ignored when ≥ the chunk size or without `--chunk-size` | Deferred |
| L9 | Low | CI gaps: `bundled-models` never compiled, actions unpinned, `--locked` unused | Deferred |
| P1–P3 | — | PDF worker boundary findings | P2 fixed; P1, P3 for the PDF session (below) |

The parser findings (H5–H7) come from a fuzzing pass over the non-PDF parsers:
241 crafted inputs (bombs, deep nesting, huge repeat counts, entity expansion,
XXE, odd encodings, wrong extensions) and 10,000 byte-level mutants of xlsx,
xlsb, ods, docx, odt, pptx and epub fixtures, of which 225 were flagged.
That pass ran without a memory ceiling and drove filegoblin processes to 60–100
GB on this 24 GB machine before they were killed; `main` has since gained a CLI
memory ceiling (`4283928`, merged here). Every crafted input and flagged mutant
was then replayed one at a time under `FILEGOBLIN_MEMORY_MB=6144` with a 120 s
timeout, against a release build of this branch before the parser fixes, and
again after them.

---

## Critical

### C1. Symlinks in a walked tree read arbitrary local files

**Where:** `crates/core/src/walk.rs` `walk_tree` (anything that is not a directory
was pushed as a file, symlinks included), reached from `parsers/archive.rs`
(tar's `unpack_in` creates symlinks), `routing.rs` (GitHub clones check symlinks
out) and any `--horde` directory.

**Repro (v1.16.0):**
```bash
python3 - <<'EOF'
import tarfile, io, os
with tarfile.open('sym.tar', 'w') as t:
    ti = tarfile.TarInfo('readme.txt'); ti.size = 6; t.addfile(ti, io.BytesIO(b'hello\n'))
    for name, target in [('secret.txt', '/etc/hosts'), ('home.md', os.path.expanduser('~/.zshrc')),
                         ('up.txt', '../../../../../../../../etc/hosts')]:
        ti = tarfile.TarInfo(name); ti.type = tarfile.SYMTYPE; ti.linkname = target; t.addfile(ti)
EOF
filegoblin -q sym.tar        # prints ~/.zshrc and /etc/hosts as archive members, exit 0
```
The same happens with `filegoblin https://github.com/<attacker>/<repo>` (git checks
the link out) and with `--horde` over any downloaded repository. Over MCP, a model
steered by a page or a file can ask for exactly that archive or repo, and the
contents of `~/.ssh/id_rsa` land in its context.

**Fix:** a symlink is followed only when its canonical target is inside the walked
root; the rest (and dangling links) are skipped and listed on stderr. A file named
directly on the command line is still read wherever it points.
**Tests:** `walk::tests::symlinks_that_leave_the_root_are_skipped`,
`parsers::archive::tests::tar_symlinks_cannot_read_files_outside_the_archive`.

### C2. IPv6 spellings of private addresses bypass the SSRF guard

**Where:** `crates/core/src/net.rs` `is_blocked_ip`. IPv6 addresses were checked
only against `::1`, `fc00::/7` and `fe80::/10`. An IPv4-mapped address
(`::ffff:a.b.c.d`) is connected by a dual-stack socket to the IPv4 host. The
textual check, the resolver check and the redirect check all share the function, so
an AAAA record of `::ffff:127.0.0.1` passed too.

**Repro (v1.16.0):**
```bash
python3 -m http.server 8765 --bind 127.0.0.1 &
filegoblin http://127.0.0.1:8765/          # refused, as documented
filegoblin 'http://[::ffff:127.0.0.1]:8765/'   # the page, exit 0
filegoblin 'http://[::ffff:7f00:1]:8765/'      # the page, exit 0
```
`[::ffff:169.254.169.254]` (cloud metadata) also passed the guard; on this Mac it
only failed at connect time.

**Fix:** an IPv6 address that embeds an IPv4 one (mapped, compatible, NAT64
`64:ff9b::/96`, 6to4 `2002::/16`) is judged by that IPv4 address; `0.0.0.0/8` and
multicast are blocked as well. **Test:** `net::tests::ipv6_forms_of_blocked_ipv4_are_blocked`.

### C3. The PyPI `models` extra depends on unregistered names

**Where:** `scripts/package_registries.py` `build_pypi` gave the base wheel
`Requires-Dist: filegoblin-models==V; extra == "models"`, while `publish.yml`
uploads the base package only. **Evidence:** the live `filegoblin==1.16.0`
metadata declares that dependency; `https://pypi.org/pypi/filegoblin-models/json`
and every `filegoblin-model-*` project return 404. The PyPI page
(`packaging/python/README.md`) told users to run `pip install "filegoblin[models]"`.

Whoever registers `filegoblin-models` (any version `1.16.0`) is installed by that
command, and a wheel can run code (a `.pth` file runs on every Python start; an
sdist runs `setup.py` at install).

**Fix in the repo:** `PYPI_MODELS_EXTRA = False` leaves the extra out of future
wheels until the model projects exist; the release smoke test installs the model
package by name; the PyPI README points at `filegoblin --install-models`;
RELEASING.md says to flip the flag back when the projects exist.
**Still needed:** v1.16.0's metadata is permanent. Register the nine names on PyPI
under the project account (RELEASING.md "Model packages on PyPI" has the steps), or
yank 1.16.0 on PyPI if that cannot happen soon.

## High

### H1. `--browser` could be navigated anywhere

**Where:** `crates/core/src/browser.rs`. Only the seed URL was vetted (by the static
fetch); Chrome resolved and connected on its own. The ADR and README called this a
known limitation for redirects, but a page needs no redirect: script can navigate.

**Repro (v1.16.0; the seed uses C2 so that it passes the guard — with a public page
the result is the same):**
```bash
mkdir inner && cd inner && echo '<html><body><main><h1>INNER SECRET</h1><p>only on loopback ...</p></main></body></html>' > index.html
python3 -m http.server 8766 --bind 127.0.0.1 &  cd ..
echo "<html><body><main><p>Innocent text ...</p></main><script>location.href='http://127.0.0.1:8766/'</script></body></html>" > hop.html
python3 -m http.server 8765 --bind 127.0.0.1 &
filegoblin --browser 'http://[::ffff:127.0.0.1]:8765/hop.html'   # prints "INNER SECRET"
```

**Fix:** `browser/guard_proxy.rs`, a loopback HTTP proxy that Chrome must use
(`--proxy-server`, `--proxy-bypass-list=<-loopback>` so loopback is not exempt,
`--force-webrtc-ip-handling-policy=disable_non_proxied_udp`). It resolves each
`CONNECT host:port` or absolute-form `http://` request once, refuses the host if
any address is blocked, and connects to the address it checked (no second lookup
to rebind). A render that ends on a refusal (ours, or Chrome's net-error page after
a refused `CONNECT`) fails; refused subresources are listed on stderr. With
`HTTPS_PROXY` set, allowed connections are forwarded to that proxy (`NO_PROXY`
honoured), which replaces passing the proxy straight to Chrome.
**Tests:** `browser::tests::a_page_cannot_navigate_the_render_to_a_blocked_address`
(fails with an allow-everything policy, passes with the guard),
`browser::guard_proxy::tests::*`.
**Limits:** a page may still *load* blocked subresources' 403s and keep its own
content; only navigations fail the render. The proxy does not speak proxy
authentication (neither did Chrome with `--proxy-server`).

### H2. HTTP error statuses were success

**Where:** `crates/core/src/lib.rs` `fetch_url` never looked at the status;
reqwest returns 4xx/5xx as ordinary responses. The page was converted, the run
exited 0, and the error page was cached, so the next run served it from cache. The
same path turned a redirect the guard refused (reqwest's policy `stop()` returns the
3xx response) into "content".

**Repro (v1.16.0):**
```bash
FILEGOBLIN_ALLOW_LOCAL_URLS=1 filegoblin --no-cache -q http://127.0.0.1:8770/missing.html
# "# Error response / Error code: 404 ..."  exit 0
FILEGOBLIN_ALLOW_LOCAL_URLS=1 filegoblin http://127.0.0.1:8770/missing.html
# "♻️ Using cached fetch of ..."
```
**Fix:** `net::check_status` before the body is read or cached: a 4xx/5xx is an
error naming the status, a 3xx says the redirect was declined. Network errors now
carry their source chain (the resolver's refusal used to show only as "error
sending request"). **Test:** `tests::error_statuses_fail_the_fetch_and_are_not_cached`.

### H3. npm launcher fork bomb

**Where:** `npm/bin/filegoblin.js` `findBinary`. Without the platform package
(`npm install --omit=optional`, a lockfile from another OS, an unsupported arch)
the PATH fallback ran `which filegoblin`, which finds npm's shim for this very
launcher. It spawned itself, without limit.
**Repro:** a fake global install with the shim first on PATH and no
`@filegoblin/<platform>`: 481 node processes after 2 seconds (found by the packaging
sub-review; re-checked here with `npm/test/launcher.test.js` against the old file,
which fails).
**Fix:** PATH hits that resolve to this script are skipped, and the launcher marks
the child's environment so a launcher started through any other route stops at the
fallback. **Test:** `node --test npm/test/launcher.test.js` ("a missing platform
package fails instead of recursing"), now a step of Horde Check on Linux and macOS.

### H4. No test gate before a permanent publish

**Where:** `release.yml` (dist) runs no tests; `publish.yml` started on any
successful Release. **Evidence:** Horde Check was red on `main` from August until
`3eeb15a`, including on `1a433ef` (release v1.15.0), which was published.
For v1.16.0 Horde Check and Release started three seconds apart.
**Fix:** `publish.yml`'s `check` job waits (up to 90 minutes) for Horde Check on
the tagged commit and refuses to publish unless it passed. RELEASING.md now says to
tag only a green commit, since the GitHub release itself is not gated (release.yml
is generated by dist and is not edited by hand).
**Not verified:** the new job has not run on GitHub (no push from this session).
The `gh`/`jq` expressions were checked locally against sample JSON.

### H5. A parser panic ended the whole run

**Where:** `crates/core/src/lib.rs` `route_and_gobble` called the parser with no
panic boundary. calamine (xlsx: `xlsx/mod.rs:1817` multiply/add overflow in debug
builds, `:1774` subtract overflow; xlsb: `utils.rs:32` slice out of range,
`xlsb/mod.rs:326` missing key) and docx-rs (`reader/read_zip.rs:17-19` unwraps,
`documents/content_types.rs`, `reader/attributes/width.rs`) panic on values taken
from the file. A panic on a `--horde` worker thread surfaced as "The ingestion
thread panicked", exit 1, and every other file of the run was lost.
**Repro (v1.16.0 + main):** 185 of the 225 flagged mutants exit 1 with a panic;
`test_assets/hostile/calamine_panic.xlsb` (1.2 KB) is one. A directory holding it
and a text file printed nothing for the text file.
**Fix:** the dispatch runs under `catch_unwind`; a panic is that file's error
("its parser crashed (…)"), exit 4 when it was the only file. docx-rs is also
wrapped in `office.rs`, so a docx it chokes on falls back to filegoblin's own
streaming reader and still converts. **Test:**
`golden::a_parser_panic_fails_only_that_file` (exit 1 before the fix).

### H6. CPU spins that no memory ceiling sees

Three inputs ran at full CPU with flat memory (20–60 MB) until killed:
- **Truncated `word/numbering.xml`** (`<w:numbering …><w:abstractNum w:abstractNumId="0">`
  and nothing more, with a `styles.xml` present): docx-rs's numbering reader
  (`reader/numberings.rs`) loops forever, xml-rs returning the same EOF error on
  every call (seen in `sample`). Six docx mutants spin the same way.
- **An `.ods` whose `<table:table>` is never closed:** calamine's ODS reader spins
  (three mutants and two crafted files).
- **Deeply nested HTML:** `html.rs` refused more than 1,000 levels, but only after
  `Html::parse_document`, whose cost grows with the square of the depth: 100,000
  nested `<div>`s took 21 s, 300,000 over 120 s (release). The same reached the
  crawler (it parses each page for links first), email HTML bodies (20 s for a
  deep one) and EPUB chapters.

**Repro (before):** `perl -e 'alarm 120; exec @ARGV' filegoblin -q docx_hang_numbering_truncated.docx`
→ killed by the alarm, 21 MB; likewise `ods_unclosed_table_min.ods`,
`html_deep_div_300000.html`.
**Fix:** `parsers::check_package_xml` parses every XML part of a zip package
(syntax, a root element, every element closed) under the existing per-part cap.
A docx that fails goes straight to the streaming reader instead of docx-rs; a zip
workbook that fails is refused ("`content.xml` is malformed or cut short") before
calamine sees it. HTML gets a linear pre-scan (`html::refuse_deep_nesting`) that
refuses more than 5,000 levels of real nesting before any parse; void elements,
elements the parser closes itself (`<p>`, `<li>`, `<td>`, …), comments and script
text are not counted, and the parsed-DOM limit of 1,000 still applies after it.
**Tests:** `office::tests::a_truncated_numbering_part_does_not_hang`,
`sheet::tests::a_damaged_ods_fails_instead_of_hanging`,
`html::tests::absurd_nesting_is_refused_before_parsing`,
`html::tests::ordinary_unclosed_markup_is_not_counted_as_nesting`.
**Upstream:** the docx-rs and calamine loops are worth reporting to those
projects (not done: no issues filed on third-party repos without the
maintainer's say-so).

### H7. Small files, huge memory

Under the ceiling each of these now ends with exit 5 and a message, where before
they grew until the OS killed them:

| Input | Size | Before the ceiling | Now |
|---|---|---|---|
| `docx_gridspan_1e9.docx` (`<w:gridSpan w:val="1000000000"/>`) | 871 B | unbounded | **fixed at the source**: 95 MB, converts |
| `xlsx_cellrow_4294967295.xlsx`, `xlsx_farcell.xlsx`, `xlsb_farcell.xlsb`, `xlsb_col_huge.xlsb` (one cell far from A1) | < 2 KB | unbounded (calamine allocates the dense rectangle) | exit 5 |
| `csv_single_line_500MB.csv` (250 M fields in one row) | 500 MB | > 6 GB | exit 5 |
| `epub_16x200MB.epub` | ~1 MB | > 6 GB | exit 5 |

`TableBuilder::push_cell` now caps a row at 1,024 columns (`MAX_TABLE_COLUMNS`),
so no repeat count from a file (docx `gridSpan`, ODF `number-columns-repeated`)
can allocate past it. **Test:** `parsers::table_tests::a_row_cannot_outgrow_the_column_cap`.
**Deferred:** the calamine dense-range allocation needs a sparse reader (calamine's
cell reader rather than `worksheet_range`); the one-row CSV needs a field cap in
the CSV path. Both are bounded by the ceiling today. Other heavy-but-bounded
inputs under the ceiling: `pptx_16x200MB.pptx` 3.3 GB, `xlsx_2GBbomb_sst.xlsx`
5.2 GB (then an error), `xlsx_cellrow_100000000.xlsx` 3.1 GB.

**After the fixes:** the 212 crafted files and mutants that had panicked or spun
were replayed against a debug build of this branch: 97 exit 0, 115 exit 4, none
panics, none takes over 10 s.

**Not reproduced:** the crafted XXE, billion-laughs and `file://` relationship
cases (docx, odt, xlsx, pptx, epub, feeds, SVG, XHTML) all converted without
reading a local file or expanding entities; the encoding cases (UTF-16 with and
without BOM, UTF-32, Shift-JIS, GBK) decoded or were refused as binary.

## Medium

### M1. Crawler: failures vanished, error pages became content

**Where:** `crates/core/src/parsers/crawler.rs`. Every failure (guard refusal,
network error, oversized page, parse error) became an empty string and was
dropped; the status code was never read.
**Repro (v1.16.0):** a local site whose index links to a missing page and a 404:
```bash
FILEGOBLIN_ALLOW_LOCAL_URLS=1 filegoblin --horde --json -q http://127.0.0.1:8770/
# /missing.html and /boom appear as "Error response ... Error code: 404" documents, error: null
```
**Fix:** `fetch_page` returns the reason for every way of not getting a page;
non-2xx are errors; failures are in-band `GobbleDocument::failed` records (so
`--json` lists them and stderr says why). Also counted links into `in_flight`
before sending them: the old order (send, then increment) let the main loop finish
a received link and see `in_flight == 0` while the page's other links were still
queued, ending the crawl early. That race was found by reading and not reproduced.
**Test:** `parsers::crawler::tests::errors_are_reported_and_charsets_honoured`.

### M2. Crawler ignored the page charset

Bodies went through `String::from_utf8_lossy`; the single-URL path honoured
`Content-Type` and `<meta charset>`. **Repro:** the same site with a
`<meta charset="iso-8859-1">` index: `Caf� cr�me br�l�e` under `--horde`, `Café
crème brûlée` without. **Fix:** the crawler decodes through `FetchedBody::decode_text`;
`application/xhtml+xml` pages are read too. Same test as M1.

### M3. Archive members were not named under their archive

**Where:** `parsers/archive.rs` `extract_archive`. The remapping to
`{archive}/{path}` only fired on a temp-dir prefix the walk never produces (the ADR's
2026-10-22 entry noted it as a known gap). The consequence was worse than cosmetic:
**Repro (v1.16.0):** `a.zip` and `b.zip`, each holding a `README.md`:
`filegoblin a.zip b.zip --split --write out` leaves one `out/README.md`, containing
b's text, exit 0.
**Fix:** members are `{archive}/{inner path}`; an archive fetched over HTTP is
named by its URL. **Tests:** `parsers::archive::tests::members_are_named_under_their_archive`;
`golden::plain_tar_is_extracted` updated.

### M4. `--split` overwrote colliding names

**Where:** `crates/core/src/formatters.rs` (both split writers).
`sanitize_split_path` maps `notes` and `notes.md` to `notes.md`, and `A.txt`/`a.txt`
are one file on macOS and Windows. **Repro (v1.16.0):** a directory with `notes` and
`notes.md`: `filegoblin --horde dir --split --write out` leaves only `# md notes`.
**Fix:** a second document that would take a used name (compared
case-insensitively) gets `name.2.md`. **Test:**
`format_and_chunking::split_never_overwrites_one_document_with_another`.

### M5. Large documents held twice on the streaming path

**Repro (v1.16.0):** 1.5 GB of `a` gzipped to 6.8 MB:
`/usr/bin/time -l filegoblin -q big.log.gz` → 3.18 GB peak RSS.

Two copies, at different moments. `parsers/archive.rs` `extract_single_gz` read the
whole member (up to the 2 GiB cap) into a `Vec` before writing it out; that buffer
was freed before conversion, so it was not the peak. The peak was
`formatters.rs` `stream_and_output`, which built each document's whole rendered
block (`render_block`, a `format!` of the content) before writing it, so every
streamed document briefly existed twice — for any large input, not only archives.

**Fix:** the gzip member is inflated straight to disk under the same cap, and the
streaming writer writes the breadcrumb and the content directly (`write_block`).
After: 1.59 GB peak, the size of the document itself (debug build).
**Tests:** `parsers::archive::tests::single_gzip_cap_counts_actual_bytes`,
`formatters::tests::write_block_matches_render_block`; the goldens cover the bytes.
**Still true:** an archive may hold up to 2 GiB of text and that much is read into
memory to convert it; a per-member size limit would be the next step.

### M6. `FILEGOBLIN_ALLOW_LOCAL_URLS=0` opted in

`allow_local_urls()` used `var_os(..).is_some()`. **Repro (v1.16.0):**
`FILEGOBLIN_ALLOW_LOCAL_URLS=0 filegoblin http://127.0.0.1:8765/` fetched the
page. **Fix:** only `1`/`true`/`yes`/`on` opt in.
**Test:** `net::tests::only_a_yes_value_opts_in_to_local_urls`.

### M7. npm launcher and signals

`spawnSync` blocked the event loop, so a SIGTERM to the launcher (a supervisor, a
timeout, an agent harness) killed node and left the binary running; a binary killed
by a signal was reported as exit 1, the binary's own "unexpected error" code.
**Fix:** asynchronous spawn, SIGINT/SIGTERM/SIGHUP forwarded, exit `128 + signal`.
**Test:** `npm/test/launcher.test.js` ("SIGTERM reaches the binary…"; fails on the
old launcher).

### M8. `publish.yml` dispatch input

`run-id` was interpolated into a `run:` script, and a manual run published the
`registry-packages` artifact of *any* run, including a failed Release (that job
needs only `build`). The `pypi`/`npm` environments have no protection rules, and
trusted publishing is scoped to workflow + environment, not ref.
**Fix:** `run-id` goes through `env:`, must be numeric, and must name a successful
`Release` run. **Deferred:** restricting the environments to `main` and tags is a
repository setting (GitHub → Settings → Environments); pinning
`pypa/gh-action-pypi-publish@release/v1` and `npm install -g npm@latest` to exact
versions is left to the maintainer.

### M9. A missing platform archive was a warning

`scripts/package_registries.py` printed `warning: no archive for …`, then rewrote the
launcher's `optionalDependencies` to the platforms it had, publishing a release that
silently dropped one. **Fix:** fatal.

### M10. Flaky `browser::tests::discovery_and_js_render`

**Evidence from CI logs** (`gh run view --log-failed` over the last 60 Horde Check
runs): the ubuntu failures are both in the *Features* jobs (`ocr`, `vision`),
runs 38927545908 and 38935022599. Each failed with "Browser produced no usable DOM
… (page may block headless rendering)", and in each the lib test binary finished in
30.3–30.6 s: the 30 s wall-clock kill. Passing runs finish the same binary in ~3 s.
So Chrome wrote **nothing at all** for 30 s, and its own `--timeout=15000` never
fired: it stalled before the headless handler started navigating, not on the page.
(The Windows failures in the same period were the `/bin/ls` override, fixed in
`3eeb15a`.)

What the evidence does not settle is *what* Chrome waited on. The error hid it twice:
it blamed the page, and `browser_diagnostics` filtered out exactly the lines that
would say (anything mentioning `dbus`, `GPU` or `Vulkan`). The leading candidate is a
D-Bus/keyring lookup at startup (Chrome on Linux asks the secret service for the
profile's cookie key; D-Bus calls time out after 25 s, and 25 s + startup ≈ 30 s);
it could not be confirmed here (no Linux machine or container).
**Fixed:** `--password-store=basic` and `--use-mock-keychain` (Puppeteer's
defaults, which keep Chrome off the OS keyring); a wall-clock kill with no DOM now
says "made no progress … in 30s" with Chrome's stderr, falling back to the
unfiltered lines when the filter leaves nothing (checked with a fake browser that
prints a D-Bus error and sleeps). The test server's read timeout went from 2 s to
10 s (a request on a slow preconnect socket was being dropped). If it flakes again,
the failure message will now name the cause.

### M11. Linux glibc floor (deferred)

Wheels are `manylinux_2_35` (glibc ≥ 2.35); the npm platform packages carry no
`libc` field, and no document states the floor. Alpine or RHEL 8 users get an install
that "succeeds" and a binary that won't start. Recommendation: document the floor and
add `"libc": ["glibc"]` to the Linux platform packages. Not verified on a real musl
system.

### M12. Unpinned release inputs (deferred)

`scripts/vision_models.py` downloads model weights from `…/resolve/main/…` with no
revision or hash, and `vision-models.yml` installs torch/transformers/onnx unpinned,
then the weights ship permanently to three registries. Pin Hugging Face revisions
and SHA-256s, and the pip versions.

## Low

- **L1. `--chunk-size 0`** (or `0.0001k`) was accepted and emitted one chunk per
  word. Now a usage error (exit 2). Test: `format_and_chunking::a_zero_chunk_size_is_refused`.
- **L2. Crawl order** followed network completion; the markdown stream of the same
  site differed between runs. Pages are now sorted by URL (tree map first).
- **L3. A gzip whose payload starts with 1 KiB of NULs** read as an empty tar: exit
  4 with an empty tree. Zero tar entries now fall back to the single-file path.
  Test: `parsers::archive::tests::gzip_that_reads_as_an_empty_tar_is_a_single_file`.
- **L4. npm dev paths:** installed, `../../target/release/filegoblin` resolves to
  `node_modules/target/…`, checked before the platform package, so a dependency
  named `target` could supply the binary. Dev paths are now used only outside
  `node_modules`.
- **L5. Docs:** README said the model `.tar.gz` can be named in `FILEGOBLIN_MODELS`
  (only directories are read — `vision/models.rs`); said renamed files route
  correctly (only unknown or missing extensions do: a `.docx` renamed `.pdf` fails
  in the PDF parser); the Roadmap listed shipped/removed flags (`--plain`,
  `--format md`, `--chunk-tokens`, `--chunk-by`) and the shipped PyPI channel;
  the security notes described the old `--browser` limitation. RELEASING.md's pin
  list missed the README heading the release commit changes, and said "State as of
  v1.15.0". All corrected.
- **L6. `-q`:** per-file errors ("⚠️ Failed to sniff path …") still print under
  `-q`. `--help` says `-q` suppresses *auxiliary* output, GEMINI.md says it
  "silences stderr entirely". Errors on stderr are arguably what an agent wants, so
  this was left alone; one of the two documents should change.
- **L7. MCP:** a directory source gets `--horde` with no depth or size bound, so
  `convert("/")` walks the whole disk before the 1 MiB result cap applies; and with
  four conversions in flight a fifth runs on the read loop, blocking `ping` and
  cancellation — the thing the worker threads exist to prevent. Deferred: bounding
  it needs a decision on limits.
- **L8. `--chunk-overlap`** is silently ignored when it is ≥ `--chunk-size`
  (`--chunk-size 10 --chunk-overlap 50` → no overlap) and when there is no
  `--chunk-size`. The default (50) is larger than small chunk sizes, so making it a
  hard error needs a different default. Deferred.
- **L9. CI:** `bundled-models` is first compiled by the release's `full` job; no
  job uses `--locked`; third-party actions are referenced by mutable tags;
  `release.yml` interpolates `${{ github.ref_name }}` into scripts (dist-generated,
  needs push access to exploit). Deferred.

## For the PDF session

Not fixed here (`parsers/pdf*.rs` belong to the PDF session), except P2.

- **P1. The worker boundary fails open.** `pdf_worker.rs` `Pool::convert`: when a
  worker cannot be started, the PDF converts in the parent "instead", with a
  one-time warning that `-q` suppresses. Anything that makes `spawn` fail — the
  per-user process limit, `EMFILE`, the binary being replaced mid-run (the repo's own
  post-commit hook reinstalls `~/.cargo/bin/filegoblin`) — silently removes the
  isolation, and a hostile PDF can then take down or exhaust the parent.
  **Repro:** `(ulimit -u 1; exec filegoblin test_assets/financial_report.pdf)` →
  "could not start a PDF worker (… Resource temporarily unavailable (os error 35));
  converting PDFs in this process instead", exit 0. Recommendation: fail that file
  (an ordinary per-file error) instead, or at least never fall back silently.
- **P2. Memory budgets added up past the machine.** `default_memory_mb` gave each
  PDF worker half of physical RAM, `default_workers` is 2, and since `4283928` the
  CLI process itself may also hold half: one run could hold 1.5× the machine's RAM
  before any limit fired (two hostile PDFs in a `--horde` run or two MCP calls,
  plus the parent). Found by reading. **Fixed** (with the coordinating session's
  approval, the one change in `pdf_worker.rs`): the defaults are shares of one run
  budget, the machine's memory. The CLI keeps half; the workers split the other
  half (`memory::worker_share_mb`). Test:
  `memory::tests::the_cli_and_its_workers_share_one_budget`. An explicit
  `FILEGOBLIN_PDF_MEMORY_MB` still overrides.
- **P3.** The worker samples its footprint every 25 ms, so it can overshoot by
  whatever it allocates in that window (already in the ADR as open).

## Checked and fine

- Zip: `enclosed_name` refuses `../` and absolute entries; zip symlink entries are
  written as regular files; the 2 GiB cap counts actual inflated bytes; nested
  archives are refused. Tar: `unpack_in` refuses traversal; a hard link to
  `/etc/hosts` fails the archive with an error (exit 4).
- `--split` path sanitising keeps every file inside the split root.
- MCP: flag-like and NUL sources are refused in-band; results are truncated at
  1 MiB with a marker; `--browser` is never enabled for MCP.
- Invalid `--chunk`, `--compress`, `--format` values are usage errors (exit 2).
- Packaging (sub-review): every file that pins the version says 1.16.0;
  `dist plan --tag=v1.16.0` passes and rejects other tags; the PyPI launcher
  `execve`s on POSIX and passes exit codes, args and stdin through; npm/PyPI names
  and counts agree with RELEASING.md.

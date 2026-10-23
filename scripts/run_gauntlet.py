#!/usr/bin/env python3
"""Runs the adversarial gauntlet: every case in gauntlet_manifest.json through
filegoblin, scored two ways.

* **Canaries** (pass/fail): required strings, forbidden strings, syntax
  markers from the manifest.
* **Quality** (0–100): how close the output is to the hand-checked gold
  excerpts in test_assets/gauntlet/gold/ (see scripts/gauntlet_grade.py).
  Quality keeps moving after every canary passes, so it measures the impact
  of each change.

    python3 scripts/run_gauntlet.py                    # all cases, default --vision
    python3 scripts/run_gauntlet.py --vision off       # one mode
    python3 scripts/run_gauntlet.py --only FORM_1,TBL_1
    python3 scripts/run_gauntlet.py --jobs 4           # faster; latencies then overlap
    python3 scripts/run_gauntlet.py --regrade target/gauntlet/smart   # rescore saved outputs
    python3 scripts/run_gauntlet.py --compare target/gauntlet/off target/gauntlet/smart

Outputs and results.json go to target/gauntlet/<mode>/. GAUNTLET_SCORECARD.md
is rewritten by any run or regrade that covers every case (--no-scorecard to
skip); the hand-written block between its <!-- notes --> markers is kept.
"""
import argparse
import concurrent.futures
import json
import os
import re
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gauntlet_grade import grade_case, weakest  # noqa: E402
import gauntlet_formats  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GAUNTLET_DIR = os.path.join(ROOT, "test_assets", "gauntlet")
GOLD_DIR = os.path.join(GAUNTLET_DIR, "gold")
MANIFEST_PATH = os.path.join(GAUNTLET_DIR, "gauntlet_manifest.json")


def binary_path():
    # FILEGOBLIN_BIN overrides; otherwise the repo's builds, then PATH.
    if os.environ.get("FILEGOBLIN_BIN"):
        return os.environ["FILEGOBLIN_BIN"]
    for p in ("release", "debug"):
        cand = os.path.join(ROOT, "target", p, "filegoblin")
        if os.path.exists(cand):
            return cand
    return "filegoblin"


def canary_errors(item, stdout, exit_code, stderr):
    errors = []
    if exit_code != 0:
        errors.append(f"Non-zero exit ({exit_code}): {stderr.strip()[:100]}")
    low = stdout.lower()
    for canary in item.get("canaries_present", []):
        alts = canary if isinstance(canary, list) else [canary]
        if not any(a in stdout or a.lower() in low for a in alts):
            errors.append(f"Missing canary: '{alts[0]}'")
    for forbidden in item.get("forbidden_absent", []):
        pattern = r"(?<![A-Za-z0-9])" + re.escape(forbidden) + r"(?![A-Za-z0-9])"
        if re.search(pattern, stdout):
            errors.append(f"Found forbidden token: '{forbidden}'")
    # A list item means any one of its alternatives (equally valid conventions,
    # e.g. a merged header written once or repeated across its columns).
    for syn in item.get("syntax_contains", []):
        alts = syn if isinstance(syn, list) else [syn]
        if not any(a in stdout for a in alts):
            errors.append(f"Missing syntax marker: '{alts[0]}'")
    return errors


def fmt_q(q):
    return "  —  " if q is None else f"{q:5.1f}"


def summarize(results):
    passed = sum(r["passed"] for r in results)
    graded = [r["quality"] for r in results if r["quality"] is not None]
    quality = sum(graded) / len(graded) if graded else None
    return passed, quality, len(graded)


def run(args):
    with open(MANIFEST_PATH) as f:
        manifest = json.load(f)
    if args.only:
        wanted = set(args.only.split(","))
        manifest = [m for m in manifest if m["id"] in wanted]
    mode = args.vision or "default"
    if args.regrade and not args.vision:
        # The mode the saved outputs were made with.
        try:
            with open(os.path.join(args.regrade, "results.json"), encoding="utf-8") as fh:
                mode = json.load(fh).get("mode") or mode
        except (OSError, ValueError):
            pass
    out_dir = args.regrade or args.out or os.path.join(ROOT, "target", "gauntlet", mode)
    os.makedirs(out_dir, exist_ok=True)
    binary = binary_path()

    print("=" * 78)
    print(f"  FILEGOBLIN ADVERSARIAL GAUNTLET  ·  --vision {mode}  ·  {len(manifest)} cases")
    print("=" * 78)
    print(f"{'':6} {'ID':<11} {'quality':>7}  {'time':>8}  {'weakest component / first failure'}")

    def convert(item):
        """Runs one case (or reads its saved output with --regrade):
        (stdout, stderr, exit code, seconds, format checks)."""
        out_path = os.path.join(out_dir, f"{item['id']}.md")
        if args.regrade:
            if not os.path.exists(out_path):
                return None
            stdout = open(out_path, encoding="utf-8").read()
            meta_path = out_path + ".meta.json"
            meta = json.load(open(meta_path)) if os.path.exists(meta_path) else {}
            return stdout, meta.get("stderr", ""), meta.get("exit", 0), meta.get("elapsed", 0.0), None
        cmd = [binary, os.path.join(GAUNTLET_DIR, item["file"]), "-q"]
        if args.vision:
            cmd += ["--vision", args.vision]
        start = time.time()
        res = subprocess.run(cmd, capture_output=True, text=True)
        elapsed = time.time() - start
        with open(out_path, "w", encoding="utf-8") as fh:
            fh.write(res.stdout)
        with open(out_path + ".meta.json", "w") as fh:
            json.dump({"exit": res.returncode, "stderr": res.stderr[-2000:], "elapsed": elapsed}, fh)
        fmt = check_formats(binary, item, res.stdout, args.formats, out_dir) if args.formats else None
        return res.stdout, res.stderr, res.returncode, elapsed, fmt

    # Cases run one at a time by default so the latencies mean something;
    # --jobs N trades that for a faster run.
    pool = concurrent.futures.ThreadPoolExecutor(max(1, args.jobs))
    converted = pool.map(convert, manifest)  # in order, as each case finishes

    results = []
    for item, done in zip(manifest, converted):
        if done is None:
            continue
        case_id = item["id"]
        stdout, stderr, exit_code, elapsed, fmt = done
        errors = canary_errors(item, stdout, exit_code, stderr)
        grade = grade_case(stdout, os.path.join(GOLD_DIR, case_id))
        quality = None if grade is None else grade["score"]
        passed = not errors
        status = "\033[92mPASS\033[0m" if passed else "\033[91mFAIL\033[0m"
        note = errors[0] if errors else weakest(grade)
        print(f"[{status}] {case_id:<11} {fmt_q(quality):>7}  {elapsed:7.1f}s  {note[:60]}")
        if fmt is not None:
            print(f"        formats {'ok  ' if fmt['pass'] else 'FAIL'} plain cov {100 * fmt['plain']['coverage']:.1f}% "
                  f"residue {fmt['plain']['residue']} · chunks {fmt['chunks'].get('chunks', 0)} cov "
                  f"{100 * fmt['chunks'].get('coverage', 0):.1f}% oversize {fmt['chunks'].get('oversize', '?')} "
                  f"broken {fmt['chunks'].get('broken', '?')} {fmt['chunks'].get('error', '')}")
        results.append({
            "id": case_id, "file": item["file"], "category": item["category"],
            "description": item["description"], "passed": passed, "errors": errors,
            "elapsed_ms": elapsed * 1000, "quality": quality,
            "components": grade["components"] if grade else {},
            "leaks": grade["leaks"] if grade else [],
            "excerpts": grade["excerpts"] if grade else {},
            "formats": fmt,
        })

    pool.shutdown()
    passed, quality, n_graded = summarize(results)
    fmts = [r["formats"] for r in results if r.get("formats")]
    print("=" * 78)
    if fmts:
        print(f"FORMATS: {sum(f['pass'] for f in fmts)}/{len(fmts)} cases keep markdown content and "
              f"format guarantees in --format plain and --chunk-size {args.formats} --format json")
    print(f"CANARIES: {passed}/{len(results)} ({100 * passed / max(1, len(results)):.1f}%)   "
          f"QUALITY: {fmt_q(quality).strip()} (mean of {n_graded} graded cases)")
    print("=" * 78)

    with open(os.path.join(out_dir, "results.json"), "w") as fh:
        json.dump({"mode": mode, "cases": results}, fh, indent=1)

    full_run = not args.only and len(results) == len(json.load(open(MANIFEST_PATH)))
    if full_run and not args.no_scorecard:
        write_scorecard(results, mode)
    return 0 if passed == len(results) else 1


def check_formats(binary, item, markdown, size, out_dir):
    """Runs the case through --format plain and --chunk-size/--format json
    (always --vision off: the format layer does not depend on the PDF engine,
    and the markdown it is compared with must come from the same engine)."""
    src = os.path.join(GAUNTLET_DIR, item["file"])
    base = [binary, src, "-q", "--vision", "off"]
    md = subprocess.run(base, capture_output=True, text=True).stdout
    plain = subprocess.run(base + ["--format", "plain"], capture_output=True, text=True).stdout
    chunks = subprocess.run(base + ["--chunk-size", str(size), "--format", "json"],
                            capture_output=True, text=True).stdout
    with open(os.path.join(out_dir, f"{item['id']}.chunks.json"), "w", encoding="utf-8") as fh:
        fh.write(chunks)
    p = gauntlet_formats.check_plain(md, plain)
    c = gauntlet_formats.check_chunks(md, chunks, size)
    return {"plain": p, "chunks": c, "pass": gauntlet_formats.passes(p, c)}


NOTES_START, NOTES_END = "<!-- notes -->", "<!-- /notes -->"


def scorecard_notes(path):
    """The hand-written block between the notes markers of the current
    scorecard (measurement conditions, history), kept across rewrites."""
    try:
        text = open(path, encoding="utf-8").read()
    except OSError:
        return ""
    start, end = text.find(NOTES_START), text.find(NOTES_END)
    if start < 0 or end < start:
        return ""
    return text[start:end + len(NOTES_END)]


def write_scorecard(results, mode):
    passed, quality, n_graded = summarize(results)
    path = os.path.join(ROOT, "GAUNTLET_SCORECARD.md")
    notes = scorecard_notes(path)
    with open(path, "w") as f:
        f.write("# Filegoblin Adversarial Gauntlet Scorecard\n\n")
        f.write(f"Mode: `--vision {mode}`\n\n")
        f.write(f"**Canaries:** {passed}/{len(results)} ({100 * passed / len(results):.1f}%) · ")
        f.write(f"**Quality:** {fmt_q(quality).strip()} / 100 (mean over {n_graded} cases with gold excerpts)\n\n")
        if notes:
            f.write(notes + "\n\n")
        f.write("Quality is graded against hand-checked gold excerpts in `test_assets/gauntlet/gold/`; "
                "components are completeness, precision, order, tables, headings and math "
                "(see `scripts/gauntlet_grade.py`).\n\n")
        f.write("| ID | Category | Canaries | Quality | Weakest | Latency | First failure |\n")
        f.write("| :--- | :--- | :--- | ---: | :--- | ---: | :--- |\n")
        for r in results:
            status = "✅ PASS" if r["passed"] else "❌ FAIL"
            grade = {"components": r["components"], "leaks": r["leaks"]} if r["components"] else None
            f.write(f"| **{r['id']}** | {r['category']} | {status} | {fmt_q(r['quality']).strip()} | "
                    f"{weakest(grade) if grade else ''} | {r['elapsed_ms'] / 1000:.1f} s | "
                    f"{r['errors'][0] if r['errors'] else ''} |\n")
    print(f"Scorecard written to: {path}")


def compare(a_dir, b_dir):
    a = {r["id"]: r for r in json.load(open(os.path.join(a_dir, "results.json")))["cases"]}
    bj = json.load(open(os.path.join(b_dir, "results.json")))
    b = {r["id"]: r for r in bj["cases"]}
    print(f"{'ID':<11} {'A':>6} {'B':>6} {'Δ':>6}   canaries A→B")
    deltas = []
    for cid in b:
        if cid not in a:
            continue
        qa, qb = a[cid]["quality"], b[cid]["quality"]
        d = None if qa is None or qb is None else qb - qa
        if d is not None:
            deltas.append(d)
        mark = "" if a[cid]["passed"] == b[cid]["passed"] else ("  FIXED" if b[cid]["passed"] else "  BROKE")
        print(f"{cid:<11} {fmt_q(qa)} {fmt_q(qb)} {'' if d is None else f'{d:+6.1f}'}   "
              f"{'P' if a[cid]['passed'] else 'F'}→{'P' if b[cid]['passed'] else 'F'}{mark}")
    if deltas:
        print(f"mean Δ quality: {sum(deltas) / len(deltas):+.2f} over {len(deltas)} cases")
    return 0


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--vision", choices=["off", "smart", "all"], help="passed to filegoblin")
    ap.add_argument("--only", help="comma-separated case IDs")
    ap.add_argument("--out", help="where outputs and results.json go")
    ap.add_argument("--regrade", metavar="DIR", help="rescore outputs saved in DIR instead of running")
    ap.add_argument("--compare", nargs=2, metavar=("A", "B"), help="per-case quality deltas between two runs")
    ap.add_argument("--formats", type=int, nargs="?", const=512, metavar="TOKENS",
                    help="also check --format plain and --chunk-size TOKENS --format json (default 512)")
    ap.add_argument("--no-scorecard", action="store_true")
    ap.add_argument("--jobs", type=int, default=1,
                    help="cases converted at once (default 1, which keeps latencies comparable)")
    a = ap.parse_args()
    sys.exit(compare(*a.compare) if a.compare else run(a))

#!/usr/bin/env python3
"""olmOCR-bench: the external PDF benchmark, run and scored here.

olmOCR-bench (allenai, ODC-BY) is 1,403 single-page PDFs in seven categories
with 7,010 machine-checked tests: arXiv math, headers/footers, long tiny
text, multi-column order, old scans, old scans with math, tables. Unlike the
gauntlet, no gold here was written for filegoblin, so it is the check that a
change helps PDFs in general rather than the gauntlet's documents.

Setup (once):
    git clone https://github.com/allenai/olmocr ~/src/olmocr
    python3 -m venv ~/.venvs/olmocr && ~/.venvs/olmocr/bin/pip install -e "~/src/olmocr[bench]"
    ~/.venvs/olmocr/bin/playwright install chromium        # the math tests render KaTeX
    huggingface-cli download allenai/olmOCR-bench --repo-type dataset --local-dir ~/data/olmOCR-bench
    export OLMOCR_BENCH=~/data/olmOCR-bench/bench_data OLMOCR_REPO=~/src/olmocr

Usage (run the scorer with the venv's python):
    scripts/olmocr_bench.py run NAME [--split dev|test|all] [--jobs 6] [-- extra filegoblin args]
    scripts/olmocr_bench.py score NAME [--split ...] [--save results.json]
    scripts/olmocr_bench.py compare A B [--split ...]     # per-category deltas and tests gained/lost

Outputs go to $OLMOCR_BENCH/NAME/<category>/<stem>_pg1_repeat1.md, the layout
the official scorer reads. The overall score is the mean of the per-category
pass rates over the 7 categories plus "baseline" (the official definition).

The split is fixed and stratified: in each category the PDFs are ordered by
the SHA-1 of their path and the first eighth (at least 6) form "dev". Tune on
dev; report "test", which a change was never tuned on.
"""
import argparse
import concurrent.futures as cf
import glob
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CATEGORIES = ["arxiv_math", "headers_footers", "long_tiny_text", "multi_column",
              "old_scans", "old_scans_math", "table_tests", "baseline"]


def bench_dir():
    d = os.environ.get("OLMOCR_BENCH")
    if not d or not os.path.isdir(os.path.join(d, "pdfs")):
        sys.exit("Set OLMOCR_BENCH to olmOCR-bench's bench_data directory (see the setup notes in this script).")
    return d


def all_pdfs(bench):
    base = os.path.join(bench, "pdfs")
    return sorted(os.path.relpath(p, base) for p in glob.glob(os.path.join(base, "**", "*.pdf"), recursive=True))


def split(pdfs, which):
    if which == "all":
        return pdfs
    by_cat = {}
    for p in pdfs:
        by_cat.setdefault(p.split("/")[0], []).append(p)
    dev = set()
    for ps in by_cat.values():
        ps = sorted(ps, key=lambda p: hashlib.sha1(p.encode()).hexdigest())
        dev.update(ps[:max(6, math.ceil(len(ps) / 8))])
    return [p for p in pdfs if (p in dev) == (which == "dev")]


def binary():
    if os.environ.get("FILEGOBLIN_BIN"):
        return os.environ["FILEGOBLIN_BIN"]
    for p in ("release", "debug"):
        cand = os.path.join(ROOT, "target", p, "filegoblin")
        if os.path.exists(cand):
            return cand
    return "filegoblin"


def run(args):
    bench = bench_dir()
    pdfs = split(all_pdfs(bench), args.split)
    out_root = os.path.join(bench, args.name)
    cmd = [binary(), "-q"] + (args.extra or ["--vision", "smart"])
    t0 = time.time()

    def one(rel):
        out = os.path.join(out_root, os.path.splitext(rel)[0] + "_pg1_repeat1.md")
        if os.path.exists(out) and not args.force:
            return rel, 0.0, None
        os.makedirs(os.path.dirname(out), exist_ok=True)
        start = time.time()
        try:
            r = subprocess.run(cmd + [os.path.join(bench, "pdfs", rel)], capture_output=True, text=True, timeout=900)
            text, err = r.stdout, (None if r.returncode == 0 else f"exit {r.returncode}: {r.stderr.strip()[-200:]}")
        except subprocess.TimeoutExpired:
            text, err = "", "timeout"
        text = re.sub(r"^// --- FILE_START:.*---\n", "", text)
        with open(out, "w", encoding="utf-8") as fh:
            fh.write(text)
        return rel, time.time() - start, err

    errors = []
    with cf.ThreadPoolExecutor(args.jobs) as ex:
        for i, (rel, _, err) in enumerate(ex.map(one, pdfs)):
            if err:
                errors.append((rel, err))
            if i % 100 == 0:
                print(f"{i}/{len(pdfs)} {time.time() - t0:.0f}s", flush=True)
    print(f"converted {len(pdfs)} PDFs in {time.time() - t0:.0f}s; {len(errors)} non-zero exits")
    for rel, err in errors[:20]:
        print("  ", rel, err)
    return 0


def load_tests(bench):
    repo = os.environ.get("OLMOCR_REPO")
    if repo:
        sys.path.insert(0, os.path.expanduser(repo))
    from olmocr.bench.tests import BaselineTest, load_tests as load  # noqa: E402
    tests = []
    for path in sorted(glob.glob(os.path.join(bench, "*.jsonl"))):
        cat = os.path.basename(path)[:-len(".jsonl")]
        tests += [(cat, t) for t in load(path)]
    for pdf in sorted({t.pdf for _, t in tests}):
        tests.append(("baseline", BaselineTest(id=f"{pdf}_baseline", pdf=pdf, page=1, type="baseline")))
    return tests


def score(name, which, save=None, quiet=False):
    bench = bench_dir()
    keep = set(split(all_pdfs(bench), which))
    tests = [(c, t) for c, t in load_tests(bench) if t.pdf in keep]
    cache = {}

    def output(pdf):
        if pdf not in cache:
            path = os.path.join(bench, name, os.path.splitext(pdf)[0] + "_pg1_repeat1.md")
            cache[pdf] = open(path, encoding="utf-8").read() if os.path.exists(path) else None
        return cache[pdf]

    def one(ct):
        cat, t = ct
        md = output(t.pdf)
        if md is None:
            return cat, t, False, "no output"
        try:
            ok, why = t.run(md)
        except Exception as e:  # a test that crashes counts as failed
            ok, why = False, f"error: {e}"
        return cat, t, ok, why or ""

    results, per = {}, {}
    with cf.ThreadPoolExecutor(12) as ex:
        for cat, t, ok, why in ex.map(one, tests):
            results[t.id] = {"cat": cat, "pdf": t.pdf, "ok": ok, "why": why[:300]}
            per.setdefault(cat, []).append(ok)
    rates = {c: 100 * sum(per[c]) / len(per[c]) for c in CATEGORIES if c in per}
    overall = sum(rates.values()) / len(rates)
    if not quiet:
        for c, r in rates.items():
            print(f"{c:18s} {r:6.1f}%  ({sum(per[c])}/{len(per[c])})")
        print(f"{'overall':18s} {overall:6.2f}%   ({which}: {len(keep)} PDFs)")
    if save:
        with open(save, "w") as fh:
            json.dump({"rates": rates, "overall": overall, "tests": results}, fh)
    return rates, overall, results


def compare(a, b, which):
    ra, oa, ta = score(a, which, quiet=True)
    rb, ob, tb = score(b, which, quiet=True)
    print(f"{'category':18s} {a[:9]:>9s} {b[:9]:>9s}      Δ")
    for c in CATEGORIES:
        if c in ra and c in rb:
            print(f"{c:18s} {ra[c]:9.1f} {rb[c]:9.1f} {rb[c] - ra[c]:+6.1f}")
    print(f"{'overall':18s} {oa:9.2f} {ob:9.2f} {ob - oa:+6.2f}")
    gained = [k for k, v in tb.items() if v["ok"] and k in ta and not ta[k]["ok"]]
    lost = [k for k, v in tb.items() if not v["ok"] and k in ta and ta[k]["ok"]]
    print(f"tests gained {len(gained)}, lost {len(lost)}")
    for k in lost[:30]:
        print("  lost", k, tb[k]["why"][:120])
    return 0


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("name")
    r.add_argument("--split", default="all", choices=["dev", "test", "all"])
    r.add_argument("--jobs", type=int, default=6)
    r.add_argument("--force", action="store_true", help="convert again even when an output exists")
    r.add_argument("extra", nargs="*", help="filegoblin arguments after --, default --vision smart")
    s = sub.add_parser("score")
    s.add_argument("name")
    s.add_argument("--split", default="all", choices=["dev", "test", "all"])
    s.add_argument("--save")
    c = sub.add_parser("compare")
    c.add_argument("a")
    c.add_argument("b")
    c.add_argument("--split", default="all", choices=["dev", "test", "all"])
    a = ap.parse_args()
    if a.cmd == "run":
        sys.exit(run(a))
    if a.cmd == "score":
        score(a.name, a.split, a.save)
        sys.exit(0)
    sys.exit(compare(a.a, a.b, a.split))

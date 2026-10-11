#!/usr/bin/env python3
"""PDF engine evaluation: gauntlet canaries + survival checks + reference fidelity.

The gauntlet alone rewards "make this substring appear". This harness adds the
checks that punish damage, so an improvement on one document cannot silently
corrupt others:

  1. Gauntlet   — the PDF cases from test_assets/gauntlet/gauntlet_manifest.json,
                  scored exactly as scripts/run_gauntlet.py scores them.
  2. Survival   — test_assets/pdf_survival/manifest.json (synthetic PDFs with exact
                  ground truth) plus curated phrases from real documents that must
                  come through verbatim (doses, citations, form references).
  3. Fidelity   — when `pdftotext` is installed, every PDF is also compared with it
                  as an independent reference using a neutral normaliser (lowercase,
                  alphanumeric tokens; no per-document rewrites):
                    word recall / precision   multiset overlap of tokens
                    4-gram recall             catches words glued or split apart
                    numeric survival          share of reference prose lines'
                                              "word NUMBER word" triples that survive

Usage:
  python3 scripts/pdf_eval.py [--bin PATH] [--label NAME] [--only ID,ID] [--extra DIR]

Outputs a table and writes target/pdf_eval/<label>/{summary.json,<id>.md}.
"""

import argparse
import collections
import json
import os
import re
import shutil
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GAUNTLET = os.path.join(ROOT, "test_assets", "gauntlet")
SURVIVAL = os.path.join(ROOT, "test_assets", "pdf_survival")

# Verbatim phrases from real documents (verified against pdftotext). These are
# not canaries for features; they are sentences that any correct extraction
# reproduces unchanged.
REAL_SURVIVAL = {
    "FDA_1": ["KEYTRUDA 200 mg every 3 weeks", "axitinib 5 mg orally twice daily",
              "lenvatinib 20 mg orally once daily", "2 mg/kg every 3 weeks"],
    "SCOTUS_1": ["304 U. S. 64, 78 (1938)", "559 U. S. 393, 398 (2010)", "550 U. S. 544, 570 (2007)"],
    "FORM_1": ["Form 1040 (2025)", "Subtract line 10 from line 9"],
    "TECH_1": ["up to 150 Mbps", "up to 80 MHz"],
}


def norm_ws(s):
    s = s.replace("**", "").replace("__", "")
    return re.sub(r"\s+", " ", s).strip()


TOKEN_RE = re.compile(r"[^\W_]+", re.UNICODE)


def tokens(s):
    return [t.lower() for t in TOKEN_RE.findall(s)]


def multiset_overlap(a, b):
    ca, cb = collections.Counter(a), collections.Counter(b)
    return sum((ca & cb).values())


def ngrams(toks, n):
    return [tuple(toks[i:i + n]) for i in range(len(toks) - n + 1)]


NUM_TRIPLE_RE = re.compile(r"\b([A-Za-z]{2,}) (\d[\d,.]*) ([A-Za-z]{2,})\b")


def numeric_triples(ref_text):
    """'word NUMBER word' triples from prose-looking reference lines."""
    out = []
    for line in ref_text.splitlines():
        words = line.split()
        if len(words) < 8:
            continue
        alpha = sum(1 for w in words if re.fullmatch(r"[a-z][a-z,.;:'()-]*", w))
        if alpha < len(words) * 0.6:
            continue
        for m in NUM_TRIPLE_RE.finditer(line):
            out.append(m.group(0))
    return out


def run_bin(binary, path, timeout=1800):
    t0 = time.time()
    try:
        res = subprocess.run([binary, path, "-q"], capture_output=True, text=True, timeout=timeout)
        out, code, err = res.stdout, res.returncode, res.stderr
    except subprocess.TimeoutExpired:
        out, code, err = "", -1, "timeout"
    return out, code, err, time.time() - t0


def pdftotext(path, cache_dir):
    if not shutil.which("pdftotext"):
        return None
    os.makedirs(cache_dir, exist_ok=True)
    key = os.path.join(cache_dir, re.sub(r"[^A-Za-z0-9_.-]", "_", os.path.relpath(path, ROOT)) + ".txt")
    if not os.path.exists(key):
        subprocess.run(["pdftotext", path, key], capture_output=True)
    if not os.path.exists(key):
        return None
    with open(key, errors="replace") as f:
        return f.read()


def fidelity(out, ref):
    if ref is None or len(tokens(ref)) < 50:
        return {}
    rt, ot = tokens(ref), tokens(out)
    inter = multiset_overlap(rt, ot)
    r4, o4 = ngrams(rt, 4), ngrams(ot, 4)
    triples = numeric_triples(ref)
    flat_out = norm_ws(out)
    surv = sum(1 for t in triples if t in flat_out)
    return {
        "recall": inter / max(1, len(rt)),
        "precision": inter / max(1, len(ot)),
        "r4": multiset_overlap(r4, o4) / max(1, len(r4)),
        "num_surv": (surv / len(triples)) if triples else None,
        "num_n": len(triples),
    }


def check_survival(out, spec):
    flat = norm_ws(out)
    fails = []
    for p in spec.get("must_contain", []):
        if norm_ws(p) not in flat:
            fails.append(f"missing: {p}")
    for p in spec.get("must_not_contain", []):
        if norm_ws(p) in flat:
            fails.append(f"present: {p}")
    for a, b in spec.get("ordered", []):
        ia, ib = flat.find(norm_ws(a)), flat.find(norm_ws(b))
        if ia < 0 or ib < 0 or ia > ib:
            fails.append(f"order: {a!r} !< {b!r}")
    heading_lines = [norm_ws(l.lstrip("#")) for l in out.splitlines() if l.lstrip().startswith("#")]
    for h in spec.get("headings", []):
        if not any(norm_ws(h) in hl for hl in heading_lines):
            fails.append(f"heading: {h}")
    if spec.get("no_number_noise"):
        # Line-number gutters must not leak between words ("the 12 order").
        for m in re.finditer(r"\b[a-z]{2,} (\d{1,2}) [a-z]{2,}\b", flat):
            ctx = m.group(0)
            if not any(ctx in norm_ws(p) for p in spec.get("must_contain", [])) and int(m.group(1)) <= 28:
                fails.append(f"number noise: {ctx}")
                break
    for row in spec.get("table_cells", []):
        if not any(all(c in line for c in row) and "|" in line for line in out.splitlines()):
            fails.append(f"table row: {row}")
    total = (len(spec.get("must_contain", [])) + len(spec.get("must_not_contain", [])) +
             len(spec.get("ordered", [])) + len(spec.get("headings", [])) +
             (1 if spec.get("no_number_noise") else 0) + len(spec.get("table_cells", [])))
    return total - len(fails), total, fails


def gauntlet_check(out, code, item):
    errs = []
    if code != 0:
        errs.append(f"exit {code}")
    for c in item.get("canaries_present", []):
        if c not in out and c.lower() not in out.lower():
            errs.append(f"canary: {c}")
    for f in item.get("forbidden_absent", []):
        if re.search(r"(?<![A-Za-z0-9])" + re.escape(f) + r"(?![A-Za-z0-9])", out):
            errs.append(f"forbidden: {f[:30]!r}")
    for s in item.get("syntax_contains", []):
        if s not in out:
            errs.append(f"syntax: {s[:30]!r}")
    return errs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=os.environ.get("FILEGOBLIN_BIN",
                                                   os.path.join(ROOT, "target", "debug", "filegoblin")))
    ap.add_argument("--label", default="latest")
    ap.add_argument("--only", default="")
    ap.add_argument("--extra", default="", help="directory of extra held-out PDFs (fidelity only)")
    ap.add_argument("--skip", default="", help="comma-separated ids to skip")
    args = ap.parse_args()

    only = set(filter(None, args.only.split(",")))
    skip = set(filter(None, args.skip.split(",")))
    outdir = os.path.join(ROOT, "target", "pdf_eval", args.label)
    refdir = os.path.join(ROOT, "target", "pdf_eval", "_ref")
    os.makedirs(outdir, exist_ok=True)

    cases = []
    for item in json.load(open(os.path.join(GAUNTLET, "gauntlet_manifest.json"))):
        if item["file"].lower().endswith(".pdf"):
            cases.append(("gauntlet", item["id"], os.path.join(GAUNTLET, item["file"]), item))
    for spec in json.load(open(os.path.join(SURVIVAL, "manifest.json"))):
        cid = "SV_" + os.path.splitext(spec["file"])[0]
        cases.append(("survival", cid, os.path.join(SURVIVAL, spec["file"]), spec))
    if args.extra:
        for fn in sorted(os.listdir(args.extra)):
            if fn.lower().endswith(".pdf"):
                cases.append(("extra", "HO_" + os.path.splitext(fn)[0][:24], os.path.join(args.extra, fn), {}))

    rows, summary = [], {}
    for kind, cid, path, item in cases:
        if (only and cid not in only) or cid in skip:
            continue
        out, code, err, dt = run_bin(args.bin, path)
        with open(os.path.join(outdir, cid + ".md"), "w") as f:
            f.write(out)
        rec = {"kind": kind, "time": dt, "exit": code}
        if kind == "gauntlet":
            errs = gauntlet_check(out, code, item)
            rec["gauntlet_pass"] = not errs
            rec["gauntlet_errors"] = errs
            if cid in REAL_SURVIVAL:
                spec = {"must_contain": REAL_SURVIVAL[cid]}
                ok, tot, fails = check_survival(out, spec)
                rec["surv"] = (ok, tot)
                rec["surv_fails"] = fails
        elif kind == "survival":
            ok, tot, fails = check_survival(out, item)
            rec["surv"] = (ok, tot)
            rec["surv_fails"] = fails
        rec.update(fidelity(out, pdftotext(path, refdir)))
        summary[cid] = rec
        g = "" if "gauntlet_pass" not in rec else ("PASS" if rec["gauntlet_pass"] else "FAIL")
        s = "" if "surv" not in rec else f"{rec['surv'][0]}/{rec['surv'][1]}"
        fid = ""
        if "recall" in rec:
            ns = "-" if rec["num_surv"] is None else f"{rec['num_surv']*100:5.1f}%"
            fid = f"R {rec['recall']*100:5.1f}  P {rec['precision']*100:5.1f}  R4 {rec['r4']*100:5.1f}  num {ns}"
        line = f"{cid:<34} {g:<5} {s:<6} {fid:<48} {dt:7.1f}s"
        print(line, flush=True)
        for e in rec.get("gauntlet_errors", [])[:3] + rec.get("surv_fails", [])[:4]:
            print(f"      - {e}", flush=True)

    with open(os.path.join(outdir, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1)

    g = [r for r in summary.values() if "gauntlet_pass" in r]
    s = [r["surv"] for r in summary.values() if "surv" in r]
    fids = [r for r in summary.values() if "recall" in r]
    nums = [r for r in fids if r.get("num_surv") is not None]
    print("=" * 100)
    print(f"gauntlet PDF cases: {sum(r['gauntlet_pass'] for r in g)}/{len(g)}   "
          f"survival checks: {sum(a for a, _ in s)}/{sum(b for _, b in s)}")
    if fids:
        avg = lambda k, rs: sum(r[k] for r in rs) / len(rs)
        print(f"fidelity vs pdftotext (mean over {len(fids)}): recall {avg('recall', fids)*100:.1f}  "
              f"precision {avg('precision', fids)*100:.1f}  4-gram {avg('r4', fids)*100:.1f}  "
              f"numeric {avg('num_surv', nums)*100 if nums else 0:.1f}")


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""A page-level scan evaluation built from Library of Congress crowd
transcriptions (By the People, crowd.loc.gov), scored by character error rate.

Each page is a manuscript or print scan whose gold text is the volunteers'
reviewed transcription, published with the image in the Library's "Selected
Datasets" collection (no known copyright restrictions). The campaigns favour
ones transcribed after the page reader (LightOnOCR-2, January 2026) was
trained, so its training data is unlikely to hold these transcriptions.

    scripts/loc_eval.py fetch                  # dataset CSVs into $LOC_EVAL/datasets
    scripts/loc_eval.py build                  # sample pages, download images, write PDFs + gold
    scripts/loc_eval.py run NAME [--split dev|test|all] [--jobs 3] [-- filegoblin args]
    scripts/loc_eval.py cloud NAME --provider claude|gemini --model M [--split ...]
    scripts/loc_eval.py score NAME [--split ...]
    scripts/loc_eval.py compare A B [--split ...]

Data lives in $LOC_EVAL (default ~/Library/Caches/fgv/loceval): `pdfs/`,
`gold/`, `manifest.json`, and one folder per run under `runs/`. `build`
refuses to overwrite a set marked FROZEN. Inspect dev pages freely; report
test scores only, as with the olmOCR-bench test split.

Metrics, after the same normalization of output and gold (markup stripped,
Unicode quotes and dashes folded, line-break hyphens joined, whitespace
collapsed): CER (corpus: total edits / total gold characters, and the median
page), case-folded CER, WER, and an order-free bag-of-words F1 that does not
punish a marginal note read in a different place.
"""
import argparse
import collections
import concurrent.futures as cf
import csv
import glob
import hashlib
import io
import json
import math
import os
import re
import statistics
import subprocess
import sys
import time
import unicodedata
import urllib.request
import zipfile

sys.path.insert(0, os.path.dirname(__file__))
import engines  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DATA = os.path.expanduser(os.environ.get("LOC_EVAL", "~/Library/Caches/fgv/loceval"))
UA = {"User-Agent": "filegoblin-eval/0.1"}

# (name, dataset item id, pages, what it is, transcribed)
CAMPAIGNS = [
    ("fleetwood", "2025474987", 70, "Christian Fleetwood diaries and letters, handwritten", "2026-01/02"),
    ("goldsborough", "2025474267", 80, "Louis M. Goldsborough naval correspondence, handwritten", "2025-07..2026-04"),
    ("antrim", "2023514955", 40, "Benajah Jay Antrim journals, handwritten", "2025-01/02"),
    ("wilson", "2025474985", 40, "Woodrow Wilson papers, suffrage file: typed and handwritten letters", "2024-08/09"),
    ("douglass", "2023514913", 30, "Frederick Douglass papers: letters, mostly handwritten", "2024-02..05"),
    ("print", "2025474991", 40, "African American Perspectives in Print: printed pamphlets", "2025-02/03"),
]
# Pages per item (a diary volume, a correspondence folder) are capped at about
# twice the even share, so no single hand dominates a campaign.
# Pages whose gold does not match the image, found by flagging pages where
# both runs' output length or word overlap was far off and then comparing only
# the image with the gold. Excluded from scoring, kept in the manifest.
EXCLUDE = {
    "goldsborough-1370537": "gold is a different letter (Galveston, 1890s) from the page shown",
    "print-1249733": "gold covers part of the first of two columns",
    "fleetwood-1419385": "gold repeats a passage that is on the page once",
}
LONG_SIDE = 3000  # pixels; larger originals are scaled down by the image server
DPI = 200


def get(url, tries=4):
    for i in range(tries):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=120) as r:
                return r.read()
        except Exception:
            if i == tries - 1:
                raise
            time.sleep(5 * (i + 1))


def sha(s):
    return hashlib.sha1(s.encode()).hexdigest()


# ---------------------------------------------------------------- fetch/build

def fetch(_args):
    for name, item, *_ in CAMPAIGNS:
        d = os.path.join(DATA, "datasets", name)
        if glob.glob(os.path.join(d, "*.csv")):
            continue
        os.makedirs(d, exist_ok=True)
        meta = json.loads(get(f"https://www.loc.gov/item/{item}/?fo=json"))
        url = meta["resources"][0]["files"][0][0]["url"]
        with zipfile.ZipFile(io.BytesIO(get(url))) as z:
            for m in z.namelist():
                if m.endswith((".csv", ".txt")):
                    open(os.path.join(d, os.path.basename(m)), "wb").write(z.read(m))
        print(f"{name}: {url}")
        time.sleep(1)


def usable(text):
    """Pages whose gold can be trusted: some prose, no illegible marks (`?`) and
    no struck-through text (`[...]`); marginalia (`[*...*]`) are fine."""
    t = text.strip()
    return 200 <= len(t) <= 3000 and not re.search(r"\[(?!\*)|\?", t) and "nothing to transcribe" not in t.lower()


def sample(name, n):
    csv.field_size_limit(10**9)
    f = glob.glob(os.path.join(DATA, "datasets", name, "*.csv"))[0]
    rows = [r for r in csv.DictReader(open(f, encoding="utf-8-sig")) if r["DownloadUrl"] and usable(r["Transcription"])]
    rows.sort(key=lambda r: sha(r["AssetId"]))
    cap = max(2, math.ceil(2 * n / len({r["ItemId"] for r in rows})))
    per_item = collections.Counter()
    out = []
    for r in rows:
        if per_item[r["ItemId"]] < cap:
            per_item[r["ItemId"]] += 1
            out.append(r)
        if len(out) == n:
            break
    return out


def jpeg_pdf(jpeg, w, h, gray):
    """A one-page PDF showing `jpeg` (w x h px) at DPI, with the JPEG bytes as is."""
    pw, ph = w * 72 / DPI, h * 72 / DPI
    content = f"q {pw:.2f} 0 0 {ph:.2f} 0 0 cm /Im0 Do Q".encode()
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2f} {ph:.2f}] "
        f"/Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>".encode(),
        f"<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace "
        f"/{'DeviceGray' if gray else 'DeviceRGB'} /BitsPerComponent 8 /Filter /DCTDecode "
        f"/Length {len(jpeg)} >>\nstream\n".encode() + jpeg + b"\nendstream",
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream",
    ]
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offs = []
    for i, o in enumerate(objs, 1):
        offs.append(len(out))
        out += f"{i} 0 obj\n".encode() + o + b"\nendobj\n"
    x = len(out)
    out += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    out += b"".join(f"{o:010d} 00000 n \n".encode() for o in offs)
    out += f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n".encode()
    return bytes(out)


def build(args):
    from PIL import Image

    if os.path.exists(os.path.join(DATA, "FROZEN")) and not args.force:
        sys.exit(f"{DATA} is FROZEN; not rebuilding")
    for sub in ("pdfs", "gold", "images"):
        os.makedirs(os.path.join(DATA, sub), exist_ok=True)
    pages = []
    for name, item, n, what, when in CAMPAIGNS:
        rows = sample(name, n)
        # Whole items go to dev until it holds about a third of the pages, so
        # the same hand is never on both sides of the split.
        dev, held = set(), 0
        for it, k in sorted(collections.Counter(r["ItemId"] for r in rows).items(), key=lambda x: sha("split:" + x[0])):
            if held >= len(rows) / 3:
                break
            dev.add(it)
            held += k
        for r in rows:
            pid = f"{name}-{r['AssetId']}"
            pages.append({"id": pid, "campaign": name, "asset": r["Asset"], "asset_id": r["AssetId"],
                          "item": r["ItemId"], "image": r["DownloadUrl"], "dataset": item,
                          "split": "dev" if r["ItemId"] in dev else "test"})
            if pid in EXCLUDE:
                pages[-1]["excluded"] = EXCLUDE[pid]
            open(os.path.join(DATA, "gold", pid + ".txt"), "w", encoding="utf-8").write(r["Transcription"])
    for i, p in enumerate(pages):
        img = os.path.join(DATA, "images", p["id"] + ".jpg")
        if not os.path.exists(img):
            base = p["image"].split("/full/")[0]
            info = json.loads(get(base + "/info.json"))
            size = "full" if max(info["width"], info["height"]) <= LONG_SIDE else f"!{LONG_SIDE},{LONG_SIDE}"
            open(img, "wb").write(get(f"{base}/full/{size}/0/default.jpg"))
            time.sleep(0.5)
        data = open(img, "rb").read()
        im = Image.open(io.BytesIO(data))
        if im.mode not in ("L", "RGB") or im.format != "JPEG":
            buf = io.BytesIO()
            im.convert("RGB").save(buf, "JPEG", quality=95)
            data, im = buf.getvalue(), Image.open(buf)
        p["width"], p["height"] = im.size
        open(os.path.join(DATA, "pdfs", p["id"] + ".pdf"), "wb").write(jpeg_pdf(data, *im.size, im.mode == "L"))
        if i % 25 == 0:
            print(f"{i}/{len(pages)}", flush=True)
    manifest = {"built": time.strftime("%Y-%m-%d"), "campaigns": [dict(zip(("name", "dataset", "pages", "what", "transcribed"), c)) for c in CAMPAIGNS],
                "long_side": LONG_SIDE, "dpi": DPI, "pages": pages}
    json.dump(manifest, open(os.path.join(DATA, "manifest.json"), "w"), indent=1)
    c = collections.Counter((p["campaign"], p["split"]) for p in pages if not p.get("excluded"))
    for name, *_ in CAMPAIGNS:
        print(f"{name:13} dev {c[(name, 'dev')]:3}  test {c[(name, 'test')]:3}")
    print(f"{len(pages)} pages in {DATA}, {sum(1 for p in pages if p.get('excluded'))} excluded for bad gold")


def manifest():
    return json.load(open(os.path.join(DATA, "manifest.json")))


def pages(which):
    """Pages of a split ("dev", "test", "all"), or of a JSON list of page ids (a sample)."""
    if which not in ("dev", "test", "all"):
        ids = set(json.load(open(which)))
        return [p for p in manifest()["pages"] if not p.get("excluded") and p["id"] in ids]
    return [p for p in manifest()["pages"] if not p.get("excluded") and (which == "all" or p["split"] == which)]


# ------------------------------------------------------------------------ run

def binary():
    if os.environ.get("FILEGOBLIN_BIN"):
        return os.environ["FILEGOBLIN_BIN"]
    cand = os.path.join(ROOT, "target", "release", "filegoblin")
    return cand if os.path.exists(cand) else "filegoblin"


def run(args):
    out_root = os.path.join(DATA, "runs", args.name)
    os.makedirs(out_root, exist_ok=True)
    cmd = [binary(), "-q"] + (args.extra or ["--vision", "smart"])
    todo = pages(args.split)
    tpath = os.path.join(out_root, "timings.json")
    timings = json.load(open(tpath)) if os.path.exists(tpath) else {}
    t0 = time.time()

    def one(p):
        out = os.path.join(out_root, p["id"] + ".md")
        if os.path.exists(out) and not args.force:
            return None
        start = time.time()
        try:
            r = subprocess.run(cmd + [os.path.join(DATA, "pdfs", p["id"] + ".pdf")], capture_output=True,
                               text=True, timeout=args.timeout, env=engines.filegoblin_env())
            text, err = r.stdout, (None if r.returncode == 0 else f"exit {r.returncode}: {r.stderr.strip()[-200:]}")
        except subprocess.TimeoutExpired:
            text, err = "", "timeout"
        open(out, "w", encoding="utf-8").write(text)
        timings[p["id"]] = time.time() - start
        return (p["id"], err) if err else None

    errors = []
    with cf.ThreadPoolExecutor(args.jobs) as ex:
        for i, e in enumerate(ex.map(one, todo)):
            if e:
                errors.append(e)
            if i % 25 == 0:
                print(f"{i}/{len(todo)} {time.time() - t0:.0f}s", flush=True)
    json.dump(timings, open(tpath, "w"))
    open(os.path.join(out_root, "command.txt"), "w").write(" ".join(cmd) + "\n")
    print(f"converted {len(todo)} pages in {time.time() - t0:.0f}s; {len(errors)} errors")
    for e in errors[:20]:
        print("  ", *e)


# ---------------------------------------------------------------------- cloud

# What a person would ask a chat model for after attaching the PDF; the same
# words for every model.
CLOUD_PROMPT = ("Convert this PDF to Markdown. Transcribe all of its text exactly as written, in reading "
                "order. Do not summarize, correct, or add any commentary; output only the Markdown.")


def cloud_claude(model, pdf):
    import anthropic
    import base64
    client = anthropic.Anthropic(max_retries=6)
    r = client.messages.create(
        model=model, max_tokens=16000,
        messages=[{"role": "user", "content": [
            {"type": "document", "source": {"type": "base64", "media_type": "application/pdf",
                                            "data": base64.standard_b64encode(pdf).decode()}},
            {"type": "text", "text": CLOUD_PROMPT}]}])
    text = "".join(b.text for b in r.content if b.type == "text")
    err = None if r.stop_reason == "end_turn" else f"stop {r.stop_reason}"
    return text, err, {"in": r.usage.input_tokens, "out": r.usage.output_tokens}


def cloud_gemini(model, pdf):
    from google import genai
    from google.genai import types
    client = genai.Client()
    r = client.models.generate_content(
        model=model, contents=[types.Part.from_bytes(data=pdf, mime_type="application/pdf"), CLOUD_PROMPT])
    text = r.text or ""
    reason = r.candidates[0].finish_reason if r.candidates else None
    err = None if reason is None or str(reason).endswith("STOP") else f"finish {reason}"
    u = r.usage_metadata
    return text, err, {"in": u.prompt_token_count, "out": (u.candidates_token_count or 0) + (u.thoughts_token_count or 0)}


def cloud(args):
    """Each page's PDF, the file filegoblin reads, sent to a hosted model with CLOUD_PROMPT;
    written where `score` looks for a run. Keys come from the environment (ANTHROPIC_API_KEY,
    GEMINI_API_KEY) or the SDKs' own sign-in. Not yet run against the live APIs: the 2026-11-14
    comparison went through the web apps instead (docs/COMPARISON.md)."""
    call = {"claude": cloud_claude, "gemini": cloud_gemini}[args.provider]
    out_root = os.path.join(DATA, "runs", args.name)
    os.makedirs(out_root, exist_ok=True)
    todo = pages(args.split)[: args.limit or None]
    upath = os.path.join(out_root, "usage.json")
    usage = json.load(open(upath)) if os.path.exists(upath) else {}
    t0 = time.time()

    def one(p):
        out = os.path.join(out_root, p["id"] + ".md")
        if os.path.exists(out) and not args.force:
            return None
        pdf = open(os.path.join(DATA, "pdfs", p["id"] + ".pdf"), "rb").read()
        start = time.time()
        try:
            text, err, u = call(args.model, pdf)
        except Exception as e:  # recorded, not fatal: a refusal or outage is a failed page
            text, err, u = "", f"{type(e).__name__}: {str(e)[:200]}", {}
        open(out, "w", encoding="utf-8").write(text)
        usage[p["id"]] = dict(u, s=round(time.time() - start, 1), **({"error": err} if err else {}))
        return (p["id"], err) if err else None

    errors = []
    with cf.ThreadPoolExecutor(args.jobs) as ex:
        for i, e in enumerate(ex.map(one, todo)):
            if e:
                errors.append(e)
            if i % 25 == 0:
                print(f"{i}/{len(todo)} {time.time() - t0:.0f}s", flush=True)
    json.dump(usage, open(upath, "w"), indent=0)
    open(os.path.join(out_root, "command.txt"), "w").write(
        f"cloud {args.provider} {args.model}\nprompt: {CLOUD_PROMPT}\n")
    tin = sum(u.get("in", 0) for u in usage.values())
    tout = sum(u.get("out", 0) for u in usage.values())
    print(f"{len(todo)} pages in {time.time() - t0:.0f}s; {len(errors)} errors; tokens in {tin} out {tout}")
    for e in errors[:20]:
        print("  ", *e)


# ---------------------------------------------------------------------- score

FOLD = str.maketrans({"‘": "'", "’": "'", "‚": "'", "‛": "'", "′": "'",
                      "“": '"', "”": '"', "„": '"', "″": '"',
                      "–": "-", "—": "-", "‒": "-", "−": "-", "­": "",
                      "ſ": "s", " ": " "})


def clean_output(md):
    t = re.sub(r"^// --- FILE_START:.*---\n", "", md)
    t = re.sub(r"!\[[^\]]*\]\([^)]*\)", " ", t)          # images
    t = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", t)        # links
    t = re.sub(r"<br\s*/?>", "\n", t)
    t = re.sub(r"<[^>]+>", " ", t)                        # html tags
    t = re.sub(r"(?m)^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$", "", t)  # table rules
    t = t.replace("|", " ")
    t = re.sub(r"(?m)^\s{0,3}#{1,6}\s+", "", t)          # headings
    t = re.sub(r"(?m)^\s*>\s?", "", t)                    # quotes
    t = re.sub(r"(?m)^\s*(?:[-*+]|\d+[.)])\s+(?=\S)", "", t)  # list markers
    t = re.sub(r"(?m)^\s*(?:-{3,}|\*{3,}|_{3,})\s*$", "", t)  # rules
    t = re.sub(r"\*\*|__|(?<!\w)[*_]|[*_](?!\w)", "", t)  # emphasis
    t = t.replace("$$", " ").replace("$", "")
    t = re.sub(r"\\([\\`*_{}\[\]()#+\-.!|])", r"\1", t)   # markdown escapes
    return t


def clean_gold(t):
    return t.replace("[*", " ").replace("*]", " ")


def norm(t):
    t = unicodedata.normalize("NFKC", t).translate(FOLD)
    t = re.sub(r"(\w)-[ \t]*\n[ \t]*(\w)", r"\1\2", t)  # line-break hyphens
    return re.sub(r"\s+", " ", t).strip()


def words(t):
    return [w for w in re.sub(r"[^\w\s']", " ", t.lower()).split() if w.strip("'")]


def page_scores(pred, gold):
    from rapidfuzz.distance import Levenshtein

    p, g = norm(clean_output(pred)), norm(clean_gold(gold))
    pw, gw = words(p), words(g)
    cp, cg = collections.Counter(pw), collections.Counter(gw)
    hit = sum((cp & cg).values())
    f1 = 2 * hit / (len(pw) + len(gw)) if pw or gw else 1.0
    return {"chars": len(g), "edits": Levenshtein.distance(p, g), "edits_ci": Levenshtein.distance(p.lower(), g.lower()),
            "words": len(gw), "word_edits": Levenshtein.distance(pw, gw), "bow_f1": f1}


def scored(name, which):
    out = {}
    for p in pages(which):
        f = os.path.join(DATA, "runs", name, p["id"] + ".md")
        if not os.path.exists(f):
            continue
        gold = open(os.path.join(DATA, "gold", p["id"] + ".txt"), encoding="utf-8").read()
        out[p["id"]] = dict(page_scores(open(f, encoding="utf-8").read(), gold), campaign=p["campaign"])
    return out


def summary(rows):
    c = sum(r["chars"] for r in rows) or 1
    return {"pages": len(rows), "cer": 100 * sum(r["edits"] for r in rows) / c,
            "cer_ci": 100 * sum(r["edits_ci"] for r in rows) / c,
            "median": 100 * statistics.median(r["edits"] / max(1, r["chars"]) for r in rows) if rows else 0,
            "wer": 100 * sum(r["word_edits"] for r in rows) / (sum(r["words"] for r in rows) or 1),
            "bow_f1": 100 * statistics.mean(r["bow_f1"] for r in rows) if rows else 0}


def score(args):
    s = scored(args.name, args.split)
    want = len(pages(args.split))
    if len(s) < want:
        print(f"note: {want - len(s)} of {want} pages have no output in this run")
    tpath = os.path.join(DATA, "runs", args.name, "timings.json")
    t = json.load(open(tpath)) if os.path.exists(tpath) else {}
    print(f"{args.name} ({args.split})")
    print(f"{'campaign':13} {'pages':>5} {'CER':>6} {'CER-ci':>6} {'median':>6} {'WER':>6} {'BoW F1':>6} {'s/page':>6}")
    groups = [(c[0], [r for r in s.values() if r["campaign"] == c[0]]) for c in CAMPAIGNS] + [("ALL", list(s.values()))]
    for g, rows in groups:
        if not rows:
            continue
        m = summary(rows)
        ids = [k for k, r in s.items() if g == "ALL" or r["campaign"] == g]
        sec = statistics.mean(t[k] for k in ids if k in t) if any(k in t for k in ids) else float("nan")
        print(f"{g:13} {m['pages']:5} {m['cer']:6.2f} {m['cer_ci']:6.2f} {m['median']:6.2f} {m['wer']:6.2f} {m['bow_f1']:6.2f} {sec:6.1f}")
    json.dump(s, open(os.path.join(DATA, "runs", args.name, f"scores-{os.path.splitext(os.path.basename(args.split))[0]}.json"), "w"))


def compare(args):
    a, b = scored(args.a, args.split), scored(args.b, args.split)
    both = sorted(set(a) & set(b))
    print(f"{args.b} vs {args.a} ({args.split}, {len(both)} pages): CER per page, a change of more than 1 point counts")
    print(f"{'campaign':13} {'CER a':>6} {'CER b':>6} {'better':>6} {'worse':>6}")
    for g in [c[0] for c in CAMPAIGNS] + ["ALL"]:
        ks = [k for k in both if g == "ALL" or a[k]["campaign"] == g]
        if not ks:
            continue
        d = [100 * (b[k]["edits"] - a[k]["edits"]) / max(1, a[k]["chars"]) for k in ks]
        print(f"{g:13} {summary([a[k] for k in ks])['cer']:6.2f} {summary([b[k] for k in ks])['cer']:6.2f} "
              f"{sum(x < -1 for x in d):6} {sum(x > 1 for x in d):6}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("fetch")
    b = sub.add_parser("build")
    b.add_argument("--force", action="store_true")
    r = sub.add_parser("run")
    r.add_argument("name")
    r.add_argument("--split", default="dev", help="dev, test, all, or a JSON file of page ids")
    r.add_argument("--jobs", type=int, default=3)
    r.add_argument("--timeout", type=int, default=900)
    r.add_argument("--force", action="store_true")
    r.add_argument("extra", nargs="*")
    k = sub.add_parser("cloud")
    k.add_argument("name")
    k.add_argument("--provider", required=True, choices=["claude", "gemini"])
    k.add_argument("--model", required=True)
    k.add_argument("--split", default="dev", help="dev, test, all, or a JSON file of page ids")
    k.add_argument("--jobs", type=int, default=4)
    k.add_argument("--limit", type=int, default=0)
    k.add_argument("--force", action="store_true")
    s = sub.add_parser("score")
    s.add_argument("name")
    s.add_argument("--split", default="dev", help="dev, test, all, or a JSON file of page ids")
    c = sub.add_parser("compare")
    c.add_argument("a")
    c.add_argument("b")
    c.add_argument("--split", default="dev", help="dev, test, all, or a JSON file of page ids")
    args = ap.parse_args()
    {"fetch": fetch, "build": build, "run": run, "cloud": cloud, "score": score, "compare": compare}[args.cmd](args)


if __name__ == "__main__":
    main()

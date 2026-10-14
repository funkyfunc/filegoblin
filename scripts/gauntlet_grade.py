#!/usr/bin/env python3
"""Graded gauntlet scoring: how close is filegoblin's output to a hand-checked
reference?

Each case can have gold excerpts in `test_assets/gauntlet/gold/<ID>/*.md`
(conventions in that folder's README). For every excerpt the scorer finds
where it sits in the output and measures:

  completeness  share of the gold words present near that spot (multiset)
  precision     share of the output in that span that is gold, divided by
                how often its phrases repeat nearby: junk, duplicated text
                and leaked page furniture lower it
  order         share of the present words that are in the right order
                (longest common subsequence over present words)
  tables        per gold table row, share of its cells found in one output
                table row: catches merged, split and shifted cells
  headings      F1 of gold headings against output heading lines in the span
  math          per gold formula, similarity of the closest output formula

  text  = harmonic mean of completeness and precision
  score = weighted mean of text .40, order .15, tables .25, headings .10,
          math .10, over the components the excerpt has (text and order
          always; tables, headings and math only when the gold contains them)

A case scores the mean of its excerpts, minus 5 points per `absent` string
from `checks.json` found in the output (floored at 0). A case with checks but
no excerpts (copyrighted books) scores the share of absent strings kept out. Standard library
only, so it runs anywhere the gauntlet runs.

    python3 scripts/gauntlet_grade.py OUTPUT.md test_assets/gauntlet/gold/FIN_1
"""
from __future__ import annotations

import difflib
import json
import os
import re
import sys
import unicodedata
from collections import Counter, defaultdict

WEIGHTS = {"text": 0.40, "order": 0.15, "tables": 0.25, "headings": 0.10, "math": 0.10}

# ---------------------------------------------------------------- normalize

_UNI_MATH = {
    "≤": r" \le ", "≥": r" \ge ", "≠": r" \neq ", "×": r" \times ", "÷": r" \div ",
    "±": r" \pm ", "∞": r" \infty ", "∑": r" \sum ", "∏": r" \prod ", "∫": r" \int ",
    "√": r" \sqrt ", "→": r" \to ", "←": r" \leftarrow ", "⇒": r" \Rightarrow ",
    "∈": r" \in ", "∉": r" \notin ", "⊂": r" \subset ", "⊆": r" \subseteq ",
    "∪": r" \cup ", "∩": r" \cap ", "∀": r" \forall ", "∃": r" \exists ",
    "∂": r" \partial ", "∇": r" \nabla ", "≈": r" \approx ", "≡": r" \equiv ",
    "·": r" \cdot ", "…": "...", "α": r" \alpha ", "β": r" \beta ", "γ": r" \gamma ",
    "δ": r" \delta ", "ε": r" \epsilon ", "θ": r" \theta ", "λ": r" \lambda ",
    "μ": r" \mu ", "π": r" \pi ", "σ": r" \sigma ", "τ": r" \tau ", "φ": r" \phi ",
    "ω": r" \omega ", "Σ": r" \Sigma ", "Ω": r" \Omega ", "Δ": r" \Delta ",
}
_TOKEN_ALIASES = {"leq": "le", "geq": "ge", "ne": "neq", "varepsilon": "epsilon",
                  "varphi": "phi", "rightarrow": "to"}
_DASHES = dict.fromkeys(map(ord, "‐‑‒–—―−"), "-")
_QUOTES = {ord("‘"): "'", ord("’"): "'", ord("‚"): "'", ord("“"): '"', ord("”"): '"', ord("„"): '"'}


def _mermaid_labels(m: re.Match) -> str:
    # A diagram's content is its node and edge labels; ids and arrows are syntax.
    labels = re.findall(r'["\[(|{]"?([^"\])}|\n]+?)"?[\])}|]', m.group(1))
    return "\n\n".join(l.strip() for l in labels if l.strip())


def clean(text: str) -> str:
    """Output or gold → comparable text: no comments, no file banners,
    Mermaid diagrams reduced to their labels."""
    text = re.sub(r"```mermaid\n(.*?)```", _mermaid_labels, text, flags=re.S)
    text = re.sub(r"<!--.*?-->", " ", text, flags=re.S)
    text = re.sub(r"(?m)^// --- FILE_(START|END).*$", " ", text)
    text = unicodedata.normalize("NFKC", text).replace("\u00ad", "")
    text = text.translate(_DASHES).translate(_QUOTES)
    for k, v in _UNI_MATH.items():
        text = text.replace(k, v)
    return text


_WORD = re.compile(r"[^\W_]+(?:[.,'][^\W_]+)*")


# LaTeX presentation commands: how a formula is typeset, not what it says.
_TEX_FORMATTING = {"left", "right", "mathrm", "text", "textrm", "operatorname", "mathit",
                   "mathbf", "mathbb", "mathcal", "boldsymbol", "displaystyle", "textstyle",
                   "quad", "qquad", "big", "bigl", "bigr", "big", "bigg", "biggl", "biggr"}


def words(text: str) -> list[str]:
    out = []
    for w in _WORD.findall(text.lower()):
        w = _TOKEN_ALIASES.get(w, w)
        if w not in _TEX_FORMATTING:
            out.append(w)
    return out


def _row_cells(line: str) -> list[str] | None:
    s = line.strip()
    if not (s.startswith("|") and s.count("|") >= 3) or _is_sep_row(s):
        return None
    return [c.strip() for c in s.strip("|").split("|")]


def _collapse_spans(cells: list[str], above: list[str] | None) -> str:
    """A merged cell's text repeated across the columns or down the rows it
    covers reads the same as the text written once: both are fine
    conventions, so repeats count once."""
    if above is not None:
        cells = ["" if i < len(above) and c and c == above[i] else c for i, c in enumerate(cells)]
    kept = [c for i, c in enumerate(cells) if i == 0 or not c or c != cells[i - 1]]
    return " ".join(kept)


def _is_sep_row(line: str) -> bool:
    s = line.strip()
    return s.startswith("|") and set(s) <= set("|-: ")


def line_tokens(text: str) -> tuple[list[str], list[tuple[int, str]]]:
    """Tokens of the whole text, plus (first-token index, line) per line."""
    toks: list[str] = []
    lines = []
    above = None
    for line in text.splitlines():
        lines.append((len(toks), line))
        if _is_sep_row(line):
            continue
        cells = _row_cells(line)
        if cells is None:
            toks.extend(words(line))
            above = None
        else:
            toks.extend(words(_collapse_spans(cells, above)))
            above = cells
    return toks, lines


# ------------------------------------------------------------------- locate

def locate(gold: list[str], out: list[str], index: dict) -> tuple[int, int] | None:
    """Token range of the output most likely to hold the gold excerpt, or None."""
    n = len(gold)
    if n == 0 or not out:
        return None
    bucket = max(25, n // 4)
    votes: Counter = Counter()
    # Phrases (3 words) place long excerpts reliably; single words are the
    # fallback for short ones whose words the output separates (Mermaid
    # node ids, table pipes).
    for k in ((3, 1) if n >= 12 else (1,)):
        for i in range(n - k + 1):
            for p in index.get((k, tuple(gold[i:i + k])), ()):
                votes[(p - i) // bucket] += 1
        if votes:
            break
    if not votes:
        return None
    best = max(votes, key=lambda b: votes[b] + 0.5 * (votes[b - 1] + votes[b + 1]))
    start = best * bucket
    lo = max(0, start - n // 2 - 60)
    hi = min(len(out), start + n + n // 2 + 60)
    return lo, hi


def build_index(out: list[str]) -> dict:
    idx: dict = defaultdict(list)
    for k in (1, 3):
        for i in range(len(out) - k + 1):
            idx[(k, tuple(out[i:i + k]))].append(i)
    return idx


# ------------------------------------------------------------------ metrics

def text_scores(gold: list[str], window: list[str]) -> dict:
    sm = difflib.SequenceMatcher(None, gold, window, autojunk=False)
    blocks = [b for b in sm.get_matching_blocks() if b.size]
    lcs = sum(b.size for b in blocks)
    if not blocks:
        return {"completeness": 0.0, "precision": 0.0, "order": 0.0, "text": 0.0, "span": (0, 0)}
    # The span the excerpt occupies: from the first to the last matched run
    # of 2+ words (single-word matches at the edges are coincidences).
    solid = [b for b in blocks if b.size >= 2] or blocks
    s0, s1 = solid[0].b, solid[-1].b + solid[-1].size
    span = window[s0:s1]
    g, o = Counter(gold), Counter(span)
    present = sum((g & Counter(window)).values())
    in_span = sum((g & o).values())
    completeness = present / len(gold)
    precision = in_span / max(1, len(span))
    order = min(1.0, lcs / present) if present else 0.0
    text = 0.0 if completeness + precision == 0 else 2 * completeness * precision / (completeness + precision)
    return {"completeness": completeness, "precision": precision, "order": order, "text": text,
            "span": (s0, s1)}


def duplication(gold: list[str], out: list[str], a: int, b: int) -> float:
    """How many times, on average, the excerpt's phrases appear around its
    span (1.0 = once). Text repeated by a bad merge, a table cell copied into
    every column, or a page emitted twice all push it above 1."""
    n = len(gold)
    grams = Counter(tuple(gold[i:i + 3]) for i in range(n - 2))
    unique = [g for g, c in grams.items() if c == 1]
    if len(unique) < 5:
        return 1.0
    region = out[max(0, a - 2 * n):b + 2 * n]
    seen = Counter(tuple(region[i:i + 3]) for i in range(len(region) - 2))
    found = [min(seen[g], 4) for g in unique if seen[g]]
    return sum(found) / len(found) if found else 1.0


def _cell(c: str) -> str:
    c = re.sub(r"[*_`]", "", c.lower())
    return re.sub(r"[^\w.%$()\-,/]", "", c)


def parse_tables(lines: list[str]) -> list[list[list[str]]]:
    tables, cur = [], []
    for line in lines:
        s = line.strip()
        if s.startswith("|") and s.count("|") >= 2:
            if not _is_sep_row(s):
                cells = [_cell(c) for c in s.strip("|").split("|")]
                cur.append(cells)
        elif cur:
            tables.append(cur)
            cur = []
    if cur:
        tables.append(cur)
    return tables


def table_score(gold_tables, out_tables) -> float | None:
    rows = [[c for c in r if c] for t in gold_tables for r in t]
    rows = [r for r in rows if len(r) >= 2]
    if not rows:
        return None
    out_rows = [Counter(c for c in r if c) for t in out_tables for r in t]
    if not out_rows:
        return 0.0
    total = 0.0
    for r in rows:
        need = Counter(r)
        best = max(sum((need & o).values()) for o in out_rows)
        total += best / len(r)
    return total / len(rows)


def _heading_text(line: str) -> str | None:
    m = re.match(r"\s{0,3}#{1,6}\s+(.*)", line)
    return " ".join(words(m.group(1))) if m else None


def heading_score(gold_lines, out_lines) -> float | None:
    gold = [h for h in map(_heading_text, gold_lines) if h]
    if not gold:
        return None
    gold_text = {" ".join(words(l)) for l in gold_lines}
    out = [h for h in map(_heading_text, out_lines) if h]

    def match(h, pool):
        return any(difflib.SequenceMatcher(None, h, p).ratio() >= 0.85 for p in pool)

    recall = sum(match(h, out) for h in gold) / len(gold)
    # Output headings that are gold body text (not gold headings) are false.
    false = [h for h in out if not match(h, gold) and match(h, gold_text)]
    precision = (len(out) - len(false)) / len(out) if out else 0.0
    return 0.0 if recall + precision == 0 else 2 * recall * precision / (recall + precision)


_MATH = re.compile(r"\$\$(.+?)\$\$|\$(.+?)\$|\\\((.+?)\\\)|\\\[(.+?)\\\]", re.S)


def _norm_tex(t: str) -> str:
    t = re.sub(r"\\(left|right|big|Big|bigg|Bigg)\b", "", t)
    t = re.sub(r"\\[,;:! ]|\\quad|\\qquad|~", "", t)
    t = re.sub(r"\\(mathrm|text|operatorname|mathit|mathbf|boldsymbol|mathbb|mathcal)\s*", r"\\\1", t)
    t = re.sub(r"\\(leq)\b", r"\\le", t)
    t = re.sub(r"\\(geq)\b", r"\\ge", t)
    return re.sub(r"\s+", "", t)


def math_score(gold_text: str, out_text: str) -> float | None:
    gold = [_norm_tex(next(g for g in m.groups() if g)) for m in _MATH.finditer(gold_text)]
    gold = [g for g in gold if len(g) >= 2]
    if not gold:
        return None
    out = [_norm_tex(next(g for g in m.groups() if g)) for m in _MATH.finditer(out_text)]
    if not out:
        return 0.0
    total = 0.0
    for g in gold:
        total += max(difflib.SequenceMatcher(None, g, o, autojunk=False).ratio() for o in out)
    return total / len(gold)


# -------------------------------------------------------------------- grade

def grade_excerpt(gold_md: str, out_toks, out_lines, index) -> dict:
    gold_md = clean(gold_md)
    gtoks, glines = line_tokens(gold_md)
    gold_line_text = [l for _, l in glines]
    where = locate(gtoks, out_toks, index)
    if where is None:
        comps = {"text": 0.0, "order": 0.0, "completeness": 0.0, "precision": 0.0}
        span_lines: list[str] = []
    else:
        lo, hi = where
        ts = text_scores(gtoks, out_toks[lo:hi])
        s0, s1 = ts.pop("span")
        a, b = lo + s0, lo + s1
        dup = duplication(gtoks, out_toks, a, b)
        if dup > 1.0:
            ts["precision"] /= dup
            c, p = ts["completeness"], ts["precision"]
            ts["text"] = 0.0 if c + p == 0 else 2 * c * p / (c + p)
        comps = ts
        # Output lines overlapping the matched span, with a little margin so
        # a table or heading straddling the edge is still seen whole.
        margin = max(40, (b - a) // 5)
        span_lines = [l for i, (t0, l) in enumerate(out_lines)
                      if t0 <= b + margin and (out_lines[i + 1][0] if i + 1 < len(out_lines) else t0 + 1) >= a - margin]
    span_text = "\n".join(span_lines)
    extra = {
        "tables": table_score(parse_tables(gold_line_text), parse_tables(span_lines)),
        "headings": heading_score(gold_line_text, span_lines),
        "math": math_score(gold_md, span_text),
    }
    comps.update({k: v for k, v in extra.items() if v is not None})
    used = {k: comps[k] for k in WEIGHTS if k in comps}
    wsum = sum(WEIGHTS[k] for k in used)
    comps["score"] = sum(WEIGHTS[k] * v for k, v in used.items()) / wsum
    return comps


def grade_case(output: str, gold_dir: str) -> dict | None:
    """Scores 0–100 for one case, or None when it has no gold."""
    if not os.path.isdir(gold_dir):
        return None
    files = sorted(f for f in os.listdir(gold_dir) if f.endswith(".md"))
    checks_path = os.path.join(gold_dir, "checks.json")
    checks = json.load(open(checks_path)) if os.path.exists(checks_path) else {}
    if not files and not checks:
        return None
    out = clean(output)
    out_toks, out_lines = line_tokens(out)
    index = build_index(out_toks)
    excerpts = {}
    for f in files:
        with open(os.path.join(gold_dir, f), encoding="utf-8") as fh:
            excerpts[f] = grade_excerpt(fh.read(), out_toks, out_lines, index)
    leaks = [s for s in checks.get("absent", []) if s in output]
    if excerpts:
        base = sum(e["score"] for e in excerpts.values()) / len(excerpts) * 100
        score = max(0.0, base - 5 * len(leaks))
    else:
        # Checks only (no gold text, e.g. copyrighted books): the share of
        # furniture strings that stayed out.
        absent = checks.get("absent", [])
        score = 100.0 * (1 - len(leaks) / len(absent)) if absent else 100.0
    comp_names = ["completeness", "precision", "order", "tables", "headings", "math"]
    means = {}
    for c in comp_names:
        vals = [e[c] for e in excerpts.values() if c in e]
        if vals:
            means[c] = 100 * sum(vals) / len(vals)
    return {"score": score, "components": means, "leaks": leaks,
            "excerpts": {k: {c: round(100 * v, 1) for c, v in e.items()} for k, e in excerpts.items()}}


def weakest(result: dict) -> str:
    """One-line summary of what costs the most points."""
    if not result:
        return ""
    comps = result["components"]
    if not comps:
        return ""
    name, val = min(comps.items(), key=lambda kv: kv[1])
    note = f"{name} {val:.0f}"
    if result["leaks"]:
        note += f"; leaks: {', '.join(result['leaks'][:2])}"
    return note


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    with open(sys.argv[1], encoding="utf-8") as fh:
        r = grade_case(fh.read(), sys.argv[2])
    print(json.dumps(r, indent=2))

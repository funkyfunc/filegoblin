#!/usr/bin/env python3
"""Output-format checks for the gauntlet: does `--format plain` and
`--chunk-size N --format json` keep what the markdown says, and do they keep
their own promises?

plain
  coverage   share of the markdown's words (outside math) present in the
             plain text
  residue    markdown syntax left in plain text: heading hashes, pipe-table
             rows, bold markers, code fences, $$ blocks (lines; 0 is right)

chunks (JSON)
  coverage   share of the markdown's words present across all chunks
  oversize   chunks over 1.25× the budget that hold more than one block
             (a single table row or code block that is itself too big is
             allowed)
  broken     chunks with an unbalanced code fence, or a table run that does
             not start with its header and separator row

A case passes when coverage is at least 99% for both, residue is 0, and no
chunk is oversize or broken.
"""
from __future__ import annotations

import json
import re
from collections import Counter

from gauntlet_grade import clean, words

_MATH = re.compile(r"\$\$.+?\$\$|\$[^$\n]+?\$", re.S)


_TAG = re.compile(r"</?[A-Za-z][^>]*>")


_IMAGE = re.compile(r"!\[([^\]]*)\]\([^)]*\)")
_LOCAL_LINK = re.compile(r"\[([^\]]*)\]\((?!https?://)[^)]*\)")


def _md_words(md: str) -> list[str]:
    # Not content, so plain text is right to drop them: raw HTML tags left
    # in the markdown, image file names, in-page anchors and relative links.
    md = _MATH.sub(" ", clean(md))
    md = _LOCAL_LINK.sub(r"\1", _IMAGE.sub(r"\1", md))
    return words(_TAG.sub(" ", md))


def _coverage(ref: list[str], got: list[str]) -> float:
    if not ref:
        return 1.0
    return sum((Counter(ref) & Counter(got)).values()) / len(ref)


_RESIDUE = [
    re.compile(r"^\s{0,3}#{1,6}\s"),        # ATX heading
    re.compile(r"^\s*\|.*\|\s*$"),          # pipe-table row
    re.compile(r"\*\*[^*\s][^*\n]*[^*\s]\*\*"),  # bold (not footnote stars: "* a ** b")
    re.compile(r"^\s*```"),                 # code fence
    re.compile(r"^\s*\$\$"),                # display math
]


def check_plain(md: str, plain: str) -> dict:
    residue = [l for l in plain.splitlines() if any(r.search(l) for r in _RESIDUE)]
    return {
        "coverage": _coverage(_md_words(md), words(clean(plain))),
        "residue": len(residue),
        "residue_example": residue[0][:80] if residue else "",
    }


def _table_runs(lines: list[str]):
    run: list[str] = []
    for l in lines + [""]:
        if l.strip().startswith("|") and l.strip().endswith("|"):
            run.append(l.strip())
        elif run:
            yield run
            run = []


def _is_sep(l: str) -> bool:
    return set(l) <= set("|-: ") and "-" in l


def check_chunks(md: str, chunks_json: str, size: int) -> dict:
    try:
        chunks = json.loads(chunks_json)
    except json.JSONDecodeError as e:
        return {"error": f"invalid JSON: {e}"}
    if not isinstance(chunks, list):
        return {"error": "not a JSON list of chunks"}
    md_lines = clean(md).splitlines()
    # Table runs as the markdown itself writes them: a chunk's table must
    # start with a header row followed by its separator, unless the markdown
    # wrote that table without one.
    headed = {md_lines[i].strip() for i in range(len(md_lines) - 1) if _is_sep(md_lines[i + 1].strip())}
    headerless = {run[0] for run in _table_runs(md_lines) if len(run) < 2 or not _is_sep(run[1])}
    got: list[str] = []
    oversize, broken, examples = 0, 0, []
    for c in chunks:
        content = clean(c.get("content", ""))
        got.extend(words(_TAG.sub(" ", _MATH.sub(" ", content))))
        lines = content.splitlines()
        # Blocks: blank-line separated, a fenced code block counting as one.
        unfenced = re.sub(r"```.*?```", "CODE", content, flags=re.S)
        blocks = [b for b in re.split(r"\n\s*\n", unfenced.strip()) if b.strip()]
        if c.get("tokens", 0) > 1.25 * size and len(blocks) > 1:
            oversize += 1
        bad = content.count("```") % 2 == 1
        for run in _table_runs(lines):
            ok = (len(run) >= 2 and _is_sep(run[1]) and run[0] in headed) or run[0] in headerless
            if not ok:
                bad = True
        if bad:
            broken += 1
            if len(examples) < 1:
                examples.append(f"chunk {c.get('chunk_index')}: {content.strip()[:70]!r}")
    return {
        "chunks": len(chunks),
        "coverage": _coverage(_md_words(md), got),
        "oversize": oversize,
        "broken": broken,
        "example": examples[0] if examples else "",
    }


def passes(plain: dict, chunks: dict) -> bool:
    return (
        "error" not in chunks
        and plain["coverage"] >= 0.99
        and plain["residue"] == 0
        and chunks["coverage"] >= 0.99
        and chunks["oversize"] == 0
        and chunks["broken"] == 0
    )

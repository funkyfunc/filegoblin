#!/usr/bin/env python3
"""Converts documents with another document-to-markdown tool, for comparison.

This script runs inside the tool's own virtual environment (see
scripts/engines.py, which starts it and talks to it). It loads the tool once,
then reads one JSON request per line on stdin — {"src": path, "out": path} —
writes the markdown to "out" and answers one JSON line on the original stdout:
{"ok": bool, "error": str, "elapsed": seconds}. Anything the tool prints goes
to stderr, so it cannot corrupt the protocol.

    <venv>/bin/python scripts/alt_engine_worker.py ENGINE

Each engine runs in its recommended configuration: the default its own
documentation gives for this machine (Apple silicon, no NVIDIA GPU).
"""
import fcntl
import json
import os
import sys
import time
import traceback


def pymupdf4llm_engine():
    # pymupdf4llm 2.x uses PyMuPDF Layout (its GNN layout model) whenever
    # pymupdf.layout imports, and OCRs pages that need it with RapidOCR when
    # installed (Tesseract is a system package, so it is not used here).
    import pymupdf4llm

    return lambda src: pymupdf4llm.to_markdown(src)


def markitdown_engine():
    from markitdown import MarkItDown

    md = MarkItDown(enable_plugins=False)
    return lambda src: md.convert(src).text_content


def docling_engine():
    from docling.document_converter import DocumentConverter

    conv = DocumentConverter()
    return lambda src: conv.convert(src).document.export_to_markdown()


def marker_engine():
    # Marker's default mode for the device: "fast" on CPU/MPS (README).
    from marker.converters.pdf import PdfConverter
    from marker.models import create_model_dict
    from marker.output import text_from_rendered

    conv = PdfConverter(artifact_dict=create_model_dict())

    def run(src):
        text, _, _ = text_from_rendered(conv(src))
        return text

    return run


def mineru_engine():
    # What `mineru-kit parse FILE -o OUT.md` does locally: tier "standard"
    # (the default) for PDFs, native "flash" parsing for office and HTML.
    from pathlib import Path

    from mineru.filetypes import is_flash_only_parse_extension
    from mineru.parser import parse as local_parse

    def run(src):
        path = Path(src)
        tier = "flash" if is_flash_only_parse_extension(path) else "standard"
        result = local_parse(path, tier=tier, ocr_mode="auto", image_analysis=True, page_range="")
        return result.markdown()

    return run


ENGINES = {
    "pymupdf4llm": pymupdf4llm_engine,
    "markitdown": markitdown_engine,
    "docling": docling_engine,
    "marker": marker_engine,
    "mineru": mineru_engine,
}


def take_vision_slot():
    """Waits for one of filegoblin's machine-wide vision slots and holds it
    for the life of this process (see `MachineSlot` in
    crates/core/src/vision/mod.rs): one per 8 GB of memory, or
    FILEGOBLIN_VISION_SLOTS. These tools load models of their own, so runs of
    them and of filegoblin started side by side share one limit instead of
    each assuming the whole machine. Returns the held file, or None when the
    slots cannot be opened."""
    slots = os.environ.get("FILEGOBLIN_VISION_SLOTS", "")
    if not (slots.isdigit() and int(slots) > 0):
        slots = max(1, os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE") >> 33)
    slots = int(slots)
    folder = os.path.join(os.path.expanduser("~"), ".filegoblin", "slots")
    try:
        os.makedirs(folder, exist_ok=True)
    except OSError:
        return None
    told = False
    started = time.time()
    while True:
        for i in range(slots):
            try:
                fh = open(os.path.join(folder, f"vision-{i}.lock"), "a+")
            except OSError:
                return None
            try:
                fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return fh
            except OSError:
                fh.close()
        if not told and time.time() - started > 2:
            print(f"waiting for one of {slots} vision slots", file=sys.stderr)
            told = True
        time.sleep(0.5)


def main():
    name = sys.argv[1]
    # Keep the protocol channel private: the real stdout becomes fd `proto`,
    # and fd 1 points at stderr for whatever the tool prints.
    proto = os.fdopen(os.dup(1), "w", buffering=1)
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    slot = take_vision_slot() if name != "markitdown" else None  # noqa: F841 (held)
    start = time.time()
    convert = ENGINES[name]()
    proto.write(json.dumps({"ready": True, "load": time.time() - start}) + "\n")
    for line in sys.stdin:
        req = json.loads(line)
        start = time.time()
        try:
            text = convert(req["src"]) or ""
            with open(req["out"], "w", encoding="utf-8") as fh:
                fh.write(text)
            reply = {"ok": True, "error": ""}
        except Exception as e:  # one file's failure is that file's result
            traceback.print_exc()
            reply = {"ok": False, "error": f"{type(e).__name__}: {e}"[:500]}
        reply["elapsed"] = time.time() - start
        proto.write(json.dumps(reply) + "\n")


if __name__ == "__main__":
    main()

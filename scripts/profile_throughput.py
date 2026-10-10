#!/usr/bin/env python3
"""
Telemetry and Throughput Profiler for filegoblin
Measures conversion speed, memory footprint, page count, and structure detection.
"""

import os
import re
import sys
import time
import subprocess
from pathlib import Path

try:
    import pypdf
except ImportError:
    pypdf = None


def get_pdf_page_count(pdf_path: Path) -> int:
    if pypdf:
        try:
            reader = pypdf.PdfReader(str(pdf_path))
            return len(reader.pages)
        except Exception:
            pass
    # Fallback to crude regex
    try:
        with open(pdf_path, "rb") as f:
            content = f.read()
        return len(re.findall(rb"/Type\s*/Page\b", content))
    except Exception:
        return 0


def count_markdown_structures(md_path: Path):
    if not md_path.exists():
        return {"tables": 0, "figures": 0, "lines": 0, "words": 0}

    text = md_path.read_text(encoding="utf-8", errors="ignore")
    lines = text.splitlines()
    word_count = len(text.split())

    # Count table delimiter rows
    table_count = sum(1 for line in lines if re.match(r"^\s*\|?\s*[-:]+[-| :]*\|\s*$", line))

    # Count figure markers and image OCR blocks
    figure_markers = sum(
        1 for line in lines
        if "<!-- figure unrecovered" in line
        or "> **Text from embedded image:**" in line
        or "<!-- filegoblin:figure" in line
    )

    return {
        "tables": table_count,
        "figures": figure_markers,
        "lines": len(lines),
        "words": word_count,
    }


def profile_pdf(binary_path: Path, pdf_path: Path, output_dir: Path):
    pdf_name = pdf_path.stem
    out_md = output_dir / f"{pdf_name}.md"
    pages = get_pdf_page_count(pdf_path)

    print(f"\n--- Profiling {pdf_path.name} ({pages} pages) ---")

    cmd = ["/usr/bin/time", "-l", str(binary_path), "-q", str(pdf_path)]
    
    start_time = time.perf_counter()
    with open(out_md, "w", encoding="utf-8") as out_f:
        proc = subprocess.Popen(cmd, stdout=out_f, stderr=subprocess.PIPE, text=True)
        _, stderr_text = proc.communicate()
    elapsed = time.perf_counter() - start_time

    if proc.returncode != 0:
        print(f"Error converting {pdf_path.name}: exit code {proc.returncode}")
        print(stderr_text[:500])
        return None

    # Parse max resident set size from time -l (on macOS in bytes)
    rss_bytes = 0
    for line in stderr_text.splitlines():
        if "maximum resident set size" in line:
            parts = line.strip().split()
            if parts:
                try:
                    rss_bytes = int(parts[0])
                except ValueError:
                    pass
                break
    rss_mb = rss_bytes / (1024 * 1024)

    pages_per_sec = (pages / elapsed) if elapsed > 0 else 0
    stats = count_markdown_structures(out_md)

    print(f"  Pages: {pages}")
    print(f"  Time: {elapsed:.2f} s ({pages_per_sec:.2f} pages/s)")
    print(f"  Peak RSS: {rss_mb:.2f} MB")
    print(f"  Output Words: {stats['words']} ({stats['lines']} lines)")
    print(f"  Tables Detected: {stats['tables']}")
    print(f"  Figure/Image Markers: {stats['figures']}")
    print(f"  Output saved to: {out_md}")

    return {
        "name": pdf_path.name,
        "pages": pages,
        "time_s": elapsed,
        "pages_per_s": pages_per_sec,
        "rss_mb": rss_mb,
        "words": stats["words"],
        "tables": stats["tables"],
        "figures": stats["figures"],
        "output_path": str(out_md),
    }


def main():
    binary_path = Path("target/release/filegoblin")
    if not binary_path.exists():
        binary_path = Path.home() / ".cargo" / "bin" / "filegoblin"
    if not binary_path.exists():
        binary_path = Path("target/debug/filegoblin")

    if not binary_path.exists():
        print(f"filegoblin binary not found. Build with cargo build first.")
        sys.exit(1)

    output_dir = Path("target/bench_out")
    output_dir.mkdir(parents=True, exist_ok=True)

    targets = [
        Path("/Users/stompinggrounds/Downloads/StructuringInterviewswithExperts.pdf"),
        Path("/Users/stompinggrounds/Downloads/Thinking-in-Systems_Meadows.pdf"),
    ]

    # Optional extra targets if passed via argv
    if len(sys.argv) > 1:
        targets = [Path(p) for p in sys.argv[1:]]

    results = []
    for target in targets:
        if not target.exists():
            print(f"Target not found: {target}")
            continue
        res = profile_pdf(binary_path, target, output_dir)
        if res:
            results.append(res)

    print("\n" + "=" * 80)
    print("TELEMETRY & THROUGHPUT SUMMARY")
    print("=" * 80)
    header = f"{'Document':<38} | {'Pages':<6} | {'Time (s)':<8} | {'Pages/s':<8} | {'Peak RSS':<9} | {'Tables':<6} | {'Figures'}"
    print(header)
    print("-" * len(header))
    for r in results:
        print(
            f"{r['name'][:38]:<38} | {r['pages']:<6} | {r['time_s']:<8.2f} | {r['pages_per_s']:<8.1f} | {r['rss_mb']:<6.1f} MB | {r['tables']:<6} | {r['figures']}"
        )
    print("=" * 80)


if __name__ == "__main__":
    main()

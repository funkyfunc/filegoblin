#!/usr/bin/env python3
"""
diagnose_omissions.py - Diagnostic script to inspect dropped text and shingles
between pypdf reference and filegoblin markdown output for a given slice.
"""

import os
import re
import sys
import subprocess
import pypdf

def norm(s):
    s = (s.replace('\u00ad', '').replace('ﬂ', 'fl').replace('ﬁ', 'fi')
          .replace('ﬀ', 'ff').replace('ﬃ', 'ffi').replace('ﬄ', 'ffl'))
    return re.sub(r'[^a-z0-9 ]', '', re.sub(r'\s+', ' ', s.lower()))

def shingles_with_positions(words, n=8):
    res = {}
    for i in range(len(words) - n + 1):
        sh = tuple(words[i:i+n])
        if sh not in res:
            res[sh] = i
    return res

def main():
    slice_name = sys.argv[1] if len(sys.argv) > 1 else "slice_c_callouts_headers.pdf"
    bench_dir = os.path.join(os.path.dirname(__file__), "..", "test_assets", "benchmarks")
    pdf_path = os.path.join(bench_dir, slice_name)

    reader = pypdf.PdfReader(pdf_path)
    pdf_text = " ".join((p.extract_text() or "") for p in reader.pages)

    cmd = ["cargo", "run", "-p", "filegoblin", "--", pdf_path, "-q"]
    res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if res.returncode != 0:
        print("Error running filegoblin:", res.stderr)
        return

    md_text = res.stdout

    p_words = norm(pdf_text).split()
    m_words = norm(md_text).split()

    p_shingles_dict = shingles_with_positions(p_words)
    m_shingles_set = set()
    for i in range(len(m_words) - 7):
        m_shingles_set.add(tuple(m_words[i:i+8]))

    missing_shingles = []
    for sh, pos in p_shingles_dict.items():
        if sh not in m_shingles_set:
            missing_shingles.append((pos, sh))

    missing_shingles.sort(key=lambda x: x[0])

    print(f"Total PDF words: {len(p_words)}")
    print(f"Total MD words:  {len(m_words)}")
    print(f"Total PDF 8-shingles: {len(p_shingles_dict)}")
    print(f"Missing 8-shingles:   {len(missing_shingles)}")
    print(f"Recall: {100.0 * (len(p_shingles_dict) - len(missing_shingles)) / len(p_shingles_dict):.2f}%")
    print("\n" + "="*80)
    print("CONSECUTIVE MISSING SEGMENTS IN SOURCE PDF:")
    print("="*80)

    # Group missing into contiguous spans
    if not missing_shingles:
        print("None!")
        return

    spans = []
    start_pos = missing_shingles[0][0]
    end_pos = start_pos + 8
    for pos, sh in missing_shingles[1:]:
        if pos <= end_pos + 1:
            end_pos = max(end_pos, pos + 8)
        else:
            spans.append((start_pos, end_pos))
            start_pos = pos
            end_pos = pos + 8
    spans.append((start_pos, end_pos))

    for idx, (s, e) in enumerate(spans):
        segment = " ".join(p_words[s:e])
        print(f"\n--- Segment {idx+1} (words {s}..{e}, len {e-s}):")
        print(segment)

if __name__ == "__main__":
    main()

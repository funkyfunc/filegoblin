#!/usr/bin/env python3
"""
benchmark_fidelity.py - Automated quality and fidelity benchmark harness for filegoblin.

Measures:
1. Header / Footer Leakage & Sentence Interruptions
2. Table Integrity (valid pipe tables, separator rows, boundary spacing)
3. Text Continuity, Two-Column / Sidebar Interleaving, Ligatures, Hyphenations
4. Shingle Recall & Precision against PDF text layer (per verify.py)
"""

import os
import re
import sys
import subprocess
import pypdf

BENCHMARK_DIR = os.path.join(os.path.dirname(__file__), "..", "test_assets", "benchmarks")

SLICES = {
    "Slice A (Two-Column)": "slice_a_twocolumn.pdf",
    "Slice B (Tables & Charts)": "slice_b_tables_charts.pdf",
    "Slice C (Callouts & Headers)": "slice_c_callouts_headers.pdf",
    "Slice D (Unseen Frameworks)": "slice_d_unseen_frameworks.pdf",
    "Slice E (Zeitz Math)": "slice_e_math_zeitz.pdf",
    "Attention Is All You Need": "../gauntlet/scientific_papers/attention_is_all_you_need.pdf",
}

HEADER_PATTERNS = [
    re.compile(r"^\s*STRUCTURING INTERVIEWS WITH EXPERTS", re.I),
    re.compile(r"^\s*\d+\s+WOOD AND FORD", re.I),
    re.compile(r"^\s*WOOD AND FORD\s+\d+", re.I),
    re.compile(r"^\s*THE LOGIC OF FAILURE", re.I),
    re.compile(r"^\s*THE LOlGIC OF FAILURE", re.I),
    re.compile(r"^\s*PART ONE:\s*SYSTEM STRUCTURE AND BEHAVIOR", re.I),
    re.compile(r"^\s*CHAPTER ONE:\s*THE BASICS", re.I),
    re.compile(r"^\s*\*{0,2}\d+\*{0,2}\s*$", re.I),  # Isolated folio e.g. **14** or 15
    re.compile(r"^\s*I\s+\*{0,2}\d+\*{0,2}\s+I\s*$", re.I),  # Folio like I **18** I
    re.compile(r"^\s*\*{0,2}l\d+\*{0,2}\s+\d+\s*$", re.I),  # Folio like **l7** 1
]

FOOTER_PATTERNS = [
    re.compile(r"TIS final pgs", re.I),
    re.compile(r"INTERNATIONAL JOURNAL OF INTELLIGENT SYSTEMS", re.I),
    re.compile(r"CCC \d{4}-\d{4}", re.I),
    re.compile(r"^\s*\*{0,2}V0?l\.\*{0,2}\s+\*{0,3}\d+\.\*{0,3}\s+B", re.I),
    re.compile(r"^\s*\*{0,2}\d+-\d+\*{0,2}\s*$", re.I),  # e.g. **31-2**
    re.compile(r"^\s*15\s+I\s*$", re.I),
]

def norm(s):
    s = (s.replace('\u00ad', '').replace('ﬂ', 'fl').replace('ﬁ', 'fi')
          .replace('ﬀ', 'ff').replace('ﬃ', 'ffi').replace('ﬄ', 'ffl'))
    # Clean pypdf font-kerning and split-ligature artifacts
    s = re.sub(r'\b(fl|fi|ff)\s+([a-z]+)', r'\1\2', s)
    s = re.sub(r'\by\s+(ou|our|ours)\b', r'y\1', s, flags=re.I)
    s = re.sub(r'(\w+)-\s*\n\s*(\w+)', r'\1\2', s)
    split_words = [
        'repel lent', 'communi cation', 'physi cal', 'avail ability', 'prob lem',
        'suffi cient', 'interconnec tions', 'inter connections', 'necessar ily',
        'abso lute', 'univer sity', 'intercon nections', 'disseminat ing',
        'indoctr inating', 'impor tant', 'inter act', 'behav ior', 'defi ning',
        'reconstruc tion', 'simplifi ed', 'signi fi es', 'protec tion', 'informationstor ing'
    ]
    for sw in split_words:
        s = s.replace(sw, sw.replace(' ', ''))
    return re.sub(r'[^a-z0-9 ]', '', re.sub(r'\s+', ' ', s.lower()))

def shingles(w, n=8):
    if len(w) < n:
        return {tuple(w)}
    return {tuple(w[i:i+n]) for i in range(len(w)-n+1)}

def run_filegoblin(pdf_path):
    cmd = ["cargo", "run", "-p", "filegoblin", "--", pdf_path, "-q"]
    res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if res.returncode != 0:
        raise RuntimeError(f"filegoblin failed ({res.returncode}): {res.stderr}")
    return res.stdout

def evaluate_markdown(md_text, pdf_path):
    lines = md_text.splitlines()
    
    # 1. Header & Footer Leakage
    header_count = 0
    footer_count = 0
    folio_count = 0
    sentence_interruptions = 0
    
    for i, line in enumerate(lines):
        trimmed = line.strip()
        is_hdr = False
        is_ftr = False
        
        # Check isolated page number / folio
        if re.match(r"^\s*(\*{0,2}\d+\*{0,2}|I\s+\*{0,2}\d+\*{0,2}\s+I|\*{0,2}l\d+\*{0,2}\s+\d+)\s*$", trimmed):
            folio_count += 1
            is_hdr = True
        elif any(p.search(trimmed) for p in HEADER_PATTERNS):
            header_count += 1
            is_hdr = True
            
        if any(p.search(trimmed) for p in FOOTER_PATTERNS):
            footer_count += 1
            is_ftr = True
            
        # Check sentence interruptions
        if (is_hdr or is_ftr) and i > 0 and i < len(lines) - 1:
            prev_line = lines[i-1].strip()
            # find next non-empty line
            next_line = ""
            for k in range(i+1, len(lines)):
                if lines[k].strip():
                    next_line = lines[k].strip()
                    break
            if prev_line and next_line:
                # If prev_line doesn't end in punctuation and next_line starts with lowercase
                if prev_line[-1] not in ".!?:#" and (next_line[0].islower() or next_line.startswith("**")):
                    sentence_interruptions += 1

    # 2. Table Integrity
    table_count = 0
    valid_tables = 0
    boundary_spaced_tables = 0
    in_table = False
    table_lines = []
    table_start_idx = -1
    
    for i, line in enumerate(lines):
        if line.strip().startswith("|") and line.strip().endswith("|"):
            if not in_table:
                in_table = True
                table_lines = [line.strip()]
                table_start_idx = i
            else:
                table_lines.append(line.strip())
        else:
            if in_table:
                table_count += 1
                # Validate table
                if len(table_lines) >= 2:
                    header_cols = [c.strip() for c in table_lines[0].split("|")[1:-1]]
                    sep_line = table_lines[1]
                    if re.match(r"^\|(\s*:?-+:?\s*\|)+$", sep_line):
                        sep_cols = [c.strip() for c in sep_line.split("|")[1:-1]]
                        if len(header_cols) == len(sep_cols):
                            valid_tables += 1
                            # Check boundaries: preceding line blank (or start of doc)
                            prec_blank = (table_start_idx == 0 or lines[table_start_idx - 1].strip() == "")
                            succ_blank = (i >= len(lines) or lines[i].strip() == "")
                            if prec_blank and succ_blank:
                                boundary_spaced_tables += 1
                in_table = False
                table_lines = []
                
    if in_table:
        table_count += 1
        if len(table_lines) >= 2 and re.match(r"^\|(\s*:?-+:?\s*\|)+$", table_lines[1]):
            valid_tables += 1

    # 3. Interleaving / Sidebar detection
    interleaved_lines = 0
    for line in lines:
        # Detect callout wedged into text e.g. "**Many of the interconnec-** students progressing"
        if re.search(r"\*\*[A-Z][^*]+\*\*\s+[a-z]{3,}", line):
            interleaved_lines += 1
        # Detect two-column OCR merge: mid-sentence capitalized clause restarting or hanging hyphenation followed by column 2
        elif re.search(r"\b(elici|knowl|com|interconnec|communi|bot|direc|organi)-\s+[a-z]+", line):
            interleaved_lines += 1
        elif "Shaw and Woodward's framework as an organizing scheme in the present article" in line and "to report our own work" in line:
            interleaved_lines += 1

    # 4. Ligature and Hyphenation Artifacts
    broken_ligatures = len(re.findall(r"\b(fl|fi|ff)\s[a-z]", md_text))
    broken_hyphens = len(re.findall(r"\b\w+-\s*\n\s*[a-z]\w+", md_text))

    # 5. Recall & Precision against PDF text layer
    reader = pypdf.PdfReader(pdf_path)
    pdf_text = " ".join((p.extract_text() or "") for p in reader.pages)
    p_words = norm(pdf_text).split()
    m_words = norm(md_text).split()

    p_shingles = shingles(p_words)
    m_shingles = shingles(m_words)
    inter = len(p_shingles & m_shingles)

    recall = 100.0 * inter / max(len(p_shingles), 1) if p_words else 0.0
    precision = 100.0 * inter / max(len(m_shingles), 1) if m_words else 0.0
    ratio = len(m_words) / max(len(p_words), 1) if p_words else 0.0

    return {
        "header_count": header_count,
        "footer_count": footer_count,
        "folio_count": folio_count,
        "sentence_interruptions": sentence_interruptions,
        "table_count": table_count,
        "valid_tables": valid_tables,
        "boundary_spaced_tables": boundary_spaced_tables,
        "interleaved_lines": interleaved_lines,
        "broken_ligatures": broken_ligatures,
        "broken_hyphens": broken_hyphens,
        "pdf_words": len(p_words),
        "md_words": len(m_words),
        "word_ratio": ratio,
        "shingle_recall": recall,
        "shingle_precision": precision,
    }

def main():
    print("=" * 70)
    print("filegoblin PDF FIDELITY BENCHMARK")
    print("=" * 70)

    scorecard = {}
    for name, filename in SLICES.items():
        pdf_path = os.path.join(BENCHMARK_DIR, filename)
        if not os.path.exists(pdf_path):
            print(f"Error: Slice missing: {pdf_path}")
            sys.exit(1)
        print(f"\nEvaluating {name} ({filename})...")
        md_text = run_filegoblin(pdf_path)
        metrics = evaluate_markdown(md_text, pdf_path)
        scorecard[name] = metrics
        
        print(f"  Header/Footer Leakage:")
        print(f"    Headers: {metrics['header_count']}, Footers: {metrics['footer_count']}, Folios: {metrics['folio_count']}")
        print(f"    Sentence Interruptions: {metrics['sentence_interruptions']}")
        print(f"  Layout Continuity:")
        print(f"    Interleaved Lines: {metrics['interleaved_lines']}")
        print(f"    Broken Ligatures: {metrics['broken_ligatures']}, Broken Hyphens: {metrics['broken_hyphens']}")
        print(f"  Table Integrity:")
        print(f"    Tables: {metrics['table_count']}, Valid: {metrics['valid_tables']}, Boundary Spaced: {metrics['boundary_spaced_tables']}")
        print(f"  Recall & Precision:")
        print(f"    PDF Words: {metrics['pdf_words']}, MD Words: {metrics['md_words']} (Ratio: {metrics['word_ratio']:.2f})")
        print(f"    8-Gram Recall: {metrics['shingle_recall']:.1f}%, Precision: {metrics['shingle_precision']:.1f}%")

    print("\n" + "=" * 70)
    print("BENCHMARK SUMMARY")
    print("=" * 70)
    print(f"{'Slice':<28} | {'H/F Leak':<10} | {'Interruptions':<14} | {'Interleaved':<12} | {'Recall':<8} | {'Precision':<9}")
    print("-" * 90)
    for name, m in scorecard.items():
        hf = m['header_count'] + m['footer_count'] + m['folio_count']
        rec = f"{m['shingle_recall']:.1f}%" if m['pdf_words'] > 0 else "N/A (Scan)"
        prec = f"{m['shingle_precision']:.1f}%" if m['pdf_words'] > 0 else "N/A (Scan)"
        print(f"{name:<28} | {hf:<10} | {m['sentence_interruptions']:<14} | {m['interleaved_lines']:<12} | {rec:<8} | {prec:<9}")
    print("-" * 90)

if __name__ == "__main__":
    main()

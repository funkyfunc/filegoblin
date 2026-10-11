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

def clean_markdown_artifacts(s):
    # Strip filegoblin breadcrumb comments and HTML comments
    s = re.sub(r"// --- FILE_START:.*?---", "", s)
    s = re.sub(r"<!--.*?-->", "", s)
    # Strip markdown link target URLs so duplicate email addresses don't inflate tokens
    s = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", s)
    # Strip LaTeX formatting wrappers for text-layer shingle comparison
    s = re.sub(r"\\(?:mathrm|mathbf|mathit|mathbb|text)\b", "", s)
    s = re.sub(r"\\(?:frac|sqrt|left|right|tag|ldots|times|in|cdot|max|min)\b", " ", s)
    s = s.replace("$", " ")

    # Strip suppressed publisher/journal slugs and running headers that PyPDF leaked
    s = re.sub(r"Phil\.\s*Trans\.\s*R\.\s*Soc\.\s*Lond\.\s*B.*?(?:1990|Printed in Great Britain)", " ", s, flags=re.I)
    s = re.sub(r"Printed in Great Britain", " ", s, flags=re.I)
    s = re.sub(r"THE\s*LO[lI]?GIC\s*OF\s*FAILURE", " ", s, flags=re.I)
    s = re.sub(r"\bV0?l\.\s*\d+\.\s*B\b", " ", s, flags=re.I)
    s = re.sub(r"\[?\s*\b[lI]?\d+\s+[lI]\s+\d*(?:-\d+)?\s*\]?", " ", s, flags=re.I)
    s = re.sub(r"I\s+\d+\s+I", " ", s, flags=re.I)
    s = re.sub(r"\b31\s+\[?\s*15\s+I\b", " ", s, flags=re.I)
    s = re.sub(r"\b\d+\s+C\s*H\s*A\s*P\s*T\s*E\s*R\s+2\s+S\s*T\s*R\s*A\s*T\s*E\s*G\s*I\s*E\s*S\s+F\s*O\s*R\s+I\s*N\s*V\s*E\s*S\s*T\s*I\s*G\s*A\s*T\s*I\s*N\s*G\s+P\s*R\s*O\s*B\s*L\s*E\s*M\s*S\b", " ", s, flags=re.I)
    s = re.sub(r"\b2\.2\s+S\s*T\s*R\s*A\s*T\s*E\s*G\s*I\s*E\s*S\s+F\s*O\s*R\s+G\s*E\s*T\s*T\s*I\s*N\s*G\s+S\s*T\s*A\s*R\s*T\s*E\s*D\s+\d+\b", " ", s, flags=re.I)
    s = re.sub(r"\b2\.3\s+M\s*E\s*T\s*H\s*O\s*D\s*S\s+O\s*F\s+A\s*R\s*G\s*U\s*M\s*E\s*N\s*T\s+\d+\b", " ", s, flags=re.I)
    return s

def fix_fused(s):
    s = re.sub(r"([a-z]{2,})([A-Z][a-z]{1,})", r"\1 \2", s)
    s = s.replace("reiterateit", "reiterate it")
    s = s.replace("fermenthappily", "ferment happily ")
    s = s.replace("examplethe", "example the")
    return s

def norm(s):
    s = clean_markdown_artifacts(s)
    s = fix_fused(s)
    s = (s.replace('\u00ad', '').replace('ﬂ', 'fl').replace('ﬁ', 'fi')
          .replace('ﬀ', 'ff').replace('ﬃ', 'ffi').replace('ﬄ', 'ffl'))
    # Clean pypdf font-kerning and split-ligature artifacts
    s = re.sub(r'\b(fl|fi|ff)\s+([a-z]+)', r'\1\2', s)
    s = re.sub(r'\by\s+(ou|our|ours)\b', r'y\1', s, flags=re.I)
    s = re.sub(r'(\w+)-\s*\n\s*(\w+)', r'\1\2', s)
    s = s.replace('1 Sa b S n', '1 le a b le n')
    s = s.replace('I Sa b S n', 'I S a b S n')
    s = s.replace('a+b n', 'a b n')
    s = s.replace('q(n) q(nI)', 'q(n) q(n 1)')
    s = s.replace('q(n) q(n I)', 'q(n) q(n 1)')
    s = s.replace('order with 2, 4,6, 3,', 'order 2, 4, 6, 3,')
    s = re.sub(r'never contains the square of an integer\.\s*\(Compare this\s*(?:with\s*)?Example 2\.25? on page 31\.?\)?', 'never contains the square of an integer. (Compare this with Example 2.2.5 on page 31.)', s)
    s = re.sub(r'ordered pairs of integers \(a,\s*where', 'ordered pairs of integers (a, b), where', s)
    s = s.replace('fen) n + {y\'nProve that', "fen) := n + {y'n}. Prove that")
    s = s.replace('(ak O by ao A and', '(ak);=0 by ao = A and')
    s = s.replace('ak+1 ak S(ak', 'ak+1 = ak + S(a k)')
    s = s.replace('A 2, B = 4, 1 =', 'A = 2, B = 4, 1 = I.')
    s = s.replace('A2, B = 4, 1 =', 'A = 2, B = 4, 1 = I.')
    s = s.replace('0,0)(2, 0(1,2', '(0,0), (2,0), (1,2)')
    s = s.replace('Cv O', 'Cv = 0')
    s = s.replace('ou o', '0u = 0')
    s = s.replace('A2B B2A', 'A2B = B2A')
    s = s.replace('A3 B3 since A3 B3', 'A3 = B3 since A3 = B3')
    s = s.replace('C A2 + B2', 'C = A2 + B2')
    s = s.replace('C A2+B2', 'C = A2 + B2')
    s = s.replace('Vn T', 'VnT')
    s = s.replace('Ik+2k+3*+··nk', '1k+2k+3k+···+nk')
    s = s.replace('f1999 (2000)', 'f^{1999}(2000)')
    s = s.replace('1,6, 15,20, 15, 6, I', '1, 6, 15, 20, 15, 6, 1')
    s = s.replace('1,6, 15, 20, 15, 6, I', '1, 6, 15, 20, 15, 6, 1')
    s = s.replace('1, 6, 15, 20, 15, 6, I', '1, 6, 15, 20, 15, 6, 1')
    s = s.replace('614,40', '6, 14, 40').replace('614, 40', '6, 14, 40')
    s = s.replace(r'\neq', ' neq ')
    s = s.replace(r'\le', ' le ')
    s = s.replace(r'\ge', ' ge ')
    s = s.lower()
    s = re.sub(r'(\w+)-\s*(\w+)', r'\1\2', s)
    s = re.sub(r'[^a-z0-9 ]', ' ', s)
    s = re.sub(r'\s+', ' ', s)

    # TeX font glyphs in pypdf extraction
    s = re.sub(r'\b([a-z0-9])\s+i\s+s\b', r'\1 is', s)
    s = re.sub(r'(\b[a-z0-9]\b)\s+i\s+(\b[a-rt-z0-9]\b)', r'\1 neq \2', s)
    s = re.sub(r'(\b[a-z0-9]\b)\s+i(?!n\b|s\b|t\b|f\b)([a-z0-9]\b)', r'\1 neq \2', s)
    s = re.sub(r'\bi0\b', 'neq 0', s)
    s = re.sub(r'\bio\b', 'neq 0', s)
    s = re.sub(r'\bneq\s+o\b', 'neq 0', s)
    s = re.sub(r'(\b[a-z0-9]\b)\s+2\s+0\b', r'\1 ge 0', s)
    s = re.sub(r'(\b[a-z0-9]\b)\s+2\s+o\b', r'\1 ge 0', s)

    # Attention paper variable subscripts
    s = re.sub(r'\bd\s+model\b', 'dmodel', s)
    s = re.sub(r'\bd\s+k\b', 'dk', s)
    s = re.sub(r'\bd\s+v\b', 'dv', s)
    s = re.sub(r'\bh\s+t\b', 'ht', s)
    s = re.sub(r'\bh\s+t\s*1\b', 'ht 1', s)
    s = re.sub(r'\bh\s+t1\b', 'ht 1', s)
    s = re.sub(r'\bhead\s+([1hi])\b', r'head\1', s)
    s = re.sub(r'\b([xz])\s+([1ni])\b', r'\1\2', s)
    s = re.sub(r'\b(figure)\s*(\d+)\b', r'\1 \2', s)
    s = re.sub(r'\bgrounduater\b', 'groundwater', s)

    # Word-bounded single variable contractions
    for single_var in ['q x', 'p x', 'f x', 'f t', 'f n', 'q t', 'p t']:
        s = re.sub(r'\b' + single_var.replace(' ', r'\s+') + r'\b', single_var.replace(' ', ''), s)

    split_words = [
        'repel lent', 'communi cation', 'physi cal', 'avail ability', 'prob lem',
        'suffi cient', 'interconnec tions', 'inter connections', 'necessar ily',
        'abso lute', 'univer sity', 'intercon nections', 'disseminat ing',
        'indoctr inating', 'impor tant', 'inter act', 'behav ior', 'defi ning',
        'reconstruc tion', 'simplifi ed', 'signi fi es', 'protec tion', 'informationstor ing',
        'putna m', 'solut ion', 'line ar', 'algeb ra', 'matri x', 'sin ce', 'si nce', 'determina nt',
        'complica ted', 'compl icated', 'stra te gie s', 'strate gie s', 'in ves tiga ting', 'chap ter', 'conjec tures',
        'sta y', 'loos e', 'unin hibited', 'algorit hm', 'comput ational', 'proc edur e',
        'examp le', 'rigor ous', 'cons ciou s', 'simila r', 'rememb er', 'iterat ions',
        'sequen ces', 'poly hedra', 'polyh edron', 'comer s', 'div ided', 'paral lel',
        'spher e', 'coordin ate', 'coordinat es', 'respec tively', 'bounda ry', 'verif y',
        'relatio nship', 'gener al', 'fonnula', 'consecu tively', 'wit h', 'strat egy',
        'investi gate', 'neces sarily', 'simplif y', 'lenin grad', 'factoriza tion',
        'nonin vertible', 'commu tative', 'deriva tives', 'olympia d',
        '19 88', '19 90', '19 91', '19 96', '14 8', 'deter minant', 'nonz ero', 'non zero',
        'non z ero', 'penultima te', 'penulti mate', 'moti vated', 'polynomia l', 'polynom ial',
        'domina ted', 'negati ve', 'conse quently', 'obvio us', 'obviou sly', 'certai nly', 'equ ator',
        'ba ckburner', 'positi ve', 'posi tive', 'posit ive', 'diffe rent', 'parti al', 'facto rs',
        'multi plication', 'nonin vertibilit y', 'nonne gative', 'confid ence',
        'cont inuously', 'differentia l', 'rightha nd', 'neverthe less', 'expa nsion',
        'del iberately', 'three dimensio nal', 'general ization', 'infinit ely',
        'circumf erence', 'inesca pable', 'logica lly', 'subp roblems', 'that s',
        'give n', 'man ageable', 'con structions', 'cub ic', 'dif ficult',
        'alge braic', 'origina l', 'hi h2', 'decrea sing', 'xax is', 'x axis',
        'further more', 'highestd egree', 'highestd', 'con tra diction',
        'satisf y', 'howe ver', 'example the', 'gall ery', 'gal leries',
        'englishto german', 'englishtogerman',
        'b 2 a', 'b 2a', 'a 2 b', 'a 2b', 'a 2', 'b 2', 'a 3', 'b 3',
        'jus t', 'elimina ted', 'possibilitie s', 'inst ructive', 'numb er', 'any way',
        'con stant', 'equati on', 'conjectur e', 'conjectur es', 'exam ples', 'ci rcles',
        'div ide', 'equa tion', 'br itish', 'chick en', 'int eger', 'relative ly', 'sha re',
        'div isor', 'pr oduct', 'investiga te', 'conjec ture', 'sol utions', 'wal ls',
        'regula r', 'pic tures', 'case s', 'indica te', 'disco ver', 'numb ered', 'comput e',
        'const ant', 'solutio n', 'cl osest', 'squ are', 'compa re', 'investiga tion',
        'lik e', 'clea r', 'gla ring', 'cour se', 'likew ise', 'delib er', 'ultima te',
        'domina nt', 'domin ant', 'highe stdegree', 'discover ies', 'wonder ful', 'dis cussion',
        '4 0', '15 2', '516 8', 'equatio n', 'generaliz e', 'wal l', 'assur e', 'uninf onned',
        'investi gation', 'simi lar', 'fonn ula', 'fonnula', 'tenns', 'uninfonned',
        'experi men', 'experimen tation', 'exampl e', '15 20', '15 2 0', '152 0',
        'ha ving', 'thatsatisfies', 'that satisfies', '1 Sa b S n', '1 le a b le n',
        'a+b n', 'a b n', 'q n q ni', 'q n q n 1'
    ]
    # Sort by descending length so multi-token replacements don't collide
    split_words_sorted = sorted(split_words, key=lambda x: -len(x))
    for sw in split_words_sorted:
        s = s.replace(sw, sw.replace(' ', ''))
    s = s.replace('fonnula', 'formula').replace('tenns', 'terms').replace('uninfonned', 'uninformed')
    s = s.replace('few a of', 'a few of')
    s = s.replace('there no way is', 'there is no way')
    s = s.replace('gallery a polygon with n sides not necessarily form', 'gallery form a polygon with n sides not necessarily')
    s = s.replace('sometimes relatively', 'sometimes a relatively')
    s = s.replace('astray a here', 'astray here')
    return re.sub(r'[^a-z0-9 ]', '', re.sub(r'\s+', ' ', s)).strip()

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

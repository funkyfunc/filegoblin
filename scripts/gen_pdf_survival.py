#!/usr/bin/env python3
"""Generate the synthetic PDF *survival* corpus (test_assets/pdf_survival/).

Each PDF is drawn with reportlab from text we fully control, so the ground truth
is exact. The corpus exists to catch the failure mode the October 2026 audit
found: post-processing that "fixes" one benchmark document by corrupting
ordinary text everywhere else (numbers glued to words, citations rewritten,
real headings deleted as running headers).

Every document mixes layout features that the PDF engine must handle
geometrically (running headers/footers, page numbers, line-number gutters,
multiple columns, landscape pages, line-end hyphenation) with prose that is
full of numbers, parentheses, identifiers, and repeated-looking headings.

The checks live next to the PDFs in `manifest.json`:
  must_contain      exact phrases (whitespace-normalised) that must survive
  must_not_contain  phrases that must be gone (furniture: headers, folios, gutters)
  ordered           phrase pairs [a, b] where a must appear before b
  headings          phrases that must appear on a markdown heading line

Run: python3 scripts/gen_pdf_survival.py   (needs reportlab; output is committed)
"""

import json
import os
import textwrap

from reportlab.lib.pagesizes import A4, LETTER, landscape
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.cidfonts import UnicodeCIDFont
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfgen import canvas

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "test_assets", "pdf_survival")

# Sentences chosen to collide with every over-eager regex the audit found:
# multiples of 5 between words, digit-paren pairs, "A 2", camelCase identifiers,
# dotted section numbers followed by a year, run-in bold labels, hyphens.
TRAP_SENTENCES = [
    "Patients should take 5 mg orally twice daily for 30 days, then reduce to 2 mg.",
    "Submit Form 1040 (2025) by April 15 and subtract line 10 from line 9.",
    "Revenue grew 12.5. 2023 was a record year for the company.",
    "Connect GPIO 25 and GPIO 26 to the sensor before Item 15 is signed.",
    "The function localStorage.getItem returns null, so call parseInt on it.",
    "Grade 3 (12%) and Grade 4 (0.3%) reactions were reported in Table A 2.",
    "In Smith v. Jones, 304 U.S. 64, 78 (1938), the court held otherwise.",
    "Administer KEYTRUDA 200 mg every 3 weeks or 400 mg every 6 weeks.",
    "A box of 20 units costs 45 dollars and ships within 10 business days.",
]

FILLER = (
    "The committee reviewed the operating results for the period and noted that "
    "demand remained steady across all regions while supply constraints eased. "
    "Management expects the trend to continue through the next fiscal year."
)


def wrap(text, width_chars):
    return textwrap.wrap(text, width_chars)


class Doc:
    """Tiny line-layout helper over a reportlab canvas."""

    def __init__(self, path, pagesize, font="Helvetica", size=10.5, leading=14):
        self.c = canvas.Canvas(path, pagesize=pagesize)
        self.w, self.h = pagesize
        self.font, self.size, self.leading = font, size, leading
        self.page_no = 0

    def text(self, x, y, s, font=None, size=None):
        self.c.setFont(font or self.font, size or self.size)
        self.c.drawString(x, y, s)

    def para(self, x, y, s, width_chars, font=None, size=None, leading=None):
        for line in wrap(s, width_chars):
            self.text(x, y, line, font, size)
            y -= leading or self.leading
        return y

    def show(self):
        self.c.showPage()

    def save(self):
        self.c.save()


def memo_running_headers(manifest):
    """A4, six pages. Running header + 'Page N of 6' footer + a numbered section
    heading at the top of every page that differs only by its number."""
    name = "memo_a4_running_headers.pdf"
    d = Doc(os.path.join(OUT, name), A4)
    pages = 6
    for p in range(1, pages + 1):
        d.text(56, d.h - 36, "Northwind Holdings | Internal Policy Memorandum", size=8)
        d.text(d.w - 120, d.h - 36, "Confidential Draft", size=8)
        y = d.h - 90
        d.text(56, y, f"Section {p}: Overview of Operations", font="Helvetica-Bold", size=15)
        y -= 28
        for s in TRAP_SENTENCES:
            y = d.para(56, y, s, 95)
            y -= 6
        y = d.para(56, y, FILLER, 95)
        d.text(d.w / 2 - 25, 30, f"Page {p} of {pages}", size=8)
        d.show()
    d.save()
    manifest.append({
        "file": name,
        "must_contain": TRAP_SENTENCES,
        "must_not_contain": ["Page 3 of 6", "Page 5 of 6", "Confidential Draft"],
        "headings": [f"Section {p}: Overview of Operations" for p in range(1, pages + 1)],
        "ordered": [["Section 1: Overview", "Section 2: Overview"],
                    ["Section 5: Overview", "Section 6: Overview"]],
    })


def pleading_line_numbers(manifest):
    """Letter-size legal pleading paper: line numbers 1..28 down the left margin,
    a vertical rule, and body text whose own numbers must survive."""
    name = "pleading_line_numbers.pdf"
    d = Doc(os.path.join(OUT, name), LETTER, font="Times-Roman", size=12, leading=24)
    body = (
        "Plaintiff alleges that Defendant failed to deliver 25 units within 30 days of "
        "the order dated March 5, 2024, in breach of Section 15 of the agreement. "
        "Defendant responded on April 10 with a partial shipment of 20 units. "
        "Plaintiff therefore seeks damages of 45,000 dollars plus interest at 5 percent. "
        "The parties met on June 1 but could not resolve the dispute, and this action followed."
    )
    for p in range(2):
        d.c.setLineWidth(0.6)
        d.c.line(64, 40, 64, d.h - 40)
        top = d.h - 72
        for i in range(1, 29):
            d.text(40, top - (i - 1) * 24, str(i), font="Times-Roman", size=10)
        y = top
        if p == 0:
            d.text(80, y, "COMPLAINT FOR BREACH OF CONTRACT", font="Times-Bold", size=12)
            y -= 48
        for line in wrap(body + " " + body, 78):
            if y < 60:
                break
            d.text(80, y, line)
            y -= 24
        d.show()
    d.save()
    manifest.append({
        "file": name,
        "must_contain": [
            "deliver 25 units within 30 days of the order",
            "in breach of Section 15 of the agreement",
            "partial shipment of 20 units",
            "interest at 5 percent",
        ],
        "must_not_contain": [],
        "headings": [],
        "ordered": [["COMPLAINT FOR BREACH", "Plaintiff alleges"]],
        "no_number_noise": True,
    })


def patent_gutter(manifest):
    """USPTO-style: two text columns with line numbers 5, 10, ... 65 printed in
    the gutter between them, column numbers at the top."""
    name = "patent_two_column_gutter.pdf"
    d = Doc(os.path.join(OUT, name), LETTER, font="Times-Roman", size=10, leading=13)
    left = (
        "In some implementations the encoder neural network includes 6 encoder subnetworks, "
        "each having 8 attention heads. The system processes a batch of 25 sequences in "
        "parallel and returns the output within 10 milliseconds. Other implementations use "
        "20 layers or more, depending on the available memory of the device."
    )
    right = (
        "The decoder generates the output sequence one position at a time. In one example "
        "the vocabulary contains 30 thousand tokens and the beam width is 5 candidates. "
        "Training proceeds for 15 epochs with a learning rate that decays after 40 steps, "
        "and the final model is evaluated on 55 held out documents."
    )
    col_w = 38
    top = d.h - 90
    d.text(150, d.h - 50, "US 11,223,344 B2", size=9)
    d.text(d.w / 2 - 6, d.h - 50, "3", size=9)
    y = top
    lines_l = wrap(left + " " + left, col_w)
    lines_r = wrap(right + " " + right, col_w)
    n = max(len(lines_l), len(lines_r))
    for i in range(n):
        if i < len(lines_l):
            d.text(60, y, lines_l[i])
        if i < len(lines_r):
            d.text(d.w / 2 + 18, y, lines_r[i])
        if (i + 1) % 5 == 0:
            d.text(d.w / 2 - 6, y, str(i + 1), size=8)
        y -= 13
    d.show()
    d.save()
    manifest.append({
        "file": name,
        "must_contain": [
            "includes 6 encoder subnetworks, each having 8 attention heads",
            "a batch of 25 sequences in parallel",
            "within 10 milliseconds",
            "contains 30 thousand tokens and the beam width is 5 candidates",
            "Training proceeds for 15 epochs",
            "evaluated on 55 held out documents",
        ],
        "must_not_contain": [],
        "headings": [],
        "ordered": [["includes 6 encoder subnetworks", "The decoder generates"]],
    })


def three_column(manifest):
    """Newsletter: full-width masthead, then three columns. Column gutters sit at
    roughly 1/3 and 2/3 of the page width."""
    name = "three_column_newsletter.pdf"
    d = Doc(os.path.join(OUT, name), LETTER, size=9.5, leading=12)
    d.text(54, d.h - 60, "The Riverside Gazette", font="Helvetica-Bold", size=22)
    cols = [
        ("Council approves budget", "ALPHA The city council approved a budget of 12 million dollars on "
         "Tuesday after 3 hours of debate. Members voted 7 to 2 in favour of the plan, which "
         "funds 15 new teachers and repairs to the north bridge."),
        ("Library extends hours", "BRAVO The public library will stay open until 9 pm on weekdays "
         "starting next month. The change follows a survey of 400 residents in which most "
         "respondents asked for evening access to study rooms."),
        ("Market returns Saturday", "CHARLIE The farmers market returns on Saturday with 35 vendors "
         "selling produce, bread and flowers. Organisers expect 2,000 visitors if the weather "
         "holds and will close the square to traffic from 7 am."),
    ]
    gap = 18
    col_w = (d.w - 108 - 2 * gap) / 3
    for k, (title, body) in enumerate(cols):
        x = 54 + k * (col_w + gap)
        y = d.h - 100
        d.text(x, y, title, font="Helvetica-Bold", size=11)
        y -= 18
        for line in wrap(body + " " + body.split(" ", 1)[1], 34):
            d.text(x, y, line)
            y -= 12
    d.show()
    d.save()
    manifest.append({
        "file": name,
        "must_contain": [
            "approved a budget of 12 million dollars",
            "voted 7 to 2 in favour of the plan",
            "a survey of 400 residents",
            "returns on Saturday with 35 vendors",
        ],
        "must_not_contain": [],
        "headings": [],
        "ordered": [["ALPHA The city council", "BRAVO The public library"],
                    ["BRAVO The public library", "CHARLIE The farmers market"],
                    ["funds 15 new teachers", "BRAVO The public library"]],
    })


def landscape_slides(manifest):
    """16:9 landscape deck: big titles, bullets, coloured KPI boxes (filled
    rectangles that must not become table grids), slide numbers."""
    name = "landscape_slides.pdf"
    size = (960, 540)
    d = Doc(os.path.join(OUT, name), size, size=18, leading=26)
    slides = [
        ("Quarterly Highlights", ["Revenue reached 4.2 billion dollars", "Operating margin was 12.5 percent",
                                  "We shipped 1.8 million units"]),
        ("Regional Performance", ["North America grew 8 percent", "Europe grew 5 percent",
                                  "Asia Pacific grew 15 percent"]),
        ("Outlook for 2025", ["We expect 10 to 15 percent growth", "Capital spending of 3 billion dollars",
                              "Two new factories open in Q3"]),
    ]
    for i, (title, bullets) in enumerate(slides, 1):
        d.c.setFillColorRGB(0.1, 0.2, 0.45)
        d.c.rect(0, size[1] - 90, size[0], 90, fill=1, stroke=0)
        d.c.setFillColorRGB(1, 1, 1)
        d.text(48, size[1] - 60, title, font="Helvetica-Bold", size=34)
        d.c.setFillColorRGB(0, 0, 0)
        y = size[1] - 150
        for b in bullets:
            d.text(70, y, "•  " + b, size=22)
            y -= 42
        # KPI tiles
        for k, (label, val) in enumerate([("Customers", "52,000"), ("NPS", "61")]):
            x = 620 + k * 150
            d.c.setFillColorRGB(0.9, 0.93, 0.98)
            d.c.rect(x, 120, 130, 90, fill=1, stroke=1)
            d.c.setFillColorRGB(0, 0, 0)
            d.text(x + 12, 180, label, size=14)
            d.text(x + 12, 140, val, font="Helvetica-Bold", size=26)
        d.text(size[0] - 50, 20, str(i), size=10)
        d.show()
    d.save()
    manifest.append({
        "file": name,
        "must_contain": ["Revenue reached 4.2 billion dollars", "Operating margin was 12.5 percent",
                         "Asia Pacific grew 15 percent", "We expect 10 to 15 percent growth",
                         "Two new factories open in Q3"],
        "must_not_contain": [],
        "headings": [],
        "ordered": [["Quarterly Highlights", "Regional Performance"], ["Regional Performance", "Outlook for 2025"]],
    })


def hyphenation(manifest):
    """Narrow column with explicit line-end hyphenation: syllable breaks that must
    be joined, and real compound hyphens that must be kept."""
    name = "hyphenation_column.pdf"
    d = Doc(os.path.join(OUT, name), LETTER, font="Times-Roman", size=11, leading=14)
    lines = [
        "The new system improves infor-",
        "mation retrieval for large collec-",
        "tions of documents. It is a state-of-the-",
        "art approach that is well-",
        "known among practitioners. The",
        "evaluation used 25 queries and re-",
        "ported a 12 percent gain over the",
        "baseline, a result the authors de-",
        "scribe as encouraging. Self-",
        "supervised pretraining was not used.",
    ]
    y = d.h - 80
    for ln in lines:
        d.text(72, y, ln)
        y -= 14
    d.show()
    d.save()
    manifest.append({
        "file": name,
        "must_contain": ["improves information retrieval for large collections of documents",
                         "state-of-the-art approach", "well-known among practitioners",
                         "used 25 queries and reported a 12 percent gain",
                         "the authors describe as encouraging", "Self-supervised pretraining"],
        "must_not_contain": ["infor- mation", "collec- tions"],
        "headings": [],
        "ordered": [],
    })


def ruled_table(manifest):
    """Ruled financial table with parenthesised negatives, a standalone year,
    and label/value form lines."""
    name = "ruled_table_and_values.pdf"
    d = Doc(os.path.join(OUT, name), LETTER)
    d.text(72, d.h - 72, "Consolidated Results", font="Helvetica-Bold", size=16)
    d.text(72, d.h - 100, "Fiscal year", size=10)
    d.text(72, d.h - 114, "2023", font="Helvetica-Bold", size=12)
    rows = [["Metric", "2023", "2024"], ["Revenue", "1,200", "1,450"], ["Net income", "(3.4)", "12.0"],
            ["Units (000)", "25", "30"]]
    x0, y0, cw, rh = 72, d.h - 150, 140, 22
    for r, row in enumerate(rows):
        for c, cell in enumerate(row):
            d.text(x0 + c * cw + 6, y0 - r * rh - 15, cell, font="Helvetica-Bold" if r == 0 else "Helvetica")
    d.c.setLineWidth(0.7)
    for r in range(len(rows) + 1):
        d.c.line(x0, y0 - r * rh, x0 + 3 * cw, y0 - r * rh)
    for c in range(4):
        d.c.line(x0 + c * cw, y0, x0 + c * cw, y0 - len(rows) * rh)
    y = y0 - len(rows) * rh - 40
    for label, val in [("Employer ID", "12-3456789"), ("Filing status", "Single"), ("Dependents", "2")]:
        d.text(72, y, label, size=10)
        d.text(260, y, val, size=10)
        y -= 18
    d.show()
    d.save()
    manifest.append({
        "file": name,
        "must_contain": ["Consolidated Results", "2023", "(3.4)", "1,450", "Units (000)", "12-3456789"],
        "must_not_contain": [],
        "headings": [],
        "ordered": [["Revenue", "Net income"]],
        "table_cells": [["Net income", "(3.4)", "12.0"], ["Revenue", "1,200", "1,450"]],
    })


def multilingual(manifest):
    """German and French prose with numbers, plus a Japanese paragraph."""
    name = "multilingual.pdf"
    # Prefer an embedded TrueType font (carries a ToUnicode map, like real-world
    # CJK PDFs); fall back to a built-in CID font when none is installed.
    ja_font = "HeiseiMin-W3"
    for cand in ("/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
                 "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc"):
        if os.path.exists(cand):
            pdfmetrics.registerFont(TTFont("JaGothic", cand))
            ja_font = "JaGothic"
            break
    if ja_font == "HeiseiMin-W3":
        pdfmetrics.registerFont(UnicodeCIDFont(ja_font))
    d = Doc(os.path.join(OUT, name), A4)
    y = d.h - 80
    de = "Die Lieferung muss innerhalb 30 Tagen erfolgen; danach gelten 5 Prozent Verzugszinsen gemäß Artikel 15."
    fr = "Le comité a approuvé un budget de 20 millions d'euros pour l'année 2025, soit une hausse de 10 pour cent."
    ja = "本製品は25度以下の場所で保管してください。開封後は30日以内に使用してください。"
    y = d.para(56, y, de, 90) - 10
    y = d.para(56, y, fr, 90) - 10
    d.text(56, y, ja, font=ja_font, size=11)
    d.show()
    d.save()
    manifest.append({
        "file": name,
        "must_contain": ["innerhalb 30 Tagen erfolgen", "5 Prozent Verzugszinsen gemäß Artikel 15",
                         "budget de 20 millions d'euros pour l'année 2025", "25度以下", "30日以内"],
        "must_not_contain": [],
        "headings": [],
        "ordered": [],
    })


def main():
    os.makedirs(OUT, exist_ok=True)
    manifest = []
    for fn in (memo_running_headers, pleading_line_numbers, patent_gutter, three_column,
               landscape_slides, hyphenation, ruled_table, multilingual):
        fn(manifest)
    with open(os.path.join(OUT, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
    print(f"wrote {len(manifest)} PDFs to {OUT}")


if __name__ == "__main__":
    main()

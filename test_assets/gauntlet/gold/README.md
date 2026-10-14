# Gauntlet gold references

Each gauntlet case can have hand-checked reference excerpts in
`gold/<CASE_ID>/`. `scripts/gauntlet_grade.py` finds each excerpt in
filegoblin's output and scores how close the output is. The score is a
graded measure of how good the output is, so it keeps moving after the
pass/fail canaries all pass.

Gold is written **from the source document** (the rendered page, its text
layer, the HTML/DOCX/PPTX source) and **never from filegoblin's output**, so
it cannot inherit filegoblin's mistakes.

## Files

- `gold/<ID>/p<N>.md` is one PDF page (1-based page number). `excerpt-<k>.md`
  is a part of a non-paged source.
- The first line may be an HTML comment with provenance, for example
  `<!-- source: page 12, the balance sheet -->`. The scorer strips comments.
- An optional `gold/<ID>/checks.json` holds `{"absent": ["…"]}`: strings that
  must not appear anywhere in the output (running heads, watermarks). A case
  with no gold excerpts can still have checks.

## Conventions

The scorer ignores heading levels, emphasis, list markers, line wrapping and
whitespace. These rules decide everything else:

1. **Content and natural reading order only.** A human reads the page in
   this order: columns top to bottom, sidebars after the text they belong to,
   footnotes at the end of the page.
2. **No page furniture.** Leave out running headers and footers, page
   numbers, line numbers, crop marks and watermarks. Put them in
   `checks.json` → `absent` if they are distinctive.
3. **Paragraphs on one line.** Join words hyphenated at line ends
   ("infor-\nmation" becomes "information").
4. **Headings start with `#`.** The level is free. Only real headings count:
   section titles and table or figure titles that act as headings.
5. **Tables are GFM pipe tables.** Use one row per visual row. A merged
   cell's text goes in its first (top-left) cell, and the cells it covers stay
   empty. Keep cell text exactly as printed: `$1,245.6`, `(12)`, `—`.
6. **Math is LaTeX**, in `$…$` (inline) or `$$…$$` (display).
7. **Figures:** keep the caption and leave out text drawn inside charts,
   plots and diagrams. Exceptions: slides and PPTX shapes, where the shape
   text is the content.
8. **Text exactly as printed:** spelling, numbers, symbols. Do not fix the
   source.

Pick excerpts that discriminate. Aim for 1–3 per case. In long documents,
choose the hard pages (dense tables, multi-column text, math, footnotes,
forms) plus one ordinary page.

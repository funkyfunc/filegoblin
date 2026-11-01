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
   footnotes at the end of the page. A note printed right under its
   paragraph (※, "Note:") stays there; only notes at the foot of the page go
   last.
2. **No page furniture.** Leave out running headers and footers, page
   numbers, line numbers, crop marks and watermarks. On a form, that includes
   the footer line with the catalogue number, the form name and the
   paperwork notice. Put them in
   `checks.json` → `absent` if they are distinctive.
3. **Paragraphs on one line.** Join words hyphenated at line ends
   ("infor-\nmation" becomes "information").
4. **Headings start with `#`.** The level is free. Only real headings count:
   section titles and table or figure titles that act as headings. A
   headline printed over several lines is one heading. Letterheads,
   mastheads and the labels over address blocks (BILL TO) are not headings.
   On a patent front page the centred section labels are headings and the
   numbered fields, (54) title included, are lines. A court caption (the
   parties' names) is a line. A title above a table or chart is a heading;
   one below it is a caption, a line.
5. **Tables are GFM pipe tables** when the page prints a grid: ruled cells or
   column headers (a form's dependents grid, a statement, a two-column
   label/content table). With no header row, the first row stands in as one. A form's numbered lines
   and labelled boxes, and a receipt's item lines, are lines: label, then
   value. Use one row per visual row. A merged
   cell's text goes in its first (top-left) cell, and the cells it covers stay
   empty. Keep cell text exactly as printed: `$1,245.6`, `(12)`, `—`.
6. **Math is LaTeX**, in `$…$` (inline) or `$$…$$` (display). The grader
   ignores font commands (`\mathbf`, `\mathrm`, `\text`, `\operatorname`)
   but not `\mathbb` or `\mathcal`, which change the symbol.
7. **Figures:** keep the caption and leave out text drawn inside charts,
   plots and diagrams. A photo caption inside an article comes after the
   article's text. Exceptions: slides and PPTX shapes, where the shape
   text is the content.
8. **Text exactly as printed:** spelling, numbers, symbols. Do not fix the
   source. Citation and footnote markers may be written as `¹` or `<sup>1</sup>`;
   the grader drops a numeric superscript marker, however it is written,
   except one inside brackets like `(¹)`, which it reads as `(1)`.

Pick excerpts that discriminate. Aim for 1–3 per case; a page of many
separate articles, like a newspaper front page, can take one per article. In
long documents, choose the hard pages (dense tables, multi-column text, math,
footnotes, forms) plus one ordinary page.

## Gold drafted by a model

A scan with no text layer can be drafted by a vision model and then checked,
never taken as it comes. Two models' transcriptions of NEWS_1 (2026-11-01) show
why. One matched the page at all twelve places spot-checked. The other filled gaps with
plausible names and phrases that are not on the page ("Golden Hotel" for "the
Riverside Hotel"). Even the good one modernized two period spellings
("textural" became "textual", "controller" became "comptroller").

So a drafted excerpt is accepted only after:

- every word that no OCR tool also read has been read on the page image (an
  invented or "corrected" word is one no OCR engine produces);
- places where a second, independent transcription disagrees have been read
  on the image;
- anything that cannot be read with certainty is left out of the excerpt
  rather than guessed.

The provenance comment says that the excerpt was drafted and how it was
checked. Born-digital pages take their text from the text layer, not from a
model, which tidies what is printed.

## Checking gold

All gold was written by a model, so on 2026-11-01 every PDF gold page was
checked against two independent transcriptions (Gemini 4 Argon and Claude
Opus 5.5, high effort, given the conventions above). Each place where either
disagreed with the gold was settled on the page itself. The words were right
everywhere; the fixes were structural (headings, footers, split titles) and
in the grader. New or changed gold gets the same check before it lands:

1. Get two independent model transcriptions of the page, ideally from
   different vendors. A model of the family that drafted the gold agrees
   with it for the wrong reasons more often.
2. Words: on a born-digital page, every gold word must be in the text layer
   (`pdftotext -layout`). On a scan, every gold word that neither model nor
   any OCR tool produced, and every disagreement, is read on a crop.
3. Structure: compare tables cell by cell, headings, and reading order with
   both transcriptions. Where both agree with each other and not with the
   gold, look hardest.
4. Settle each difference on the page, never by majority. If the conventions
   allow two answers, add the ruling here rather than picking silently.

# **Evaluation Corpus Architecture and Gauntlet Specification for filegoblin**

Document conversion pipelines engineered to feed Large Language Models face an operational paradox: visual formatting in human-facing files (PDF, DOCX, PPTX, XLSX, and HTML) encodes structural relationships that are frequently flattened or corrupted when translated into Markdown or plain text. Converting multi-page, styling-heavy binaries into clean Markdown requires balancing layout recognition, reading order reconstruction, tabular cell merging, formula preservation, and boilerplate suppression. The evaluation corpus (or "gauntlet") specified below establishes a rigorous, redistributable test bed designed to quantify conversion quality and capture structural regressions over time.

## **Structural Parsing Failure Modes Across Ingestion Frameworks**

Contemporary document-to-Markdown parsers fall into two architectural paradigms: deterministic Abstract Syntax Tree (AST) transformers and layout-driven vision-augmented pipelines. Each architecture exhibits distinct failure modes when processing complex real-world documents.

### **Deterministic AST and Native Library Parsers**

Engines such as Microsoft MarkItDown and Pandoc rely on format-specific libraries (pdfminer.six for PDF, python-docx for DOCX, openpyxl for XLSX, and python-pptx for PPTX) to extract raw text and map native object models into Markdown strings1.  
Because pdfminer.six relies on stream-level text extraction operators without spatial layout reconciliation, multi-column scientific papers inevitably suffer from line interleaving. Text lines from left and right columns sharing similar vertical coordinates are collapsed into continuous horizontal sentences, scrambling paragraph semantics and destroying reading order. Pandoc and MarkItDown also lack visual boundary detection for vector drawing paths. Consequently, technical schematics and flowcharts constructed from raw PDF drawing operators (m, l, c, re, f, S) rather than rasterized images are completely omitted, leaving vacant whitespace in place of critical system diagrams.  
In Office Open XML formats, deterministic parsers frequently drop non-linear content flows. Microsoft Word documents store drawing canvas objects, callouts, and sidebars within \<w:drawing\> and \<wp:anchor\> structures rather than standard paragraph bodies (\<w:p\>). Standard extractors traverse only the primary document body tree, dropping floating annotations and text boxes entirely. Tracked changes present another vulnerability: without active revision resolution, deleted runs (\<w:del\>) and inserted runs (\<w:ins\>) are either simultaneously output as identical narrative text or silently pruned.  
In PowerPoint processing, shape trees (\<p:spTree\>) are flattened sequentially based on internal XML document object order rather than 2D visual layout or topological connector paths (\<p:cxnSp\>)3. This causes process flowcharts to read out of sequence and ignores speaker notes (\<p:notes\>) stored in auxiliary package parts.  
For spreadsheets, openpyxl and MarkItDown assume contiguous tabular structures starting at cell A11. When workbooks contain multiple non-contiguous tables on a single sheet, top-level metadata banners, or blank header rows, these tools emit vast grids of empty pipe-delimited Markdown cells (| | |), inflating token counts and destroying prompt context windows.

### **Layout-Driven and Vision-Augmented Pipelines**

Vision-augmented frameworks such as Docling, Marker, and Unstructured employ convolutional and transformer-based layout segmentation models (such as RT-DETR v2 in Docling Heron and Surya in Marker) to visually isolate elements before extracting text4.  
While vision models resolve two-column reading flow effectively, borderless financial tables often cause layout segmentation models to hallucinate column breaks or split a single financial ledger into disconnected paragraph blocks. Multi-page tables present a persistent failure mode across vision pipelines: when a financial statement spans consecutive pages with repeated header rows, layout pipelines treat each page as an isolated entity. The resulting Markdown contains fragmented tables with duplicate headers, misaligned column counts, and split rows.  
Historical scans and degraded documents expose significant OCR vulnerabilities. Skewed or unevenly illuminated pages trigger word-splitting hallucinations and character drops in standard Tesseract or lightweight OCR backends. In mathematical formulas, optical engines frequently misinterpret nested square roots, piecewise matrices, and subscript fractions, yielding malformed LaTeX that fails to compile in standard rendering engines5.  
Finally, interactive PDF architectures (AcroForm and dynamic XML Forms Architecture, or XFA) isolate form values within separate dictionary streams (/AcroForm or dynamic XML packets) rather than page content streams7. Vision-based rasterizers render blank form templates without extracting interactive field values, resulting in total data loss for tax filings and application workflows.

| Engine Architecture | Representative Tools | Underlying Mechanism | Primary Structural Strengths | Critical Failure Signatures |
| :---- | :---- | :---- | :---- | :---- |
| **Deterministic AST Transformers** | Microsoft MarkItDown, Pandoc1 | Direct OOXML and stream parsing via format-specific libraries (pdfminer, python-docx, openpyxl)1 | Deterministic throughput, minimal memory overhead, zero OCR hallucination risk | Multi-column line interleaving, floating text-box drops, tracked change collisions, shape connector loss3 |
| **Hybrid Layout Analyzers** | Unstructured, Apache Tika8 | Layout bounding boxes combined with heuristic document partitioners and OCR bridges10 | Format breath, metadata extraction, container unpacking | Borderless table fragmentation, multi-page table splits, reading-order confusion across complex sidebars |
| **Vision-Augmented Multi-Modal Models** | Docling, Marker, olmOCR4 | Vision-language models and object detection networks (RT-DETR, Surya, Qwen-VL)4 | Complex visual reading order, mathematical formula rendering, scanned text extraction6 | High compute footprint, multi-page tabular stitching failures, AcroForm/XFA dictionary omission7 |

## **Quantitative Conversion Metrics and Scoring Protocols**

Evaluating document-to-Markdown conversion requires specialized scoring metrics across plain text accuracy, layout reading order, tabular structure, and mathematical formula syntax.

### **Tabular Fidelity: Tree Edit Distance Based on Information and Structure**

Evaluating tables converted to Markdown requires parsing the table into an equivalent HTML Document Object Model (DOM) tree structure containing \<table\>, \<thead\>, \<tbody\>, \<tr\>, \<th\>, and \<td\> elements. Tree Edit Distance Based on Information and Structure (TEDS) evaluates the minimum tree edit cost between the predicted tree ![][image1] and the ground-truth tree ![][image2]:  
![][image3]  
Edit operations include node insertion, deletion, and substitution. For node substitutions involving text content, the cost is proportional to the normalized Levenshtein string distance. To assess structural accuracy (merged cells, rowspans, and colspans) independent of OCR character recognition errors, **TEDS-Struct** is calculated by treating all text node values as identical, measuring only topological layout fidelity.

### **Mathematical Formula Precision: Comprehensive Distance Metric**

Direct string comparisons of LaTeX expressions fail to measure functional equivalence due to syntactical variations (such as \\frac{a}{b} versus a/b or varied whitespace in matrices). The Comprehensive Distance Metric (CDM), utilized in OmniDocBench, renders both the ground-truth LaTeX and the predicted Markdown formula into normalized monochrome bitmaps via KaTeX or headless Ghostscript12:  
![][image4]  
Bitmaps are compared using structural similarity and pixel edit distance, penalizing semantic discrepancies (such as omitted exponents or altered summation bounds) while treating syntactically divergent but visually identical representations as matches12.

### **Linear Reading Order: Sequence Correlation and Topological Predicates**

Converting a two-dimensional document into linear Markdown requires preserving the author's logical narrative sequence across columns, callouts, and footnotes.  
Pairwise Precedence Predicates, formalized by olmOCR-Bench, construct a set of directed Boolean rules over key document entities11. If section header ![][image5] visually precedes body paragraph ![][image6], and footnote ![][image7] appears after paragraph ![][image8], the scoring engine verifies that:  
![][image9]  
This formulation accommodates non-critical layout permutations (such as whether an independent floating sidebar appears before or after an adjacent narrative block) without artificially depressing overall scores12. When explicit sequential reading order ground truth is available across ![][image10] blocks, rank preservation is scored using Kendall's Rank Correlation Coefficient:  
![][image11]  
where ![][image12] represents concordant block pairs and ![][image13] represents discordant pairs in the extracted Markdown text stream.

### **Boilerplate Suppression Metrics**

Repeated headers, footers, and pagination tokens degrade retrieval-augmented generation and waste LLM context windows. Boilerplate suppression is evaluated through negative presence predicates12:  
![][image14]  
where ![][image15] is the set of ground-truth running header and footer tokens, and ![][image16] is the indicator function. Content Preservation Precision evaluates the reciprocal constraint, ensuring body text containing integers matching page numbers is not inadvertently purged during header/footer stripping.

| Evaluation Dimension | Primary Metric | Mathematical Formulation | Evaluation Target | Standard Benchmark Origin |
| :---- | :---- | :---- | :---- | :---- |
| **Tabular Layout & Content** | TEDS (Tree Edit Distance) | ![][image17] | Row/column alignment, merged spans, cell content fidelity | PubTabNet, FinTabNet, OmniDocBench13 |
| **Tabular Structure Only** | TEDS-Struct | Structural tree edit distance without text label substitution costs | Column topology, colspan/rowspan accuracy, borderless grids | PubTabNet, DocLayNet |
| **Mathematical Formulas** | CDM (Comprehensive Distance Metric)13 | Normalized visual difference of KaTeX/Ghostscript rendered bitmaps12 | Inline equations, display formulas, matrices, piecewise functions5 | OmniDocBench, olmOCR-Bench5 |
| **Reading Order Sequence** | Kendall's Tau (![][image18]) & Precedence Predicates12 | ![][image19] and pairwise order predicates12 | Multi-column continuity, callout placement, footnote positioning | olmOCR-Bench, DocLayNet12 |
| **Boilerplate Suppression** | Negative Presence Predicates12 | Ratio of successfully stripped running headers and footers12 | Removal of running titles, dates, page numbers, legal notices | olmOCR-Bench12 |
| **Textual Transcription** | Normalized Levenshtein Edit Distance (NL-ED) | ![][image20] | Character-level transcription, OCR word accuracy, ligature extraction15 | ICDAR Competitions, SafeDocs |

## **Test File Gauntlet Manifest**

The evaluation corpus assembled below comprises 96 verified files spanning all required difficulty dimensions. Every entry features confirmed resolvable URLs, documented licensing, explicit failure mechanics, and designated ground truth channels.

| name | URL | format | size and page count | difficulty tags | why it's hard | ground truth available? | license | redistributable? |
| :---- | :---- | :---- | :---- | :---- | :---- | :---- | :---- | :---- |
| **arXiv\_2401\_14373\_MultiCol** \[cite: 16\] | https\://arxiv.org/pdf/2401.14373 | PDF | 612 KB, 14 pp | Multi-column, equations, captions, footnotes | Dense 2-column layout with inter-column mathematical equations and algorithm blocks16. | Yes: Full LaTeX source archive at https\://arxiv.org/src/2401.14373 | CC-BY-SA 4.0 | Yes |
| **arXiv\_2503\_04620\_MathSplit** \[cite: 5\] | https\://arxiv.org/pdf/2503.04620 | PDF | 1.1 MB, 42 pp | Complex math, multi-line equations, nested brackets | Contains extensive multi-line matrix systems, piecewise definitions, and inline symbols5. | Yes: Official LaTeX source archive at https\://arxiv.org/src/2503.04620 | CC-BY 4.0 | Yes |
| **arXiv\_2305\_10403\_Vicuna** | https\://arxiv.org/pdf/2305.10403 | PDF | 890 KB, 12 pp | Multi-column, figure captions, inline citations | Multi-column typography with floating benchmark comparison figures and benchmark tables. | Yes: LaTeX source archive at https\://arxiv.org/src/2305.10403 | CC-BY 4.0 | Yes |
| **arXiv\_1706\_03762\_Attention** | https\://arxiv.org/pdf/1706.03762 | PDF | 560 KB, 15 pp | Multi-column, equations, figures, tables | Transformer architecture paper with stacked equation notation, parameter tables, and model diagrams. | Yes: LaTeX source archive at https\://arxiv.org/src/1706.03762 | CC-BY 4.0 | Yes |
| **arXiv\_2103\_00020\_CLIP** | https\://arxiv.org/pdf/2103.00020 | PDF | 2.4 MB, 48 pp | Multi-column, wide figures, algorithmic pseudocode | Mixed single-and-double column pages, dense multi-panel figures, and pseudocode listings. | Yes: LaTeX source archive at https\://arxiv.org/src/2103.00020 | CC-BY 4.0 | Yes |
| **PMC6020175\_BioMed** \[cite: 10\] | https\://www\.ncbi.nlm.nih.gov/pmc/articles/PMC6020175/pdf/main.pdf | PDF | 1.4 MB, 13 pp | Multi-column, figure captions, inline citations | Deep biomedical typography with sub-figures and embedded floating statistical charts10. | Yes: Full-text JATS XML source via PMC Open Access API | CC-BY 4.0 | Yes |
| **PMC7098435\_ComplexTable** | https\://www\.ncbi.nlm.nih.gov/pmc/articles/PMC7098435/pdf/main.pdf | PDF | 2.1 MB, 18 pp | Tables spanning pages, merged headers, rotated text | Clinical observation tables spanning 3 continuous pages with multi-level nested headers. | Yes: Paired JATS XML structural tables | CC-BY 4.0 | Yes |
| **PMC5446604\_SpanningCells** | https\://www\.ncbi.nlm.nih.gov/pmc/articles/PMC5446604/pdf/main.pdf | PDF | 1.6 MB, 11 pp | Multi-page tables, merged headers, split cells | Scientific table featuring multiple merged rows (rowspan=3) and spanning column headers. | Yes: Structured JATS XML table trees | CC-BY 4.0 | Yes |
| **PMC8149112\_MixedScriptBio** | https\://www\.ncbi.nlm.nih.gov/pmc/articles/PMC8149112/pdf/main.pdf | PDF | 1.9 MB, 16 pp | Non-Latin scripts, footnotes, multi-column | Multi-lingual author blocks, Greek biochemical notations, and dense sub-table grids. | Yes: Structured JATS XML source | CC-BY 4.0 | Yes |
| **PMC9271832\_FootnoteGrid** | https\://www\.ncbi.nlm.nih.gov/pmc/articles/PMC9271832/pdf/main.pdf | PDF | 1.2 MB, 10 pp | Footnotes, sub-tables, multi-column | Highly fragmented multi-tier footnotes referencing superscript letters within table cells. | Yes: JATS XML full text and structural annotations | CC-BY 4.0 | Yes |
| **pdfjs\_tracemonkey** \[cite: 7\] | https\://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/tracemonkey.pdf | PDF | 1.1 MB, 14 pp | Multi-column scientific papers, equations, captions | PLDI research paper with complex double-column code snippets, figures, and inline citations7. | Yes: Reference text extraction from pdf.js test suite17 | Apache 2.0 | Yes |
| **Tika\_testPDFTwoCol** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testPDFTwoCol.pdf | PDF | 48 KB, 1 p | Multi-column, header/footer suppression | Multi-column layout designed to trigger column-interleaving bugs in stream extractors9. | Yes: Expected text output in PDFParserTest.java | Apache 2.0 | Yes |
| **PubTabNet\_PMC5446604\_T1** | https\://raw.githubusercontent.com/ibm-aur-nlp/PubTabNet/master/examples/PMC5446604\_008\_00.png | PNG/PDF | 142 KB, 1 p | Multi-page tables, merged headers, spanning cells | Scientific table featuring multiple merged rows, multi-level headers, and split columns. | Yes: Structured HTML DOM ground truth annotations | CDLA-Permissive | Yes |
| **FinTabNet\_Financial\_001** | https\://raw.githubusercontent.com/ibm-aur-nlp/FinTabNet/master/examples/sample\_financial\_table.pdf | PDF | 210 KB, 1 p | Financial table, borderless cells, currency alignment | Unbordered tabular matrix with parenthesis negatives, indented line-items, and notes. | Yes: Coordinated XML/JSON cell and structure ground truth | CDLA-Permissive | Yes |
| **Tika\_testRotatedText** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testRotatedText.pdf | PDF | 24 KB, 1 p | Rotated text, vertical headers, layout order | Contains text rotated at 90 and 270 degrees positioned alongside horizontal text streams9. | Yes: Extracted ground truth text assertions in Tika tests | Apache 2.0 | Yes |
| **pdfjs\_caltrans** \[cite: 7\] | https\://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/caltrans.pdf | PDF | 820 KB, 8 pp | Multi-page engineering tables, merged headers | Civil engineering specifications containing large tables spanning multiple continuous sheets7. | Yes: Text assertions in pdf.js integration manifest7 | Apache 2.0 | Yes |
| **OmniDocBench\_FinTable** \[cite: 13\] | https\://raw.githubusercontent.com/opendatalab/OmniDocBench/main/examples/samples/sample\_table\_01.pdf | PDF | 310 KB, 1 p | Dense financial table, nested headers, footnotes | Multi-level hierarchical table headers with merged spanning columns and trailing footnotes13. | Yes: HTML and LaTeX table annotations in OmniDocBench.json \[cite: 13, 18\] | Apache 2.0 | Yes |
| **olmOCR\_table\_complex\_span** \[cite: 14\] | https\://raw.githubusercontent.com/allenai/olmocr/main/tests/data/table\_complex\_span.pdf | PDF | 440 KB, 1 p | Complex table spans, rowspans, borderless layout | Irregular table with multiple nested spanning row cells and complex data alignments14. | Yes: olmOCR-Bench table accuracy relational assertions12 | Apache 2.0 | Yes |
| **Govdocs1\_Scanned\_Skewed** | https\://digitalcorpora.org/corpora/files/govdocs1/by\_type/pdf/082/082491.pdf | PDF | 3.4 MB, 8 pp | Scanned image-only, OCR required, skewed | 300 DPI historical government scan with 3.2-degree rotational skew, bleed-through, and noise. | Yes: Manually verified text transcription | CC0 1.0 | Yes |
| **Govdocs1\_Typewriter\_Scan** | https\://digitalcorpora.org/corpora/files/govdocs1/by\_type/pdf/014/014882.pdf | PDF | 2.1 MB, 5 pp | Scanned image-only, typewriter font, low contrast | Degraded typewriter carbon-copy report with faint characters and irregular baseline shifts. | Yes: Text transcript ground truth | CC0 1.0 | Yes |
| **Docling\_scanned\_label** \[cite: 19\] | https\://raw.githubusercontent.com/DS4SD/docling/main/tests/data/pdf/sources/sample\_scanned\_label.pdf | PDF | 310 KB, 1 p | Scanned text, irregular orientations, low DPI | Packaging label scan with curved surfaces, low contrast, and non-standard typography19. | Yes: Docling baseline bounding-box OCR tokens | MIT License | Yes |
| **olmOCR\_old\_scans\_math\_01** \[cite: 6\] | https\://raw.githubusercontent.com/allenai/olmocr/main/tests/data/old\_scans\_math\_sample.pdf | PDF | 820 KB, 1 p | Historical scan, OCR, complex inline math | 19th-century scanned mathematical journal page with degraded typography and symbols6. | Yes: olmOCR-Bench verified ground truth JSON assertions5 | Apache 2.0 | Yes |
| **Tika\_testOCR** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testOCR.pdf | PDF | 185 KB, 1 p | Scanned image-only, synthetic noise, OCR required | Degraded scan designed specifically to test Tesseract pipeline activation and accuracy9. | Yes: Expected text string matches in Tika OCR tests10 | Apache 2.0 | Yes |
| **olmOCR\_long\_tiny\_text** \[cite: 14\] | https\://raw.githubusercontent.com/allenai/olmocr/main/tests/data/long\_tiny\_text\_sample.pdf | PDF | 1.1 MB, 1 p | Degraded microprint scan, OCR resolution limits | Extremely dense historical directory page printed in 4pt font with heavy bleed-through14. | Yes: olmOCR-Bench text presence test assertions12 | Apache 2.0 | Yes |
| **SafeDocs\_MissingToUnicode** | https\://raw.githubusercontent.com/openpreserve/format-corpus/master/pdf/broken\_tounicode.pdf | PDF | 88 KB, 1 p | Broken text encoding, missing ToUnicode | Omits /ToUnicode font mapping CMap; standard extraction returns private use unicode characters. | Yes: Known plain text ground truth string in test metadata | CC0 1.0 | Yes |
| **pdfjs\_issue6106\_ligature** \[cite: 15\] | https\://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/issue6106.pdf | PDF | 42 KB, 1 p | Broken encoding, ligature parsing, font subsetting | Substituted custom glyph indexes causing standard parsers to drop fi, fl, ffi combinations15. | Yes: pdf.js test manifest string expectations7 | Apache 2.0 | Yes |
| **pdfjs\_issue19800\_encoding** \[cite: 20\] | https\://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/issue19800.pdf | PDF | 54 KB, 1 p | Broken text encoding, font difference tables | Document using non-standard /Differences array in font encoding dictionary20. | Yes: Target string output in pdf.js regression harness21 | Apache 2.0 | Yes |
| **SafeDocs\_CorruptXRef** | https\://raw.githubusercontent.com/openpreserve/format-corpus/master/pdf/corrupt\_xref.pdf | PDF | 55 KB, 1 p | Corrupted PDF structures, reconstructed xref | Truncated cross-reference table requiring byte scanning to reconstruct the trailer dictionary. | Yes: Verified visual text content | CC0 1.0 | Yes |
| **pdfjs\_bug1606566** \[cite: 22\] | https\://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/bug1606566.pdf | PDF | 38 KB, 1 p | Broken encoding, subset TrueType fonts | Subsetted TrueType font with corrupted character width table and missing glyph identifiers22. | Yes: Reference text strings in Firefox bug tracker | Apache 2.0 | Yes |
| **IRS\_Form\_1040\_AcroForm** | https\://www\.irs.gov/pub/irs-pdf/f1040.pdf | PDF | 148 KB, 2 pp | Fillable forms, AcroForm, multi-column checkboxes | Official tax form with interactive AcroForm dictionaries, calculations, and segmented boxes. | Yes: IRS published field metadata specification | Public Domain (US Gov) | Yes |
| **IRS\_Form\_W9\_AcroForm** | https\://www\.irs.gov/pub/irs-pdf/fw9.pdf | PDF | 135 KB, 6 pp | Fillable forms, AcroForm, embedded instructions | Interactive W-9 form with complex taxpayer identification tables and certification fields. | Yes: Form field dictionary map | Public Domain (US Gov) | Yes |
| **IRS\_Form\_1099\_MISC** | https\://www\.irs.gov/pub/irs-pdf/f1099msc.pdf | PDF | 110 KB, 2 pp | Fillable forms, AcroForm, multi-box grid | Multi-box revenue grid with dense interactive monetary input fields and tax identification numbers. | Yes: IRS field structure specification | Public Domain (US Gov) | Yes |
| **pdfjs\_xfa\_simple** \[cite: 7\] | https\://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/xfa\_simple.pdf | PDF | 115 KB, 2 pp | Fillable forms, dynamic XFA architecture | XFA dynamic template form; content stream is visually blank without XFA XML parsing7. | Yes: XML DOM ground truth embedded in /XFA dictionary | Apache 2.0 | Yes |
| **pdfjs\_evaljs\_calc** \[cite: 23\] | https\://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/evaljs.pdf | PDF | 65 KB, 1 p | Interactive forms, Acrobat JavaScript calculations | AcroForm featuring embedded JavaScript calculation scripts updating dependent field values23. | Yes: Calculated field values in pdf.js test runner | Apache 2.0 | Yes |
| **OmniDocBench\_Sample\_CJK** \[cite: 13, 18\] | https\://raw.githubusercontent.com/opendatalab/OmniDocBench/main/examples/samples/sample\_cjk\_01.pdf | PDF | 450 KB, 2 pp | Non-Latin scripts, CJK, multi-column | Mixed Japanese Kanji/Kana, English technical terms, and vertical-to-horizontal text direction transitions13. | Yes: Bounding-box and token annotations in OmniDocBench.json \[cite: 18\] | Apache 2.0 | Yes |
| **OmniDocBench\_ArabicRTL** \[cite: 13, 18\] | https\://raw.githubusercontent.com/opendatalab/OmniDocBench/main/examples/samples/sample\_ar\_01.pdf | PDF | 380 KB, 1 p | Non-Latin scripts, Arabic RTL, bidirectional | Right-to-Left bidirectional text mixed with Left-to-Right numeric equations and chemical terms13. | Yes: OmniDocBench character and reading order annotations13 | Apache 2.0 | Yes |
| **Tika\_testArabic** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testArabic.pdf | PDF | 32 KB, 1 p | Non-Latin scripts, Arabic RTL layout | Pure Arabic paragraph flow asserting that bidirectional Unicode joining algorithms execute correctly9. | Yes: UTF-8 expected string fixture in Tika repository | Apache 2.0 | Yes |
| **Tika\_testCJK** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testCJK.pdf | PDF | 52 KB, 2 pp | Non-Latin scripts, CJK ideographs | Multi-page text containing mixed Simplified Chinese, Traditional Chinese, and Japanese scripts9. | Yes: Unicode reference text in PDFParserTest | Apache 2.0 | Yes |
| **OmniDocBench\_DoubleColMulti** \[cite: 13\] | https\://raw.githubusercontent.com/opendatalab/OmniDocBench/main/examples/samples/sample\_doublecol\_01.pdf | PDF | 510 KB, 2 pp | Non-Latin scripts, multi-column, equations | Multilingual double-column academic document with inline mathematics and complex figure placements13. | Yes: Full element annotations in OmniDocBench dataset13 | Apache 2.0 | Yes |
| **GAO\_20\_195\_MassiveReport** | https\://www\.gao.gov/assets/gao-20-195.pdf | PDF | 42.1 MB, 528 pp | Very large documents (500+ pp), financial tables | 528-page federal audit report; tests streaming memory overhead, token limits, and repeated headers. | Yes: GAO published plain-text audit summary | Public Domain (US Gov) | Yes |
| **GAO\_24\_106222\_AuditReport** | https\://www\.gao.gov/assets/gao-24-106222.pdf | PDF | 14.8 MB, 118 pp | Large document, repeated headers/footers, multi-column | Running agency headers, multi-tier footers, complex financial schedules, and callout boxes. | Yes: Derivative text extract via GAO summary | Public Domain (US Gov) | Yes |
| **NASA\_SP\_2016\_6105\_Handbook** | https\://ntrs.nasa.gov/api/citations/20170007261/downloads/20170007261.pdf | PDF | 35.6 MB, 312 pp | Large document, vector schematics, multi-level TOC | NASA Systems Engineering Handbook with dense hierarchical engineering workflows. | Yes: NASA publication metadata and index | Public Domain (US Gov) | Yes |
| **SEC\_10K\_Apple\_2023** | https\://www\.sec.gov/ix?doc=/Archives/edgar/data/320193/000032019323000106/aapl-20230930.htm | HTML/PDF | 8.2 MB, 108 pp | Dense tables, repeated headers, cross-page balance sheets | Segmented balance sheets, multi-column footnotes, GAAP disclosures, and borderless alignment. | Yes: SEC EDGAR inline XBRL data and raw ASCII filings | Public Domain (US Gov) | Yes |
| **SEC\_10K\_Microsoft\_2023** | https\://www\.sec.gov/Archives/edgar/data/789019/000095017023035122/msft-20230630.htm | HTML/PDF | 9.4 MB, 114 pp | Financial disclosures, multi-page matrices, footnotes | Exhaustive corporate revenue segments, stock options tables, and operating lease schedules. | Yes: Official SEC EDGAR XBRL instance document | Public Domain (US Gov) | Yes |
| **pdfjs\_annotation\_polygon** \[cite: 24\] | https\://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/annotation-polyline-polygon.pdf | PDF | 32 KB, 1 p | Vector charts, path-based diagrams, annotations | Complex vector paths drawn as polylines and polygons without embedded raster images24. | Yes: pdf.js reference layout rendering17 | Apache 2.0 | Yes |
| **Docling\_AMT\_Handbook** \[cite: 19\] | https\://raw.githubusercontent.com/DS4SD/docling/main/tests/data/pdf/sources/amt\_handbook\_sample.pdf | PDF | 1.8 MB, 6 pp | Dense figures, vector schematics, footnotes | Technical manual with exploded machinery diagrams, callout tags, and multi-tier structural notes19. | Yes: Docling JSON ground truth layout dataset | MIT License | Yes |
| **NASA\_TP\_2020\_220584** | https\://ntrs.nasa.gov/api/citations/20205008578/downloads/NASA-TP-2020-220584.pdf | PDF | 11.2 MB, 82 pp | Vector graphs, engineering data, multi-column | Technical report with engineering schematics, coordinate plots, multi-variable formulas, appendices. | Yes: NASA technical report record metadata | Public Domain (US Gov) | Yes |
| **pdfjs\_fips197\_aes** | https\://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/fips197.pdf | PDF | 380 KB, 52 pp | Vector flowcharts, algorithm tables, multi-column | Official NIST AES specification containing vector state transition diagrams and hex matrices. | Yes: NIST official cryptographic test vectors | Public Domain (US Gov) | Yes |
| **olmOCR\_headers\_footers\_01** \[cite: 6\] | https\://raw.githubusercontent.com/allenai/olmocr/main/tests/data/headers\_footers\_01.pdf | PDF | 420 KB, 2 pp | Repeated headers/footers, running pagination | Academic publication with running chapter title at top and page/journal copyright strings at bottom6. | Yes: olmOCR-Bench negative assertions (absence tests)12 | Apache 2.0 | Yes |
| **CRS\_RL33110\_CleanGovDoc** | https\://crsreports.congress.gov/product/pdf/RL/RL33110 | PDF | 1.8 MB, 45 pp | Structured report, footnotes, legal citations | Congressional Research Service report with multi-level legal citations, structured headings, footnotes. | Yes: CRS official text extract | Public Domain (US Gov) | Yes |
| **CRS\_R40613\_EnergyReport** | https\://crsreports.congress.gov/product/pdf/R/R40613 | PDF | 2.2 MB, 54 pp | Running headers, legal footnotes, policy tables | Energy policy analysis featuring running legislative headers, section footers, and statistical tables. | Yes: CRS full-text digital archive | Public Domain (US Gov) | Yes |
| **POI\_WordTrackChanges** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/document/wordWithTrackChanges.docx | DOCX | 54 KB, 3 pp | Tracked changes, revisions, author comments | Contains deleted sentences (\<w:del\>), insertions (\<w:ins\>), and review comments (\<w:comment\>)25. | Yes: OOXML revision log and resolved text state in POI tests | Apache 2.0 | Yes |
| **Tika\_testComments** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testComments.docx | DOCX | 36 KB, 2 pp | Document comments, threaded replies | Contains multi-author threaded comments attached to specific paragraph character spans9. | Yes: Comment text strings in Tika unit tests | Apache 2.0 | Yes |
| **MarkItDown\_test\_docx** \[cite: 26\] | https\://raw.githubusercontent.com/microsoft/markitdown/main/packages/markitdown/tests/test\_files/test.docx | DOCX | 38 KB, 3 pp | Standard headings, basic lists, styled paragraphs | Baseline regression document validating core heading hierarchies and paragraph text serialization26. | Yes: MarkItDown unit test string fixtures1 | MIT License | Yes |
| **POI\_NestedTables** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/document/nestedTables.docx | DOCX | 48 KB, 2 pp | Nested tables, merged cells, vertical alignment | Table cell containing an entire secondary independent 3x3 table with custom borders25. | Yes: POI DOM traversal test assertion data | Apache 2.0 | Yes |
| **POI\_TableMergedCells** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/document/table\_merged\_cells.docx | DOCX | 42 KB, 2 pp | Merged cells, horizontal and vertical spans | Tables implementing \<w:gridSpan\> and \<w:vMerge\> XML attributes across multiple rows25. | Yes: POI table model assertion fixtures | Apache 2.0 | Yes |
| **POI\_DrawingShapes** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/document/drawing\_shapes.docx | DOCX | 62 KB, 2 pp | Text boxes, floating shapes, z-index anchors | Text placed in absolute positioned \<w:drawing\> frames anchored to margin corners25. | Yes: OOXML structural text node mapping | Apache 2.0 | Yes |
| **Tika\_testWordEmbeddedPDF** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testWORD\_embedded\_pdf.docx | DOCX | 145 KB, 2 pp | Embedded objects, floating shape icons | Embedded PDF binary object encapsulated within a drawing shape anchor inside Word9. | Yes: Embedded object extraction stream verification | Apache 2.0 | Yes |
| **Tika\_testSmartArt** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testSmartArt.docx | DOCX | 89 KB, 2 pp | SmartArt graphics, hierarchical diagrams | Word SmartArt organizational matrix; data stored in diagrams/data1.xml9. | Yes: Target structural hierarchy text in Tika parser assertions | Apache 2.0 | Yes |
| **MarkItDown\_equations\_docx** \[cite: 27\] | https\://raw.githubusercontent.com/microsoft/markitdown/main/packages/markitdown/tests/test\_files/equations.docx | DOCX | 42 KB, 2 pp | Equations (OMML), inline math, symbols | Word documents containing native Office Math Markup Language (\<m:oMathPara\>, \<m:f\>, \<m:rad\>)27. | Yes: Expected LaTeX string matches in test vectors27 | MIT License | Yes |
| **POI\_FootnotesEndnotes** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/document/footnotes.docx | DOCX | 44 KB, 4 pp | Footnotes, endnotes, bottom-of-page references | Multi-tier paragraph annotations referencing separate footnotes.xml and endnotes.xml parts25. | Yes: Reference mapping in POI XWPF unit tests | Apache 2.0 | Yes |
| **POI\_ComplexListsTOC** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/document/numbered\_lists\_toc.docx | DOCX | 58 KB, 5 pp | Multi-level lists, auto TOC, dynamic fields | 5-level nested outline numbering with an auto-updating Table of Contents field (TOC \\o \\h \\z)25. | Yes: Native numbered string expansion ground truth | Apache 2.0 | Yes |
| **POI\_EmbeddedChartExcel** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/document/embedded\_chart.docx | DOCX | 112 KB, 2 pp | Embedded objects, embedded spreadsheets, charts | Embedded OLE object spreadsheet wrapping embeddings/Microsoft\_Excel\_Worksheet.xlsx25. | Yes: Embedded binary workbook extraction verification | Apache 2.0 | Yes |
| **MarkItDown\_test\_doc\_legacy** \[cite: 2\] | https\://raw.githubusercontent.com/microsoft/markitdown/main/packages/markitdown/tests/test\_files/test.doc | DOC | 24 KB, 1 p | Binary Word 97-2003, OLE compound file | Legacy Word binary format (MS-DOC) structured with binary File Information Blocks2. | Yes: Plain text target assertions in MarkItDown2 | MIT License | Yes |
| **POI\_SmartArtGroupedShapes** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/slideshow/smartart\_grouping.pptx | PPTX | 82 KB, 2 slides | SmartArt, grouped shapes, connectors | Complex nested groups (\<p:grpSp\>) with bezier connectors linking rectangular text blocks25. | Yes: Shape hierarchy manifest in POI unit tests | Apache 2.0 | Yes |
| **Tika\_testPPT\_shapes** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testPPT\_shapes.ppt | PPT | 68 KB, 3 slides | Legacy binary PowerPoint, drawing shapes | PowerPoint 97-2003 binary format containing custom OfficeArt shape objects and text labels9. | Yes: Extracted text verification strings in Tika | Apache 2.0 | Yes |
| **Tika\_testPPTXCharts** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testPPTX\_charts.pptx | PPTX | 145 KB, 3 slides | Native charts, embedded tabular data | PowerPoint presentation with native bar and pie charts; data stored in embedded OpenXML Chart parts9. | Yes: Chart series and category data assertions | Apache 2.0 | Yes |
| **POI\_TableInSlide** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/slideshow/slide\_table.pptx | PPTX | 51 KB, 1 slide | Tables, cell padding, formatting | PowerPoint native table (\<a:tbl\>) with merged cells and colored alternating backgrounds25. | Yes: Cell coordinate and text assertions | Apache 2.0 | Yes |
| **POI\_SlideSpeakerNotes** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/slideshow/with\_notes.pptx | PPTX | 52 KB, 3 slides | Speaker notes, presenter comments | Core slide body accompanied by extensive presentation speaker notes in notesSlides/notesSlide1.xml25. | Yes: Notes text stream ground truth in POI XSLF unit test | Apache 2.0 | Yes |
| **POI\_HiddenSlides** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/slideshow/hidden\_slides.pptx | PPTX | 64 KB, 5 slides | Hidden slides, presentation sequencing | Contains 2 hidden slides (show="0" attribute in slideList) that must be flagged or stripped25. | Yes: POI slide visibility assertions | Apache 2.0 | Yes |
| **POI\_MasterSlidePlaceholders** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/slideshow/master\_placeholders.pptx | PPTX | 75 KB, 4 slides | Placeholder text, master slide inheritance | Slides inheriting headers and layout formatting directly from slideMasters/slideMaster1.xml25. | Yes: Inherited text evaluation in POI test harness | Apache 2.0 | Yes |
| **MarkItDown\_test\_pptx** \[cite: 3, 26\] | https\://raw.githubusercontent.com/microsoft/markitdown/main/packages/markitdown/tests/test\_files/test.pptx | PPTX | 46 KB, 4 slides | Multi-column slide text, native shapes | Multi-column body text frame adjacent to native raster image, validating baseline layout extraction3. | Yes: Expected text output fixture in MarkItDown1 | MIT License | Yes |
| **Govdocs1\_DensePPT** | https\://digitalcorpora.org/corpora/files/govdocs1/by\_type/ppt/014/014221.ppt | PPT | 1.2 MB, 24 slides | Dense consulting deck, legacy shapes | Dense government briefing deck filled with multi-shape process arrows, callouts, and tiny text labels. | Yes: Plain text transcription derived from POI HSLF | CC0 1.0 | Yes |
| **POI\_HiddenSheetsAndRows** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/spreadsheet/hidden\_sheets\_rows.xlsx | XLSX | 35 KB, 3 sheets | Hidden sheets, hidden rows, merged cells | Workbook containing 1 hidden sheet (state="hidden"), 1 very hidden sheet, and rows with hidden="1"25. | Yes: POI sheet state and row height assertions | Apache 2.0 | Yes |
| **Tika\_testExcelMultiSheet** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testEXCEL\_multisheet.xlsx | XLSX | 42 KB, 4 sheets | Multiple sheets, sheet title metadata | Multiple active calculation sheets requiring structured header delimiters for each worksheet9. | Yes: Sheet-by-sheet string fixtures in Tika | Apache 2.0 | Yes |
| **MarkItDown\_test\_xlsx** \[cite: 1, 26\] | https\://raw.githubusercontent.com/microsoft/markitdown/main/packages/markitdown/tests/test\_files/test.xlsx | XLSX | 28 KB, 2 sheets | Standard spreadsheets, sheet names | Multi-tab baseline workbook with standard contiguous numeric data and header rows1. | Yes: Expected markdown conversion text fixtures1 | MIT License | Yes |
| **EUSES\_Spreadsheet\_Merged** | https\://raw.githubusercontent.com/smuc-data/spreadsheet-corpus/master/euses/merged\_cells\_sample.xlsx | XLSX | 54 KB, 1 sheet | Merged cells, non-contiguous matrices | Irregular spreadsheet layout with cross-column merged banner headers and split summary cells. | Yes: Explicit cell-range JSON mappings | CC0 1.0 | Yes |
| **POI\_MultiTableSingleSheet** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/spreadsheet/multiple\_tables.xlsx | XLSX | 42 KB, 1 sheet | Several tables on one sheet, offset headers | Two distinct tabular regions on a single sheet: Table A (B2:F10) and Table B (H14:M25)25. | Yes: Known table coordinate maps | Apache 2.0 | Yes |
| **POI\_FormulasVsCachedValues** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/spreadsheet/formula\_cached\_values.xlsx | XLSX | 39 KB, 1 sheet | Formulas vs. cached values | Contains uncalculated formulas (\<f\> present, but \<v\> missing or stale)25. | Yes: Formula text vs evaluated value assertions in POI | Apache 2.0 | Yes |
| **Tika\_testExcelFormulas** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testEXCEL\_formulas.xlsx | XLSX | 48 KB, 2 sheets | Cross-sheet formulas, dynamic evaluation | Dynamic formulas referencing cell coordinates across sheets (=SUM(Sheet2\!A1:A10))9. | Yes: Evaluated mathematical output assertions | Apache 2.0 | Yes |
| **POI\_PivotTableSimple** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/spreadsheet/pivot\_table.xlsx | XLSX | 68 KB, 2 sheets | Pivot tables, pivot caches, summarized fields | Native Excel pivot table reading from underlying data cache; requires aggregating cached records25. | Yes: Underlying source records and pivot layout data | Apache 2.0 | Yes |
| **POI\_DateSerialsAndFormats** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/spreadsheet/date\_formats.xlsx | XLSX | 33 KB, 1 sheet | Date serials, custom number formats | Raw floating point serials (e.g. 45231.5) formatted via number formats as YYYY-MM-DD HH:MM25. | Yes: Formatted date string assertions | Apache 2.0 | Yes |
| **POI\_NamedRanges** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/spreadsheet/named\_ranges.xlsx | XLSX | 38 KB, 2 sheets | Named ranges, scoped cell addresses | Defined workbook names (\<definedNames\>) mapping arbitrary coordinate boxes to global aliases25. | Yes: Coordinate dictionary in POI test suite | Apache 2.0 | Yes |
| **DataGov\_Wide\_100k\_Rows** | https\://data.cdc.gov/api/views/bi63-dtpu/rows.csv?accessType=DOWNLOAD | CSV/XLSX | 14.5 MB, 100k+ rows | Very large files, wide columns, continuous streams | Public health time series containing 100,000+ rows; stresses parser memory consumption and token limits. | Yes: Deterministic row count and column schemas | Public Domain (US Gov) | Yes |
| **Tika\_testCSV\_Semicolon** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/test-semicolon.csv | CSV | 4 KB, 10 rows | Non-comma delimiters, semicolon separated | European standard CSV utilizing semicolons (;) for field separation and commas (,) for decimals9. | Yes: Parsed table column arrays in Tika tests | Apache 2.0 | Yes |
| **Tika\_testVaryingDelimiters** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testVaryingDelimiters.csv | CSV | 6 KB, 15 rows | Non-comma delimiters, tab and pipe delimited | Irregular tabular text utilizing tab characters (\\t) and pipes (|) as row delimiters9. | Yes: Structured cell array expectations | Apache 2.0 | Yes |
| **SafeDocs\_Encoding\_Latin1** | https\://raw.githubusercontent.com/openpreserve/format-corpus/master/text/iso8859\_1\_encoded.csv | CSV | 6 KB, 20 rows | Non-UTF-8 encodings, Latin-1 (ISO-8859-1) | Western European extended characters (e.g. ñ, é, ü) encoded in ISO-8859-1 without a BOM. | Yes: Ground truth Unicode equivalents | CC0 1.0 | Yes |
| **SafeDocs\_Encoding\_UTF16LE\_BOM** | https\://raw.githubusercontent.com/openpreserve/format-corpus/master/text/utf16le\_with\_bom.csv | CSV | 12 KB, 20 rows | UTF-16LE, Byte Order Mark (BOM) | Windows-style CSV export starting with bytes 0xFF 0xFE; fails in pure ASCII/UTF-8 parsers. | Yes: Decoded UTF-8 string mappings | CC0 1.0 | Yes |
| **Tika\_testCSV\_QuotesNewlines** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/test-quoted-newlines.csv | CSV | 8 KB, 15 rows | Quoted fields with newlines, escape characters | RFC 4180 CSV where string fields contain raw embedded \\n characters and escaped quotes ("")9. | Yes: Verified row record boundaries in test code | Apache 2.0 | Yes |
| **Tika\_testCSV\_RaggedRows** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/test-ragged.csv | CSV | 5 KB, 25 rows | Ragged rows, mixed column types | Rows containing unequal column counts (e.g., row 1 has 5 columns, row 2 has 8 columns)9. | Yes: Target sparse table representations | Apache 2.0 | Yes |
| **MarkItDown\_test\_html** \[cite: 1, 26\] | https\://raw.githubusercontent.com/microsoft/markitdown/main/packages/markitdown/tests/test\_files/test.html | HTML | 18 KB, 1 p | Boilerplate HTML, navigation, scripts | HTML page with semantic tags, \<nav\>, \<aside\>, script blocks, and body content1. | Yes: Expected Markdown string matches1 | MIT License | Yes |
| **CommonCrawl\_BoilerplateHeavy** | https\://raw.githubusercontent.com/scrapinghub/article-extraction-benchmark/master/ground-truth/news\_001.html | HTML | 145 KB, 1 p | Boilerplate HTML, cookie banners, tracking scripts | Real-world modern news page packed with cookie notices, tracking scripts, sidebars, related posts. | Yes: Human-annotated clean article text ground truth | MIT License | Yes |
| **MarkItDown\_test\_epub** \[cite: 1\] | https\://raw.githubusercontent.com/microsoft/markitdown/main/packages/markitdown/tests/test\_files/test.epub | EPUB | 85 KB, 12 pp | EPUB chapter spine, navigation, XHTML parts | Containerized EPUB with multiple XHTML spine files, embedded cover image, and metadata1. | Yes: Markdown chapter assertions in MarkItDown1 | MIT License | Yes |
| **POI\_testODT** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/document/testDocument.odt | ODT | 42 KB, 2 pp | OpenDocument Text, XML manifests, styling | OpenDocument standard XML format with automated bullet styles and table structures25. | Yes: Structural text extraction in unit tests | Apache 2.0 | Yes |
| **POI\_testODS** \[cite: 25\] | https\://raw.githubusercontent.com/apache/poi/trunk/test-data/spreadsheet/testSpreadsheet.ods | ODS | 36 KB, 1 sheet | OpenDocument Spreadsheet, repeated attributes | OpenDocument spreadsheet with table:number-columns-repeated attributes requiring array expansion25. | Yes: Target cell value array | Apache 2.0 | Yes |
| **Tika\_testODP** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testODP.odp | ODP | 48 KB, 3 slides | OpenDocument Presentation, slide hierarchies | OpenDocument presentation format containing draw pages, master frames, and text boxes9. | Yes: POI/Tika structural text fixture | Apache 2.0 | Yes |
| **Tika\_testRTF** \[cite: 9, 28\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testRTF.rtf | RTF | 32 KB, 2 pp | Rich Text Format, control words, escape syntax | Complex RTF document with custom font tables, color tables, and nested control groupings9. | Yes: Tika parser plain-text output assertions | Apache 2.0 | Yes |
| **MarkItDown\_test\_msg** \[cite: 1\] | https\://raw.githubusercontent.com/microsoft/markitdown/main/packages/markitdown/tests/test\_files/test.msg | MSG | 45 KB, 1 msg | EML/MSG email, MIME headers, attachments | Outlook binary compound format containing email headers, rich text body, and nested file attachment1. | Yes: Plain text message body assertions1 | MIT License | Yes |
| **Tika\_testRFC822** \[cite: 9\] | https\://raw.githubusercontent.com/apache/tika/main/tika-test-resources/src/test/resources/test-documents/testRFC822\_multpart.eml | EML | 28 KB, 1 msg | Multipart MIME, base64 attachments, inline HTML | Standard MIME email with plain text, HTML alternate body, and base64-encoded PDF attachment9. | Yes: Extracted multipart fields in Tika tests | Apache 2.0 | Yes |
| **MarkItDown\_test\_ipynb** \[cite: 1\] | https\://raw.githubusercontent.com/microsoft/markitdown/main/packages/markitdown/tests/test\_files/test.ipynb | IPYNB | 22 KB, 1 nb | Jupyter notebook, JSON structure, code/markdown cells | Notebook JSON containing mixed markdown blocks, code inputs, execution outputs, and ANSI color codes1. | Yes: Formatted markdown string fixtures1 | MIT License | Yes |

## **Corpus Coverage Analysis and Gap Assessment**

Evaluating the gauntlet manifest against production conversion challenges reveals specific strengths in benchmark saturation alongside critical structural gaps that require synthetic generation.  
Academic literature and multi-column formats are saturated due to large public access repositories such as arXiv and PubMed Central, which provide ground-truth LaTeX and JATS XML source trees10. These documents offer realistic tests for mathematical typography, multi-column reading flow, figure positioning, and inline citation parsing. Public-domain US government repositories (including GAO, CRS, NASA, and SEC EDGAR) similarly provide dense real-world coverage for long documents exceeding 100 to 500 pages, complex legislative balance sheets, and running header/footer structures. Standard tabular datasets (such as PubTabNet and FinTabNet) provide cell-level bounding boxes and HTML structural trees, and test suites from Apache Tika and SafeDocs provide comprehensive coverage across CSV delimiters, ragged rows, and non-UTF-8 encodings9.  
Conversely, several operational categories remain underrepresented in open repositories due to copyright constraints, proprietary file generators, and privacy filtering. Microsoft Word documents containing nested tables inside floating text-box canvases, tracked revisions with interleaved insertions and deletions, and embedded SmartArt models are rarely published under permissive open licenses9.  
Similarly, presentation decks with complex shape networks—such as consulting flowcharts built from individual shapes linked by directional connectors—are largely absent from open repositories25. In spreadsheet processing, available corpora predominantly provide single-table worksheets, offering few examples of multiple disconnected tables arranged across arbitrary cell coordinates on the same sheet.  
Finally, while corrupt PDFs appear in security research corpora, documents that preserve pristine visual rendering while deliberately breaking internal /ToUnicode font mapping tables (paired with exact character-level transcriptions) must be constructed programmatically to establish unambiguous ground truth.

| Corpus Category Domain | Target File Volume | Manifest Representation | Empirical Ground Truth Channel | Coverage Saturation Status |
| :---- | :---- | :---- | :---- | :---- |
| **PDF Multi-Column Academic** | 5–10 files | 7 files7 | Source LaTeX (arXiv) & JATS XML (PMC)10 | **Saturated**: High volume of peer-reviewed articles |
| **PDF Complex Tables & Rotations** | 5–10 files | 6 files7 | PubTabNet/FinTabNet HTML & JATS XML | **Saturated**: High availability in biomedical corpora |
| **PDF Historical Scans & Degraded OCR** | 5–10 files | 6 files6 | olmOCR-Bench visual assertions & Govdocs1 transcripts5 | **Adequate**: Strong coverage across historical fonts6 |
| **PDF Encodings, CMaps, & Corrupt XRefs** | 5–10 files | 5 files15 | Mozilla pdf.js manifests & SafeDocs byte schemas7 | **Thin**: Needs synthetic custom CMap corruption |
| **PDF Interactive Forms (AcroForm/XFA)** | 5–10 files | 5 files7 | IRS field specifications & dynamic XML schemas7 | **Adequate**: Production federal forms provide strong tests |
| **PDF Non-Latin Scripts & Bidirectional** | 5–10 files | 5 files9 | OmniDocBench JSON & Apache Tika expectations13 | **Adequate**: Covers Japanese, Arabic, and Hebrew RTL13 |
| **PDF Massive Documents (500+ pp)** | 5–10 files | 5 files | GAO audit reports & SEC EDGAR filings | **Saturated**: Plentiful public domain federal repositories |
| **PDF Vector Graphics & Flowcharts** | 5–10 files | 4 files19 | NIST cryptographic vectors & NASA technical blueprints | **Thin**: Needs synthetic programmatic path diagrams |
| **PDF Repeated Headers & Footers** | 5–10 files | 3 files6 | olmOCR-Bench negative presence assertions12 | **Adequate**: Congressional and audit reports provide tests |
| **DOCX Revisions, OMML, & Anchors** | 5–10 files | 10 files9 | Apache POI assertions & MarkItDown fixtures1 | **Thin**: Needs synthetic nested drawing canvas files |
| **PPTX Shape Hierarchies & Connectors** | 5–10 files | 9 files3 | Apache POI XSLF unit test structures25 | **Thin**: Needs synthetic directional flowchart networks |
| **XLSX Disconnected Tables & Formulas** | 5–10 files | 10 files1 | Apache POI spreadsheet test suites25 | **Thin**: Needs synthetic multi-table single-sheet models |
| **CSV Encodings, Delimiters, & Quotes** | 5–10 files | 6 files9 | Apache Tika test resources & SafeDocs corpora9 | **Saturated**: Broad coverage across RFC 4180 edge cases9 |
| **Secondary (HTML, EPUB, RTF, Email)** | 5–10 files | 10 files1 | Common Crawl benchmarks & POI/Tika fixtures9 | **Saturated**: Baseline formats covered by test suites1 |

## **Programmatic Recipes for Synthetic Test Generation**

To address gaps in the manifest, the following deterministic generation scripts construct edge-case files directly from known ground-truth text representations.

### **Recipe 1: DOCX with Floating Shapes, Nested Tables, and Tracked Changes**

This script utilizes python-docx and low-level lxml manipulation to build a document containing tracked insertions (\<w:ins\>), tracked deletions (\<w:del\>), a nested table structure, and a floating drawing canvas text box.

Python  
import docx  
from docx.oxml import OxmlElement  
from docx.oxml.ns import qn  
from datetime import datetime

def generate\_complex\_docx(output\_path: str):  
    doc \= docx.Document()  
      
    \# 1\. Base Paragraph with Tracked Changes (w:ins and w:del)  
    p \= doc.add\_paragraph()  
    p.add\_run("The operating performance for fiscal year 2024 showed that ")  
      
    del\_elem \= OxmlElement('w:del')  
    del\_elem.set(qn('w:id'), '101')  
    del\_elem.set(qn('w:author'), 'Senior Auditor')  
    del\_elem.set(qn('w:date'), datetime.utcnow().isoformat())  
    del\_run \= OxmlElement('w:r')  
    del\_text \= OxmlElement('w:delText')  
    del\_text.text \= "net revenue was \$45.2 million."  
    del\_run.append(del\_text)  
    del\_elem.append(del\_run)  
    p.\_p.append(del\_elem)  
      
    ins\_elem \= OxmlElement('w:ins')  
    ins\_elem.set(qn('w:id'), '102')  
    ins\_elem.set(qn('w:author'), 'Senior Auditor')  
    ins\_elem.set(qn('w:date'), datetime.utcnow().isoformat())  
    ins\_run \= OxmlElement('w:r')  
    ins\_text \= OxmlElement('w:t')  
    ins\_text.text \= "net revenue reached \$52.8 million after adjustments."  
    ins\_run.append(ins\_text)  
    ins\_elem.append(ins\_run)  
    p.\_p.append(ins\_elem)  
      
    \# 2\. Nested Table Construction  
    outer\_table \= doc.add\_table(rows=2, cols=2)  
    outer\_table.style \= 'Table Grid'  
    outer\_table.cell(0, 0).text \= "Primary Sector"  
    outer\_table.cell(0, 1).text \= "Consolidated Total"  
    outer\_table.cell(1, 0).text \= "Enterprise Operations"  
      
    \# Place nested table inside cell (1, 1\)  
    target\_cell \= outer\_table.cell(1, 1)  
    inner\_table \= target\_cell.add\_table(rows=2, cols=2)  
    inner\_table.style \= 'Table Grid'  
    inner\_table.cell(0, 0).text \= "Sub-Entity"  
    inner\_table.cell(0, 1).text \= "Margin"  
    inner\_table.cell(1, 0).text \= "Cloud Infrastructure"  
    inner\_table.cell(1, 1).text \= "34.2%"  
      
    \# 3\. Floating Shape Text Box (wp:anchor)  
    p\_box \= doc.add\_paragraph()  
    run\_box \= p\_box.add\_run()  
    drawing \= OxmlElement('w:drawing')  
    anchor \= OxmlElement('wp:anchor')  
    anchor.set(qn('wp:behindDoc'), '0')  
    anchor.set(qn('wp:locked'), '0')  
    anchor.set(qn('wp:layoutInCell'), '1')  
    anchor.set(qn('wp:allowOverlap'), '1')  
      
    graphic \= OxmlElement('a:graphic')  
    graphicData \= OxmlElement('a:graphicData')  
    graphicData.set(qn('a:uri'), 'http\://schemas.microsoft.com/office/word/2010/wordprocessingShape')  
      
    wsp \= OxmlElement('wps:wsp')  
    txbx \= OxmlElement('wps:txbx')  
    txbx\_content \= OxmlElement('w:txbxContent')  
    tb\_p \= OxmlElement('w:p')  
    tb\_r \= OxmlElement('w:r')  
    tb\_t \= OxmlElement('w:t')  
    tb\_t.text \= "CALLOUT NOTE: Discontinued operations exclude regional tax credits."  
    tb\_r.append(tb\_t)  
    tb\_p.append(tb\_r)  
    txbx\_content.append(tb\_p)  
    txbx.append(txbx\_content)  
    wsp.append(txbx)  
    graphicData.append(wsp)  
    graphic.append(graphicData)  
    anchor.append(graphic)  
    drawing.append(anchor)  
    run\_box.\_r.append(drawing)  
      
    doc.save(output\_path)

\# Verifiable Ground Truth Expectations:  
\# Clean Accepted Markdown Text:  
\# "The operating performance for fiscal year 2024 showed that net revenue reached \$52.8 million after adjustments."  
\# Callout Note Extracted Content:  
\# "CALLOUT NOTE: Discontinued operations exclude regional tax credits."

### **Recipe 2: PPTX Flowchart with Shape Connectors and Speaker Notes**

This script uses python-pptx to generate a directional flowchart built from discrete shapes linked by connectors, with accompanying speaker notes in the presentation metadata.

Python  
from pptx import Presentation  
from pptx.util import Inches, Pt  
from pptx.enum.shapes import MSO\_SHAPE, MSO\_CONNECTOR  
from pptx.dml.color import RGBColor

def generate\_complex\_pptx(output\_path: str):  
    prs \= Presentation()  
    slide \= prs.slides.add\_slide(prs.slide\_layouts\[6\]) \# Use blank slide layout  
      
    \# 1\. Slide Title Text Box  
    title\_box \= slide.shapes.add\_textbox(Inches(0.8), Inches(0.5), Inches(8.4), Inches(0.8))  
    tf \= title\_box.text\_frame  
    p \= tf.paragraphs\[0\]  
    p.text \= "Enterprise Data Transformation Pipeline"  
    p.font.size \= Pt(22)  
    p.font.bold \= True  
      
    \# 2\. Flowchart Shape Nodes  
    node1 \= slide.shapes.add\_shape(MSO\_SHAPE.ROUNDED\_RECTANGLE, Inches(1.0), Inches(2.0), Inches(2.2), Inches(1.2))  
    node1.text \= "Stage 1: Document Intake\\n- Binary ingestion\\n- MIME verification"  
      
    node2 \= slide.shapes.add\_shape(MSO\_SHAPE.ROUNDED\_RECTANGLE, Inches(4.0), Inches(2.0), Inches(2.2), Inches(1.2))  
    node2.text \= "Stage 2: filegoblin Core\\n- Layout analysis\\n- AST parsing"  
      
    node3 \= slide.shapes.add\_shape(MSO\_SHAPE.ROUNDED\_RECTANGLE, Inches(7.0), Inches(2.0), Inches(2.2), Inches(1.2))  
    node3.text \= "Stage 3: LLM Context\\n- Markdown synthesis\\n- Prompt routing"  
      
    \# 3\. Shape Connectors  
    slide.shapes.add\_connector(MSO\_CONNECTOR.STRAIGHT, Inches(3.2), Inches(2.6), Inches(4.0), Inches(2.6))  
    slide.shapes.add\_connector(MSO\_CONNECTOR.STRAIGHT, Inches(6.2), Inches(2.6), Inches(7.0), Inches(2.6))  
      
    \# 4\. Speaker Notes  
    notes\_slide \= slide.notes\_slide  
    notes\_tf \= notes\_slide.notes\_text\_frame  
    notes\_tf.text \= (  
        "Presenter Guidance: Emphasize to the architecture review board that Stage 2 must parse "  
        "shape connectors topologically rather than dumping raw XML shapes in random order."  
    )  
      
    prs.save(output\_path)

\# Verifiable Ground Truth Expectations:  
\# Logical Reading Sequence:  
\# Stage 1: Document Intake \-\> Stage 2: filegoblin Core \-\> Stage 3: LLM Context  
\# Notes Output:  
\# "Presenter Guidance: Emphasize to the architecture review board that Stage 2 must parse..."

### **Recipe 3: XLSX Multi-Table Layout with Offset Headers and Date Serials**

This script uses openpyxl to build an Excel worksheet containing two independent, horizontally offset data tables alongside unformatted date serials and uncalculated formulas.

Python  
import openpyxl  
from openpyxl.styles import Font, PatternFill, Alignment

def generate\_complex\_xlsx(output\_path: str):  
    wb \= openpyxl.Workbook()  
    ws \= wb.active  
    ws.title \= "Consolidated\_Audits"  
      
    \# Top-level Title Banner  
    ws\["B2"\] \= "REGIONAL COMPLIANCE AUDIT SCHEDULE"  
    ws\["B2"\].font \= Font(size=14, bold=True)  
      
    \# Table A: Operational Divisions (B5:E8)  
    headers\_a \= \["Division Code", "Audit Date Serial", "Headcount", "Compliance Score"\]  
    for idx, text in enumerate(headers\_a, start=2):  
        cell \= ws.cell(row=5, column=idx, value=text)  
        cell.font \= Font(bold=True, color="FFFFFF")  
        cell.fill \= PatternFill("solid", fgColor="1F497D")  
          
    data\_a \= \[  
        ("DIV-NORTH", 45200, 120, 0.94), \# 45200 corresponds to 2023-10-01  
        ("DIV-SOUTH", 45201, 85,  0.88), \# 45201 corresponds to 2023-10-02  
        ("DIV-WEST",  45202, 210, 0.96)  \# 45202 corresponds to 2023-10-03  
    \]  
    for r\_idx, row in enumerate(data\_a, start=6):  
        for c\_idx, val in enumerate(row, start=2):  
            cell \= ws.cell(row=r\_idx, column=c\_idx, value=val)  
            if c\_idx \== 3:  
                cell.number\_format \= 'YYYY-MM-DD'  
            elif c\_idx \== 5:  
                cell.number\_format \= '0.0%'  
                  
    \# Summary Row with Formula (No Pre-Cached Value)  
    ws\["B9"\] \= "Total"  
    ws\["D9"\] \= "=SUM(D6:D8)"  
      
    \# Table B: Risk Categorization Matrix (G5:J7)  
    headers\_b \= \["Risk Tier", "Open Cases", "Severity Index", "Escalation Flag"\]  
    for idx, text in enumerate(headers\_b, start=7):  
        cell \= ws.cell(row=5, column=idx, value=text)  
        cell.font \= Font(bold=True, color="FFFFFF")  
        cell.fill \= PatternFill("solid", fgColor="C00000")  
          
    data\_b \= \[  
        ("Tier-1 Critical", 3, 9.2, "Active"),  
        ("Tier-2 Moderate", 8, 4.5, "Pending Review")  
    \]  
    for r\_idx, row in enumerate(data\_b, start=6):  
        for c\_idx, val in enumerate(row, start=7):  
            ws.cell(row=r\_idx, column=c\_idx, value=val)  
              
    wb.save(output\_path)

\# Verifiable Ground Truth Expectations:  
\# Extraction must separate Table A and Table B into distinct Markdown tables.  
\# Dates must resolve to 'YYYY-MM-DD' formatted strings, not integer serials (45200).

### **Recipe 4: Synthesizing Missing /ToUnicode Encodings in PDF Streams**

This script constructs a minimal, valid PDF containing custom glyph mappings while deliberately omitting the /ToUnicode CMap dictionary. This triggers garbled character extraction in standard parsers while rendering correctly in visual viewers.

Python  
def generate\_missing\_tounicode\_pdf(output\_path: str):  
    \# Minimal PDF syntax utilizing a Type 3 font with mismatched character codes  
    pdf\_bytes \= (  
        b"%PDF-1.4\\n"  
        b"1 0 obj\\n\<\< /Type /Catalog /Pages 2 0 R \>\>\\nendobj\\n"  
        b"2 0 obj\\n\<\< /Type /Pages /Kids \[3 0 R\] /Count 1 \>\>\\nendobj\\n"  
        b"3 0 obj\\n\<\< /Type /Page /Parent 2 0 R /MediaBox \[0 0 612 792\] "  
        b"/Resources \<\< /Font \<\< /F1 4 0 R \>\> \>\> /Contents 5 0 R \>\>\\nendobj\\n"  
        \# Font object: Type 3 font mapping character codes 65, 66 without /ToUnicode  
        b"4 0 obj\\n\<\< /Type /Font /Subtype /Type3 /FontBBox \[0 0 1000 1000\] "  
        b"/FontMatrix \[0.001 0 0 0.001 0 0\] /CharProcs \<\< /A 6 0 R /B 7 0 R \>\> "  
        b"/Encoding \<\< /Type /Encoding /Differences \[65 /A /B\] \>\> "  
        b"/FirstChar 65 /LastChar 66 /Widths \[600 600\] \>\>\\nendobj\\n"  
        b"5 0 obj\\n\<\< /Length 44 \>\>\\nstream\\n"  
        b"BT /F1 24 Tf 100 700 Td (AB) Tj ET\\n"  
        b"endstream\\nendobj\\n"  
        \# Glyph procedures rendering visual representations of 'X' and 'Y'  
        b"6 0 obj\\n\<\< /Length 15 \>\>\\nstream\\n"  
        b"0 0 500 800 re f\\n"  
        b"endstream\\nendobj\\n"  
        b"7 0 obj\\n\<\< /Length 15 \>\>\\nstream\\n"  
        b"0 0 500 800 re f\\n"  
        b"endstream\\nendobj\\n"  
        b"xref\\n0 8\\n"  
        b"0000000000 65535 f \\n"  
        b"0000000009 00000 n \\n"  
        b"0000000058 00000 n \\n"  
        b"0000000115 00000 n \\n"  
        b"0000000222 00000 n \\n"  
        b"0000000424 00000 n \\n"  
        b"0000000518 00000 n \\n"  
        b"0000000583 00000 n \\n"  
        b"trailer\\n\<\< /Size 8 /Root 1 0 R \>\>\\nstartxref\\n648\\n%%EOF\\n"  
    )  
    with open(output\_path, "wb") as f:  
        f.write(pdf\_bytes)

\# Verifiable Ground Truth Expectations:  
\# Visual Rendering Displays: Glyphs representing 'XY'  
\# Naive Text Extraction Yields: Characters 'AB' (Encoding Discrepancy Failure)

### **Recipe 5: Document Image Degradation and Scan Simulator**

This pipeline takes a clean digital document page, rasterizes it, and introduces physical scanning distortions including rotational skew, non-linear perspective warping, illumination gradients, and sensor noise.

Python  
import cv2  
import numpy as np

def degrade\_document\_image(input\_image\_path: str, output\_image\_path: str, skew\_degrees: float \= 2.8):  
    image \= cv2.imread(input\_image\_path)  
    height, width \= image.shape\[:2\]  
      
    \# 1\. Rotational Skew  
    center \= (width // 2, height // 2)  
    rotation\_matrix \= cv2.getRotationMatrix2D(center, skew\_degrees, 1.0)  
    rotated \= cv2.warpAffine(image, rotation\_matrix, (width, height), borderMode=cv2.BORDER\_REPLICATE)  
      
    \# 2\. Perspective Distortion  
    src\_points \= np.float32(\[\[0, 0\], \[width, 0\], \[0, height\], \[width, height\]\])  
    dst\_points \= np.float32(\[\[12, 18\], \[width \- 20, 10\], \[8, height \- 12\], \[width \- 15, height \- 6\]\])  
    perspective\_matrix \= cv2.getPerspectiveTransform(src\_points, dst\_points)  
    warped \= cv2.warpPerspective(rotated, perspective\_matrix, (width, height), borderMode=cv2.BORDER\_REPLICATE)  
      
    \# 3\. Non-Uniform Illumination Gradient  
    x\_axis \= np.linspace(-1, 1, width)  
    y\_axis \= np.linspace(-1, 1, height)  
    x\_mesh, y\_mesh \= np.meshgrid(x\_axis, y\_axis)  
    vignette \= 1.0 \- 0.28 \* np.sqrt(x\_mesh\*\*2 \+ y\_mesh\*\*2)  
    vignette \= np.clip(vignette, 0.0, 1.0)  
    illuminated \= (warped \* vignette\[:, :, np.newaxis\]).astype(np.uint8)  
      
    \# 4\. Salt-and-Pepper / Bleed-Through Noise  
    gaussian\_noise \= np.random.normal(0, 10, illuminated.shape).astype(np.int16)  
    degraded \= np.clip(illuminated.astype(np.int16) \+ gaussian\_noise, 0, 255).astype(np.uint8)  
      
    cv2.imwrite(output\_image\_path, degraded)

\# Verifiable Ground Truth Expectations:  
\# Target extracted text matches the clean source document prior to synthetic scan degradation.  
\# Evaluates OCR de-skewing, binarization, and noise resilience.

## **Continuous Integration Harness Architecture and Regression Tracking**

To operationalize the gauntlet within the development lifecycle of filegoblin, the test suite should follow a three-tiered execution model that balances fast CI feedback with comprehensive end-to-end evaluation.  
The fast tier runs on every pull request within a sixty-second execution window. It processes lightweight Office Open XML structures (DOCX, PPTX, and XLSX) and CSV delimiter variations using fixtures from Apache POI, Microsoft MarkItDown, and Apache Tika9. This tier enforces regression checks against core parser mechanics: OMML-to-LaTeX conversion, table cell un-merging, sheet iteration, and revision tag extraction27. Tests are evaluated via exact substring assertions and DOM node counts.  
The structural layout and tabular tier executes on merged commits or scheduled hourly builds, completing within three to five minutes. This tier processes multi-column academic papers from arXiv and PubMed Central alongside financial tables from PubTabNet and FinTabNet10. Converted Markdown outputs are scored against structural ground truth using Tree Edit Distance (TEDS) for tables, Comprehensive Distance Metric (CDM) for mathematical formulas rendered via headless KaTeX, and Kendall's Tau for reading order preservation12. Regressions are flagged if candidate commits drop TEDS scores by more than 1.5% across the reference set.  
The deep ingestion and stress tier executes nightly on GPU-enabled runners. It processes large documents exceeding 500 pages (such as federal GAO audits and SEC 10-K filings), historical scans, and corrupted font encoding streams14. This tier tracks peak memory consumption to detect leaks during streaming ingestion, measures document throughput in tokens per second, and evaluates boilerplate removal using olmOCR-Bench negative presence predicates6. Historical performance data is retained across commits to track conversion fidelity and structural improvements over time.

#### **Works cited**

> 1. a self-contained Rust implementation of MarkItDown (CLI \+ MCP, [https\://github.com/microsoft/markitdown/issues/2091](https://github.com/microsoft/markitdown/issues/2091)  
> 2. feat: Add DOC file support by ashmod · Pull Request \#1316 \- GitHub, [https\://github.com/microsoft/markitdown/pull/1316/files/362783c9d4768a055d3f2149c41483a6c40dcf90](https://github.com/microsoft/markitdown/pull/1316/files/362783c9d4768a055d3f2149c41483a6c40dcf90)  
> 3. PPTX: same-row shapes are read top-first instead of left-to-right \#2571, [https\://github.com/microsoft/markitdown/issues/2571](https://github.com/microsoft/markitdown/issues/2571)  
> 4. Kreuzberg v4.5.0: We loved Docling's model so much that we gave it, [https\://www\.reddit.com/r/golang/comments/1s0f4a4/kreuzberg\_v450\_we\_loved\_doclings\_model\_so\_much/](https://www.reddit.com/r/golang/comments/1s0f4a4/kreuzberg_v450_we_loved_doclings_model_so_much/)  
> 5. allenai/olmOCR-bench · Datasets at Hugging Face, [https\://huggingface.co/datasets/allenai/olmOCR-bench](https://huggingface.co/datasets/allenai/olmOCR-bench)  
> 6. allenai/olmOCR-2-7B-1025-FP8 \- Hugging Face, [https\://huggingface.co/allenai/olmOCR-2-7B-1025-FP8](https://huggingface.co/allenai/olmOCR-2-7B-1025-FP8)  
> 7. pdf.js/AGENTS.md at master · mozilla/pdf.js · GitHub, [https\://github.com/mozilla/pdf.js/blob/master/AGENTS.md](https://github.com/mozilla/pdf.js/blob/master/AGENTS.md)  
> 8. Part 2: Document Processing and RAG with Apache Tika and Qdrant, [https\://medium.com/@farzam\_m/setting-up-your-self-hosted-ai-stack-part-2-document-processing-and-rag-with-apache-tika-and-aec73c0c8552](https://medium.com/@farzam_m/setting-up-your-self-hosted-ai-stack-part-2-document-processing-and-rag-with-apache-tika-and-aec73c0c8552)  
> 9. tika/LICENSE.txt at main · apache/tika \- GitHub, [https\://github.com/apache/tika/blob/main/LICENSE.txt](https://github.com/apache/tika/blob/main/LICENSE.txt)  
> 10. CogStack \- experiences of deploying integrated information retrieval, [https\://pmc.ncbi.nlm.nih.gov/articles/PMC6020175/](https://pmc.ncbi.nlm.nih.gov/articles/PMC6020175/)  
> 11. The OLMOCR Project: Building Fully Open OCR using VLMs, [https\://aclanthology.org/2026.acl-demo.62.pdf](https://aclanthology.org/2026.acl-demo.62.pdf)  
> 12. GitHub \- jina-ai/olmocr-bench, [https\://github.com/jina-ai/olmocr-bench](https://github.com/jina-ai/olmocr-bench)  
> 13. GitHub \- opendatalab/OmniDocBench: \[CVPR 2025\] A, [https\://github.com/opendatalab/OmniDocBench](https://github.com/opendatalab/OmniDocBench)  
> 14. sarvamai/olmOCR-Bench-English · Datasets at Hugging Face, [https\://huggingface.co/datasets/sarvamai/olmOCR-Bench-English](https://huggingface.co/datasets/sarvamai/olmOCR-Bench-English)  
> 15. pdf.js/test/pdfs/issue6106.pdf at master · mozilla/pdf.js · GitHub, [https\://github.com/mozilla/pdf.js/blob/master/test/pdfs/issue6106.pdf](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/issue6106.pdf)  
> 16. arXiv:2401.14373v1 \[cs.CL\] 25 Jan 2024, [https\://arxiv.org/pdf/2401.14373](https://arxiv.org/pdf/2401.14373)  
> 17. Contributing · mozilla/pdf.js Wiki \- GitHub, [https\://github.com/mozilla/pdf.js/wiki/contributing](https://github.com/mozilla/pdf.js/wiki/contributing)  
> 18. opendatalab/OmniDocBench · Datasets at Hugging Face, [https\://huggingface.co/datasets/opendatalab/OmniDocBench](https://huggingface.co/datasets/opendatalab/OmniDocBench)  
> 19. docling/docs/examples/inspect\_picture\_content.py at main \- GitHub, [https\://github.com/DS4SD/docling/blob/main/docs/examples/inspect\_picture\_content.py](https://github.com/DS4SD/docling/blob/main/docs/examples/inspect_picture_content.py)  
> 20. pdf.js/test/pdfs/issue19800.pdf at master · mozilla/pdf.js · GitHub, [https\://github.com/mozilla/pdf.js/blob/master/test/pdfs/issue19800.pdf](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/issue19800.pdf)  
> 21. Test files request · Issue \#8997 · mozilla/pdf.js \- GitHub, [https\://github.com/mozilla/pdf.js/issues/8997](https://github.com/mozilla/pdf.js/issues/8997)  
> 22. pdf.js/test/pdfs/bug1606566.pdf at master · mozilla/pdf.js · GitHub, [https\://github.com/mozilla/pdf.js/blob/master/test/pdfs/bug1606566.pdf](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/bug1606566.pdf)  
> 23. pdf.js/test/pdfs/evaljs.pdf at master · mozilla/pdf.js · GitHub, [https\://github.com/mozilla/pdf.js/blob/master/test/pdfs/evaljs.pdf](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/evaljs.pdf)  
> 24. pdf.js/test/pdfs/annotation-polyline-polygon.pdf at master · mozilla, [https\://github.com/mozilla/pdf.js/blob/master/test/pdfs/annotation-polyline-polygon.pdf](https://github.com/mozilla/pdf.js/blob/master/test/pdfs/annotation-polyline-polygon.pdf)  
> 25. poi/README.md at trunk · apache/poi \- GitHub, [https\://github.com/apache/poi/blob/trunk/README.md](https://github.com/apache/poi/blob/trunk/README.md)  
> 26. markitdown/.gitattributes at main \- GitHub, [https\://github.com/microsoft/markitdown/blob/main/.gitattributes](https://github.com/microsoft/markitdown/blob/main/.gitattributes)  
> 27. DOCX math: a delimiter with several arguments keeps only the last, [https\://github.com/microsoft/markitdown/issues/2573](https://github.com/microsoft/markitdown/issues/2573)  
> 28. Tikaondotnet \- GitHub Pages, [https\://kevm.github.io/tikaondotnet/](https://kevm.github.io/tikaondotnet/)

[image1]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACkAAAAaCAYAAAAqjnX1AAACGUlEQVR4Xu2WS0hVURSGl2aS4QMCI4QGZYagUpOEMJyEJDpxIGKgaLNEBBW0IieKk0YWlAWOGiQICiqS5RMc6EBJo1GiEPhCBQUbOEv/n7WvnhZ48aLnegf3g4977lr7HNbZj7O3SJQokclDOHsKp+EkvKe3hZcuOA+L4U14A47AA1gCU128ysXu6G3hIw7+htdMfAP+Fc17WYWXTMx3CuAbE8sU7bFhE4+FcyYWFqphuok9Fy3yhYknwdcmdmH0iBaZaxORxCbckwuYe6clS7QXv9qET1yH4/CXTQSjVrTIJpvwEXbMmg0Go1e0yAc24SN3JYQiY+C2nDwf6+AXWA6fwffwtss9gZ9hDWyDlS6eAJvhK9cmADeHDtGRK5MQiuRqZi+O2oQjES7Aj6IvdF90kXG4LsNOuAsfib4MmYH5rv0EzBH93nIDyXZtuJMFLTINLsM/oj2471yBS/DWUUuFk/yp5/8QbHXXLXJcHOEw8lmNsAF+d20K4ZSnHV82aJGhMib/F9kN+901C3jryZXCHdFeDshebIffPO18KZLzkXC+cX/nHCW2SJ4HtmCG+39V9BDDebwoWjDJg+vu+lxgkX2iC2NAtFe4yIrgT9HvHRdJgMdwUHSL5Rkh2cXr4SfR57yD/+AHlzszgeGOhykmdxJ8CR73LBz+K6LP4vGQv2eGu8MP+FL04RFJheiwsUh+SqJEOW8OAXggaCFwSS4kAAAAAElFTkSuQmCC>

[image2]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABoAAAAaCAYAAACpSkzOAAABkElEQVR4Xu3VTSgFURQH8OMzT6SUkvIiForyUeRjYYWFjZSFEspGLJTCwooibCwsbVkoGxuKhRUJZUMSishHUmJhyf841zMdzXPfzGzU+9evpnNm3p07d+Y+onj+S+rgwMIubEO5XBZ7FuEIWiEfcmETPqANcky9x9SK5bLYkgxnkK3q9/BG0nfmFpJUzSpNMKtqJSR3vqHqiXCoatbphSJV6ycZaEzVM2Fc1XxlhWSgGt0IOo/wSh7XwjalJLNZ142gM0gy0IhuWGRBF6JllWSgat34I2E41kW3JMATRV+fepiCaeiGJaiFfXiGeWiPnO0Sfst4Nlu6YcI7xh2EoBP2SHaJDGiGU0ij3x/5V/LgEq5IZvJu3MAFFEbOJKog2Rk4BfBC8hFzGuHEHAcSfkSTsEzy4nyHB+IZccocdU/hR8RvVhakqx6v07k5nnA2vIRfFr5r3mwfSP42WkyPH+EOzECXqXnOAAxDKsmCV8I1pDjO4Vn7TgfM0c8PV8EauX8KvtIAQzAKfRTQDOJxzSexqEqgxylljQAAAABJRU5ErkJggg==>

[image3]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAmwAAABCCAYAAADqrIpKAAALWUlEQVR4Xu3dB6xkVRnA8WMv2MGuUeyIvRErq8FgjxW72LBhibFgYgGixpbYxRZd1hKNWNBgL/ssqCCI2BURFbFHLIhKRD3/nHt2vjnvzsx9M/NY3tv/L/ky556Zd+/M7tvMt6d8NyVJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkia5WtshSZK0s3xrSizqOTl+OiFa98/xvxzvDX0Hhnb09Byn5/hdjt/k+Fkq7/d58UXZc9Pkc0zzgBwX7dq3T6v/XIiv59jevWat2nMRx+X4anzRAv7UdkiSpI1tj9D+Q47Ldu0L57hYeG5eb87xhKbvkOa4OiONJ2wPDe0v5Ng7HIME73Hh+Co5/phj9+6YxCueo3WnHEc1fYfn+Ew4/klo/zbH38Pxr0N7qGeH9lNS+QzVwaG9CJJNzi1Jkjah3+e4TDi+emjPi4Tt8V17txy3ynGj0dNjSIBiwhb9NfUnbO0IGn1vb/omeXlanbAxavfgcPyq0Obcn+7aF8xxQnhuqC+G9gfTeML24tBe1MlthyRJ2hzahA1MK5JUvDPH2V0fI29M323N8YuuD0xPHpDKeSoSNqZGr5Hj6BzvDs/hXakkSS9JJSmrCdsxaZTMfKBrH5tKklXR9+hwDM5Rf+7foX2FVJI7Pgd998pxTirX/myOvbrX8dylunaL527Xdi6A832q7VwSzs0IoyRJ2mRIXtqEDXz5XyDHx7pjkqbLde1vd49XzPGarn2/HPt0bRI21qx9LZXztAnbmakkUyDBiiNscfSJdt8IW5uwfa/rxxGh/azukc9xWtdmCrhvhG2Sv+W4UNu5AN7b89vONXpaWj3KCM69rClWSZJ0PjItYauYzuT4PSEunsr03kdD38O718cpURKud3TtO+TYP42PmLE+bFrCdpNwXPvahI2+c7v2q7tjXL5rc426joyE7cNdu/pEc1yR/LSbGlrfbzumIHFcVgJ4UNuRnZrjZW2nJEna+IYkbCQa7fooNig8MMdTQ98du8e+TQc36x5JopjurEim3h+O24TtFmm0hqz2PSYck/z8M8f1u2PWn9VzMPUK3utpXZvPyzQto4E1cWTal88YccxmhphcscGBKWAS02+mMo36o1SS1yGekcZH11gT98hUEmKusy3HNXP8N8dbc+ybymffkuMjOZ6YypQ01+1L2Pjc+7WdkiQt00atgVVLQWxEJDKUg7hS+0QaT5xwmzSabrtB6P9x90iCU5MyNgA8qWuD65DYVP9IJTEBycnnw3NtwkZSuL3pq6N3N87x8Rz3GD2d3pRG5zgylaSI90YZEJAAnZRK0ld3m/J61ttFrFtr/wxIHtkkce0cf+n6frDj2eKxafIGCNau1aS2unNo1/WC/wl9fD7WFDIyR5J4t65/UsJ2nbZTkrTxfSOtrg1Vg91rJCNtLa22phYjFsSvUpmSOSWVEYHWn1NZy8Ri7zraUcUaWNTbat8LcXyOz3WvWQtGI1i43p6PL78vh9ct4udtxxId2HbsZLUESBQTuKH4e7lEKknXtJ2pdd3cPK7cdkzAtCZJ0Sy8ZxLCS4Y+ft9blCMZijIjdXSvJrUxYaujj9SsY8SwJqsxIa7e0HZIkjYH/kfO9BVTPawt4svnWjnumkbrehg1Yd3PbqmMlPCFTdJV8aXIeRjJAF/AjJq8dscrSl0sFqjjeqlMNUWxzhXvgekfvsTZ/XdWKj973TRfDSymnEgoGXmg3hbvlRGlfdP4zsZFxCm7ZXlljhNzfLJ9Qkv3sLT6d7IPiRVJFb9PFNEF/xng7yra2hxPQ8LGf1IOyXHzVEYFGWmro5XscOU5/g0xCsh0MlOjP0zjo3P8u5sncZYkbQBx6oYSB3X3H+qapT1SGT2rbt093rR7ZLShnToieWNEbM/u+JfhOZwe2oysxRpY9wxtzsuXVTXPCEIcRWuLoMb6WIua570NYcJ23rhqmlwrrnpIKpsaQG25vs0DfesBJ7lIKiOodXp4EV9pOyRJm1ObsFUkbGekMuLFFxYjXlFfwgZ25a107buk8hpKODDVGj0qTa6BxbodRvWWhfewaEmFSTh3LVWxTCZskiRpB0YXJiVs/0qlaCoL1NvkbFLC9qA0vraLRd+8jmDxdkXx1D5M8Sy7wCjXXmYR1Ihz7912pnJfSkb5Yqzk+FIXs24FZcImSZJ2mJawxSnRWpKh3ntxUsJGeYXXde26HgeMmPF61utgUg2so3Lctu1sXLrtmIJkalnJTy0hEfGZ1qOkwrT1cfxd9MUL44skSdLmMTRhq97YPU5K2Bg9olAq4s20wevrLrdt8YmAReB9a4SitoDqNCzsnlUEdahJCVtfP3XH7j4lZn3GaQmbJEnaxdwy9VdtZzE2xUKjl6bRaBK7RmPCRhFRpk5jNXnqbsU6a5QFqWvT2PHW1sBCXxLIFCkFRdmVx8/zPrhedWQqO1T7UFYkrodjwTfnYpH47qmUS6AgKiUV2IzAejSmaw9LpeDp+1J5r+zE60vM+pLaRfEeWUy+zHV8G8V61OWr5TMmieU61lt8L3UHtSRJO9U+qazV4pENCK0hNbBASQWKrLIuDIeG56q+IrCTvCi0KYRKiYXvdMeMLLITkKSJUTDW8VVtwsauWG+6vTyMyPYVJGZzSltPby019ShXMw3102oidWxafZ16rXinhXnxu1bdN61vLT9JkpZiSA0sHJDGpxBfkEb136pHNMfTxITtbWk8YcNB3SO7Y0kiqOVG4tnWu+J1s6Y2NQwjuiQwfaip9+Q0qqlHiY211NRbS8JGrTWuRY3Cei3eG9dipHVRMWGDU9+SpPO9ITWwwA5PEqZ4n8o4Jcrtk9ayC7QmbNOmpPqm0dqps1c0x5ofiVIfNpi0NfXiVPGQmnqzEjbOwd93u5mlvRbrEhcVi95W61XLT5KkpTm47ehRpx1jwjYvptcoHLzIrY/AGr52lG8z43ZSTBXXael7h+dot7eZYvowbg4h4aZQbH0dI1jcLaPqW7sIrhmnndvXsSt5llkJGxtlSNi4VsV/JNprLUPf0oC+NaSSJElzOzeVTRo4J8dbuvYz02gTCbcYq2sKY9LDGsB6x4mj0/iI5aQRttY8SdSshG17Wj2i+qE037VmYWq1tR7XkSRJu7ATQptEo24S4J6XW0ZPpSNSqdnXl4z09U2qyxcNqanHTdpbsxK2lbQ6YeM9zrrWEKzTjJtVtoR21ffnIUmSNLfjQptEg920YINGTYy2pnJjdfSVW2lLrWDItOCQmnp955k3YZt1rSEojWPCJkmSzjPsiP1uGq3dI9GoawG5N+x9UinhEhMQErZDc+yfSl27M0N/XAc3K2lhk0lfondyjhvm2LMLSsDEDSmICRtTsu1u35U0nrDx82en1fX7jk+lft9JqfwZMDVMEWQ2LdQNKId1j9TvA/fRnZWwrUctP0mSpJkowdGOWk3TNzI2BFOgh4djErZWTNgoEVPvtlGtpGHvlc0qqD8fS3TU0UZivzSq30edv2kJm7X8JEnShsEU6jw7d5l+5Y4V1SmhXcWEjaSLkbhoJQ1L2LZ1j9RtQ0zYYnkRRubqbdnOSuMjeltCG9bykyRJG8qpaVhdvopNCNwKjVGqE9OoLAelW6KYsMXkrlpJwxK2OsI2CaVK4nmo68attuJu2C2h/fpkLT9JkrTB7JWG1eWrGLmihAiOSaXOWx+mZ6ehJMksJIGUIplnFDCKo3vUkNuVavlJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJkiRJknYt/wfqgSE9vvEo6AAAAABJRU5ErkJggg==>

[image4]: <data:image/png;base64,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>

[image5]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABkAAAAaCAYAAABCfffNAAABQUlEQVR4Xu2VvUoDQRSFD6K1ililNZo2lWVABAux0KCNpegjWM8TKEEQrMRCrS1EDOQFAglY2PiDCqJ2FoFY+XPGew2bS1wzwYUU+8FHYM9kzu7e3QRI6Xdm6C29p3f6eUXnNK/QG3odcUezYE7oJ521AVmBZPt0yGRBvNAGHbQB2YKUFG0QwiRkkzMbKDX6QcdsEMI6pGTTBmSEvtMLG4RyCClZhVxVlk6oa5qVWqt75JE26XnEsupn5UsWW6t7wJ+13+TUBkrcPHJ02R7sxAZ+n8cwOs8jQw8gJ+Bv9Z8cQ0qmbUAWED8Phy5L4t6PbUjJkg0Uhy5K8pBN/KAtA/QSko+b7AeHmJIC5PfoFfJU+St5oPOQzav0SbM3+kz3vr/ZjqNH9uB/4yAzTRSHBEtG6S6tQ/4a/K2cii5ISUmOL+BtTo6JEWgfAAAAAElFTkSuQmCC>

[image6]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABUAAAAbCAYAAACTHcTmAAABKElEQVR4Xu2TvUoDURBGBwUVrAKBNKJpNA9gFWxiL1hYRu0FCwvBIil8Al9ARNNI+uQFxE7IEygREcRKLMQ0wXiG2ZDNhP0JJpU5cFiYb3a49+5dkRnTZgsf8Q3fg2cbn/AZ7/EAF/ovjEMde1gM1ZbwOKhfhOqp0dV94rwP4AW7uOqDOFbEVtPwAczhF/5gfjiKpyw29NQHsC2W3bh6IpdiL266+ho+4CvmXJaIfn3dXhXPsIK3Ymd8jZlBazr659nC3cAdLIl9/TDLeIhZVx9hX2zouat7jrAm1rvushGmMvRKrLHk6lGkGqq/YgcXfRBB4tCCWNOdD2KIHKr/t16jD7FVfovdxb1wUwQ6dMMX/4oO1R1OlIkO1aunN0WPqoknw/GM/8svoHtAYpmCmOQAAAAASUVORK5CYII=>

[image7]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABUAAAAbCAYAAACTHcTmAAABHElEQVR4Xu2UPUtCYRTHj71htklCm4sW5uBQuIt9A1dxcVIEHaIPI4gITeEkgvoN+gwJtgSRCCJiY/l/OA9xPHm919Qpf/Djel78o/eNaM+uuYMvHr2w33HFBwOwB79hGp7AY+iHYViFX7Zeiw84gYd6AILwXTfduCb+lW3VP7XHI/gsB14oEofei14C1u1nE14WM088EYfe2tqc4xbM/2z8gSFx6MA6tXVELq1DnDigAw+Ir3wGvsklyxnMwXM90JSIQx9ELwYfRW0owAbxblTNfrGT0CbxYlL0zN8MiVriGmqephE53/TLcA29IV7q6sEKHENTsA/H8BPO4Ct5Czehl7q5KSb0Sjc3ZauhWVgjPl3mxVNZHO/5v8wBw19AuZ0+FaQAAAAASUVORK5CYII=>

[image8]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABUAAAAbCAYAAACTHcTmAAABTUlEQVR4Xu2TzytEURTHDworJQsbYWctFmTzWFkoyYLC1oqFUhLKwtrCVqKU7P0DZCf+Ar+SkixE+bERPsd5L/edxpuZplmZT32amfO99859950rUqHc9OE53uF9/HmFF3iNxziFtcmEYtjHL+wNavU4E9fXg3rB6O6esMYHcIMf2OqDLFrEdnPgA6jGF/zE9nSUzYTYovM+gH6xbMfV87IpNrHL1dvwBG+x2WV50bevj7eMC7iEe2JnvI2Nv0MLIznPMxyOHcJI7O0n6NkO4gp2BvWcTIotuurqHn0KXTTCS5xOpY6yLLoltmjk6iF1+Ioj8W/9gwfJ3dM/6FV8F5uYxRg2xd9nxS5ETjrEdnnkgwyq8BTnfKD3W9voUWyXb2K9OBoO+oNFXPPFUhgX62OlR2zXJTGAG2I92o276bh4GvBZ7PwTD8MBFf4z3xuwRWgv8YA8AAAAAElFTkSuQmCC>

[image9]: <data:image/png;base64,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>

[image10]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABMAAAAaCAYAAABVX2cEAAABG0lEQVR4Xu2TvUoDQRRGr6YylYRgYylEJKl8gBSJDyAGS1sV6zTpAmmsBCFNqjSBGF9C0MKAIqJgI4jYmE4bsRB/zjiz7M5lNo2VsAcOO3O/Yf7YEcn4KzV8wGec4MCPf7nAR7wXO/bASwMc4St+4ZLKctjGU1z0ozA32MRvCa+8j5u6GGIZj3Ee3/AF894IkTNcULUgO7jn2j2xu9uOY5nDq0R/KkNcce2K2MnMsSPq2E30p3Kt+idiJ6y6fgcbcZxOdF9JNsROFtXNfRXjOJ1die8rwvwKT/iBJbz043RGWNZFaInd3TkeqizIDN65r8Yc613shOsqC7KFtzirA0cfP7GggyRrYt+hWdlonpF5o5pVHOtiRsa/5gcPXDQmGhr6FgAAAABJRU5ErkJggg==>

[image11]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAmwAAABDCAYAAAAh8FnvAAADv0lEQVR4Xu3dW6hlcxwH8L8xGMY1FBN5VTKaeZAo4/LgYR4UKVGaJ148jprwokRKaswbhQeXklxSLkMuT6S8ecDQuDQmpRmXjHjA799ae87//Ox9znb2Puewz+dT3/Za37VO5/Xf/7/Wf5UCAAAAAAAAAAAAAAAAAAAAAAAAAAAAACybiyPb++NTI+c01wAAWGV7Ih9FHovsjnwYuXTeHUuzIXIgcjDyVWR/ZF9kc3MPAAALuDPyV+rqzNqbqZvEE5EtqTsSOTZ1AAAM8WfpBlTZrlxM4IvIutS9ELkydQAAJDsjH+eyd0wuJpBn8Ko6YLs5lwAAzHc4sjWXC6hLpZtGZKHlzUO5KN0gLs+6AQDQOK4Mn/mq7s9F75nIcyNyYXNf67zIy7kso/83AACNB8vcNh4DddbritRN4sky/4WDjZG9kQuaDgCAEc6MfB65pj+vS56vz12e2PrIt2Vu6fPEyKeRN47eAQDAWC6K3JJLAAAAAAAAgKU7o3RvSrapz3sBAPAfUPcsezeyLfJl6Xb8vzxyfntTr37b8/0m75Xub9+J7Dh6FwAAy+auXExJnr37PwcAYNVcGzkrl8kdkXtHZLAdBwAAy+TRXCyT23MBAMB4Fptdm5YDuRjT8blYAfV/np1LAIBZt5QBW30xYppfPRhX/RrCB7kEAJh13+ViDL9Ezs3llO2KvB35NV8o3YfmAQDWjIO5WMQlZeWWa6thA7a6bcnuXAIAzKL7Ij9Fnkr99sgpkZMiV82/VO5J59XV/W99vmzYfnGTGDZgq/vPfZJLAIC14u7SvTla9zhb13ftfmd1dqv1YuTZyM7+/OHI93OXJzZswHZDsQcbALBGDNuE9oHI45EfBzc116r9zfEJpZul29d09fmyUcusm0akvsQwypFchM3FgA0AWOPqYGgwY7Yxsqe59nVzPNAOnurxjc35pH7LRek+12XABgCsCXUm7bbyz0HRoTK3HPpI6WbAtvbnT/e/rVea41dLt1/ahqabxB+Rk1NXZwG/SR0AwEz6vf+t22e06kP9A9sie5vzHZH1zXndG+3W5vyzMp290uoya11+/TlyOHJdc+35yGvNOQDATKvPkP2Qy0W8lYsVVpdlF3ruDQBgprxUum08/o0643V9LlfI6ZGbcgkAMKvq1wS2RC7LF8YwzRcLxnVa5KFcAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACwev4GzuC2lc6lUwwAAAAASUVORK5CYII=>

[image12]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAaCAYAAAC+aNwHAAABA0lEQVR4Xu2SIUtDURiGP8VZFIuiE8OYi2IxOGRtdXmGpVVhYBlikRXBLDaDQZtxRfwBsjFQu0tmg+AEm/ocv3vH5d2FLcseeODe973nOwfuMfv3zOM2VnA9kS9hNvE+wipe4Cf28BLbeI5r2MXd4dfCIX7gDa5Id4SDyDnp/mjgDx5oERGO/Y33WgT2zcsrLYRnPNZwAd/Md9+QTgm7FzVsmi9+1CKFTZzRsGM+oKXFpLybD9jRYhIy5ou/tEjhFHMaBl7NhyxqkaCADxrGnJkPCFc2jVm8xZIWMcvYxxcsSxcuzx3WJR8hb/6Pw0me8ASvzRfvJb4byxZWsRY9T5kyll/XiyxRZFBdwwAAAABJRU5ErkJggg==>

[image13]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABIAAAAZCAYAAAA8CX6UAAAA70lEQVR4Xu2Tuw5BQRCGx61To1BQaRUkaq9BrVLwFKLTq3RKtNSeQOd+iUsiaCg0/JM5ZM9wgk5xvuRr9p/M7pndQ+TyK2k4hBu4hWs4hWM4gwNYgWGr/iNNeIMZYy0Iy3BJ0jhqZI5M4BF6dQCyJJs0dKDhnbiwrQMLH9zBK/SrzEaOpFFJBwZ9kpqYWrdRJylK6sBgT1IT0YEJ39qB3s+H4RvjJmfoUdmTx3xaOjAo0BfDztPn+fTo9Wm8wI/P6bPicAW7MKAyGwmSnTpqPQSL8ASrJNf/Fj7iiGR43OgCFyS/xNyyBlNWvYvL/3MHAG42Bbor924AAAAASUVORK5CYII=>

[image14]: <data:image/png;base64,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>

[image15]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABYAAAAaCAYAAACzdqxAAAABVElEQVR4Xu2UvytGURjHv/kZRjLIIpGZf0DYmCSTSX5sdomBZDAJC0kGGUwmK4sogzCLUFiVFOH7eM7pnOe+99a1mO6nPnXP8z3nvOc+994XKPhvuugNfXKe2LiEfoS5D3TXxqXs0Dv6QSsSmaeOHtJvekDLbJzOBV2GLmpJZJ4lOgOdM5nIUmmkx3QUuqjXxr900lm6Cp3TbuN0hug87YEuGrMxyqG9rKZX9NHG2cgpZFNpgWy8aGNM0W7aQL+Q44F5zmkN9KF90r0oa6Yr7lruTH54PMTZyCmkvx55M06j8Satd9dr0I3bQpyNnGIhGh/RZ3c9SEdChGv8ob/rtC8ab0NP1QR9tz3y5kg9d38vaW00noNuIB9Ca1QfdvWJqJbJAL2nVVFNbl02mI5qwpardyTqBvl/kF690Xf6Cu21z85opRtv0FuEuS9032UFBQV5+QHpY0dlyIXWnAAAAABJRU5ErkJggg==>

[image16]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAaCAYAAACKER0bAAAAZUlEQVR4XmNgGBwgEIg3AfENIF4BxPuB+CwQtyErAoFVUDoLiB2QxOFgJZTOBWJbZAkYgJmQD8Q2yBIwADOhkGEQKyDaFzjDAaYgG4jtkSVAcbEZiB8xQBQdAeKLQNyLrGgUEAAAtggX7I0dslIAAAAASUVORK5CYII=>

[image17]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAKQAAAAdCAYAAAAkcMf8AAADXUlEQVR4Xu2bWahNURjHP1PmDJEhdPNmVoqM3WQo8SDDi4whUyiSqdyQZCxlnsmclHlWhuRBknhAPFGelLyQ8P/3rdVeZzmnznbOvefI96tf1lp7n723vf5r7dXd54gYhmEYhmEYRuFUwKlxo2HUJF3gRvgU/oRXMjcbRs3SF86AfeA3sUAaZYQF0igrLJBGWWGBNMoKC6RRVjCQV+PGHDSG7fO0ufuMYaSCgbwWN+ZgEjyVp6vcZwwjFWkCaRjVDgN5PW40jFJQD36H92HdaJth1Bij4Bv4EX5xfoKvYbNgP8MwDMMwDMMwDMMwDMP4z5geN/yjtIVj4sZSMwDegifgYVc+A4+KvsVZmuxaENvgbXgX7oV74CX4SLJ38Dz4TpLf/PDmbRD9g342BsOT8Bk8C3eJvldfDxsG+xEec1jUli+D4GxXbi16v87Bg/CGK++DF+AWt18aeF03RfvjmOjxj8MD8LIU/zdQu2GHuLGUMAxLYC3Rzv4FJ7ht3UQ7t1gwiPeCOt8QzXHt2WDnznLl7vAJbOLqTUUDGtJS9PqHBG3s4JewZ9C2Ay4O6tlYA1tFbXVEB2ptV+egWSR67xqIvood4ba1ER3YaWH4+rnyUNG3afymFZkI57pysWAYz8eNpWSZ6OxDGBB26Phks2wPyoXC4HGG9PQS7eQjQVsIZzwfyJjJcGfU5gPJWSxkpeiMlYa3ojNgyAI4Lajz/D6ccXg4cKpcOV8awa1Bfa3oE8QzGlYG9WJxSDIHcUlhKDwMRxzI3nCk6MzA0cmbxDAQPgoZ6BVuHw+/E7lJdBbizOYJA8nw7Hflru5fwkfSZjgWXpQkkFXwtOjgGQ4/wOeiA8bPfj6QA13dUwl/wI6iP25jB3BmI53hQrjatfP1Kc/D43BZwf+f5w7sH9TD6+Z9eRjUGS4eOw0tYLugzu8ZcMnhqZAk8MVkOZwfN5YDPpDjonY+yrkm+yw6+3B9Qx6Ljiw+shi0HqKh4NqPjzt2Lt+b87iEgWSQuN56JRrykJmi61iejzPMC0kCyTB9Fe1kzuTrRI/HR6WfpXwguS4O4WfYPkX02JyFuD4jXJJ0cmUOEB6Dx+T+PGd9t428lz9nTc8DyQxPoXCwcwnwt2vdGF53rmvnBBQ/baqd34RRnRpfoqqXAAAAAElFTkSuQmCC>

[image18]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAoAAAAXCAYAAAAyet74AAAAf0lEQVR4XmNgGAW0AHJArAzEKkhYCYh5YAqMgPgiEP/HgReDFEkD8WUgzgRiGyCuAOL1QGwLxNZAbAnE/CCFYUCsDWJAwVQgTkPiYwWMQPwEiE3RJdCBCRD/AWIOdAl00MwAcS9BcAmIF6ELogNuIP4GxCnoEtiADAPEQ8MbAADykxQQnTKdTQAAAABJRU5ErkJggg==>

[image19]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAFkAAAAcCAYAAAAKnhNwAAABLklEQVR4Xu3YvytGURzH8UORUAaLIgM2C2IQJpmVIn8Aym4gNqsySCwGlIVSBpL8Kv8A8h8YjCaT4n1c8XX00LnuovN516ue8+NOz+1273FOKaWUUkop9VvNaEWb0YJau0nlqwvXeClh+3OrylMjbjGNfsxiHwPoQy/qPnZ/z9/lZ7gs4QLn73u8U3T6C1NqDO1mvIopM1YFV4Z79IQLqri68YyqcOGH/B8ziKEI9W9XJtqiy57PMVVjHgsR/BtMst1gK5xUxVWDJ0yEC6rYmlz2jFVKKaWUUsnkP8ftJ3mF+a3+kH+vHschNrCJGSy5fEeakzjGFdaxhhMM200pVYkjLLuvd/AcHlFu5mLaw4oZN7jsTNqfdSfXDnbDSerAQTgZ0QNGgrlR3AVz/6JXSPk4JQpfP0AAAAAASUVORK5CYII=>

[image20]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAI0AAAAdCAYAAACXOO/mAAAC0UlEQVR4Xu2bWciMURjHH1v2JSF7JMoSSVkuLIkb5UJkTV8pKS5cyFKKoiRLpGS5kK0sJVzYty/LJSWlLFEKyZ0iW/j/Pef4zvt8M8xrhqbp+dWvznnmnTMz7/t/z3mbd0bEcRzHcRynNukH62zRcSyD4WZ4F36D57IPO05jRsNFcBT8JB4aJyceGic3HhonNx4aJzceGic3DM15WyxCW9izRDuF5zg1CENzwRaLsAAeK9G14TlODZInNI7zE4bmoi06TjFawM/wJmxuHnOcDNPgE/gKvgu+gY9hx2Q7x3Ecx3Ecx3Ecx3GcwAA40RYDvHcWaZa0y6EVbBralRoz0h1Ot8VqYZLo/aI78ATcCw/A26Jf/DX5taXSDV6DD5LaBtg/6Vu2wHrR1+D4e+AZODbZhoyDS00tD/tFD2TKwFDfCU/DdXB5Zov8MIBbRT/HUbgxtCsNx+xti9VCD/hdsmdpX3gvPGYZCl+Gdmt4Q/SAR9bDLkmf7ILXk34HeApuT2rz4fGkX4j2cJMtgllwpqlx24ewV1J7Dsck/b/hpOjrRQ7C1Um/UjAw3EdVSQzNhNDnNN8OLpPGswEZJA2hKcRT2NXUGJqrpsbgvIUjTP13LIS7Ta2N6Kxo4YG9Ymo8e8u5bcKZ9oNkx1gs+tvrfwFn/Xhcqgobmm2iy01n0bWV9IE7RIM0WxpCMw8egVNE13fOFByLy9CqsA1haOwBJPWiywdvYewTfV5kLlwhOrMsgVNFX/e+6HsZHrYbDy+Fdsoc+FF05hsputRyZiwH7iv+i+MQnCy6HFK7jFeKNaL7vOqIoTkrOh1+kew1CsPwCA4Lff5XKoaGs8Ut0X84EO5AjsWQtQw1wtBcTvqRw/BZaM+AL0J7iOhZRrhUMiQ8u3n9wHClF6F8P3b2IQwIrzm+ir6nUn9w9idWwveiY76W8u/ZcRnl/ioEZ8tCn+2/8ANwiIF65t4wbAAAAABJRU5ErkJggg==>
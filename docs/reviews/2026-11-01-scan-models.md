# Scanned and handwritten pages: which model could close the gap (2026-11-01)

Smart mode trails MinerU 4.1.2 by 0.7 overall points on the olmOCR-bench dev split (77.21 vs 77.93), and
the two scanned categories hold nearly all of it: old_scans 28.8 (MinerU 38.8, marker 42.5) and
old_scans_math 65.0 (76.2, 82.5). This is the phase-1 study the brief asked for: what the gap is made of,
which permissively licensed models could close it on the CPU, what each costs, and a cheap signal for when to
use one. Everything here was measured on the 19 dev pages of the two categories (13 old_scans, 6
old_scans_math); nothing was tuned on the test split. One fix came from it: its first run showed the reader
writing out running heads, so phase 2 drops them (it has no threshold to tune, and changes nothing on dev).

## 1. What the gap is made of

Of the dev tests that MinerU or marker pass and filegoblin smart fails:

| page | kind | tests lost | why |
|---|---|---:|---|
| old_scans/63 | printed bulletin | 4 | two columns of the masthead read interleaved ("Entered as second class Jersey, matter under January the Act 8, 1915…") |
| old_scans/74 | printed letterhead + handwriting | 4 | the printed Red Cross letterhead is dropped entirely; the handwriting is misread ("Chif - Justier waili, of lwashinglin") |
| old_scans/3, 42, 96 | handwriting | 5 | PP-OCRv6 misreads cursive |
| old_scans_math/1_pg72 | printed math | 7 | the top block of equations is laid out as text and OCR'd ("S (^{2} + ax{2}) n 22 (2n - 2)…") |
| old_scans_math/1_pg113, 3_pg199, 3_pg94, 3_pg148 | printed math | 11 | formula recognition on degraded scans |

So the brief's premise is half right. Handwriting accounts for roughly 6 of the 13 old_scans tests; the
rest are printed scans (reading order, a dropped region), and every old_scans_math loss is a formula on a
printed scan. Six handwritten pages (20, 50, 53, 72, 83, 92) are failed by **all** three tools: their tests
ask for exact strings of 19th-century cursive.

Because pass/fail on 19 pages is coarse (one test is 1.25 points of old_scans), the study also reports a
soft score: for every `present`/`order` text on a page, the best fuzzy alignment of that text inside the
output (rapidfuzz `partial_ratio`, case and spaces normalized), averaged per page group.

| soft score | filegoblin smart | MinerU | marker |
|---|---:|---:|---:|
| 8 handwritten pages | 66.9 | 79.3 | 84.9 |
| 4 printed pages | 94.8 | 97.6 | 96.8 |
| page 74 (mixed) | 66.0 | 96.6 | 97.5 |

## 2. Candidates

All licences checked on the model cards (Hugging Face API, 2026-10-31). Excluded on licence: Chandra
(OpenRAIL), HunyuanOCR (Tencent community licence), MinerU2.5 VLM (AGPL), olmOCR-2 7B and dots.ocr (too big
for a CPU path, 3–8 B). Timings are on this M4 Pro with 8–10 threads **while two other sessions were
benchmarking on the same machine**, so read them as upper bounds with ±50% noise. PyTorch fp32 numbers are
not rten numbers; the onnxruntime 4-bit run is the closest proxy for what a quantized rten build would do.

| model | licence | params / weights | peak memory | seconds per page | old_scans (dev) | old_scans_math (dev) | soft, handwritten |
|---|---|---|---|---|---:|---:|---:|
| PP-OCRv6 medium (today) | Apache-2.0 | 139 MB det+rec | (whole pipeline) | 5–40 s per PDF, whole pipeline | 28.8 | 65.0 | 66.9 |
| TrOCR-base-handwritten, on PP-OCR's line crops | MIT | 334 M; 1.3 GB fp32, ~0.35 GB int8 | 3.2 GB (PyTorch) | +7–30 s (0.25–0.55 s per line) | 30.0 (+1, on 74) | not run | **61.0 (worse)** |
| PaddleOCR-VL-1.5, whole page, `OCR:` | Apache-2.0 | 0.96 B; 3.6 GB fp32 ONNX, ~0.8 GB q8 | 5.2 GB (PyTorch) | 35–350 s (PyTorch), 0.8 s per token | 5 of 13 pages run | not run | 84.7 on 5 pages (LightOnOCR 95.3 on the same 5) |
| granite-docling-258M | Apache-2.0 | 258 M | — | >15 min on one page | stopped | — | loops ("1 1 1 1 …") on handwriting |
| **LightOnOCR-2-1B, whole page** | Apache-2.0 (Pixtral encoder + Qwen3-0.6B, both Apache-2.0) | 1.0 B; 4.0 GB fp32, 0.75 GB 4-bit, 1.35 GB int8 | 5.9 GB PyTorch fp32; **2.85 GB onnxruntime 4-bit** | 34–456 s PyTorch fp32; **58–101 s 4-bit at 1540 px; 23–76 s 4-bit at 1024 px** | **42.5** | **86.2** | **89.9** |

Not measured: Florence-2 (MIT) and TrOCR small/large. Florence-2's OCR task targets printed text, and TrOCR-base
being *worse* than PP-OCRv6 on these pages (IAM is modern single-writer handwriting; on period cursive the
decoder's language model writes fluent English that isn't on the page: "Soccer friends", "Belgium") gives no
reason to expect its siblings to flip. PaddleOCR publishes no separate permissive handwriting recognizer;
PP-OCRv6 medium already is its handwriting-improved line model.

### LightOnOCR-2-1B in detail

- Whole page, fp32: old_scans 42.5 (= marker), old_scans_math 86.2 (above marker's 82.5). It gains on every
  handwritten page that anyone can pass (3: +2, 42: +1, 53: +2, 96: +3), takes the 74 letterhead (+3) and
  63's masthead (+3), and reads scanned math as LaTeX (1_pg72: 1 → 9 of 12; 1_pg113: 8 → 12).
- It loses on long printed pages (68: −2, 72: −1), where it ran into the 1,500-token cap and took 6–8
  minutes. It is not a replacement for the pipeline on printed scans.
- Time goes to the vision encoder: at 1540 px a page is ~9,000 patches through 24 full-attention layers
  (28–48 s of the 58–101 s in onnxruntime 4-bit); decoding is ~0.1 s per token; a handwritten letter is
  150–250 tokens. At 1024 px the encoder takes 9–14 s.
- 4-bit costs some accuracy against fp32 on the 5 pages compared (−4 tests: 42, 53, 96); 1024 px costs about
  as much again. int8 (1.35 GB) is the likely middle ground; not yet measured.
- rten: the onnx-community exports use onnxruntime contrib ops (`GroupQueryAttention`, `MultiHeadAttention`,
  `SkipSimplifiedLayerNormalization`, `GatherBlockQuantized`) that rten 0.24 does not have. rten does have
  `MatMulNBits`, `MatMulInteger` and fusions for RMSNorm, GQA matmul and SiLU, so a plain `torch.onnx.export`
  of the encoder and of a KV-cached decoder step (as `scripts/vision_models.py` already does for the MFR
  decoder) should load. That export is the first piece of phase 2 and its speed in rten is the open question.

## 3. When to use it: OCR confidence

PP-OCR already scores every line it reads (mean softmax of the emitted CTC symbols). The page signal is the
**character-weighted mean line score**, which costs nothing: OCR has run by the time it is known.

| pages (dev) | weighted mean line score |
|---|---|
| 8 handwritten old_scans pages | 0.61–0.85 |
| old_scans/74 (handwriting over a printed letterhead) | 0.912 |
| old_scans_math/1_pg72 (math OCR'd as text) | 0.776 |
| other 5 old_scans_math pages | 0.943–0.966 |
| 4 printed old_scans pages | 0.974–0.994 |
| 21 other dev pages that ran OCR (scans and pictures of text in the other five categories) | 0.966–0.999 |

A threshold of **0.92** separates every handwritten page from every printed one on dev, and also catches
1_pg72, where a VLM is the right tool anyway. Routing only those 10 pages to LightOnOCR (fp32) and keeping
the pipeline everywhere else:

| dev | filegoblin smart | routed (≤ 0.92 → LightOnOCR) | MinerU | marker |
|---|---:|---:|---:|---:|
| old_scans | 28.8 | **42.5** | 38.8 | 42.5 |
| old_scans_math | 65.0 | **75.0** | 76.2 | 82.5 |
| soft, handwritten | 66.9 | **89.9** | 79.3 | 84.9 |
| tests lost | — | 0 | | |

That is about +3.0 overall dev points (77.21 → ~80.2, against MinerU's 77.93), with printed pages untouched
and as fast as today. The threshold was read off these same 19 pages, so the test split has to confirm it.
Gauntlet scans were not checked yet.

## 4. Verdict

- **LightOnOCR-2-1B clearly pays off in quality** and is the only candidate that does. TrOCR is cheap but does
  not help on period handwriting; PaddleOCR-VL is good but behind LightOnOCR on every page and slower; granite-
  docling fails.
- The cost is real: 0.75–1.35 GB more weights (today's whole model set is ~0.5 GB), 2–3 GB peak, and an
  estimated half a minute to two minutes per routed page on the CPU. Printed pages pay nothing.
- Phase 2 therefore starts with the step that decides the cost: export the Pixtral encoder and a KV-cached
  Qwen3 decoder step to standard-op ONNX, quantize the matmuls, and time them in rten. Section 5 records
  the result.

## 5. Phase 2: the page reader

### Export and speed in rten

- **Graphs.** The Pixtral encoder and projector were rebuilt in plain torch with the patch grid read from the
  input's shape, so one graph takes any page size; it matches transformers to 3e-5 at three sizes. The Qwen3
  decoder was rebuilt as one step with a key/value cache per layer (matches to 2e-5 in logits). Both were
  folded with onnxruntime's basic optimizer and quantized to symmetric 4-bit `MatMulNBits` with int8
  compute; embeddings are int8 rows. Files: vision 258 MB, decoder 373 MB, embeddings 156 MB, vocabulary
  3 MB: **790 MB**.
- **Two rten quirks.** A node whose name equals another node's output name breaks planning ("Missing
  input"), so every node is renamed. rten has no i64 tensors: `start` is fed as i32.
- **Encoder.** 10 s in rten at 1008×784 (onnxruntime 4-bit: 9–14 s). At 1540 px, attention over ~9,000
  blocks needs about 5 GB, so the encoder input is capped at 1008 px on the long side.
- **Decoder.** The first export took 461 ms per token at a 1,150-token context: every step sliced and
  re-stacked a 28-layer cache and copied it again for grouped-query attention (`repeat_interleave`), plus a
  transpose of the keys. One input and output per layer, keys stored transposed, and grouped-query
  attention by broadcasting brought that to 165–230 ms (machine shared with two benchmarking sessions). What
  is left is mostly the 4-bit matmuls (59%) and appending to the cache (13%). Computing `MatMulNBits` in
  float was slower (226 ms); int8 weights (`MatMulInteger`) were no faster and 60% bigger.
- **Accuracy of the shipped form** (4-bit, 1008 px, our graphs through onnxruntime) against fp32 at
  1540 px on 7 routed pages: the soft score is the same (handwriting 86–100 per page), but 5 exact-string
  tests are lost (42, 74, 96). Running the encoder in fp32 did not bring them back, so it is the
  resolution, not the quantization.

### Wiring

- `vision::reader` (new): loads lazily, only when the run may hold 4 GB or more (`memory::budget_mb`); a
  handwritten page peaked at 3.1 GB with the whole pipeline loaded. It decodes greedily, stops at 1,536
  tokens or on a repeating tail, and returns nothing in either case, so the page keeps PP-OCR's reading.
  `FILEGOBLIN_READER=0` switches it off. Before each page it checks that the process's footprint plus 1,280 MB
  is within the budget (the reader adds 0.7–1.1 GB to the pipeline's peak); without that check, a 4 GB PDF
  worker went over on R2's six pages and was killed with the whole document.
- `vision::ocr::read_with_confidence`: the character-weighted mean line score, counting lines too unsure to
  keep. In Rust it gives the same values as the study's Python (0.844, 0.804, 0.612, …).
- In `convert_scanned_page_gated`, after layout and OCR, a page goes to the reader when OCR confidence is at
  most 0.92 (`ROUTE_BELOW`), or when the layout model found at least 3 formulas (`ROUTE_FORMULAS`) and OCR
  confidence is below 0.975 (`ROUTE_FORMULAS_BELOW`; the dev math scans are 0.94–0.97, and the gauntlet showed
  why: its R2, a clean scan of a printed math book at 0.98–0.99, lost 93.2 → 88.4 in quality and took 360 s
  instead of 41 when all six pages went to the reader). The
  second rule came from the dev run: on scans of printed math the reader turns OCR and formula fragments
  back into derivations (old_scans_math 75.0 → 83.8). On the dev split's scanned pages the layout model
  found 6–22 formulas on the math pages and 0 or 1 on every other.
- Clean-up of the reader's Markdown: it writes out the whole page, so among its first and last 8 lines a line
  goes when OCR read every word of it inside a furniture region, and among the first and last 3, a bare page
  number. Picture placeholders (`![image](image_1.png)`, linking to files it does not produce) go too. The
  test split's first run needed the margin filter (headers_footers 91.6 → 89.7, now 91.3); on dev it changes
  no test.
- Progress: a PDF worker is stopped after 120 s without news, and a routed page takes longer than that.
  The reader now reports progress after encoding and every 32 tokens. So does a conversion waiting for a
  machine-wide vision slot (75355f5), because a routed page can hold a slot that long. Without that, an
  unrelated printed page (old_scans/69) was killed while it waited.
- Models: `lighton-vision.onnx`, `lighton-embed.onnx`, `lighton-step.onnx` and `lighton-vocab.json` are built
  by a new `reader` step in `scripts/vision_models.py`. They go into the model archive (`--install-models`)
  and the `bundled-models` binary. They are left out of the PyPI and npm model packages, which would need new
  package names. `--models-status` lists them as an optional set.

### Results

Dev split, the 19 scan pages (the other 161 dev pages cannot change: none crosses either threshold):

| dev | before | after | MinerU | marker |
|---|---:|---:|---:|---:|
| old_scans | 28.8 | **37.5** | 38.8 | 42.5 |
| old_scans_math | 65.0 | **83.8** | 76.2 | 82.5 |
| soft, handwritten | 66.9 | **88.1** | 79.3 | 84.9 |
| **overall (all 180 dev PDFs)** | **77.21** | **80.65** | 77.93 | 70.80 |

Tests gained on dev: 3 (+2), 42 (+1), 53 (+2), 74 (+1), 96 (+1), 1_pg72 (+8), 1_pg113 (+3), 3_pg148 (+1),
3_pg199 (+3), 3_pg94 (+2). Lost: 4_pg512 (−2; its 21 display formulas were already all right).

Time, two conversions at a time on the shared machine: printed scan pages are as before (8–41 s per PDF);
routed handwritten pages take 29–84 s (74, the longest letter, 134 s), routed math pages 92–159 s
(before: 5–40 s).

Test split with the committed build and the rebuilt models (`scan-test-final5`, 1,223 PDFs):

| test | before | after |
|---|---:|---:|
| old_scans | 28.9 | **39.9** |
| old_scans_math | 47.6 | **67.2** |
| headers_footers | 91.6 | 91.2 |
| **overall** | **74.31** | **78.17** |

143 tests gained, 16 lost: 12 formulas on routed math pages, 3 for the letterhead of a Bengali notice (PP-OCR
read it as noise, which passed the "header absent" tests; the reader transcribes it), and 1 pencilled note on
a letter. Routing every formula page regardless of confidence scored old_scans_math 72.8 (overall 78.89), but
it made the gauntlet's R2 worse; the cap costs those 5.6 points. Gauntlet: 29/29, no case moved.

## Reproducing

Scratch harness (not in the repo): a temporary hook in `vision/ocr.rs` dumped each line crop with its score
(`FILEGOBLIN_OCR_DUMP`) and replaced readings from files (`FILEGOBLIN_OCR_OVERRIDE`), so line recognizers were
measured inside the real pipeline; whole-page models wrote the bench layout directly. Saved runs in
`$OLMOCR_BENCH`: `scan-base` (byte-identical to `cs-final` on these pages), `scan-trocr-base`, `scan-lighton`
(fp32), `scan-lighton-q4`, `scan-lighton-q4-1024`, `scan-paddlevl` (5 pages), `scan-route-lighton`,
`scan-ours-1008` (our 4-bit graphs through onnxruntime), `scan-final` (the committed build, 19 scan pages),
`scan-dev-final` (all dev pages), `scan-dev-final3` and `scan-dev-final5` (all dev pages, with the margin filter; 5 with the rebuilt models),
`scan-test-final` (test split, before the margin filter), `scan-test-final3` (with it, every formula page routed),
`scan-test-final5` (the committed build, rebuilt models). Gauntlet: `target/gauntlet/scan-final-smart`.

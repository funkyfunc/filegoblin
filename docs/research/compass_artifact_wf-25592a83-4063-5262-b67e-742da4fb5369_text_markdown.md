# DIAGRAM_MATH_RESEARCH.md — Deterministic Diagram, Chart & Math Extraction for `filegoblin`

**Bottom line:** Office-native diagrams, charts and equations (SmartArt, glued connectors, `c:chartSpace`/`cx:chartSpace`, OMML) can be turned into Mermaid, GFM tables and LaTeX deterministically on CPU in pure Rust, and as of October 2026 no mainstream tool does this well. Born-digital PDF vector diagrams and PDF math can be solved only in part with geometry and font tables. Raster figures and broken math text layers need an optional, clearly flagged ONNX fallback.

## TL;DR

- **Solved deterministically, so build it first:** SmartArt `dgm:dataModel` → Mermaid, `p:cxnSp` `stCxn`/`endCxn` → flowchart edges, chart XML caches → GFM tables or Mermaid `pie`/`xychart`, and OMML → LaTeX. All four are XML tree walks over the ZIP package and need only `zip` + `quick-xml`/`roxmltree`. Docling (Python) ignores PPTX charts, python-pptx (and so Docling and MarkItDown) drops PowerPoint OMML, and MarkItDown's OMML converter has open bugs that silently drop content.
- **Partly solvable on CPU with geometry:** vector PDF flowcharts. Interpret content streams with a pure-Rust interpreter (`hayro-interpret`, Apache-2.0 OR MIT, v0.8.0), then cluster rectangles, segments, arrowhead triangles and text spans with an R-tree (`rstar`). Output SVG always and Mermaid only when a confidence gate passes. PDF math text recovery (CMSY/CMMI/CMEX without ToUnicode) is fixable with published TeX encoding tables. Rebuilding 2-D layout (fractions, scripts) from glyph geometry is research-grade, so emit it only behind a confidence gate.
- **Needs a hybrid fallback:** raster charts and diagrams, scanned math, and math whose glyph layer is unrecoverable. Use small ONNX math-OCR models (LaTeX-OCR/pix2tex ports, UniMERNet-tiny ONNX, Pix2Text MFR) behind an `--ml` feature flag, and never by default. The 10x position for `filegoblin` is "every structured object a document already contains, extracted losslessly and instantly, with no models."

---

## 1. Executive Synthesis — Verdicts

| Capability | Verdict | Determinism | Notes |
|---|---|---|---|
| SmartArt → Mermaid (text + topology) | **Solved** | 100% topology; direction heuristic from layout family | Data model is a typed graph (`ptLst` + `cxnLst`) |
| SmartArt via `dsp:` drawing cache | **Solved** | 100% | Pre-rendered shapes with text; fallback |
| Glued connectors → flowchart | **Solved** | 100% when `stCxn`/`endCxn` present | Direction from `headEnd`/`tailEnd` |
| Unglued connectors → flowchart | **Heuristic** | Geometric snapping | Needs group-transform math; confidence score |
| Classic charts (`c:`) → table/Mermaid | **Solved** | 100% from caches | Embedded workbook fallback |
| Chartex (`cx:`) → table | **Solved (harder)** | 100% from `cx:chartData` | [MS-ODRAWXML] §5.22, not ISO 29500 |
| OMML → LaTeX | **Solved** | 100% with curated symbol table | n-ary, `sepChr`, `borderBox`, accents, `eqArr` |
| Vector PDF figure → SVG | **Solved** | 100% (re-emit paths) | Pure-Rust interpreter exists |
| Vector PDF figure → Mermaid | **Heuristic** | Rule-based geometry | Box-and-arrow only |
| PDF TeX math glyph → Unicode | **Mostly solved** | Table-driven | OMS/OML/OMX + extended AGL |
| PDF math 2-D layout → LaTeX | **Research-grade** | Heuristic | Confidence-gated |
| Raster diagrams/charts/scanned math | **Hybrid only** | ML | Optional ONNX |

**Strategic conclusion:** The competitors are converging on VLMs: Granite-Docling-258M, Marker's Surya VLM "served by a local inference server", and Datalab's Chandra.\[1\]\[2\] That leaves a clear opening for a tool that recovers the semantic objects already present in the file, with no inference cost, no GPU and byte-for-byte reproducible output.

---

## 2. OpenXML Diagram Specification

### 2.1 SmartArt package anatomy

A SmartArt graphic is a `p:graphicFrame` (PPTX) or `w:drawing` (DOCX) whose `a:graphicData` has `uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"` and contains `dgm:relIds`. The relIds point to four parts. These are the data model (`dgm:dataModel`), the layout definition (`dgm:layoutDef`), the quick-style definition (`dgm:styleDef`) and the color transform (`dgm:colorsDef`).\[3\] Office 2010+ writers usually add a fifth part, the drawing cache (`dsp:drawing`, [MS-ODRAWXML]).

```xml
<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram">
  <dgm:relIds r:dm="rId2" r:lo="rId3" r:qs="rId4" r:cs="rId5"/>
</a:graphicData>
```

| Part | Typical path | Relationship type (suffix) | Use |
|---|---|---|---|
| Data model | `ppt/diagrams/data1.xml` | `…/relationships/diagramData` | **Primary**: nodes, text, topology |
| Layout def | `ppt/diagrams/layout1.xml` | `…/diagramLayout` | `uniqueId` → family/direction |
| Quick style | `ppt/diagrams/quickStyle1.xml` | `…/diagramQuickStyle` | Ignore |
| Colors | `ppt/diagrams/colors1.xml` | `…/diagramColors` | Ignore |
| Drawing cache | `ppt/diagrams/drawing1.xml` | `http://schemas.microsoft.com/office/2007/relationships/diagramDrawing` | **Fallback**: laid-out `dsp:sp` shapes |

The data model points to the drawing part through `dsp:dataModelExt relId="…"` inside `dgm:extLst`. Resolve it through the *slide's* relationships.

### 2.2 `dgm:dataModel` hierarchy

The children of `dgm:dataModel` are `bg`, `cxnLst`, `extLst`, `ptLst` and `whole`. A `dgm:cxn` has the attributes `destId`, `destOrd`, `modelId`, `parTransId`, `presId`, `sibTransId`, `srcId`, `srcOrd` and `type`.\[4\] The connection type (`ST_CxnType`) is one of `parOf`, `presOf`, `presParOf` or `unknownRelationship`.\[3\]\[5\]

```xml
<dgm:dataModel>
  <dgm:ptLst>
    <dgm:pt modelId="{D0}" type="doc"/>
    <dgm:pt modelId="{A1}"><dgm:prSet phldrT="[Text]"/>
      <dgm:t><a:bodyPr/><a:p><a:r><a:t>Plan</a:t></a:r></a:p></dgm:t></dgm:pt>
    <dgm:pt modelId="{T1}" type="parTrans" cxnId="{C1}"/>
    <dgm:pt modelId="{S1}" type="sibTrans" cxnId="{C1}"/>
    <dgm:pt modelId="{P1}" type="pres"><dgm:prSet presAssocID="{A1}" presName="node"/></dgm:pt>
  </dgm:ptLst>
  <dgm:cxnLst>
    <dgm:cxn modelId="{C1}" srcId="{D0}" destId="{A1}" srcOrd="0" destOrd="0" parTransId="{T1}" sibTransId="{S1}"/>
    <dgm:cxn modelId="{C9}" type="presOf" srcId="{A1}" destId="{P1}" srcOrd="0" destOrd="0"/>
  </dgm:cxnLst>
</dgm:dataModel>
```

| Concept | Meaning | Extraction rule |
|---|---|---|
| `pt@type="doc"` | Single root of the semantic tree | Tree root; not rendered |
| `pt@type="node"` (default) | User content node | **Keep**; text from `dgm:t//a:t` |
| `pt@type="asst"` | Assistant node (org charts) | Keep; side node |
| `parTrans` / `sibTrans` | Transition points bound to a `cxn` via `cxnId` | Text → edge label |
| `pt@type="pres"` | Layout-engine presentation points | **Drop** |
| `cxn@type="parOf"` (default) | Semantic parent → child edge | **Keep**; order by `srcOrd` |
| `presOf` / `presParOf` | Data→presentation binding / presentation tree | Drop (or map to `dsp` shapes) |
| `modelId` | Identity (GUID or int) | Hash-map key |

Microsoft's custom-layout guide notes that the `presOf` tag "refers to child text within the node".\[6\] That is why `presOf` is a binding and not a topology edge.

### 2.3 Layout definition → Mermaid diagram family

Read `dgm:layoutDef@uniqueId`. If it is missing, use the doc point's `dgm:prSet@loTypeId`. The URIs follow `urn:microsoft.com/office/officeart/2005/8/layout/<name>`. Newer builds use 2008/2009 namespaces, so match on the final segment.

| Layout name prefix | Category | Mermaid target | Edge semantics |
|---|---|---|---|
| `hierarchy*`, `orgChart1`, `horizontalOrgChart` | Hierarchy | `flowchart TD` (horizontal → `LR`) | `parOf` = parent → child |
| `process*`, `chevron*`, `hProcess*`, `arrow*` | Process | `flowchart LR` (`vProcess*` → TD) | Sibling sequence by `srcOrd` |
| `cycle*` | Cycle | `flowchart LR` | Sequence + last → first |
| `radial*` | Hub | `flowchart TD` | Hub → spokes |
| `venn*` | Relationship | Mermaid `venn` or list | Sets |
| `pyramid*`, `funnel1` | Pyramid | Ordered list / TD chain | Sequence |
| `matrix*` | Matrix | GFM 2×2 table | Grid |
| `list*`, `default`, `vList*` | List | Nested Markdown list | Tree |
| `lProcess*` | Process + sub-bullets | `flowchart` + `subgraph` | Level-2 → subgraph |

**Rule of thumb:** `parOf` always gives the *tree*. The layout family decides whether siblings at depth 1 *also* get sequence edges (process/cycle) or not. Treat this table as a curated, versioned lookup. Unknown layouts default to a nested list, which loses nothing.

### 2.4 SmartArt → Mermaid blueprint (Rust)

```rust
const DGM: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
const A: &str   = "http://schemas.openxmlformats.org/drawingml/2006/main";

pub fn smartart_to_mermaid(data_xml: &str, layout_uid: &str) -> String {
    let doc = roxmltree::Document::parse(data_xml).unwrap();
    let mut text = HashMap::new(); let mut root = String::new();
    let mut kids: HashMap<String, Vec<(u32, String, Option<String>)>> = HashMap::new();
    for n in doc.descendants() {
        if n.has_tag_name((DGM, "pt")) {
            let id = n.attribute("modelId").unwrap_or_default().to_string();
            if n.attribute("type") == Some("doc") { root = id.clone(); }
            let t: Vec<_> = n.descendants().filter(|d| d.has_tag_name((A, "t"))).filter_map(|d| d.text()).collect();
            text.insert(id, t.join(" "));
        } else if n.has_tag_name((DGM, "cxn")) && n.attribute("type").unwrap_or("parOf") == "parOf" {
            kids.entry(n.attribute("srcId").unwrap().into()).or_default().push((
                n.attribute("srcOrd").and_then(|s| s.parse().ok()).unwrap_or(0),
                n.attribute("destId").unwrap().into(), n.attribute("parTransId").map(Into::into)));
        }
    }
    kids.values_mut().for_each(|v| v.sort_by_key(|k| k.0));
    let (seq, dir) = family(layout_uid);            // e.g. ("process", "LR")
    let mut out = format!("flowchart {dir}\n");
    let mut stack = vec![root.clone()];
    while let Some(p) = stack.pop() {
        for (_, c, tr) in kids.get(&p).into_iter().flatten() {
            out += &format!("  {}[\"{}\"]\n", mid(c), esc(&text[c]));
            if p != root && seq == "tree" {
                let lbl = tr.as_ref().and_then(|t| text.get(t)).filter(|s| !s.is_empty());
                out += &match lbl { Some(l) => format!("  {} -->|\"{}\"| {}\n", mid(&p), esc(l), mid(c)),
                                    None    => format!("  {} --> {}\n", mid(&p), mid(c)) };
            }
            stack.push(c.clone());
        }
    }
    if seq != "tree" {                                // process / cycle: chain top-level nodes
        let top: Vec<_> = kids.get(&root).into_iter().flatten().map(|k| mid(&k.1)).collect();
        for w in top.windows(2) { out += &format!("  {} --> {}\n", w[0], w[1]); }
        if seq == "cycle" && top.len() > 2 { out += &format!("  {} --> {}\n", top[top.len()-1], top[0]); }
    }
    out
}
fn esc(s: &str) -> String { s.replace('"', "#quot;").replace('\n', "<br/>") }
```

`mid()` maps GUIDs to stable `n0, n1…` ids, and `family()` implements the §2.3 table. **Fallback:** if `parOf` yields an empty tree, as some third-party generators produce, use the `dsp:drawing` part. It holds a flat list of `dsp:sp` shapes with `dsp:txBody` and `a:xfrm`. Feed those into the same geometric connector-snapping pipeline as §2.6.

### 2.5 DrawingML connectors (`p:cxnSp`)

```xml
<p:sp><p:nvSpPr><p:cNvPr id="4" name="Rounded Rectangle 3"/>…</p:nvSpPr>
  <p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="1828800" cy="685800"/></a:xfrm>
    <a:prstGeom prst="roundRect"/></p:spPr>
  <p:txBody><a:bodyPr/><a:p><a:r><a:t>Start</a:t></a:r></a:p></p:txBody></p:sp>
<p:cxnSp>
  <p:nvCxnSpPr><p:cNvPr id="8" name="Straight Arrow Connector 7"/>
    <p:cNvCxnSpPr><a:stCxn id="4" idx="2"/><a:endCxn id="5" idx="0"/></p:cNvCxnSpPr><p:nvPr/></p:nvCxnSpPr>
  <p:spPr><a:xfrm flipV="1">…</a:xfrm><a:prstGeom prst="straightConnector1"/>
    <a:ln w="12700"><a:tailEnd type="triangle"/></a:ln></p:spPr>
</p:cxnSp>
```

- **IDs:** `stCxn@id`/`endCxn@id` refer to the `id` attribute of the target shape's `p:cNvPr` inside `p:nvSpPr`. `idx` is the index of the connection site on that shape.\[7\] IDs are unique per slide, so key the map by `(slide, id)`.
- **Routing is not stored semantically.** The spec says that once a connection is specified, "it is left to the generating application to determine the exact path the connector takes".\[8\] The `a:xfrm` box and the preset (`straightConnector1`, `bentConnector2–5`, `curvedConnector2–5`) only record the last-rendered geometry. For topology, trust `stCxn`/`endCxn` over geometry.
- **Connection sites:** each preset in `presetShapeDefinitions.xml` (ECMA-376 Part 1 annex) has an `a:cxnLst` of `a:cxn` elements with `ang` and `a:pos x/y` guide formulas. The `ang` attribute "specifies the incoming connector angle".\[9\] For rect, roundRect and ellipse the four cardinal sites are numbered 0 = top, 1 = left, 2 = bottom, 3 = right, counter-clockwise from the top. The Go library pptxgo (github.com/mmonterroca/pptxgo/pptx) documents this numbering and says its `drawingml.StCxn` doc comment explains "how this was confirmed against a real render rather than assumed from the schema". Other presets number their sites differently. Topology needs only `id`. `idx` matters only for SVG endpoint placement.
- **Direction:** `a:ln/a:tailEnd` is drawn at the path's end point and `a:headEnd` at its start point (after `flipH`/`flipV`). Tail arrow only → `A --> B`; head only → `B --> A`; both → `A <--> B`; neither → `A --- B`.
- **Groups (`p:grpSp`):** child coordinates live in child space. Map them with `x' = off.x + (x − chOff.x) · ext.cx / chExt.cx` (and the same for y), composing recursively and applying `rot` (60000ths of a degree) and flips around the group centre. Shapes inside groups can be glue targets.
- **Word equivalents:** DOCX uses `wps:wsp` (in `wpg:wgp` groups or `wpc:wpc` canvases), and connectors carry `wps:cNvCnPr` with `a:stCxn`/`a:endCxn`. Legacy VML uses `o:rules/o:r[@type="connector"]/o:proxy@idref`. Word glues mainly inside canvases, so expect most DOCX flowcharts to need geometric snapping.
- **Labels:** text comes from `p:txBody//a:t` (PPTX), or from `wps:txbx/w:txbxContent//w:t` (DOCX).

### 2.6 Flowchart reconstruction algorithm

```text
walk(spTree, T=identity):
  p:grpSp  -> walk(child, T ∘ group_xfrm)
  p:sp     -> shapes[id] = {bbox: T(xfrm), prst, text}
  p:cxnSp  -> conns.push({st: stCxn?, end: endCxn?, p0,p1 = endpoints(T(xfrm), flips), head, tail})
edges: a = st.id ?? snap(p0); b = end.id ?? snap(p1)
snap(p): rstar nearest-k=3 shape bboxes; accept if dist(p, boundary) < τ = max(0.05·min(w,h), 2pt)
         else junction node (merge chained bent connectors later)
orient by head/tail; prst → Mermaid shape (rect→[ ], roundRect→( ), ellipse→(( )),
  flowChartDecision→{ }, flowChartTerminator→([ ]), parallelogram→[/ /])
direction: mean |dy| > |dx| → TD else LR
confidence = glued_ends / total_ends; < 0.8 → emit with %% warning
```

Free-floating unfilled text boxes near a connector midpoint become edge labels.

### 2.7 Chart XML (`c:chartSpace`)

A frame with `a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"` holds `<c:chart r:id="rIdN"/>`, which points to `ppt/charts/chartN.xml` or `word/charts/chartN.xml`. The chart's `.rels` usually links an embedded workbook (`…/relationships/package`).

```xml
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
 <c:chart><c:title>…Revenue…</c:title><c:plotArea>
  <c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>
   <c:ser><c:idx val="0"/><c:order val="0"/>
    <c:tx><c:strRef><c:f>Sheet1!$B$1</c:f><c:strCache><c:pt idx="0"><c:v>2025</c:v></c:pt></c:strCache></c:strRef></c:tx>
    <c:cat><c:strRef><c:f>Sheet1!$A$2:$A$4</c:f><c:strCache><c:ptCount val="3"/>
      <c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt><c:pt idx="2"><c:v>Q3</c:v></c:pt></c:strCache></c:strRef></c:cat>
    <c:val><c:numRef><c:f>Sheet1!$B$2:$B$4</c:f><c:numCache><c:ptCount val="3"/>
      <c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="2"><c:v>30</c:v></c:pt></c:numCache></c:numRef></c:val>
   </c:ser><c:axId val="111"/><c:axId val="222"/></c:barChart>
  <c:catAx><c:axId val="111"/><c:title>…</c:title></c:catAx><c:valAx><c:axId val="222"/></c:valAx>
 </c:plotArea></c:chart><c:externalData r:id="rId1"/>
</c:chartSpace>
```

| Chart element | Series data | Notes |
|---|---|---|
| `c:barChart`/`c:bar3DChart` | `c:cat`, `c:val` | `barDir` col/bar; `grouping` clustered/stacked/percentStacked |
| `c:lineChart`, `c:areaChart`, `c:radarChart`, `c:stockChart` | `c:cat`, `c:val` | |
| `c:pieChart`/`c:doughnutChart`/`c:ofPieChart` | `c:cat`, `c:val` | Usually 1 series |
| `c:scatterChart` | `c:xVal`, `c:yVal` | Numeric x |
| `c:bubbleChart` | + `c:bubbleSize` | |
| Combo | Several chart elements in one `plotArea` | Merge on shared axis |

**Rules:**
1. Size arrays by `ptCount` and fill from `c:pt[@idx]`. Missing `idx` is blank, so never assume dense data.
2. Categories may be `c:numRef` (dates via `formatCode`) or `c:multiLvlStrRef`. Flatten the latter to "Parent / Child".
3. Literals appear as `c:strLit`/`c:numLit`.
4. If caches are missing, resolve `c:f` against the embedded XLSX with `calamine` (pure Rust) and tag `source=workbook`.
5. The series name is `c:tx`. Axis titles join chart groups via `c:axId`.

**Chartex (`cx:`):** PowerPoint 2016 added eight chart types: waterfall, funnel, treemap, sunburst, histogram, pareto, box & whisker and region map.\[10\]\[11\]\[12\] They "are stored in a different XML vocabulary (chartex, the cx: namespace, in a chartEx{n}.xml part)".\[13\] The namespace is `http://schemas.microsoft.com/office/drawing/2014/chartex`, and the type is specified in [MS-ODRAWXML] §5.22, not ISO/IEC 29500.\[13\]\[14\] Frames usually sit in `mc:AlternateContent` (Choice `cx`, with a picture Fallback).\[15\] Data lives in `cx:chartData/cx:data[@id]` with `cx:strDim[@type="cat"]` and `cx:numDim[@type="val"|"size"]` (`cx:f`, `cx:lvl/cx:pt`). Series are `cx:plotAreaRegion/cx:series[@layoutId="waterfall"|"treemap"|"sunburst"|"boxWhisker"|"funnel"|"paretoLine"|"regionMap"|"clusteredColumn"]` with `cx:dataId`. Treemap and sunburst use multi-level `cx:lvl`, which becomes a path-column table.\[16\] python-pptx cannot read chartex.\[17\]

### 2.8 Chart → Markdown/Mermaid representation (state as of Oct 2026)

Mermaid's docs header currently shows **12.1.0**. The XY chart page says it "includes two fundamental chart types: the bar chart and the line chart", and that the y-axis "cannot have categorical values".\[18\] Recent additions: legends for named series (v11.17.0+), `showDataLabel` (v11.14.0+), and per-point text labels on `line` only (v11.16.0+). The official docs now use the keyword `xychart`.\[18\] Older guides and renderers expect `xychart-beta`, and one tutorial claims the bare keyword "won't parse", which held only for older versions.\[19\] Make the keyword configurable, and default to `xychart-beta` for GitHub-style renderers that pin older Mermaid. Mermaid v11.13.0 (April 2026) added Venn and Ishikawa,\[20\] and the docs list Sankey, Radar, Treemap, Wardley and others.\[18\]

| Source chart | Canonical output | Optional Mermaid | Why |
|---|---|---|---|
| Bar/column/line/area | **GFM table** (categories × series) | `xychart` bar/line (+`horizontal`) | xychart cannot stack/percent-stack faithfully |
| Pie/doughnut | GFM table | `pie showData` | Single series only |
| Scatter/bubble | GFM table (x, y[, size]) | None | No point marks; avoid lossy plots |
| Combo bar+line | GFM table | `xychart` | No secondary axis, so flag it |
| Waterfall/histogram/pareto/box/funnel | GFM table | Waterfall → `xychart bar` + note | No native types |
| Treemap/sunburst | Path table | `treemap` | Native (beta) |
| Radar | GFM table | `radar` | |

**Policy:** the GFM table is the source of truth, and Mermaid is decoration. Always add a caption, and escape `|` and line breaks inside cells. MarkItDown had exactly this bug: a series named `Windows | Linux` added an extra column and "disconnect[ed] labels from their numeric values".\[21\]

---

## 3. Vector PDF Geometry Blueprint

### 3.1 Operators to intercept (ISO 32000-1/-2 §8.5)

| Group | Operators | Interpretation |
|---|---|---|
| Graphics state | `q`/`Q`, `cm`, `w`, `J`, `j`, `d`, `gs` | Stack {CTM, line width, dash, colours, alpha}; `cm` pre-multiplies CTM |
| Path construction | `m`, `l`, `c`, `v`, `y`, `h`, `re` | Subpaths in user space; transform by CTM at paint time |
| Painting | `S`, `s`, `f`/`F`, `f*`, `B`, `B*`, `b`, `b*`, `n` | Emit {stroke?, fill?, rule}; `n` discards |
| Clipping | `W`, `W*` | Clip region; strong figure-boundary signal |
| XObjects | `Do` | Form: recurse with CTM × `/Matrix`; Image: raster bbox |
| Text | `BT/ET`, `Tf`, `Tm`, `Td`, `TD`, `T*`, `Tj`, `TJ`, `'`, `"`, `Tz`, `Ts`, `Tc`, `Tw` | Glyph boxes; `Ts` rise = superscript signal |
| Marked content | `BMC`/`BDC`/`EMC` | `/Figure`, `/Artifact` hints in tagged PDF |

The output of interpretation is a flat display list of `PathItem { segs, ctm, stroke, fill, color }` plus `Glyph { bbox, unicode, font, size, rise }`. Everything after that is computational geometry.

### 3.2 Library evaluation

| Library | Language | License | Vector-path access | Verdict |
|---|---|---|---|---|
| **hayro / hayro-interpret** | Pure Rust, `forbid(unsafe)` | Apache-2.0 OR MIT | Interpreter "render[s] them into an abstract `Device`, which clients can implement"; fill/stroke path hooks, glyph runs with `as_unicode()`/`transform()`; kurbo `PathEl` | **Primary.** v0.8.0 (4 Oct 2026); handles the "1000+ PDFs" in its PDFBox/pdf.js-derived test suite; companions `hayro-syntax`, `hayro-font`, `hayro-cmap`, `hayro-svg`. Caveat: "still very much in development… lacks pretty much any documentation" |
| **pdfium-render** | Rust → C++ Pdfium | Permissive | `PdfPagePathObject` segments (`PdfPathSegments`, `raw()`/`transform()` since 0.8.10); MoveTo/LineTo/Bezier + close; no `re` | **Optional backend.** "binds to a Pdfium library at run-time" (v0.9.4, Sep 2026); native dependency |
| **lopdf** | Pure Rust | MIT (to verify) | `Content::decode` → `Operation{operator, operands}`; you build CTM/state/XObject recursion | Minimal custom interpreter |
| **pdf (pdf-rs)** | Pure Rust | MIT (to verify) | Typed `pdf::content::Op` (`MoveTo`, `LineTo`, `Close`, `Stroke`…), v0.10.0 | Viable; "writing … still experimental" |
| **MuPDF / PyMuPDF `get_drawings()`** | C | **AGPL** or commercial\[22\]\[23\] | Excellent path dicts\[24\]\[25\] | **Do not link.** AGPL obligations apply "even if it's just parsing PDFs for a RAG system"\[24\]\[26\]\[27\] |
| **Poppler** | C++ | GPL | `OutputDev` | Reference only |
| **pdfplumber / pdfminer.six** | Python | MIT | `lines`, `rects`, `curves` | Algorithmic reference (ruling-line tables) |

Artifex's own documentation states the core difficulty directly. Even with perfect path extraction "it may still be unclear (and programmatically impossible to determine) which paths belong together and actually are part of a larger figure".\[25\] That is why §3.3 is the hard part, not the parsing.

### 3.3 Algorithm: display list → figure regions → node/edge graph

```text
Stage 0 Normalise: flatten Béziers (0.25pt) keeping originals for SVG; classify:
  RECT (closed 4 axis-aligned / `re`), ROUNDRECT (4 lines + 4 arcs), ELLIPSE (4 cubics, κ≈0.5523),
  DIAMOND (closed 4-gon ~45°), ARROWHEAD (filled triangle < 40pt²), SEGMENT (stroked open),
  HAIRLINE (< 0.3pt, long → table rule); drop page-background rects and clip-only (`n`) paths
Stage 1 Figure regions: rstar RTree of bboxes inflated 3pt → union-find overlap components;
  merge by shared clip / Form XObject; HAIRLINE grids → table detector;
  attach glyphs inside hull + caption band ("Figure N")
Stage 2 Nodes: closed shapes with area > A_min; dedupe fill+stroke twins;
  label = glyphs whose centre ∈ bbox, sorted (baseline, x)
Stage 3 Edges: non-boundary SEGMENTs → endpoint graph, snap r = max(1.5·stroke_w, 1pt), chain degree-2 joints;
  per chain: nearest node boundary to each end within τ; arrowhead = ARROWHEAD centroid within
  1.5·size of an end with apex pointing outward, or open "V" (two short segments at ±20–40°);
  both ends snapped → edge; one free → T-junction split or SVG-only; label = free glyph cluster near midpoint
Stage 4 Emit: SVG always (lossless); Mermaid if conf = snapped_chains/chains × labelled/nodes ≥ 0.75
```

**Text-label coordinates.** For each glyph, compute the device-space quad as `T_text = [Tfs·Th, 0, 0, Tfs, 0, Trise] × Tm × CTM`. Take the advance from `/Widths` or the CID `/W` array, and ascent/descent from `/FontDescriptor`. hayro exposes each glyph's transform, so the origin and em size come from `props.transform × glyph.transform()`.\[28\] Group glyphs into words by gaps smaller than 0.25 em, and into lines by baseline tolerance.

**Literature grounding:**
- **ChemScraper** (arXiv 2311.12161) is the closest modern analogue. It is a "compiler-like recognition architecture" that pulls PDF characters and graphics with SymbolScraper, builds a minimum spanning tree over primitives, and rewrites it into a visual graph and then a semantic graph. It runs in O(n² log n), dominated by MST construction, and its born-digital parser beats image-based recognition because of "the additional information available from PDF instructions."\[29\]\[30\]
- **Shao & Futrelle**, "Recognition and Classification of Figures in PDF Documents" (GREC 2005, LNCS 3926). They interpret PDF into graphics/text objects, then find "graphemes", meaning pairs of primitives that satisfy geometric constraints, and classify figures with boosting. Futrelle et al. (ICDAR 2003) covered diagram extraction and layout analysis in PDFs.\[31\]
- Rusiñol et al. studied flowchart recognition for patent retrieval (CLEF-IP 2012), and structure-diagram recognition for financial announcements is in arXiv 2304.13240. Image-based systems (Arrow R-CNN, DiagramNet, FR-DETR, Flowchart2Mermaid's VLM pipeline, arXiv 2512.02170) are the raster fallback family.\[32\]\[33\]
- Ruling-line table detection (pdfplumber's strategy) reuses the same R-tree and segment-intersection machinery.

**Delaunay vs. MST vs. R-tree.** For box-and-arrow diagrams, R-tree nearest-neighbour snapping plus endpoint union-find is enough and runs in O(n log n). Use Delaunay or MST (`spade` for pure-Rust Delaunay) only for free-form diagrams where primitives touch without explicit connectors, as ChemScraper does.\[29\]

---

## 4. Mathematical Formula Extraction

### 4.1 OMML structure

Equations live in `w:p` as `m:oMath` (inline) or `m:oMathPara` (display). The namespace is `http://schemas.openxmlformats.org/officeDocument/2006/math`. PowerPoint stores OMML inside `a14:m` within `mc:AlternateContent`, with a picture Fallback. python-pptx, and therefore Docling, drops both of these. docling.rs has just fixed this (PR #584, 6 Oct 2026).\[34\] Each object is `m:<obj>` → `m:<obj>Pr` + arguments (`m:e`, `m:num`, `m:den`, `m:sub`, `m:sup`, `m:deg`, `m:lim`, `m:fName`). Text is `m:r` → `m:rPr` (`m:sty` p/b/i/bi, `m:scr` roman/script/fraktur/double-struck/sans-serif/monospace, `m:nor`) + `m:t`.

### 4.2 OMML → LaTeX mapping table

| OMML element | Key properties | LaTeX output | Pitfalls |
|---|---|---|---|
| `m:oMathPara` | `m:jc` | `$$…$$` (multiple → `gathered`) | With surrounding words → inline `$…$` |
| `m:f` | `m:type` bar/skw/lin/noBar | `\frac{num}{den}`; `lin`→`a/b`; `noBar`→`\genfrac{}{}{0pt}{}{a}{b}` (`\binom` inside `()`) | Brace when used as script base (Docling bug: `\frac{(x-c)}{v}^{2}`) |
| `m:sSup` / `m:sSub` / `m:sSubSup` | `m:alnScr` | `{e}^{sup}`, `{e}_{sub}`, `{e}_{sub}^{sup}` | Always brace base |
| `m:sPre` | | `{}_{sub}^{sup}{e}` | |
| `m:rad` | `m:degHide` | `\sqrt{e}` / `\sqrt[deg]{e}` | Empty deg → plain |
| `m:nary` | `m:chr` (default ∫), `m:limLoc`, `m:subHide`, `m:supHide` | `\sum`, `\prod`, `\int`, `\iint`, `\oint`, `\bigcup`, `\bigcap`, `\bigoplus`…; `\limits` for `undOvr` | **Missing `m:chr` = integral** |
| `m:d` | `m:begChr` (default `(`), `m:endChr` (default `)`), `m:sepChr` (default `│`) | `\left( e_1 \middle| e_2 \right)` | Multiple `m:e`;\[35\] empty `begChr` → `\left.`; `{`→`\{`, `⟨`→`\langle` |
| `m:m` | `m:mcs` | `matrix`; inside `m:d` ( ) → `pmatrix`, [ ] → `bmatrix`, \| \| → `vmatrix` | Collapse delimiter+matrix |
| `m:eqArr` | | `\begin{aligned}…\\…\end{aligned}`; `&` from Word alignment marks | texmath fixed eqArr (#196)\[36\] |
| `m:func` | `m:fName` | `\sin`, `\cos`, `\log`, `\ln`, `\exp`, `\lim`, `\max`, `\det`… else `\operatorname{}` | Docling missed `log`; MarkItDown crashed on unknown names\[37\] |
| `m:limLow` / `m:limUpp` | | `\lim_{…}` if base is lim, else `\underset{}{}` / `\overset{}{}` | |
| `m:acc` | `m:chr` (default U+0302) | `\hat`, `\tilde`, `\bar`, `\dot`, `\ddot`, `\vec`, `\check`, `\breve`; wide variants | Default char |
| `m:bar` | `m:pos` | `\overline{}` / `\underline{}` | |
| `m:groupChr` | `m:chr` (default ⏟), `m:pos` | `\underbrace{}` / `\overbrace{}`; arrows → `\xrightarrow{}` | With `limLow` → `\underbrace{e}_{label}` |
| `m:box` | | transparent | |
| `m:borderBox` | `hide*`, `strike*` | `\boxed{e}` | MarkItDown drops it, so a boxed equation becomes `$$$$`\[38\] |
| `m:phant` | | `\phantom{}` | |
| `m:r` / `m:t` | `m:sty`, `m:scr`, `m:nor` | Unicode → LaTeX; `double-struck`→`\mathbb`, `script`→`\mathcal`, `fraktur`→`\mathfrak`, `sty=p`→`\mathrm`, `b`→`\mathbf`, `nor`→`\text{}` | `m:r` with no `m:t` crashed MarkItDown and dropped *every* equation in the file\[39\] |
| `w:sym` | `w:font`, `w:char` | Symbol-font PUA → Unicode | texmath "Handle w:sym (#65)"\[36\] |

**Unicode symbol table.** Ship a static `phf` map of about 2,500 entries: Greek, relations (≤ `\le`, ≥ `\ge`, ≠ `\neq`, ≈ `\approx`, ∈ `\in`), arrows, operators (× `\times`, · `\cdot`, ± `\pm`, ∞ `\infty`, ∂ `\partial`, ∇ `\nabla`) and Mathematical Alphanumerics (U+1D400–1D7FF). Generate it at build time from `unicode-math-table.tex`, or regenerate the data from texmath's tables rather than copying GPL code. Escape LaTeX specials only in `m:nor`/text runs. Docling escaped en-dash and caret "as text macros" inside math, which is wrong.\[40\] Map U+2212 → `-`.

### 4.3 Recommended OMML algorithm (Rust)

```rust
pub fn omml_to_latex(n: roxmltree::Node) -> String {
    match n.tag_name().name() {
        "oMath"|"e"|"num"|"den"|"sub"|"sup"|"deg"|"lim"|"fName"|"box" => children(n),
        "f"       => frac(n),
        "sSup"    => format!("{{{}}}^{{{}}}", arg(n,"e"), arg(n,"sup")),
        "sSub"    => format!("{{{}}}_{{{}}}", arg(n,"e"), arg(n,"sub")),
        "sSubSup" => format!("{{{}}}_{{{}}}^{{{}}}", arg(n,"e"), arg(n,"sub"), arg(n,"sup")),
        "rad"     => radical(n),      // degHide / empty deg
        "nary"    => nary(n),         // default chr ∫
        "d"       => delim(n),        // ALL m:e joined by sepChr (default │)
        "m"       => matrix(n),  "eqArr" => eq_arr(n),  "func" => func(n),
        "acc"     => accent(n),  "bar" => bar(n),  "groupChr" => group_chr(n),
        "limLow"|"limUpp" => lim(n),
        "borderBox" => format!(r"\boxed{{{}}}", arg(n,"e")),
        "r"       => run(n),          // m:t may be absent → ""
        name if name.ends_with("Pr") => String::new(),
        _         => children(n),     // unknown: keep content, never drop
    }
}
```

**Invariant:** unknown or malformed constructs keep their child content and never discard the equation. Every competitor bug listed above comes from violating this rule.

| Implementation | Language | Status / pitfalls |
|---|---|---|
| **texmath** (jgm), used by Pandoc | Haskell | Most mature; OMML since Pandoc 1.9 (2012); long fix history (eqArr, delimiters, arrows, `w:sym`); GPL\[36\]\[41\] |
| `OMML2MML.XSL` + `mmltex` | XSLT | Two hops; no good pure-Rust XSLT; MS licensing of the XSL |
| **dwml** / MarkItDown `omml.py` | Python | MarkItDown's copy is "adapted from the dwml project"; open bugs: `borderBox` dropped, multi-arg `m:d` keeps last only, crash on `m:r` without `m:t`, UTF-16 XML drops equations\[35\]\[38\]\[39\]\[42\]\[43\] |
| **Docling** `omml.py` | Python | Fraction braces, text escaping, `log` (#3120) |
| **docling.rs** `omml.rs` | Rust (roxmltree) | About 507 lines; now wired into PPTX `a14:m` and RTF `\mmath` (Oct 2026); good cross-check |
| **ooxml-omml** v0.1.0 | Rust | Typed ECMA-376 structs; no LaTeX writer |
| **docx2tex** (le-tex) | XSLT/XProc | Java/Saxon stack |

**Recommendation:** write a native `omml.rs` of about 800 lines with `roxmltree`, port test *ideas* from texmath's `test/reader/omml`, and differential-test against docling.rs.

### 4.4 PDF math glyphs: why `≥` becomes garbage

1. **TeX fonts use non-Unicode built-in encodings.** CMSY uses OMS, CMMI uses OML and CMEX uses OMX. They are embedded as Type 1 fonts, often without `/ToUnicode`. ISO 32000-1 §9.10.2 falls back from ToUnicode to the encoding's glyph names → AGL, and otherwise it guesses. CMSY and CMEX place glyphs at codes 0x00–0x1F. all2md PR #544 ("fix(pdf): read TeX math symbols embedded without a Unicode map") reports that, without ToUnicode, PyMuPDF "returns the raw codes", so 'Scoring >=50%' comes out as 'Scoring \x15 50%'. The same PR lists CMSY codes 0x00 −, 0x03 ∗, 0x0F • and **0x15 ≥**. A naive extractor strips the control character or maps it through WinAnsi/Standard to an unrelated printable. Those are the "≥ → 2"-class errors.
2. **Glyph names outside the AGL.** pdfTeX CharSets use names such as `lscript` (ℓ), `floorleft`/`floorright` and `negationslash`. hayro-interpret 0.7.0 returned `None` for these, and the issue reports "dozens of missing ℓ on body pages".\[44\] Other Rust extractors had incomplete AGL tables (oxidize-pdf mapped only digits and A–C), which corrupts `/Differences`-encoded fonts.\[45\]
3. **Wrong ToUnicode maps.** A present map can be wrong. In the DEV Community post "My missing ToUnicode test did not explain every PDF" (token111), the author kept ToUnicode but changed the entry for 假 to 真. The page still rendered as the original notice, "yet all three extractors returned the wrong Chinese character". Extractor agreement therefore does not establish correctness.
4. **Layout loss.** A fraction is numerator, a filled `re` rule and a denominator on different baselines, and scripts are smaller glyphs with a raised baseline. Reading-order extraction linearises these into `a + b 2 2vah`-style soup.

**Deterministic recovery stack:**

```text
1. ToUnicode present → use, but VALIDATE against TeX tables for TeX fonts (log conflicts)
2. BaseFont (strip "ABCDEF+") ∈ CMSY|CMMI|CMEX|CMR|MSAM|MSBM|EUFM|rsfs|LM*|STIX*… → OMS/OML/OMX/OT1/U tables
3. glyph name from Type1/CFF charset or /Differences → AGL + AGLFN + TeX extended names, uniXXXX
4. TrueType cmap (3,1) / (3,0) symbol → U+F0xx − 0xF000
5. else U+FFFD + record (font, code) for optional shape hashing
```

**Layout reconstruction (heuristic, gated):** in math-dense zones, detect:
- **Scripts:** glyph size < 0.8 × base size, baseline shift > 0.2 em.
- **Fractions:** a horizontal filled rule (height < 1pt) with centred runs above and below.
- **Radicals:** a CMEX radical glyph plus an overline rule.
- **Big operators:** CMEX `∑`/`∫` with stacked limits.

This is InftyReader-style structural analysis applied to born-digital glyphs, where the symbols are already certain. Emit LaTeX only when every glyph in the zone is assigned, and otherwise emit Unicode with a `<!-- math-uncertain -->` marker.

### 4.5 Lightweight ML fallback for math (optional, flagged)

| Model / project | Runtime | CPU viability | Notes |
|---|---|---|---|
| LaTeX-OCR / pix2tex | ONNX ports (RapidLaTeXOCR)\[46\] | BrightCoding's "From Pixel to Proof" (Sep 2025) claims RapidLaTeXOCR runs "inference in 40 ms on a laptop CPU"; the dsh-latex-ocr plugin (yunnalab), which calls the local pix2tex CLI, reports "about 1-7 seconds per recognition" on CPU | Crop-level; needs a detector; MIT |
| UniMERNet-tiny ONNX | Pure ONNX Runtime\[47\] | Built for CPU | Check weights licence |
| Pix2Text MFR (TrOCR-based) | ONNX default backend\[48\]\[49\] | Yes | Includes YOLO formula detector |
| Texify (Datalab) | Superseded by Surya/Chandra | — | Modified OpenRAIL-M weights |
| Granite-Docling-258M | 258M VLM, Apache-2.0 (17 Sep 2025)\[2\]\[50\] | Slow per page | IBM's Hugging Face model card reports equation-recognition F1 0.968 vs 0.947 for smoldocling-256m-preview (edit distance 0.073 vs 0.119) |

**Policy:** run ML only on raster formula images and on PDF math zones that failed the deterministic gate. Use `ort`, which is a **native dependency**, behind the Cargo feature `ml-math`, or `tract` (pure-Rust ONNX, narrower operator coverage). Tag output `source=ocr, confidence=…`.

---

## 5. Competitive Landscape (October 2026)

| Tool | Diagrams | Charts | Equations | Runtime / licence | Where it fails |
|---|---|---|---|---|---|
| **IBM Docling** (+ Granite-Docling, docling.rs) | Layout model finds pictures; DocumentFigureClassifier-v2.5 (26 classes) labels them | PDF: VLM chart→table for bar, pie, line (2026); **PPTX charts ignored** (issue #2306) | PDF: formula model/VLM; DOCX OMML with known bugs; PPTX OMML dropped (python-pptx) | MIT; docling.rs uses ONNX; CUDA images | No SmartArt topology, connector graph or vector-diagram→graph |
| **Marker / Surya** (Datalab) | Layout + image crops | Image crops | Surya VLM; strong inline math | Code Apache-2.0 since v2.0.0 (2026-07-20; 1.x was GPL-3.0); weights modified OpenRAIL-M (≥$5M revenue/funding needs licence; Chandra $2M); GPU-preferred | Heavy, licence-gated, page-image-first, so Office semantics are discarded |
| **Microsoft MarkItDown** | Alt text; optional LLM captions | PPTX charts → tables via python-pptx | DOCX OMML (dwml-derived) with open bugs\[43\]\[51\] | MIT; mammoth + python-pptx | No SmartArt, chartex or connectors; PDF via pdfminer (no math)\[52\] |
| **Pandoc** | Images; TikZ kept as `RawBlock "latex"` | None | Best-in-class via texmath; `Math InlineMath/DisplayMath` AST nodes\[53\]\[54\] | GPL; Haskell | No diagram/chart semantics; table model fine for tables only |
| MinerU, Nougat, olmOCR, LlamaParse, Mathpix, Unstructured, GROBID | Raster/VLM detection | VLM/hosted | VLM/OCR | GPU or SaaS | Ignore native Office objects; non-deterministic |
| **pptx2markdown** (Python, 2026) | SmartArt via `smartart2md` | `chart2md` | `omml2latex` | Python | Closest deterministic peer; PPTX-only |

**Unserved gaps, ranked by value to filegoblin:**
1. **SmartArt → Mermaid with real topology.** Nobody mainstream does it, and python-pptx exposes `has_smart_art` but "SmartArt is not yet supported."\[55\]
2. **Connector graphs → Mermaid flowcharts** from `stCxn`/`endCxn`, including DOCX `wps` and VML.
3. **Chart caches → exact tables, including chartex.**
4. **A correct-by-construction OMML → LaTeX in pure Rust**, for DOCX + PPTX `a14:m` with a never-drop invariant.
5. **Vector PDF diagram → SVG (lossless) + Mermaid (gated)** in a single static binary.
6. **TeX-font-aware PDF text extraction**, fixing the most common cause of garbled math without any model.
7. **Determinism and speed as a feature:** byte-identical output, no GPU and no model download, so it suits CI and air-gapped use.

---

## 6. Phase-by-Phase Implementation Roadmap (`crates/core`)

**Dependency policy:** pure Rust by default. A native dependency is allowed only behind an opt-in Cargo feature.

| Crate | Purpose | Native deps? |
|---|---|---|
| `zip` (rust deflate backend) | OPC package reading | No |
| `quick-xml` | Streaming parse of large parts | No |
| `roxmltree` | DOM for diagram/chart/OMML subtrees | No |
| `calamine` | Embedded XLSX fallback for `c:f` | No |
| `phf` | Static symbol/encoding tables | No |
| `hayro-interpret` / `-syntax` / `-font` / `-cmap` | PDF interpretation, fonts, CMaps | No (`forbid(unsafe)`) |
| `lopdf` or `pdf` | Plan-B content parsing | No |
| `kurbo` | Béziers, affines, flattening | No |
| `rstar` | R-tree spatial index | No |
| `spade` (optional) | Delaunay | No |
| `petgraph` | Components, topo order | No |
| `pdfium-render` (feature `pdfium`) | Alternate PDF backend | **Yes**: runtime-loaded Pdfium |
| `ort` (feature `ml-math`) | ONNX Runtime | **Yes**: onnxruntime shared lib |
| `tract-onnx` (feature `ml-math-pure`) | Pure-Rust ONNX | No |

**Phase 0 — Foundations (1–2 weeks).**
- An `opc` module: package reader, part-relative `.rels`, and an `mc:AlternateContent` resolver that prefers Choices whose `Requires` namespaces are known (`a14`, `cx`, `wps`).
- An `ir` module: `DiagramGraph { nodes, edges, direction, provenance, confidence }`, `ChartData`, `MathExpr`.
- `insta` golden-snapshot tests.

**Phase 1 — OMML → LaTeX (2 weeks, highest ROI).**
- Implement §4.2/§4.3 with the never-drop invariant and the `phf` symbol table.
- Wire it into DOCX paragraphs, DOCX textboxes and PPTX `a14:m`.
- Test default-character cases, malformed runs and UTF-16 parts, and differential-test against docling.rs/pandoc.

**Phase 2 — Charts (2 weeks).**
- `chart::classic`: all `c:*Chart`, combo merge, `multiLvlStrRef`, literals, sparse `idx`.
- `chart::ex`: cx dims, `layoutId`, levels.
- Emitter: GFM table always, optional Mermaid with a configurable `xychart` keyword, `calamine` fallback.

**Phase 3 — SmartArt (2 weeks).**
- Data-model graph, the §2.3 family table and the Mermaid emitter.
- `dsp:drawing` fallback, and a nested list for unknown layouts.

**Phase 4 — Connectors (2–3 weeks).**
- spTree walker with group transforms, flips and rotation.
- PPTX `p:cxnSp`, DOCX `wps`/`wpg`/`wpc` and VML `o:rules`.
- Glue-first topology, R-tree snapping, junction merging, and a confidence score in a `%%` comment.

**Phase 5 — PDF text correctness for math (2 weeks).**
- TeX encoding tables, extended AGL and ToUnicode validation.
- Upstream the glyph-name additions to hayro (issue #1369).

**Phase 6 — Vector PDF figures (4–6 weeks).**
- `pdf::geom` implementing hayro's `Device`.
- Classifier, clustering, node/edge assembly and arrowheads (§3.3).
- SVG always, gated Mermaid, and ruling-line tables on the same index.

**Phase 7 — Math layout reconstruction (research, 4+ weeks).** Rules for scripts, fractions, radicals and big operators, emitted only when the confidence gate passes.

**Phase 8 — Optional hybrid.** `ml-math` with `ort`/`tract`. Models are fetched explicitly (`filegoblin models fetch`), never bundled, and output is tagged `source=ocr`.

**Ordering rationale:** Phases 1–4 are pure XML transforms with exact answers, so each ships independently and can be tested against snapshots. Phases 5–6 depend on a pre-1.0 interpreter, so pin it and wrap it behind a `PdfBackend` trait with lopdf as plan B. Phases 7–8 are opt-in.

---

## 7. Caveats

- **hayro's API is not stable.** The crate says it "lacks pretty much any documentation"\[56\] and its API changed between 0.7 and 0.8. Exact `Device` signatures and pdfium-render's `PdfPathSegmentType` variant names were not verified for this report, so check them on docs.rs. The licenses of lopdf and pdf-rs (believed to be MIT) also need confirming.
- **Mermaid keyword drift.** `xychart` vs `xychart-beta` depends on the renderer, and GitHub often lags. Newer types (treemap, radar, venn) may not render everywhere.\[57\]
- **The SmartArt layout table is heuristic.** Microsoft ships around 200 built-in layouts, and the family → direction mapping is curated, not specified. The `parOf` tree is exact, but sequence edges are an interpretation.
- **Unglued connectors and Word drawings** rely on geometric snapping. Report a confidence score, and never present inferred edges as certain.
- **Marker licensing facts** come partly from secondary write-ups (the v2.0.0 Apache-2.0 relicensing date) and partly from Datalab's own pricing page (the $5M Marker/Surya and $2M Chandra thresholds).\[58\]\[59\] Older articles cite GPL-3.0 and $2M because they describe the 1.x era.\[60\]\[61\]
- **ML performance figures:** the RapidLaTeXOCR 40 ms claim comes from a BrightCoding blog post and the pix2tex 1–7 s figure from the dsh-latex-ocr plugin README, not independent benchmarks. The Granite-Docling equation F1 (0.968 vs 0.947) comes from IBM's own Hugging Face model card. Benchmark all of them on your own corpus before relying on them.
- **The competitor bug list is a snapshot** of open issues in October 2026. Some may be fixed by the time `filegoblin` ships, but the architectural gaps (no SmartArt topology, no connector graphs, no chartex, rasterise-first PDF pipelines) are structural.

## Sources

1. [GitHub - datalab-to/marker: Convert PDF to markdown + JSON quickly with high accuracy · GitHub](https://github.com/datalab-to/marker)
2. [IBM Granite-Docling: End-to-end document understanding](https://www.ibm.com/new/announcements/granite-docling-end-to-end-document-conversion)
3. [diagram package - github.com/mgilbir/spine/common/dml/diagram - Go Packages](https://pkg.go.dev/github.com/mgilbir/spine/common/dml/diagram)
4. [Class DGM](https://docs.dndocs.com/n/DocumentFormat.OpenXml.Linq/3.0.0/api/DocumentFormat.OpenXml.Linq.DGM.html)
5. [SmartArt System](https://deepwiki.com/ONLYOFFICE/sdkjs/5.3-smartart-system)
6. [Creating Custom SmartArt Layouts with Office Open XML for Office 2007 and Office 2010](<https://learn.microsoft.com/en-us/previous-versions/office/developer/office-2010/gg583880(v=office.14)>)
7. [Office Open XML - DrawingML - Connectors - Non-Visual Properties](http://officeopenxml.com/drwSp-nvCxnSpPr.php)
8. [ConnectionShape Class (DocumentFormat.OpenXml.Drawing)](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.connectionshape?view=openxml-2.8.1)
9. [cxn (Shape Connection Site)](https://c-rex.net/samples/ooxml/e1/Part4/OOXML_P4_DOCX_cxn_topic_ID0E5IGNB.html)
10. [fix(chart): complete remaining ChartEx layouts and fail-closed behavior · Issue #1297 · yukiyokotani/office-open-xml-viewer](https://github.com/yukiyokotani/office-open-xml-viewer/issues/1297)
11. [mschart: NEWS.md](https://rdrr.io/cran/mschart/f/NEWS.md)
12. [GitHub - office-kit/xlsx: A JavaScript library for Excel (XLSX) files, inspired by openpyxl · GitHub](https://github.com/office-kit/xlsx)
13. [Support chartex chart types (waterfall, funnel, treemap, sunburst, histogram, pareto, box & whisker, region map) · Issue #3 · keithcsqueensu/python-pptx](https://github.com/keithcsqueensu/python-pptx/issues/3)
14. [xlsx/chart-type--treemap-chart — case detail](https://loadfix.github.io/ooxml-reference-corpus/case/xlsx__chart-type--treemap-chart.html)
15. [feat: Excel 2016 charts (waterfall, histogram, Pareto, box and whisker, treemap, sunburst, funnel) by haris-musa · Pull Request #195 · haris-musa/excel-mcp-server](https://github.com/haris-musa/excel-mcp-server/pull/195)
16. [ms\_sunburstchart: Sunburst chart object in mschart: Chart Generation for 'Microsoft Word', 'Microsoft Excel' and 'Microsoft PowerPoint' Documents](https://rdrr.io/cran/mschart/man/ms_sunburstchart.html)
17. [GitHub - t-ujiie-g/moon-pptx: Pure-MoonBit library for reading, building, and writing PPTX (OOXML) presentations with a type-safe builder API. · GitHub](https://github.com/t-ujiie-g/moon-pptx)
18. [Mermaid](https://docs.mermaidchart.com/mermaid-oss/syntax/xyChart.html)
19. [Mermaid Bar Chart Syntax & Examples: Copy-Paste XY Chart Code](https://mermaideditor.lol/blog/mermaid-xy-chart-tutorial)
20. [Mermaid.js v11.13.0: Two New Diagram Types and Our Most Polished Release Yet](https://mermaid.ai/blog/posts/mermaid-v11-13-0-two-new-diagram-types-and-our-most-polished-release-yet)
21. [Fix Markdown escaping for PowerPoint chart table labels by dhairyajangir · Pull Request #2585 · microsoft/markitdown](https://github.com/microsoft/markitdown/pull/2585)
22. [pymupdf · PyPI](https://pypi.org/project/pymupdf/1.19.5/)
23. [License - MuPDF 1.27.0](https://mupdf.readthedocs.io/en/1.27.0/license.html)
24. [FAQ - PyMuPDF documentation](https://pymupdf.readthedocs.io/en/latest/faq/index.html)
25. [Extracting and Creating Vector Graphics in a PDF Using PyMuPDF](https://artifex.com/blog/extracting-and-creating-vector-graphics-in-a-pdf-using-python-pymupdf)
26. [Is PyMuPDF Free for Commercial Use? The AGPL License Explained](https://www.file2markdown.ai/blog/is-pymupdf-free-for-commercial-use)
27. [Licensing](https://pymupdf.io/licensing)
28. [Research: hayro-interpret as the benchmark text extractor · Issue #4 · shythulu/PDFPundit](https://github.com/shythulu/PDFPundit/issues/4)
29. [ChemScraper: Leveraging PDF Graphics Instructions for Molecular Diagram Parsing](https://arxiv.org/pdf/2311.12161)
30. [ChemScraper: Leveraging PDF Graphics Instructions for Molecular Diagram Parsing](https://arxiv.org/html/2311.12161v4)
31. [Recognition and Classification of Figures in PDF Documents](https://link.springer.com/chapter/10.1007/11767978_21)
32. [Structure Diagram Recognition in Financial Announcements](https://arxiv.org/pdf/2304.13240)
33. [Flowchart2Mermaid: A Vision-Language Model Powered System for Converting Flowcharts into Editable Diagram Code](https://arxiv.org/pdf/2512.02170)
34. [fix(pptx,rtf): OMML equations become LaTeX instead of being dropped by artiz · Pull Request #584 · docling-project/docling.rs](https://github.com/docling-project/docling.rs/pull/584)
35. [DOCX math: a delimiter with several arguments keeps only the last one · Issue #2573 · microsoft/markitdown](https://github.com/microsoft/markitdown/issues/2573)
36. [texmath :: Stackage Server](https://www.stackage.org/nightly-2025-05-27/package/texmath-0.12.10.2)
37. [fix: preserve malformed docx math content by he-yufeng · Pull Request #1998 · microsoft/markitdown](https://github.com/microsoft/markitdown/pull/1998)
38. [DOCX math: a boxed formula (\`\<m:borderBox\>\`) and everything inside it are dropped · Issue #2610 · microsoft/markitdown](https://github.com/microsoft/markitdown/issues/2610)
39. [bug: DOCX math converter crashes (and silently drops all equations) when a math run has no text child · Issue #2188 · microsoft/markitdown](https://github.com/microsoft/markitdown/issues/2188)
40. [OMML-to-LaTeX conversion produces incorrect output for fractions, math operators, and functions · Issue #3120 · docling-project/docling](https://github.com/docling-project/docling/issues/3120)
41. [Pandoc - twenty-years-of-pandoc](https://pandoc.org/twenty-years-of-pandoc.html)
42. [fix(docx): preserve equations in UTF-16 XML by LeonSGP43 · Pull Request #2602 · microsoft/markitdown](https://github.com/microsoft/markitdown/pull/2602)
43. [DOCX Converter](https://deepwiki.com/microsoft/markitdown/3.2.1-docx-converter)
44. [Incomplete glyph-name → Unicode for TeX/CM/MathTime names (lscript, floorleft, ceilingleft, …) · Issue #1369 · LaurenzV/hayro](https://github.com/LaurenzV/hayro/issues/1369)
45. [fix(text): complete Adobe Glyph List mapping in glyph\_name\_to\_unicode for /Differences font encodings · Issue #572 · bzsanti/oxidizePdf](https://github.com/bzsanti/oxidizePdf/issues/572)
46. [From Pixel to Proof: Converting Images of Math Formulas into LaTeX Code - BrightCoding](https://www.blog.brightcoding.dev/2025/09/28/from-pixel-to-proof-converting-images-of-math-formulas-into-latex-code)
47. [math-ocr · GitHub Topics · GitHub](https://github.com/topics/math-ocr)
48. [breezedeus/pix2text-mfr · Hugging Face](https://huggingface.co/breezedeus/pix2text-mfr)
49. [LatexOCR - Pix2Text - Read the Docs](https://pix2text.readthedocs.io/zh-cn/stable/pix2text/latex_ocr/)
50. [ibm-granite/granite-docling-258M · Hugging Face](https://huggingface.co/ibm-granite/granite-docling-258M)
51. [Add support for mathematical formulas in DOCX conversion · Issue #289 · microsoft/markitdown](https://github.com/microsoft/markitdown/issues/289)
52. [Math in latex format in markdown files · microsoft/markitdown · Discussion #1305](https://github.com/microsoft/markitdown/discussions/1305)
53. [GitHub - jgm/texmath: A Haskell library for converting LaTeX math to MathML. · GitHub](https://github.com/jgm/texmath)
54. [8.13 Math](https://pandoc.org/demo/example33/8.13-math.html)
55. [python-pptx-ng · PyPI](https://pypi.org/project/python-pptx-ng/)
56. <https://docs.rs/hayro-interpret>
57. [Expand Mermaid showcase to cover all diagram types, including newer architecture/kanban/radar ones · Issue #111 · JeanKadang/DOC-GitHub-Practice-Skills](https://github.com/JeanKadang/DOC-GitHub-Practice-Skills/issues/111)
58. [Docling vs MinerU vs Marker (2026): Code and Model Licenses](https://particula.tech/blog/docling-vs-mineru-vs-marker-pdf-parser)
59. [Pricing](https://www.datalab.to/pricing)
60. [Marker: How a Multi-Stage CV Pipeline Achieves 25 Pages/Second PDF Parsing](https://starlog.is/articles/developer-tools/datalab-to-marker/)
61. [Datalab develops small AI models to power 'document intelligence' in the enterprise](https://www.forkable.io/p/datalab-develops-small-ai-models)

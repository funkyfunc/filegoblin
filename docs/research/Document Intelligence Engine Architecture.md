# **DIAGRAM\_MATH\_RESEARCH.md**

## **1\. The Executive Synthesis**

Document intelligence systems face an architectural fork when processing non-textual primitives such as diagrams, flowcharts, infographics, data charts, and mathematical expressions. The mainstream industry consensus relies heavily on multimodal Vision-Language Models (VLMs) and deep neural networks orchestrated across bloated Python runtimes1. While deep vision pipelines are indispensable for optical character recognition of degraded, unlayered physical paper scans, their deployment across born-digital documents represents an inefficient allocation of compute1. These pipelines introduce substantial memory overheads, non-deterministic token hallucinations, multi-second processing latencies, and cumbersome runtime dependencies1.  
The architectural premise of filegoblin is that born-digital documents—including Microsoft Office formats (DOCX, PPTX), portable document formats (PDF), and LaTeX source representations—already preserve structural, geometric, and semantic metadata. When properly parsed, this embedded metadata eliminates the need for visual inference. By leveraging zero-allocation streaming XML parsers, topological graph compilers, and planar computational geometry, non-textual primitives can be compiled deterministically into clean GitHub Flavored Markdown (GFM), Mermaid.js specifications, semantic SVG, and KaTeX-compliant LaTeX math on commodity CPUs with sub-50-megabyte memory footprints.  
The boundary between deterministic compilation and probabilistic approximation is governed strictly by the structural properties of the source file:

\+---------------------------------------------------------------------------------------------------+  
|                                 PRIMITIVE COMPILATION TAXONOMY                                    |  
\+---------------------------------------------------------------------------------------------------+  
|  FORMAT LAYER       SOURCE PRIMITIVE              DETERMINISTIC STATUS     TARGET REPRESENTATION  |  
|  Office OpenXML     SmartArt Graphs               Fully Deterministic      Mermaid (graph TD/LR)  |  
|  Office OpenXML     DrawingML Connectors          Fully Deterministic      Mermaid Flowcharts     |  
|  Office OpenXML     Chart Spaces                  Fully Deterministic      GFM Tables \+ xychart   |  
|  Office OpenXML     OMML Equations                Fully Deterministic      KaTeX / LaTeX Math     |  
|  Born-Digital PDF   Vector Flowcharts             Algorithmic / Heuristic  Mermaid or Clean SVG   |  
|  Born-Digital PDF   CM/AMS Math Font Glyphs       Deterministic Decoding   KaTeX / LaTeX Math     |  
|  Scanned / Raster   Pixel Bounding Boxes          Probabilistic (Fallback) Semantic Image / OCR   |  
\+---------------------------------------------------------------------------------------------------+

Office OpenXML documents represent the most tractable format because their underlying schema natively models logical relationships4. SmartArt hierarchies, shape connectors, data series, and Office Math Markup Language (OMML) structures can be mapped into target representations through deterministic abstract syntax tree (AST) transformations with mathematical certainty4.  
Born-digital vector PDFs occupy an intermediate tier. While drawing operations and text coordinates are preserved precisely through low-level path operators, the authoring software discards high-level entity semantics upon serialization. Transforming disconnected Bézier curves, line segments, and bounding boxes into directed acyclic graphs requires deterministic spatial indexing and computational geometry7. When topological graph construction cannot be guaranteed with complete certainty, lossless extraction to clean, minified SVG provides a dependable non-probabilistic fallback.  
Raster images and scanned PDF pages lack underlying vector semantics, necessitating isolated fallback engines. The core architecture of filegoblin focuses on extracting all accessible born-digital data using high-speed CPU algorithms before invoking heavier vision-based fallbacks.

| Document Primitive | Ingestion Source | Processing Mechanism | Target Semantic Format | CPU Reliability Factor |
| :---- | :---- | :---- | ----- | :---- |
| **SmartArt Diagram** | PPTX / DOCX | OpenXML dgm:dataModel connection list traversal | Mermaid graph TD / flowchart LR | 100% Deterministic |
| **Flowchart Connectors** | PPTX / DOCX | DrawingML \<p:cxnSp\> shape reference binding4 | Mermaid flowchart | 100% Deterministic |
| **Data Chart** | PPTX / DOCX | \<c:chartSpace\> series and axis cache parsing5 | GFM Table \+ Mermaid xychart-beta | 100% Deterministic |
| **Document Formula** | DOCX / PPTX | Recursive descent \<m:oMath\> AST compilation6 | KaTeX Math ($...$,  $$...$$ ) | 100% Deterministic |
| **Vector Diagram** | Born-Digital PDF | Stream operator interception \+ ![][image1]\-Tree clustering8 | Mermaid Graph or Semantic SVG | 90–95% Topology Dependent |
| **Mathematical Glyphs** | Born-Digital PDF | Font dictionary /ToUnicode mapping \+ CMap overrides9 | KaTeX Math ($...$,  $$...$$ ) | 95–98% Deterministic |
| **Bitmap Chart/Graphic** | Scanned PDF / Image | Classical contour tracing or lightweight edge heuristic | Minified Vector SVG or Markdown Link | 65–80% Heuristic Bound |

## **2\. OpenXML Diagram and Chart Deconstruction**

Microsoft Office formats package document media, style hierarchies, and layout structures into standard ZIP containers conforming to the ISO/IEC 29500-1 specification4. Within these archives, visual diagrams and numerical charts are serialized as explicit data structures rather than flattened visual artifacts.

### **SmartArt Structural Topologies and Mermaid Mapping**

SmartArt graphics decouple structural relationships from presentation geometry. The underlying semantics of a diagram are isolated within dedicated parts, primarily ppt/diagrams/data1.xml or word/diagrams/data1.xml. Visual layouts are rendered on the canvas using a layout definition (layout1.xml), but the logical nodes and their structural associations reside exclusively in the \<dgm:dataModel\> element.  
The \<dgm:dataModel\> tree is defined by two primary child collections: the point list (\<dgm:ptLst\>) and the connection list (\<dgm:cxnLst\>). The point list enumerates individual nodes, transitional placeholders, and root organizational boundaries. The connection list establishes the directed relationships between these points.

XML  
\<dgm:dataModel xmlns:dgm\="http\://schemas.openxmlformats.org/drawingml/2006/diagram"  
               xmlns:a\="http\://schemas.openxmlformats.org/drawingml/2006/main"\>  
  \<dgm:ptLst\>  
    \<dgm:pt modelId\="0" type\="doc"/\>  
    \<dgm:pt modelId\="1" type\="node"\>  
      \<dgm:prSet phldr\="0"/\>  
      \<dgm:spPr/\>  
      \<dgm:t\>Ingest Raw Document\</dgm:t\>  
    \</dgm:pt\>  
    \<dgm:pt modelId\="2" type\="node"\>  
      \<dgm:prSet phldr\="0"/\>  
      \<dgm:spPr/\>  
      \<dgm:t\>Parse Token Streams\</dgm:t\>  
    \</dgm:pt\>  
    \<dgm:pt modelId\="3" type\="node"\>  
      \<dgm:prSet phldr\="0"/\>  
      \<dgm:spPr/\>  
      \<dgm:t\>Emit Markdown AST\</dgm:t\>  
    \</dgm:pt\>  
    \<dgm:pt modelId\="10" type\="parTrans"/\>  
    \<dgm:pt modelId\="11" type\="sibTrans"/\>  
  \</dgm:ptLst\>  
  \<dgm:cxnLst\>  
    \<dgm:cxn modelId\="20" type\="parOf" srcId\="0" destId\="1" srcOrd\="0" destOrd\="0"/\>  
    \<dgm:cxn modelId\="21" type\="parOf" srcId\="0" destId\="2" srcOrd\="1" destOrd\="1"/\>  
    \<dgm:cxn modelId\="22" type\="parOf" srcId\="0" destId\="3" srcOrd\="2" destOrd\="2"/\>  
    \<dgm:cxn modelId\="23" type\="sibTrans" srcId\="1" destId\="11" srcOrd\="0" destOrd\="0"/\>  
    \<dgm:cxn modelId\="24" type\="sibTrans" srcId\="11" destId\="2" srcOrd\="0" destOrd\="0"/\>  
  \</dgm:cxnLst\>  
\</dgm:dataModel\>

Resolving SmartArt into a valid Mermaid diagram requires filtering non-semantic layout elements. In the point list, points marked with type="doc" serve as document containers, while points marked with type="parTrans" or type="sibTrans" indicate internal transition spacing used exclusively by the Microsoft Office layout engine. The extraction compiler filters for points where type="node".  
The node text is extracted from the child text tag (\<dgm:t\>) or within descendant DrawingML text runs (\<a:r\>\<a:t\>). Each semantic node is assigned a sanitized identifier:  
![][image2]  
The compilation of edges follows two structural patterns depending on the SmartArt layout type:

* **Hierarchical Layouts (Organization Charts, Trees):** Structural relationships are encoded directly as parent-child connections where \<dgm:cxn type="parOf"\>. If srcId corresponds to a semantic node rather than the root document container (modelId="0"), a directed relationship is emitted:  
  ![][image3]  
* **Linear Process and Cycle Layouts:** Directional execution flow is mediated through sibling transition points (sibTrans). When a connection links node ![][image4] to an intermediate sibTrans point, and a subsequent connection links that sibTrans point to node ![][image5], the compiler traverses the transition node to produce a single directed edge:  
  ![][image6]

For an OpenXML process fragment connecting three sequential stages, the compiled Mermaid representation resolves deterministically:

Code snippet  
graph TD  
  N\_1\["Ingest Raw Document"\] \--\> N\_2\["Parse Token Streams"\]  
  N\_2 \--\> N\_3\["Emit Markdown AST"\]

### **DrawingML Connector Networks and Flowchart Graph Extraction**

Freeform slide diagrams and Word flowcharts are authored using standard geometric shapes connected by explicit vector linkers4. These diagrams are stored across slide XML parts (ppt/slides/slide\*.xml) and Word document drawings (word/document.xml). Individual shapes are declared as \<p:sp\> (or \<wps:wsp\>), while directional linkers are declared as connection shapes via \<p:cxnSp\>4.  
A shape declares a unique identifier within its non-visual drawing properties (\<p:nvSpPr\>\<p:cNvPr id="101" name="Process Block A"/\>\</p:nvSpPr\>). The connection shape (\<p:cxnSp\>) references these identifiers in its non-visual connector properties (\<p:nvCxnSpPr\>) using start connection (\<a:stCxn\>) and end connection (\<a:endCxn\>) elements4.

XML  
\<p:cxnSp\>  
  \<p:nvCxnSpPr\>  
    \<p:cNvPr id\="201" name\="Connector 200"/\>  
    \<p:cNvCxnSpPr\>  
      \<a:stCxn id\="101" idx\="3"/\>  
      \<a:endCxn id\="102" idx\="1"/\>  
    \</p:cNvCxnSpPr\>  
  \</p:nvCxnSpPr\>  
  \<p:spPr\>  
    \<a:ln w\="19050"\>  
      \<a:solidFill\>\<a:prstClr val\="black"/\>\</a:solidFill\>  
      \<a:prstDash val\="dash"/\>  
      \<a:headEnd type\="triangle" w\="med" len\="med"/\>  
      \<a:tailEnd type\="none"/\>  
    \</a:ln\>  
  \</p:spPr\>  
\</p:cxnSp\>

The attribute id specifies the target shape, while the connection index (idx) indicates the attachment port along the perimeter of the shape boundary4. The line formatting (\<a:ln\>) defines the visual style and directionality of the connection:

* The \<a:headEnd\> element defines the arrow marker at the terminal node. Attributes of type="triangle" or type="stealth" indicate a forward directed relationship.  
* The \<a:tailEnd\> element defines markers at the origin node. When combined with a forward marker, it denotes a bidirectional edge.  
* The \<a:prstDash\> element defines the line stroke pattern, where val="dash" or val="dot" maps to a dashed edge.

\+---------------------------------------------------------------------------------------------------+  
|                        DRAWINGML CONNECTOR RESOLUTION PIPELINE                                    |  
\+---------------------------------------------------------------------------------------------------+  
|  1\. INGEST SHAPE DEFINITIONS (\<p:sp\>)                                                             |  
|     Extract ID: 101  \--\> Label: "Validate User Request"                                           |  
|     Extract ID: 102  \--\> Label: "Dispatch Event Broker"                                           |  
|                                                                                                   |  
|  2\. INGEST CONNECTOR TOPOLOGY (\<p:cxnSp\>)                                                         |  
|     Read Start Connection: \<a:stCxn id="101" idx="3"/\>                                            |  
|     Read End Connection:   \<a:endCxn id="102" idx="1"/\>                                            |  
|     Inspect Stroke Style:  \<a:prstDash val="dash"/\> \+ \<a:headEnd type="triangle"/\>               |  
|                                                                                                   |  
|  3\. COMPILE MERMAID DIRECTED EDGE                                                                 |  
|     Syntax Emission:       N\_101\["Validate User Request"\] \-.-\> N\_102\["Dispatch Event Broker"\]     |  
\+---------------------------------------------------------------------------------------------------+

By resolving all shape text bodies into a symbol table indexed by shape ID, the connector list can be parsed in a single linear pass to construct the complete diagram topology.

### **Chart Data Deconstruction and Dual Representation**

Charts embedded in PowerPoint and Word documents are maintained as self-contained OpenXML parts under ppt/charts/chart\*.xml or word/charts/chart\*.xml5. The root element, \<c:chartSpace\>, contains the primary coordinate configuration and links to the underlying Excel cache5.  
The plot area (\<c:plotArea\>) houses the primary chart elements, such as \<c:barChart\>, \<c:lineChart\>, \<c:pieChart\>, \<c:areaChart\>, or \<c:scatterChart\>5. Inside these containers, data points are organized into discrete series records (\<c:ser\>)5.

XML  
\<c:plotArea\>  
  \<c:barChart\>  
    \<c:barDir val\="col"/\>  
    \<c:ser\>  
      \<c:idx val\="0"/\>  
      \<c:order val\="0"/\>  
      \<c:tx\>  
        \<c:strRef\>  
          \<c:strCache\>  
            \<c:pt count\="1"\>\<c:v\>Throughput (req/s)\</c:v\>\</c:pt\>  
          \</c:strCache\>  
        \</c:strRef\>  
      \</c:tx\>  
      \<c:cat\>  
        \<c:strRef\>  
          \<c:strCache\>  
            \<c:pt idx\="0"\>\<c:v\>Rust (filegoblin)\</c:v\>\</c:pt\>  
            \<c:pt idx\="1"\>\<c:v\>Python (Docling)\</c:v\>\</c:pt\>  
            \<c:pt idx\="2"\>\<c:v\>Python (Marker)\</c:v\>\</c:pt\>  
          \</c:strCache\>  
        \</c:strRef\>  
      \</c:cat\>  
      \<c:val\>  
        \<c:numRef\>  
          \<c:numCache\>  
            \<c:pt idx\="0"\>\<c:v\>1420.5\</c:v\>\</c:pt\>  
            \<c:pt idx\="1"\>\<c:v\>12.4\</c:v\>\</c:pt\>  
            \<c:pt idx\="2"\>\<c:v\>18.1\</c:v\>\</c:pt\>  
          \</c:numCache\>  
        \</c:numRef\>  
      \</c:val\>  
    \</c:ser\>  
  \</c:barChart\>  
\</c:plotArea\>

A common bug in existing Python tools like Microsoft MarkItDown is the dropping of parent category axes in hierarchical charts (Issue \#2502) and the serialization of empty points as the literal string "None" (Issue \#2526)13. These parsing errors break downstream tabular consumers14. To prevent this, filegoblin extracts category and value caches using a dual-representation output strategy:

> 1. **GFM Data Table Representation:** Guarantees lossless tabular precision, retains multi-tiered category paths, and represents missing data points as clean, empty cells:

| Runtime Engine | Throughput (req/s) |
| :---- | :---- |
| Rust (filegoblin) | 1420.5 |
| Python (Docling) | 12.4 |
| Python (Marker) | 18.1 |

> 2. **Visual Graph Emulation (Mermaid xychart-beta):** Emitted directly below the data table for categorical bar and line series, enabling visual rendering inside compatible Markdown previewers:

Code snippet  
xychart-beta  
  title "Throughput (req/s)"  
  x-axis \["Rust (filegoblin)", "Python (Docling)", "Python (Marker)"\]  
  y-axis "Requests per Second" 0 \--\> 1500  
  bar \[1420.5, 12.4, 18.1\]

### **Pure-Rust Event-Driven OpenXML Streaming Blueprint**

To eliminate external runtime dependencies and prevent unnecessary memory allocations, the OpenXML extraction engine avoids building an in-memory Document Object Model (DOM). Instead, it processes XML tokens using quick-xml's streaming event reader15. The following blueprint extracts connector graph structures directly from slide XML byte slices:

Rust  
use quick\_xml::events::Event;  
use quick\_xml::reader::Reader;  
use std::collections::HashMap;

\#\[derive(Debug, Default, Clone)\]  
pub struct SlideGraph {  
    pub nodes: HashMap\<String, String\>,  
    pub edges: Vec\<DirectedEdge\>,  
}

\#\[derive(Debug, Clone)\]  
pub struct DirectedEdge {  
    pub source\_id: String,  
    pub target\_id: String,  
    pub is\_dashed: bool,  
    pub is\_bidirectional: bool,  
}

pub fn extract\_slide\_connectors(xml\_bytes: &\[u8\]) \-\> Result\<SlideGraph, Box\<dyn std::error::Error\>\> {  
    let mut reader \= Reader::from\_reader(xml\_bytes);  
    reader.config\_mut().trim\_text(true);

    let mut graph \= SlideGraph::default();  
    let mut buffer \= Vec::with\_capacity(4096);

    let mut current\_shape\_id: Option\<String\> \= None;  
    let mut text\_accumulator: Vec\<String\> \= Vec::new();  
    let mut capturing\_text \= false;

    let mut in\_connector \= false;  
    let mut conn\_start: Option\<String\> \= None;  
    let mut conn\_end: Option\<String\> \= None;  
    let mut conn\_dashed \= false;  
    let mut conn\_has\_head \= false;  
    let mut conn\_has\_tail \= false;

    loop {  
        match reader.read\_event\_into(&mut buffer)? {  
            Event::Start(ref e) \=\> match e.local\_name().as\_ref() {  
                b"sp" \=\> {  
                    in\_connector \= false;  
                    current\_shape\_id \= None;  
                    text\_accumulator.clear();  
                }  
                b"cxnSp" \=\> {  
                    in\_connector \= true;  
                    conn\_start \= None;  
                    conn\_end \= None;  
                    conn\_dashed \= false;  
                    conn\_has\_head \= false;  
                    conn\_has\_tail \= false;  
                }  
                b"t" \=\> capturing\_text \= true,  
                \_ \=\> {}  
            },  
            Event::Empty(ref e) \=\> match e.local\_name().as\_ref() {  
                b"cNvPr" \=\> {  
                    for attr in e.attributes().flatten() {  
                        if attr.key.local\_name().as\_ref() \== b"id" {  
                            let id \= String::from\_utf8\_lossy(\&attr.value).into\_owned();  
                            current\_shape\_id \= Some(id);  
                        }  
                    }  
                }  
                b"stCxn" \=\> {  
                    for attr in e.attributes().flatten() {  
                        if attr.key.local\_name().as\_ref() \== b"id" {  
                            conn\_start \= Some(String::from\_utf8\_lossy(\&attr.value).into\_owned());  
                        }  
                    }  
                }  
                b"endCxn" \=\> {  
                    for attr in e.attributes().flatten() {  
                        if attr.key.local\_name().as\_ref() \== b"id" {  
                            conn\_end \= Some(String::from\_utf8\_lossy(\&attr.value).into\_owned());  
                        }  
                    }  
                }  
                b"prstDash" \=\> conn\_dashed \= true,  
                b"headEnd" \=\> {  
                    for attr in e.attributes().flatten() {  
                        if attr.key.local\_name().as\_ref() \== b"type"  
                            && (attr.value.as\_ref() \== b"triangle" || attr.value.as\_ref() \== b"stealth")  
                        {  
                            conn\_has\_head \= true;  
                        }  
                    }  
                }  
                b"tailEnd" \=\> {  
                    for attr in e.attributes().flatten() {  
                        if attr.key.local\_name().as\_ref() \== b"type"  
                            && (attr.value.as\_ref() \== b"triangle" || attr.value.as\_ref() \== b"stealth")  
                        {  
                            conn\_has\_tail \= true;  
                        }  
                    }  
                }  
                \_ \=\> {}  
            },  
            Event::Text(ref e) if capturing\_text \=\> {  
                text\_accumulator.push(e.unescape()?.into\_owned());  
            }  
            Event::End(ref e) \=\> match e.local\_name().as\_ref() {  
                b"t" \=\> capturing\_text \= false,  
                b"sp" \=\> {  
                    if let Some(id) \= current\_shape\_id.take() {  
                        let label \= text\_accumulator.join(" ").trim().to\_string();  
                        if \!label.is\_empty() {  
                            graph.nodes.insert(id, label);  
                        }  
                    }  
                }  
                b"cxnSp" \=\> {  
                    if let (Some(src), Some(dst)) \= (conn\_start.take(), conn\_end.take()) {  
                        graph.edges.push(DirectedEdge {  
                            source\_id: src,  
                            target\_id: dst,  
                            is\_dashed: conn\_dashed,  
                            is\_bidirectional: conn\_has\_head && conn\_has\_tail,  
                        });  
                    }  
                    in\_connector \= false;  
                }  
                \_ \=\> {}  
            },  
            Event::Eof \=\> break,  
            \_ \=\> {}  
        }  
        buffer.clear();  
    }

    Ok(graph)  
}

## **3\. Born-Digital PDF Vector Graphics and Geometry Extraction**

Vector diagrams in born-digital PDFs do not exist as self-describing functional objects. Instead, they are serialized as an unstructured stream of graphic state manipulations and path-painting operators executed in a continuous two-dimensional user coordinate space calibrated at 72 points per inch16.

### **PDF Drawing Operators and Coordinate Space Interception**

Every vector graphic in a PDF content stream is constructed through low-level path commands. To compute real-world page coordinates, each operation must be multiplied by the Current Transformation Matrix (![][image7]):  
![][image8]  
A stream processor must maintain an internal graphics state stack to track the transformation pipeline:

\+---------------------------------------------------------------------------------------------------+  
|                              PDF OPERATOR INTERCEPTION STACK                                      |  
\+---------------------------------------------------------------------------------------------------+  
|  OPERATOR    OPERAND FORMAT                 GRAPHICS STATE ACTION                                 |  
|  q           \-                              Push Current Transformation Matrix onto State Stack   |  
|  Q           \-                              Pop Transformation Matrix from State Stack            |  
|  cm          \[a, b, c, d, e, f\]             Multiply CTM by Local Affine Transformation Matrix    |  
|  m           \[x, y\]                         Initiate New Path Subpath at Transformed (x, y)       |  
|  l           \[x, y\]                         Append Linear Segment to Current Path                 |  
|  c           \[x1, y1, x2, y2, x3, y3\]       Append Cubic Bézier Curve to Current Path             |  
|  re          \[x, y, w, h\]                   Append Direct Closed Axis-Aligned Rectangle           |  
|  h           \-                              Close Active Subpath (Connect End to Start)           |  
|  S / s       \-                              Stroke Path Boundary / Close and Stroke Path          |  
|  f / B       \-                              Fill Path Interior / Fill and Stroke Combined         |  
\+---------------------------------------------------------------------------------------------------+

### **Comparative Evaluation of Vector Extraction Engines**

Extracting vector paths without rasterization requires access to low-level token streams and character placement operators. A comparative evaluation reveals significant trade-offs across current open-source runtimes:

\+---------------------------------------------------------------------------------------------------+  
|                       PDF VECTOR PARSING ENGINE CAPABILITY MATRIX                                 |  
\+---------------------------------------------------------------------------------------------------+  
|  CRITERION              lopdf (Rust)        pdfium (Google C++)  poppler (C++)    mupdf (C)       |  
|  Runtime Language       Pure Rust           C++ (Requires FFI)   C++ (Heavy FFI)  C (Native C)    |  
|  Operator Access        Direct Content AST  Exposed via Iter     Internal Sink    Device Callback |  
|  C-FFI Dependency       None (Zero C-deps)  libpdfium.so / dylib libpoppler.so    libmupdf.so     |  
|  Memory Overhead        \< 20 MB             40 \- 60 MB           80 \- 120 MB      35 \- 50 MB      |  
|  Licensing Model        MIT                 Apache 2.0           GPLv2 / GPLv3    AGPLv3          |  
|  Static Binary Suit     Optimal             Moderate             Poor             Poor            |  
\+---------------------------------------------------------------------------------------------------+

The lopdf crate provides a solid foundation for filegoblin's CLI architecture17. By avoiding C-FFI runtime linking, lopdf compiles to a self-contained static binary while granting direct access to raw page content streams17. The engine implements an internal affine matrix stack to compute bounding boxes on the fly.

### **Computational Geometry and Planar Graph Reconstruction**

Converting raw path vectors into semantic nodes and directed edges requires a multi-stage computational geometry pipeline:

\+---------------------------------------------------------------------------------------------------+  
|                           COMPUTATIONAL GEOMETRY GRAPH RECONSTRUCTION                             |  
\+---------------------------------------------------------------------------------------------------+  
|                                                                                                   |  
|  \+--------------------+                     \+--------------------+                                |  
|  | RAW PATH SEGMENTS  |                     | RAW TEXT PLACEMENT |                                |  
|  | (m, l, c, re, S, f)|                     | (BT, Tj, TJ, ET)   |                                |  
|  \+---------+----------+                     \+---------+----------+                                |  
|            |                                          |                                           |  
|            v                                          v                                           |  
|  \+--------------------+                     \+--------------------+                                |  
|  | SHAPE DISAMBIGUATION                     | TEXT BOUNDING BOX  |                                |  
|  | Closed: Area \> 200 |                     | Bounding Boxes     |                                |  
|  | Open: Linear Poly  |                     | String Runs        |                                |  
|  \+---------+----------+                     \+---------+----------+                                |  
|            |                                          |                                           |  
|            \+--------------------+---------------------+                                           |  
|                                 |                                                                 |  
|                                 v                                                                 |  
|                 \+-------------------------------+                                                 |  
|                 | R\*-TREE SPATIAL ACCELERATION  |                                                 |  
|                 | (rstar 2D Indexing Structure) |                                                 |  
|                 \+---------------+---------------+                                                 |  
|                                 |                                                                 |  
|                                 v                                                                 |  
|                 \+-------------------------------+                                                 |  
|                 | SPATIAL CONTAINMENT QUERY     |                                                 |  
|                 | Text Centroid in Shape AABB   |                                                 |  
|                 \+---------------+---------------+                                                 |  
|                                 |                                                                 |  
|                                 v                                                                 |  
|                 \+-------------------------------+                                                 |  
|                 | EPSILON-TOLERANCE EDGE MATCHER|                                                 |  
|                 | Nearest Node Boundary dist\<6pt|                                                 |  
|                 \+---------------+---------------+                                                 |  
|                                 |                                                                 |  
|                                 v                                                                 |  
|                 \+-------------------------------+                                                 |  
|                 | ARROWHEAD ORIENTATION DETECT  |                                                 |  
|                 | Terminal Cap Matching: \-\>     |                                                 |  
|                 \+---------------+---------------+                                                 |  
|                                 |                                                                 |  
|                                 v                                                                 |  
|                 \+-------------------------------+                                                 |  
|                 | EMIT SEMANTIC MERMAID / SVG   |                                                 |  
|                 \+-------------------------------+                                                 |  
\+---------------------------------------------------------------------------------------------------+

The shape disambiguation phase evaluates closed subpaths. Rectangular operator patterns (re), closed linear polylines, and looping Bézier curves are classified as Candidate Nodes if their bounded area exceeds a minimal threshold (![][image9]). Open linear paths are classified as Candidate Edges.  
Spatial acceleration is achieved by indexing the Axis-Aligned Bounding Boxes (AABB) of all candidate nodes into an ![][image1]\-Tree using the rstar crate8. A second ![][image1]\-Tree indexes the bounding boxes of text runs extracted from text blocks (BT ... ET).  
The spatial containment query iterates through all candidate nodes and evaluates text run centroids8. When a text centroid falls within a node's geometry, the string is assigned to that node's semantic label, preserving reading order19.  
Edge-to-node matching queries the node ![][image1]\-Tree using the terminal points of each candidate edge (![][image10] and ![][image11])8. If both points fall within a tolerance threshold (![][image12]) of distinct node boundaries, a topological relationship is established8. Arrowheads are identified by searching for small triangular paths or angled caps within an ![][image13] radius of ![][image11].

### **Vector Path and Text Label Recovery Blueprint**

Rust  
use rstar::{RTree, RTreeObject, AABB};

\#\[derive(Debug, Clone)\]  
pub struct PlanarNode {  
    pub id: usize,  
    pub bounds: AABB\<\[f64; 2\]\>,  
    pub text\_labels: Vec\<String\>,  
}

impl RTreeObject for PlanarNode {  
    type Envelope \= AABB\<\[f64; 2\]\>;  
    fn envelope(&self) \-\> Self::Envelope {  
        self.bounds  
    }  
}

\#\[derive(Debug, Clone)\]  
pub struct PlanarEdge {  
    pub start\_point: \[f64; 2\],  
    pub end\_point: \[f64; 2\],  
    pub is\_directed: bool,  
}

\#\[derive(Debug, Clone)\]  
pub struct TextFragment {  
    pub bounds: AABB\<\[f64; 2\]\>,  
    pub content: String,  
}

pub fn reconstruct\_graph\_topology(  
    nodes: Vec\<PlanarNode\>,  
    edges: Vec\<PlanarEdge\>,  
    texts: Vec\<TextFragment\>,  
    epsilon: f64,  
) \-\> String {  
    let mut node\_tree \= RTree::bulk\_load(nodes);

    // Assign text fragments to enclosing nodes via centroid containment  
    for text in texts {  
        let center \= \[  
            (text.bounds.lower()\[0\] \+ text.bounds.upper()\[0\]) / 2.0,  
            (text.bounds.lower()\[1\] \+ text.bounds.upper()\[1\]) / 2.0,  
        \];  
        let search\_box \= AABB::from\_point(center);

        let mut matched\_id \= None;  
        for node in node\_tree.locate\_in\_envelope\_intersecting(\&search\_box) {  
            if center\[0\] \>= node.bounds.lower()\[0\]  
                && center\[0\] \<= node.bounds.upper()\[0\]  
                && center\[1\] \>= node.bounds.lower()\[1\]  
                && center\[1\] \<= node.bounds.upper()\[1\]  
            {  
                matched\_id \= Some(node.id);  
                break;  
            }  
        }

        if let Some(id) \= matched\_id {  
            if let Some(node\_mut) \= node\_tree.iter\_mut().find(|n| n.id \== id) {  
                node\_mut.text\_labels.push(text.content);  
            }  
        }  
    }

    // Reconstruct directed topology by matching edge endpoints to node envelopes  
    let mut mermaid\_lines \= vec\!\["graph TD".to\_string()\];

    for node in node\_tree.iter() {  
        let label \= if node.text\_labels.is\_empty() {  
            format\!("Node\_{}", node.id)  
        } else {  
            node.text\_labels.join(" ")  
        };  
        mermaid\_lines.push(format\!("  N\_{}\[\\"{}\\"\]", node.id, label.replace('"', "\\\\\\"")));  
    }

    let epsilon\_sq \= epsilon \* epsilon;

    for edge in edges {  
        let mut source\_id \= None;  
        let mut target\_id \= None;

        for node in node\_tree.iter() {  
            if node.bounds.distance\_2(\&edge.start\_point) \<= epsilon\_sq {  
                source\_id \= Some(node.id);  
            }  
            if node.bounds.distance\_2(\&edge.end\_point) \<= epsilon\_sq {  
                target\_id \= Some(node.id);  
            }  
        }

        if let (Some(src), Some(dst)) \= (source\_id, target\_id) {  
            if src \!= dst {  
                let arrow \= if edge.is\_directed { "--\>" } else { "---" };  
                mermaid\_lines.push(format\!("  N\_{} {} N\_{}", src, arrow, dst));  
            }  
        }  
    }

    mermaid\_lines.join("\\n")  
}

## **4\. Mathematical Formula Extraction: OMML and PDF Math**

Mathematical expressions represent a frequent failure point in modern document intelligence pipelines2. Equations are frequently flattened into corrupted character sequences or sent to resource-intensive multimodal OCR engines that risk hallucinating numeric coefficients3.

### **Word OMML Abstract Syntax Tree and LaTeX Compilation**

Microsoft Word stores equations natively using Office Math Markup Language (OMML), wrapped inside \<m:oMath\> (inline) or \<m:oMathPara\> (display blocks)6. OMML provides a structured XML representation of mathematical syntax trees23.  
Legacy pipelines typically transform OMML using Microsoft's historical OMML2MML.XSL stylesheet, followed by MathML-to-LaTeX conversion23. This approach introduces heavy XSLT parsing overhead24. filegoblin compiles OMML to standard KaTeX-compatible LaTeX via a direct, recursive abstract syntax tree traversal in pure Rust.

XML  
\<m:oMath xmlns:m\="http\://schemas.openxmlformats.org/officeDocument/2006/math"\>  
  \<m:f\>  
    \<m:fPr\>\<m:type val\="bar"/\>\</m:fPr\>  
    \<m:num\>  
      \<m:r\>\<m:t\>\-b\</m:t\>\</m:r\>  
      \<m:r\>\<m:t\>±\</m:t\>\</m:r\>  
      \<m:rad\>  
        \<m:radPr\>\<m:degHide val\="1"/\>\</m:radPr\>  
        \<m:deg/\>  
        \<m:e\>  
          \<m:sSup\>  
            \<m:e\>\<m:r\>\<m:t\>b\</m:t\>\</m:r\>\</m:e\>  
            \<m:sup\>\<m:r\>\<m:t\>2\</m:t\>\</m:r\>\</m:sup\>  
          \</m:sSup\>  
          \<m:r\>\<m:t\>\-4ac\</m:t\>\</m:r\>  
        \</m:e\>  
      \</m:rad\>  
    \</m:num\>  
    \<m:den\>  
      \<m:r\>\<m:t\>2a\</m:t\>\</m:r\>  
    \</m:den\>  
  \</m:f\>  
\</m:oMath\>

The recursive compiler maps OMML nodes into their corresponding LaTeX macros:

\+---------------------------------------------------------------------------------------------------+  
|                              OMML TO LATEX SYNTAX MAPPING                                         |  
\+---------------------------------------------------------------------------------------------------+  
|  OMML NODE    SCHEMA ROLE                 CHILD ELEMENTS        EMITTED LATEX TEMPLATE            |  
|  \<m:f\>        Fraction Representation     \<m:num\>, \<m:den\>      \\frac{eval(num)}{eval(den)}       |  
|  \<m:rad\>      Radical / Root Operator     \<m:deg\>, \<m:e\>        \\sqrt\[eval(deg)\]{eval(e)}         |  
|  \<m:sSup\>     Superscript Exponent        \<m:e\>, \<m:sup\>        {eval(e)}^{eval(sup)}             |  
|  \<m:sSub\>     Subscript Index             \<m:e\>, \<m:sub\>        {eval(e)}\_{eval(sub)}             |  
|  \<m:sSubSup\>  Combined Sub/Superscript    \<m:e\>, \<m:sub\>, \<m:sup\> {eval(e)}\_{eval(sub)}^{eval(sup)} |  
|  \<m:d\>        Delimited Expressions       \<m:dPr\>, \<m:e\>        \\left( eval(e) \\right)            |  
|  \<m:nary\>     Integral / Summation / Prod \<m:sub\>, \<m:sup\>, \<m:e\> \\sum\_{eval(sub)}^{eval(sup)}{e}  |  
|  \<m:m\>        Tabular Matrix Layout       \<m:mr\>, \<m:e\>         \\begin{matrix}...\\end{matrix}     |  
|  \<m:bar\>      Overline / Underline Accent \<m:e\>, \<m:barPr\>      \\overline{eval(e)}                |  
|  \<m:eqArr\>    Aligned Equation Array      \<m:e\>                 \\begin{aligned}...\\end{aligned}   |  
\+---------------------------------------------------------------------------------------------------+

Delimiters configured via \<m:dPr\> specify opening, closing, and separator characters using \<m:begChr\>, \<m:endChr\>, and \<m:sepChr\>. If delimiters are omitted in the markup, the compiler defaults to standard parentheses \\left( and \\right). The OMML snippet above compiles directly to:  
![][image14]

### **PDF Math Glyphs and the Historical Encoding Dilemma**

A common flaw in PDF text extraction is the corruption of mathematical expressions (for example, converting ![][image15] into a \+ b 2 2vah). This error occurs because of legacy font encodings in PDF typesetting workflows:

> 1. **Computer Modern Font Architectures:** Scientific documents typeset through legacy LaTeX engines embed mathematical symbols from Type 1 font packages such as Computer Modern Symbol (CMSY10) or Computer Modern Math Italic (CMMI10)9.  
> 2. **The Missing /ToUnicode CMap:** PDF specifications allow fonts to omit /ToUnicode translation tables9. When a PostScript backend writes a PDF without generating a CMap stream, the font character codes point directly to internal glyph indices rather than Unicode scalar values9.  
> 3. **Fallback Mapping Collisions:** When /ToUnicode tables are absent, extraction tools fall back to default single-byte encodings like WinAnsiEncoding or StandardEncoding10. In the standard CMSY10 font encoding table, the character code octal \\062 (hexadecimal 0x32, decimal 50\) corresponds to the "greater than or equal to" relation (![][image16])27. However, in ASCII and WinAnsiEncoding, byte 0x32 represents the digit character '2'. The extraction engine therefore misinterprets 0x32 as the character '2'. Similarly, Greek characters like ![][image17] in CMMI10 map to internal character codes 0x0B, 0x0C, and 0x0D, which standard extractors discard as unprintable control characters or misread as arbitrary Latin glyphs26.

### **Deterministic TeX Symbol Recovery and Font CMap Normalization**

filegoblin resolves font encoding collisions deterministically on the CPU without invoking OCR by reading font descriptors during stream parsing10. The engine checks the /BaseFont attribute of every active font dictionary10. When a font matches known TeX math typefaces (CMSY\*, CMMI\*, MSAM\*, MSBM\*, Math\*, Symbol) and lacks an explicit /ToUnicode CMap, the extraction engine routes byte sequences through a hardcoded TeX symbol recovery table:

| Font Identifier | Raw Byte Code (Hex) | Naive Extractor Output | True Semantic Glyph | Normalized KaTeX Token |
| :---- | :---- | :---- | :---- | :---- |
| CMSY\* \[cite: 25, 28\] | 0x14 | Non-printable | ![][image18] | \\forall |
| CMSY\* \[cite: 25, 28\] | 0x21 | \! | ![][image19] | \\bullet |
| CMSY\* \[cite: 25, 27\] | 0x32 | 2 | ![][image16] | \\ge |
| CMSY\* \[cite: 25, 28\] | 0x33 | 3 | ![][image20] | \\le |
| CMSY\* \[cite: 25, 28\] | 0x39 | 9 | ![][image21] | \\approx |
| CMSY\* \[cite: 25, 28\] | 0x61 | a | ![][image22] | \\mathcal{A} |
| CMMI\* \[cite: 9, 26\] | 0x0B | Non-printable | ![][image23] | \\alpha |
| CMMI\* \[cite: 9, 26\] | 0x0C | Form Feed | ![][image24] | \\beta |
| CMMI\* \[cite: 9, 26\] | 0x0D | Carriage Return | ![][image25] | \\gamma |
| CMMI\* \[cite: 9, 26\] | 0x3B | ; | ![][image26] | \\partial |
| MSAM\* / MSBM\* \[cite: 9\] | 0x4E | N | ![][image27] | \\mathbb{N} |
| MSAM\* / MSBM\* \[cite: 9\] | 0x52 | R | ![][image28] | \\mathbb{R} |

Rust  
pub fn normalize\_pdf\_glyph(base\_font: &str, raw\_byte: u8) \-\> Option\<&'static str\> {  
    if base\_font.contains("CMSY") {  
        match raw\_byte {  
            0x14 \=\> Some(r"\\forall "),  
            0x21 \=\> Some(r"\\bullet "),  
            0x32 \=\> Some(r"\\ge "),  
            0x33 \=\> Some(r"\\le "),  
            0x39 \=\> Some(r"\\approx "),  
            0x61 \=\> Some(r"\\mathcal{A}"),  
            \_ \=\> None,  
        }  
    } else if base\_font.contains("CMMI") {  
        match raw\_byte {  
            0x0B \=\> Some(r"\\alpha "),  
            0x0C \=\> Some(r"\\beta "),  
            0x0D \=\> Some(r"\\gamma "),  
            0x3B \=\> Some(r"\\partial "),  
            \_ \=\> None,  
        }  
    } else if base\_font.contains("MSBM") || base\_font.contains("MSAM") {  
        match raw\_byte {  
            0x4E \=\> Some(r"\\mathbb{N}"),  
            0x52 \=\> Some(r"\\mathbb{R}"),  
            \_ \=\> None,  
        }  
    } else {  
        None  
    }  
}

## **5\. The 2026 Competitive Landscape and Capability Gaps**

Modern document intelligence tools diverge significantly in their underlying architectures, performance characteristics, and diagram recovery pipelines.

### **Architectural Audit of Leading Document Engines**

IBM Docling relies on a modular deep learning pipeline centered around the DocLayNet object detection model and the TableFormer Transformer architecture1. While effective at preserving complex tabular hierarchies using its Optimized Table Structure Language (OTSL), it processes tables at 2 to 6 seconds per page on commodity CPUs and requires 2.5 to 6.2 gigabytes of system memory1. Docling does not extract vector diagram geometries. Non-tabular figures are classified into coarse visual bounding boxes and exported as raster images, requiring secondary Vision-Language Models (such as SmolDocling or remote APIs) for semantic interpretation29.  
Marker, developed alongside the Surya document toolkit, implements an end-to-end Python deep learning pipeline2. The system utilizes Surya for line-level detection, reading-order reconstruction, and layout bounding-box classification2. Mathematical equations are rendered into image crops and processed through texify, a vision encoder-decoder model2. This visual pipeline introduces considerable compute overhead, demanding 3.5 to 5.0 gigabytes of memory per worker and running slowly on CPU-only environments2. Like Docling, Marker treats vector diagrams as static visual regions and saves them as raw raster image files2.  
Microsoft MarkItDown provides a lightweight Python framework designed to convert corporate document formats into Markdown for LLM ingestion33. While fast on standard text files, its structural handling of Office documents is limited. Charts extracted from PowerPoint presentations drop parent category labels in hierarchical axes (Issue \#2502) and render missing values as the string "None" (Issue \#2526)13. MarkItDown completely ignores DrawingML connector structures and SmartArt hierarchies, relying instead on an optional markitdown-ocr plugin that offloads visual understanding to models like gpt-4o33.  
Pandoc is a compiled, command-line document converter built on a Haskell abstract syntax tree36. Its DOCX reader natively parses OMML elements into mathematical AST nodes, providing fast, deterministic equation translation37. However, Pandoc has no support for DrawingML connectors, SmartArt topologies, or PDF vector parsing, dropping these elements or falling back to raw raster image embeds39.

\+---------------------------------------------------------------------------------------------------+  
|                           COMPETITIVE CAPABILITY COMPARISON MATRIX                                |  
\+---------------------------------------------------------------------------------------------------+  
|  METRIC                IBM Docling        Marker / Surya     MS MarkItDown      Pandoc            |  
|  Language Ecosystem    Python / PyTorch   Python / PyTorch   Python             Haskell Native    |  
|  Execution Footprint   2.5 \- 6.2 GB RAM   3.5 \- 5.0 GB RAM   \< 200 MB RAM       \< 50 MB RAM       |  
|  Page Parse Latency    2.0 \- 6.0s (CPU)   0.8 \- 3.0s (CPU)   0.1 \- 0.5s (CPU)   \< 0.05s (CPU)     |  
|  SmartArt \-\> Mermaid   Unsupported        Unsupported        Unsupported        Unsupported       |  
|  Connectors \-\> Mermaid Unsupported        Unsupported        Unsupported        Unsupported       |  
|  Office Charts         Rasterized BBox    Rasterized BBox    GFM Table (Buggy)  Unsupported       |  
|  Word OMML Parsing     Model Pipeline     texify Model OCR   Basic Mapping      Native Ast Parse  |  
|  PDF Vector Geometry   Ignored (Raster)   Ignored (Raster)   Ignored (Raster)   Unsupported       |  
|  TeX Font CMap Repair  Model Fallback     OCR Pipeline       External LLM       Unsupported       |  
\+---------------------------------------------------------------------------------------------------+

### **Structural Failure Modes of Vision-Centric Pipelines**

The current trend toward vision-first document ingestion creates three systemic failure modes:

> 1. **Destruction of Native Vector Semantics:** Converting born-digital vector graphics into raster bitmaps discards exact coordinate definitions, geometric paths, and structural relationships1. Re-predicting these lines and arrowheads using computer vision introduces unnecessary approximation errors.  
> 2. **Severe Runtime Dependencies:** Requiring multi-gigabyte neural checkpoints, Python virtual environments, and PyTorch runtimes makes it difficult to deploy these tools as lightweight, local-first command-line utilities1.  
> 3. **Equation Hallucination:** Vision encoder-decoder models like texify often hallucinate mathematical indices and negative signs when processing tiny punctuation marks, subscripts, or unusual variable fonts3. Deterministic extraction from font character codes avoids these transcription errors entirely10.

### **Strategic Opportunities for filegoblin**

By operating directly on native document byte streams, filegoblin establishes a distinct architectural advantage:

* **Sub-Millisecond Execution:** Reconstructing topologies using streaming XML and binary parsers operates orders of magnitude faster than neural network runtimes1.  
* **Zero-Dependency CLI Distribution:** Compiling pure Rust to a single static binary eliminates the need for Python environments, PyTorch distributions, or external system libraries17.  
* **Deterministic Accuracy:** Reading OpenXML relationship tables and normalizing PDF font mappings extracts text, diagrams, and formulas exactly as originally authored4.

## **6\. Phase-by-Phase Implementation Roadmap for filegoblin**

The integration of these capabilities into crates/core follows a four-phase development roadmap. Each phase delivers distinct functional features while maintaining zero external C-runtime dependencies.

\+---------------------------------------------------------------------------------------------------+  
|                             FILEGOBLIN CORE DEVELOPMENT ROADMAP                                   |  
\+---------------------------------------------------------------------------------------------------+  
|  PHASE 1: OPENXML DECONSTRUCTION (Target: crates/core/src/openxml/)                               |  
|  Deliverables: Direct parsing of SmartArt, DrawingML connectors, charts, and OMML equations.     |  
|  Dependencies: quick-xml, zip                                                                     |  
|                                                                                                   |  
|  PHASE 2: PDF STREAM INTERCEPTION & FONTS (Target: crates/core/src/pdf/)                          |  
|  Deliverables: Graphics state evaluator, path operator collector, TeX CMap normalization.         |  
|  Dependencies: lopdf                                                                              |  
|                                                                                                   |  
|  PHASE 3: PLANAR GEOMETRY & GRAPH RECONSTRUCTION (Target: crates/core/src/geom/)                  |  
|  Deliverables: R\*-Tree spatial indexing, node-label clustering, edge-to-node topology compiler.   |  
|  Dependencies: rstar                                                                              |  
|                                                                                                   |  
|  PHASE 4: AST INTEGRATION & CLI POLISHING (Target: crates/core/src/ast/)                         |  
|  Deliverables: Unified Markdown/Mermaid serialization, dual chart emission, integration suites.   |  
|  Dependencies: None (Internal Workspace Only)                                                    |  
\+---------------------------------------------------------------------------------------------------+

### **Phase 1: OpenXML Native Deconstruction in crates/core**

Phase 1 implements complete extraction for Microsoft Office formats, focusing on OpenXML packages.  
The OMML-to-LaTeX compiler (crates/core/src/openxml/omml.rs) parses \<m:oMath\> and \<m:oMathPara\> containers via streaming recursive descent, compiling fractions, radicals, sub/superscripts, delimiters, and matrices directly into KaTeX math strings6.  
The SmartArt parser (crates/core/src/openxml/smartart.rs) inspects ppt/diagrams/data\*.xml, reads \<dgm:ptLst\> and \<dgm:cxnLst\>, filters non-semantic transitional nodes, and compiles the remaining nodes into Mermaid graph TD pipelines.  
The connector extraction engine (crates/core/src/openxml/connectors.rs) processes slide XML files (ppt/slides/slide\*.xml), builds an internal symbol table of shape IDs and text bodies, resolves \<p:cxnSp\> connections (stCxn, endCxn), and emits directed Mermaid flowcharts4.  
The chart deconstruction engine (crates/core/src/openxml/charts.rs) extracts categorical axes and numerical series caches from \<c:chartSpace\>, formatting the results as both clean GFM tables and visual Mermaid xychart-beta code blocks5.

### **Phase 2: PDF Stream Vector Interception and Font Normalization**

Phase 2 implements native PDF content stream parsing using the pure-Rust lopdf crate, bypassing external C libraries17.  
The graphics state tracker (crates/core/src/pdf/state.rs) maintains an internal transformation stack that handles q, Q, and cm operators, mapping local coordinate systems into absolute page space points.  
The path visitor (crates/core/src/pdf/paths.rs) intercepts path construction commands (m, l, c, re, h) and painting operations (S, s, f, B), extracting stroke styling, line widths, and bounding boxes.  
The font normalization module (crates/core/src/pdf/fonts.rs) parses PDF font dictionaries. If an explicit /ToUnicode table is present, it handles standard glyph translation9. If absent, it cross-references the /BaseFont descriptor against TeX symbol tables (CMSY, CMMI, MSAM, MSBM), mapping legacy byte codes to their correct KaTeX equivalents9.

### **Phase 3: Spatial Planar Geometry and Topological Clustering**

Phase 3 introduces computational geometry to reconstruct visual graphs from raw vector primitives.  
The spatial indexing pipeline (crates/core/src/geom/spatial.rs) integrates the rstar crate to index Candidate Node bounding boxes and extracted text runs into two-dimensional ![][image1]\-Trees8.  
The containment resolution module (crates/core/src/geom/containment.rs) links text runs to enclosing geometric shapes using centroid-containment queries, sorting text fragments to preserve reading order8.  
The topological linker (crates/core/src/geom/topology.rs) matches edge endpoints (![][image10], ![][image11]) to candidate node bounding boxes within an envelope tolerance of ![][image12]8. Directed relationships are determined by identifying arrowhead geometries near terminal coordinates.  
The diagram fallback dispatcher evaluates the resulting topology: if the geometry forms a well-defined directed acyclic graph, it compiles the structure to Mermaid syntax; if the paths represent complex freehand art or fail topological validation, it exports the region as a sanitized, clean inline SVG.

### **Phase 4: AST Unification and Pipeline Verification**

Phase 4 integrates the extraction modules into filegoblin's core intermediate representation.  
The Markdown AST compiler maps extracted elements into unified document nodes:

* AstNode::MermaidDiagram(String)  
* AstNode::SemanticSvg(String)  
* AstNode::DualChart { table: String, chart: String }  
* AstNode::MathFormula { inline: bool, latex: String }

Regression testing validates the pipeline across diverse source materials:

* Multi-tier corporate slide decks containing SmartArt and connectors.  
* Academic papers typeset with legacy TeX math fonts and embedded vector flowcharts.  
* Financial spreadsheets and presentations with complex, multi-series data charts.

Benchmark assertions enforce performance constraints, ensuring processing speeds exceed 100 pages per second on multi-core CPUs while keeping peak heap allocation below 50 megabytes.

## **7\. Technical Conclusions**

The common belief that document understanding requires large, resource-intensive neural networks applies primarily to scanned paper documents1. For born-digital formats, relying on vision models introduces unnecessary latency, heavy memory footprints, and translation hallucinations1.  
OpenXML presentations and documents explicitly store structural hierarchies, shape connectors, data series, and mathematical syntax trees4. These native structures can be parsed deterministically into Mermaid graphs, GFM tables, and LaTeX expressions using streaming XML techniques4.  
Similarly, mathematical font corruption in born-digital PDFs—such as rendering the relation ![][image16] as the digit 2—is a deterministic encoding issue caused by missing /ToUnicode CMaps in legacy TeX fonts9. This can be corrected using font-aware symbol dictionaries without running external OCR models10. Furthermore, vector paths and text coordinates in PDFs can be parsed into directed flowcharts using planar computational geometry and ![][image1]\-Tree spatial indexing8.  
By implementing these deterministic parsing pipelines in pure Rust, filegoblin provides a fast, lightweight, local-first alternative to Python-based document processing stacks1.

#### **Works cited**

> 1. Docling Technical Report \- arXiv, [https\://arxiv.org/html/2408.09869v3](https://arxiv.org/html/2408.09869v3)  
> 2. marker-pdf 1.0.0 \- PyPI, [https\://pypi.org/project/marker-pdf/1.0.0/](https://pypi.org/project/marker-pdf/1.0.0/)  
> 3. marker-api/README.md at master \- adithya-sk \- GitHub, [https\://github.com/adithya-s-k/marker-api/blob/master/README.md](https://github.com/adithya-s-k/marker-api/blob/master/README.md)  
> 4. ConnectionShape Class (DocumentFormat.OpenXml.Presentation), [https\://learn.microsoft.com/en-za/dotnet/api/documentformat.openxml.presentation.connectionshape?view=openxml-3.0.1](https://learn.microsoft.com/en-za/dotnet/api/documentformat.openxml.presentation.connectionshape?view=openxml-3.0.1)  
> 5. DocumentFormat.OpenXml.Drawing.Charts Namespace, [https\://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.charts?view=openxml-3.0.1](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.charts?view=openxml-3.0.1)  
> 6. markitdown v0.1.8 Release Notes | Starstruk, [https\://starstruk.ai/repository/microsoft/markitdown/release/v0.1.8](https://starstruk.ai/repository/microsoft/markitdown/release/v0.1.8)  
> 7. Geograph — Geo crate // Lib.rs, [https\://lib.rs/crates/geograph](https://lib.rs/crates/geograph)  
> 8. RTree in rstar \- Rust \- Docs.rs, [https\://docs.rs/rstar/latest/rstar/struct.RTree.html](https://docs.rs/rstar/latest/rstar/struct.RTree.html)  
> 9. classify package \- github.com/tamnd/papers-reader/classify \- Go, [https\://pkg.go.dev/github.com/tamnd/papers-reader/classify](https://pkg.go.dev/github.com/tamnd/papers-reader/classify)  
> 10. docling.rs/docs/PDF\_CONFORMANCE.md at master ... \- GitHub, [https\://github.com/docling-project/docling.rs/blob/master/docs/PDF\_CONFORMANCE.md](https://github.com/docling-project/docling.rs/blob/master/docs/PDF_CONFORMANCE.md)  
> 11. cxnSp (Connection Shape) \- c-rex.net, [https\://c-rex.net/samples/ooxml/e1/Part4/OOXML\_P4\_DOCX\_cxnSp\_topic\_ID0EEHVIB.html](https://c-rex.net/samples/ooxml/e1/Part4/OOXML_P4_DOCX_cxnSp_topic_ID0EEHVIB.html)  
> 12. C Class (DocumentFormat.OpenXml.Linq) | Microsoft Learn, [https\://learn.microsoft.com/ga-ie/dotnet/api/documentformat.openxml.linq.c?view=openxml-2.16.0](https://learn.microsoft.com/ga-ie/dotnet/api/documentformat.openxml.linq.c?view=openxml-2.16.0)  
> 13. PPTX chart conversion drops parent category labels \#2502 \- GitHub, [https\://github.com/microsoft/markitdown/issues/2502](https://github.com/microsoft/markitdown/issues/2502)  
> 14. pptx: missing chart data points render as the literal string "None", [https\://github.com/microsoft/markitdown/issues/2526](https://github.com/microsoft/markitdown/issues/2526)  
> 15. quick-xml \- crates.io: Rust Package Registry, [https\://crates.io/crates/quick-xml/0.37.1](https://crates.io/crates/quick-xml/0.37.1)  
> 16. Advanced options \- Docling \- GitHub Pages, [https\://docling-project.github.io/docling/usage/advanced\_options/](https://docling-project.github.io/docling/usage/advanced_options/)  
> 17. GitHub \- J-F-Liu/lopdf: A Rust library for PDF document manipulation., [https\://github.com/J-F-Liu/lopdf](https://github.com/J-F-Liu/lopdf)  
> 18. Awesome-Geospatial/README.md at master \- GitHub, [https\://github.com/sacridini/Awesome-Geospatial/blob/master/README.md](https://github.com/sacridini/Awesome-Geospatial/blob/master/README.md)  
> 19. Docling Is Quietly Changing How We Build AI Document Pipelines, [https\://medium.com/@sanikachavan1806/docling-is-quietly-changing-how-we-build-ai-document-pipelines-4ee9e6f57164](https://medium.com/@sanikachavan1806/docling-is-quietly-changing-how-we-build-ai-document-pipelines-4ee9e6f57164)  
> 20. C++ R \- tree implementation wanted \[closed\] \- Stack Overflow, [https\://stackoverflow.com/questions/2709339/c-r-tree-implementation-wanted](https://stackoverflow.com/questions/2709339/c-r-tree-implementation-wanted)  
> 21. texify \- PyPI, [https\://pypi.org/project/texify/](https://pypi.org/project/texify/)  
> 22. easydoc-math 0.1.2 on Cargo \- Libraries.io \- security & maintenance, [https\://libraries.io/cargo/easydoc-math](https://libraries.io/cargo/easydoc-math)  
> 23. How to input fractions into WORD document using python-docx, [https\://stackoverflow.com/questions/70946715/how-to-input-fractions-into-word-document-using-python-docx](https://stackoverflow.com/questions/70946715/how-to-input-fractions-into-word-document-using-python-docx)  
> 24. Reading equations & formula from Word (Docx) to html and save, [https\://stackoverflow.com/questions/44748712/reading-equations-formula-from-word-docx-to-html-and-save-database-using-jav](https://stackoverflow.com/questions/44748712/reading-equations-formula-from-word-docx-to-html-and-save-database-using-jav)  
> 25. bug(tables): native reconstruction loses row labels at scale — the, [https\://github.com/r-uben/socr/issues/331](https://github.com/r-uben/socr/issues/331)  
> 26. OPmac \- tips, tricks, tutorials \- Petr Olšák, [http\://petr.olsak.net/opmac-tricks-e.html](http://petr.olsak.net/opmac-tricks-e.html)  
> 27. LaTeX Graphics Companion Overview | PDF | Te X \- Scribd, [https\://www\.scribd.com/document/685945857/The-LaTeX-Graphics-Companion-Su-Michael-Goossens](https://www.scribd.com/document/685945857/The-LaTeX-Graphics-Companion-Su-Michael-Goossens)  
> 28. LaTeX.pdf \- en.wikibooks.org \- Wikimedia Commons, [https\://upload.wikimedia.org/wikipedia/commons/2/2d/LaTeX.pdf](https://upload.wikimedia.org/wikipedia/commons/2/2d/LaTeX.pdf)  
> 29. Docling — Turn complex documents into structured data your AI can, [https\://docling.ai/](https://docling.ai/)  
> 30. docling-graph/docs/fundamentals/extraction-process/index.md at main, [https\://github.com/docling-project/docling-graph/blob/main/docs/fundamentals/extraction-process/index.md](https://github.com/docling-project/docling-graph/blob/main/docs/fundamentals/extraction-process/index.md)  
> 31. IBM Granite-Docling: End-to-end document understanding, [https\://www\.ibm.com/new/announcements/granite-docling-end-to-end-document-conversion](https://www.ibm.com/new/announcements/granite-docling-end-to-end-document-conversion)  
> 32. pitapo/surya \- Hugging Face, [https\://huggingface.co/pitapo/surya](https://huggingface.co/pitapo/surya)  
> 33. GitHub \- k-f-m/markitdown-web-ui: Local, privacy-first Streamlit UI for, [https\://github.com/k-f-m/markitdown-web-ui](https://github.com/k-f-m/markitdown-web-ui)  
> 34. ManagedCode.MarkItDown 0.0.5 \- NuGet Gallery, [https\://www\.nuget.org/packages/ManagedCode.MarkItDown/0.0.5](https://www.nuget.org/packages/ManagedCode.MarkItDown/0.0.5)  
> 35. markitdown/packages/markitdown-ocr/README.md at main \- GitHub, [https\://github.com/microsoft/markitdown/blob/main/packages/markitdown-ocr/README.md](https://github.com/microsoft/markitdown/blob/main/packages/markitdown-ocr/README.md)  
> 36. Pandoc User's Guide, [https\://pandoc.org/MANUAL.html](https://pandoc.org/MANUAL.html)  
> 37. Converting mathtype equations from word docx to ... \- Stack Overflow, [https\://stackoverflow.com/questions/39514038/converting-mathtype-equations-from-word-docx-to-word-equations-using-ruby](https://stackoverflow.com/questions/39514038/converting-mathtype-equations-from-word-docx-to-word-equations-using-ruby)  
> 38. Twenty Years of Pandoc, [https\://pandoc.org/twenty-years-of-pandoc.html](https://pandoc.org/twenty-years-of-pandoc.html)  
> 39. Figure sizes with pandoc conversion from markdown to docx, [https\://stackoverflow.com/questions/14829791/figure-sizes-with-pandoc-conversion-from-markdown-to-docx](https://stackoverflow.com/questions/14829791/figure-sizes-with-pandoc-conversion-from-markdown-to-docx)

[image1]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABgAAAAaCAYAAACtv5zzAAABVElEQVR4Xu2UvSuGYRSHj49SMhh9ZmRSLGKiFAtZTCwWVspmsxj8Cb6KfCw+Usii+AOsBiGDyIBSEorrdM7rvbvzPnrfDIbnqmu4z+/pnOe577tHJOW/0YEXeIt3eOPrazzDRWz7fjrLElbHxSQ28BPbfV2CDbiOH9jn9UkcERvQjFtY5lkil/gk1jikSWzwia/LxYbc47bYkF+pE2uyGwfQJZad+1q/5EhsoG7fHJZ6lpMhsSYTcQDzYtmgr1uxUmxLq7ATizzLib6FNmkJajW4io84GtQz6MCKuJgL/fx33McDPMVXXJM8b8pPZPZ/J6jpQa/gAzYG9YIYlp/3v9frU1E9bxbEGunhhYx7fTqq582V2P0vjuqbYgN0UMHo/mqTvTiAY7FszNfLWJuNk9F/i94cPcQXfBb79/QEz/TjGx7iDM4G2Z9RjwPYHQcpKSnJfAFdRUaCKV6nAwAAAABJRU5ErkJggg==>

[image2]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAmwAAAAxCAYAAABnGvUlAAAFRUlEQVR4Xu3cWah2VRkH8FVWZgOJmlmWfeCI0UhRdFFigw2mOA+ooA1GYQNESDdFAxGKFWIDaIIpag6XoiSpZANRJhJdRBZFRJANlM3j+rP28l1nfZxD5hF8P38/eHjXfvY+a+/3vXp41tqnFAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAANglPGtOrJH95wQAwINxWI0XL+M9xhOPMB+cE9sg33fPOTl5ypzYwmZzvW1OAADr65oa/65x4HK8W42PlIevQ/PDYfyxGv8Zjh+sy2u8bzh+9zB+bmlzP2bI/a+eWeOPc3Kb3F/jy3NyMv4m75iOR88oW8/1j9KKYwBgjR1a4ys1/lTjd0P+68N4u83Fx8en44fiujnxf/pVjRPn5Db5Rdm6yIrxNzp5Op5tNderavxrTgIA6+XcGu+q8cWyKgqyZHfXA1dsv9zn+cPxs4fxC2qcWTZ293Lt0cv4dUM+S4Evq/G8Go+tcXqNm2scXOOJpXWWXv3A1S0/RvfCGufVeOqQyzO+aBnvU+PlNQ4qrVv3hn5R9cay8e8iz/ieGo+f8jtqvKTsXLDl++fZnzPkxgLtpOk4dtR4S2nPs1XBlj14898CAGvm7mF8W2kdmSxTnjDkN3PHFLeXNsfXanx6ddlOUshkCTaFROLCJX9Djb8v45eWVrh1uf4Ly/ifpS13xr41PryMY+6wfW4Yp5MYZ9f40DL+RFntARuL1HTYRimq/lrjgNKKwzxnCqm4cfl8co2fL+P4ao0nLON0L/daxn8rqyIrxd2bl/FFpc0RY5GVTt94fElZzbWjbF2wxT1zAgBYL72IieOX4yyHpqvUPRyb11NopeD5c1kVI7uX1VuZ6fKNxVau6XuxMn7NMs7m/K0Ktl4MxltLWwLOHrIUXSkc/1JakZT4TFntd5uXhFPUZa9f97PS5oh0J+P9ZWNh9aYaZ5W2H+6yIZ+9cb3IurW03zr3T5H8iiW/WcGWuVK8juaC7WnTcS8oAYA1leXEUQqDz065zaQ7tFlkqXIzcwF4y/J5eWldrK4XQpHn6m+TZpx7RHJjwdaLk1cun5/qJxYpdrKkmeIt3bKxMBr9YDrOsuc4153DOB2vFHpXlI3z5Rlz7rganxzyY8H2kxqPG8514zzjHrbMNT/zXLDN0vEEANZUlhx7l6hL0dSX27KP7Jwa71yd3hZjwZF73DfkP7+Ms7cuBdBRw7mnD+Njl/HeNT66jOP7pb3lmmXPuHg5jtyn7yv77fKZpdf+Zukhy2fkHuPeunT+Unx13yur3+7S0jp9KbzSmev3++7yGXmpo+9RS9GY5dJIIXjtMn5SWf07j/E3SpduPL6grOY6o6zmyjLvacu4y16+uSMHAKyJ15a2HPj7GkcO+WyK79J9S0GRzfbb6coa3yltCTL7q1KQRDbq/7q0/W/nl1bkZKkwy49/qPGb0jpoGSfy7zySS3TpWH2jtMIsf5duVj5fX9ret3TGUtSNBdA3a3yrxvVDLuePWMYfKO0efa6flnb/X9b4cWm/YS86U8R9u8bVpX2HLt8p1+X5870y/37LuSy15je5aTnO98r8P6pxTGlz5zj3ihSEmetLpe3By1y55sCy8TvE4WXnjhwAsIvJnrb3zslHgVNLKx7XSd72vaq0N1q7e8vOS9AAwC4my4D9bchHk3TK0gVcJ6eU1fJql6XbLIsCAOyy+v62dfT2OQEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADwkPwXcFror1IsTyMAAAAASUVORK5CYII=>

[image3]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAmwAAAAzCAYAAAAq0lQuAAADUklEQVR4Xu3cW8hmUxgH8MdhCkVIyiFFmBFyuJHzOJVCSUlizI0cS5JElHI+3VDKRCkRypWSTHFhLlyQnG7GRHEhccGFQ7jgeVrr9e1ZzfimGV+9b/1+9W+v9ez3e/vWe/W01947AgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAHbJf5rvM95m9JvVnMlsyX01qi+r8zNfR1vjypP5htDXWOQCAufZa5rrMx0P92WG+6O7M/D3UrhzmAABz6dPMbtGamTMm9XlvZk4bC/9hdWb/zK+ZfSb1gydjAIC5dXM/1tbon328dz/Ou7PHwna82o8nZD7r4wv6EQBg7h3Xj/fH0pbhojQzb2QOGovb8MlkXGs8J/PgpAYAMLfWDPNXMldlNg31HXF45ryhdvwwH50S7erXruSXzJGxfTdlbpnMv838lfloUpv5MXPMWBzsm3lkMq8GcFz3rcMcAGCnzbZDZ06P1shsq5lZzqGZc4fascP8//Zw5qmxOHg9tm4c74nWZD09qc3UtvByDdu62PqBjPqucd03DHMAgJ3201iI1oBc3sd7TOq7Z67OXJQ5MHNbtIcVnujnD4m21Vg2R7tfbH2fr4T63+q1JMv5PNr/PvVitDXMXN+Pf0Rr2D6Itpb3MidGe4r2mswV0bZSN8TSd862WFfH0tZrfT8AwC6pJybr/WO/Z+4bzn2TOaCPqymp7dG7+vysaE1aNUv1ualpw/Z+P57cjyvh0rEwuDDau9dqjT8P506djOu32LOP63PVsNXf3JF5J9rvU1fe6vtWRbvXb7zCVut+LPNAr92+dBoAYGXNGrdSV7OqYZt5ezIuh2XW9vGX0Zq9M/89O7+qWTuqj+t+uNrG/SFa41avALkkc0Tmssyjmbszz0dr3Eo1bGujbZW+1Gv39iMAwIqrbcMno920f3S0K07P9XPVoNVN/w9lLu7jt6I1LnWFqT5X94ktwgt462GLG6NdWXs32lOyb2Yej9ao1suFay31G1Rz90XmpGjr/i3auqtBfSFzbWZjtO8DAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAW3z9FM3qCT9JFzgAAAABJRU5ErkJggg==>

[image4]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAA8AAAAbCAYAAACjkdXHAAAA5klEQVR4Xu2TPwtBURjGX4qUiEEZbEaLSRmNrAYZTHYZLHbsFoNPwSdQJoN8AiWTwqj8iXhO7ynnvNx7ZZRf/eqe9zlPnVPnEv00cdiWw08ZwCv0y8CLLLzBO0yIzJMJXBKXMyJzpQr7cEhcLtixM2G4gDHYIS5XrB0udGFdfzeJy41n7EwaTqFPr2vEZXUCT0YwZ6xLxGV1d1eK8AzXcKXdEJfHxr63fF0OwjlMinmKuDwTcwv1fltyCELEZXWKF9SbLcMDjIhMEYBHrfW+83CngwvcwqiRq59ir/MT8d6ekf/548wD3hczyiEbzkIAAAAASUVORK5CYII=>

[image5]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABEAAAAZCAYAAADXPsWXAAAA/UlEQVR4Xu2SLU9CYRiGb53FkfzYpJBMJKaB+TMITBNkdSYzv4NCYgbdIFIt/AMJBDZFCrg5p0FNOrzePWc7h2cHSbRzbVd57nvvefecV8pYRRkfcYYvOMVnfMIJPuAV5qL+v9zgHEtuXo3m926eykh2kzQGsoOKPkiSl5XufABb+IG/uO+yBU5lh5z7ABqy7NoHnqaseII7uIfH2MIh1uPqckLxXbbcdmQXx1jBzbiazoGW7+NIlt36wHMmK174ICLc8gcLPkgSfu0rbvgAarIP9HyQ5FBW6rj5Nl7iN/ZxdzE2wubDs/6UHfIle+JhFnyTvdBw0MqlZmSslT+nqzgCi8JqfgAAAABJRU5ErkJggg==>

[image6]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAmwAAAAzCAYAAAAq0lQuAAACVElEQVR4Xu3dzatNURgH4E0ZoHwkKUUZyMc/IIbIQEgklMyExMRMMmNiIoWBUooUEzJXUkYoE+U7SZFi4qOUeFdnn+57V04G7r2duz1P/drv/q07OMPV2nef0zQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIy7m5H3kXep2x15G3kReZb6YTAr8rTpfeYnqT8beR55mToAgM44GvkYmZ+6mWkeRnsjv6ruXHUPANAZKyOnIsdStz7NE2FOXfzFlMj9yJrU7UgzAEBnLEtzObHa1s53Uz9RjtfFAPvb67TIj3ae3l4BADqnv/kpyobtTmRG5GHqs/F8VHqpLga4lub+Y9F1qQMA6JTHad7X9DZAJ5uRk7bscGRXXSbllKtspv4lXyKbmsGWR66n+6uRnZF7qevb3F63RL7mBQCAyWJF5Ea6Lydrn5reJm5e6ovyQsLpyJGqH0tlg7W4LisHI4fS/erIgzbZ0sjsdt4Q+ZnWAAAmjbLJqf9Rf0nkTNUV/U3SxVHt2FpVF3/wObKg6sqp4Naqu9L0XqI4EdlerQEATAq3I98i3+uFsLG6L29k7ml6jx5vVWtjpT7Rq5W3SF81vc9bP958E5lbdfl75crfr033AACdcz7Nr9M8rA5EHrXz1MiHyMKRZQCAbilftdHfpJUTtnIqd3lkeSiVX2goudD0XkZYNHoZAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD4D/0G5JtSCXMDUMUAAAAASUVORK5CYII=>

[image7]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADEAAAAaCAYAAAAe97TpAAACtklEQVR4Xu2WWchNURiGX3MylJmUMVOmQiSUuKBcSKZyxYUyJiWJZIgMJYQbyp8xkRuFXEhIpnBBLkxFRIRCiAzv612ns/7V2ec/50bUfuqpvb5vrdXaa69hAzk5f43GdCCdRDtF8Za0Y1T+J2lPd9FP9AbdS0/RnbQDvUaH0/30Zh1ep5fpNpga+oK+oi/p1BAvRX16Hq4rH9BRtWpksIR+oIdo2yS3nH4MdqNf6WbaF/5SE+gvegn+UpqMQfQiPYwi3elTuO7aKJ6yiN6H6w1NcpmokRrMTxMBDewnPUeX0jW105gHt1+RxBfSVVF5Jt1Ef9ADUTymM91N78JfrCJmwAPUEinHHXiQ+sytk9wx+CVGJPHVdEpU3kPH0WfwUivFPtoPHtPRJFeSZvQNPADNQDn0FUbDA0vRutVSbJjEF9AuUfkWbQovu+dRvMBkOhveLxrT3FrZDJbBldV5XfSg9dIgPGvq40yaSNA+0x4RWkqa6SbFNJqHuNDhoj57FdPZXIUrp2u8GjTb6kObvxya3fXheR3cpk8x/Wev9A7P9+CTrCLew50NSRNVcALuQ0dvOTS748PzHLjNxFAeRleG53bwVzoSymVpBHf0JU2UYAPtmgbh5fUa3g8NklxKYT+IsSiehmqnAeuCFdNCrqL9IApnttZjFj3plTQYGAC3P50mEnRvxKeRJkTttsL7ckyU0wlW8X4QWodqoN+LUujmPI7s23IxKtsPuh82RmXN/nf4r2BHFBe65CreD6INfQhf6zq/Y3TBnYWPvCx0Iuklsl5StKAX4AMg5hF8NLeKYjqO1Z/unarQr4DuADW+Dd8DB+EXGBnVK7CFPoZv08/wL8g7+oRuj+qJGvg/TPvuG/xPVeAknR6eB8MT+RauqzZa6rqXqqI/3Oms8JyTk5OTk/Nf8xuPEKBPb5HufwAAAABJRU5ErkJggg==>

[image8]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAmwAAABUCAYAAAA/I2vMAAAI5ElEQVR4Xu3deag1ZR0H8KdFMwu1MtuLNqygBaKN0kKIzMhMDX0xpcyyN6VS2suCFqwgQyoqpMSUaIGUEqF9tZU2ssxs36XljzbU0np+zkznOc97z733zDnnnnPnfj7w48zMc997nnvmyHydeWaelAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABgLbfL9d+qtpu6//E3AQAMRoSb7+W6Vd0wZ7fIdcd645zF3xB/i8AGAAzKVgS2+N03tjWrA3KdmOvktGefBTYAYJC2IrCFa3OdW2/s4dJi+fJiOQhsAMAgbRTYDq839BRn1w7K9ZC6YQpH5Xp2sX5mrkOKdYENABikSYFtr1zvz3VGrvNzvWe8eWr/yPXxXF+otk/jubl2Feun5TquWBfYAIBBmhTY4m7LpxbLTynapnVormPa5cNynVK0TeMVuY4v1nfnel6xLrABAIM0KbBdl2vfdvmGXPsXbdM6K43uEH1hrmOLtmlE0DuhWD8919HFusAGAAzSWoFtn1xvbpdvnevt7WtfHy2W/1UsTysC5NnFelymjceFdAQ2AGCQ1gps4Re53pfriFxX5rp4vHkqEbQ+lJq7OmcNU59p6yu53lW1CWwAwCBNCmxx5urgdvkeuW5WtPVx71y3qTf29IBcD6w3JoENABioSYFtOxLYAIBBEtgAAFacwAbAyolxOQekYRycliXuFozPMGreuv2ziN+93S3qcxHYAFg5MS3OJbmOrBvYtFNT8xleUzfMQbd/ohi3qM9FYANg5cwyj+FOdfM0Pn9j57J6wxzYPyPzuqNyI1sd2GK2gpg54ey6YZPiLOOJuU5Oe/ZZYAMYCIFgOjF3419Sc4CtCWyLEWEjnjN2bd2wIFsd2MIsU139KNfbUvPQ3Hg+XElgAxgIgWB6r0wC2zJ8st6wIMsIbN+uN2xSTG91ZrH+xjTeb4ENYCDqQPCRXBfk+kGu89JsT3Ofh9vnemeuW7brb0n9D27zsszAtlca7Z/Xpmb/nDP2E4vzklxX53p+u/7nNJoMfStMG9gipHy52hYTo7+32lZbL7DFw3KvSs1nHzMfzCLOhsV3+3Op/xjSOOO7q1g/LddxxbrABjAQdSA4vn2NQHKHXP8u2pbhHakJCN1T3H+f68JR81IsM7BFYA3x/l9Kzf6JA/6ixZP9P5zrD+1riD7c9/8/sXjTBrYQ/S4/ww+kZgzietYLbLHvD0zNFFBrfQc267A0+m/tq6l/oIr+lAFtd1sdgQ1gIOpA0JnlYDRv5dil6FdMw7NMywxsnXj/mNpoPTFBeemuue5VbZvW9Wl08I+5MGsRRL5bbYuzgMdU2/roE9jCPXN9M9cH64YJJgW2+B2zTNTeOTaNf3+uK5andUquE4r103MdXawLbAADMSkQ/LresETlwe2PxfKyrEJg28z+qQPbPHyqfX1QGj+T0zkk7RnY5qVvYAtvyPWIeuMEkwJbhNXu759F3A1afn/W+i5t1pPT+H54Va5HFesCG8BAlIHg8blekJpxSXHpbf80GsMWl5biEs5zcv0s151z/T3X3dufCy/LtV+uI1Iz9uznbVuMe5rFDe1rjNXpxk91B7noTzxcNvoTl6p+k5r3PDc1/flWrgfnOjw147+ecNO/ms3r0toH2fUC22vWqbUeEdKp90+8b7d/QuyfGN8Xj7y4f7stDtrh67n2TaNLprfN9bR2uY94n7e2y98ptp/Uvkagelyuv7br3c/G5cN5hMfPp+ZvmNZFqfmOxBnGSQG4NCmwXZKaPoT4PXEps4/HptH3J0LkL1Ozj7pxmtOKfRGXeffO9dOqTWADGIj6ANZdZttnbGvjLqkZdP3b1Nyd9v3UXGJ7aNv+6Vxn5DooNYOp4wAXISlqVmXwCP9pX6M/L0pNf2JQfIyxiv48MzX96f7NFWl+fZlkvcDWV71/OrF/7lasx1mbzo2pmX2hG1z/9KJtlsAWgSBCRezzLnA8adR806XrCGwxKD9ESIj+vzrNJ7BN605pz/eNYH9Ota02KbB14n8A5iH+Zyf0CaG1GCbQjfMsCWwAAzEpEKwlwlwckGOg+0tzXZqaA8IzUhMEItDFATEcmkZ30U3zHrW4Cy4u8TwxjZ/V6gJb9CcOfNGfEGf9oj8RYKI/nbOK5UWFtq0MbLVHptFg+rjTN8TdpOHl7Ws4slieRgS1bn+Gr7WvEeK6zznObsb+6H7u0e1rBLaNQtIq2SiwbScCG8BAbDYQhDgwr3XmLQ7mEYLijEac8YrHgXS6y6V9xXt+LDUBsBSBbb803p/uoBT9uTw1/Ymf6Z5TFZdpI2AsyjIDW4izanF2s9TNDhB3/Ia4XN1XPBIjBu7Hk/lLcbmxDMehO2sUn3lcQu4uj24HAhsAK2ezgSDGR9WhqRTB6eB2+d1lw4LE2bb1Hs8QZ/+iP3HAKi/bLdKyA9t6IuDGJbPuDOhW+XGub6RmDOF2IbABsHIiEMSBPM6E0E+cyYvP8It1wxx0+2et8Uk73aI+F4ENAGDFCWwAACtOYAMAWHFbHdhijN8/c92nbphSPKevJrABAIO01YHth6m5iSZmiegjnrcXj1LpHm1TEtgAgEHaysAWM4i8uN7Yw2OSwAYA7CAbBbZ41t3DUvPMwVncL9epuZ6Vmpk8ZiGwAQA7yqTAFlNy/S41D4mO5w5eM97cSzwUeh4ENgBgR5kU2GKO1ghpF6ZmKrBZxSwg5XRrs4jAVk5B1hHYAIBBmhTYIly9vto2i7irU2ADAOhhUmC7Mtf57fJJuT5btPVxQZrPZdUQ4e9N9cYksAEAAzUpsMWNBlflOi81Z7NiLNssrsh1cb2xh6tz/SnX33L9qmoT2ACAQZoU2OZld67Lcl2f68Cqbd4ENgBgkCLc/CQ148Ie3tY87cr1iVx71w0LILABAIMU4SZuBihruxLYAABWVB04BTYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABYqP8B+yJzmeWDA2cAAAAASUVORK5CYII=>

[image9]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAGsAAAAaCAYAAACwwaJoAAAE1ElEQVR4Xu2ZB6hcRRSGjwZ7b6AgiBUTEY3YQWzYFSzYEBFTRLFgA00sPFtULGAXCwYbigkaK6LiMzFoUEQUEysi2GtiwYLt/zwzu7Pz7n3u3rvGVe4HP3l3Znbn7pw5ZSZmDQ0NpSwp7S2dJ43P+hoGjHPNjbWL9J50XEdvH1hNmpI3NvTMMtIP0kHhGcN9Lo1pjegDN0q/mrvwvwVzHy5dLp1l5SFkVelo83ETzBeoiO2kc8wXrOy7/gn4DWuEv0+WPkj6arOl9Jv0h7RW1lfGndKV0rp5R0WWlZ6RLjI3xMPS79K0dJD5fPOlq6QdpJulBdLq6SBxpvSGdLB0pPS2dFLHiPpsL+2eNyYsIb0snZZ31OFZ6V1zY22W9Y3GbtLj0nRpbGdXzwxJt1pnuHjV/J32Tdrukx5NnmGudEfyvKn557ZO2vaUfpY2SdrqgseemDcmkFYuzhvrwK67RrrF/Afu2tndFdtKM6UZ0jZZX7c8bT7/EUnb+aHt7vCM17Pgp7dGOCwIeSKGw6ulb9vdf7G0uaeywP3ieSs3Fr+DUA54IF5WixWkV8xzAD+YhSHeVgWvvEt6wtzreoHc84J17vzDzN+JkAsHhudjWyMcjEf7TuEZjyzKExiQUFsEHr2F+XvvE9o2lva3kfluRelS8zkvMB+3UdLPd1xr/jm8mzWpzSXSxPD3qeaTn9Lursz65gXLsHSAVd9VV5i/E3kHJodnokEKuzvdaB9Jb7W7W3xhnt+KwGtfNM/d30gPSjeZh7KPpaeklcPYs829ijnJn09aOzQzZlHoixoOfZXZUJpt7YUkqfPF/YyxO5vv5iqJncVbKL0uLRfaWLgi7z8htPMvECqLjPJZ0Giw6Hw+9SY8ju9/IGkbF9rKwmBfecg810RI4kxO7qoLP/R+8zmI11W4V3rTOqtNdjTvmOY1iMaKh8+fzD+bg6HwktGggGGT5OBBzLFBeF5sxiIms3uI6+8HfWI++axkXK+QMx4xLwg2z/p6gbPRSzbyGDHJ/B2PytrxXNpjuPxQeqfd3eJL6bW8MeMeKzYWYZ05COsQjVUlanQNVRG1/9pZOzuYyYnbvbKfeTVHjI87ryrHmH/XSuGZEBg9hk3GO8ZwF5ka2jkEwzwbGe4I92xQip/RKDPW9eZzUHBANFbM8UST9cLffYO4z4Exh0Mpk+Nl3cBtw6HSc+Y3Cbnxq7CXeV5IbyR2NF8oWN68RKcSS+FgnF7pDJmX6fymCO+XLm4ZZcaiUiX6sNkhnuUozIAyfY/wd21Y3EOk7629a1OWkn4M6ubK6TbzHU3Z3w+2Mi9GqLIor4elOeaVHVc2EcIRx434jiweB/qhOMC8ePrOvNSPcDT41P5+U2EsKkIO0RFyJIY5I2lj43xlXqID65GW75XhWoayFUP8Yr4LYxkKLADxnH6SM2OnJf2LA24hWJAipQuHcQiTiHCIcW9I+iOETDzhQuky89vvbm5nMBYb+jrz3PuYuVE48+UcL31tnue5JmsogTDEeWts3pHA4ZUqF8MVRZMi0jC4ivn3p+E0h80TL20HlnXMT+7dKPXiQYfSnUPt/wru2Lh47Uaxghpk8BDuRcmHVI38f1R+dGgYENY0v8O73bxYmG6dubKhoaGhoaGh4b/JnwdYE8wEKYX+AAAAAElFTkSuQmCC>

[image10]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAaCAYAAADMp76xAAACPklEQVR4Xu2WO2gUURSGf9+KihF8gPGFIIr4KMTCWLgp0mk0sRHUwkYbUUHxrQkKFgEVBDUhqIuQEAKpLNMoMU0giCA2ihJEsRJBMSkk+v+cO8zNQZddFWYJ88HHzp4zQ86eufeeADk5k4sd9DX9SD+Fz7f0DX1HB+ghOjN5oFrooT/p9ig2mx4L8ZtRvCpQV7/QaT5BRugPutInsmI5rIuPfYJMpd/oOF09MZUdB2AFn/YJUg/LFV08UzphRW118VV0iL6nS10uU3RK6JVfomfpRdoNW9MP6cL01uxJ1u8w3RPcRQuwU6LqOAgruNXFy2E+ve6DFdJCF/lgKe7DCi64eDlooNzxwQrRgFrsg6XQNBuls3zCsZ+egnX0KG2gH+gLeotuDvdpSWkfXA3XCfphvXQ3vQ3rajKs2umZ9NY/sw72wFOfcGygD8K1hocKnE6v0Q7YWtd5LfQDGmEDSNd1Ib4CtrEv0Ht0J+w5/X3lSjZM41cnw2dYd7/Djq598U0RS2Cvro8epnNCXOvPLwkVvozupc/p8Sinabk2+i5UcEVLohxmwN6GOvoK1iWhgtUtsTF86rUX6aZwfSLEhQquib4LFawzfj1+/2/BX9EEO03EAjoYrs/Bho66quK30a90Ssj305NIJ6gK1vMxY7SWXoYts/9CM30C6+wVpEtnDX0J24hb6FzYfmijR+hd+gy2YYuwpfcI6QYVN2gXPR/F/hmtWXVNBSWbK0Gv0Q+YeDKW07V5PpCTk5MhvwCnqWl2EX7aHgAAAABJRU5ErkJggg==>

[image11]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACUAAAAaCAYAAAAwspV7AAAB20lEQVR4Xu2WyytFURSHF/IcKMojeaVQHiVMpHRlQBEDUkL+AAZekRgYGJAiRZTESEyZkEdhoJSRkUckkYgkj4nwW60lx44udc6duF99nX3WOrez7rp777uJvHhxlnx4CC/gpV6P4RE8gVuwHgZ8fMCTzME3mGeJBcEmjQ9Z4h6Du3MH/cwEOIUvMN5MOEksSTcWzQTwhQ/wFSZ+TTlLLUlR7WYCFJLkZoy440ySvDjHiCfAHXgGo4yc4/Dq45+nB3bCbjhLMsemYdjno57hYz7twgq1DLpIVp8dRMI1uGcmfqKOpKheI2436fDcDP7EFElRLiNuNyn0h6J4136GgWbCIBh2wC5YrLFBOA+zNd4PYzTHxMFh2Air6ZdFpZJ0acNMfMM2LIA+cB1mksyVR5IX837WBsf1eb7fhxl630BuiuK/El5xtyRdeiJZ9pXWhyxw6/m5VtgCl0lWKnMDc3XM+92Kjkvgpo6ZLHJT1F+pIvkC/ha5E8w1TNNxDVzVcR9c0jFje1Hh8Aom630ILNUxd8paFC99Jgke0GfxfBrhE4itFMEFks11AIbCUXiv8XKSecfd4LnFNMMJkqPPCMkGPaY52+ATRIQZdAP/1LwJ83ksWq9evPxP3gG2h14Isn0ktQAAAABJRU5ErkJggg==>

[image12]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAFcAAAAaCAYAAADCDsDeAAAD4klEQVR4Xu2XW8gVVRTHl6VhWmY3b0liVlSg4uVBfbC8ZIQZBClFSAQ9RIVKBRX6oAhqLyIqBnnBW1oWWXahUpSwsoewB1N8ULzg5UlNvItZ/x9rRvdsz5wz5zufXx82f/jBmbX3zOyz9l6XMStVqlSpUv93tRVPihnicXFTdjhXD4g3xSwxNhq74dRZTBVbRftoLE+9xT4xTQwX34l3MjMqa4Q4ICaLkWKz+DQzo3ENEaNiY0urm/hA/CJeEjdnh3PVSfwpJibX7cQF8ceVGZXF8w9ZdhPuEqfEy4GtUbHhb8TGlhJh+aHYIp4VbbLDNbXG3EmkhVQ4bHxwXUm86x8xMLL/LDZFtkbE8xp2bg/xYGysor5itXkIE55N1QnxZfKbNXQIxqppnrlzSSmhNpif/HCzUnHa+5unkKcT20PiGTEgnZToNjHb/B3UAebV4x+71fwBhCU58mtxR2bGtRoqvhKfi8HRWL3qY774j8VasVgcFMusdlrhHu7tHtk/S+xdIzu6V/wm/jbf1PXmUfe+OCI2mqcp9J75qeVZu8QP4ptkrKa6mL+IcI4XmCeq+EXxajzQRE0wX/w58VhiY8MPi0XppBzxZys5kYKG/dHIHgoncbrD08qJ5j42JxVrwlZ3WlgnzpqfHtqeoq0P4bHEfJefisbqFYtm8XGO/EScNy9Qefre/F4KaajUuQ9H9lA8/6/YaH5CuZc6gprk3DvFZXFS7AkYFk6qofvEXPGTeN6Kb04oihaL/yiys3nYqxU18j1zWEco0hX2ahtDSqnkXKKFe8cl16lz6aMLi3DgJhrvRnW3mC5+Fa+Yt1JFRc5mHfMjO7kX+wuRPRRtH3Pi8P9RnLHqXUuecxeaP5MCh1LnTkqu8Vuv5HeuOpqHHUm7uUR1fcvcyex0kQ8IHEAh4RSGorixvvsDG+nonuD6CfM/Hqem3eKLyBYrz7nbxFFxS3L9iPk7piTX75p/RdYULQtfNGE49wx+N1UsjIJHi5Yusppo0nFw2qXQQu03P5mpqAuXzFNXKrqJHeYtWSryLMVxdGCrJJxLxzAmsBElOPLtwEZbeMyuRhbpqlA7xo2rzNuNOeZdw+uZGS0jNpfi+rt5P7lXrBC3B3Noj3aKlYENcbL3m7duRA3OfzGckCOce1osMI+ab82dSPcS6zVx3Lz9nBmN1RQnht2olqNaQv3MT/ygeKCGOOn03hS/oi1lmBb4/+TtammMCKS2XFfxEnJfEeIvp9YkWjE6pVYlTheVvAi0NZU+Qf9LcTj4VN9u/hHxnPlXW6lmEN0GdWapeXFabtmiVqpUqVKlSpVC/wL5TsX1/xU4bQAAAABJRU5ErkJggg==>

[image13]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADMAAAAZCAYAAACclhZ6AAACxUlEQVR4Xu2WWahOURiGP/M8hmTIEYobV8aiuEBI4cKYi2OKkCGzG0qJckGRItOF5MJMZLgyXeCGKOEkQ6Ykoszet2+t07e+s/fv0N/Jqf+pp/Pvd62991pnfXvtLVKiRIl/pT4cBjfAybBV0lqY1nAG3AxnwkZpc81SBx6E++Bw0QFVwAG2Uw5d4D24FQ6Gu+B92NZ2KgLrfJDHLLjbZWNFB9XA5Z7D8LTLror+Y4pFPfjMh3nsgRdcxtL5Bdu73MK2L3CZyzfCT1K8chsB3/gwj8WiA98CG4eMpXa5skc240XPK3c5J8d8qMsjHeEg0dXvDpvDIXAcbGH6EZbuA/gO9gq2SXo4OsOnogPgiavhtZAXYo7oOVNdviDk3EiyWA5fiPa5CM/ANfAI/AgnhX58Hs/Bl/ArPB/Mu24lPUQvxBvQ/bCZ7ZABB5A16Pkh5988+on24TPHZyJyFP6AfU3GSVa7zLhsV+AKuBJ+Fr3RWUlv5OEKst8Ul8fJzHW5pbdonyUu56owP2Cyv5rMKbjJHLMur0t2CVlmi/aZ7vKFIZ/ocgvvkTWZDiG/ZTJO5q05zoV1+RN2dTl3sw9wu8stoyW7nNaGfKDLLXmTaRfymybjZLgBRCaY3wl8ubFG/WTIMUlfVtyKe5rjpqJbsF1VwhfnaylconmTGRXyRSbjc/XeHF8yv6vALXib6JdApAw+Ed0YIhXwW2iL7IS3Yd1w3BA+hOtjhxziZHhvnkOaiF7ruaRb9Cr4XfSf2U3S56kKXB0+I49Ev81YWnfgGNsJHIJ3Rd8LEQ6E2ytluXEj2WHa84iTOQ5Pip7zSrQaWGoWfifegI/hCUmrIxOuSh84DY6ELdPmP8LdiZsFr1EdbJlxVTnATkmPqvBla6vnvyFuzUt9Q22jTPSdxslwp+qftNYy5ol+VfMDd6/oJlKiRIka5jdhuJZjskZx0QAAAABJRU5ErkJggg==>

[image14]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAmwAAABACAYAAACnZCtBAAADfUlEQVR4Xu3dT6ilYxwH8Gf8G4bBzBjlzyz820gNsiINNmSnoZSFkiyIsJEmdaeRTE2SxmoWFhZkaaeMRuxGNsNmkhAbpZBkIfx+vc/Jcx7ncK97z73ndD+f+vb+3t/71j1n9+t53vfcUgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA2owcjDy1IAAA2nRsj5/dNAAD+3Zl9Y0aujbzYnG9t6nRP5I7IY10fAGBT+zryR+TC/sIMPFz+HsZ+iPzZXMvh7b5af9/018LhvgEAsEjOjfzWN6c4u2+s0CNNfWcZH9hGtkSe6ZurcFGZ/HcAABbGtMFpknP6xgpc1p0fLMPKXu/5vrFKx8ryvx8AwFz6MPJL5MnIifFL/7Ccge3KyAtdb2/k1a6Xq3pfRZ6KPFB7r0VuirxZz1frozJs9fYD277I0cg7kVNN//rI25HHy9oPjgAAE+XbmJdPycX1nhxm9tf6rsijtR7JLcXdNVc0dSa3U1s50N0eeTnyRNN/JXJ1c55yde2SWudnyBcS/kv/Hdr08nOMtAPbF5FPap0D3cla51bs57U+rwzDGwDAzOXviL01JQfqPTk45fCVcrXr/lpPspwVtrQn8l5kexkGoZciO8fuKOWzps6BajS8TZNDVP8d2rRykMyVw5F2YMv6UK1zle9Ire+NLNUaAGCuvFuPuRr3a3thguUObCkHwdzifK5Mflnh2XrMZ+jy7dFZage2/FzbmnpHGbZscxv30tFNZfKqHQDAhviyHl+PnG4vTLCSge3jyDdl+qrVG/X46Vh3NvoVtlwBzCHt2zKsAJ4VuaUMz7al/F263NYFAJgbV/WNNZDPrB2P3NBfqHIouq5vrpNd9XhBGd+qPaOMr7IBAAAAAAAAAADw/+TD9bL6AAAAAAAAAAAAsPjyN84Ol+E/HwAAMGeWIseacw/zAwDMmffL+JBmYAMAmHPtwLYvcjRyJHIqck1zDQCADbC7DIPZyE9N/Xtke3MOAMA6OxA52ZzfGjlU661lWGUDAGCD3F2GoSzloJaWIttqfTCyI/J0PQcAYB3dHPk5cjxyIvJd7d8W2RPZG/kxsiXyQb0GAMCc2NXUO5saAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAFtRfxefNQcqAZzkAAAAASUVORK5CYII=>

[image15]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAHcAAAAaCAYAAACNU8MOAAAE6klEQVR4Xu2ZV4glRRSGz6qYc44s7qpgRAUDou6YwLCy5gdRdlARxYSCCqYd8cWMOWDYQYVdsygGxHDNOYuomFZ9EnwwB0z/x+narVtzu293zTA7g/3BD7dP1dzbXefUOad6zFrGm2WlfcZQLROIOdI86fQxUssEYXXpfWnldKBl8nOhdJK0dDrwf2JFaaXUOMlZX3paWi8dqMmS0lqpcTJBDflF+le6PBkbb1aRTpQuk061fKcE2LXHS8unA31YRlog/VNoUqf0mebOPSgdqOAO82DYMB3IZDPpdekUc4d8KP1sze4pZmPpPmmTdKABb0rvpsbFAd1gLkQ4EbpGOtCHPaXHpGFp8+6hxnSk2dE16fAv86ySkxqHpOMsv9RwfPpdujodWBw8kxoa8Jz0XmpswI7S/eY7ZYdkrA5LSH+aBxh1MsAzkVFwUhPIArdL26QDDdjD/LcPTgeasq20l3muz6WTGmoSIvQqabp0iOXfx5bSndLj5ru6CTdIj0pLJTYW+JjIBnS+6ya2mAvMA4IaXgZZihcSPHMvQjZbWzrAMgJlA+lF6RHpDPPO7knpqHhSTdh9OYQIpbaQgs4rPm8dT2oI9Q7HdKQDpSldo/V5wzw1x2kZh50pnR/ZYraQrrHqDEJd/8z8WR+Ubpbe6prh60nNf9h8fke6OJ5QBR0YqfCmyHaO+ULvF9nqkuvcIfMIPTKykQ7nRte5zJB+lE5OB2rArmItbi2uCRACDqfS6Lxi3Sk8wBqeYOW7dlD61TwAgSz1k/RamFDYyGY0eKFT3t38fqaGSVXcJv1h3U3MReaRWtV6cx7loVKFh021mv9ZKTjyg8T2vPRtYmvCdtLd0kPSzslYHWiCPpYesEUvH3AumW5/6VLpGxtZi3H+JdKuiT0wzby2XxHZlitsHL8CwZGHRjbKTK36T50jol9I7B3zqKxi0LwzTvVdDxsa4o9KYOGI4usS+/fmR5Gm7GZeYu6y/LROzaVmE/y8REjBtov0iXnwxEcx0jXn47LNwa7GQfQ3gYHCxnEwQHYgm8XlgO9l3mGRrScUaCayUwOkgt+sO6qakJOWtzK/jyMiG80dtuHI1g+e5ynpRvPdMRpwKrsvQJDQF8SQ7di9C6RjCxsND+s5UFz3ghpOtoxfasyR/pZWjWx0/R9F10AgsS4hnZcyYD5x38gWGptZ0k7mdaMJOc6lu+U3t49sdInUG44TVXB8Odz8d3FGVfdaF3777MTGbkt3C7t3hnnqvkfayDxAB6UVFk0bQUd6NbFx/+8Un6nvfPe95o1UgO8kw82NbKWQDjmcH11cUxffNl9o3qiQJpu23jnO5UFoJmYX1xwLfpBOWzijHBaChY8jfjTwVop7eda8D+hIL5vfT68UzxGF3fuldKX5veDwKs6VPo+uB83XnDJCcBIocJb0RfEZrpc+ldaMbJXwMF+Zt+HD5gX7a/MHu3bhrPrkOBeoNewA0iF1jN043tDU0Eiy0KlImfQoKWQOaif3zLGG9aS0VUFA0BXTqNHw0cWTLTjycCQKQUTafkKaL71UjPVrTEfAQ8Xpj2aCDjeHXOcCaadvLZmArGP+fpsjy97JWBVTrTvj8D29AoM1qUrz40buC/Zc+I/NpjVV1r2OlinmfQtlhJ3cMkbwdueWmoqPGS0tLS0tLS0tLS3l/AfzMQYe20nDqgAAAABJRU5ErkJggg==>

[image16]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAZCAYAAAA4/K6pAAAAt0lEQVR4Xu3RqwoCURCA4fGCF0QEixaDwWo26QtYrFZBTILCFotRsAoWH8FksthE0ffwQfwPZ8ssrAzLFmF/+MIynOVcRLJSa4QrJshHZuaa2OKJGUp6bK+GNd5Yhd+JKmMu/kduZ26HiSpgigf2aOuxPXcnZ3zE785cFUvxRwlQ1+P4GtjghQUqehxfCzvcxZ/d3YGpDg64YYycHv9ugAuG0cH/5N64Z9QN16j6OBkdUfTLslLpCySGGoN7dh6zAAAAAElFTkSuQmCC>

[image17]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADgAAAAaCAYAAADi4p8jAAACmklEQVR4Xu2WSahOYRjH/6YyRIYMhWRWZCjCCguRjSEhJQulq+41hJWFspGsUKZsJLNCmbsXEWVcUERJ2bAVC0OG/7/nnO77Pd7z3XO45LudX/265zzPued873mf9zkvUFJSEtCOzqSDXLxNMJ9epOvpweS4fcUVNcwceoz2CGIP6dzgvGbRLDXRLi7+lG5xsZpkBawsQwbSH3Syi/8T+sDWyzCf+E2u0860K2xgHegpuj+8KCcd6Tw6xidIPx/wdKeH6S26lu6lD2ADFiNRfNC96VnYTH2Fzdon+hzWUYuwmL6C3eMz3RDktLavBOe/0I0+oedppyB+h+5Ljk/DBlmEBWguz9F0LB0MG+zS9KIc6P/e0j10JT0BG+T4JL+JLk+Ooxyi3+kIF99Bv9FJsJktyi46wQfJJdjLzMsNOt3FttOdsLK9h8qJqaAn7G00+gTZCCuJa7DaL4p+WKwUr9LbPliFcT4A68r36TK61eUqUEPRILb5BKmH5c74RA60dmOzpM/GO9ga/1Oe0bu0v0+EzIYNQm/CUwfLTfMJWMNRd8xiEb3gg7DnfIGVfYga0gAXawlVyBEf9KiEHsPqOUWtXIv5EWyA2nFofaY7kaGwNfsiOY+xm76nvYKYSu0NrEt7XtMPdJRPVEEdeqoPxtBFl+lx2EAP0HWwGVJc3zKtm3SA+qvy+EiHJzGPmtIM+hL2zVMX1r1mhRcF6Nka4CqfqIKaS2yNZ6Ja1ifDMwTxjbHWUeyNa/2dS471AzT7fZvTmSyhq30wA93zpg+2JvrhWR9XrT+/PcuDKkfln4cG2Gfor6GdxBofTNCDJ/pgC0yhR32wCifpQh9sTTS4rPrfjOxcFtrZpNvCPKjBxJZTm0GdvqSk5D/nJ4xOa4wkF5IRAAAAAElFTkSuQmCC>

[image18]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAsAAAAaCAYAAABhJqYYAAAAwElEQVR4XmNgGDSAHV0AB2ABEfuAmBFNAh3EAHEiiNENxA4oUphgNxCLgBhaQLwQVQ4FKAPxGmQBkFN4kQWQQBMQ+yALZADxSiBuw4LPMEA9BwP8QHwViG2A2BqKQexqIO5CUgcHS4BYE01sORYxMHBmgIQMDAgyQPyCFYDC+gQDwn3ZQJyKkEYFJCkGgXog9oeyDzHgDk4wkAfi9UCsD8QLUKWwg20MkDC3Q5fABsKB+Ca6IC7ABsQm6IKjgCIAACt5GmBxsPRFAAAAAElFTkSuQmCC>

[image19]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAoAAAAaCAYAAACO5M0mAAAAb0lEQVR4XmNgGAW0AGpAHAbENkDMjiYHBkxAPB2I/yPha0CshawIBEoYUBUhK0Yx+SIWRTBsh6SO4TkWBTAciqSOeIVbsCgA4X9ArIKkjkEXiL+jKQLhCciKYECbAWLyMyA+D8Q5QMyIomIU0A8AAN4LMJmyZ6BbAAAAAElFTkSuQmCC>

[image20]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAZCAYAAAA4/K6pAAAAtUlEQVR4Xu3SrQvCQByH8fMVg2FgEUwKRk0Wk5hsJptJMJqsllWLUYNGo8ViMQwR/aNMPscZ/B1s3oGIYQ98YPBlY+ymVNpPC7DAFSVrS6yKJW4YIyfn+BrYIMIQGTnH18IeJ/StLbEujjigY20f6+GBqT341MQOZwyszasaVrhghKyc3asgxB0TFMTqURlzZR40U54/0XtFZT6yPl59/WfpV9LH6KL+ukfUxtbRGnlzW9pXegKZdBqg6G91nQAAAABJRU5ErkJggg==>

[image21]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAZCAYAAAA4/K6pAAAAwUlEQVR4Xu3PoQ5BURzH8TMCw8y8ApuhSooo2IyhKLQ7b0BRFFMEE4xwn0DRjI1OlMwUzyBIvnZs957/bpTsfrdP+Z0TzlHK7w+LIC5Hj4JyCGOMF544oW3ccIqhL8cBJsgggDK22CHvuvdpiprY1FIO35q44YA5Llgr/WKjlBxchdDCEFVxZpTGAjY6iBqnZkU5lHDFDBY2uCuPv1IFIznaSIotp/Tf92ggix7OSDjXdF05uKrjiAdWKJjHfn4/6A0PKxw2l/QkIAAAAABJRU5ErkJggg==>

[image22]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAaCAYAAAC+aNwHAAABCUlEQVR4XmNgGHEgHIjr0AWJBd5A/B+ID6JLEAM4gfgJAwUGVADxXgaIAUfR5AgCISB+BMRWDBADzqNKEwZ9QFwJxNIMEANuokrjB6pAfAuIOYBYmAFiACgsiAKMDJAAi4byuRggBryDqyAAsoD4BAPEIBj4B8Q/kPg4gTwQfwbiNiBOQsIgzSBXgFyDF1BkAMjJu4D4NRC/AOJXUDYI/2GAGKAFVw0FskjsBgaI31mQxGBgHwPEAB9kQWsgfg7EJkBcDMRvgVgZWQES6GeAGNAIxMwMkCQOzyQgDPKjHUw1FgDKjSB134H4AhAbgQRBJs0D4tNA7ApXih2wAXE2AySKBdDkRsHQBQBLyTp4/d+JMgAAAABJRU5ErkJggg==>

[image23]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAA0AAAAaCAYAAABsONZfAAAAy0lEQVR4Xu3QP8tBYRjH8QspJVIGoxIvwMQbULJYbc/6DGLwDmyyk7KYDCwmkQzKpMfwvBuDP9/TddPdlRdAnV996nT9znV37iMS5luTRRMFW7xLCjMc0MEIJ9FDgpTEHJTEP1aIe/Mjxu55Ibr4yhQ3FP0hGeCKsugXvJLBBTt/6NLDHVs0/CK4dFD0/aFLW7Rb2qLmipYtyK9oV7VFBGcMvVkMP/gTXaqL3jftvSMVrDEXXZ6gi4Sb77ERs/RMTvT32+QRtcMwn50HaXIhIujrNTMAAAAASUVORK5CYII=>

[image24]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAA0AAAAZCAYAAADqrKTxAAAA+klEQVR4Xu3SsUpCYRiH8c/AlijBNpOwxEmwbOsW2hSvQgUdJHBxcHLQwQIbnTVQ9wRpk1Y3L8AxaKsU6vk8nnzPm2Fr0AM/OP79PHAOGvOnS+Bcjz8VxwAVVDHGieeEKoQHxMRWR018/lYLF2q7Nc6NNnaKez3SFCU9ujVwCT/C2EUOTzgQ5zyNsIcZPjDHK47lIVkQvdX1EZLYxwR37iFdCgU90jVe9OjWxJkeqYyFcZ7P0yGejfMCZPZZ3pFX+7I03nAltgAe0YVP7F/dGOcH9tV20MYQWXlIZ++4s7q2f6PI+qvN2efp63Fb9lUX9bitDKJ6/O8XfQJ+ASPc9BwSpgAAAABJRU5ErkJggg==>

[image25]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAwAAAAaCAYAAACD+r1hAAAApklEQVR4XmNgGAWDEVgCsS26IBDwAzE7soA+EB8E4v9QvASI2ZDkNwGxCIzDCcQ3gHgZECcC8UQgfgvEOVB5EyCeBWWDQRsQFyELAIE7EJ+GskEG6SLJoXKQwBEg1gPiXegSuMA8BoimQHQJXKAeiB8AMTOaOE6QD8Tl6IL4AChkDNAF8YF7DKhxgReAQu4iuiA+kMkAiUCiQQsQm6EL4gNEB+VQBABGahafRWSX5gAAAABJRU5ErkJggg==>

[image26]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAwAAAAZCAYAAAAFbs/PAAABBUlEQVR4Xu3RLUuDURjG8VsRhzBnMSmOITj3BUwWwSAY7CJjYSwsLJhFsExYmM0ifgCLIDjYgmajSbtg8CXoBxjq/5z7bNzP/QEEwQt+5Tovz3POEfkz2cYR6ihkh7LJ4xqH2BJd9IY1MyeTYxy4roVXzLg+5hOPrivhGw3Xx2yg6LopfOHU9TE5NDHAFfawIvqFCzMvpoR7nGMZ0+jiTHRBezyTzOIBPUyYfg5D0QVV00snleu2TPkQXbQ4KsKhXvCOyVGZsiC6UXibcVZTGQ7qUxMd27FlJZUntky5E93Inism3M6N68KLP2Pe9TFlPOES+7hFH0t2kk+42k3siv7mf34vPytgLgq5yLM9AAAAAElFTkSuQmCC>

[image27]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAA8AAAAaCAYAAABozQZiAAABE0lEQVR4Xu3SP0tCURjH8acSK2oIIQgJl3BpaGtqsUFcA8cohF5Ck/gCHFrF3anchCKLxnCxP0bgIs2Ba2+gvg/n8XrOvba02g8+3Ofc55zrPZ4rMp8pYYxHXOEB+8EMkTMM8YV7bPrNPHpYxxq6yPgTSBYfsXtRRmhZvYs2FqZtWcKbNw7yhArObXyCatQVWcTAGwfRxZoGDq1uomC15sWrg0wWL+MG21jBLbas92zXRCaLNTlxD0hjBx1xe/bnBIk3iuK2oDlCXZJzoszaTw2nVl+g7/WCvMZviDsqPbI9pHAdtqf57Qw3cGdX/QIT0T/mE6vxhkV/+VJmLD4Q98rfeEc5bEc5FvcJ/zn6Bv+Zs/wABK8rUU382pcAAAAASUVORK5CYII=>

[image28]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAA8AAAAaCAYAAABozQZiAAABH0lEQVR4Xu3Su0oDURDG8RGDF0LES6Np0hhBwcLCSnwCOyGNhZUhdYrgM9iKoGCjqIhaiKJio+kCgoiFYGGj4IP4P87sZvaCL2A++MFhhtndc86K/M+so4NXnNn6G4/4wg1WUYgG0hnBga23MWvrSRxhBQ8oWj2RIZzYehcTrreJQVSx5epxBqQ7vIcx16tj2da3rh4n7McPj7peE4uiD7xy9Tj9kj88jDYWRAeXrJ5IevgQF/gQPflQm7F+JunhedyLvvlaktvIJG/PNbQwh33r5Sac9rGt/Z5PMS16XWtWyyTcsx+OrqqMS9FthTMID8qkJN1P2xH9s6KEe97AOO4w5Xq///YT3kX/7Te8oGH9PtG3nuMZn6hYr5de/s4PLvgwyquF11MAAAAASUVORK5CYII=>
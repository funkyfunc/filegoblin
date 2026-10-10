#!/usr/bin/env python3
import os, sys, zipfile, json, urllib.request

GAUNTLET_DIR = "/Users/stompinggrounds/Development/filegoblin/test_assets/gauntlet"
os.makedirs(GAUNTLET_DIR, exist_ok=True)

manifest = []

# --- PPTX Cases ---

# P1: Flowchart with connectors
p1_path = f"{GAUNTLET_DIR}/p1_connectors_flowchart.pptx"
manifest.append({
    "id": "P1",
    "file": "p1_connectors_flowchart.pptx",
    "category": "PPTX Diagrams",
    "description": "Flowchart with glued connectors (<p:cxnSp>)",
    "canaries_present": ["Ingest Input", "Validate Schema", "Store Record"],
    "syntax_contains": ["flowchart", "-->"],
})

# P2: Grouped shapes (MarkItDown #332)
p2_path = f"{GAUNTLET_DIR}/p2_grouped_shapes.pptx"
manifest.append({
    "id": "P2",
    "file": "p2_grouped_shapes.pptx",
    "category": "PPTX Grouped Shapes",
    "description": "Text inside nested <p:grpSp> groups (MarkItDown #332)",
    "canaries_present": ["CANARY_GROUPED_SHAPE_TEXT_ALPHA", "CANARY_GROUPED_SHAPE_TEXT_BETA"],
})

# P3: Chart with missing data points (MarkItDown #2526)
p3_content = '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <c:chart>
    <c:title><c:tx><c:rich><a:p><a:r><a:t>Quarterly Metrics With Gaps</a:t></a:r></a:p></c:rich></c:tx></c:title>
    <c:plotArea>
      <c:barChart>
        <c:barDir val="col"/>
        <c:ser>
          <c:idx val="0"/><c:order val="0"/>
          <c:tx><c:strRef><c:strCache><c:pt count="1"><c:v>Throughput</c:v></c:pt></c:strCache></c:strRef></c:tx>
          <c:cat>
            <c:strRef>
              <c:strCache>
                <c:ptCount val="3"/>
                <c:pt idx="0"><c:v>Q1_NORMAL</c:v></c:pt>
                <c:pt idx="1"><c:v>Q2_MISSING_VAL</c:v></c:pt>
                <c:pt idx="2"><c:v>Q3_NORMAL</c:v></c:pt>
              </c:strCache>
            </c:strRef>
          </c:cat>
          <c:val>
            <c:numRef>
              <c:numCache>
                <c:ptCount val="2"/>
                <c:pt idx="0"><c:v>100.5</c:v></c:pt>
                <c:pt idx="2"><c:v>300.5</c:v></c:pt>
              </c:numCache>
            </c:numRef>
          </c:val>
        </c:ser>
      </c:barChart>
    </c:plotArea>
  </c:chart>
</c:chartSpace>'''

p3_path = f"{GAUNTLET_DIR}/p3_chart_gaps.pptx"
with zipfile.ZipFile(p3_path, 'w', zipfile.ZIP_DEFLATED) as z:
    z.writestr('[Content_Types].xml', '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
  <Override PartName="/ppt/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>
</Types>''')
    z.writestr('_rels/.rels', '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>''')
    z.writestr('ppt/presentation.xml', '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst>
</p:presentation>''')
    z.writestr('ppt/_rels/presentation.xml.rels', '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>
</Relationships>''')
    z.writestr('ppt/slides/slide1.xml', '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
      <p:grpSpPr/>
      <p:graphicFrame>
        <p:nvGraphicFramePr><p:cNvPr id="3" name="Chart 1"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>
        <a:graphic>
          <a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart">
            <c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rIdChart1"/>
          </a:graphicData>
        </a:graphic>
      </p:graphicFrame>
    </p:spTree>
  </p:cSld>
</p:sld>''')
    z.writestr('ppt/slides/_rels/slide1.xml.rels', '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rIdChart1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/>
</Relationships>''')
    z.writestr('ppt/charts/chart1.xml', p3_content)

manifest.append({
    "id": "P3",
    "file": "p3_chart_gaps.pptx",
    "category": "PPTX Chart Gaps",
    "description": "Chart with missing data points, must not print literal 'None' (MarkItDown #2526)",
    "canaries_present": ["Q1_NORMAL", "Q3_NORMAL", "100.5", "300.5"],
    "forbidden_absent": ["None", "NaN"],
})

# --- DOCX Cases ---

# D1: Merged table cells in Word
d1_doc = '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:tbl>
      <w:tr>
        <w:tc>
          <w:tcPr><w:gridSpan w:val="2"/></w:tcPr>
          <w:p><w:r><w:t>CANARY_MERGED_HEADER_ACROSS_TWO_COLS</w:t></w:r></w:p>
        </w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>CANARY_CELL_COL1</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>CANARY_CELL_COL2</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
  </w:body>
</w:document>'''

d1_path = f"{GAUNTLET_DIR}/d1_merged_table.docx"
with zipfile.ZipFile(d1_path, 'w', zipfile.ZIP_DEFLATED) as z:
    z.writestr('[Content_Types].xml', '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>''')
    z.writestr('_rels/.rels', '''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>''')
    z.writestr('word/document.xml', d1_doc)

manifest.append({
    "id": "D1",
    "file": "d1_merged_table.docx",
    "category": "DOCX Tables",
    "description": "Word table with gridSpan merged headers",
    "canaries_present": ["CANARY_MERGED_HEADER_ACROSS_TWO_COLS", "CANARY_CELL_COL1", "CANARY_CELL_COL2"],
})

# D3: Text boxes in AlternateContent (Docling #4569)
manifest.append({
    "id": "D3",
    "file": "d3_textbox_alternate_content.docx",
    "category": "DOCX Floating Text Boxes",
    "description": "Floating text box inside mc:AlternateContent (Docling #4569)",
    "canaries_present": ["CANARY_BODY_BEFORE_TEXTBOX", "CANARY_TEXTBOX_ALTERNATE_CONTENT", "CANARY_BODY_AFTER_TEXTBOX"],
})

# D4: Equations (OMML)
manifest.append({
    "id": "D4",
    "file": "equations.docx",
    "category": "DOCX Equations",
    "description": "Word equations in OMML format compiled to LaTeX",
    "canaries_present": ["m=1"],
    "syntax_contains": ["$"],
})

# --- PDF Cases ---
manifest.append({
    "id": "R1",
    "file": "slice_a_twocolumn.pdf",
    "category": "PDF Layout",
    "description": "Two-column scanned layout without line interleaving",
    "canaries_present": ["Structuring Interviews with Experts"],
})

manifest.append({
    "id": "R2",
    "file": "slice_e_math_zeitz.pdf",
    "category": "PDF Math",
    "description": "Heavy mathematical notation and inequalities (CMSY/CMMI)",
    "canaries_present": ["Zeitz"],
})

# Write the Gauntlet manifest
with open(f"{GAUNTLET_DIR}/gauntlet_manifest.json", "w") as f:
    json.dump(manifest, f, indent=2)

print(f"Generated Gauntlet with {len(manifest)} test cases in {GAUNTLET_DIR}")

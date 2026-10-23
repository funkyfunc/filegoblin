use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

pub struct OfficeGobbler;

impl Gobble for OfficeGobbler {
    /// Converts .docx to structured markdown: heading styles become `#` levels,
    /// numbered/bulleted paragraphs become list items, bold/italic runs keep
    /// their emphasis, and hyperlinks keep their URLs. Tables use the same
    /// "Sequence of Records" dialect as the PDF and sheet parsers.
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> Result<String> {
        if !path.exists() {
            anyhow::bail!("Mischievous I/O Error: file not found at {:?}", path);
        }

        let buf = std::fs::read(path).context("Failed to read docx buffer")?;

        // Every Word package is a zip: `.docx`, its macro/template variants
        // (`.docm`/`.dotx`/`.dotm`), and renamed documents that routing found
        // by their `word/` entries. This used to demand an extension of exactly
        // `docx`, so all of those were refused.
        if buf.starts_with(b"PK") {
            // Scan zip archive for document.xml, rels, charts, and smartart diagrams
            let mut doc_xml = String::new();
            let mut rels_xml = String::new();
            let mut chart_xmls: HashMap<String, String> = HashMap::new();
            let mut smartart_xmls: HashMap<String, String> = HashMap::new();

            if let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(&buf)) {
                for i in 0..archive.len() {
                    if let Ok(mut file) = archive.by_index(i) {
                        let name = file.name().to_string();
                        if name == "word/document.xml" {
                            doc_xml = crate::parsers::read_zip_entry_to_string(&mut file, &name)
                                .unwrap_or_default();
                        } else if name == "word/_rels/document.xml.rels" {
                            rels_xml = crate::parsers::read_zip_entry_to_string(&mut file, &name)
                                .unwrap_or_default();
                        } else if name.starts_with("word/charts/chart") && name.ends_with(".xml") {
                            if let Ok(c) =
                                crate::parsers::read_zip_entry_to_string(&mut file, &name)
                            {
                                chart_xmls.insert(name, c);
                            }
                        } else if name.starts_with("word/diagrams/data") && name.ends_with(".xml") {
                            if let Ok(c) =
                                crate::parsers::read_zip_entry_to_string(&mut file, &name)
                            {
                                smartart_xmls.insert(name, c);
                            }
                        }
                    }
                }
            }

            let docx_res = docx_rs::read_docx(&buf);
            let mut extracted = String::new();

            if let Ok(docx) = docx_res {
                let rels_chart_map = extract_docx_rels_charts(&rels_xml);
                let rels_diagram_map = extract_docx_rels_diagrams(&rels_xml);
                let (body_paragraphs, math_by_para_id) = if !doc_xml.is_empty() {
                    crate::parsers::omml::extract_docx_body_paragraphs(&doc_xml)
                } else {
                    (Vec::new(), HashMap::new())
                };

                // rid → URL, populated by the reader from document rels.
                let links: HashMap<&str, &str> = docx
                    .hyperlinks
                    .iter()
                    .map(|(rid, target, _)| (rid.as_str(), target.as_str()))
                    .collect();

                let mut blocks: Vec<(bool, String)> = Vec::new(); // (is_list_item, text)
                let mut used_charts: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut used_smartart: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut body_p_idx = 0usize;

                for child in &docx.document.children {
                    match child {
                        docx_rs::DocumentChild::Paragraph(p) => {
                            let bp = body_paragraphs.get(body_p_idx);
                            let math_text = bp
                                .and_then(|b| {
                                    if b.has_math {
                                        Some(b.text.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .or_else(|| math_by_para_id.get(&p.id).map(|s| s.as_str()));

                            let md = paragraph_to_md(p, &links, math_text);
                            let final_md = if md.trim().is_empty() {
                                bp.map(|b| b.text.trim().to_string()).unwrap_or_default()
                            } else {
                                md
                            };

                            if !final_md.trim().is_empty() {
                                let is_list = p.property.numbering_property.is_some();
                                blocks.push((is_list, final_md));
                            }

                            // Emit any charts anchored to this paragraph
                            if let Some(b) = bp {
                                for rid in &b.chart_rids {
                                    if let Some(target) = rels_chart_map.get(rid)
                                        && let Some(chart_xml) = chart_xmls.get(target)
                                    {
                                        used_charts.insert(target.clone());
                                        if let Ok(chart) =
                                            crate::parsers::openxml_chart::OpenXmlChart::parse(
                                                chart_xml,
                                            )
                                        {
                                            let chart_md = chart.render_markdown();
                                            if !chart_md.is_empty() {
                                                blocks.push((false, chart_md));
                                            }
                                        }
                                    }
                                }

                                for rid in &b.diagram_rids {
                                    if let Some(target) = rels_diagram_map.get(rid)
                                        && let Some(dgm_xml) = smartart_xmls.get(target)
                                    {
                                        used_smartart.insert(target.clone());
                                        if let Ok(diag) =
                                            crate::parsers::smartart::SmartArtDiagram::parse(
                                                dgm_xml,
                                            )
                                        {
                                            if let Some(mermaid) = diag.render_mermaid() {
                                                blocks.push((
                                                    false,
                                                    format!("```mermaid\n{}\n```", mermaid),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }

                            body_p_idx += 1;
                        }
                        docx_rs::DocumentChild::Table(t) => {
                            blocks.push((false, format_table(t, &math_by_para_id)));
                        }
                        _ => {}
                    }
                }

                // Emit any remaining unassociated charts
                let mut leftover_charts: Vec<_> = chart_xmls
                    .iter()
                    .filter(|(path, _)| !used_charts.contains(*path))
                    .collect();
                leftover_charts.sort_by_key(|(path, _)| (*path).clone());

                for (_path, chart_xml) in leftover_charts {
                    if let Ok(chart) = crate::parsers::openxml_chart::OpenXmlChart::parse(chart_xml)
                    {
                        let chart_md = chart.render_markdown();
                        if !chart_md.is_empty() {
                            blocks.push((false, chart_md));
                        }
                    }
                }

                // Emit any remaining unassociated SmartArt diagrams
                let mut leftover_diagrams: Vec<_> = smartart_xmls
                    .iter()
                    .filter(|(path, _)| !used_smartart.contains(*path))
                    .collect();
                leftover_diagrams.sort_by_key(|(path, _)| (*path).clone());

                for (_path, dgm_xml) in leftover_diagrams {
                    if let Ok(diag) = crate::parsers::smartart::SmartArtDiagram::parse(dgm_xml) {
                        if let Some(mermaid) = diag.render_mermaid() {
                            blocks.push((false, format!("```mermaid\n{}\n```", mermaid)));
                        }
                    }
                }

                // If document contains DrawingML flowchart connectors, extract them
                if doc_xml.contains("cxnSp") {
                    if let Ok(diag) = crate::parsers::drawingml::SlideDiagram::parse(&doc_xml) {
                        if let Some(mermaid) = diag.render_mermaid() {
                            blocks.push((false, format!("```mermaid\n{}\n```", mermaid)));
                        }
                    }
                }

                // Blank line between blocks, except between consecutive list items
                let mut prev_was_list = false;
                for (is_list, block) in blocks {
                    if !extracted.is_empty() {
                        extracted.push('\n');
                        if !(is_list && prev_was_list) {
                            extracted.push('\n');
                        }
                    }
                    extracted.push_str(block.trim_end());
                    prev_was_list = is_list;
                }
            }

            // Fallback to streaming quick-xml reader if docx-rs failed or produced empty output
            if extracted.trim().is_empty() && !doc_xml.is_empty() {
                if let Ok(streaming_res) =
                    parse_docx_xml_streaming(&doc_xml, &rels_xml, &chart_xmls, &smartart_xmls)
                {
                    return Ok(streaming_res);
                }
            }

            if extracted.trim().is_empty() {
                anyhow::bail!(
                    "No text could be extracted from this .docx (content may live in text \
                     boxes, content controls, or images rather than paragraphs)"
                );
            }
            Ok(extracted.trim().to_string())
        } else {
            anyhow::bail!("Not a Word document: the file is not an OOXML (zip) package")
        }
    }
}

/// Extracts relationship ID to SmartArt diagram data target path mapping from `word/_rels/document.xml.rels`.
fn extract_docx_rels_diagrams(xml: &str) -> HashMap<String, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut map = HashMap::new();
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(ref e))
            | Ok(quick_xml::events::Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let mut id = None;
                    let mut target = None;
                    let mut is_diagram = false;
                    for attr in e.attributes().flatten() {
                        match attr.key.local_name().as_ref() {
                            b"Id" => {
                                if let Ok(val) = attr.unescape_value() {
                                    id = Some(val.into_owned());
                                }
                            }
                            b"Type" => {
                                if let Ok(val) = attr.unescape_value() {
                                    if val.ends_with("/diagramData") || val.contains("diagramData")
                                    {
                                        is_diagram = true;
                                    }
                                }
                            }
                            b"Target" => {
                                if let Ok(val) = attr.unescape_value() {
                                    let t = val.into_owned();
                                    if t.contains("diagrams/data") {
                                        is_diagram = true;
                                    }
                                    target = Some(t);
                                }
                            }
                            _ => {}
                        }
                    }
                    if is_diagram && let (Some(id), Some(t)) = (id, target) {
                        let normalized = if let Some(stripped) = t.strip_prefix("../") {
                            format!("word/{}", stripped)
                        } else if let Some(stripped) = t.strip_prefix('/') {
                            stripped.to_string()
                        } else if t.starts_with("word/") {
                            t
                        } else {
                            format!("word/{}", t)
                        };
                        map.insert(id, normalized);
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Extracts relationship ID to chart target path mapping from `word/_rels/document.xml.rels`.
fn extract_docx_rels_charts(xml: &str) -> HashMap<String, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut map = HashMap::new();
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(ref e))
            | Ok(quick_xml::events::Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let mut id = None;
                    let mut target = None;
                    let mut is_chart = false;
                    for attr in e.attributes().flatten() {
                        match attr.key.local_name().as_ref() {
                            b"Id" => {
                                if let Ok(val) = attr.unescape_value() {
                                    id = Some(val.into_owned());
                                }
                            }
                            b"Type" => {
                                if let Ok(val) = attr.unescape_value() {
                                    if val.ends_with("/chart") || val.contains("chart") {
                                        is_chart = true;
                                    }
                                }
                            }
                            b"Target" => {
                                if let Ok(val) = attr.unescape_value() {
                                    target = Some(val.into_owned());
                                }
                            }
                            _ => {}
                        }
                    }
                    if is_chart && let (Some(id), Some(t)) = (id, target) {
                        let normalized = if let Some(stripped) = t.strip_prefix("../") {
                            format!("word/{}", stripped)
                        } else if let Some(stripped) = t.strip_prefix('/') {
                            stripped.to_string()
                        } else if t.starts_with("word/") {
                            t
                        } else {
                            format!("word/{}", t)
                        };
                        map.insert(id, normalized);
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Extracts relationship ID to hyperlink target mapping from `word/_rels/document.xml.rels`.
fn extract_docx_rels_links(xml: &str) -> HashMap<String, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut map = HashMap::new();
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(ref e))
            | Ok(quick_xml::events::Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let mut id = None;
                    let mut target = None;
                    let mut is_link = false;
                    for attr in e.attributes().flatten() {
                        match attr.key.local_name().as_ref() {
                            b"Id" => {
                                if let Ok(val) = attr.unescape_value() {
                                    id = Some(val.into_owned());
                                }
                            }
                            b"Type" => {
                                if let Ok(val) = attr.unescape_value() {
                                    if val.ends_with("/hyperlink") || val.contains("hyperlink") {
                                        is_link = true;
                                    }
                                }
                            }
                            b"Target" => {
                                if let Ok(val) = attr.unescape_value() {
                                    target = Some(val.into_owned());
                                }
                            }
                            _ => {}
                        }
                    }
                    if is_link && let (Some(id), Some(t)) = (id, target) {
                        map.insert(id, t);
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Streaming quick-xml fallback parser for Word documents when docx-rs cannot parse non-standard
/// or complex OpenXML structures (e.g. `<mc:AlternateContent>` floating text boxes, merged table
/// `<w:gridSpan>` headers, or minimal XML packages).
pub fn parse_docx_xml_streaming(
    doc_xml: &str,
    rels_xml: &str,
    chart_xmls: &HashMap<String, String>,
    smartart_xmls: &HashMap<String, String>,
) -> Result<String> {
    let rels_chart_map = extract_docx_rels_charts(rels_xml);
    let rels_diagram_map = extract_docx_rels_diagrams(rels_xml);
    let rels_links_map = extract_docx_rels_links(rels_xml);

    let mut reader = quick_xml::Reader::from_str(doc_xml);
    reader.config_mut().trim_text(false);

    let mut blocks: Vec<(bool, String)> = Vec::new();
    let mut used_charts: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut used_smartart: std::collections::HashSet<String> = std::collections::HashSet::new();

    struct CellState {
        text: String,
        span: usize,
    }
    struct RowState {
        cells: Vec<CellState>,
    }
    struct TableState {
        rows: Vec<RowState>,
        current_cell: Option<CellState>,
    }
    let mut tbl_stack: Vec<TableState> = Vec::new();

    #[derive(Default, Clone)]
    struct ParaState {
        text: String,
        heading_level: Option<usize>,
        is_list_item: bool,
        list_level: usize,
        chart_rids: Vec<String>,
        diagram_rids: Vec<String>,
    }
    let mut p_stack: Vec<ParaState> = Vec::new();

    let mut in_bold = false;
    let mut in_italic = false;
    let mut current_link_url: Option<String> = None;
    let mut in_body = false;
    let mut in_fallback = false;
    let mut choice_taken = false;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"AlternateContent" => {
                        choice_taken = false;
                    }
                    b"Choice" => {
                        choice_taken = true;
                    }
                    b"Fallback" => {
                        in_fallback = true;
                    }
                    _ if in_fallback && choice_taken => {
                        // Skip duplicate content from fallback
                    }
                    b"body" => in_body = true,
                    b"tbl" if in_body => {
                        tbl_stack.push(TableState {
                            rows: Vec::new(),
                            current_cell: None,
                        });
                    }
                    b"tr" if !tbl_stack.is_empty() => {
                        if let Some(tbl) = tbl_stack.last_mut() {
                            tbl.rows.push(RowState { cells: Vec::new() });
                        }
                    }
                    b"tc" if !tbl_stack.is_empty() => {
                        if let Some(tbl) = tbl_stack.last_mut() {
                            tbl.current_cell = Some(CellState {
                                text: String::new(),
                                span: 1,
                            });
                        }
                    }
                    b"gridSpan" if !tbl_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Ok(span) = val.parse::<usize>() {
                                        if let Some(tbl) = tbl_stack.last_mut() {
                                            if let Some(ref mut cell) = tbl.current_cell {
                                                cell.span = span.max(1);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    b"p" if in_body => {
                        p_stack.push(ParaState::default());
                    }
                    b"pStyle" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Some(lvl) = heading_level(&val) {
                                        if let Some(p) = p_stack.last_mut() {
                                            p.heading_level = Some(lvl);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    b"numPr" if !p_stack.is_empty() => {
                        if let Some(p) = p_stack.last_mut() {
                            p.is_list_item = true;
                        }
                    }
                    b"ilvl" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Ok(lvl) = val.parse::<usize>() {
                                        if let Some(p) = p_stack.last_mut() {
                                            p.list_level = lvl;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    b"hyperlink" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"id" {
                                if let Ok(val) = attr.unescape_value() {
                                    current_link_url = rels_links_map.get(val.as_ref()).cloned();
                                }
                            }
                        }
                    }
                    b"b" | b"bCs" => {
                        let mut val_off = false;
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(v) = attr.unescape_value() {
                                    if v == "0" || v == "false" {
                                        val_off = true;
                                    }
                                }
                            }
                        }
                        if !val_off {
                            in_bold = true;
                        }
                    }
                    b"i" | b"iCs" => {
                        let mut val_off = false;
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(v) = attr.unescape_value() {
                                    if v == "0" || v == "false" {
                                        val_off = true;
                                    }
                                }
                            }
                        }
                        if !val_off {
                            in_italic = true;
                        }
                    }
                    b"chart" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"id" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Some(p) = p_stack.last_mut() {
                                        p.chart_rids.push(val.to_string());
                                    }
                                }
                            }
                        }
                    }
                    b"relIds" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"dm" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Some(p) = p_stack.last_mut() {
                                        p.diagram_rids.push(val.to_string());
                                    }
                                }
                            }
                        }
                    }
                    b"oMath" if !p_stack.is_empty() => {
                        let mut math_attrs = HashMap::new();
                        for attr in e.attributes().flatten() {
                            let k =
                                String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                            if let Ok(v) = attr.unescape_value() {
                                math_attrs.insert(k, v.into_owned());
                            }
                        }
                        let latex = crate::parsers::omml::parse_omml_element_to_latex(
                            &mut reader,
                            "oMath",
                            math_attrs,
                        );
                        let trimmed = latex.trim();
                        if !trimmed.is_empty() {
                            if let Some(p) = p_stack.last_mut() {
                                p.text.push_str(&format!("${}$", trimmed));
                            }
                        }
                    }
                    b"oMathPara" if !p_stack.is_empty() => {
                        let mut math_attrs = HashMap::new();
                        for attr in e.attributes().flatten() {
                            let k =
                                String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                            if let Ok(v) = attr.unescape_value() {
                                math_attrs.insert(k, v.into_owned());
                            }
                        }
                        let latex = crate::parsers::omml::parse_omml_element_to_latex(
                            &mut reader,
                            "oMathPara",
                            math_attrs,
                        );
                        let trimmed = latex.trim();
                        if !trimmed.is_empty() {
                            if let Some(p) = p_stack.last_mut() {
                                p.text.push_str(&format!("$${}$$", trimmed));
                            }
                        }
                    }
                    b"t" if !p_stack.is_empty() => {
                        let mut text = String::new();
                        loop {
                            match reader.read_event() {
                                Ok(quick_xml::events::Event::Text(ref t)) => {
                                    if let Ok(s) = std::str::from_utf8(t.as_ref()) {
                                        if let Ok(unescaped) = quick_xml::escape::unescape(s) {
                                            text.push_str(&unescaped);
                                        }
                                    }
                                }
                                Ok(quick_xml::events::Event::GeneralRef(ref ge)) => {
                                    if let Some(s) = crate::parsers::resolve_xml_entity(ge) {
                                        text.push_str(&s);
                                    }
                                }
                                Ok(quick_xml::events::Event::End(ref end_e))
                                    if end_e.local_name().as_ref() == b"t" =>
                                {
                                    break;
                                }
                                Ok(quick_xml::events::Event::Eof) | Err(_) => break,
                                _ => {}
                            }
                        }
                        let formatted_text = match (in_bold, in_italic) {
                            (true, true) => emphasize(&text, "***"),
                            (true, false) => emphasize(&text, "**"),
                            (false, true) => emphasize(&text, "*"),
                            (false, false) => text,
                        };
                        let final_text = if let Some(ref url) = current_link_url {
                            if !formatted_text.trim().is_empty() {
                                format!("[{}]({})", formatted_text.trim(), url)
                            } else {
                                formatted_text
                            }
                        } else {
                            formatted_text
                        };
                        if let Some(p) = p_stack.last_mut() {
                            p.text.push_str(&final_text);
                        }
                    }
                    b"tab" if !p_stack.is_empty() => {
                        if let Some(p) = p_stack.last_mut() {
                            p.text.push('\t');
                        }
                    }
                    b"br" if !p_stack.is_empty() => {
                        if let Some(p) = p_stack.last_mut() {
                            p.text.push('\n');
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    _ if in_fallback && choice_taken => {}
                    b"gridSpan" if !tbl_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Ok(span) = val.parse::<usize>() {
                                        if let Some(tbl) = tbl_stack.last_mut() {
                                            if let Some(ref mut cell) = tbl.current_cell {
                                                cell.span = span.max(1);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    b"pStyle" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Some(lvl) = heading_level(&val) {
                                        if let Some(p) = p_stack.last_mut() {
                                            p.heading_level = Some(lvl);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    b"numPr" if !p_stack.is_empty() => {
                        if let Some(p) = p_stack.last_mut() {
                            p.is_list_item = true;
                        }
                    }
                    b"ilvl" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Ok(lvl) = val.parse::<usize>() {
                                        if let Some(p) = p_stack.last_mut() {
                                            p.list_level = lvl;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    b"chart" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"id" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Some(p) = p_stack.last_mut() {
                                        p.chart_rids.push(val.to_string());
                                    }
                                }
                            }
                        }
                    }
                    b"relIds" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"dm" {
                                if let Ok(val) = attr.unescape_value() {
                                    if let Some(p) = p_stack.last_mut() {
                                        p.diagram_rids.push(val.to_string());
                                    }
                                }
                            }
                        }
                    }
                    b"b" | b"bCs" => in_bold = true,
                    b"i" | b"iCs" => in_italic = true,
                    b"tab" if !p_stack.is_empty() => {
                        if let Some(p) = p_stack.last_mut() {
                            p.text.push('\t');
                        }
                    }
                    b"br" if !p_stack.is_empty() => {
                        if let Some(p) = p_stack.last_mut() {
                            p.text.push('\n');
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"AlternateContent" => {
                        choice_taken = false;
                    }
                    b"Fallback" => {
                        in_fallback = false;
                    }
                    b"body" => in_body = false,
                    b"r" => {
                        in_bold = false;
                        in_italic = false;
                    }
                    b"hyperlink" => {
                        current_link_url = None;
                    }
                    b"tc" if !tbl_stack.is_empty() => {
                        if let Some(tbl) = tbl_stack.last_mut() {
                            if let Some(cell) = tbl.current_cell.take() {
                                if let Some(row) = tbl.rows.last_mut() {
                                    row.cells.push(cell);
                                }
                            }
                        }
                    }
                    b"tbl" if !tbl_stack.is_empty() => {
                        if let Some(tbl) = tbl_stack.pop() {
                            let mut builder = crate::parsers::TableBuilder::default();
                            for row in tbl.rows {
                                for cell in row.cells {
                                    builder.push_cell(cell.text.trim(), cell.span);
                                }
                                builder.end_row();
                            }
                            let table_md = builder.render();
                            if !table_md.trim().is_empty() {
                                blocks.push((false, table_md));
                            }
                        }
                    }
                    b"p" => {
                        if let Some(finished_p) = p_stack.pop() {
                            if p_stack.is_empty() {
                                if let Some(tbl) = tbl_stack.last_mut() {
                                    if let Some(ref mut cell) = tbl.current_cell {
                                        if !cell.text.is_empty() && !cell.text.ends_with(' ') {
                                            cell.text.push(' ');
                                        }
                                        cell.text.push_str(finished_p.text.trim());
                                    }
                                } else {
                                    let formatted = if let Some(lvl) = finished_p.heading_level {
                                        format!("{} {}", "#".repeat(lvl), finished_p.text.trim())
                                    } else if finished_p.is_list_item {
                                        format!(
                                            "{}- {}",
                                            "  ".repeat(finished_p.list_level),
                                            finished_p.text.trim()
                                        )
                                    } else {
                                        finished_p.text.trim().to_string()
                                    };
                                    if !formatted.is_empty() {
                                        blocks.push((finished_p.is_list_item, formatted));
                                    }
                                    for rid in &finished_p.chart_rids {
                                        if let Some(target) = rels_chart_map.get(rid)
                                            && let Some(chart_xml) = chart_xmls.get(target)
                                        {
                                            used_charts.insert(target.clone());
                                            if let Ok(chart) =
                                                crate::parsers::openxml_chart::OpenXmlChart::parse(
                                                    chart_xml,
                                                )
                                            {
                                                let chart_md = chart.render_markdown();
                                                if !chart_md.is_empty() {
                                                    blocks.push((false, chart_md));
                                                }
                                            }
                                        }
                                    }
                                    for rid in &finished_p.diagram_rids {
                                        if let Some(target) = rels_diagram_map.get(rid)
                                            && let Some(dgm_xml) = smartart_xmls.get(target)
                                        {
                                            used_smartart.insert(target.clone());
                                            if let Ok(diag) =
                                                crate::parsers::smartart::SmartArtDiagram::parse(
                                                    dgm_xml,
                                                )
                                            {
                                                if let Some(mermaid) = diag.render_mermaid() {
                                                    blocks.push((
                                                        false,
                                                        format!("```mermaid\n{}\n```", mermaid),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                if let Some(parent) = p_stack.last_mut() {
                                    if !parent.text.is_empty()
                                        && !parent.text.ends_with(' ')
                                        && !parent.text.ends_with('\n')
                                    {
                                        parent.text.push(' ');
                                    }
                                    parent.text.push_str(finished_p.text.trim());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let mut leftover_charts: Vec<_> = chart_xmls
        .iter()
        .filter(|(path, _)| !used_charts.contains(*path))
        .collect();
    leftover_charts.sort_by_key(|(path, _)| (*path).clone());

    for (_path, chart_xml) in leftover_charts {
        if let Ok(chart) = crate::parsers::openxml_chart::OpenXmlChart::parse(chart_xml) {
            let chart_md = chart.render_markdown();
            if !chart_md.is_empty() {
                blocks.push((false, chart_md));
            }
        }
    }

    let mut leftover_diagrams: Vec<_> = smartart_xmls
        .iter()
        .filter(|(path, _)| !used_smartart.contains(*path))
        .collect();
    leftover_diagrams.sort_by_key(|(path, _)| (*path).clone());

    for (_path, dgm_xml) in leftover_diagrams {
        if let Ok(diag) = crate::parsers::smartart::SmartArtDiagram::parse(dgm_xml) {
            if let Some(mermaid) = diag.render_mermaid() {
                blocks.push((false, format!("```mermaid\n{}\n```", mermaid)));
            }
        }
    }

    if doc_xml.contains("cxnSp") {
        if let Ok(diag) = crate::parsers::drawingml::SlideDiagram::parse(doc_xml) {
            if let Some(mermaid) = diag.render_mermaid() {
                blocks.push((false, format!("```mermaid\n{}\n```", mermaid)));
            }
        }
    }

    let mut extracted = String::new();
    let mut prev_was_list = false;
    for (is_list, block) in blocks {
        if !extracted.is_empty() {
            extracted.push('\n');
            if !(is_list && prev_was_list) {
                extracted.push('\n');
            }
        }
        extracted.push_str(block.trim_end());
        prev_was_list = is_list;
    }

    if extracted.trim().is_empty() {
        anyhow::bail!(
            "No text could be extracted from this .docx (content may live in text boxes, content controls, or images rather than paragraphs)"
        );
    }
    Ok(extracted.trim().to_string())
}

/// One paragraph as a markdown block: heading, list item, or plain text.
fn paragraph_to_md(
    p: &docx_rs::Paragraph,
    links: &HashMap<&str, &str>,
    math_override: Option<&str>,
) -> String {
    let text = if let Some(m) = math_override {
        m.to_string()
    } else {
        children_to_md(&p.children, links)
    };
    if text.trim().is_empty() {
        return String::new();
    }

    if let Some(style) = &p.property.style
        && let Some(level) = heading_level(&style.val)
    {
        return format!("{} {}", "#".repeat(level), text.trim());
    }

    if let Some(num) = &p.property.numbering_property {
        let level = num.level.as_ref().map(|l| l.val).unwrap_or(0);
        return format!("{}- {}", "  ".repeat(level), text.trim());
    }

    text
}

/// Maps Word style ids ("Heading1", "Heading 2", "Title") to markdown levels.
fn heading_level(style_val: &str) -> Option<usize> {
    let s = style_val.to_ascii_lowercase();
    if s == "title" {
        return Some(1);
    }
    s.strip_prefix("heading")
        .and_then(|n| n.trim().parse::<usize>().ok())
        .map(|n| n.clamp(1, 6))
}

fn children_to_md(children: &[docx_rs::ParagraphChild], links: &HashMap<&str, &str>) -> String {
    let mut out = String::new();
    for child in children {
        match child {
            docx_rs::ParagraphChild::Run(run) => out.push_str(&run_to_md(run)),
            docx_rs::ParagraphChild::Hyperlink(h) => {
                let inner = children_to_md(&h.children, links);
                let url = match &h.link {
                    docx_rs::HyperlinkData::External { rid, .. } => {
                        links.get(rid.as_str()).map(|u| u.to_string())
                    }
                    docx_rs::HyperlinkData::Anchor { anchor } => Some(format!("#{}", anchor)),
                };
                match url {
                    Some(url) if !inner.trim().is_empty() => {
                        out.push_str(&format!("[{}]({})", inner.trim(), url));
                    }
                    _ => out.push_str(&inner),
                }
            }
            _ => {}
        }
    }
    out
}

/// A run's text with `**bold**` / `*italic*` markers from its run properties.
fn run_to_md(run: &docx_rs::Run) -> String {
    let mut text = String::new();
    for child in &run.children {
        match child {
            docx_rs::RunChild::Text(t) => text.push_str(&t.text),
            docx_rs::RunChild::Tab(_) => text.push('\t'),
            docx_rs::RunChild::Break(_) => text.push('\n'),
            _ => {}
        }
    }
    let marker = match (
        run.run_property.bold.is_some(),
        run.run_property.italic.is_some(),
    ) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        (false, false) => return text,
    };
    emphasize(&text, marker)
}

/// Wraps only the non-whitespace core in emphasis markers — `**text **` (with
/// an inner space) is not valid markdown emphasis.
fn emphasize(text: &str, marker: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let start = text.find(trimmed).unwrap_or(0);
    let (lead, rest) = text.split_at(start);
    let (core, trail) = rest.split_at(trimmed.len());
    format!("{}{}{}{}{}", lead, marker, core, marker, trail)
}

/// A Word table as a standard markdown pipe table (row 0 is the header row).
fn format_table(t: &docx_rs::Table, math_by_para_id: &HashMap<String, String>) -> String {
    let mut builder = crate::parsers::TableBuilder::default();
    for row_child in t.rows.iter() {
        let docx_rs::TableChild::TableRow(row) = row_child;
        for cell_child in row.cells.iter() {
            let docx_rs::TableRowChild::TableCell(cell) = cell_child;
            builder.push_cell(&extract_cell_text(cell, math_by_para_id), 1);
        }
        builder.end_row();
    }
    builder.render()
}

fn extract_cell_text(
    cell: &docx_rs::TableCell,
    math_by_para_id: &HashMap<String, String>,
) -> String {
    let mut text = String::new();
    for child in &cell.children {
        match child {
            docx_rs::TableCellContent::Paragraph(p) => {
                if let Some(m) = math_by_para_id.get(&p.id) {
                    text.push_str(m);
                } else {
                    text.push_str(&p.raw_text());
                }
                text.push(' ');
            }
            // Markdown has no nested tables, and rendering one as a pipe table
            // inside a cell produced `| a | b |---|---| c |` garbage once the
            // parent flattened its newlines. Flatten to readable text instead.
            docx_rs::TableCellContent::Table(t) => {
                text.push_str(&flatten_nested_table(t, math_by_para_id));
                text.push(' ');
            }
            _ => {}
        }
    }
    text.trim().to_string()
}

/// A table inside a cell, as `a: b; c: d / e: f` — cells joined by `; `, rows
/// by ` / `. Lossy by necessity, but readable and never malformed.
fn flatten_nested_table(t: &docx_rs::Table, math_by_para_id: &HashMap<String, String>) -> String {
    let rows: Vec<String> = t
        .rows
        .iter()
        .map(|row_child| {
            let docx_rs::TableChild::TableRow(row) = row_child;
            row.cells
                .iter()
                .map(|cell_child| {
                    let docx_rs::TableRowChild::TableCell(cell) = cell_child;
                    extract_cell_text(cell, math_by_para_id)
                })
                .filter(|c| !c.trim().is_empty())
                .collect::<Vec<_>>()
                .join("; ")
        })
        .filter(|r| !r.trim().is_empty())
        .collect();
    rows.join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use docx_rs::{Paragraph, Run, Table, TableCell, TableRow};

    #[test]
    fn test_format_table_markdown_pipe() {
        // Construct a table manually
        let t = Table::new(vec![
            TableRow::new(vec![
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Header1"))),
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Header2"))),
            ]),
            TableRow::new(vec![
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Value1"))),
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Value2"))),
            ]),
            TableRow::new(vec![
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Value3"))),
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Value4"))),
            ]),
        ]);

        let math = HashMap::new();
        let result = format_table(&t, &math);
        println!("Table:\n{}", result);
        assert!(result.contains("| Header1 | Header2 |"));
        assert!(result.contains("|---|---|"));
        assert!(result.contains("| Value1 | Value2 |"));
        assert!(result.contains("| Value3 | Value4 |"));
    }

    #[test]
    fn test_office_missing_file_errors() {
        let gobbler = OfficeGobbler;
        let p = Path::new("does_not_exist.docx");
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        assert!(gobbler.gobble(p, &(&args).into()).is_err());
    }

    fn write_word_package(suffix: &str, text: &str) -> tempfile::NamedTempFile {
        let file = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        docx_rs::Docx::new()
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text(text)))
            .build()
            .pack(file.reopen().unwrap())
            .unwrap();
        file
    }

    /// Regression: only an extension of exactly `docx` was parsed, so `.docm`,
    /// `.dotx`, `.dotm`, `.DOCX` and renamed documents were all refused.
    #[test]
    fn word_package_variants_are_parsed() {
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        for suffix in [".docx", ".docm", ".dotx", ".dotm", ".DOCX", ".bin"] {
            let file = write_word_package(suffix, "Goblin minutes");
            let md = OfficeGobbler
                .gobble(file.path(), &(&args).into())
                .unwrap_or_else(|e| panic!("{suffix}: {e:#}"));
            assert!(md.contains("Goblin minutes"), "{suffix}: {md}");
        }
    }

    /// A renamed Word document routes here by its zip entries, not its name.
    #[test]
    fn renamed_word_document_routes_by_content() {
        let file = write_word_package(".bin", "Renamed hoard");
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let (format, md) = crate::route_and_gobble(&file.path().to_string_lossy(), &args);
        assert_eq!(format, crate::format::Format::Docx);
        assert!(md.unwrap().contains("Renamed hoard"));
    }

    #[test]
    fn non_zip_word_file_is_an_error() {
        let mut file = tempfile::Builder::new().suffix(".docx").tempfile().unwrap();
        std::io::Write::write_all(&mut file, b"plain text pretending").unwrap();
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let err = OfficeGobbler
            .gobble(file.path(), &(&args).into())
            .unwrap_err();
        assert!(err.to_string().contains("Not a Word document"), "{err}");
    }

    #[test]
    fn heading_styles_map_to_markdown_levels() {
        assert_eq!(heading_level("Heading1"), Some(1));
        assert_eq!(heading_level("Heading 3"), Some(3));
        assert_eq!(heading_level("Title"), Some(1));
        assert_eq!(heading_level("Heading9"), Some(6));
        assert_eq!(heading_level("Normal"), None);
    }

    #[test]
    fn paragraph_styles_render_as_markdown() {
        let links = HashMap::new();
        let heading = Paragraph::new()
            .add_run(Run::new().add_text("Chapter One"))
            .style("Heading2");
        assert_eq!(paragraph_to_md(&heading, &links, None), "## Chapter One");

        let bold = Paragraph::new().add_run(Run::new().add_text("important").bold());
        assert_eq!(paragraph_to_md(&bold, &links, None), "**important**");

        let listed = Paragraph::new()
            .add_run(Run::new().add_text("first item"))
            .numbering(docx_rs::NumberingId::new(1), docx_rs::IndentLevel::new(0));
        assert_eq!(paragraph_to_md(&listed, &links, None), "- first item");
    }

    #[test]
    fn emphasize_keeps_surrounding_whitespace_outside_markers() {
        assert_eq!(emphasize("word ", "**"), "**word** ");
        assert_eq!(emphasize(" word", "*"), " *word*");
    }

    #[test]
    fn test_office_equations_docx() {
        let mut path = Path::new("test_assets/benchmarks/equations.docx");
        let alt = Path::new("../../test_assets/benchmarks/equations.docx");
        if !path.exists() && alt.exists() {
            path = alt;
        }
        if !path.exists() {
            return;
        }
        let gobbler = OfficeGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let md = gobbler.gobble(path, &(&args).into()).unwrap();
        assert!(md.contains("For $m=1$,"));
        assert!(md.contains(r"$$\sin") || md.contains("$$sin"));
        assert!(md.contains("The angle is $θ="));
        assert!(md.contains("From Eq. 36.1.3,"));
        assert!(md.contains(r"$$\frac{a}{λ}"));
    }

    #[test]
    fn test_office_chart_extraction() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let docx_path = dir.path().join("chart_doc.docx");
        let file = std::fs::File::create(&docx_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#).unwrap();

        zip.start_file("_rels/.rels", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#).unwrap();

        zip.start_file("word/_rels/document.xml.rels", options)
            .unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="charts/chart1.xml"/>
</Relationships>"#).unwrap();

        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Quarterly Summary</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        zip.start_file("word/charts/chart1.xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <c:chart>
    <c:title><c:tx><c:rich><a:p><a:r><a:t>Quarterly Revenue</a:t></a:r></a:p></c:rich></c:tx></c:title>
    <c:plotArea>
      <c:barChart>
        <c:ser>
          <c:tx><c:v>Revenue ($M)</c:v></c:tx>
          <c:cat>
            <c:strRef>
              <c:strData>
                <c:pt idx="0"><c:v>EMEA</c:v></c:pt>
                <c:pt idx="1"><c:v>APAC</c:v></c:pt>
              </c:strData>
            </c:strRef>
          </c:cat>
          <c:val>
            <c:numRef>
              <c:numData>
                <c:pt idx="0"><c:v>32.5</c:v></c:pt>
                <c:pt idx="1"><c:v>18.2</c:v></c:pt>
              </c:numData>
            </c:numRef>
          </c:val>
        </c:ser>
      </c:barChart>
    </c:plotArea>
  </c:chart>
</c:chartSpace>"#).unwrap();

        zip.finish().unwrap();

        let gobbler = OfficeGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let md = gobbler.gobble(&docx_path, &(&args).into()).unwrap();
        assert!(md.contains("Quarterly Summary"));
        assert!(md.contains("### Chart: Quarterly Revenue"));
        assert!(md.contains("| EMEA | 32.5 |"));
        assert!(md.contains("| APAC | 18.2 |"));
    }

    #[test]
    fn test_office_smartart_extraction() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let docx_path = dir.path().join("smartart_doc.docx");
        let file = std::fs::File::create(&docx_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#).unwrap();

        zip.start_file("_rels/.rels", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#).unwrap();

        zip.start_file("word/_rels/document.xml.rels", options)
            .unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramData" Target="diagrams/data1.xml"/>
</Relationships>"#).unwrap();

        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>
    <w:p>
      <w:r><w:t>Project Pipeline Overview</w:t></w:r>
      <dgm:relIds r:dm="rId2"/>
    </w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        zip.start_file("word/diagrams/data1.xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<dgm:dataModel xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <dgm:ptLst>
    <dgm:pt modelId="0" type="doc"/>
    <dgm:pt modelId="1" type="node"><dgm:t><a:p><a:r><a:t>Phase 1: Design</a:t></a:r></a:p></dgm:t></dgm:pt>
    <dgm:pt modelId="2" type="node"><dgm:t><a:p><a:r><a:t>Phase 2: Build</a:t></a:r></a:p></dgm:t></dgm:pt>
    <dgm:pt modelId="3" type="node"><dgm:t><a:p><a:r><a:t>Phase 3: Deploy</a:t></a:r></a:p></dgm:t></dgm:pt>
    <dgm:pt modelId="11" type="sibTrans"/>
    <dgm:pt modelId="12" type="sibTrans"/>
  </dgm:ptLst>
  <dgm:cxnLst>
    <dgm:cxn modelId="20" type="parOf" srcId="0" destId="1" srcOrd="0" destOrd="0"/>
    <dgm:cxn modelId="21" type="parOf" srcId="0" destId="2" srcOrd="1" destOrd="1"/>
    <dgm:cxn modelId="22" type="parOf" srcId="0" destId="3" srcOrd="2" destOrd="2"/>
    <dgm:cxn modelId="23" type="sibTrans" srcId="1" destId="11" srcOrd="0" destOrd="0"/>
    <dgm:cxn modelId="24" type="sibTrans" srcId="11" destId="2" srcOrd="0" destOrd="0"/>
    <dgm:cxn modelId="25" type="sibTrans" srcId="2" destId="12" srcOrd="0" destOrd="0"/>
    <dgm:cxn modelId="26" type="sibTrans" srcId="12" destId="3" srcOrd="0" destOrd="0"/>
  </dgm:cxnLst>
</dgm:dataModel>"#).unwrap();

        zip.finish().unwrap();

        let gobbler = OfficeGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let md = gobbler.gobble(&docx_path, &(&args).into()).unwrap();
        assert!(md.contains("Project Pipeline Overview"));
        assert!(md.contains("```mermaid\nflowchart LR"));
        assert!(md.contains(r#"N_1["Phase 1: Design"] --> N_2["Phase 2: Build"]"#));
        assert!(md.contains(r#"N_2["Phase 2: Build"] --> N_3["Phase 3: Deploy"]"#));
    }
}

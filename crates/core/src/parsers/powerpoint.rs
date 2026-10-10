use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::Result;
use std::path::Path;

pub struct PptxGobbler;

impl Gobble for PptxGobbler {
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> anyhow::Result<String> {
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut slide_xmls: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut slide_chart_rels: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut slide_smartart_rels: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut chart_xmls: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut smartart_xmls: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut notes_texts: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();
        let mut presentation_xml = String::new();
        let mut rels_xml = String::new();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();

            if name == "ppt/presentation.xml" {
                presentation_xml = crate::parsers::read_zip_entry_to_string(&mut file, &name)?;
            } else if name == "ppt/_rels/presentation.xml.rels" {
                rels_xml = crate::parsers::read_zip_entry_to_string(&mut file, &name)?;
            } else if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                let xml_content = crate::parsers::read_zip_entry_to_string(&mut file, &name)?;
                slide_xmls.insert(name, xml_content);
            } else if name.starts_with("ppt/slides/_rels/slide") && name.ends_with(".xml.rels") {
                let rels_content = crate::parsers::read_zip_entry_to_string(&mut file, &name)?;
                let slide_name = name
                    .replace("/_rels/", "/")
                    .trim_end_matches(".rels")
                    .to_string();
                let chart_targets = extract_chart_targets_from_rels(&rels_content);
                slide_chart_rels.insert(slide_name.clone(), chart_targets);
                let diagram_targets = extract_diagram_targets_from_rels(&rels_content);
                slide_smartart_rels.insert(slide_name, diagram_targets);
            } else if name.starts_with("ppt/charts/chart") && name.ends_with(".xml") {
                let xml_content = crate::parsers::read_zip_entry_to_string(&mut file, &name)?;
                chart_xmls.insert(name, xml_content);
            } else if name.starts_with("ppt/diagrams/data") && name.ends_with(".xml") {
                let xml_content = crate::parsers::read_zip_entry_to_string(&mut file, &name)?;
                smartart_xmls.insert(name, xml_content);
            } else if name.starts_with("ppt/notesSlides/notesSlide") && name.ends_with(".xml") {
                let num_str = name
                    .trim_start_matches("ppt/notesSlides/notesSlide")
                    .trim_end_matches(".xml");
                if let Ok(n) = num_str.parse::<usize>() {
                    let xml_content = crate::parsers::read_zip_entry_to_string(&mut file, &name)?;
                    // Notes slides carry a slide-number placeholder; digit-only
                    // lines are that artifact, not the speaker's words.
                    let text = extract_text_from_slide_xml(&xml_content)?
                        .lines()
                        .filter(|l| {
                            !l.trim()
                                .trim_start_matches("- ")
                                .chars()
                                .all(|c| c.is_ascii_digit())
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.trim().is_empty() {
                        notes_texts.insert(n, text);
                    }
                }
            }
        }

        let mut slide_texts: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (name, xml_content) in &slide_xmls {
            let text = extract_text_from_slide_xml(xml_content)?;
            let has_charts = slide_chart_rels.get(name).is_some_and(|c| !c.is_empty());
            let has_smartart = slide_smartart_rels.get(name).is_some_and(|s| !s.is_empty());
            if !text.trim().is_empty() || has_charts || has_smartart {
                slide_texts.insert(name.clone(), text);
            }
        }

        // True slide order comes from the presentation manifest (sldIdLst → rels
        // targets); filename numbers are only a fallback, since decks that were
        // reordered in PowerPoint keep their original file names.
        let mut ordered = manifest_slide_order(&presentation_xml, &rels_xml);
        ordered.retain(|name| slide_texts.contains_key(name));
        let mut leftovers: Vec<String> = slide_texts
            .keys()
            .filter(|k| !ordered.contains(k))
            .cloned()
            .collect();
        leftovers.sort_by_key(|name| slide_file_number(name).unwrap_or(usize::MAX));
        ordered.extend(leftovers);

        let mut used_charts: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut used_smartart: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut output = String::new();

        for (pos, name) in ordered.iter().enumerate() {
            let content = &slide_texts[name];
            let mut slide_body = content.trim().to_string();

            // Append charts for this slide
            if let Some(charts) = slide_chart_rels.get(name) {
                for chart_path in charts {
                    if let Some(chart_xml) = chart_xmls.get(chart_path) {
                        used_charts.insert(chart_path.clone());
                        if let Ok(chart) =
                            crate::parsers::openxml_chart::OpenXmlChart::parse(chart_xml)
                        {
                            let chart_md = chart.render_markdown();
                            if !chart_md.is_empty() {
                                if !slide_body.is_empty() {
                                    slide_body.push_str("\n\n");
                                }
                                slide_body.push_str(&chart_md);
                            }
                        }
                    }
                }
            }

            // Append SmartArt diagrams for this slide
            if let Some(diagrams) = slide_smartart_rels.get(name) {
                for dgm_path in diagrams {
                    if let Some(dgm_xml) = smartart_xmls.get(dgm_path) {
                        used_smartart.insert(dgm_path.clone());
                        if let Ok(diag) = crate::parsers::smartart::SmartArtDiagram::parse(dgm_xml)
                        {
                            if let Some(mermaid) = diag.render_mermaid() {
                                if !slide_body.is_empty() {
                                    slide_body.push_str("\n\n");
                                }
                                slide_body.push_str("```mermaid\n");
                                slide_body.push_str(&mermaid);
                                slide_body.push_str("\n```");
                            }
                        }
                    }
                }
            }

            output.push_str(&format!("## Slide {}\n\n{}", pos + 1, slide_body));
            if let Some(notes) = slide_file_number(name).and_then(|n| notes_texts.get(&n)) {
                output.push_str(&format!("\n\n**Speaker notes:**\n{}", notes));
            }
            output.push_str("\n\n---\n\n");
        }

        // If any charts in the presentation were not attached to specific slides, append them
        for (chart_path, chart_xml) in &chart_xmls {
            if !used_charts.contains(chart_path) {
                if let Ok(chart) = crate::parsers::openxml_chart::OpenXmlChart::parse(chart_xml) {
                    let chart_md = chart.render_markdown();
                    if !chart_md.is_empty() {
                        output.push_str(&format!("{}\n\n---\n\n", chart_md));
                    }
                }
            }
        }

        // If any SmartArt in the presentation were not attached to specific slides, append them
        for (dgm_path, dgm_xml) in &smartart_xmls {
            if !used_smartart.contains(dgm_path) {
                if let Ok(diag) = crate::parsers::smartart::SmartArtDiagram::parse(dgm_xml) {
                    if let Some(mermaid) = diag.render_mermaid() {
                        output.push_str(&format!("```mermaid\n{}\n```\n\n---\n\n", mermaid));
                    }
                }
            }
        }

        if output.trim().is_empty() {
            anyhow::bail!(
                "No text could be extracted from this presentation \
                 (image-only slides, or text stored outside the slide parts)"
            );
        }

        Ok(output)
    }
}

/// The `N` in `ppt/slides/slideN.xml` (notes files use the same number).
fn slide_file_number(name: &str) -> Option<usize> {
    name.trim_start_matches("ppt/slides/slide")
        .trim_end_matches(".xml")
        .parse()
        .ok()
}

/// Reads `<p:sldIdLst>` from presentation.xml (r:id order) and resolves each id
/// through presentation.xml.rels to a slide part name. Returns full zip paths
/// (`ppt/slides/slideN.xml`) in presentation order; empty when either part is
/// missing or unparsable.
fn manifest_slide_order(presentation_xml: &str, rels_xml: &str) -> Vec<String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    // rels: Id → Target
    let mut id_to_target: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut reader = Reader::from_str(rels_xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if e.local_name().as_ref() == b"Relationship" =>
            {
                let mut id = None;
                let mut target = None;
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"Id" => id = attr.unescape_value().ok().map(|v| v.into_owned()),
                        b"Target" => target = attr.unescape_value().ok().map(|v| v.into_owned()),
                        _ => {}
                    }
                }
                if let (Some(id), Some(target)) = (id, target) {
                    id_to_target.insert(id, target);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    // presentation.xml: sldId r:id attributes, in document order
    let mut order = Vec::new();
    let mut reader = Reader::from_str(presentation_xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if e.local_name().as_ref() == b"sldId" =>
            {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref().ends_with(b"id")
                        && let Ok(rid) = attr.unescape_value()
                        && let Some(target) = id_to_target.get(rid.as_ref())
                    {
                        // Targets are relative to ppt/ ("slides/slide1.xml").
                        let full = format!("ppt/{}", target.trim_start_matches('/'));
                        order.push(full);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    order
}

fn extract_text_from_slide_xml(xml: &str) -> Result<String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let diagram = crate::parsers::drawingml::SlideDiagram::parse(xml).ok();
    let connected_ids = diagram
        .as_ref()
        .map(|d| d.connected_shape_ids())
        .unwrap_or_default();

    let mut reader = Reader::from_str(xml);
    // NOT trim_text(true): it trims each text event, and entity refs split a
    // run into several, eating the spaces around them ("R&amp;D x" -> "R&Dx").
    // Text is only collected inside <a:t>, so untrimmed events are safe here.
    reader.config_mut().trim_text(false);

    let mut output = String::new();
    let mut in_text_node = false;
    let mut image_descriptions: Vec<String> = Vec::new();

    let mut in_table = false;
    let mut table = crate::parsers::TableBuilder::default();
    let mut current_cell_text = String::new();

    let mut is_connected_shape = false;

    loop {
        let event = reader.read_event().map_err(|e| {
            anyhow::anyhow!(
                "Malformed slide XML at byte {}: {}",
                reader.buffer_position(),
                e
            )
        })?;
        let is_empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"sp" => {
                        is_connected_shape = false;
                    }
                    b"cNvPr" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"id"
                                && let Ok(val) = attr.unescape_value()
                            {
                                let id = val.trim().to_string();
                                if connected_ids.contains(&id) {
                                    is_connected_shape = true;
                                }
                            }
                            if (attr.key.as_ref() == b"descr" || attr.key.as_ref() == b"title")
                                && let Ok(val) = attr.unescape_value()
                            {
                                let d = val.trim().to_string();
                                if !d.is_empty()
                                    && !d.ends_with(".png")
                                    && !d.ends_with(".jpg")
                                    && !d.ends_with(".jpeg")
                                    && !d.ends_with(".svg")
                                    && !image_descriptions.contains(&d)
                                {
                                    image_descriptions.push(d);
                                }
                            }
                        }
                    }
                    b"a:tbl" | b"tbl" => {
                        in_table = true;
                        output.push('\n'); // Pad before table
                    }
                    // A self-closing cell still occupies a column.
                    b"a:tc" | b"tc" if in_table => {
                        current_cell_text.clear();
                        if is_empty {
                            table.push_cell("", 1);
                        }
                    }
                    b"a:t" | b"t" => in_text_node = true,
                    b"a:p" | b"p" if !in_table && !is_connected_shape => {
                        output.push_str("\n- ");
                    }
                    _ => (),
                }
            }
            Event::Text(ref e) => {
                if in_text_node
                    && let Ok(text) = std::str::from_utf8(e.as_ref())
                    && let Ok(decoded) = quick_xml::escape::unescape(text)
                {
                    if in_table {
                        current_cell_text.push_str(&decoded);
                    } else if !is_connected_shape {
                        output.push_str(&decoded);
                    }
                }
            }
            // CDATA is rare in slide parts but free to honour; without this the
            // text inside one would vanish with no warning.
            Event::CData(ref e) => {
                if in_text_node {
                    let text = String::from_utf8_lossy(e.as_ref());
                    if in_table {
                        current_cell_text.push_str(&text);
                    } else if !is_connected_shape {
                        output.push_str(&text);
                    }
                }
            }
            Event::GeneralRef(ref e) => {
                if in_text_node && let Some(s) = crate::parsers::resolve_xml_entity(e) {
                    if in_table {
                        current_cell_text.push_str(&s);
                    } else if !is_connected_shape {
                        output.push_str(&s);
                    }
                }
            }
            Event::End(ref e) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"a:tbl" | b"tbl" => {
                        in_table = false;
                        output.push('\n');
                        output.push_str(table.render().trim_end());
                        output.push('\n');
                    }
                    b"a:tr" | b"tr" if in_table => table.end_row(),
                    b"a:tc" | b"tc" if in_table => {
                        table.push_cell(&current_cell_text, 1);
                        current_cell_text.clear();
                    }
                    b"a:t" | b"t" => in_text_node = false,
                    b"a:p" | b"p" if !in_table && !is_connected_shape => {
                        output.push('\n');
                    }
                    b"sp" => {
                        is_connected_shape = false;
                    }
                    _ => (),
                }
            }
            Event::Eof => break,
            _ => (),
        }
    }

    if output.trim().is_empty() && !image_descriptions.is_empty() {
        for desc in &image_descriptions {
            output.push_str(&format!("\n- [Image: {}]", desc));
        }
    }

    // Clean up excessive newlines/bullets generated from empty paragraphs
    let cleaned = output
        .lines()
        .filter(|line| !line.trim().is_empty() && line.trim() != "-")
        .collect::<Vec<&str>>()
        .join("\n");

    let mut result = cleaned;
    if let Some(ref diag) = diagram
        && let Some(mermaid) = diag.render_mermaid()
    {
        if !result.trim().is_empty() {
            result.push_str("\n\n");
        }
        result.push_str("```mermaid\n");
        result.push_str(&mermaid);
        result.push_str("\n```");
    }

    Ok(result)
}

fn extract_chart_targets_from_rels(rels_xml: &str) -> Vec<String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut targets = Vec::new();
    let mut reader = Reader::from_str(rels_xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if e.local_name().as_ref() == b"Relationship" =>
            {
                let mut is_chart = false;
                let mut target: Option<String> = None;
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"Type" => {
                            if let Ok(val) = attr.unescape_value() {
                                if val.ends_with("/chart") {
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
                if is_chart && let Some(t) = target {
                    let normalized = if let Some(stripped) = t.strip_prefix("../") {
                        format!("ppt/{}", stripped)
                    } else if let Some(stripped) = t.strip_prefix('/') {
                        stripped.to_string()
                    } else {
                        format!("ppt/charts/{}", t.trim_start_matches("charts/"))
                    };
                    targets.push(normalized);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    targets
}

fn extract_diagram_targets_from_rels(rels_xml: &str) -> Vec<String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut targets = Vec::new();
    let mut reader = Reader::from_str(rels_xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if e.local_name().as_ref() == b"Relationship" =>
            {
                let mut is_diagram = false;
                let mut target: Option<String> = None;
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"Type" => {
                            if let Ok(val) = attr.unescape_value() {
                                if val.ends_with("/diagramData") {
                                    is_diagram = true;
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
                if is_diagram && let Some(t) = target {
                    let normalized = if let Some(stripped) = t.strip_prefix("../") {
                        format!("ppt/{}", stripped)
                    } else if let Some(stripped) = t.strip_prefix('/') {
                        stripped.to_string()
                    } else {
                        format!("ppt/diagrams/{}", t.trim_start_matches("diagrams/"))
                    };
                    targets.push(normalized);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_powerpoint_table_extraction() {
        let xml = r#"
            <a:p><a:r><a:t>Regular Text</a:t></a:r></a:p>
            <a:tbl>
                <a:tr>
                    <a:tc><a:txBody><a:p><a:r><a:t>Header1</a:t></a:r></a:p></a:txBody></a:tc>
                    <a:tc><a:txBody><a:p><a:r><a:t>Header2</a:t></a:r></a:p></a:txBody></a:tc>
                </a:tr>
                <a:tr>
                    <a:tc><a:txBody><a:p><a:r><a:t>Value1</a:t></a:r></a:p></a:txBody></a:tc>
                    <a:tc><a:txBody><a:p><a:r><a:t>Value2</a:t></a:r></a:p></a:txBody></a:tc>
                </a:tr>
            </a:tbl>
        "#;

        let result = extract_text_from_slide_xml(xml).unwrap();
        assert!(result.contains("Regular Text"));
        assert!(result.contains("| Header1 | Header2 |"));
        assert!(result.contains("|---|---|"));
        assert!(result.contains("| Value1 | Value2 |"));
    }

    /// A self-closing `<a:tc/>` still holds its column, and a short row is
    /// padded rather than shifting its values left.
    #[test]
    fn empty_slide_table_cells_keep_their_column() {
        let xml = r#"<a:tbl>
                <a:tr>
                    <a:tc><a:txBody><a:p><a:r><a:t>H1</a:t></a:r></a:p></a:txBody></a:tc>
                    <a:tc><a:txBody><a:p><a:r><a:t>H2</a:t></a:r></a:p></a:txBody></a:tc>
                    <a:tc><a:txBody><a:p><a:r><a:t>H3</a:t></a:r></a:p></a:txBody></a:tc>
                </a:tr>
                <a:tr>
                    <a:tc><a:txBody><a:p><a:r><a:t>left</a:t></a:r></a:p></a:txBody></a:tc>
                    <a:tc/>
                    <a:tc><a:txBody><a:p><a:r><a:t>right</a:t></a:r></a:p></a:txBody></a:tc>
                </a:tr>
            </a:tbl>"#;
        let result = extract_text_from_slide_xml(xml).unwrap();
        assert!(result.contains("| left |  | right |"), "got: {result}");
    }

    /// Malformed slide XML is an error, not a silently truncated slide.
    #[test]
    fn malformed_slide_xml_fails_loudly() {
        let xml = r#"<a:p><a:r><a:t>Good</a:t></a:r></a:p><a:p><<<"#;
        let err = extract_text_from_slide_xml(xml).unwrap_err();
        assert!(
            err.to_string().contains("Malformed slide XML"),
            "got: {err}"
        );
    }

    /// Entities are separate events in quick-xml 0.39; without handling them
    /// an `&amp;` in slide text disappeared.
    #[test]
    fn entities_in_slide_text_survive() {
        let xml = r#"<a:p><a:r><a:t>R&amp;D &lt;strategy&gt;</a:t></a:r></a:p>"#;
        let result = extract_text_from_slide_xml(xml).unwrap();
        assert!(result.contains("R&D <strategy>"), "got: {result}");
    }

    #[test]
    fn manifest_order_beats_filename_order() {
        let rels = r#"<Relationships>
            <Relationship Id="rId2" Target="slides/slide2.xml"/>
            <Relationship Id="rId1" Target="slides/slide1.xml"/>
        </Relationships>"#;
        // The deck was reordered: slide2 is presented first.
        let pres = r#"<p:presentation xmlns:p="x" xmlns:r="y"><p:sldIdLst>
            <p:sldId id="257" r:id="rId2"/>
            <p:sldId id="256" r:id="rId1"/>
        </p:sldIdLst></p:presentation>"#;
        assert_eq!(
            manifest_slide_order(pres, rels),
            vec!["ppt/slides/slide2.xml", "ppt/slides/slide1.xml"]
        );
    }

    #[test]
    fn test_powerpoint_connector_flowchart() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path =
            Path::new(&manifest_dir).join("../../test_assets/benchmarks/connector_flowchart.pptx");
        if path.exists() {
            let gobbler = PptxGobbler;
            let md = gobbler
                .gobble(&path, &ParseOptions::default())
                .expect("parse connector flowchart pptx");
            assert!(md.contains("```mermaid\nflowchart TD"));
            assert!(md.contains(r#"N_101["Ingest Input"] --> N_102["Validate Schema"]"#));
            assert!(md.contains(r#"N_102 -.-> N_103["Store Record"]"#));
        }
    }

    #[test]
    fn test_powerpoint_smartart_process() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path =
            Path::new(&manifest_dir).join("../../test_assets/benchmarks/smartart_process.pptx");
        if path.exists() {
            let gobbler = PptxGobbler;
            let md = gobbler
                .gobble(&path, &ParseOptions::default())
                .expect("parse smartart process pptx");
            assert!(md.contains("```mermaid\nflowchart LR"));
            assert!(md.contains(
                r#"N_1["Phase 1: Research"] --> N_2["Phase 2: Prototype"] --> N_3["Phase 3: Production"]"#
            ));
        }
    }
}

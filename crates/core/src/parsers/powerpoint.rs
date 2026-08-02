use crate::parsers::gobble::{Gobble, ParseOptions};
use std::io::Read;
use std::path::Path;

pub struct PptxGobbler;

impl Gobble for PptxGobbler {
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> anyhow::Result<String> {
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut slide_texts: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut notes_texts: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();
        let mut presentation_xml = String::new();
        let mut rels_xml = String::new();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();

            if name == "ppt/presentation.xml" {
                file.read_to_string(&mut presentation_xml)?;
            } else if name == "ppt/_rels/presentation.xml.rels" {
                file.read_to_string(&mut rels_xml)?;
            } else if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                let mut xml_content = String::new();
                file.read_to_string(&mut xml_content)?;
                let text = extract_text_from_slide_xml(&xml_content);
                if !text.trim().is_empty() {
                    slide_texts.insert(name, text);
                }
            } else if name.starts_with("ppt/notesSlides/notesSlide") && name.ends_with(".xml") {
                let num_str = name
                    .trim_start_matches("ppt/notesSlides/notesSlide")
                    .trim_end_matches(".xml");
                if let Ok(n) = num_str.parse::<usize>() {
                    let mut xml_content = String::new();
                    file.read_to_string(&mut xml_content)?;
                    // Notes slides carry a slide-number placeholder; digit-only
                    // lines are that artifact, not the speaker's words.
                    let text = extract_text_from_slide_xml(&xml_content)
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

        let mut output = String::new();
        for (pos, name) in ordered.iter().enumerate() {
            let content = &slide_texts[name];
            output.push_str(&format!("## Slide {}\n\n{}", pos + 1, content));
            if let Some(notes) = slide_file_number(name).and_then(|n| notes_texts.get(&n)) {
                output.push_str(&format!("\n\n**Speaker notes:**\n{}", notes));
            }
            output.push_str("\n\n---\n\n");
        }

        if output.is_empty() {
            return Ok("No text could be extracted from this presentation.".to_string());
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

fn extract_text_from_slide_xml(xml: &str) -> String {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    // NOT trim_text(true): it trims each text event, and entity refs split a
    // run into several, eating the spaces around them ("R&amp;D x" -> "R&Dx").
    // Text is only collected inside <a:t>, so untrimmed events are safe here.
    reader.config_mut().trim_text(false);

    let mut output = String::new();
    let mut in_text_node = false;

    // Table state trackers (tables render as markdown pipe tables, row 0 = header)
    let mut in_table = false;
    let mut current_row_idx = 0;
    let mut current_cell_text = String::new();
    let mut row_cells: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                match e.name().as_ref() {
                    b"a:tbl" => {
                        in_table = true;
                        current_row_idx = 0;
                        output.push('\n'); // Pad before table
                    }
                    b"a:tr" if in_table => {
                        row_cells.clear();
                    }
                    b"a:tc" if in_table => {
                        current_cell_text.clear();
                    }
                    b"a:t" => in_text_node = true,
                    b"a:p" if !in_table => output.push_str("\n- "), // Start of a paragraph/bullet outside tables
                    _ => (),
                }
            }
            Ok(Event::Text(e)) => {
                if in_text_node
                    && let Ok(text) = std::str::from_utf8(e.as_ref())
                    && let Ok(decoded) = quick_xml::escape::unescape(text)
                {
                    if in_table {
                        current_cell_text.push_str(&decoded);
                    } else {
                        output.push_str(&decoded);
                    }
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if in_text_node && let Some(s) = crate::parsers::resolve_xml_entity(&e) {
                    if in_table {
                        current_cell_text.push_str(&s);
                    } else {
                        output.push_str(&s);
                    }
                }
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"a:tbl" => {
                    in_table = false;
                    output.push('\n'); // Pad after table
                }
                b"a:tr" if in_table => {
                    output.push_str(&format!("\n| {} |", row_cells.join(" | ")));
                    if current_row_idx == 0 {
                        output.push_str(&format!("\n|{}", "---|".repeat(row_cells.len())));
                    }
                    current_row_idx += 1;
                }
                b"a:tc" if in_table => {
                    row_cells.push(
                        current_cell_text
                            .trim()
                            .replace('|', "\\|")
                            .replace('\n', " "),
                    );
                }
                b"a:t" => in_text_node = false,
                b"a:p" if !in_table => output.push('\n'),
                _ => (),
            },
            Ok(Event::Eof) => break,
            Err(_) => break, // Silently ignore malformed XML chunks
            _ => (),
        }
    }

    // Clean up excessive newlines/bullets generated from empty paragraphs
    output
        .lines()
        .filter(|line| !line.trim().is_empty() && line.trim() != "-")
        .collect::<Vec<&str>>()
        .join("\n")
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

        let result = extract_text_from_slide_xml(xml);
        assert!(result.contains("Regular Text"));
        assert!(result.contains("| Header1 | Header2 |"));
        assert!(result.contains("|---|---|"));
        assert!(result.contains("| Value1 | Value2 |"));
    }

    /// Entities are separate events in quick-xml 0.39; without handling them
    /// an `&amp;` in slide text disappeared.
    #[test]
    fn entities_in_slide_text_survive() {
        let xml = r#"<a:p><a:r><a:t>R&amp;D &lt;strategy&gt;</a:t></a:r></a:p>"#;
        let result = extract_text_from_slide_xml(xml);
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
}

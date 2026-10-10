use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Clone)]
struct OmmlNode {
    tag: String,
    text: String,
    children: Vec<OmmlNode>,
}

/// Converts an OMML XML snippet (e.g. `<m:oMath>...</m:oMath>`) into a LaTeX string.
pub fn omml_to_latex(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut nodes = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let child = parse_node(&mut reader, &tag);
                nodes.push(child);
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                nodes.push(OmmlNode {
                    tag,
                    text: String::new(),
                    children: Vec::new(),
                });
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    nodes
        .iter()
        .map(node_to_latex)
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

fn parse_node(reader: &mut Reader<&[u8]>, start_tag: &str) -> OmmlNode {
    let mut node = OmmlNode {
        tag: start_tag.to_string(),
        text: String::new(),
        children: Vec::new(),
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let child = parse_node(reader, &tag);
                node.children.push(child);
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                node.children.push(OmmlNode {
                    tag,
                    text: String::new(),
                    children: Vec::new(),
                });
            }
            Ok(Event::Text(ref e)) => {
                if let Ok(t) = std::str::from_utf8(e.as_ref())
                    && let Ok(unescaped) = quick_xml::escape::unescape(t)
                {
                    node.text.push_str(&unescaped);
                }
            }
            Ok(Event::CData(ref e)) => {
                let text = String::from_utf8_lossy(e.as_ref());
                node.text.push_str(&text);
            }
            Ok(Event::End(ref e)) => {
                if e.local_name().as_ref() == start_tag.as_bytes() {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    node
}

fn node_to_latex(node: &OmmlNode) -> String {
    match node.tag.as_str() {
        "t" => node.text.clone(),
        "f" => {
            let num = node
                .children
                .iter()
                .find(|c| c.tag == "num")
                .map(node_to_latex)
                .unwrap_or_default();
            let den = node
                .children
                .iter()
                .find(|c| c.tag == "den")
                .map(node_to_latex)
                .unwrap_or_default();
            format!("\\frac{{{}}}{{{}}}", num.trim(), den.trim())
        }
        "num" | "den" | "e" | "sup" | "sub" => node
            .children
            .iter()
            .map(node_to_latex)
            .collect::<Vec<_>>()
            .join(""),
        "sSup" => {
            let base = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            let sup = node
                .children
                .iter()
                .find(|c| c.tag == "sup")
                .map(node_to_latex)
                .unwrap_or_default();
            format!("{}^{{{}}}", base.trim(), sup.trim())
        }
        "sSub" => {
            let base = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            let sub = node
                .children
                .iter()
                .find(|c| c.tag == "sub")
                .map(node_to_latex)
                .unwrap_or_default();
            format!("{}_{{{}}}", base.trim(), sub.trim())
        }
        "sSubSup" => {
            let base = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            let sub = node
                .children
                .iter()
                .find(|c| c.tag == "sub")
                .map(node_to_latex)
                .unwrap_or_default();
            let sup = node
                .children
                .iter()
                .find(|c| c.tag == "sup")
                .map(node_to_latex)
                .unwrap_or_default();
            format!("{}_{{{}}}^{{{}}}", base.trim(), sub.trim(), sup.trim())
        }
        "d" => {
            let e = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            format!("({})", e.trim())
        }
        "rad" => {
            let deg = node
                .children
                .iter()
                .find(|c| c.tag == "deg")
                .map(node_to_latex);
            let e = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            if let Some(d) = deg {
                let dt = d.trim();
                if !dt.is_empty() {
                    return format!("\\sqrt[{}]{{{}}}", dt, e.trim());
                }
            }
            format!("\\sqrt{{{}}}", e.trim())
        }
        "nary" => {
            let sub = node
                .children
                .iter()
                .find(|c| c.tag == "sub")
                .map(node_to_latex)
                .unwrap_or_default();
            let sup = node
                .children
                .iter()
                .find(|c| c.tag == "sup")
                .map(node_to_latex)
                .unwrap_or_default();
            let e = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            if !sub.trim().is_empty() && !sup.trim().is_empty() {
                format!(
                    "\\sum_{{{}}}^{{{}}}{{{}}}",
                    sub.trim(),
                    sup.trim(),
                    e.trim()
                )
            } else {
                e
            }
        }
        _ => node
            .children
            .iter()
            .map(node_to_latex)
            .collect::<Vec<_>>()
            .join(""),
    }
}

/// Parses a `<w:p>` XML string and extracts full text including inline/display math.
pub fn extract_paragraph_with_math(p_xml: &str) -> String {
    let mut reader = Reader::from_str(p_xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if tag == "oMath" {
                    let node = parse_node(&mut reader, "oMath");
                    let latex = node_to_latex(&node);
                    let trimmed = latex.trim();
                    if !trimmed.is_empty() {
                        out.push_str(&format!("${}$", trimmed));
                    }
                } else if tag == "oMathPara" {
                    let node = parse_node(&mut reader, "oMathPara");
                    let latex = node_to_latex(&node);
                    let trimmed = latex.trim();
                    if !trimmed.is_empty() {
                        out.push_str(&format!("$${}$$", trimmed));
                    }
                } else if tag == "t" {
                    // Standard w:t text run
                    let mut text = String::new();
                    loop {
                        match reader.read_event() {
                            Ok(Event::Text(ref t)) => {
                                if let Ok(s) = std::str::from_utf8(t.as_ref())
                                    && let Ok(unescaped) = quick_xml::escape::unescape(s)
                                {
                                    text.push_str(&unescaped);
                                }
                            }
                            Ok(Event::End(ref end_e)) if end_e.local_name().as_ref() == b"t" => {
                                break;
                            }
                            Ok(Event::Eof) | Err(_) => break,
                            _ => {}
                        }
                    }
                    out.push_str(&text);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out.trim().to_string()
}

#[derive(Debug, Clone, Default)]
pub struct DocxBodyParagraph {
    pub id: Option<String>,
    pub text: String,
    pub has_math: bool,
    pub chart_rids: Vec<String>,
}

/// Extracts direct body paragraphs from a Word document.xml, along with any chart references and math formulas.
/// Also returns a map of paraId -> math-augmented text for any paragraphs (including those inside tables).
pub fn extract_docx_body_paragraphs(
    doc_xml: &str,
) -> (
    Vec<DocxBodyParagraph>,
    std::collections::HashMap<String, String>,
) {
    let mut reader = Reader::from_str(doc_xml);
    reader.config_mut().trim_text(false);

    let mut body_paragraphs = Vec::new();
    let mut math_by_para_id = std::collections::HashMap::new();

    let mut current_p = DocxBodyParagraph::default();
    let mut in_p = false;
    let mut in_body = false;
    let mut tbl_depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"body" => in_body = true,
                    b"tbl" => tbl_depth += 1,
                    b"p" if in_body => {
                        in_p = true;
                        current_p = DocxBodyParagraph::default();
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"paraId" {
                                if let Ok(val) = attr.unescape_value() {
                                    current_p.id = Some(val.to_string());
                                }
                            }
                        }
                    }
                    b"chart" if in_p => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"id" {
                                if let Ok(val) = attr.unescape_value() {
                                    current_p.chart_rids.push(val.to_string());
                                }
                            }
                        }
                    }
                    b"oMath" if in_p => {
                        current_p.has_math = true;
                        let node = parse_node(&mut reader, "oMath");
                        let latex = node_to_latex(&node);
                        let trimmed = latex.trim();
                        if !trimmed.is_empty() {
                            current_p.text.push_str(&format!("${}$", trimmed));
                        }
                    }
                    b"oMathPara" if in_p => {
                        current_p.has_math = true;
                        let node = parse_node(&mut reader, "oMathPara");
                        let latex = node_to_latex(&node);
                        let trimmed = latex.trim();
                        if !trimmed.is_empty() {
                            current_p.text.push_str(&format!("$${}$$", trimmed));
                        }
                    }
                    b"t" if in_p => {
                        let mut text = String::new();
                        loop {
                            match reader.read_event() {
                                Ok(Event::Text(ref t)) => {
                                    if let Ok(s) = std::str::from_utf8(t.as_ref())
                                        && let Ok(unescaped) = quick_xml::escape::unescape(s)
                                    {
                                        text.push_str(&unescaped);
                                    }
                                }
                                Ok(Event::End(ref end_e))
                                    if end_e.local_name().as_ref() == b"t" =>
                                {
                                    break;
                                }
                                Ok(Event::Eof) | Err(_) => break,
                                _ => {}
                            }
                        }
                        current_p.text.push_str(&text);
                    }
                    b"tab" if in_p => {
                        current_p.text.push('\t');
                    }
                    b"br" if in_p => {
                        current_p.text.push('\n');
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"p" && in_body && tbl_depth == 0 {
                    let mut empty_p = DocxBodyParagraph::default();
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == b"paraId" {
                            if let Ok(val) = attr.unescape_value() {
                                empty_p.id = Some(val.to_string());
                            }
                        }
                    }
                    body_paragraphs.push(empty_p);
                } else if in_p {
                    if local.as_ref() == b"chart" {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"id" {
                                if let Ok(val) = attr.unescape_value() {
                                    current_p.chart_rids.push(val.to_string());
                                }
                            }
                        }
                    } else if local.as_ref() == b"tab" {
                        current_p.text.push('\t');
                    } else if local.as_ref() == b"br" {
                        current_p.text.push('\n');
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"body" => in_body = false,
                    b"tbl" => tbl_depth = tbl_depth.saturating_sub(1),
                    b"p" if in_p => {
                        in_p = false;
                        if current_p.has_math {
                            if let Some(ref id) = current_p.id {
                                math_by_para_id.insert(id.clone(), current_p.text.clone());
                            }
                        }
                        if tbl_depth == 0 {
                            body_paragraphs.push(current_p.clone());
                        }
                        current_p = DocxBodyParagraph::default();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    (body_paragraphs, math_by_para_id)
}

/// Extracts all paragraphs from a Word document.xml, including inline and display math.
pub fn extract_all_paragraphs_from_document_xml(doc_xml: &str) -> Vec<String> {
    let (paragraphs, _) = extract_docx_body_paragraphs(doc_xml);
    paragraphs
        .into_iter()
        .map(|p| p.text.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_omml_fraction_and_superscript() {
        let xml = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
            <m:f>
                <m:num><m:r><m:t>a</m:t></m:r></m:num>
                <m:den><m:r><m:t>λ</m:t></m:r></m:den>
            </m:f>
            <m:r><m:t>=</m:t></m:r>
            <m:sSup>
                <m:e><m:r><m:t>10</m:t></m:r></m:e>
                <m:sup><m:r><m:t>-4</m:t></m:r></m:sup>
            </m:sSup>
        </m:oMath>"#;

        let latex = omml_to_latex(xml);
        assert_eq!(latex, r"\frac{a}{λ}=10^{-4}");
    }

    #[test]
    fn test_extract_paragraph_with_math() {
        let xml = r#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
            <w:r><w:t>The angle is </w:t></w:r>
            <m:oMath>
                <m:r><m:t>θ</m:t></m:r>
                <m:r><m:t>=</m:t></m:r>
                <m:r><m:t>2.2</m:t></m:r>
            </m:oMath>
            <w:r><w:t> rad.</w:t></w:r>
        </w:p>"#;

        let result = extract_paragraph_with_math(xml);
        assert_eq!(result, "The angle is $θ=2.2$ rad.");
    }

    #[test]
    fn test_extract_all_paragraphs_from_document_xml() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
            <w:body>
                <w:p><w:r><w:t>First line</w:t></w:r></w:p>
                <w:p><m:oMathPara><m:oMath><m:r><m:t>E=mc2</m:t></m:r></m:oMath></m:oMathPara></w:p>
            </w:body>
        </w:document>"#;

        let ps = extract_all_paragraphs_from_document_xml(xml);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0], "First line");
        assert_eq!(ps[1], "$$E=mc2$$");
    }

    #[test]
    fn test_equations_docx() {
        let mut path = std::path::Path::new("test_assets/benchmarks/equations.docx");
        let alt_path = std::path::Path::new("../../test_assets/benchmarks/equations.docx");
        if !path.exists() && alt_path.exists() {
            path = alt_path;
        }
        if !path.exists() {
            return;
        }
        let file = std::fs::File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut doc_file = archive.by_name("word/document.xml").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut doc_file, &mut xml).unwrap();

        let ps = extract_all_paragraphs_from_document_xml(&xml);
        for (i, p) in ps.iter().enumerate() {
            println!("P{}: {}", i, p);
        }
        assert_eq!(ps.len(), 5);
        assert!(ps[0].contains("$m=1$"));
        assert!(ps[1].starts_with("$$sin"));
    }
}

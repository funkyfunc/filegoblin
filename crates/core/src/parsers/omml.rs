use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
struct OmmlNode {
    tag: String,
    attrs: HashMap<String, String>,
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
                let mut attrs = HashMap::new();
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                    if let Ok(val) = attr.unescape_value() {
                        attrs.insert(key, val.into_owned());
                    }
                }
                let child = parse_node(&mut reader, &tag, attrs);
                nodes.push(child);
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let mut attrs = HashMap::new();
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                    if let Ok(val) = attr.unescape_value() {
                        attrs.insert(key, val.into_owned());
                    }
                }
                nodes.push(OmmlNode {
                    tag,
                    attrs,
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

/// Parses an OMML element (e.g. `<m:oMath>` or `<m:oMathPara>`) directly from a quick-xml Reader into LaTeX.
pub fn parse_omml_element_to_latex(
    reader: &mut Reader<&[u8]>,
    tag: &str,
    attrs: HashMap<String, String>,
) -> String {
    let node = parse_node(reader, tag, attrs);
    node_to_latex(&node)
}

fn parse_node(
    reader: &mut Reader<&[u8]>,
    start_tag: &str,
    start_attrs: HashMap<String, String>,
) -> OmmlNode {
    let mut node = OmmlNode {
        tag: start_tag.to_string(),
        attrs: start_attrs,
        text: String::new(),
        children: Vec::new(),
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let mut attrs = HashMap::new();
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                    if let Ok(val) = attr.unescape_value() {
                        attrs.insert(key, val.into_owned());
                    }
                }
                let child = parse_node(reader, &tag, attrs);
                node.children.push(child);
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let mut attrs = HashMap::new();
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                    if let Ok(val) = attr.unescape_value() {
                        attrs.insert(key, val.into_owned());
                    }
                }
                node.children.push(OmmlNode {
                    tag,
                    attrs,
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
            Ok(Event::GeneralRef(ref e)) => {
                if let Some(s) = crate::parsers::resolve_xml_entity(e) {
                    node.text.push_str(&s);
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
        "t" => {
            let cleaned = node
                .text
                .replace(['\u{2061}', '\u{2062}', '\u{2063}', '\u{2064}'], "");
            match cleaned.as_str() {
                "sin" => "\\sin ".to_string(),
                "cos" => "\\cos ".to_string(),
                "tan" => "\\tan ".to_string(),
                "sec" => "\\sec ".to_string(),
                "csc" => "\\csc ".to_string(),
                "cot" => "\\cot ".to_string(),
                "sinh" => "\\sinh ".to_string(),
                "cosh" => "\\cosh ".to_string(),
                "tanh" => "\\tanh ".to_string(),
                "ln" => "\\ln ".to_string(),
                "log" => "\\log ".to_string(),
                "exp" => "\\exp ".to_string(),
                "lim" => "\\lim ".to_string(),
                "max" => "\\max ".to_string(),
                "min" => "\\min ".to_string(),
                "det" => "\\det ".to_string(),
                "dim" => "\\dim ".to_string(),
                "deg" => "\\deg ".to_string(),
                "∘" => "\\circ".to_string(),
                other => other.to_string(),
            }
        }
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
        "num" | "den" | "e" | "sup" | "sub" | "lim" => node
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
            let dpr = node.children.iter().find(|c| c.tag == "dPr");
            let beg_chr = dpr
                .and_then(|p| p.children.iter().find(|c| c.tag == "begChr"))
                .and_then(|c| c.attrs.get("val").cloned())
                .unwrap_or_else(|| "(".to_string());
            let end_chr = dpr
                .and_then(|p| p.children.iter().find(|c| c.tag == "endChr"))
                .and_then(|c| c.attrs.get("val").cloned())
                .unwrap_or_else(|| ")".to_string());
            let sep_chr = dpr
                .and_then(|p| p.children.iter().find(|c| c.tag == "sepChr"))
                .and_then(|c| c.attrs.get("val").cloned())
                .unwrap_or_else(|| ",".to_string());

            let left_delim = match beg_chr.as_str() {
                "(" => "(",
                "[" => "[",
                "{" => "\\{",
                "|" => "|",
                "‖" => "\\|",
                "⟨" | "<" => "\\langle ",
                "" => "",
                other => other,
            };
            let right_delim = match end_chr.as_str() {
                ")" => ")",
                "]" => "]",
                "}" => "\\}",
                "|" => "|",
                "‖" => "\\|",
                "⟩" | ">" => "\\rangle ",
                "" => "",
                other => other,
            };

            let elements: Vec<String> = node
                .children
                .iter()
                .filter(|c| c.tag == "e")
                .map(node_to_latex)
                .collect();
            let inner = elements.join(&sep_chr);
            format!("{}{}{}", left_delim, inner.trim(), right_delim)
        }
        "rad" => {
            let hide_deg = node
                .children
                .iter()
                .find(|c| c.tag == "radPr")
                .and_then(|p| p.children.iter().find(|c| c.tag == "degHide"))
                .and_then(|c| c.attrs.get("val").map(|v| v == "1" || v == "true"))
                .unwrap_or(false);
            let deg = if hide_deg {
                None
            } else {
                node.children
                    .iter()
                    .find(|c| c.tag == "deg")
                    .map(node_to_latex)
            };
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
            let npr = node.children.iter().find(|c| c.tag == "naryPr");
            let chr = npr
                .and_then(|p| p.children.iter().find(|c| c.tag == "chr"))
                .and_then(|c| c.attrs.get("val").cloned())
                .unwrap_or_else(|| "∑".to_string());
            let op = match chr.as_str() {
                "∫" => "\\int",
                "∬" => "\\iint",
                "∭" => "\\iiint",
                "∮" => "\\oint",
                "∏" => "\\prod",
                "∐" => "\\coprod",
                "⋃" => "\\bigcup",
                "⋂" => "\\bigcap",
                _ => "\\sum",
            };
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
            match (!sub.trim().is_empty(), !sup.trim().is_empty()) {
                (true, true) => {
                    format!("{}_{{{}}}^{{{}}} {}", op, sub.trim(), sup.trim(), e.trim())
                }
                (true, false) => format!("{}_{{{}}} {}", op, sub.trim(), e.trim()),
                (false, true) => format!("{}^{{{}}} {}", op, sup.trim(), e.trim()),
                (false, false) => format!("{} {}", op, e.trim()),
            }
        }
        "m" => {
            let mut rows = Vec::new();
            for mr in node.children.iter().filter(|c| c.tag == "mr") {
                let cells: Vec<String> = mr
                    .children
                    .iter()
                    .filter(|c| c.tag == "e")
                    .map(|cell| node_to_latex(cell).trim().to_string())
                    .collect();
                rows.push(cells.join(" & "));
            }
            format!("\\begin{{matrix}} {} \\end{{matrix}}", rows.join(" \\\\ "))
        }
        "bar" => {
            let pos = node
                .children
                .iter()
                .find(|c| c.tag == "barPr")
                .and_then(|p| p.children.iter().find(|c| c.tag == "pos"))
                .and_then(|c| c.attrs.get("val").map(|v| v.as_str()))
                .unwrap_or("top");
            let e = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            if pos == "bot" {
                format!("\\underline{{{}}}", e.trim())
            } else {
                format!("\\overline{{{}}}", e.trim())
            }
        }
        "acc" => {
            let chr = node
                .children
                .iter()
                .find(|c| c.tag == "accPr")
                .and_then(|p| p.children.iter().find(|c| c.tag == "chr"))
                .and_then(|c| c.attrs.get("val").map(|v| v.as_str()))
                .unwrap_or("^");
            let e = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            let macro_name = match chr {
                "^" | "̂" => "\\hat",
                "¯" | "̄" => "\\bar",
                "˙" | "̇" => "\\dot",
                "¨" | "̈" => "\\ddot",
                "→" | "⃗" => "\\vec",
                "~" | "̃" => "\\tilde",
                "˘" | "̆" => "\\breve",
                "ˇ" | "̌" => "\\check",
                _ => "\\hat",
            };
            format!("{{{}}}{{{}}}", macro_name, e.trim())
        }
        "limLow" => {
            let e = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            let lim = node
                .children
                .iter()
                .find(|c| c.tag == "lim")
                .map(node_to_latex)
                .unwrap_or_default();
            format!("{}_{{{}}}", e.trim(), lim.trim())
        }
        "limUpp" => {
            let e = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            let lim = node
                .children
                .iter()
                .find(|c| c.tag == "lim")
                .map(node_to_latex)
                .unwrap_or_default();
            format!("{}^{{{}}}", e.trim(), lim.trim())
        }
        "func" => {
            let fname = node
                .children
                .iter()
                .find(|c| c.tag == "fName")
                .map(node_to_latex)
                .unwrap_or_default();
            let e = node
                .children
                .iter()
                .find(|c| c.tag == "e")
                .map(node_to_latex)
                .unwrap_or_default();
            format!("{} {}", fname.trim(), e.trim())
        }
        "eqArr" => {
            let exprs: Vec<String> = node
                .children
                .iter()
                .filter(|c| c.tag == "e")
                .map(|e| node_to_latex(e).trim().to_string())
                .collect();
            format!(
                "\\begin{{aligned}} {} \\end{{aligned}}",
                exprs.join(" \\\\ ")
            )
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
                    let mut attrs = HashMap::new();
                    for attr in e.attributes().flatten() {
                        let key =
                            String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                        if let Ok(val) = attr.unescape_value() {
                            attrs.insert(key, val.into_owned());
                        }
                    }
                    let node = parse_node(&mut reader, "oMath", attrs);
                    let latex = node_to_latex(&node);
                    let trimmed = latex.trim();
                    if !trimmed.is_empty() {
                        out.push_str(&format!("${}$", trimmed));
                    }
                } else if tag == "oMathPara" {
                    let mut attrs = HashMap::new();
                    for attr in e.attributes().flatten() {
                        let key =
                            String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                        if let Ok(val) = attr.unescape_value() {
                            attrs.insert(key, val.into_owned());
                        }
                    }
                    let node = parse_node(&mut reader, "oMathPara", attrs);
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
                            Ok(Event::GeneralRef(ref ge)) => {
                                if let Some(s) = crate::parsers::resolve_xml_entity(ge) {
                                    text.push_str(&s);
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

/// Extracts direct body paragraphs from a Word document.xml, along with any chart references,
/// image placeholders, and math formulas. Also returns a map of paraId -> math-augmented text.
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

    let mut p_stack: Vec<DocxBodyParagraph> = Vec::new();
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
                        let mut p = DocxBodyParagraph::default();
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"paraId" {
                                if let Ok(val) = attr.unescape_value() {
                                    p.id = Some(val.to_string());
                                }
                            }
                        }
                        p_stack.push(p);
                    }
                    b"chart" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"id" {
                                if let Ok(val) = attr.unescape_value() {
                                    p_stack.last_mut().unwrap().chart_rids.push(val.to_string());
                                }
                            }
                        }
                    }
                    b"cNvPr" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"name" {
                                if let Ok(val) = attr.unescape_value() {
                                    let name = val.trim();
                                    if !name.is_empty()
                                        && !name.starts_with("Chart ")
                                        && !name.starts_with("Diagram ")
                                    {
                                        let current_p = p_stack.last_mut().unwrap();
                                        if let Some(pos) = current_p.text.find("[Image: Picture ") {
                                            if let Some(end) = current_p.text[pos..].find(']') {
                                                current_p.text.replace_range(
                                                    pos..pos + end + 1,
                                                    &format!("[Image: {}]", name),
                                                );
                                                continue;
                                            }
                                        }
                                        let img_str = format!("[Image: {}]", name);
                                        if !current_p.text.contains(&img_str) {
                                            if !current_p.text.is_empty()
                                                && !current_p.text.ends_with('\n')
                                                && !current_p.text.ends_with(' ')
                                            {
                                                current_p.text.push(' ');
                                            }
                                            current_p.text.push_str(&img_str);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    b"docPr" if !p_stack.is_empty() => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"name" {
                                if let Ok(val) = attr.unescape_value() {
                                    let name = val.trim();
                                    if !name.is_empty()
                                        && !name.starts_with("Chart ")
                                        && !name.starts_with("Diagram ")
                                    {
                                        let current_p = p_stack.last_mut().unwrap();
                                        if !current_p.text.contains("[Image: ") {
                                            if !current_p.text.is_empty()
                                                && !current_p.text.ends_with('\n')
                                                && !current_p.text.ends_with(' ')
                                            {
                                                current_p.text.push(' ');
                                            }
                                            current_p.text.push_str(&format!("[Image: {}]", name));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    b"oMath" if !p_stack.is_empty() => {
                        let mut attrs = HashMap::new();
                        for attr in e.attributes().flatten() {
                            let key =
                                String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                            if let Ok(val) = attr.unescape_value() {
                                attrs.insert(key, val.into_owned());
                            }
                        }
                        let node = parse_node(&mut reader, "oMath", attrs);
                        let latex = node_to_latex(&node);
                        let trimmed = latex.trim();
                        if !trimmed.is_empty() {
                            let current_p = p_stack.last_mut().unwrap();
                            current_p.has_math = true;
                            current_p.text.push_str(&format!("${}$", trimmed));
                        }
                    }
                    b"oMathPara" if !p_stack.is_empty() => {
                        let mut attrs = HashMap::new();
                        for attr in e.attributes().flatten() {
                            let key =
                                String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                            if let Ok(val) = attr.unescape_value() {
                                attrs.insert(key, val.into_owned());
                            }
                        }
                        let node = parse_node(&mut reader, "oMathPara", attrs);
                        let latex = node_to_latex(&node);
                        let trimmed = latex.trim();
                        if !trimmed.is_empty() {
                            let current_p = p_stack.last_mut().unwrap();
                            current_p.has_math = true;
                            current_p.text.push_str(&format!("$${}$$", trimmed));
                        }
                    }
                    b"t" if !p_stack.is_empty() => {
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
                                Ok(Event::GeneralRef(ref ge)) => {
                                    if let Some(s) = crate::parsers::resolve_xml_entity(ge) {
                                        text.push_str(&s);
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
                        p_stack.last_mut().unwrap().text.push_str(&text);
                    }
                    b"tab" if !p_stack.is_empty() => {
                        p_stack.last_mut().unwrap().text.push('\t');
                    }
                    b"br" if !p_stack.is_empty() => {
                        p_stack.last_mut().unwrap().text.push('\n');
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
                } else if !p_stack.is_empty() {
                    if local.as_ref() == b"chart" {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"id" {
                                if let Ok(val) = attr.unescape_value() {
                                    p_stack.last_mut().unwrap().chart_rids.push(val.to_string());
                                }
                            }
                        }
                    } else if local.as_ref() == b"cNvPr" {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"name" {
                                if let Ok(val) = attr.unescape_value() {
                                    let name = val.trim();
                                    if !name.is_empty()
                                        && !name.starts_with("Chart ")
                                        && !name.starts_with("Diagram ")
                                    {
                                        let current_p = p_stack.last_mut().unwrap();
                                        if let Some(pos) = current_p.text.find("[Image: Picture ") {
                                            if let Some(end) = current_p.text[pos..].find(']') {
                                                current_p.text.replace_range(
                                                    pos..pos + end + 1,
                                                    &format!("[Image: {}]", name),
                                                );
                                                continue;
                                            }
                                        }
                                        let img_str = format!("[Image: {}]", name);
                                        if !current_p.text.contains(&img_str) {
                                            if !current_p.text.is_empty()
                                                && !current_p.text.ends_with('\n')
                                                && !current_p.text.ends_with(' ')
                                            {
                                                current_p.text.push(' ');
                                            }
                                            current_p.text.push_str(&img_str);
                                        }
                                    }
                                }
                            }
                        }
                    } else if local.as_ref() == b"docPr" {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"name" {
                                if let Ok(val) = attr.unescape_value() {
                                    let name = val.trim();
                                    if !name.is_empty()
                                        && !name.starts_with("Chart ")
                                        && !name.starts_with("Diagram ")
                                    {
                                        let current_p = p_stack.last_mut().unwrap();
                                        if !current_p.text.contains("[Image: ") {
                                            if !current_p.text.is_empty()
                                                && !current_p.text.ends_with('\n')
                                                && !current_p.text.ends_with(' ')
                                            {
                                                current_p.text.push(' ');
                                            }
                                            current_p.text.push_str(&format!("[Image: {}]", name));
                                        }
                                    }
                                }
                            }
                        }
                    } else if local.as_ref() == b"tab" {
                        p_stack.last_mut().unwrap().text.push('\t');
                    } else if local.as_ref() == b"br" {
                        p_stack.last_mut().unwrap().text.push('\n');
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"body" => in_body = false,
                    b"tbl" => tbl_depth = tbl_depth.saturating_sub(1),
                    b"p" => {
                        if let Some(finished_p) = p_stack.pop() {
                            if p_stack.is_empty() {
                                if finished_p.has_math {
                                    if let Some(ref id) = finished_p.id {
                                        math_by_para_id.insert(id.clone(), finished_p.text.clone());
                                    }
                                }
                                if tbl_depth == 0 {
                                    body_paragraphs.push(finished_p);
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
                                    if finished_p.has_math {
                                        parent.has_math = true;
                                    }
                                }
                            }
                        }
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
    fn test_omml_matrix_and_delimiters() {
        let xml = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
            <m:d>
                <m:dPr>
                    <m:begChr m:val="["/>
                    <m:endChr m:val="]"/>
                </m:dPr>
                <m:e>
                    <m:m>
                        <m:mr>
                            <m:e><m:r><m:t>1</m:t></m:r></m:e>
                            <m:e><m:r><m:t>0</m:t></m:r></m:e>
                        </m:mr>
                        <m:mr>
                            <m:e><m:r><m:t>0</m:t></m:r></m:e>
                            <m:e><m:r><m:t>1</m:t></m:r></m:e>
                        </m:mr>
                    </m:m>
                </m:e>
            </m:d>
        </m:oMath>"#;

        let latex = omml_to_latex(xml);
        assert_eq!(latex, r"[\begin{matrix} 1 & 0 \\ 0 & 1 \end{matrix}]");
    }

    #[test]
    fn test_omml_nary_integral() {
        let xml = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
            <m:nary>
                <m:naryPr>
                    <m:chr m:val="∫"/>
                </m:naryPr>
                <m:sub><m:r><m:t>0</m:t></m:r></m:sub>
                <m:sup><m:r><m:t>1</m:t></m:r></m:sup>
                <m:e><m:r><m:t>x dx</m:t></m:r></m:e>
            </m:nary>
        </m:oMath>"#;

        let latex = omml_to_latex(xml);
        assert_eq!(latex, r"\int_{0}^{1} x dx");
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
        assert!(ps[1].starts_with(r"$$\sin") || ps[1].starts_with("$$sin"));
    }
}

use anyhow::Result;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default, Clone)]
pub struct SlideDiagram {
    pub shapes: HashMap<String, String>,
    pub connectors: Vec<SlideConnector>,
}

#[derive(Debug, Default, Clone)]
pub struct SlideConnector {
    pub id: String,
    pub start_id: String,
    pub end_id: String,
    pub is_dashed: bool,
    pub has_head: bool,
    pub has_tail: bool,
    pub label: Option<String>,
}

impl SlideDiagram {
    /// Parses shapes and connectors from a slide XML string (`p:sld`).
    pub fn parse(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(false);

        let mut diagram = SlideDiagram::default();

        let mut current_shape_id: Option<String> = None;
        let mut current_text_accumulator: Vec<String> = Vec::new();
        let mut in_text_node = false;

        let mut in_connector = false;
        let mut current_conn = SlideConnector::default();
        let mut conn_text_accumulator: Vec<String> = Vec::new();

        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let local = e.local_name();
                    match local.as_ref() {
                        b"sp" => {
                            in_connector = false;
                            current_shape_id = None;
                            current_text_accumulator.clear();
                        }
                        b"cxnSp" => {
                            in_connector = true;
                            current_conn = SlideConnector::default();
                            // In PowerPoint, connectors default to forward directed arrows
                            current_conn.has_head = true;
                            conn_text_accumulator.clear();
                        }
                        b"cNvPr" => {
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"id"
                                    && let Ok(val) = attr.unescape_value()
                                {
                                    let id_str = val.trim().to_string();
                                    if in_connector {
                                        current_conn.id = id_str;
                                    } else {
                                        current_shape_id = Some(id_str);
                                    }
                                }
                            }
                        }
                        b"stCxn" => {
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"id"
                                    && let Ok(val) = attr.unescape_value()
                                {
                                    current_conn.start_id = val.trim().to_string();
                                }
                            }
                        }
                        b"endCxn" => {
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"id"
                                    && let Ok(val) = attr.unescape_value()
                                {
                                    current_conn.end_id = val.trim().to_string();
                                }
                            }
                        }
                        b"prstDash" => {
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"val"
                                    && let Ok(val) = attr.unescape_value()
                                {
                                    let v = val.trim();
                                    if v != "solid" {
                                        current_conn.is_dashed = true;
                                    }
                                }
                            }
                        }
                        b"headEnd" => {
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"type"
                                    && let Ok(val) = attr.unescape_value()
                                {
                                    let v = val.trim();
                                    if v == "none" {
                                        current_conn.has_head = false;
                                    } else if v == "triangle"
                                        || v == "stealth"
                                        || v == "arrow"
                                        || v == "oval"
                                        || v == "diamond"
                                    {
                                        current_conn.has_head = true;
                                    }
                                }
                            }
                        }
                        b"tailEnd" => {
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"type"
                                    && let Ok(val) = attr.unescape_value()
                                {
                                    let v = val.trim();
                                    if v == "none" {
                                        current_conn.has_tail = false;
                                    } else if v == "triangle"
                                        || v == "stealth"
                                        || v == "arrow"
                                        || v == "oval"
                                        || v == "diamond"
                                    {
                                        current_conn.has_tail = true;
                                    }
                                }
                            }
                        }
                        b"t" => in_text_node = true,
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if in_text_node
                        && let Ok(text) = std::str::from_utf8(e.as_ref())
                        && let Ok(decoded) = quick_xml::escape::unescape(text)
                    {
                        if in_connector {
                            conn_text_accumulator.push(decoded.into_owned());
                        } else {
                            current_text_accumulator.push(decoded.into_owned());
                        }
                    }
                }
                Ok(Event::GeneralRef(ref e)) => {
                    if in_text_node && let Some(s) = crate::parsers::resolve_xml_entity(e) {
                        if in_connector {
                            conn_text_accumulator.push(s);
                        } else {
                            current_text_accumulator.push(s);
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local = e.local_name();
                    match local.as_ref() {
                        b"t" => in_text_node = false,
                        b"sp" => {
                            if let Some(id) = current_shape_id.take() {
                                let label = current_text_accumulator.join(" ").trim().to_string();
                                if !label.is_empty() {
                                    diagram.shapes.insert(id, label);
                                }
                            }
                            current_text_accumulator.clear();
                        }
                        b"cxnSp" => {
                            if !current_conn.start_id.is_empty() && !current_conn.end_id.is_empty()
                            {
                                let label = conn_text_accumulator.join(" ").trim().to_string();
                                if !label.is_empty() {
                                    current_conn.label = Some(label);
                                }
                                diagram.connectors.push(std::mem::take(&mut current_conn));
                            }
                            in_connector = false;
                            conn_text_accumulator.clear();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }

        Ok(diagram)
    }

    /// Set of shape IDs that are connected by at least one connector.
    pub fn connected_shape_ids(&self) -> HashSet<String> {
        let mut ids = HashSet::new();
        for c in &self.connectors {
            ids.insert(c.start_id.clone());
            ids.insert(c.end_id.clone());
        }
        ids
    }

    /// Renders the connectors and their connected shapes into a Mermaid `flowchart TD`.
    pub fn render_mermaid(&self) -> Option<String> {
        if self.connectors.is_empty() {
            return None;
        }

        let mut lines = vec!["flowchart TD".to_string()];
        let mut defined_nodes: HashSet<String> = HashSet::new();

        for c in &self.connectors {
            let start_lbl = self
                .shapes
                .get(&c.start_id)
                .map(|s| s.as_str())
                .unwrap_or("");
            let end_lbl = self.shapes.get(&c.end_id).map(|s| s.as_str()).unwrap_or("");

            let start_node = if defined_nodes.insert(c.start_id.clone()) && !start_lbl.is_empty() {
                format!("N_{}[\"{}\"]", c.start_id, escape_mermaid(start_lbl))
            } else {
                format!("N_{}", c.start_id)
            };

            let end_node = if defined_nodes.insert(c.end_id.clone()) && !end_lbl.is_empty() {
                format!("N_{}[\"{}\"]", c.end_id, escape_mermaid(end_lbl))
            } else {
                format!("N_{}", c.end_id)
            };

            let arrow = match (c.is_dashed, c.has_tail, c.has_head) {
                (false, true, true) => "<-->",
                (false, true, false) => "<--",
                (false, false, false) => "---",
                (false, false, true) => "-->",
                (true, true, true) => "<-.->",
                (true, true, false) => "<-.-",
                (true, false, false) => "-.-",
                (true, false, true) => "-.->",
            };

            if let Some(ref lbl) = c.label {
                if !lbl.trim().is_empty() {
                    lines.push(format!(
                        "  {} {}|\"{}\"| {}",
                        start_node,
                        arrow,
                        escape_mermaid(lbl.trim()),
                        end_node
                    ));
                    continue;
                }
            }

            lines.push(format!("  {} {} {}", start_node, arrow, end_node));
        }

        Some(lines.join("\n"))
    }
}

fn escape_mermaid(s: &str) -> String {
    s.replace('"', "#quot;").replace('\n', "<br/>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_connector_flowchart() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:nvSpPr><p:cNvPr id="101" name="Box 1"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
        <p:txBody><a:bodyPr/><a:p><a:r><a:t>Ingest Input</a:t></a:r></a:p></p:txBody>
      </p:sp>
      <p:sp>
        <p:nvSpPr><p:cNvPr id="102" name="Box 2"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
        <p:txBody><a:bodyPr/><a:p><a:r><a:t>Validate Schema</a:t></a:r></a:p></p:txBody>
      </p:sp>
      <p:sp>
        <p:nvSpPr><p:cNvPr id="103" name="Box 3"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
        <p:txBody><a:bodyPr/><a:p><a:r><a:t>Store Record</a:t></a:r></a:p></p:txBody>
      </p:sp>
      <p:cxnSp>
        <p:nvCxnSpPr>
          <p:cNvPr id="201" name="Connector 1"/>
          <p:cNvCxnSpPr>
            <a:stCxn id="101" idx="3"/>
            <a:endCxn id="102" idx="1"/>
          </p:cNvCxnSpPr>
        </p:nvCxnSpPr>
        <p:spPr>
          <a:ln w="19050">
            <a:headEnd type="triangle"/>
          </a:ln>
        </p:spPr>
      </p:cxnSp>
      <p:cxnSp>
        <p:nvCxnSpPr>
          <p:cNvPr id="202" name="Connector 2"/>
          <p:cNvCxnSpPr>
            <a:stCxn id="102" idx="3"/>
            <a:endCxn id="103" idx="1"/>
          </p:cNvCxnSpPr>
        </p:nvCxnSpPr>
        <p:spPr>
          <a:ln w="19050">
            <a:prstDash val="dash"/>
            <a:headEnd type="triangle"/>
          </a:ln>
        </p:spPr>
      </p:cxnSp>
    </p:spTree>
  </p:cSld>
</p:sld>"#;

        let diagram = SlideDiagram::parse(xml).expect("parse diagram");
        assert_eq!(diagram.shapes.len(), 3);
        assert_eq!(diagram.connectors.len(), 2);

        let mermaid = diagram.render_mermaid().expect("render mermaid");
        assert!(mermaid.contains("flowchart TD"));
        assert!(mermaid.contains(r#"N_101["Ingest Input"] --> N_102["Validate Schema"]"#));
        assert!(mermaid.contains(r#"N_102 -.-> N_103["Store Record"]"#));
    }
}

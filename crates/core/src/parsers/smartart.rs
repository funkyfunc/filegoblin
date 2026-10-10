use anyhow::Result;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Default, Clone)]
pub struct SmartArtDiagram {
    pub nodes: BTreeMap<String, String>, // modelId -> text
    pub edges: Vec<(String, String)>,    // (srcId, destId)
}

#[derive(Debug, Default, Clone)]
struct DiagramPoint {
    model_id: String,
    pt_type: String, // "doc", "node", "asst", "parTrans", "sibTrans", "pres"
    text: String,
}

#[derive(Debug, Default, Clone)]
struct DiagramConnection {
    conn_type: String, // "parOf", "sibTrans", etc.
    src_id: String,
    dest_id: String,
}

impl SmartArtDiagram {
    /// Parses SmartArt data from a `data*.xml` string (`dgm:dataModel`).
    pub fn parse(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(false);

        let mut points: HashMap<String, DiagramPoint> = HashMap::new();
        let mut connections: Vec<DiagramConnection> = Vec::new();

        let mut current_point: Option<DiagramPoint> = None;
        let mut current_text_accumulator: Vec<String> = Vec::new();
        let mut in_text_node = false;

        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) => {
                    let local = e.local_name();
                    match local.as_ref() {
                        b"pt" => {
                            let pt = parse_pt(e);
                            current_point = Some(pt);
                            current_text_accumulator.clear();
                        }
                        b"cxn" => {
                            if let Some(cxn) = parse_cxn(e) {
                                connections.push(cxn);
                            }
                        }
                        b"t" => in_text_node = true,
                        _ => {}
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let local = e.local_name();
                    match local.as_ref() {
                        b"pt" => {
                            let pt = parse_pt(e);
                            points.insert(pt.model_id.clone(), pt);
                        }
                        b"cxn" => {
                            if let Some(cxn) = parse_cxn(e) {
                                connections.push(cxn);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if in_text_node
                        && let Ok(text) = std::str::from_utf8(e.as_ref())
                        && let Ok(decoded) = quick_xml::escape::unescape(text)
                    {
                        current_text_accumulator.push(decoded.into_owned());
                    }
                }
                Ok(Event::GeneralRef(ref e)) => {
                    if in_text_node && let Some(s) = crate::parsers::resolve_xml_entity(e) {
                        current_text_accumulator.push(s);
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local = e.local_name();
                    match local.as_ref() {
                        b"t" => in_text_node = false,
                        b"pt" => {
                            if let Some(mut pt) = current_point.take() {
                                pt.text = current_text_accumulator.join(" ").trim().to_string();
                                points.insert(pt.model_id.clone(), pt);
                            }
                            current_text_accumulator.clear();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }

        let mut diagram = SmartArtDiagram::default();

        // 1. Filter semantic nodes (exclude layout placeholders "doc", "pres", "parTrans", "sibTrans")
        for (id, pt) in &points {
            if pt.pt_type == "node" || pt.pt_type == "asst" {
                if !pt.text.is_empty() {
                    diagram.nodes.insert(id.clone(), pt.text.clone());
                } else {
                    diagram.nodes.insert(id.clone(), format!("Node_{}", id));
                }
            }
        }

        // 2. Resolve relationships
        // A. Transitions via sibTrans / parTrans
        for (t_id, pt) in &points {
            if pt.pt_type == "sibTrans" || pt.pt_type == "parTrans" {
                let srcs: Vec<&str> = connections
                    .iter()
                    .filter(|c| c.dest_id == *t_id)
                    .map(|c| c.src_id.as_str())
                    .collect();
                let dsts: Vec<&str> = connections
                    .iter()
                    .filter(|c| c.src_id == *t_id)
                    .map(|c| c.dest_id.as_str())
                    .collect();

                for &s in &srcs {
                    for &d in &dsts {
                        if s != d && diagram.nodes.contains_key(s) && diagram.nodes.contains_key(d)
                        {
                            diagram.edges.push((s.to_string(), d.to_string()));
                        }
                    }
                }
            }
        }

        // B. Direct hierarchy connections (parOf)
        for cxn in &connections {
            if cxn.conn_type == "parOf"
                && cxn.src_id != "0"
                && diagram.nodes.contains_key(&cxn.src_id)
                && diagram.nodes.contains_key(&cxn.dest_id)
                && cxn.src_id != cxn.dest_id
            {
                diagram
                    .edges
                    .push((cxn.src_id.clone(), cxn.dest_id.clone()));
            }
        }

        // Deduplicate edges while preserving order
        let mut seen = HashSet::new();
        diagram.edges.retain(|e| seen.insert(e.clone()));

        Ok(diagram)
    }

    /// Renders the SmartArt diagram into a Mermaid `flowchart TD` or `flowchart LR`.
    pub fn render_mermaid(&self) -> Option<String> {
        if self.nodes.is_empty() {
            return None;
        }

        let mut lines = vec!["flowchart LR".to_string()];

        // If edges form a simple sequential chain (e.g. 1 -> 2 -> 3):
        // Build adjacency map
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut in_degrees: HashMap<&str, usize> = HashMap::new();
        for (src, dst) in &self.edges {
            adj.entry(src.as_str()).or_default().push(dst.as_str());
            *in_degrees.entry(dst.as_str()).or_default() += 1;
            in_degrees.entry(src.as_str()).or_default();
        }

        // Check if graph is a simple linear chain
        let mut is_chain = !self.edges.is_empty();
        for outs in adj.values() {
            if outs.len() > 1 {
                is_chain = false;
                break;
            }
        }
        for &in_deg in in_degrees.values() {
            if in_deg > 1 {
                is_chain = false;
                break;
            }
        }

        if is_chain {
            // Find root (node with in_degree == 0)
            let root = in_degrees
                .iter()
                .find(|(_, deg)| **deg == 0)
                .map(|(node, _)| *node);
            if let Some(mut curr) = root {
                let mut chain = vec![curr];
                while let Some(nexts) = adj.get(curr) {
                    if let Some(&next) = nexts.first() {
                        if chain.contains(&next) {
                            break; // Avoid cycle loop
                        }
                        chain.push(next);
                        curr = next;
                    } else {
                        break;
                    }
                }

                if chain.len() == self.nodes.len() {
                    let formatted_chain: Vec<String> = chain
                        .iter()
                        .map(|id| {
                            let text = self.nodes.get(*id).map(|s| s.as_str()).unwrap_or("");
                            format!("N_{}[\"{}\"]", id, escape_mermaid(text))
                        })
                        .collect();
                    lines.push(format!("  {}", formatted_chain.join(" --> ")));
                    return Some(lines.join("\n"));
                }
            }
        }

        // General graph representation:
        let mut defined_nodes: HashSet<String> = HashSet::new();
        for (src, dst) in &self.edges {
            let src_text = self.nodes.get(src).map(|s| s.as_str()).unwrap_or("");
            let dst_text = self.nodes.get(dst).map(|s| s.as_str()).unwrap_or("");

            let src_str = if defined_nodes.insert(src.clone()) && !src_text.is_empty() {
                format!("N_{}[\"{}\"]", src, escape_mermaid(src_text))
            } else {
                format!("N_{}", src)
            };

            let dst_str = if defined_nodes.insert(dst.clone()) && !dst_text.is_empty() {
                format!("N_{}[\"{}\"]", dst, escape_mermaid(dst_text))
            } else {
                format!("N_{}", dst)
            };

            lines.push(format!("  {} --> {}", src_str, dst_str));
        }

        // If there are standalone nodes not in any edge
        for (id, text) in &self.nodes {
            if !defined_nodes.contains(id) {
                lines.push(format!("  N_{}[\"{}\"]", id, escape_mermaid(text)));
            }
        }

        Some(lines.join("\n"))
    }
}

fn parse_pt(e: &quick_xml::events::BytesStart) -> DiagramPoint {
    let mut pt = DiagramPoint {
        pt_type: "node".to_string(),
        ..Default::default()
    };
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"modelId"
            && let Ok(val) = attr.unescape_value()
        {
            pt.model_id = val.trim().to_string();
        } else if attr.key.as_ref() == b"type"
            && let Ok(val) = attr.unescape_value()
        {
            pt.pt_type = val.trim().to_string();
        }
    }
    pt
}

fn parse_cxn(e: &quick_xml::events::BytesStart) -> Option<DiagramConnection> {
    let mut cxn = DiagramConnection {
        conn_type: "parOf".to_string(),
        ..Default::default()
    };
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"type"
            && let Ok(val) = attr.unescape_value()
        {
            cxn.conn_type = val.trim().to_string();
        } else if attr.key.as_ref() == b"srcId"
            && let Ok(val) = attr.unescape_value()
        {
            cxn.src_id = val.trim().to_string();
        } else if attr.key.as_ref() == b"destId"
            && let Ok(val) = attr.unescape_value()
        {
            cxn.dest_id = val.trim().to_string();
        }
    }
    if !cxn.src_id.is_empty() && !cxn.dest_id.is_empty() {
        Some(cxn)
    } else {
        None
    }
}

fn escape_mermaid(s: &str) -> String {
    s.replace('"', "#quot;").replace('\n', "<br/>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_smartart_process() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<dgm:dataModel xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <dgm:ptLst>
    <dgm:pt modelId="0" type="doc"/>
    <dgm:pt modelId="1" type="node"><dgm:t>Phase 1: Research</dgm:t></dgm:pt>
    <dgm:pt modelId="2" type="node"><dgm:t>Phase 2: Prototype</dgm:t></dgm:pt>
    <dgm:pt modelId="3" type="node"><dgm:t>Phase 3: Production</dgm:t></dgm:pt>
    <dgm:pt modelId="11" type="sibTrans"/>
    <dgm:pt modelId="12" type="sibTrans"/>
  </dgm:ptLst>
  <dgm:cxnLst>
    <dgm:cxn modelId="20" type="parOf" srcId="0" destId="1"/>
    <dgm:cxn modelId="21" type="parOf" srcId="0" destId="2"/>
    <dgm:cxn modelId="22" type="parOf" srcId="0" destId="3"/>
    <dgm:cxn modelId="23" type="sibTrans" srcId="1" destId="11"/>
    <dgm:cxn modelId="24" type="sibTrans" srcId="11" destId="2"/>
    <dgm:cxn modelId="25" type="sibTrans" srcId="2" destId="12"/>
    <dgm:cxn modelId="26" type="sibTrans" srcId="12" destId="3"/>
  </dgm:cxnLst>
</dgm:dataModel>"#;

        let diagram = SmartArtDiagram::parse(xml).expect("parse smartart");
        assert_eq!(diagram.nodes.len(), 3);
        assert_eq!(diagram.edges.len(), 2);

        let mermaid = diagram.render_mermaid().expect("render mermaid");
        assert!(mermaid.contains("flowchart LR"));
        assert!(mermaid.contains(r#"N_1["Phase 1: Research"] --> N_2["Phase 2: Prototype"] --> N_3["Phase 3: Production"]"#));
    }
}

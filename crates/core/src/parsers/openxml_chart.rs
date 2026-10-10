use anyhow::Result;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct OpenXmlChart {
    pub title: Option<String>,
    pub categories: Vec<(usize, String)>,
    pub series: Vec<ChartSeries>,
}

#[derive(Debug, Default, Clone)]
pub struct ChartSeries {
    pub name: Option<String>,
    pub values: Vec<(usize, String)>,
}

impl OpenXmlChart {
    /// Parses an OpenXML chart XML string (`c:chartSpace`).
    pub fn parse(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(false);

        let mut chart = OpenXmlChart::default();

        let mut in_title = false;
        let mut in_ser = false;
        let mut in_tx = false;
        let mut in_cat = false;
        let mut in_val = false;
        let mut in_v = false;
        let mut in_at = false;
        let mut current_pt_idx: Option<usize> = None;
        let mut current_text = String::new();

        let mut current_series = ChartSeries::default();
        let mut pt_counter = 0usize;

        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let local = e.local_name();
                    match local.as_ref() {
                        b"title" => in_title = true,
                        b"ser" => {
                            in_ser = true;
                            current_series = ChartSeries::default();
                        }
                        b"tx" => in_tx = true,
                        b"cat" | b"xVal" => {
                            in_cat = true;
                            pt_counter = 0;
                        }
                        b"val" | b"yVal" => {
                            in_val = true;
                            pt_counter = 0;
                        }
                        b"pt" => {
                            current_pt_idx = None;
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"idx"
                                    && let Ok(val) = attr.unescape_value()
                                    && let Ok(idx) = val.parse::<usize>()
                                {
                                    current_pt_idx = Some(idx);
                                }
                            }
                            if current_pt_idx.is_none() {
                                current_pt_idx = Some(pt_counter);
                            }
                            pt_counter += 1;
                        }
                        b"v" => {
                            in_v = true;
                            current_text.clear();
                        }
                        b"t" => {
                            in_at = true;
                            current_text.clear();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if (in_v || in_at)
                        && let Ok(text) = std::str::from_utf8(e.as_ref())
                        && let Ok(decoded) = quick_xml::escape::unescape(text)
                    {
                        current_text.push_str(&decoded);
                    }
                }
                Ok(Event::CData(ref e)) => {
                    if in_v || in_at {
                        let text = String::from_utf8_lossy(e.as_ref());
                        current_text.push_str(&text);
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local = e.local_name();
                    match local.as_ref() {
                        b"title" => in_title = false,
                        b"ser" => {
                            in_ser = false;
                            chart.series.push(std::mem::take(&mut current_series));
                        }
                        b"tx" => in_tx = false,
                        b"cat" | b"xVal" => in_cat = false,
                        b"val" | b"yVal" => in_val = false,
                        b"pt" => {
                            current_pt_idx = None;
                        }
                        b"v" | b"t" => {
                            in_v = false;
                            in_at = false;
                            let val = current_text.trim().to_string();
                            if !val.is_empty() {
                                if in_title && chart.title.is_none() {
                                    chart.title = Some(val);
                                } else if in_tx && in_ser && current_series.name.is_none() {
                                    current_series.name = Some(val);
                                } else if in_cat {
                                    if let Some(idx) = current_pt_idx {
                                        if !chart.categories.iter().any(|(i, _)| *i == idx) {
                                            chart.categories.push((idx, val));
                                        }
                                    }
                                } else if in_val && in_ser {
                                    if let Some(idx) = current_pt_idx {
                                        current_series.values.push((idx, val));
                                    }
                                }
                            }
                            current_text.clear();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }

        // Sort categories by index
        chart.categories.sort_by_key(|(idx, _)| *idx);
        for s in &mut chart.series {
            s.values.sort_by_key(|(idx, _)| *idx);
        }

        Ok(chart)
    }

    /// Formats the chart into a Markdown heading and pipe table.
    pub fn render_markdown(&self) -> String {
        let mut cat_map: BTreeMap<usize, String> = BTreeMap::new();
        for (idx, cat) in &self.categories {
            cat_map.insert(*idx, cat.clone());
        }

        // If categories were not explicitly listed in cat/xVal, infer from series indices
        if cat_map.is_empty() {
            for s in &self.series {
                for (idx, _) in &s.values {
                    cat_map
                        .entry(*idx)
                        .or_insert_with(|| format!("Item {}", idx + 1));
                }
            }
        }

        if cat_map.is_empty() && self.series.is_empty() {
            return String::new();
        }

        let mut out = String::new();
        if let Some(ref title) = self.title {
            if !title.trim().is_empty() {
                out.push_str(&format!("### Chart: {}\n\n", title.trim()));
            }
        } else {
            out.push_str("### Chart Data\n\n");
        }

        // Build table headers
        let mut headers = vec!["Category".to_string()];
        for (i, s) in self.series.iter().enumerate() {
            let name = s.name.as_deref().unwrap_or("").trim();
            if name.is_empty() {
                headers.push(format!("Series {}", i + 1));
            } else {
                headers.push(name.to_string());
            }
        }

        out.push_str(&format!("| {} |\n", headers.join(" | ")));
        let sep: Vec<&str> = headers.iter().map(|_| ":---").collect();
        out.push_str(&format!("| {} |\n", sep.join(" | ")));

        // Build data rows
        for (idx, cat_name) in &cat_map {
            let mut row = vec![cat_name.clone()];
            for s in &self.series {
                let val = s
                    .values
                    .iter()
                    .find(|(i, _)| i == idx)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("");
                row.push(val.to_string());
            }
            out.push_str(&format!("| {} |\n", row.join(" | ")));
        }

        out.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_openxml_chart() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <c:chart>
    <c:title><c:tx><c:rich><a:p><a:r><a:t>Quarterly Revenue by Region</a:t></a:r></a:p></c:rich></c:tx></c:title>
    <c:plotArea>
      <c:barChart>
        <c:ser>
          <c:tx><c:strRef><c:strCache><c:pt count="1"><c:v>Revenue ($M)</c:v></c:pt></c:strCache></c:strRef></c:tx>
          <c:cat>
            <c:strRef>
              <c:strCache>
                <c:pt idx="0"><c:v>North America</c:v></c:pt>
                <c:pt idx="1"><c:v>EMEA</c:v></c:pt>
                <c:pt idx="2"><c:v>APAC</c:v></c:pt>
              </c:strCache>
            </c:strRef>
          </c:cat>
          <c:val>
            <c:numRef>
              <c:numCache>
                <c:pt idx="0"><c:v>45.2</c:v></c:pt>
                <c:pt idx="1"><c:v>32.8</c:v></c:pt>
                <c:pt idx="2"><c:v>18.5</c:v></c:pt>
              </c:numCache>
            </c:numRef>
          </c:val>
        </c:ser>
      </c:barChart>
    </c:plotArea>
  </c:chart>
</c:chartSpace>"#;

        let chart = OpenXmlChart::parse(xml).expect("parse chart");
        assert_eq!(chart.title.as_deref(), Some("Quarterly Revenue by Region"));
        assert_eq!(chart.categories.len(), 3);
        assert_eq!(chart.series.len(), 1);
        assert_eq!(chart.series[0].name.as_deref(), Some("Revenue ($M)"));
        assert_eq!(chart.series[0].values.len(), 3);

        let md = chart.render_markdown();
        assert!(md.contains("### Chart: Quarterly Revenue by Region"));
        assert!(md.contains("| Category | Revenue ($M) |"));
        assert!(md.contains("| North America | 45.2 |"));
        assert!(md.contains("| EMEA | 32.8 |"));
        assert!(md.contains("| APAC | 18.5 |"));
    }
}

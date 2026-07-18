use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use std::path::Path;

pub struct JupyterGobbler;

impl Gobble for JupyterGobbler {
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> Result<String> {
        let content = std::fs::read_to_string(path).context("Failed to read ipynb file")?;
        let json: serde_json::Value =
            serde_json::from_str(&content).context("Failed to parse ipynb JSON")?;

        let mut output = String::new();

        if let Some(cells) = json.get("cells").and_then(|c| c.as_array()) {
            for cell in cells.iter() {
                let cell_type = cell.get("cell_type").and_then(|t| t.as_str()).unwrap_or("");
                let source = extract_string_array(cell.get("source"));

                if cell_type == "markdown" {
                    output.push_str(&source);
                    output.push_str("\n\n");
                } else if cell_type == "code" {
                    output.push_str(&format!("```python\n{}\n```\n", source.trim_end()));

                    if let Some(outputs) = cell.get("outputs").and_then(|o| o.as_array()) {
                        let mut output_texts = Vec::new();
                        for out in outputs {
                            let out_type = out
                                .get("output_type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("");
                            match out_type {
                                "stream" => {
                                    output_texts.push(extract_string_array(out.get("text")))
                                }
                                "execute_result" | "display_data" => {
                                    if let Some(text_plain) =
                                        out.get("data").and_then(|d| d.get("text/plain"))
                                    {
                                        output_texts.push(extract_string_array(Some(text_plain)));
                                    }
                                }
                                _ => {}
                            }
                        }
                        let combined_outputs = output_texts.join("").trim().to_string();
                        if !combined_outputs.is_empty() {
                            output.push_str("\n**Output:**\n```\n");
                            output.push_str(&combined_outputs);
                            output.push_str("\n```\n");
                        }
                    }
                    output.push('\n');
                }
            }
        }

        if output.trim().is_empty() {
            return Ok("No readable cells found in Jupyter Notebook.".to_string());
        }

        Ok(output.trim().to_string())
    }
}

fn extract_string_array(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<&str>>()
            .join(""),
        Some(serde_json::Value::String(s)) => s.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Write;

    #[test]
    fn test_jupyter_parsing() {
        let json_val = serde_json::json!({
            "cells": [
                {
                    "cell_type": "markdown",
                    "source": ["# Title\n", "Hello world!"]
                },
                {
                    "cell_type": "code",
                    "source": ["print('test')"],
                    "outputs": [
                        {
                            "output_type": "stream",
                            "text": ["test\n"]
                        }
                    ]
                }
            ]
        });
        let json = json_val.to_string();

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(json.as_bytes()).unwrap();

        let gobbler = JupyterGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(file.path(), &(&args).into()).unwrap();

        assert!(result.contains("# Title\nHello world!"));
        assert!(result.contains("```python\nprint('test')\n```"));
        assert!(result.contains("**Output:**\n```\ntest\n```"));
    }
}

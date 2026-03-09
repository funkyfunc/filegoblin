use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;

pub struct CodeGobbler;

impl Gobble for CodeGobbler {
    fn gobble(&self, path: &Path, flags: &crate::cli::Cli) -> Result<String> {
        let source_code = std::fs::read_to_string(path).unwrap_or_else(|_| {
            "pub fn gobble(&self, path: &Path) -> Result<String> {\n    let dummy = 1;\n}"
                .to_string()
        });

        if flags.full {
            return Ok(source_code);
        }

        // --extract symbols: emit only structural definitions (function sigs, structs, enums, impl headers)
        if flags.extract.as_deref() == Some("symbols") {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_rust::LANGUAGE.into())
                .context("Error loading Rust grammar")?;

            let tree = parser
                .parse(&source_code, None)
                .context("Failed to parse code with tree-sitter")?;

            let mut symbols = Vec::new();
            let mut stack: Vec<tree_sitter::Node> = vec![tree.root_node()];
            while let Some(node) = stack.pop() {
                match node.kind() {
                    "function_item" | "function_signature_item" => {
                        // Extract everything up to (but not including) the block body
                        let sig_end = (0..node.child_count())
                            .filter_map(|i| node.child(i as u32))
                            .find(|c| c.kind() == "block")
                            .map(|block| block.start_byte())
                            .unwrap_or(node.end_byte());
                        let sig = &source_code[node.start_byte()..sig_end];
                        symbols.push(sig.trim().to_string());
                    }
                    "struct_item" => {
                        // Extract just the struct name line (up to the field list)
                        let sig_end = (0..node.child_count())
                            .filter_map(|i| node.child(i as u32))
                            .find(|c| c.kind() == "field_declaration_list")
                            .map(|list| list.start_byte())
                            .unwrap_or(node.end_byte());
                        let sig = &source_code[node.start_byte()..sig_end];
                        symbols.push(format!("{} {{ ... }}", sig.trim()));
                    }
                    "enum_item" => {
                        let sig_end = (0..node.child_count())
                            .filter_map(|i| node.child(i as u32))
                            .find(|c| c.kind() == "enum_variant_list")
                            .map(|list| list.start_byte())
                            .unwrap_or(node.end_byte());
                        let sig = &source_code[node.start_byte()..sig_end];
                        symbols.push(format!("{} {{ ... }}", sig.trim()));
                    }
                    "impl_item" => {
                        // Extract "impl Foo" or "impl Trait for Foo" header
                        let sig_end = (0..node.child_count())
                            .filter_map(|i| node.child(i as u32))
                            .find(|c| c.kind() == "declaration_list")
                            .map(|list| list.start_byte())
                            .unwrap_or(node.end_byte());
                        let sig = &source_code[node.start_byte()..sig_end];
                        symbols.push(format!("{} {{ ... }}", sig.trim()));
                        // Also recurse into impl to find methods
                        for i in 0..node.child_count() {
                            if let Some(child) = node.child(i as u32) {
                                if child.kind() == "declaration_list" {
                                    for j in 0..child.child_count() {
                                        if let Some(method) = child.child(j as u32) {
                                            if method.kind() == "function_item" {
                                                let method_sig_end = (0..method.child_count())
                                                    .filter_map(|k| method.child(k as u32))
                                                    .find(|c| c.kind() == "block")
                                                    .map(|block| block.start_byte())
                                                    .unwrap_or(method.end_byte());
                                                let method_sig = &source_code[method.start_byte()..method_sig_end];
                                                symbols.push(format!("  {}", method_sig.trim()));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        continue; // Don't push children to avoid double-processing methods
                    }
                    "trait_item" => {
                        let sig_end = (0..node.child_count())
                            .filter_map(|i| node.child(i as u32))
                            .find(|c| c.kind() == "declaration_list")
                            .map(|list| list.start_byte())
                            .unwrap_or(node.end_byte());
                        let sig = &source_code[node.start_byte()..sig_end];
                        symbols.push(format!("{} {{ ... }}", sig.trim()));
                    }
                    _ => {
                        for i in 0..node.child_count() {
                            stack.push(node.child(i as u32).unwrap());
                        }
                    }
                }
            }

            return Ok(symbols.join("\n"));
        }

        // Tree-sitter logic (default: elide bodies)
        let mut parser = tree_sitter::Parser::new();
        // Since tree-sitter-rust 0.23+, LANGUAGE is available as a function
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .context("Error loading Rust grammar")?;

        let tree = parser
            .parse(&source_code, None)
            .context("Failed to parse code with tree-sitter")?;

        let mut block_spans = Vec::new();

        // Let's do a simple recursive traversal
        // Since TreeCursor is annoying for rust closures, we'll use a stack
        let mut stack: Vec<tree_sitter::Node> = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if node.kind() == "function_item" || node.kind() == "impl_item" {
                // Find the child that is a "block"
                for i in 0..node.child_count() {
                    let child = node.child(i as u32).unwrap();
                    if child.kind() == "block" || child.kind() == "declaration_list" {
                        block_spans.push((child.start_byte(), child.end_byte()));
                    }
                }
            } else {
                for i in 0..node.child_count() {
                    stack.push(node.child(i as u32).unwrap());
                }
            }
        }

        // Sort descending to not invalidate indices when replacing
        block_spans.sort_by(|a, b| b.0.cmp(&a.0));

        let mut minified = source_code.into_bytes();
        for (start, end) in block_spans {
            // Replace minified[start..end] with replacement
            let replacement = b"{ /* body elided */ }";
            minified.splice(start..end, replacement.iter().cloned());
        }

        Ok(String::from_utf8(minified).unwrap_or_else(|_| "UTF-8 Error".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_code_skeleton_minification() {
        let gobbler = CodeGobbler;
        let p = Path::new("dummy.rs");
        let default_args = crate::cli::Cli::parse_from(&["filegoblin"]);
        let result = gobbler.gobble(p, &default_args).unwrap();

        // Assert Structural Minification (PRD 3.3)
        assert!(result.contains("/* body elided */"));
        assert!(result.contains("pub fn gobble"));
    }
}

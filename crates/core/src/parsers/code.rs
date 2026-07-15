use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;

pub struct CodeGobbler;

impl Gobble for CodeGobbler {
    /// Emits source files verbatim by default. Structural symbol extraction
    /// (`--extract symbols`) is opt-in and currently only understands Rust; for any
    /// other language it returns the full source rather than silently mangling it
    /// with the wrong grammar.
    fn gobble(&self, path: &Path, flags: &crate::cli::Cli) -> Result<String> {
        let source_code =
            std::fs::read_to_string(path).context("Failed to read source file as UTF-8")?;

        if flags.extract.as_deref() == Some("symbols") {
            let is_rust = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|e| e.eq_ignore_ascii_case("rs"))
                .unwrap_or(false);
            if is_rust {
                return extract_rust_symbols(&source_code);
            }
            // No grammar for this language yet: return full content honestly.
            return Ok(source_code);
        }

        Ok(source_code)
    }
}

/// Extracts only structural definitions (fn signatures, struct/enum/trait/impl
/// headers) from Rust source using tree-sitter.
fn extract_rust_symbols(source_code: &str) -> Result<String> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .context("Error loading Rust grammar")?;

    let tree = parser
        .parse(source_code, None)
        .context("Failed to parse code with tree-sitter")?;

    let mut symbols = Vec::new();
    let mut stack: Vec<tree_sitter::Node> = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "function_item" | "function_signature_item" => {
                let sig_end = (0..node.child_count())
                    .filter_map(|i| node.child(i as u32))
                    .find(|c| c.kind() == "block")
                    .map(|block| block.start_byte())
                    .unwrap_or(node.end_byte());
                let sig = &source_code[node.start_byte()..sig_end];
                symbols.push(sig.trim().to_string());
            }
            "struct_item" => {
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
                let sig_end = (0..node.child_count())
                    .filter_map(|i| node.child(i as u32))
                    .find(|c| c.kind() == "declaration_list")
                    .map(|list| list.start_byte())
                    .unwrap_or(node.end_byte());
                let sig = &source_code[node.start_byte()..sig_end];
                symbols.push(format!("{} {{ ... }}", sig.trim()));
                // Recurse into impl to surface method signatures.
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32)
                        && child.kind() == "declaration_list"
                    {
                        for j in 0..child.child_count() {
                            if let Some(method) = child.child(j as u32)
                                && method.kind() == "function_item"
                            {
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
                continue; // Avoid double-processing methods.
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

    Ok(symbols.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Write;

    #[test]
    fn test_full_content_is_default() {
        let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
        write!(f, "pub fn foo() {{\n    let x = 1;\n}}").unwrap();

        let gobbler = CodeGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(f.path(), &args).unwrap();

        // Default emits the file verbatim, including the body.
        assert!(result.contains("let x = 1;"));
    }

    #[test]
    fn test_extract_symbols_rust() {
        let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
        write!(f, "pub fn foo() {{\n    let x = 1;\n}}").unwrap();

        let gobbler = CodeGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "--extract", "symbols", "dummy"]);
        let result = gobbler.gobble(f.path(), &args).unwrap();

        assert!(result.contains("pub fn foo()"));
        assert!(!result.contains("let x = 1;"));
    }

    #[test]
    fn test_missing_file_errors() {
        let gobbler = CodeGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        assert!(
            gobbler
                .gobble(Path::new("nope_does_not_exist.rs"), &args)
                .is_err()
        );
    }
}

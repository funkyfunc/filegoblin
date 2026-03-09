use anyhow::Result;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, TEXT, STORED};
use tantivy::{doc, Index};
use tantivy::schema::document::Value;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum RelevanceRank {
    Trivial = 0, // .log, .bak
    Low = 1,     // .json, .csv
    Medium = 2,  // .md, .txt, .toml
    High = 3,    // .rs, .go, .py
}

fn score_file(path: &str) -> RelevanceRank {
    let lower = path.to_lowercase();
    if lower.ends_with(".log") || lower.ends_with(".bak") || lower.ends_with(".tmp") {
        RelevanceRank::Trivial
    } else if lower.ends_with(".json") || lower.ends_with(".csv") || lower.ends_with(".lock") {
        RelevanceRank::Low
    } else if lower.ends_with(".md") || lower.ends_with(".txt") || lower.ends_with(".toml") || lower.ends_with(".yaml") || lower.ends_with(".yml") {
        RelevanceRank::Medium
    } else {
        // Default to high priority for presumed source code/interfaces
        RelevanceRank::High
    }
}

/// Enforces a strict maximum token budget on a list of file pairs using Heuristic Auto-Pruning.
pub fn enforce_budget(mut pairs: Vec<(String, String)>, budget: usize, _verbose: bool) -> (Vec<(String, String)>, usize, usize) {
    // 1. Calculate the initial footprint
    let total_tokens: usize = pairs.iter().map(|(p, c)| crate::compressor::heuristic::estimate_tokens(c, p)).sum();
    
    if total_tokens <= budget {
        return (pairs, total_tokens, total_tokens);
    }
    
    // Sort files by relevance (asc), then by token size (desc - largest first to greedly reclaim space if stuck in same rank)
    pairs.sort_by(|a, b| {
        let rank_a = score_file(&a.0);
        let rank_b = score_file(&b.0);
        let cmp = rank_a.cmp(&rank_b);
        if cmp == std::cmp::Ordering::Equal {
            // If they have the same rank, we want the LARGEST file first so we can drop it
            let size_a = a.1.len();
            let size_b = b.1.len();
            size_b.cmp(&size_a)
        } else {
            cmp
        }
    });

    let mut kept = Vec::new();
    let mut kept_budget = 0;
    
    // Keep adding the highest ranked items until budget breaks
    // (Notice we iterate backwards over our sorted list to grab High -> Medium -> Low)
    for (path, content) in pairs.into_iter().rev() {
         let cost = crate::compressor::heuristic::estimate_tokens(&content, &path);
         if kept_budget + cost <= budget {
             kept.push((path, content));
             kept_budget += cost;
         } else {
             // Fallback: Rather than totally dropping a High priority file, attempt 'Skeletonization'
             if score_file(&path) == RelevanceRank::High {
                 let fallback = crate::compressor::CompressionPipeline::new(&crate::cli::CompressionLevel::Contextual, Some(&path)).process(&content);
                 let shrink_cost = crate::compressor::heuristic::estimate_tokens(&fallback, &path);
                 if kept_budget + shrink_cost <= budget {
                     kept.push((path, fallback));
                     kept_budget += shrink_cost;
                     continue; 
                 }
             }
             
             // If skeletonization failed or we are looking at low priority data, we MUST drop the file
         }
    }

    // Re-sort alphabetically for deterministic output
    kept.sort_by(|a, b| a.0.cmp(&b.0));
    (kept, total_tokens, kept_budget)
}

/// Creates a highly ephemeral BM25 reversed index in memory to execute a pure-Rust "RAG-lite" search over collected strings
pub fn semantic_search(pairs: Vec<(String, String)>, query: &str, top_k: usize) -> Result<Vec<(f32, String, String)>> {
    let mut schema_builder = Schema::builder();
    let path_field = schema_builder.add_text_field("path", TEXT | STORED);
    let body_field = schema_builder.add_text_field("body", TEXT | STORED);
    let schema = schema_builder.build();

    let index = Index::create_in_ram(schema.clone());
    let mut index_writer = index.writer(50_000_000)?;

    for (p, c) in &pairs {
        index_writer.add_document(doc!(
            path_field => p.as_str(),
            body_field => c.as_str()
        ))?;
    }
    index_writer.commit()?;

    let reader = index.reader()?;
    let searcher = reader.searcher();
    let query_parser = QueryParser::for_index(&index, vec![body_field, path_field]);
    
    let parsed_query = query_parser.parse_query(query)?;
    let top_docs = searcher.search(&parsed_query, &TopDocs::with_limit(top_k))?;

    let mut results = Vec::new();
    for (score, doc_address) in top_docs {
        let retrieved_doc: tantivy::TantivyDocument = searcher.doc(doc_address)?;
        let mut p = String::new();
        let mut b = String::new();
        for field_value in retrieved_doc.field_values() {
            if field_value.field == path_field {
                if let Some(text) = (&field_value.value).as_str() {
                    p = text.to_string();
                }
            }
            if field_value.field == body_field {
                 if let Some(text) = (&field_value.value).as_str() {
                    b = text.to_string();
                }
            }
        }
        results.push((score, p, b));
    }
    
    // Sort by score descending (most relevant first)
    results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_ranking() {
        assert_eq!(score_file("app.log"), RelevanceRank::Trivial);
        assert_eq!(score_file("data.csv"), RelevanceRank::Low);
        assert_eq!(score_file("README.md"), RelevanceRank::Medium);
        assert_eq!(score_file("main.rs"), RelevanceRank::High);
    }

    #[test]
    fn test_enforce_budget_pruning() {
        let pairs = vec![
            ("app.log".to_string(), "a ".repeat(500)), // 500 tokens
            ("main.rs".to_string(), "fn ".repeat(100)), // 100 tokens
        ];
        
        // At 150 tokens, it should completely drop the log and keep the source code via greedy sort
        let (pruned, raw, kept) = enforce_budget(pairs, 150, false);
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].0, "main.rs");
    }

    #[test]
    fn test_tantivy_ephemeral_search() {
        let pairs = vec![
            ("main.rs".to_string(), "pub fn connect_db() {}".to_string()),
            ("utils.rs".to_string(), "pub fn parse_date() {}".to_string()),
            ("crawler.rs".to_string(), "pub fn fetch_url() {}".to_string()),
        ];

        let results = semantic_search(pairs, "connect", 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "main.rs");
    }
}

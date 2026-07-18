#[allow(unused_imports)]
use anyhow::Result;
#[cfg(feature = "rag")]
use tantivy::collector::TopDocs;
#[cfg(feature = "rag")]
use tantivy::query::QueryParser;
#[cfg(feature = "rag")]
use tantivy::schema::document::Value;
#[cfg(feature = "rag")]
use tantivy::schema::{STORED, Schema, TEXT};
#[cfg(feature = "rag")]
use tantivy::{Index, doc};

use crate::pipeline::GobbleDocument;

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
    } else if lower.ends_with(".md")
        || lower.ends_with(".txt")
        || lower.ends_with(".toml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
    {
        RelevanceRank::Medium
    } else {
        // Default to high priority for presumed source code/interfaces
        RelevanceRank::High
    }
}

/// Enforces a strict maximum token budget on a list of file pairs using Heuristic Auto-Pruning.
pub fn enforce_budget(
    mut documents: Vec<GobbleDocument>,
    budget: usize,
    _verbose: bool,
) -> (Vec<GobbleDocument>, usize, usize) {
    // 1. Calculate the initial footprint
    let total_tokens: usize = documents.iter().map(|doc| doc.tokens()).sum();

    if total_tokens <= budget {
        return (documents, total_tokens, total_tokens);
    }

    // Sort files by relevance (asc), then by token size (desc - largest first to greedly reclaim space if stuck in same rank)
    documents.sort_by(|a, b| {
        let rank_a = score_file(&a.path);
        let rank_b = score_file(&b.path);
        let cmp = rank_a.cmp(&rank_b);
        if cmp == std::cmp::Ordering::Equal {
            // If they have the same rank, we want the LARGEST file first so we can drop it
            let size_a = a.content.len();
            let size_b = b.content.len();
            size_b.cmp(&size_a)
        } else {
            cmp
        }
    });

    let mut kept = Vec::new();
    let mut kept_budget = 0;

    // Keep adding the highest ranked items until budget breaks
    // (Notice we iterate backwards over our sorted list to grab High -> Medium -> Low)
    for mut doc in documents.into_iter().rev() {
        let cost = doc.tokens();
        if kept_budget + cost <= budget {
            kept.push(doc);
            kept_budget += cost;
        } else {
            // Fallback: Rather than totally dropping a High priority file, attempt 'Skeletonization'
            if score_file(&doc.path) == RelevanceRank::High {
                let fallback = crate::compressor::CompressionPipeline::new(
                    &crate::cli::CompressionLevel::Contextual,
                    Some(&doc.path),
                )
                .process(&doc.content);
                let shrink_cost =
                    crate::compressor::heuristic::estimate_tokens(&fallback, &doc.path);
                if kept_budget + shrink_cost <= budget {
                    doc.content = fallback;
                    kept.push(doc);
                    kept_budget += shrink_cost;
                    continue;
                }
            }

            // If skeletonization failed or we are looking at low priority data, we MUST drop the file
        }
    }

    // Re-sort alphabetically for deterministic output
    kept.sort_by(|a, b| a.path.cmp(&b.path));
    (kept, total_tokens, kept_budget)
}

/// Creates a highly ephemeral BM25 reversed index in memory to execute a pure-Rust "RAG-lite" search over collected strings
#[cfg(feature = "rag")]
pub fn semantic_search(
    documents: Vec<GobbleDocument>,
    query: &str,
    top_k: usize,
) -> Result<Vec<GobbleDocument>> {
    let mut schema_builder = Schema::builder();
    let path_field = schema_builder.add_text_field("path", TEXT | STORED);
    let body_field = schema_builder.add_text_field("body", TEXT | STORED);
    let schema = schema_builder.build();

    let index = Index::create_in_ram(schema.clone());
    let mut index_writer = index.writer(50_000_000)?;

    for doc in &documents {
        index_writer.add_document(doc!(
            path_field => doc.path.as_str(),
            body_field => doc.content.as_str()
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
        for field_value in retrieved_doc.field_values() {
            if field_value.field == path_field
                && let Some(text) = (&field_value.value).as_str()
            {
                p = text.to_string();
            }
        }

        // Find the original document to retain its exact content and update its score
        if let Some(mut doc) = documents.iter().find(|d| d.path == p).cloned() {
            doc.relevance_score = Some(score);
            results.push(doc);
        }
    }

    // Sort by score descending (most relevant first)
    results.sort_by(|a, b| {
        b.relevance_score
            .unwrap_or(0.0)
            .partial_cmp(&a.relevance_score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

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
        let documents = vec![
            GobbleDocument::new("app.log".to_string(), "a ".repeat(500)),
            GobbleDocument::new("main.rs".to_string(), "fn ".repeat(100)),
        ];

        // At 150 tokens, it should completely drop the log and keep the source code via greedy sort
        let (pruned, _raw, _kept) = enforce_budget(documents, 150, false);
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].path, "main.rs");
    }

    #[cfg(feature = "rag")]
    #[test]
    fn test_tantivy_ephemeral_search() {
        let documents = vec![
            GobbleDocument::new("main.rs".to_string(), "pub fn connect_db() {}".to_string()),
            GobbleDocument::new("utils.rs".to_string(), "pub fn parse_date() {}".to_string()),
            GobbleDocument::new(
                "crawler.rs".to_string(),
                "pub fn fetch_url() {}".to_string(),
            ),
        ];

        let results = semantic_search(documents, "connect", 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "main.rs");
    }
}

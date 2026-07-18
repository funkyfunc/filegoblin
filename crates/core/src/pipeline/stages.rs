use crate::cli::Cli;
#[allow(unused_imports)]
use crate::curation;
use crate::pipeline::GobbleDocument;
use crate::privacy_shield;
#[cfg(feature = "rag")]
use anyhow::Context;
use anyhow::Result;
use colored::Colorize;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use pulldown_cmark_to_cmark::cmark;

#[allow(unused_variables)]
pub fn run_curation_stage(
    raw_documents: Vec<GobbleDocument>,
    args: &Cli,
    total_targets: usize,
) -> Result<Vec<GobbleDocument>> {
    let searched_docs = if let Some(query) = args.search.as_deref() {
        #[cfg(feature = "rag")]
        {
            if !args.quiet {
                eprintln!(
                    "{}",
                    format!(
                        "🧠 Searching {} files in the hoard for '{}' (Semantic Priority)...",
                        raw_documents.len(),
                        query
                    )
                    .truecolor(0, 200, 255)
                );
            }
            curation::semantic_search(raw_documents, query, 3).context("Semantic Search failed")?
        }
        #[cfg(not(feature = "rag"))]
        {
            anyhow::bail!(
                "Semantic search requires the 'rag' feature. Recompile with --features rag."
            );
        }
    } else {
        raw_documents
    };

    let pruned_docs = if let Some(budget) = args.max_tokens {
        if !args.quiet {
            eprintln!(
                "{}",
                format!("✂️ Enforcing rigid context budget ({} tokens)...", budget)
                    .truecolor(255, 99, 71)
            );
        }
        let (kept_docs, initial_tokens, kept_budget) =
            curation::enforce_budget(searched_docs, budget, !args.quiet);
        if !args.quiet {
            eprintln!(
                "{}",
                format!(
                    "⚔️ Pruned from {} baseline tokens down to {} tokens (fit {}/{} target files).",
                    initial_tokens,
                    kept_budget,
                    kept_docs.len(),
                    total_targets
                )
                .truecolor(255, 99, 71)
            );
        }
        kept_docs
    } else {
        searched_docs
    };

    Ok(pruned_docs)
}

pub fn run_privacy_stage(
    pruned_docs: Vec<GobbleDocument>,
    _args: &Cli,
    shield: Option<&privacy_shield::PrivacyShield>,
) -> Result<Vec<GobbleDocument>> {
    let final_docs = if let Some(shield) = shield {
        use rayon::prelude::*;
        pruned_docs
            .into_par_iter()
            .map(|mut doc| {
                doc.content = shield.redact(&doc.content);
                doc
            })
            .collect()
    } else {
        pruned_docs
    };

    Ok(final_docs)
}

pub fn run_compression_stage(
    final_docs: Vec<GobbleDocument>,
    args: &Cli,
) -> Result<(Vec<GobbleDocument>, usize)> {
    // Calculate baseline tokens BEFORE compression for savings metric
    let pre_compression_tokens: usize = final_docs.iter().map(|doc| doc.tokens()).sum();

    let compressed_docs = if let Some(level) = args.compress.as_ref() {
        if !args.quiet {
            eprintln!(
                "{}",
                format!("🗜️ Shrinking the loot (Level: {:?})...", level).truecolor(0, 200, 255)
            );
        }
        final_docs
            .into_iter()
            .map(|mut doc| {
                if doc.path == "_tree.md" {
                    return doc;
                }

                let mut mapped_events = Vec::new();
                let parser = Parser::new(&doc.content);
                let mut current_lang = None;

                let default_pipeline = crate::compressor::CompressionPipeline::new(level, None);

                for event in parser {
                    match event {
                        Event::Start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(
                            ref lang,
                        ))) => {
                            current_lang = Some(lang.to_string());
                            mapped_events.push(event.clone());
                        }
                        Event::End(TagEnd::CodeBlock) => {
                            current_lang = None;
                            mapped_events.push(event.clone());
                        }
                        Event::Text(ref text) => {
                            let transformed = if let Some(lang) = &current_lang {
                                let pipeline =
                                    crate::compressor::CompressionPipeline::new(level, Some(lang));
                                pipeline.process(text)
                            } else {
                                default_pipeline.process(text)
                            };
                            mapped_events.push(Event::Text(pulldown_cmark::CowStr::Boxed(
                                transformed.into_boxed_str(),
                            )));
                        }
                        _ => {
                            mapped_events.push(event.clone());
                        }
                    }
                }

                let mut minified_markdown = String::with_capacity(doc.content.len());
                cmark(mapped_events.into_iter(), &mut minified_markdown).unwrap();

                let final_pipeline = crate::compressor::CompressionPipeline::new(level, None);
                doc.content = final_pipeline.process(&minified_markdown);

                doc.token_count = None;
                doc
            })
            .collect()
    } else {
        final_docs
    };

    Ok((compressed_docs, pre_compression_tokens))
}

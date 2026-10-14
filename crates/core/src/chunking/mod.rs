use crate::compressor::heuristic::estimate_tokens;
use serde::{Deserialize, Serialize};

pub use crate::cli::ChunkStrategy;

#[derive(Debug, Clone)]
pub struct ChunkConfig {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub strategy: ChunkStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticChunk {
    pub file: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub breadcrumbs: Vec<String>,
    pub content: String,
    pub tokens: usize,
}

/// Tracks document outline hierarchy (H1 > H2 > H3 ...).
#[derive(Debug, Clone, Default)]
pub struct OutlineTracker {
    stack: Vec<(usize, String)>,
}

impl OutlineTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, level: usize, title: &str) {
        let clean_title = title.trim().trim_end_matches('#').trim().to_string();
        while let Some(&(last_level, _)) = self.stack.last() {
            if last_level >= level {
                self.stack.pop();
            } else {
                break;
            }
        }
        self.stack.push((level, clean_title));
    }

    pub fn breadcrumbs(&self) -> Vec<String> {
        self.stack.iter().map(|(_, t)| t.clone()).collect()
    }
}

#[derive(Debug, Clone)]
pub struct TableBlock {
    pub header: String,
    pub separator: String,
    pub rows: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum BlockKind {
    Heading { level: usize, title: String },
    ThematicBreak,
    CodeBlock { lang: String, code: String },
    Table(TableBlock),
    Narrative(String),
}

#[derive(Debug, Clone)]
pub struct DocumentBlock {
    pub kind: BlockKind,
    pub raw_text: String,
    pub breadcrumbs: Vec<String>,
    pub tokens: usize,
}

/// Chunks a markdown document into layout-aware semantic chunks.
pub fn chunk_document(content: &str, file_path: &str, config: &ChunkConfig) -> Vec<SemanticChunk> {
    if content.trim().is_empty() {
        return Vec::new();
    }

    match config.strategy {
        ChunkStrategy::Layout => chunk_document_layout(content, file_path, config),
        ChunkStrategy::Tokens => chunk_document_tokens(content, file_path, config),
    }
}

/// Parses markdown into semantic structural blocks.
fn parse_semantic_blocks(content: &str, file_path: &str) -> Vec<DocumentBlock> {
    let mut blocks = Vec::new();
    let mut tracker = OutlineTracker::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // 0. YAML frontmatter: one block, kept verbatim. Read line by line
        // its `---` fences look like thematic breaks and the block falls apart.
        if blocks.is_empty()
            && trimmed == "---"
            && let Some(len) = frontmatter_len(&lines[i..])
        {
            let raw_text = lines[i..i + len].join("\n");
            let tokens = estimate_tokens(&raw_text, file_path).max(1);
            blocks.push(DocumentBlock {
                kind: BlockKind::Narrative(raw_text.clone()),
                raw_text,
                breadcrumbs: Vec::new(),
                tokens,
            });
            i += len;
            continue;
        }

        // 1. Heading
        if let Some((level, title)) = parse_heading_line(trimmed) {
            tracker.update(level, &title);
            let raw_text = line.to_string();
            let tokens = estimate_tokens(&raw_text, file_path).max(1);
            blocks.push(DocumentBlock {
                kind: BlockKind::Heading { level, title },
                raw_text,
                breadcrumbs: tracker.breadcrumbs(),
                tokens,
            });
            i += 1;
            continue;
        }

        // 2. Fenced code block
        if trimmed.starts_with("```") {
            let lang = trimmed.trim_start_matches('`').trim().to_string();
            let mut code_lines = Vec::new();
            code_lines.push(line);
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                code_lines.push(lines[i]);
                i += 1;
            }
            if i < lines.len() {
                code_lines.push(lines[i]); // closing ```
                i += 1;
            }
            let raw_text = code_lines.join("\n");
            let tokens = estimate_tokens(&raw_text, file_path).max(1);
            blocks.push(DocumentBlock {
                kind: BlockKind::CodeBlock {
                    lang,
                    code: raw_text.clone(),
                },
                raw_text,
                breadcrumbs: tracker.breadcrumbs(),
                tokens,
            });
            continue;
        }

        // 3. Pipe table
        if is_table_header(trimmed)
            && i + 1 < lines.len()
            && is_table_separator(lines[i + 1].trim())
        {
            let header = line.to_string();
            let separator = lines[i + 1].to_string();
            let mut rows = Vec::new();
            let mut raw_lines = vec![line, lines[i + 1]];
            i += 2;
            while i < lines.len() && is_table_row(lines[i].trim()) {
                rows.push(lines[i].to_string());
                raw_lines.push(lines[i]);
                i += 1;
            }
            let raw_text = raw_lines.join("\n");
            let tokens = estimate_tokens(&raw_text, file_path).max(1);
            blocks.push(DocumentBlock {
                kind: BlockKind::Table(TableBlock {
                    header,
                    separator,
                    rows,
                }),
                raw_text,
                breadcrumbs: tracker.breadcrumbs(),
                tokens,
            });
            continue;
        }

        // 4. Thematic break
        if is_thematic_break(trimmed) {
            let raw_text = line.to_string();
            blocks.push(DocumentBlock {
                kind: BlockKind::ThematicBreak,
                raw_text,
                breadcrumbs: tracker.breadcrumbs(),
                tokens: 1,
            });
            i += 1;
            continue;
        }

        // 5. Narrative block (paragraph or list)
        let mut narrative_lines = Vec::new();
        while i < lines.len() {
            let cur = lines[i];
            let cur_trimmed = cur.trim();
            if cur_trimmed.is_empty()
                || parse_heading_line(cur_trimmed).is_some()
                || cur_trimmed.starts_with("```")
                || (is_table_header(cur_trimmed)
                    && i + 1 < lines.len()
                    && is_table_separator(lines[i + 1].trim()))
                || is_thematic_break(cur_trimmed)
            {
                break;
            }
            narrative_lines.push(cur);
            i += 1;
        }

        if !narrative_lines.is_empty() {
            let raw_text = narrative_lines.join("\n");
            let tokens = estimate_tokens(&raw_text, file_path).max(1);
            blocks.push(DocumentBlock {
                kind: BlockKind::Narrative(raw_text.clone()),
                raw_text,
                breadcrumbs: tracker.breadcrumbs(),
                tokens,
            });
        }
    }

    blocks
}

/// Line count of a YAML frontmatter block opening at `lines[0]` (`---`),
/// including both fences; `None` unless every line between is blank or a
/// `key: value` line and the closing fence comes within 60 lines.
fn frontmatter_len(lines: &[&str]) -> Option<usize> {
    for (j, l) in lines.iter().enumerate().skip(1).take(60) {
        let t = l.trim();
        if t == "---" {
            return (j > 1).then_some(j + 1);
        }
        if !t.is_empty() && !t.contains(':') && !t.starts_with('-') && !l.starts_with(' ') {
            return None;
        }
    }
    None
}

fn parse_heading_line(trimmed: &str) -> Option<(usize, String)> {
    if !trimmed.starts_with('#') {
        return None;
    }
    let mut level = 0;
    for c in trimmed.chars() {
        if c == '#' {
            level += 1;
            if level > 6 {
                return None;
            }
        } else {
            break;
        }
    }
    let rest = &trimmed[level..];
    if rest.starts_with(' ') || rest.starts_with('\t') {
        let title = rest.trim().trim_end_matches('#').trim().to_string();
        Some((level, title))
    } else {
        None
    }
}

fn is_table_header(trimmed: &str) -> bool {
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.contains('|')
}

fn is_table_row(trimmed: &str) -> bool {
    trimmed.starts_with('|') && trimmed.ends_with('|')
}

fn is_table_separator(trimmed: &str) -> bool {
    if !is_table_header(trimmed) {
        return false;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    !inner.is_empty()
        && inner
            .chars()
            .all(|c| c == '-' || c == ':' || c == '|' || c.is_whitespace())
}

fn is_thematic_break(trimmed: &str) -> bool {
    let t: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3
        && (t.chars().all(|c| c == '-')
            || t.chars().all(|c| c == '*')
            || t.chars().all(|c| c == '_'))
}

fn chunk_document_layout(
    content: &str,
    file_path: &str,
    config: &ChunkConfig,
) -> Vec<SemanticChunk> {
    let blocks = parse_semantic_blocks(content, file_path);
    if blocks.is_empty() {
        return Vec::new();
    }

    let mut raw_chunks: Vec<(String, Vec<String>)> = Vec::new();
    let mut current_parts: Vec<String> = Vec::new();
    let mut current_breadcrumbs: Vec<String> = Vec::new();
    let mut current_tokens: usize = 0;
    let mut current_min_heading_level: usize = usize::MAX;
    let mut last_narrative_text: Option<String> = None;

    let flush_current = |parts: &mut Vec<String>,
                         crumbs: &mut Vec<String>,
                         tokens: &mut usize,
                         raw_chunks: &mut Vec<(String, Vec<String>)>| {
        if !parts.is_empty() {
            let text = parts.join("\n\n");
            raw_chunks.push((text, crumbs.clone()));
            parts.clear();
            crumbs.clear();
            *tokens = 0;
        }
    };

    for block in blocks {
        // Table splitting handling
        if let BlockKind::Table(ref table) = block.kind {
            if block.tokens <= config.chunk_size {
                // Table fits in a single chunk
                if current_tokens + block.tokens > config.chunk_size && !current_parts.is_empty() {
                    flush_current(
                        &mut current_parts,
                        &mut current_breadcrumbs,
                        &mut current_tokens,
                        &mut raw_chunks,
                    );
                }
                if current_breadcrumbs.is_empty() {
                    current_breadcrumbs = block.breadcrumbs.clone();
                }
                current_parts.push(block.raw_text.clone());
                current_tokens += block.tokens;
                last_narrative_text = None;
            } else {
                // Table itself exceeds chunk_size -> MANDATORY Table Context Preservation
                let header_prefix = format!("{}\n{}", table.header, table.separator);
                let header_tokens = estimate_tokens(&header_prefix, file_path).max(1);

                // If current_parts fits along with table header, include it in the first chunk
                let (first_prefix, mut table_part_tokens) = if !current_parts.is_empty()
                    && current_tokens + header_tokens < config.chunk_size
                {
                    let p = format!("{}\n\n{}", current_parts.join("\n\n"), header_prefix);
                    let toks = current_tokens + header_tokens;
                    current_parts.clear();
                    current_tokens = 0;
                    (p, toks)
                } else {
                    flush_current(
                        &mut current_parts,
                        &mut current_breadcrumbs,
                        &mut current_tokens,
                        &mut raw_chunks,
                    );
                    (header_prefix.clone(), header_tokens)
                };

                let mut current_table_rows = Vec::new();

                for row in &table.rows {
                    let row_tokens = estimate_tokens(row, file_path).max(1);
                    if table_part_tokens + row_tokens > config.chunk_size
                        && !current_table_rows.is_empty()
                    {
                        let prefix = if raw_chunks.is_empty() && current_parts.is_empty() {
                            &first_prefix
                        } else {
                            &header_prefix
                        };
                        let chunk_text = format!("{}\n{}", prefix, current_table_rows.join("\n"));
                        raw_chunks.push((chunk_text, block.breadcrumbs.clone()));
                        current_table_rows.clear();
                        table_part_tokens = header_tokens;
                    }
                    current_table_rows.push(row.clone());
                    table_part_tokens += row_tokens;
                }

                if !current_table_rows.is_empty() {
                    let prefix = if raw_chunks.is_empty() {
                        &first_prefix
                    } else {
                        &header_prefix
                    };
                    let chunk_text = format!("{}\n{}", prefix, current_table_rows.join("\n"));
                    raw_chunks.push((chunk_text, block.breadcrumbs.clone()));
                }
                last_narrative_text = None;
            }
            continue;
        }

        // Code block handling (never slice a code block that fits in chunk_size)
        if let BlockKind::CodeBlock { .. } = block.kind {
            if current_tokens + block.tokens > config.chunk_size && !current_parts.is_empty() {
                flush_current(
                    &mut current_parts,
                    &mut current_breadcrumbs,
                    &mut current_tokens,
                    &mut raw_chunks,
                );
            }
            if current_breadcrumbs.is_empty() {
                current_breadcrumbs = block.breadcrumbs.clone();
            }
            current_parts.push(block.raw_text.clone());
            current_tokens += block.tokens;
            last_narrative_text = None;
            continue;
        }

        // Heading block: flush if sibling or higher section boundary
        if let BlockKind::Heading { level, .. } = block.kind {
            if !current_parts.is_empty()
                && (level <= 2
                    || level <= current_min_heading_level
                    || current_tokens + block.tokens > config.chunk_size)
            {
                flush_current(
                    &mut current_parts,
                    &mut current_breadcrumbs,
                    &mut current_tokens,
                    &mut raw_chunks,
                );
                current_min_heading_level = usize::MAX;
            }
            if current_breadcrumbs.is_empty() {
                current_breadcrumbs = block.breadcrumbs.clone();
            }
            current_min_heading_level = current_min_heading_level.min(level);
            current_parts.push(block.raw_text.clone());
            current_tokens += block.tokens;
            last_narrative_text = None;
            continue;
        }

        // Narrative or Thematic break
        let is_narrative = matches!(block.kind, BlockKind::Narrative(_));

        if current_tokens + block.tokens > config.chunk_size && !current_parts.is_empty() {
            // Need to flush current chunk
            let overlap_seed = if config.chunk_overlap > 0 && is_narrative {
                last_narrative_text.take()
            } else {
                None
            };

            flush_current(
                &mut current_parts,
                &mut current_breadcrumbs,
                &mut current_tokens,
                &mut raw_chunks,
            );

            // Prepend overlap seed if available
            if let Some(seed) = overlap_seed {
                let seed_tokens = estimate_tokens(&seed, file_path).max(1);
                if seed_tokens <= config.chunk_overlap {
                    current_parts.push(seed);
                    current_tokens += seed_tokens;
                }
            }
        }

        if current_breadcrumbs.is_empty() {
            current_breadcrumbs = block.breadcrumbs.clone();
        }

        if is_narrative {
            // Track the tail of narrative content for potential overlap
            last_narrative_text = Some(block.raw_text.clone());
        } else {
            last_narrative_text = None;
        }

        current_parts.push(block.raw_text.clone());
        current_tokens += block.tokens;
    }

    flush_current(
        &mut current_parts,
        &mut current_breadcrumbs,
        &mut current_tokens,
        &mut raw_chunks,
    );

    let total_chunks = raw_chunks.len();
    raw_chunks
        .into_iter()
        .enumerate()
        .map(|(chunk_index, (content, breadcrumbs))| {
            let tokens = estimate_tokens(&content, file_path);
            SemanticChunk {
                file: file_path.to_string(),
                chunk_index,
                total_chunks,
                breadcrumbs,
                content,
                tokens,
            }
        })
        .collect()
}

/// Fallback hard token windowing strategy.
fn chunk_document_tokens(
    content: &str,
    file_path: &str,
    config: &ChunkConfig,
) -> Vec<SemanticChunk> {
    let words: Vec<&str> = content.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }

    // Convert chunk_size and chunk_overlap from token budgets to approx word budgets
    let target_words = (config.chunk_size as f64 * 0.75).max(1.0) as usize;
    let overlap_words = (config.chunk_overlap as f64 * 0.75) as usize;
    let step = target_words.saturating_sub(overlap_words).max(1);

    let mut windows = Vec::new();
    let mut start = 0;
    while start < words.len() {
        let end = (start + target_words).min(words.len());
        let window_words = &words[start..end];
        windows.push(window_words.join(" "));
        if end == words.len() {
            break;
        }
        start += step;
    }

    let total_chunks = windows.len();
    windows
        .into_iter()
        .enumerate()
        .map(|(chunk_index, chunk_content)| {
            let tokens = estimate_tokens(&chunk_content, file_path);
            SemanticChunk {
                file: file_path.to_string(),
                chunk_index,
                total_chunks,
                breadcrumbs: Vec::new(),
                content: chunk_content,
                tokens,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_stays_one_block() {
        let md = "---\ntitle: \"A\"\nauthor: B\n---\n\n# A\n\nBody text here.\n";
        let cfg = ChunkConfig {
            chunk_size: 1000,
            chunk_overlap: 0,
            strategy: ChunkStrategy::Layout,
        };
        let chunks = chunk_document(md, "a.md", &cfg);
        let all: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(
            all.contains("---\ntitle: \"A\"\nauthor: B\n---"),
            "frontmatter split apart: {all:?}"
        );
    }

    #[test]
    fn a_lone_rule_is_not_frontmatter() {
        let lines = ["---", "Just a paragraph after a rule.", "---"];
        assert_eq!(frontmatter_len(&lines), None);
    }

    #[test]
    fn test_outline_tracker_hierarchy() {
        let mut tracker = OutlineTracker::new();
        assert_eq!(tracker.breadcrumbs(), Vec::<String>::new());

        tracker.update(1, "Chapter 1: Foundations");
        assert_eq!(tracker.breadcrumbs(), vec!["Chapter 1: Foundations"]);

        tracker.update(2, "1.1 Basic Concepts");
        assert_eq!(
            tracker.breadcrumbs(),
            vec!["Chapter 1: Foundations", "1.1 Basic Concepts"]
        );

        tracker.update(3, "1.1.1 Axioms");
        assert_eq!(
            tracker.breadcrumbs(),
            vec![
                "Chapter 1: Foundations",
                "1.1 Basic Concepts",
                "1.1.1 Axioms"
            ]
        );

        // Sibling H2 replaces previous H2 and H3
        tracker.update(2, "1.2 Advanced Paradigms");
        assert_eq!(
            tracker.breadcrumbs(),
            vec!["Chapter 1: Foundations", "1.2 Advanced Paradigms"]
        );

        // Next H1 replaces previous H1
        tracker.update(1, "Chapter 2: Methods");
        assert_eq!(tracker.breadcrumbs(), vec!["Chapter 2: Methods"]);
    }

    #[test]
    fn test_table_context_preservation_when_split() {
        // Build a wide/large table with 20 rows
        let mut md =
            String::from("# Benchmark Results\n\n| Model | Score | Latency |\n|---|---|---|\n");
        for i in 1..=20 {
            md.push_str(&format!("| Model-{} | 95.{}% | {}ms |\n", i, i, i * 10));
        }

        // Set chunk size to ~30 tokens so the table must split across multiple chunks
        let config = ChunkConfig {
            chunk_size: 30,
            chunk_overlap: 0,
            strategy: ChunkStrategy::Layout,
        };

        let chunks = chunk_document(&md, "bench.md", &config);
        assert!(
            chunks.len() >= 2,
            "Expected table to split into multiple chunks, got {}",
            chunks.len()
        );

        // Every chunk must contain the table header and separator!
        for (idx, chunk) in chunks.iter().enumerate() {
            assert!(
                chunk.content.contains("| Model | Score | Latency |"),
                "Chunk {} is missing table header!",
                idx
            );
            assert!(
                chunk.content.contains("|---|---|---|"),
                "Chunk {} is missing table separator!",
                idx
            );
            assert_eq!(
                chunk.breadcrumbs,
                vec!["Benchmark Results"],
                "Chunk {} breadcrumbs mismatch",
                idx
            );
        }
    }

    #[test]
    fn test_code_block_integrity() {
        let md = "# Code Section\n\n```rust\nfn calculate_tax() -> f64 {\n    let rate = 0.20;\n    rate * 100.0\n}\n```\n\nSome trailing explanation.";
        let config = ChunkConfig {
            chunk_size: 50,
            chunk_overlap: 0,
            strategy: ChunkStrategy::Layout,
        };

        let chunks = chunk_document(md, "tax.md", &config);
        assert!(!chunks.is_empty());
        // Code block must remain intact
        let code_chunk = chunks
            .iter()
            .find(|c| c.content.contains("fn calculate_tax"));
        assert!(code_chunk.is_some());
        let content = &code_chunk.unwrap().content;
        assert!(content.contains("```rust"));
        assert!(content.contains("fn calculate_tax() -> f64 {"));
        assert!(content.contains("```"));
    }
}

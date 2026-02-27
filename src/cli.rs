use clap::{Parser, ValueEnum};

#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum CompressionLevel {
    /// Deterministic structural normalization. Aggressively folds whitespace and newlines without breaking code or prose structure.
    Safe,
    /// Semantic lexical pruning. Uses language-aware lexing to strip code comments while preserving docstrings. Minifies JSON/HTML.
    Contextual,
    /// Lossy linguistic distillation. Strips stopwords from English prose and optimizes text to maximize token density.
    Aggressive,
}

/// The mischievous librarian - A high-performance, robust file ingester.
#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
#[command(
    after_help = "EXAMPLES:\n  # Ingest a single file\n  filegoblin src/main.rs\n\n  # Recursively crawl a website and split into separate files\n  filegoblin https://bettercli.org --horde --split\n\n  # Parse a codebase silently and output JSON for a script\n  filegoblin ./my_project --horde -q --json\n"
)]
pub struct Cli {
    /// The target files, directories, or URLs to ingest
    pub paths: Vec<String>,

    // --- OUTPUT FORMATTING ---
    /// The specific LLM output flavor to bind the data with
    #[arg(
        short,
        long,
        default_value = "human",
        help_heading = "Output Formatting"
    )]
    pub flavor: String,

    /// Aggressively strip non-semantic characters from the final output to reduce LLM tokens
    #[arg(long, help_heading = "Output Formatting")]
    pub compress: Option<CompressionLevel>,

    /// Extract the full document instead of attempting heuristic minification
    #[arg(long, help_heading = "Output Formatting")]
    pub full: bool,

    /// Split `--horde` output into individual files within an auto-generated directory
    #[arg(long, help_heading = "Output Formatting", conflicts_with_all = ["chunk", "json"])]
    pub split: bool,

    /// Chunk combined output into multiple files based on an estimated token limit (e.g. --chunk 100k)
    #[arg(long, help_heading = "Output Formatting", conflicts_with_all = ["split", "json"])]
    pub chunk: Option<String>,

    /// Write output directly to a combined file instead of standard output
    #[arg(short = 'w', long, help_heading = "Output Formatting")]
    pub write: Option<String>,

    /// Output strictly formatted struct data (JSON) instead of markdown
    #[arg(long, help_heading = "Output Formatting", conflicts_with_all = ["split", "chunk", "compress"])]
    pub json: bool,

    // --- CRAWLING & INGESTION ---
    /// Force all ingested files through a specific WASM Component Model plugin (e.g. --plugin my_parser)
    #[arg(long, help_heading = "Crawling & Ingestion")]
    pub plugin: Option<String>,

    /// Recursive directory or website crawling
    #[arg(long, help_heading = "Crawling & Ingestion")]
    pub horde: bool,

    // --- DEVELOPER UTILITIES ---
    /// Print estimated token counts for the specific model flavor
    #[arg(long, help_heading = "Developer Utilities")]
    pub tokens: bool,

    /// Scrub PII and Secrets from the output locally using hybrid Regex/SLM Engine
    #[arg(long, help_heading = "Developer Utilities")]
    pub scrub: bool,

    /// Headless "Direct-to-Clipboard" support
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub copy: bool,

    /// OS native file explorer integration (Open the output file/dir)
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub open: bool,


    /// Launch the interactive TUI "Hoard Selector" dashboard
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub interactive: bool,

    /// Suppress all auxiliary output (mascots, progress logs, etc) to ensure clean pipeline usage
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub quiet: bool,
}

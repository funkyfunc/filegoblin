use clap::Parser;

/// The mischievous librarian - A high-performance, robust file ingester.
#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
#[command(
    after_help = "EXAMPLES:\n  # Ingest a single file\n  filegoblin src/main.rs\n\n  # Recursively crawl a website and split into separate files\n  filegoblin https://bettercli.org --horde --split\n\n  # Parse a codebase silently and output JSON for a script\n  filegoblin ./my_project --horde -q --json\n"
)]
pub struct Cli {
    /// The target file, directory, or URL to ingest
    pub path: String,

    // --- OUTPUT FORMATTING ---
    /// The specific LLM output flavor to bind the data with
    #[arg(
        short,
        long,
        default_value = "human",
        help_heading = "Output Formatting"
    )]
    pub flavor: String,

    /// Extract the full document instead of attempting heuristic minification
    #[arg(long, help_heading = "Output Formatting")]
    pub full: bool,

    /// Split `--horde` output into individual files within an auto-generated directory
    #[arg(long, help_heading = "Output Formatting")]
    pub split: bool,

    /// Output strictly formatted struct data (JSON) instead of markdown
    #[arg(long, help_heading = "Output Formatting")]
    pub json: bool,

    // --- CRAWLING & INGESTION ---
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

    /// Suppress all auxiliary output (mascots, progress logs, etc) to ensure clean pipeline usage
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub quiet: bool,
}

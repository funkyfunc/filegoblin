use clap::{Parser, ValueEnum};

#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum CompressionLevel {
    /// Deterministic structural normalization. Aggressively folds whitespace and newlines without breaking code or prose structure.
    Safe,
    /// Semantic lexical pruning. Uses language-aware lexing to strip code comments while preserving docstrings. Minifies JSON/HTML.
    Contextual,
}

/// How the Privacy Shield replaces a detected secret / PII value.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RedactStyle {
    /// Replace with a typed label revealing only the kind, e.g. `[EMAIL]`, `[AWS_KEY]`.
    #[default]
    Typed,
    /// Replace every match with an opaque `[REDACTED]`.
    Plain,
    /// Replace with a stable, indexed placeholder per value, e.g. `[EMAIL_1]`. The same
    /// value maps to the same placeholder everywhere so a downstream LLM can still track
    /// that an entity recurs, without ever seeing it.
    Pseudonym,
}

/// What `--extract` can pull out of a source file instead of its full content.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtractMode {
    /// Only structural symbols: function signatures, struct/enum/trait definitions.
    Symbols,
}

/// The mischievous librarian - A high-performance, robust file ingester.
#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
#[command(
    after_help = "EXAMPLES:\n  # Ingest a single file\n  filegoblin src/main.rs\n\n  # Recursively crawl a website and split into separate files\n  filegoblin https://bettercli.org --horde --split\n\n  # Parse a codebase silently and output JSON for a script\n  filegoblin ./my_project --horde -q --json\n"
)]
pub struct Cli {
    /// The target files, directories, or URLs to ingest. When omitted, filegoblin
    /// reads from stdin if it is piped (so `cat x | filegoblin` works bare).
    pub paths: Vec<String>,

    // --- OUTPUT FORMATTING ---
    /// Aggressively strip non-semantic characters from the final output to reduce LLM tokens
    #[arg(short = 'c', long, default_missing_value = "contextual", num_args = 0..=1, help_heading = "Output Formatting")]
    pub compress: Option<CompressionLevel>,

    /// Extract the full document instead of attempting heuristic minification
    #[arg(long, help_heading = "Output Formatting")]
    pub full: bool,

    /// Split `--horde` output into individual files within an auto-generated directory
    #[arg(short = 's', long, help_heading = "Output Formatting", conflicts_with_all = ["chunk", "json"])]
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
    #[arg(short = 'H', long, help_heading = "Crawling & Ingestion")]
    pub horde: bool,

    /// Filter horde ingestion to only include files matching glob patterns (repeatable, e.g. --include "*.rs" --include "*.toml")
    #[arg(short = 'I', long, help_heading = "Crawling & Ingestion")]
    pub include: Vec<String>,

    /// Exclude files matching glob patterns from horde ingestion (repeatable, e.g. --exclude "*test*" --exclude "*.lock")
    #[arg(short = 'E', long, help_heading = "Crawling & Ingestion")]
    pub exclude: Vec<String>,

    /// Limit recursion depth for --horde crawling (e.g. --depth 1 for top-level only)
    #[arg(long, help_heading = "Crawling & Ingestion")]
    pub depth: Option<usize>,

    // --- CURATION & INTELLIGENCE ---
    /// Local Zero-Dependency Semantic Search (evaluates and returns top matches)
    #[arg(long, help_heading = "Curation & Intelligence")]
    pub search: Option<String>,

    /// Auto-Prune Context to fit a rigid token budget (e.g. --max-tokens 100000)
    #[arg(long, help_heading = "Curation & Intelligence")]
    pub max_tokens: Option<usize>,

    /// Extract only structural symbols (function signatures, struct/enum definitions) from code files
    #[arg(long, value_enum, help_heading = "Curation & Intelligence")]
    pub extract: Option<ExtractMode>,

    /// Only ingest files changed according to git diff (optionally against a specific ref, default: HEAD)
    #[arg(long, default_missing_value = "HEAD", num_args = 0..=1, help_heading = "Curation & Intelligence")]
    pub git_diff: Option<String>,

    /// Show unified diff output instead of full file content when using --git-diff
    #[arg(long, help_heading = "Curation & Intelligence", requires = "git_diff")]
    pub diff_format: bool,

    /// Prepend a manifest table-of-contents with file paths and token counts to horde output
    #[arg(long, help_heading = "Curation & Intelligence")]
    pub manifest: bool,

    // --- DEVELOPER UTILITIES ---
    /// Print estimated token counts for the specific model flavor
    #[arg(short = 't', long, help_heading = "Developer Utilities")]
    pub tokens: bool,

    /// Print only the estimated token count to stdout (no content output)
    #[arg(long, help_heading = "Developer Utilities")]
    pub tokens_only: bool,

    /// Scrub PII and secrets from the output locally (rules + validation + entropy; zero data egress)
    #[arg(long, help_heading = "Developer Utilities")]
    pub scrub: bool,

    /// How `--scrub` replaces matches: `typed` labels (e.g. [EMAIL]) or a plain [REDACTED]
    #[arg(long, value_enum, default_value_t = RedactStyle::Typed, help_heading = "Developer Utilities")]
    pub scrub_style: RedactStyle,

    /// Exit with a non-zero status if `--scrub` redacts anything (useful as a CI / pre-send gate)
    #[arg(long, help_heading = "Developer Utilities", requires = "scrub")]
    pub scrub_fail: bool,

    /// Path to a JSON scrub config (allowlist + custom patterns). Defaults to ./.filegoblin-scrub.json if present
    #[arg(long, help_heading = "Developer Utilities", requires = "scrub")]
    pub scrub_config: Option<String>,

    /// Exempt an exact value from `--scrub` redaction (repeatable). Merged with any config allowlist
    #[arg(long, help_heading = "Developer Utilities", requires = "scrub")]
    pub scrub_allow: Vec<String>,

    /// Headless "Direct-to-Clipboard" support
    #[arg(long, help_heading = "Developer Utilities")]
    pub copy: bool,

    /// OS native file explorer integration (Open the output file/dir)
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub open: bool,

    /// Paste from system clipboard as the primary input target (equivalent to pbpaste/xclip)
    #[arg(long, help_heading = "Developer Utilities")]
    pub clipboard: bool,

    /// Prepend a heuristic structural summary to the top of the output
    #[arg(long, help_heading = "Developer Utilities")]
    pub summary: bool,

    /// Launch the interactive TUI "Hoard Selector" dashboard
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub interactive: bool,

    /// Suppress all auxiliary output (mascots, progress logs, etc) to ensure clean pipeline usage
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub quiet: bool,

    /// Internal guard: set when recursing into an already-extracted archive so we
    /// refuse to extract archives nested inside archives (bomb/quine protection).
    /// Not a user-facing flag.
    #[arg(skip)]
    pub no_nested_archives: bool,
}

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

/// How PDFs use the bundled vision models (`--vision`).
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VisionMode {
    /// Never: the fast geometric converter only.
    Off,
    /// Per page: scans, math, tables, columns, figures and short documents
    /// go through the models; plain single-column prose takes the fast path.
    #[default]
    Smart,
    /// Every page through the models.
    All,
}

/// Unified output format for the conversion pipeline.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Standard GitHub Flavored Markdown with tables, LaTeX math, and code blocks
    #[default]
    Markdown,
    /// Clean plain text with formatting stripped, space-aligned tables, and plain math
    Plain,
    /// Structured JSON documents or chunks
    Json,
}

/// Chunking strategy: layout-aware semantic boundaries or hard token windows.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChunkStrategy {
    /// Splits strictly along semantic structural boundaries (headings, tables, code blocks, paragraphs)
    #[default]
    Layout,
    /// Hard token windowing fallback
    Tokens,
}

/// Accepts `100k`, `2m`, or a bare token count, rejecting anything else at
/// parse time so a typo can never be silently ignored.
pub fn parse_token_budget(raw: &str) -> Result<String, String> {
    let c = raw.trim().to_lowercase();
    let ok = if let Some(n) = c.strip_suffix('k') {
        n.parse::<f64>().map(|n| n > 0.0).unwrap_or(false)
    } else if let Some(n) = c.strip_suffix('m') {
        n.parse::<f64>().map(|n| n > 0.0).unwrap_or(false)
    } else {
        c.parse::<usize>().map(|n| n > 0).unwrap_or(false)
    };
    if ok {
        Ok(raw.to_string())
    } else {
        Err(format!(
            "expected a token budget like `100k`, `2m`, or `5000` — got `{raw}`"
        ))
    }
}

/// Parses a token count like `512`, `1024`, `1k`, `2m` into a usize token limit.
pub fn parse_chunk_size(raw: &str) -> Result<usize, String> {
    let c = raw.trim().to_lowercase();
    if let Some(n) = c.strip_suffix('k') {
        n.parse::<f64>()
            .map(|n| (n * 1000.0) as usize)
            .map_err(|_| format!("expected a valid token size like 512, 1024, or 1k — got `{raw}`"))
    } else if let Some(n) = c.strip_suffix('m') {
        n.parse::<f64>()
            .map(|n| (n * 1_000_000.0) as usize)
            .map_err(|_| format!("expected a valid token size like 512, 1024, or 1k — got `{raw}`"))
    } else {
        c.parse::<usize>()
            .map_err(|_| format!("expected a valid token size like 512, 1024, or 1k — got `{raw}`"))
    }
}

/// The mischievous librarian - A high-performance, robust file ingester.
#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
#[command(name = "filegoblin")]
#[command(
    after_help = "EXIT CODES:\n  0  success (also a closed output pipe, e.g. `| head`)\n  1  unexpected error\n  2  bad usage / no target\n  3  --scrub-fail and something was redacted\n  4  nothing could be extracted\n\nEXAMPLES:\n  # Ingest a single file\n  filegoblin src/main.rs\n\n  # Recursively crawl a website and split into separate files\n  filegoblin https://bettercli.org --horde --split\n\n  # Parse a codebase silently and output JSON for a script\n  filegoblin ./my_project --horde -q --json\n\n  # Run as an MCP server for Claude Desktop / Claude Code\n  filegoblin --mcp\n"
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
    #[arg(short = 's', long, help_heading = "Output Formatting", conflicts_with_all = ["chunk", "json", "chunk_size"])]
    pub split: bool,

    /// Chunk combined output into multiple files based on an estimated token limit (e.g. --chunk 100k)
    ///
    /// Validated here rather than deep in the formatter, where an unparseable
    /// value used to silently disable chunking and emit one giant file.
    #[arg(
        long,
        help_heading = "Output Formatting",
        conflicts_with_all = ["split", "json", "summary", "manifest", "chunk_size"],
        value_parser = parse_token_budget,
    )]
    pub chunk: Option<String>,

    /// Write output directly to a combined file instead of standard output
    #[arg(short = 'w', long, help_heading = "Output Formatting")]
    pub write: Option<String>,

    /// Output strictly formatted struct data (JSON) instead of markdown
    #[arg(long, help_heading = "Output Formatting", conflicts_with_all = ["split", "chunk", "compress"])]
    pub json: bool,

    /// Unified output format: markdown (default), plain, or json
    #[arg(
        short = 'f',
        long,
        value_enum,
        default_value_t = OutputFormat::Markdown,
        help_heading = "Output Formatting"
    )]
    pub format: OutputFormat,

    /// Target maximum token budget per chunk for semantic chunking (e.g. 512, 1024, 1k)
    #[arg(
        long,
        help_heading = "Output Formatting",
        conflicts_with_all = ["chunk", "split"],
        value_parser = parse_chunk_size,
    )]
    pub chunk_size: Option<usize>,

    /// Overlap token budget between adjacent narrative chunks
    #[arg(long, default_value_t = 50, help_heading = "Output Formatting")]
    pub chunk_overlap: usize,

    /// Chunking strategy: layout-aware semantic boundaries (default) or hard token windows
    #[arg(
        long,
        value_enum,
        default_value_t = ChunkStrategy::Layout,
        help_heading = "Output Formatting"
    )]
    pub chunk_strategy: ChunkStrategy,

    // --- CRAWLING & INGESTION ---
    /// Force all ingested files through a specific WASM Component Model plugin (e.g. --plugin my_parser)
    #[arg(long, help_heading = "Crawling & Ingestion")]
    pub plugin: Option<String>,

    /// Recursive directory or website crawling
    #[arg(short = 'H', long, help_heading = "Crawling & Ingestion")]
    pub horde: bool,

    /// Only ingest paths matching these glob patterns (repeatable, e.g. --include "*.rs")
    ///
    /// Applies to both halves of `--horde`: file paths locally, and the URL
    /// *path* when crawling, so `--include "/guide/*"` (or just `--include
    /// guide`) scopes a crawl to that section. Out-of-scope pages are never
    /// fetched, not merely dropped from the output. The seed URL is always
    /// fetched regardless.
    #[arg(short = 'I', long, help_heading = "Crawling & Ingestion")]
    pub include: Vec<String>,

    /// Exclude paths matching these glob patterns (repeatable, e.g. --exclude "*test*")
    ///
    /// Like `--include`, this covers local paths and crawled URL paths, and
    /// prunes the crawl rather than the output — `--exclude "*/changelog/*"`
    /// means those pages are never requested.
    #[arg(short = 'E', long, help_heading = "Crawling & Ingestion")]
    pub exclude: Vec<String>,

    /// Browse the target and pick what to convert, instead of swallowing everything
    ///
    /// Opens an interactive picker: read each page or file, follow links and
    /// folders, mark what you want, then convert only that. Works for websites
    /// and local directories. Needs a terminal — the default (non-interactive)
    /// behaviour is unchanged, so scripts, pipes, and the MCP server are
    /// unaffected.
    /// `--json` is fine here (the picker draws on stderr, JSON still goes to
    /// stdout); `--clipboard` is not, since it replaces the input entirely.
    #[arg(long, help_heading = "Crawling & Ingestion", conflicts_with_all = ["mcp", "clipboard"])]
    pub walk: bool,

    /// Limit recursion depth for --horde (e.g. --depth 1 for top-level only)
    ///
    /// Local: directory levels below the target. Web: link hops from the seed
    /// page, which is depth 0 — so `--depth 1` fetches the seed and the pages
    /// it links to directly, and nothing further.
    #[arg(long, help_heading = "Crawling & Ingestion")]
    pub depth: Option<usize>,

    /// Render URLs with your installed Chrome/Chromium (headless) so
    /// JavaScript-built pages work. Uses a throwaway profile (no cookies)
    #[arg(long, help_heading = "Crawling & Ingestion")]
    pub browser: bool,

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
    ///
    /// Only the unchunked, non-JSON path builds it; combining it with those
    /// silently produced output with no manifest at all.
    #[arg(long, help_heading = "Curation & Intelligence", conflicts_with_all = ["chunk", "json"])]
    pub manifest: bool,

    // --- DEVELOPER UTILITIES ---
    /// Print estimated token counts for the specific model flavor
    #[arg(short = 't', long, help_heading = "Developer Utilities")]
    pub tokens: bool,

    /// Print only the estimated token count to stdout (no content output)
    ///
    /// Both `--json` and `--split` ignored this entirely, so `--json
    /// --tokens-only` printed the whole corpus when a number was asked for.
    #[arg(long, help_heading = "Developer Utilities", conflicts_with_all = ["json", "split", "chunk", "write", "copy", "open"])]
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
    ///
    /// The chunked path sorts the tree map to index 0 and only emits the
    /// preamble before the first chunk, so `--summary --chunk` dropped it.
    #[arg(long, help_heading = "Developer Utilities", conflicts_with_all = ["chunk", "json"])]
    pub summary: bool,

    /// PDFs: when to use the bundled vision models (layout, reading order,
    /// formulas, tables, scan OCR). `smart` decides per page; `off` is the
    /// fast geometric converter only; `all` sends every page through them.
    /// Needs a build with `--features vision` and the model weights.
    #[arg(long, value_enum, default_value_t = VisionMode::Smart)]
    pub vision: VisionMode,

    /// Skip the conversion cache (~/.filegoblin/cache) for this run: refetch
    /// URLs and redo OCR/transcription even when a cached result exists
    #[arg(long, help_heading = "Developer Utilities")]
    pub no_cache: bool,

    /// Empty the conversion cache (~/.filegoblin/cache) and exit
    #[arg(long, help_heading = "Developer Utilities")]
    pub cache_clear: bool,

    /// Download this version's PDF vision models (~500 MB, SHA-256 checked)
    /// into DIR, or ~/.filegoblin/models, and exit
    ///
    /// The source is the GitHub release, or the URL in FILEGOBLIN_MODELS_URL
    /// (for an internal mirror). Not needed with the filegoblin-full download
    /// or the `filegoblin[models]` / `filegoblin-models` packages, which carry
    /// the models already.
    #[arg(long, value_name = "DIR", num_args = 0..=1, help_heading = "Developer Utilities")]
    pub install_models: Option<Option<std::path::PathBuf>>,

    /// Show where the PDF vision models are found (and which are missing) and exit
    #[arg(long, help_heading = "Developer Utilities")]
    pub models_status: bool,

    /// Run as an MCP (Model Context Protocol) stdio server exposing a `convert` tool
    #[arg(long, help_heading = "Developer Utilities")]
    pub mcp: bool,

    /// Suppress all auxiliary output (mascots, progress logs, etc) to ensure clean pipeline usage
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub quiet: bool,

    /// Internal guard: set when recursing into an already-extracted archive so we
    /// refuse to extract archives nested inside archives (bomb/quine protection).
    /// Not a user-facing flag.
    #[arg(skip)]
    pub no_nested_archives: bool,
}

impl Cli {
    /// Returns the effective output format, prioritizing `--json` for backwards compatibility.
    pub fn output_format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else {
            self.format
        }
    }
}

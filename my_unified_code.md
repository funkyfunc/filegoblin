```tree
.
  ├── parsers/
    ├── pdf.rs
    ├── code.rs
    ├── gobble.rs
    ├── web.rs
    ├── office.rs
    ├── mod.rs
    ├── crawler.rs
    ├── ocr.rs
  ├── ui.rs
  ├── lib.rs
  ├── flavors.rs
  ├── main.rs
  ├── cli.rs
  ├── privacy_shield.rs
```

// --- FILE_START: parsers/pdf.rs ---
use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;

pub struct PdfGobbler;

impl Gobble for PdfGobbler { /* body elided */ }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdf_sequence_of_records() { /* body elided */ }
}


// --- FILE_START: parsers/code.rs ---
use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;

pub struct CodeGobbler;

impl Gobble for CodeGobbler { /* body elided */ }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_skeleton_minification() { /* body elided */ }
}


// --- FILE_START: parsers/gobble.rs ---
use anyhow::Result;
use std::path::Path;

/// The primary trait that all `filegoblin` document parsers must implement.
///
/// This ensures a unified interface for ingesting diverse file formats into
/// a target string representation (Markdown, XML, YAML) based on the chosen output flavor.
pub trait Gobble {
    /// Consumes a file at the given path and returns the extracted, structured string.
    fn gobble(&self, path: &Path) -> Result<String>;

    /// Consumes an in-memory string directly and returns the structured string.
    fn gobble_str(&self, _content: &str) -> Result<String> { /* body elided */ }
}


// --- FILE_START: parsers/web.rs ---
use crate::parsers::gobble::Gobble;
use anyhow::Result;
use std::path::Path;

pub struct WebGobbler {
    pub extract_full: bool,
}

impl WebGobbler { /* body elided */ }

impl Gobble for WebGobbler { /* body elided */ }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_heuristic_extraction() { /* body elided */ }
}


// --- FILE_START: parsers/office.rs ---
use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;

pub struct OfficeGobbler;

impl Gobble for OfficeGobbler { /* body elided */ }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_office_sequence_of_records() { /* body elided */ }
}


// --- FILE_START: parsers/mod.rs ---
pub mod code;
pub mod crawler;
pub mod gobble;
pub mod ocr;
pub mod office;
pub mod pdf;
pub mod web;


// --- FILE_START: parsers/crawler.rs ---
use crate::parsers::gobble::Gobble;
use crate::parsers::web::WebGobbler;
use anyhow::{Context, Result};
use colored::Colorize;
use dashmap::DashSet;
use governor::{Quota, RateLimiter, clock::DefaultClock, state::keyed::DefaultKeyedStateStore};
use reqwest::Url;
use robotxt::Robots;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

/// A unified struct managing web crawler state and orchestration.
pub struct GoblinCrawler {
    /// Tracks URLs we have already visited or queued to avoid infinite loops.
    /// DashSet allows lock-free concurrent ingestion from multiple workers.
    visited: Arc<DashSet<String>>,

    /// Target domain. Scoping is heavily restricted to prevent unbounded internet traversal.
    seed_domain: String,

    /// Politeness rate limiter (requests per second per domain).
    rate_limiter: Arc<RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>,

    /// Parsed Robots.txt for the domain
    robots: Option<Robots>,

    pub extract_full: bool,
}

impl GoblinCrawler { /* body elided */ }

pub fn crawl_web(url: &Url, extract_full: bool) -> Result<Vec<(String, String)>> { /* body elided */ }


// --- FILE_START: parsers/ocr.rs ---
use crate::parsers::gobble::Gobble;
use anyhow::Result;
use std::path::Path;

// Embed the Tesseract WASM directly into the executable at compile time!
// If build.rs failed offline, it may be a dummy 0-byte file.
const TESSERACT_CORE_WASM: &[u8] = include_bytes!("../../assets/tesseract-core-simd.wasm");

pub struct OcrGobbler;

impl Gobble for OcrGobbler { /* body elided */ }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_extraction() { /* body elided */ }
}


// --- FILE_START: ui.rs ---
use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    Terminal,
};
use std::{io, path::PathBuf};

/// The internal state of our TUI Application.
pub struct App<'a> {
    pub files: Vec<PathBuf>,
    pub selected_index: usize,
    pub selected_files: std::collections::HashSet<usize>,
    pub preview_content: String,
    pub view_scroll: u16,
    pub should_quit: bool,
    pub should_execute: bool,
    pub active_flags: &'a mut crate::cli::Cli,
}

impl<'a> App<'a> { /* body elided */ }

pub fn run_tui(args: &mut crate::cli::Cli) -> Result<Option<Vec<PathBuf>>> { /* body elided */ }

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> 
where 
    <B as ratatui::backend::Backend>::Error: std::error::Error + Send + Sync + 'static,
{ /* body elided */ }


// --- FILE_START: lib.rs ---
pub mod flavors;
pub mod parsers;
pub mod privacy_shield;

use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;
use url::Url;
use std::io::Read;

/// filegoblin Core
/// We keep logic in lib.rs to ensure the application is deeply testable
/// independently from the `clap` CLI layer.
#[allow(clippy::too_many_arguments)]
pub fn gobble_app(
    targets: &[String],
    flavor: &flavors::Flavor,
    full: bool,
    horde: bool,
    split: bool,
    destination: Option<&str>,
    tokens: bool,
    quiet: bool,
    json: bool,
    scrub: bool,
    copy_clipboard: bool,
    open_explorer: bool,
) -> Result<()> { /* body elided */ }

fn gobble_local(target: &str, full: bool, horde: bool) -> Result<Vec<(String, String)>> { /* body elided */ }

fn route_and_gobble(path_str: &str, full: bool) -> Result<String> { /* body elided */ }

fn fetch_url(url: &Url, quiet: bool) -> Result<String> { /* body elided */ }

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn test_fetch_url_and_gobble() { /* body elided */ }

    #[test]
    fn test_route_and_gobble_unknown_extension() { /* body elided */ }
}


// --- FILE_START: flavors.rs ---
use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub enum Flavor {
    Human,
    Anthropic,
    Gpt,
    Gemini,
}

impl std::str::FromStr for Flavor { /* body elided */ }

pub fn format_output(flavor: &Flavor, filename: &str, content: &str) -> String { /* body elided */ }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_flavor() { /* body elided */ }

    #[test]
    fn test_anthropic_flavor() { /* body elided */ }

    #[test]
    fn test_gpt_flavor() { /* body elided */ }

    #[test]
    fn test_gemini_flavor() { /* body elided */ }
}


// --- FILE_START: main.rs ---
use anyhow::Result;
use clap::Parser;
use colored::*;
use filegoblin::{flavors::Flavor, gobble_app};
use std::str::FromStr;
use std::sync::mpsc::channel;
use std::io::IsTerminal;
use notify::{Watcher, RecursiveMode};

const ASCII_MASCOT: &str = r#"
    (o_o)  <-- "I'm hungry for files."
     (W)
   --m-m--  filegoblin v1.5.0
"#;

mod cli;
use cli::Cli;

mod ui;

fn main() -> Result<()> { /* body elided */ }


// --- FILE_START: cli.rs ---
use clap::Parser;

/// The mischievous librarian - A high-performance, robust file ingester.
#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
#[command(
    after_help = "EXAMPLES:\n  # Ingest a single file\n  filegoblin src/main.rs\n\n  # Recursively crawl a website and split into separate files\n  filegoblin https://bettercli.org --horde --split\n\n  # Parse a codebase silently and output JSON for a script\n  filegoblin ./my_project --horde -q --json\n"
)]
pub struct Cli {
    /// The target file, directory, or URL to ingest
    pub path: Option<String>,

    /// Optional output file or directory (when using --split) to write the results to
    pub destination: Option<String>,

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

    /// Headless "Direct-to-Clipboard" support
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub copy: bool,

    /// OS native file explorer integration (Open the output file/dir)
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub open: bool,

    /// Watch a directory or file and automatically re-gobble on changes
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub watch: bool,

    /// Launch the interactive TUI "Hoard Selector" dashboard
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub interactive: bool,

    /// Suppress all auxiliary output (mascots, progress logs, etc) to ensure clean pipeline usage
    #[arg(short, long, help_heading = "Developer Utilities")]
    pub quiet: bool,
}


// --- FILE_START: privacy_shield.rs ---
use aho_corasick::{AhoCorasick, MatchKind};
use anyhow::{Context, Result};
use std::collections::HashMap;
use regex::Regex;

/// Heuristic Trigger Component (Tier 3)
pub struct PiiTrigger {
    window_size: usize,
    entropy_threshold: f64,
}

impl PiiTrigger { /* body elided */ }

/// Tier 3: Index Merger to reconcile overlapping or adjacent window spans
pub struct IndexMerger;

impl IndexMerger { /* body elided */ }

/// Tier 2: Refiner Component (Mocked SLM)
pub struct Tier2Refiner {
    confidence_threshold: f64,
}

impl Tier2Refiner { /* body elided */ }

pub struct PrivacyShield {
    trigger: PiiTrigger,
    refiner: Tier2Refiner,
    ac: AhoCorasick,
    regexes: Vec<Regex>,
    neural_semaphore: std::sync::Arc<tokio::sync::Semaphore>,
}

impl PrivacyShield { /* body elided */ }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pii_trigger_entropy() { /* body elided */ }

    #[test]
    fn test_privacy_shield_redaction() { /* body elided */ }

    #[test]
    fn test_comprehensive_pii_dataset() { /* body elided */ }
}

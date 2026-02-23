use anyhow::Result;
use clap::Parser;
use colored::*;
use filegoblin::{flavors::Flavor, gobble_app};
use std::str::FromStr;

const ASCII_MASCOT: &str = r#"
    (o_o)  <-- "I'm hungry for files."
     (W)
   --m-m--  filegoblin v1.5.0
"#;

/// The mischievous librarian - A high-performance, robust file ingester.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// The target file, directory, or URL to ingest
    path: String,

    /// The specific LLM output flavor to bind the data with
    #[arg(short, long, default_value = "human")]
    flavor: String,

    /// Extract the full document instead of attempting heuristic minification
    #[arg(long)]
    full: bool,

    /// Recursive directory or website crawling
    #[arg(long)]
    horde: bool,

    /// Print estimated token counts for the specific model flavor
    #[arg(long)]
    tokens: bool,
}

fn main() -> Result<()> {
    // Parse arguments and emit the initial "Crunching..." or Goblinism
    let args = Cli::parse();

    let parsed_flavor = Flavor::from_str(&args.flavor).unwrap_or(Flavor::Human);

    println!("{}", ASCII_MASCOT.truecolor(167, 255, 0).bold());
    println!("{}", "Hello Goblin!".truecolor(167, 255, 0).bold());

    // Initialize core library configurations
    gobble_app(&args.path, &parsed_flavor, args.full, args.horde, args.tokens)?;

    Ok(())
}

//! # fg-tui
//!
//! A standalone binary for the Filegoblin interactive TUI.
//! Launch with `fg-tui [path]` to explore files and select them for gobbling.

use anyhow::Result;
use clap::Parser;
use colored::*;
use filegoblin_core::{flavors::Flavor, gobble_app};
use std::io::IsTerminal;
use std::str::FromStr;

use filegoblin_core::cli::Cli;

fn main() -> Result<()> {
    let mut args = Cli::parse();

    // Force interactive mode
    args.interactive = true;

    let parsed_flavor = Flavor::from_str(&args.flavor).unwrap_or(Flavor::Human);

    let mut targets = Vec::new();

    if !std::io::stdin().is_terminal() {
        eprintln!(
            "{} Error: Cannot run the Interactive TUI while piping stdin data.",
            "❌".red()
        );
        std::process::exit(1);
    }

    targets.extend(args.paths.clone());

    // TUI Mode
    if let Some(selected_targets) = filegoblin_tui::run_tui(&mut args)? {
        if !args.quiet {
            eprintln!(
                "{}",
                "    (^w^)  <- \"Crunching time!\""
                    .truecolor(167, 255, 0)
                    .bold()
            );
        }

        gobble_app(&selected_targets, &parsed_flavor, &args)?;
    }

    Ok(())
}

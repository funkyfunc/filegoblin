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

mod cli;
use cli::Cli;

fn main() -> Result<()> {
    // Parse arguments and emit the initial "Crunching..." or Goblinism
    let args = Cli::parse();

    let parsed_flavor = Flavor::from_str(&args.flavor).unwrap_or(Flavor::Human);

    if !args.quiet {
        eprintln!("{}", ASCII_MASCOT.truecolor(167, 255, 0).bold());
        eprintln!("{}", "Hello Goblin!".truecolor(167, 255, 0).bold());
    }

    // Initialize core library configurations
    gobble_app(
        &args.path,
        &parsed_flavor,
        args.full,
        args.horde,
        args.split,
        args.tokens,
        args.quiet,
        args.json,
        args.scrub,
    )?;

    Ok(())
}

use anyhow::Result;
use clap::Parser;
use colored::*;
use filegoblin::{flavors::Flavor, gobble_app};
use std::str::FromStr;
use std::io::IsTerminal;

const ASCII_MASCOT: &str = r#"
    (o_o)  <-- "I'm hungry for files."
     (W)
   --m-m--  filegoblin v1.5.0
"#;

mod cli;
use cli::Cli;

mod ui;

fn main() -> Result<()> {
    // Parse arguments and emit the initial "Crunching..." or Goblinism
    let mut args = Cli::parse();

    let parsed_flavor = Flavor::from_str(&args.flavor).unwrap_or(Flavor::Human);

    let mut targets = Vec::new();

    // 1. Detect if we have a piped stdin stream
    if !std::io::stdin().is_terminal() {
        targets.push("-".to_string());
    }

    // 2. Add the positional path argument if it exists
    if let Some(p) = &args.path {
        targets.push(p.clone());
    }

    // 3. Prevent running with no targets
    if targets.is_empty() && !args.interactive {
        eprintln!("{} Error: No target path provided and no stdin stream detected.", "❌".red());
        eprintln!("Usage: fg <target>  --OR--  cat <file> | fg");
        std::process::exit(1);
    }

    if args.interactive {
        if targets.contains(&"-".to_string()) {
            eprintln!("{} Error: Cannot run the Interactive TUI (-i) while piping stdin data.", "❌".red());
            std::process::exit(1);
        }
        // TUI Mode hijacks the execution
        if let Some(selected_paths) = ui::run_tui(&mut args)? {
            // If the user selected files and pressed enter, execute gobble on them
            if !args.quiet {
                eprintln!("{}", ASCII_MASCOT.truecolor(167, 255, 0).bold());
                eprintln!("{}", "Hello Goblin!".truecolor(167, 255, 0).bold());
            }

            let targets: Vec<String> = selected_paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();

            gobble_app(
                &targets,
                &parsed_flavor,
                args.full,
                args.horde, // Horde is likely false if they selected a specific file, but pass the arg anyway
                args.split,
                args.write.as_deref(),
                args.tokens,
                args.quiet,
                args.json,
                args.scrub,
                args.copy,
                args.open,
            )?;
        }
        return Ok(());
    }

    if !args.quiet {
        eprintln!("{}", ASCII_MASCOT.truecolor(167, 255, 0).bold());
        eprintln!("{}", "Hello Goblin!".truecolor(167, 255, 0).bold());
    }

    gobble_app(
        &targets,
        &parsed_flavor,
        args.full,
        args.horde,
        args.split,
        args.write.as_deref(),
        args.tokens,
        args.quiet,
        args.json,
        args.scrub,
        args.copy,
        args.open,
    )?;

    Ok(())
}

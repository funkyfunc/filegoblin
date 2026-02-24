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

    // Establish core invocation closure
    let run = || -> Result<()> {
        gobble_app(
            &targets,
            &parsed_flavor,
            args.full,
            args.horde,
            args.split,
            args.tokens,
            args.quiet,
            args.json,
            args.scrub,
            args.copy,
            args.open,
        )
    };

    if args.watch {
        // Run once initially
        if let Err(e) = run() {
            eprintln!("{} Error: {}", "❌".red(), e);
        }

        if !args.quiet {
            eprintln!("{}", "👀 Watching for changes...".truecolor(0, 255, 100));
        }

        let (tx, rx) = channel();
        // A simple debounced watcher is preferred, but standard watcher is fine for Phase V MVP
        let mut watcher = notify::recommended_watcher(tx)?;
        
        let target_path = args.path.as_deref().map(std::path::Path::new);
        
        // Ensure path exists before watching
        if let Some(path) = target_path {
            if path.exists() {
                 watcher.watch(path, RecursiveMode::Recursive)?;
            } else {
                 // Handle web URLs which we obviously can't "watch" locally
                 eprintln!("{} Cannot watch a non-local path or non-existent file.", "⚠️".yellow());
                 return Ok(());
            }
        } else {
             eprintln!("{} Cannot watch a stdin stream.", "⚠️".yellow());
             return Ok(());
        }

        // Blocking loop
        for res in rx {
            match res {
                Ok(event) => {
                    // Filter out access events, we only care about modifies/creates/removes
                    let kind = event.kind;
                    if kind.is_modify() || kind.is_create() || kind.is_remove() {
                        if !args.quiet {
                            eprintln!("\n{} File changed, re-gobbling...", "🔄".truecolor(0, 200, 255));
                        }
                        if let Err(e) = run() {
                            eprintln!("{} Error: {}", "❌".red(), e);
                        }
                        if !args.quiet {
                            eprintln!("{}", "👀 Watching for changes...".truecolor(0, 255, 100));
                        }
                    }
                }
                Err(error) => eprintln!("{} Watch error: {:?}", "❌".red(), error),
            }
        }
    } else {
        run()?;
    }

    Ok(())
}

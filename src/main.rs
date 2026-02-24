use anyhow::Result;
use clap::Parser;
use colored::*;
use filegoblin::{flavors::Flavor, gobble_app};
use std::str::FromStr;
use std::sync::mpsc::channel;
use notify::{Watcher, RecursiveMode};

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

    // Establish core invocation closure
    let run = || -> Result<()> {
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
        
        let target_path = std::path::Path::new(&args.path);
        
        // Ensure path exists before watching
        if target_path.exists() {
             watcher.watch(target_path, RecursiveMode::Recursive)?;
        } else {
             // Handle web URLs which we obviously can't "watch" locally
             eprintln!("{} Cannot watch a non-local path or non-existent file.", "⚠️".yellow());
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

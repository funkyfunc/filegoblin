use anyhow::Result;
use clap::Parser;
use colored::*;
use filegoblin::{flavors::Flavor, gobble_app};
use std::str::FromStr;
use std::io::IsTerminal;

fn print_mascot() {
    use std::{thread, time::Duration};

    let is_tty = std::io::stderr().is_terminal();

    let frames = [
        vec![
            r#"    (-_-)  <-- "...""#,
            r#"     (W)"#,
            r#"   --m-m--  filegoblin v1.5.0"#,
        ],
        vec![
            r#"    (o_o)  <-- "I'm hungry for files.""#,
            r#"     (W)"#,
            r#"   --m-m--  filegoblin v1.5.0"#,
        ],
        vec![
            r#"    (^w^)  <-- "Crunching time!""#,
            r#"     (V)"#,
            r#"   --m-m--  filegoblin v1.5.0"#,
        ],
    ];

    if !is_tty {
        // Fallback for non-TTY
        for line in &frames[1] {
            eprintln!("{}", line);
        }
        return;
    }

    for (i, frame) in frames.iter().enumerate() {
        if i > 0 {
            eprint!("\x1b[3A");
        }
        for line in frame.iter() {
            let r = 167 + ((255 - 167) / 2) * (i as u8);
            let g = 255 - (20 * i as u8);
            let b = 0;
            eprintln!("\x1b[2K{}", line.truecolor(r, g, b).bold());
        }
        if i < frames.len() - 1 {
            thread::sleep(Duration::from_millis(150));
        }
    }
    eprintln!("\x1b[2K{}", "Hello Goblin!".truecolor(167, 255, 0).bold());
}

use filegoblin::cli::Cli;

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

    // 2. Add the positional path arguments if they exist
    targets.extend(args.paths.clone());

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
                print_mascot();
            }

            let targets: Vec<String> = selected_paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();

            gobble_app(
                &targets,
                &parsed_flavor,
                args.compress.as_ref(),
                args.full,
                args.horde, // Horde is likely false if they selected a specific file, but pass the arg anyway
                args.split,
                args.chunk.as_deref(),
                args.write.as_deref(),
                args.tokens,
                args.quiet,
                args.json,
                args.scrub,
                args.copy,
                args.open,
                args.plugin.as_deref(),
            )?;
        }
        return Ok(());
    }

    if !args.quiet {
        print_mascot();
    }

    gobble_app(
        &targets,
        &parsed_flavor,
        args.compress.as_ref(),
        args.full,
        args.horde,
        args.split,
        args.chunk.as_deref(),
        args.write.as_deref(),
        args.tokens,
        args.quiet,
        args.json,
        args.scrub,
        args.copy,
        args.open,
        args.plugin.as_deref(),
    )?;

    Ok(())
}

use anyhow::Result;
use clap::Parser;
use colored::*;
use filegoblin::{flavors::Flavor, gobble_app};
use std::io::IsTerminal;
use std::str::FromStr;

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
        eprintln!(
            "{} Error: No target path provided and no stdin stream detected.",
            "❌".red()
        );
        eprintln!("Usage: fg <target>  --OR--  cat <file> | fg");
        std::process::exit(1);
    }

    if args.interactive {
        if targets.contains(&"-".to_string()) {
            eprintln!(
                "{} Error: Cannot run the Interactive TUI (-i) while piping stdin data.",
                "❌".red()
            );
            std::process::exit(1);
        }
        // TUI Mode hijacks the execution
        if let Some(selected_targets) = ui::run_tui(&mut args)? {
            // If the user selected files and pressed enter, execute gobble on them
            if !args.quiet {
                print_mascot();
            }

            let targets: Vec<String> = selected_targets;

            gobble_app(&targets, &parsed_flavor, &args)?;
        }
        return Ok(());
    }

    if !args.quiet {
        print_mascot();
    }

    gobble_app(&targets, &parsed_flavor, &args)?;

    if args.watch {
        use notify::{EventKind, RecursiveMode, Watcher};
        use std::sync::mpsc::channel;
        use std::time::Duration;

        if !args.quiet {
            eprintln!(
                "\n{}",
                "👀 Watch mode active. Monitoring targets for changes..."
                    .truecolor(255, 191, 0)
                    .bold()
            );
        }

        let (tx, rx) = channel();

        // Use RecommendedWatcher
        let mut watcher = notify::RecommendedWatcher::new(tx, notify::Config::default())?;

        // Add all targets to the watcher
        for target in &targets {
            let path = std::path::Path::new(target);
            if path.exists() {
                let _ = watcher.watch(path, RecursiveMode::Recursive);
            }
        }

        let mut last_run = std::time::Instant::now();
        let debounce_duration = Duration::from_millis(500); // 500ms debounce

        loop {
            match rx.recv() {
                Ok(Ok(event)) => {
                    // Only react to actual modifications or creates
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                            if last_run.elapsed() >= debounce_duration {
                                if !args.quiet {
                                    // Clear screen
                                    print!("\x1B[2J\x1B[1;1H");
                                    eprintln!(
                                        "{}",
                                        format!(
                                            "🔄 File changed: {:?}. Regenerating...",
                                            event
                                                .paths
                                                .first()
                                                .unwrap_or(&std::path::PathBuf::new())
                                        )
                                        .truecolor(0, 200, 255)
                                    );
                                }
                                if let Err(e) = gobble_app(&targets, &parsed_flavor, &args) {
                                    eprintln!("{} Watch regeneration failed: {}", "⚠️".yellow(), e);
                                }
                                last_run = std::time::Instant::now();
                                if !args.quiet {
                                    eprintln!(
                                        "\n{}",
                                        "👀 Watching for changes...".truecolor(255, 191, 0).bold()
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Err(e)) => eprintln!("{} Watch error: {:?}", "⚠️".red(), e),
                Err(_) => break, // Channel closed
            }
        }
    }

    Ok(())
}

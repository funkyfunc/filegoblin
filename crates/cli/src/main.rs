use anyhow::Result;
use clap::Parser;
use colored::*;
use filegoblin_core::gobble_app;
use std::io::IsTerminal;

fn print_mascot() {
    use std::{thread, time::Duration};

    let is_tty = std::io::stderr().is_terminal();

    let tagline = format!("   --m-m--  filegoblin v{}", env!("CARGO_PKG_VERSION"));
    let frames = [
        vec![
            r#"    (-_-)  <-- "...""#.to_string(),
            r#"     (W)"#.to_string(),
            tagline.clone(),
        ],
        vec![
            r#"    (o_o)  <-- "I'm hungry for files.""#.to_string(),
            r#"     (W)"#.to_string(),
            tagline.clone(),
        ],
        vec![
            r#"    (^w^)  <-- "Crunching time!""#.to_string(),
            r#"     (V)"#.to_string(),
            tagline.clone(),
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
            eprintln!("\x1b[2K{}", line.as_str().truecolor(r, g, b).bold());
        }
        if i < frames.len() - 1 {
            thread::sleep(Duration::from_millis(150));
        }
    }
    eprintln!("\x1b[2K{}", "Hello Goblin!".truecolor(167, 255, 0).bold());
}

use filegoblin_core::cli::Cli;

fn main() -> Result<()> {
    #[cfg_attr(not(feature = "tui"), allow(unused_mut))]
    let mut args = Cli::parse();

    let parsed_flavor = args.flavor.clone();

    // Explicit paths win. Only when NO paths are given do we fall back to a piped
    // stdin ("cat x | filegoblin" works bare) — never in addition to paths, so
    // `filegoblin file.md` spawned with an open-but-idle stdin pipe (the normal
    // subprocess case for agents) neither blocks on stdin nor grows a phantom
    // "-" target in the output metadata.
    let mut targets = args.paths.clone();
    if targets.is_empty() && !args.interactive && !args.clipboard {
        if !std::io::stdin().is_terminal() {
            targets.push("-".to_string());
        } else {
            eprintln!(
                "{} Error: No target path provided and no stdin stream detected.",
                "❌".red()
            );
            eprintln!("Usage: filegoblin <target>  --OR--  cat <file> | filegoblin");
            std::process::exit(2);
        }
    }

    if args.interactive {
        #[cfg(feature = "tui")]
        {
            if let Some(selected_targets) = filegoblin_tui::run_tui(&mut args)? {
                if !args.quiet {
                    eprintln!(
                        "{}",
                        "    (^w^)  <- \"Crunching time!\""
                            .truecolor(167, 255, 0)
                            .bold()
                    );
                }
                let outcome = gobble_app(&selected_targets, &parsed_flavor, &args)?;
                if args.scrub_fail && outcome.redactions > 0 {
                    std::process::exit(3);
                }
                if outcome.produced == 0 {
                    std::process::exit(4);
                }
            }
            return Ok(());
        }
        #[cfg(not(feature = "tui"))]
        {
            eprintln!(
                "{} Interactive mode requires the 'tui' feature.",
                "❌".red()
            );
            eprintln!("Rebuild with: cargo install --path crates/cli --features tui");
            std::process::exit(1);
        }
    }

    if !args.quiet {
        print_mascot();
    }

    let outcome = gobble_app(&targets, &parsed_flavor, &args)?;

    // Exit-code contract, checked before watch mode (a long-running loop):
    //  - `--scrub-fail`: non-zero (3) when the Privacy Shield redacted anything.
    //  - non-zero (4) when nothing could be extracted from any target, so an
    //    agent/script never mistakes a silent no-op for success. Applies to every
    //    output mode, not just `--json`.
    if !args.watch {
        if args.scrub_fail && outcome.redactions > 0 {
            std::process::exit(3);
        }
        if outcome.produced == 0 {
            if !args.quiet {
                eprintln!(
                    "{} Nothing could be extracted from the given target(s).",
                    "❌".red()
                );
            }
            std::process::exit(4);
        }
    }

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
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                            if last_run.elapsed() >= debounce_duration =>
                        {
                            if !args.quiet {
                                // Clear screen
                                print!("\x1B[2J\x1B[1;1H");
                                eprintln!(
                                    "{}",
                                    format!(
                                        "🔄 File changed: {:?}. Regenerating...",
                                        event.paths.first().unwrap_or(&std::path::PathBuf::new())
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

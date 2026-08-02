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

/// True when the failure is just our stdout reader hanging up (`filegoblin x | head`).
/// Standard pipeline citizens exit quietly on that, they don't print an error.
fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

fn main() -> Result<()> {
    #[cfg_attr(not(feature = "tui"), allow(unused_mut))]
    let mut args = Cli::parse();

    filegoblin_core::cache::set_enabled(!args.no_cache);

    // MCP server mode owns stdin/stdout entirely: no mascot, no stdin-as-target
    // fallback, no path requirement.
    if args.mcp {
        return filegoblin_core::mcp::serve();
    }

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
                let outcome = gobble_app(&selected_targets, &args)?;
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

    let outcome = match gobble_app(&targets, &args) {
        Ok(outcome) => outcome,
        Err(e) if is_broken_pipe(&e) => std::process::exit(0),
        Err(e) => return Err(e),
    };

    // Exit-code contract:
    //  - `--scrub-fail`: non-zero (3) when the Privacy Shield redacted anything.
    //  - non-zero (4) when nothing could be extracted from any target, so an
    //    agent/script never mistakes a silent no-op for success. Applies to every
    //    output mode, not just `--json`.
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

    Ok(())
}

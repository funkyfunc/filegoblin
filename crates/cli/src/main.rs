mod walk;

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
    use filegoblin_core::parsers::pdf_worker;

    // This binary re-run to convert PDFs for its parent, one process away
    // from anything a hostile PDF can crash.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == pdf_worker::WORKER_FLAG)
    {
        return pdf_worker::serve();
    }

    let mut args = Cli::parse();

    if let Err(e) = pdf_worker::enable() {
        eprintln!("{} {e:#}", "❌".red());
        std::process::exit(2);
    }

    filegoblin_core::cache::set_enabled(!args.no_cache);

    if args.cache_clear {
        let freed = filegoblin_core::cache::clear_all()?;
        if !args.quiet {
            eprintln!("🧹 Cache cleared ({} MB freed).", freed / (1024 * 1024));
        }
        return Ok(());
    }

    if args.models_status {
        #[cfg(feature = "vision")]
        print!("{}", filegoblin_core::vision::models::status_report());
        #[cfg(not(feature = "vision"))]
        println!("This build has no PDF vision support (built without the `vision` feature).");
        return Ok(());
    }
    if let Some(dir) = args.install_models.clone() {
        #[cfg(feature = "vision")]
        {
            if filegoblin_core::vision::models::bundled_build() {
                eprintln!("This binary already has the models built in; nothing to install.");
                return Ok(());
            }
            filegoblin_core::vision::install::install(dir, args.quiet)?;
            return Ok(());
        }
        #[cfg(not(feature = "vision"))]
        {
            let _ = dir;
            anyhow::bail!(
                "this build has no PDF vision support (built without the `vision` feature)"
            );
        }
    }

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
    if targets.is_empty() && !args.clipboard {
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

    // `--walk` replaces the target list with whatever the user picked, then
    // hands off to the identical pipeline — so scrubbing, compression, output
    // modes and exit codes all behave exactly as they do without it.
    if args.walk {
        let Some(root) = targets.first().cloned() else {
            eprintln!("{} --walk needs a target to start from.", "❌".red());
            std::process::exit(2);
        };
        match walk::run(&root, &args)? {
            Some(picked) => {
                // The picker offers a directory as "include everything beneath
                // it", so honour that: without horde, a directory target falls
                // through to the single-file reader and errors.
                if picked.iter().any(|t| std::path::Path::new(t).is_dir()) {
                    args.horde = true;
                }
                targets = picked;
            }
            None => {
                eprintln!("Nothing selected — no output.");
                std::process::exit(4);
            }
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

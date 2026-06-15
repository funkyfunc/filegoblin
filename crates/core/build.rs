use clap::CommandFactory;
use clap_complete::{generate_to, shells::Shell};
use clap_mangen::Man;
use std::fs;
use std::path::PathBuf;

include!("src/cli.rs");

fn main() -> std::io::Result<()> {
    // Generate explicitly into standard target structure
    let generated_dir = PathBuf::from("target/generated");
    fs::create_dir_all(&generated_dir)?;

    let mut cmd = Cli::command();

    // Generate Manpage (filegoblin.1)
    let man = Man::new(cmd.clone());
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;
    fs::write(generated_dir.join("filegoblin.1"), buffer)?;

    // Generate Zsh Completion
    generate_to(Shell::Zsh, &mut cmd, "filegoblin", &generated_dir)?;

    // Generate Bash Completion
    generate_to(Shell::Bash, &mut cmd, "filegoblin", &generated_dir)?;

    println!(
        "cargo:warning=Generated manpage and completions in {}",
        generated_dir.display()
    );

    Ok(())
}

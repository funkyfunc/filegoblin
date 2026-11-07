use clap::CommandFactory;
use clap_complete::{generate_to, shells::Shell};
use clap_mangen::Man;
use std::fs;
use std::path::Path;

include!("src/cli.rs");

fn main() -> std::io::Result<()> {
    // Generate explicitly into standard target structure
    // We navigate up to the workspace root to ensure it behaves consistently
    let generated_dir = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/generated");
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

    if std::env::var_os("CARGO_FEATURE_BUNDLED_MODELS").is_some() {
        bundle_models()?;
    }

    println!(
        "cargo:warning=Generated manpage and completions in {}",
        generated_dir.display()
    );

    Ok(())
}

/// The model files a `bundled-models` build carries, by name.
const BUNDLED: &[&str] = &[
    "pp-doclayout-v3.onnx",
    "mfr-encoder.onnx",
    "mfr-decoder-init.onnx",
    "mfr-decoder-step.onnx",
    "mfr-tokenizer.json",
    "tatr-structure.onnx",
    "ppocr-v6m-det.onnx",
    "ppocr-v6m-rec.onnx",
    "ppocr-v6-dict.txt",
    "lighton-vision.onnx",
    "lighton-embed.onnx",
    "lighton-step.onnx",
    "lighton-vocab.json",
];

/// Writes `$OUT_DIR/bundled.rs`, which embeds each file in `BUNDLED` from
/// `$FILEGOBLIN_MODELS_DIR` and defines `bundled_file(name)`.
///
/// The assembler copies the bytes in (`.incbin`), so the compiler never holds
/// them. With `include_bytes!` rustc took about 9.4 GB for the 1.3 GB set (4.3 GB
/// this way), more than a 16 GB build runner could spare for one crate.
fn bundle_models() -> std::io::Result<()> {
    println!("cargo:rerun-if-env-changed=FILEGOBLIN_MODELS_DIR");
    // Naming any file to watch stops Cargo from rerunning this script when the
    // rest of the package changes, so the manpage sources are named too.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    let dir = std::env::var("FILEGOBLIN_MODELS_DIR").map_err(|_| {
        std::io::Error::other("the bundled-models feature needs FILEGOBLIN_MODELS_DIR")
    })?;
    let vendor = std::env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let (section, prefix) = if vendor == "apple" {
        (".section __TEXT,__const", "_")
    } else if os == "windows" {
        (".section .rdata,\"dr\"", "")
    } else {
        (".section .rodata.filegoblin_models,\"a\"", "")
    };
    let mut asm = vec![section.to_string()];
    let mut externs = String::new();
    let mut arms = String::new();
    for (i, name) in BUNDLED.iter().enumerate() {
        let path = Path::new(&dir).join(name);
        let len = fs::metadata(&path)
            .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?
            .len();
        println!("cargo:rerun-if-changed={}", path.display());
        // Forward slashes work on every host, and a backslash would be an
        // escape in the assembler's string.
        let p = path
            .to_str()
            .expect("model path is UTF-8")
            .replace('\\', "/");
        assert!(!p.contains('"'), "model path has a quote: {p}");
        let sym = format!("filegoblin_model_{i}");
        asm.push(".p2align 6".into());
        asm.push(format!(".globl {prefix}{sym}"));
        asm.push(format!("{prefix}{sym}:"));
        asm.push(format!(".incbin \"{p}\""));
        externs.push_str(&format!("    static {sym}: u8;\n"));
        arms.push_str(&format!("        {name:?} => (&raw const {sym}, {len}),\n"));
    }
    asm.push(".text".into());
    let lines: Vec<String> = asm
        .iter()
        .map(|l| format!("    {:?},", l.replace('{', "{{").replace('}', "}}")))
        .collect();
    let src = format!(
        "core::arch::global_asm!(\n{}\n);\n\n\
         unsafe extern \"C\" {{\n{externs}}}\n\n\
         /// The bytes of bundled file `name`, if this build carries it.\n\
         fn bundled_file(name: &str) -> Option<&'static [u8]> {{\n\
         \x20   let (start, len) = match name {{\n{arms}        _ => return None,\n    }};\n\
         \x20   // SAFETY: the symbol starts `len` bytes the assembler copied from\n\
         \x20   // the file, in a read-only section that lives as long as the program.\n\
         \x20   Some(unsafe {{ core::slice::from_raw_parts(start, len as usize) }})\n\
         }}\n",
        lines.join("\n")
    );
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("bundled.rs");
    fs::write(out, src)
}

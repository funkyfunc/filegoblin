use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::fs;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

wasmtime::component::bindgen!({
    path: "wit/gobbler.wit",
    world: "gobbler",
});

pub struct WasmGobbler {
    pub wasm_path: PathBuf,
}

impl Gobble for WasmGobbler {
    fn gobble(&self, path: &Path, _flags: &crate::cli::Cli) -> Result<String> {
        let file_bytes = fs::read(path).context("Failed to read file for WASM parsing")?;
        
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        let mut config = Config::new();
        config.wasm_component_model(true);
        
        let engine = Engine::new(&config).context("Failed to initialize WASM engine")?;
        let component = Component::from_file(&engine, &self.wasm_path)
            .context(format!("Failed to load WASM component from {:?}", self.wasm_path))?;

        let linker = Linker::new(&engine);
        let mut store = Store::new(&engine, ());

        let bindings = Gobbler::instantiate(&mut store, &component, &linker)
            .context("Failed to instantiate WASM plugin")?;

        match bindings.filegoblin_plugin_parser().call_gobble(&mut store, &file_bytes, &extension)? {
            Ok(markdown) => Ok(markdown),
            Err(e) => anyhow::bail!("WASM Plugin '{}' Error: {}", extension, e),
        }
    }
}

impl WasmGobbler {
    /// Attempts to locate a plugin named `{ext}.wasm` in `~/.filegoblin/plugins/` or `./plugins/`
    pub fn sniff(ext: &str) -> Option<PathBuf> {
        let file_name = format!("{}.wasm", ext);
        
        let local_path = std::env::current_dir().ok()?.join("plugins").join(&file_name);
        if local_path.exists() {
            return Some(local_path);
        }

        let home_path = home::home_dir()?.join(".filegoblin").join("plugins").join(&file_name);
        if home_path.exists() {
            return Some(home_path);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_dummy_plugin_component() {
        // We assume `examples/dummy_plugin.wasm` exists from our cargo run command
        let wasm_file = Path::new("examples/dummy_plugin.wasm");
        if !wasm_file.exists() {
             eprintln!("Skipping WASM component test since dummy_plugin.wasm is not built.");
             return;
        }

        // Create a fake `.gob` file
        let target_file = Path::new("dummy_test.gob");
        let mut f = File::create(target_file).unwrap();
        f.write_all(b"Hello Goblin!").unwrap();

        let gobbler = WasmGobbler { wasm_path: wasm_file.to_path_buf() };
        let args = crate::cli::Cli::parse_from(&["filegoblin"]);
        let result = gobbler.gobble(target_file, &args).unwrap();

        assert!(result.contains("DUMMY PARSER HIT!"));
        assert!(result.contains("Hello Goblin!"));

        std::fs::remove_file(target_file).unwrap();
    }
}

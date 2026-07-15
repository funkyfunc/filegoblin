use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

wasmtime::component::bindgen!({
    path: "wit/gobbler.wit",
    world: "gobbler",
});

/// CPU budget: a generous fuel allowance so well-behaved plugins finish but a
/// runaway (infinite loop) plugin is trapped instead of hanging the process.
const PLUGIN_FUEL: u64 = 10_000_000_000;
/// Memory ceiling a plugin may allocate (256 MiB).
const PLUGIN_MAX_MEMORY: usize = 256 * 1024 * 1024;

/// Per-store host state: carries the resource limiter.
struct HostState {
    limits: StoreLimits,
}

/// A single shared engine. Building an `Engine` is expensive and it is safe to
/// reuse across plugin invocations, so we cache it rather than rebuilding per file.
fn engine() -> Result<&'static Engine> {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    if let Some(e) = ENGINE.get() {
        return Ok(e);
    }
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.consume_fuel(true);
    let engine = Engine::new(&config).context("Failed to initialize WASM engine")?;
    Ok(ENGINE.get_or_init(|| engine))
}

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

        let engine = engine()?;
        let component = Component::from_file(engine, &self.wasm_path).context(format!(
            "Failed to load WASM component from {:?}",
            self.wasm_path
        ))?;

        // The linker is intentionally empty: no WASI, so plugins get no filesystem,
        // network, clock, or stdio access. They only see the `gobble` interface.
        let linker = Linker::new(engine);

        let limits = StoreLimitsBuilder::new()
            .memory_size(PLUGIN_MAX_MEMORY)
            .build();
        let mut store = Store::new(engine, HostState { limits });
        store.limiter(|state| &mut state.limits);
        store
            .set_fuel(PLUGIN_FUEL)
            .context("Failed to set WASM fuel budget")?;

        let bindings = Gobbler::instantiate(&mut store, &component, &linker)
            .context("Failed to instantiate WASM plugin")?;

        match bindings.filegoblin_plugin_parser().call_gobble(
            &mut store,
            &file_bytes,
            &extension,
        )? {
            Ok(markdown) => Ok(markdown),
            Err(e) => anyhow::bail!("WASM Plugin '{}' Error: {}", extension, e),
        }
    }
}

impl WasmGobbler {
    /// Locates a plugin `{ext}.wasm`. `allow_cwd` controls whether the
    /// current-directory `./plugins/` folder is searched: it is only enabled for
    /// the explicit `--plugin` path, never for implicit extension-based dispatch,
    /// so running filegoblin inside a hostile directory cannot silently execute
    /// its bundled WASM.
    pub fn sniff(ext: &str, allow_cwd: bool) -> Option<PathBuf> {
        let file_name = format!("{}.wasm", ext);

        if allow_cwd && let Ok(cwd) = std::env::current_dir() {
            let local_path = cwd.join("plugins").join(&file_name);
            if local_path.exists() {
                return Some(local_path);
            }
        }

        let home_path = home::home_dir()?
            .join(".filegoblin")
            .join("plugins")
            .join(&file_name);
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

        let gobbler = WasmGobbler {
            wasm_path: wasm_file.to_path_buf(),
        };
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(target_file, &args).unwrap();

        assert!(result.contains("DUMMY PARSER HIT!"));
        assert!(result.contains("Hello Goblin!"));

        std::fs::remove_file(target_file).unwrap();
    }
}

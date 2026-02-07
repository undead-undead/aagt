use crate::error::{Error, Result};
use std::path::Path;
use tracing::debug;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::*;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

/// Host state for Wasm execution
struct HostState {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

/// A high-performance Wasm runtime for agent skills
pub struct WasmRuntime {
    engine: Engine,
}

impl WasmRuntime {
    /// Create a new Wasm runtime
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        // wasmtime 29 defaults
        config.wasm_component_model(true);
        config.async_support(false); // We'll run it in a spawned blocking task if needed

        let engine = Engine::new(&config)
            .map_err(|e| Error::Internal(format!("Failed to create Wasm engine: {}", e)))?;

        Ok(Self { engine })
    }

    /// Execute a Wasm skill
    pub fn call(&self, wasm_path: &Path, arguments: &str) -> Result<String> {
        let component = Component::from_file(&self.engine, wasm_path)
            .map_err(|e| Error::Internal(format!("Failed to load Wasm component: {}", e)))?;

        let wasi = WasiCtxBuilder::new()
            .inherit_stdout()
            .inherit_stderr()
            .build();

        let mut store = Store::new(
            &self.engine,
            HostState {
                wasi_ctx: wasi,
                table: ResourceTable::new(),
            },
        );

        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|e| Error::Internal(format!("Failed to link WASI: {}", e)))?;

        let instance = linker
            .instantiate(&mut store, &component)
            .map_err(|e| Error::Internal(format!("Failed to instantiate Wasm component: {}", e)))?;

        // 1. Try to call run(input: string) -> string
        // In the component model, if we didn't generate bindings, we use Val/Func
        if let Some(run_func) = instance.get_func(&mut store, "run") {
            use wasmtime::component::Val;

            let mut results = [Val::Bool(false)]; // Placeholder for result
            let args = [Val::String(arguments.to_string().into())];

            debug!(
                "Executing Wasm skill with arguments (string-based) at {:?}",
                wasm_path
            );

            // Note: This assumes the 'run' function signature is (string) -> string
            // If it's different, this will fail at runtime.
            match run_func.call(&mut store, &args, &mut results) {
                Ok(_) => {
                    if let Some(Val::String(s)) = results.get(0) {
                        return Ok(s.to_string());
                    }
                    return Ok("Wasm execution completed (No string result returned)".to_string());
                }
                Err(e) => {
                    debug!(
                        "Parameterized 'run' failed, falling back to parameterless: {}",
                        e
                    );
                }
            }
        }

        // 2. Fallback to basic 'run()' without params (legacy/simple)
        if let Some(run_func) = instance.get_func(&mut store, "run") {
            debug!("Executing Wasm skill (parameterless) at {:?}", wasm_path);
            run_func
                .call(&mut store, &[], &mut [])
                .map_err(|e| Error::Internal(format!("Wasm execution failed: {}", e)))?;

            return Ok(
                "Wasm execution completed (MVP: Result should be in stdout/side-effects)"
                    .to_string(),
            );
        }

        Err(Error::Internal(
            "Wasm component must export a 'run' function".to_string(),
        ))
    }
}

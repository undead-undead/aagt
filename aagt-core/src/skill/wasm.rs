//! WASM Runtime for Dynamic Skills
//!
//! This module uses `wasmtime` to execute WASM modules as skills.
//! It provides a secure sandboxed environment with limited access to host resources.

use wasmtime::*;
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;
use crate::error::{Error, Result};

/// A WASM-based skill runtime
pub struct WasmRuntime {
    engine: Engine,
    module: Module,
}

impl WasmRuntime {
    /// Create a new WASM runtime from bytes
    pub fn new(wasm_bytes: &[u8]) -> Result<Self> {
        let mut config = Config::new();
        config.async_support(true);
        
        let engine = Engine::new(&config)
            .map_err(|e| Error::Internal(format!("Failed to create Wasm engine: {}", e)))?;
            
        let module = Module::new(&engine, wasm_bytes)
            .map_err(|e| Error::Internal(format!("Failed to compile Wasm module: {}", e)))?;
            
        Ok(Self { engine, module })
    }

    /// Call the exported `call` function in the WASM module
    pub async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let mut linker = Linker::<WasiP1Ctx>::new(&self.engine);
        preview1::add_to_linker_async(&mut linker, |t| t)?;

        let wasi = WasiCtxBuilder::new()
            .inherit_stdout()
            .inherit_stderr()
            .build_p1();

        let mut store = Store::new(&self.engine, wasi);
        
        let instance = linker.instantiate_async(&mut store, &self.module).await
            .map_err(|e| anyhow::anyhow!("Failed to instantiate Wasm module: {}", e))?;

        // ABI: We expect an exported function `call(ptr, len)` that returns a `u64` (ptr << 32 | len)
        // For simplicity in this first version, we'll use a simpler approach or a well-defined ABI.
        // Let's assume a function `allocate(size) -> ptr` and `call(ptr, len) -> u64`
        
        let call_fn = instance.get_typed_func::<(u32, u32), u64>(&mut store, "call")
            .map_err(|_| anyhow::anyhow!("Wasm module must export 'call(u32, u32) -> u64'"))?;
        
        let alloc_fn = instance.get_typed_func::<u32, u32>(&mut store, "allocate")
            .map_err(|_| anyhow::anyhow!("Wasm module must export 'allocate(u32) -> u32'"))?;

        let memory = instance.get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow::anyhow!("Wasm module must export 'memory'"))?;

        // 1. Allocate space for input
        let input_bytes = arguments.as_bytes();
        let input_len = input_bytes.len() as u32;
        let input_ptr = alloc_fn.call_async(&mut store, input_len).await?;

        // 2. Write input to memory
        memory.write(&mut store, input_ptr as usize, input_bytes)?;

        // 3. Call the function
        let packed_result = call_fn.call_async(&mut store, (input_ptr, input_len)).await?;
        
        // 4. Extract result string
        let result_ptr = (packed_result >> 32) as u32 as usize;
        let result_len = (packed_result & 0xFFFFFFFF) as u32 as usize;
        
        let mut result_bytes = vec![0u8; result_len];
        memory.read(&mut store, result_ptr, &mut result_bytes)?;
        
        Ok(String::from_utf8_lossy(&result_bytes).to_string())
    }
}

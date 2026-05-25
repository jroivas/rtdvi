//! Thin re-export layer that unifies the wasmi and wasmtime WASM runtime APIs.
//!
//! Build with wasmtime (default):  `cargo build`
//! Build with wasmi (lightweight):  `cargo build --no-default-features --features runtime-wasmi`

#[cfg(all(feature = "runtime-wasmtime", feature = "runtime-wasmi"))]
compile_error!("'runtime-wasmtime' and 'runtime-wasmi' are mutually exclusive");
#[cfg(not(any(feature = "runtime-wasmtime", feature = "runtime-wasmi")))]
compile_error!("Enable exactly one of 'runtime-wasmtime' or 'runtime-wasmi'");

#[cfg(feature = "runtime-wasmtime")]
pub use wasmtime::{Caller, Engine, Extern, Instance, Linker, Memory, Module, Store, TypedFunc};
#[cfg(feature = "runtime-wasmi")]
pub use wasmi::{Caller, Engine, Extern, Instance, Linker, Memory, Module, Store, TypedFunc};

/// Create an engine with settings appropriate for the active runtime.
///
/// wasmtime: enables the WASM exceptions proposal (required for mlua-wasm).
/// wasmi:    uses default settings (no exceptions proposal support).
pub fn make_engine() -> Engine {
    #[cfg(feature = "runtime-wasmtime")]
    {
        let mut config = wasmtime::Config::new();
        config.wasm_exceptions(true);
        wasmtime::Engine::new(&config).expect("wasmtime engine init")
    }
    #[cfg(feature = "runtime-wasmi")]
    {
        Engine::default()
    }
}

/// Instantiate a module. Runs the start function if present.
///
/// wasmi calls this `instantiate_and_start`; wasmtime calls it `instantiate`.
pub fn instantiate<T>(
    linker: &Linker<T>,
    store: &mut Store<T>,
    module: &Module,
) -> anyhow::Result<Instance> {
    #[cfg(feature = "runtime-wasmtime")]
    {
        Ok(linker.instantiate(store, module)?)
    }
    #[cfg(feature = "runtime-wasmi")]
    {
        Ok(linker.instantiate_and_start(store, module)?)
    }
}

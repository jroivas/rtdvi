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
/// wasmtime: default config (no WASM exceptions proposal needed).
///   The mlua-wasm binary has all EH instructions stripped by wasm-opt --strip-eh.
///   Lua errors propagate as WASM traps caught by jvim as plugin errors.
/// wasmi:    uses default settings.
pub fn make_engine() -> Engine {
    #[cfg(feature = "runtime-wasmtime")]
    {
        wasmtime::Engine::default()
    }
    #[cfg(feature = "runtime-wasmi")]
    {
        Engine::default()
    }
}

/// Register all unknown imports (WASI, env.*) as trap-on-call stubs so that
/// instantiation succeeds even though jvim only provides the `jvim.*` ABI.
/// Only called for wasmtime; wasmi rejects WASM EH before reaching this stage.
pub fn stub_unknown_imports<T: 'static>(linker: &mut Linker<T>, module: &Module) -> anyhow::Result<()> {
    #[cfg(feature = "runtime-wasmtime")]
    linker.define_unknown_imports_as_traps(module)?;
    #[cfg(feature = "runtime-wasmi")]
    let _ = (linker, module);
    Ok(())
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

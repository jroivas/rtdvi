//! M52: WASM plugin loading — success paths, error handling, and WASM-specific
//! pitfalls that bit us during mlua-wasm development.
//!
//! All test plugins are written inline as WAT and compiled with `wat::parse_str`.
//! No pre-built WASM artifacts are needed.
//!
//! Tests that touch `JVIM_PLUGIN_DIR` hold `ENV_LOCK` for their duration so
//! they do not race on the global env var when Cargo runs tests in parallel.

use jvim::plugin::config::PluginEntry;
use jvim::Editor;
use std::sync::{Mutex, MutexGuard};
use tempfile::TempDir;

// ── Serialisation helpers ─────────────────────────────────────────────────────

// All tests that write JVIM_PLUGIN_DIR must hold this lock.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct DirGuard {
    _dir: TempDir,
    _env: (),      // just keeps the lifetime chain alive
    _lock: MutexGuard<'static, ()>,
}

impl Drop for DirGuard {
    fn drop(&mut self) {
        std::env::remove_var("JVIM_PLUGIN_DIR");
    }
}

/// Write `bytes` as `<name>.wasm` into a fresh TempDir, set `JVIM_PLUGIN_DIR`,
/// and return a guard that cleans up when dropped.  Acquires `ENV_LOCK`.
fn plugin_dir(name: &str, bytes: &[u8]) -> DirGuard {
    // SAFETY: Mutex::lock() returns a MutexGuard<'_> tied to the Mutex lifetime.
    // We extend it to 'static because ENV_LOCK is actually 'static.
    let lock: MutexGuard<'static, ()> = unsafe {
        std::mem::transmute(ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
    };
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(format!("{name}.wasm")), bytes).unwrap();
    std::env::set_var("JVIM_PLUGIN_DIR", dir.path());
    DirGuard { _dir: dir, _env: (), _lock: lock }
}

/// Create an empty plugin dir (no .wasm inside) – used by missing-file tests.
fn empty_plugin_dir() -> DirGuard {
    let lock: MutexGuard<'static, ()> = unsafe {
        std::mem::transmute(ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
    };
    let dir = TempDir::new().unwrap();
    std::env::set_var("JVIM_PLUGIN_DIR", dir.path());
    DirGuard { _dir: dir, _env: (), _lock: lock }
}

// ── Loading helpers ───────────────────────────────────────────────────────────

/// Load one plugin by name using the std::mem::take pattern that avoids the
/// double-borrow on `editor.plugins`.
fn load(editor: &mut Editor, name: &str) -> Result<(), String> {
    let entry = PluginEntry::Simple(name.to_string());
    let mut pm = std::mem::take(&mut editor.plugins);
    let result = pm.load(editor, &entry);
    editor.plugins = pm;
    result
}

/// Return the full text of the first scratch buffer whose display name contains
/// `[Plugin: <plugin_name>]`, or None.
fn plugin_error_buf(editor: &Editor, plugin_name: &str) -> Option<String> {
    let needle = format!("[Plugin: {plugin_name}]");
    editor.buffers.values().find_map(|b| {
        if b.display_name().contains(&needle) {
            Some(
                (0..b.line_count())
                    .map(|i| b.line(i).to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        } else {
            None
        }
    })
}

// ── WAT skeletons ─────────────────────────────────────────────────────────────

/// Minimal valid plugin: imports jvim_log, has all required exports, init returns 0.
const MINIMAL_WAT: &str = r#"
(module
  (import "jvim" "jvim_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "jvim_init") (param i32 i32) (result i32)
    (call $log (i32.const 0) (i32.const 0))
    (i32.const 0))
)
"#;

// ── 1. Happy-path load ────────────────────────────────────────────────────────

#[test]
fn load_minimal_plugin() {
    let wasm = wat::parse_str(MINIMAL_WAT).unwrap();
    let _guard = plugin_dir("myplugin", &wasm);
    let mut editor = Editor::new();

    let result = load(&mut editor, "myplugin");

    assert!(result.is_ok(), "expected Ok, got {result:?}");
    assert_eq!(editor.plugins.instances.len(), 1);
    assert_eq!(editor.plugins.instances[0].name, "myplugin");
}

// ── 2. Command registration ───────────────────────────────────────────────────

#[test]
fn load_registers_command() {
    let wat = r#"
(module
  (import "jvim" "jvim_register_command" (func $reg (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "greet")
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "jvim_init") (param i32 i32) (result i32)
    (drop (call $reg (i32.const 0) (i32.const 5)))
    (i32.const 0))
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let _guard = plugin_dir("regtest", &wasm);
    let mut editor = Editor::new();

    load(&mut editor, "regtest").unwrap();

    assert_eq!(editor.plugins.instances[0].registered_commands, vec!["greet"]);
}

// ── 3. Missing file ───────────────────────────────────────────────────────────

#[test]
fn load_missing_file() {
    let _guard = empty_plugin_dir();
    let mut editor = Editor::new();

    let result = load(&mut editor, "nosuchplugin");

    assert!(result.is_err());
    assert!(editor.plugins.instances.is_empty());
}

// ── 4. Invalid WASM bytes ─────────────────────────────────────────────────────

#[test]
fn load_invalid_wasm_bytes() {
    let _guard = plugin_dir("badwasm", b"this is not valid wasm");
    let mut editor = Editor::new();

    let result = load(&mut editor, "badwasm");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("invalid WASM"), "error was: {err}");

    assert!(
        plugin_error_buf(&editor, "badwasm").is_some(),
        "expected a [Plugin: badwasm] scratch buffer"
    );
}

// ── 5. Missing alloc export ───────────────────────────────────────────────────

#[test]
fn load_missing_alloc_export() {
    // Has memory and jvim_init but no alloc/dealloc.
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "jvim_init") (param i32 i32) (result i32) (i32.const 0))
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let _guard = plugin_dir("noalloc", &wasm);
    let mut editor = Editor::new();

    let result = load(&mut editor, "noalloc");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("alloc"), "expected 'alloc' in error, got: {err}");
}

// ── 6. Missing jvim_init export ───────────────────────────────────────────────

#[test]
fn load_missing_jvim_init_export() {
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let _guard = plugin_dir("noinit", &wasm);
    let mut editor = Editor::new();

    let result = load(&mut editor, "noinit");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("jvim_init"), "expected 'jvim_init' in error, got: {err}");
}

// ── 7. jvim_init returns non-zero ─────────────────────────────────────────────

#[test]
fn init_returns_nonzero() {
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "jvim_init") (param i32 i32) (result i32)
    (i32.const 1))
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let _guard = plugin_dir("initfail", &wasm);
    let mut editor = Editor::new();

    let result = load(&mut editor, "initfail");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("non-zero"),
        "expected 'non-zero' in error, got: {err}"
    );
    // Plugin must NOT be added when init fails.
    assert!(editor.plugins.instances.is_empty());
    // Error buffer must be opened.
    assert!(plugin_error_buf(&editor, "initfail").is_some());
}

// ── 8. Trap (unreachable) during jvim_init ────────────────────────────────────

#[test]
fn init_trap_unreachable() {
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "jvim_init") (param i32 i32) (result i32)
    unreachable)
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let _guard = plugin_dir("trapinit", &wasm);
    let mut editor = Editor::new();

    let result = load(&mut editor, "trapinit");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("trapped"),
        "expected 'trapped' in error, got: {err}"
    );
    assert!(editor.plugins.instances.is_empty());
    assert!(plugin_error_buf(&editor, "trapinit").is_some());
}

// ── 9. Log lines captured in the error buffer ─────────────────────────────────
//
// A plugin can call jvim_log during jvim_init.  If init then fails (non-zero
// return), those log lines must appear in the error scratch buffer so the user
// can see what the plugin was doing before it failed.

#[test]
fn init_log_lines_in_error_buffer() {
    let wat = r#"
(module
  (import "jvim" "jvim_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "init failed, check logs")
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "jvim_init") (param i32 i32) (result i32)
    (call $log (i32.const 0) (i32.const 23))
    (i32.const 1))
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let _guard = plugin_dir("logtest", &wasm);
    let mut editor = Editor::new();

    load(&mut editor, "logtest").unwrap_err();

    let content = plugin_error_buf(&editor, "logtest")
        .expect("expected an error buffer for logtest");
    assert!(
        content.contains("init failed, check logs"),
        "log line missing from error buffer; got:\n{content}"
    );
}

// ── 10. run_command sets status ───────────────────────────────────────────────

#[test]
fn run_command_sets_status() {
    let wat = r#"
(module
  (import "jvim" "jvim_register_command" (func $reg (param i32 i32) (result i32)))
  (import "jvim" "jvim_set_status"       (func $status (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "greet")
  (data (i32.const 8) "Hello from WASM!")
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "jvim_init") (param i32 i32) (result i32)
    (drop (call $reg (i32.const 0) (i32.const 5)))
    (i32.const 0))
  (func (export "run_command")
    (param $np i32) (param $nl i32) (param $ap i32) (param $al i32) (result i32)
    (call $status (i32.const 8) (i32.const 16))
    (i32.const 0))
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let _guard = plugin_dir("cmdtest", &wasm);
    let mut editor = Editor::new();
    load(&mut editor, "cmdtest").unwrap();

    let mut pm = std::mem::take(&mut editor.plugins);
    pm.instances[0].call_run_command(&mut editor, "greet", "[]");
    editor.plugins = pm;

    assert_eq!(
        editor.status_message.as_deref(),
        Some("Hello from WASM!"),
    );
}

// ── 11. WASI random_get is provided by the host ───────────────────────────────
//
// Pitfall we hit: Rust's `HashMap::new()` seeds its RandomState via
// `getrandom()`, which on WASM emscripten calls the WASI `random_get` function.
// Without a real implementation that function was stubbed as a trap, crashing
// any Rust WASM plugin that created a HashMap during init.
//
// jvim now provides `wasi_snapshot_preview1.random_get` in abi.rs with a
// deterministic fill, so this succeeds. All other WASI imports still trap.

#[test]
fn random_get_is_provided() {
    let wat = r#"
(module
  (import "wasi_snapshot_preview1" "random_get" (func $rng (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "jvim_init") (param i32 i32) (result i32)
    ;; Fill 8 bytes of scratch space — must not trap.
    (drop (call $rng (i32.const 0) (i32.const 8)))
    (i32.const 0))
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let _guard = plugin_dir("rnginit", &wasm);
    let mut editor = Editor::new();

    let result = load(&mut editor, "rnginit");

    assert!(
        result.is_ok(),
        "random_get trapped — jvim must provide wasi_snapshot_preview1.random_get; got: {result:?}"
    );
}

// ── 12. Error buffer is named after the plugin ────────────────────────────────

#[test]
fn error_buffer_named_after_plugin() {
    let _guard = plugin_dir("namecheck", b"garbage");
    let mut editor = Editor::new();

    load(&mut editor, "namecheck").unwrap_err();

    let found = editor
        .buffers
        .values()
        .any(|b| b.display_name().contains("[Plugin: namecheck]"));
    assert!(
        found,
        "expected a buffer named '[Plugin: namecheck]'; found: {:?}",
        editor
            .buffers
            .values()
            .map(|b| b.display_name())
            .collect::<Vec<_>>()
    );
}

//! M52: WASM plugin loading — success paths, error handling, and WASM-specific
//! pitfalls that bit us during mlua-wasm development.
//!
//! All test plugins are written inline as WAT and compiled with `wat::parse_str`.
//! No pre-built WASM artifacts are needed.
//!
//! Tests that touch `RTDVI_PLUGIN_DIR` hold `ENV_LOCK` for their duration so
//! they do not race on the global env var when Cargo runs tests in parallel.

#[cfg(feature = "render-buffer")]
use rtdvi::keymap::keys::Key;
use rtdvi::plugin::config::PluginEntry;
use rtdvi::Editor;
use std::sync::{Mutex, MutexGuard};
use tempfile::TempDir;

// ── Serialisation helpers ─────────────────────────────────────────────────────

// All tests that write RTDVI_PLUGIN_DIR must hold this lock.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct DirGuard {
    _dir: TempDir,
    _env: (),      // just keeps the lifetime chain alive
    _lock: MutexGuard<'static, ()>,
}

impl Drop for DirGuard {
    fn drop(&mut self) {
        std::env::remove_var("RTDVI_PLUGIN_DIR");
    }
}

/// Write `bytes` as `<name>.wasm` into a fresh TempDir, set `RTDVI_PLUGIN_DIR`,
/// and return a guard that cleans up when dropped.  Acquires `ENV_LOCK`.
fn plugin_dir(name: &str, bytes: &[u8]) -> DirGuard {
    // SAFETY: Mutex::lock() returns a MutexGuard<'_> tied to the Mutex lifetime.
    // We extend it to 'static because ENV_LOCK is actually 'static.
    let lock: MutexGuard<'static, ()> = unsafe {
        std::mem::transmute(ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
    };
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(format!("{name}.wasm")), bytes).unwrap();
    std::env::set_var("RTDVI_PLUGIN_DIR", dir.path());
    DirGuard { _dir: dir, _env: (), _lock: lock }
}

/// Create an empty plugin dir (no .wasm inside) – used by missing-file tests.
fn empty_plugin_dir() -> DirGuard {
    let lock: MutexGuard<'static, ()> = unsafe {
        std::mem::transmute(ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
    };
    let dir = TempDir::new().unwrap();
    std::env::set_var("RTDVI_PLUGIN_DIR", dir.path());
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

/// Minimal valid plugin: imports rtdvi_log, has all required exports, init returns 0.
const MINIMAL_WAT: &str = r#"
(module
  (import "rtdvi" "rtdvi_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "rtdvi_init") (param i32 i32) (result i32)
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
  (import "rtdvi" "rtdvi_register_command" (func $reg (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "greet")
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "rtdvi_init") (param i32 i32) (result i32)
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
    // Has memory and rtdvi_init but no alloc/dealloc.
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "rtdvi_init") (param i32 i32) (result i32) (i32.const 0))
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

// ── 6. Missing rtdvi_init export ───────────────────────────────────────────────

#[test]
fn load_missing_rtdvi_init_export() {
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
    assert!(err.contains("rtdvi_init"), "expected 'rtdvi_init' in error, got: {err}");
}

// ── 7. rtdvi_init returns non-zero ─────────────────────────────────────────────

#[test]
fn init_returns_nonzero() {
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "rtdvi_init") (param i32 i32) (result i32)
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

// ── 8. Trap (unreachable) during rtdvi_init ────────────────────────────────────

#[test]
fn init_trap_unreachable() {
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "rtdvi_init") (param i32 i32) (result i32)
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
// A plugin can call rtdvi_log during rtdvi_init.  If init then fails (non-zero
// return), those log lines must appear in the error scratch buffer so the user
// can see what the plugin was doing before it failed.

#[test]
fn init_log_lines_in_error_buffer() {
    let wat = r#"
(module
  (import "rtdvi" "rtdvi_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "init failed, check logs")
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "rtdvi_init") (param i32 i32) (result i32)
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
  (import "rtdvi" "rtdvi_register_command" (func $reg (param i32 i32) (result i32)))
  (import "rtdvi" "rtdvi_set_status"       (func $status (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "greet")
  (data (i32.const 8) "Hello from WASM!")
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "rtdvi_init") (param i32 i32) (result i32)
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
// rtdvi now provides `wasi_snapshot_preview1.random_get` in abi.rs with a
// deterministic fill, so this succeeds. All other WASI imports still trap.

#[test]
fn random_get_is_provided() {
    let wat = r#"
(module
  (import "wasi_snapshot_preview1" "random_get" (func $rng (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "alloc")   (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))
  (func (export "rtdvi_init") (param i32 i32) (result i32)
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
        "random_get trapped — rtdvi must provide wasi_snapshot_preview1.random_get; got: {result:?}"
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

// ── Render buffer via the full command path (navigation regression) ───────────

/// A plugin that registers `:md` and, when invoked, builds a 2-line render
/// buffer through the render ABI. Mirrors what the markdown plugin does, so we
/// exercise the real `run_ex_line → PluginExCommand → apply_pending` path.
#[cfg(feature = "render-buffer")]
const MD_WAT: &str = r#"
(module
  (import "rtdvi" "rtdvi_register_command" (func $reg (param i32 i32) (result i32)))
  (import "rtdvi" "rtdvi_render_span" (func $span (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "rtdvi" "rtdvi_render_newline" (func $nl))
  (import "rtdvi" "rtdvi_render_open" (func $open (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "md")
  (data (i32.const 8) "hello world")
  (data (i32.const 32) "second line")
  (data (i32.const 64) "[md]")
  (func (export "alloc") (param i32) (result i32) (i32.const 256))
  (func (export "dealloc") (param i32 i32))
  (func (export "rtdvi_init") (param i32 i32) (result i32)
    (drop (call $reg (i32.const 0) (i32.const 2)))
    (i32.const 0))
  (func (export "run_command") (param i32 i32 i32 i32) (result i32)
    (drop (call $span (i32.const 8) (i32.const 11) (i32.const -1) (i32.const -1)
                      (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)))
    (call $nl)
    (drop (call $span (i32.const 32) (i32.const 11) (i32.const -1) (i32.const -1)
                      (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)))
    (call $nl)
    (drop (call $open (i32.const 64) (i32.const 4)))
    (i32.const 0))
)
"#;

#[test]
#[cfg(feature = "render-buffer")]
fn navigation_works_in_plugin_made_render_buffer() {
    let wasm = wat::parse_str(MD_WAT).unwrap();
    let _guard = plugin_dir("mdtest", &wasm);
    let mut editor = Editor::new();
    load(&mut editor, "mdtest").unwrap();

    // A source buffer to run :md from.
    let id = editor.open_scratch();
    editor.focus_single(id);

    // Drive the real command path.
    rtdvi::command::run_ex_line(&mut editor, "md");

    // The active window must now show the (non-editable) render buffer.
    let bid = editor.active_buffer_id().expect("active buffer");
    assert!(
        !editor.buffers.get(&bid).unwrap().is_editable(),
        "active window should show the render buffer after :md"
    );

    // Navigation: `$` to end of line, then `j` down.
    rtdvi::mode::handle_key(&mut editor, Key::char('$'));
    assert_eq!(editor.active_window().unwrap().cursor.col, 10, "$ moves to line end");
    rtdvi::mode::handle_key(&mut editor, Key::char('j'));
    assert_eq!(editor.active_window().unwrap().cursor.row, 1, "j moves down");
}

// ── Regression: get_line must not leak the trailing newline ───────────────────
// A line read via rtdvi_get_line and emitted into a render buffer must produce
// exactly one render line per source line (no doubling from a stray '\n').

#[cfg(feature = "render-buffer")]
const GETLINE_RENDER_WAT: &str = r#"
(module
  (import "rtdvi" "rtdvi_register_command" (func $reg (param i32 i32) (result i32)))
  (import "rtdvi" "rtdvi_active_buffer_id" (func $abuf (result i32)))
  (import "rtdvi" "rtdvi_get_line" (func $getline (param i32 i32 i32 i32) (result i32)))
  (import "rtdvi" "rtdvi_render_span" (func $span (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "rtdvi" "rtdvi_render_newline" (func $nl))
  (import "rtdvi" "rtdvi_render_open" (func $open (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "rtest")
  (data (i32.const 16) "[r]")
  (func (export "alloc") (param i32) (result i32) (i32.const 256))
  (func (export "dealloc") (param i32 i32))
  (func (export "rtdvi_init") (param i32 i32) (result i32)
    (drop (call $reg (i32.const 0) (i32.const 5))) (i32.const 0))
  (func (export "run_command") (param i32 i32 i32 i32) (result i32)
    (local $buf i32) (local $n i32)
    (local.set $buf (call $abuf))
    (local.set $n (call $getline (local.get $buf) (i32.const 0) (i32.const 1024) (i32.const 256)))
    (drop (call $span (i32.const 1024) (local.get $n) (i32.const -1) (i32.const -1)
                      (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)))
    (call $nl)
    (local.set $n (call $getline (local.get $buf) (i32.const 1) (i32.const 2048) (i32.const 256)))
    (drop (call $span (i32.const 2048) (local.get $n) (i32.const -1) (i32.const -1)
                      (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)))
    (call $nl)
    (drop (call $open (i32.const 16) (i32.const 3)))
    (i32.const 0))
)
"#;

#[test]
#[cfg(feature = "render-buffer")]
fn get_line_into_render_buffer_does_not_double_lines() {
    let wasm = wat::parse_str(GETLINE_RENDER_WAT).unwrap();
    let _guard = plugin_dir("rtest", &wasm);
    let mut editor = Editor::new();
    load(&mut editor, "rtest").unwrap();

    // Two-line source buffer (with the usual trailing newline).
    let mut f = tempfile::NamedTempFile::new().unwrap();
    use std::io::Write;
    write!(f, "abc\ndefg\n").unwrap();
    f.flush().unwrap();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);

    rtdvi::command::run_ex_line(&mut editor, "rtest");

    let bid = editor.active_buffer_id().unwrap();
    let buf = editor.buffers.get(&bid).unwrap();
    assert!(!buf.is_editable(), "active should be the render buffer");
    // Exactly two lines, each the clean source line — no phantom blank lines.
    assert_eq!(buf.line_count(), 2, "render buffer must have exactly 2 lines");
    assert_eq!(buf.line_string(0), "abc");
    assert_eq!(buf.line_string(1), "defg");
    assert_eq!(
        buf.render_content().unwrap().lines.len(),
        buf.line_count(),
        "rope line count must equal rendered line count"
    );
}

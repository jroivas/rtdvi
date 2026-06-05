//! `:version` reports the rtdvi version and the compiled-in plugin runtime.

use rtdvi::command::run_ex_line;
use rtdvi::Editor;

fn version_message(cmd: &str) -> String {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    run_ex_line(&mut editor, cmd);
    editor.status_message.clone().unwrap_or_default()
}

#[test]
fn version_reports_rtdvi_and_its_version() {
    let msg = version_message("version");
    assert!(msg.starts_with("rtdvi "), "got {msg:?}");
    assert!(
        msg.contains(env!("CARGO_PKG_VERSION")),
        "expected crate version in {msg:?}"
    );
}

#[test]
fn version_reports_wasmtime_runtime_by_default() {
    // The default feature set builds the wasmtime runtime; its resolved
    // version (from Cargo.lock via build.rs) should appear.
    #[cfg(feature = "runtime-wasmtime")]
    {
        let msg = version_message("version");
        assert!(msg.contains("wasmtime "), "expected wasmtime in {msg:?}");
        // A real semver, not the `?` placeholder.
        assert!(!msg.contains("wasmtime ?"), "version missing in {msg:?}");
    }
}

#[test]
fn version_short_aliases_work() {
    for alias in ["ver", "ve"] {
        let msg = version_message(alias);
        assert!(msg.starts_with("rtdvi "), "alias {alias:?} gave {msg:?}");
    }
}


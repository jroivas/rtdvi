//! M10: TOML config — options and keymaps applied to the editor.

use std::io::Write;

use rtdvi::config::Config;
use rtdvi::keymap::keys::Key;
use rtdvi::mode::ModeId;
use rtdvi::{config, mode, Editor};
use tempfile::NamedTempFile;

fn open_with_text(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

#[test]
fn config_options_override_defaults() {
    let toml = r#"
[options]
tab_width = 8
number = true
"#;
    let cfg: Config = toml::from_str(toml).unwrap();
    let mut editor = Editor::new();
    editor.apply_config(cfg);
    assert_eq!(editor.config.options.tab_width, 8);
    assert!(editor.config.options.number);
}

#[test]
fn config_keymap_adds_normal_binding() {
    let toml = r#"
[options]
[[keymaps]]
mode = "normal"
keys = "<Space>j"
action = "move_down"
"#;
    let cfg: Config = toml::from_str(toml).unwrap();
    let (mut editor, _f) = open_with_text("a\nb\nc\n");
    editor.apply_config(cfg);
    // Press space then j: should move cursor down to row 1.
    mode::handle_key(&mut editor, Key::char(' '));
    mode::handle_key(&mut editor, Key::char('j'));
    assert_eq!(editor.active_window().unwrap().cursor.row, 1);
}

#[test]
fn loader_returns_default_when_file_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nope.toml");
    let cfg = config::loader::load_or_default(&path).unwrap();
    assert_eq!(cfg.options.tab_width, 4);
    assert!(!cfg.options.number);
}

#[test]
fn loader_parses_actual_file() {
    // The loader now infers format from the file extension, so the
    // temp file needs a `.toml` suffix.
    let mut tmp = NamedTempFile::with_suffix(".toml").unwrap();
    tmp.write_all(
        br#"
[options]
tab_width = 2
expandtab = true
"#,
    )
    .unwrap();
    tmp.flush().unwrap();
    let cfg = config::loader::load_or_default(tmp.path()).unwrap();
    assert_eq!(cfg.options.tab_width, 2);
    assert!(cfg.options.expandtab);
}

#[test]
fn invalid_mode_in_config_is_reported_but_doesnt_panic() {
    let toml = r#"
[[keymaps]]
mode = "lunar"
keys = "x"
action = "move_left"
"#;
    let cfg: Config = toml::from_str(toml).unwrap();
    let mut editor = Editor::new();
    let _ = editor.open_scratch();
    editor.apply_config(cfg);
    // Bad mode -> status message reports it; editor still works.
    assert_eq!(editor.mode, ModeId::Normal);
    assert!(editor.status_message.is_some());
}

// ---- Broken config is visible, not silent ---------------------------------

fn write_config(content: &str, ext: &str) -> tempfile::TempPath {
    let f = tempfile::Builder::new().suffix(ext).tempfile().unwrap();
    std::fs::write(f.path(), content).unwrap();
    f.into_temp_path()
}

#[test]
fn broken_config_reports_readable_error_and_keeps_defaults() {
    // Malformed TOML — a bare word where a table/assignment is expected.
    let path = write_config("this is not valid toml = = =\n", ".toml");
    let mut editor = Editor::new();
    let ok = editor.load_config_file(&path);
    assert!(!ok, "load should fail");
    let msg = editor.status_message.clone().unwrap_or_default();
    assert!(msg.contains("failed to load config"), "msg: {msg:?}");
    assert!(msg.contains(&path.display().to_string()), "should name the file: {msg:?}");
    assert!(msg.contains("defaults"), "should mention defaults: {msg:?}");
    // Editor still usable on defaults, and the path is remembered so
    // `:config load` can retry after a fix.
    assert_eq!(editor.config.options.tab_width, 4);
    assert_eq!(editor.config_path.as_deref(), Some(path.as_ref()));
}

#[test]
fn valid_config_loads_without_error_message() {
    let path = write_config("[options]\ntab_width = 8\n", ".toml");
    let mut editor = Editor::new();
    let ok = editor.load_config_file(&path);
    assert!(ok);
    assert_eq!(editor.config.options.tab_width, 8);
    assert!(editor.status_message.is_none(), "no error on a good config");
}

//! M10: TOML config — options and keymaps applied to the editor.

use std::io::Write;

use jvim::config::Config;
use jvim::keymap::keys::Key;
use jvim::mode::ModeId;
use jvim::{config, mode, Editor};
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

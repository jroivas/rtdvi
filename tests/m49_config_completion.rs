//! `:config <Tab>` lists the sub-commands instead of filesystem
//! entries; `:config show <Tab>` lists `toml` / `json`. Path
//! arguments (`:config load <Tab>`, `:config convert toml <Tab>`)
//! still complete from the filesystem.

use std::fs;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, Editor};
use tempfile::TempDir;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}

fn fresh() -> Editor {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    editor
}

#[test]
fn config_space_tab_lists_subcommands() {
    let mut editor = fresh();
    type_keys(&mut editor, ":config ");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().expect("completion state");
    // Sub-commands, alphabetically.
    assert_eq!(
        comp.matches,
        vec!["conv", "convert", "load", "path", "show"]
    );
    // First match auto-inserted (the `:` doesn't end up in the buffer —
    // it triggers the mode switch).
    assert_eq!(editor.command_line.input, "config conv");
}

#[test]
fn config_partial_prefix_filters_subcommands() {
    let mut editor = fresh();
    type_keys(&mut editor, ":config sh");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert_eq!(comp.matches, vec!["show"]);
    assert_eq!(editor.command_line.input, "config show");
}

#[test]
fn config_show_tab_lists_formats() {
    let mut editor = fresh();
    type_keys(&mut editor, ":config show ");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert_eq!(comp.matches, vec!["json", "toml"]);
}

#[test]
fn config_convert_tab_lists_formats() {
    let mut editor = fresh();
    type_keys(&mut editor, ":config convert ");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert_eq!(comp.matches, vec!["json", "toml"]);
}

#[test]
fn config_conv_alias_also_lists_formats() {
    let mut editor = fresh();
    type_keys(&mut editor, ":config conv ");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert_eq!(comp.matches, vec!["json", "toml"]);
}

#[test]
fn config_load_tab_completes_paths_not_subcommands() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("foo.toml"), "").unwrap();
    fs::write(dir.path().join("bar.json"), "").unwrap();
    let mut editor = fresh();
    type_keys(
        &mut editor,
        &format!(":config load {}/", dir.path().display()),
    );
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    // Should be filesystem entries (bar.json, foo.toml), not "show" etc.
    let names: Vec<&str> = comp
        .matches
        .iter()
        .filter_map(|s| s.rsplit('/').next())
        .collect();
    assert!(names.contains(&"foo.toml"), "got {:?}", comp.matches);
    assert!(names.contains(&"bar.json"), "got {:?}", comp.matches);
    assert!(!names.iter().any(|s| *s == "show"), "got {:?}", comp.matches);
}

#[test]
fn config_convert_json_tab_completes_paths() {
    // After format arg, the next position is the optional path.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("zz.toml"), "").unwrap();
    let mut editor = fresh();
    type_keys(
        &mut editor,
        &format!(":config convert json {}/", dir.path().display()),
    );
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    let names: Vec<&str> = comp
        .matches
        .iter()
        .filter_map(|s| s.rsplit('/').next())
        .collect();
    assert!(names.contains(&"zz.toml"), "got {:?}", comp.matches);
}

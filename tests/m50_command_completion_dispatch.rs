//! `ExCommand::complete_arg` drives all argument-position Tab
//! completion. No command-name logic lives in the completion module.

use std::fs;

use jvim::keymap::keys::{Key, KeyCode};
use jvim::{mode, Editor};
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

// ---- Command-name completion (sanity check the existing behaviour) -----

#[test]
fn colon_tab_lists_all_command_names() {
    let mut editor = fresh();
    type_keys(&mut editor, ":");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    // We don't pin every name — just check a sample is in.
    let names: Vec<&str> = comp.matches.iter().map(|s| s.as_str()).collect();
    for must in ["q", "w", "e", "config", "split", "tab"] {
        assert!(names.contains(&must), "missing {must:?} in {:?}", names);
    }
}

#[test]
fn colon_b_tab_filters_to_b_prefix() {
    let mut editor = fresh();
    type_keys(&mut editor, ":b");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    // Every match must start with 'b'.
    assert!(!comp.matches.is_empty());
    for m in &comp.matches {
        assert!(m.starts_with('b'), "non-b match: {m}");
    }
    // Aliases whose canonical name starts with "b" collapse into canonical.
    // "buffers" alias keeps its own name because canonical "ls" ≠ b-prefix.
    let names: Vec<&str> = comp.matches.iter().map(|s| s.as_str()).collect();
    assert!(names.contains(&"bnext"));
    assert!(names.contains(&"buffers"));
    assert!(!names.contains(&"bn"), "alias 'bn' should be merged into 'bnext'");
}

// ---- Zero-arg commands no longer list files ---------------------------

#[test]
fn quit_space_tab_does_not_offer_files() {
    let mut editor = fresh();
    type_keys(&mut editor, ":q ");
    press(&mut editor, KeyCode::Tab);
    // Either no completion state at all, or an empty match list — both
    // mean "Tab is a no-op for this position".
    let empty = editor
        .command_line
        .completion
        .as_ref()
        .map_or(true, |c| c.matches.is_empty());
    assert!(empty, "got: {:?}", editor.command_line.completion);
}

#[test]
fn bnext_space_tab_does_not_offer_files() {
    let mut editor = fresh();
    type_keys(&mut editor, ":bnext ");
    press(&mut editor, KeyCode::Tab);
    let empty = editor
        .command_line
        .completion
        .as_ref()
        .map_or(true, |c| c.matches.is_empty());
    assert!(empty);
}

// ---- Path commands still complete paths -------------------------------

#[test]
fn edit_tab_completes_filesystem_paths() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("foo.txt"), "").unwrap();
    fs::write(dir.path().join("bar.md"), "").unwrap();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    let bases: Vec<&str> = comp
        .matches
        .iter()
        .filter_map(|s| s.rsplit('/').next())
        .collect();
    assert!(bases.contains(&"foo.txt"), "got {:?}", comp.matches);
    assert!(bases.contains(&"bar.md"), "got {:?}", comp.matches);
}

#[test]
fn write_tab_completes_filesystem_paths() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("out.toml"), "").unwrap();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":w {}/", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    let bases: Vec<&str> = comp
        .matches
        .iter()
        .filter_map(|s| s.rsplit('/').next())
        .collect();
    assert!(bases.contains(&"out.toml"), "got {:?}", comp.matches);
}

// ---- :tab subcommands now come from the trait, not the central switch ----

#[test]
fn tab_space_tab_lists_subcommands() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tab ");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert_eq!(comp.matches, vec!["close", "new", "next", "prev"]);
}

#[test]
fn tab_new_tab_completes_paths() {
    // The "new" sub-command takes an optional file argument; Tab
    // there should descend into the filesystem.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("hello.rs"), "").unwrap();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":tab new {}/", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    let bases: Vec<&str> = comp
        .matches
        .iter()
        .filter_map(|s| s.rsplit('/').next())
        .collect();
    assert!(bases.contains(&"hello.rs"), "got {:?}", comp.matches);
}

#[test]
fn tab_next_does_not_offer_files() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tab next ");
    press(&mut editor, KeyCode::Tab);
    let empty = editor
        .command_line
        .completion
        .as_ref()
        .map_or(true, |c| c.matches.is_empty());
    assert!(empty);
}

// ---- :config still works through the new dispatch -----------------------

#[test]
fn config_dispatch_still_lists_subcommands() {
    let mut editor = fresh();
    type_keys(&mut editor, ":config ");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert_eq!(
        comp.matches,
        vec!["conv", "convert", "load", "path", "show"]
    );
}

#[test]
fn config_show_dispatch_lists_formats() {
    let mut editor = fresh();
    type_keys(&mut editor, ":config show ");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert_eq!(comp.matches, vec!["json", "toml"]);
}

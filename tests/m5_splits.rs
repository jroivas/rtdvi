//! M5: splits, window navigation, close.

use std::io::Write;

use jvim::keymap::keys::{Key, KeyCode, KeyMods};
use jvim::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}
fn ctrl(editor: &mut Editor, c: char) {
    mode::handle_key(editor, Key::with(KeyCode::Char(c), KeyMods::CTRL));
}

fn open(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

#[test]
fn split_creates_two_windows() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    let tab = editor.tabs.first().unwrap();
    assert_eq!(tab.tree.windows().len(), 2);
}

#[test]
fn vsplit_creates_two_windows() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    let tab = editor.tabs.first().unwrap();
    assert_eq!(tab.tree.windows().len(), 2);
}

#[test]
fn close_removes_one_window() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":close");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs.first().unwrap().tree.windows().len(), 1);
}

#[test]
fn ctrl_w_w_cycles_focus() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    let first_active = editor.tabs[0].active;
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "w");
    let second_active = editor.tabs[0].active;
    assert_ne!(first_active, second_active);
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "w");
    assert_eq!(editor.tabs[0].active, first_active);
}

#[test]
fn ctrl_w_h_l_navigates_vertical_split() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    // After vsplit, new window is on the left and gets focus.
    let left = editor.tabs[0].active;
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "l");
    let right = editor.tabs[0].active;
    assert_ne!(left, right);
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "h");
    assert_eq!(editor.tabs[0].active, left);
}

#[test]
fn ctrl_w_s_keybinding_splits() {
    let (mut editor, _f) = open("hello\n");
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "s");
    assert_eq!(editor.tabs[0].tree.windows().len(), 2);
}

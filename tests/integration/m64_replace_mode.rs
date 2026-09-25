//! `R` Replace (overtype) mode: overwrite characters, extend past EOL,
//! Backspace restoration, single-step undo.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode, KeyMods};
use rtdvi::mode::ModeId;
use rtdvi::{mode, Editor};
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

fn text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}
fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

#[test]
fn r_enters_replace_mode() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "R");
    assert_eq!(editor.mode, ModeId::Replace);
}

#[test]
fn overtypes_multiple_chars() {
    let (mut editor, _f) = open("abcdef\n");
    type_keys(&mut editor, "R");
    type_keys(&mut editor, "XYZ");
    assert_eq!(text(&editor), "XYZdef\n");
    // Cursor sits just past the last overtyped cell.
    assert_eq!(cursor(&editor), (0, 3));
}

#[test]
fn extends_line_past_end() {
    let (mut editor, _f) = open("ab\n");
    type_keys(&mut editor, "R");
    type_keys(&mut editor, "XYZW"); // overtype a,b then append Z,W
    assert_eq!(text(&editor), "XYZW\n");
}

#[test]
fn esc_returns_to_normal_and_steps_left() {
    let (mut editor, _f) = open("abcd\n");
    type_keys(&mut editor, "R");
    type_keys(&mut editor, "XY");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(editor.mode, ModeId::Normal);
    assert_eq!(text(&editor), "XYcd\n");
    // Cursor was at col 2 after typing; Esc steps it left to col 1.
    assert_eq!(cursor(&editor), (0, 1));
}

#[test]
fn backspace_restores_overtyped_chars() {
    let (mut editor, _f) = open("abcdef\n");
    type_keys(&mut editor, "R");
    type_keys(&mut editor, "XYZ");
    assert_eq!(text(&editor), "XYZdef\n");
    press(&mut editor, KeyCode::Backspace);
    press(&mut editor, KeyCode::Backspace);
    // Two backspaces restore the original b, c.
    assert_eq!(text(&editor), "Xbcdef\n");
    assert_eq!(cursor(&editor), (0, 1));
}

#[test]
fn backspace_deletes_appended_chars() {
    let (mut editor, _f) = open("ab\n");
    type_keys(&mut editor, "R");
    type_keys(&mut editor, "XYZW"); // overtype ab, append ZW
    assert_eq!(text(&editor), "XYZW\n");
    press(&mut editor, KeyCode::Backspace); // removes appended W
    press(&mut editor, KeyCode::Backspace); // removes appended Z
    assert_eq!(text(&editor), "XY\n");
    press(&mut editor, KeyCode::Backspace); // restores original b
    assert_eq!(text(&editor), "Xb\n");
}

#[test]
fn enter_breaks_line() {
    let (mut editor, _f) = open("abcd\n");
    type_keys(&mut editor, "R");
    type_keys(&mut editor, "X"); // overtype a -> X, cursor at col 1
    press(&mut editor, KeyCode::Enter);
    assert_eq!(text(&editor), "X\nbcd\n");
    assert_eq!(cursor(&editor), (1, 0));
}

#[test]
fn replace_undo_is_single_step() {
    let (mut editor, _f) = open("abcdef\n");
    type_keys(&mut editor, "R");
    type_keys(&mut editor, "XYZ");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "XYZdef\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "abcdef\n");
}

#[test]
fn ctrl_c_exits_like_esc() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "R");
    type_keys(&mut editor, "X");
    ctrl(&mut editor, 'c');
    assert_eq!(editor.mode, ModeId::Normal);
    assert_eq!(text(&editor), "Xbc\n");
}

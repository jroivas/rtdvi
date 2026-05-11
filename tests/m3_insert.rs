//! M3: insert mode, edit actions, :w save. M4: undo/redo.

use std::io::Write;

use jvim::keymap::keys::{Key, KeyCode, KeyMods};
use jvim::mode::ModeId;
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

fn open(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn buffer_text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

#[test]
fn enter_insert_and_type() {
    let (mut editor, _f) = open("abc\n");
    assert_eq!(editor.mode, ModeId::Normal);
    type_keys(&mut editor, "i");
    assert_eq!(editor.mode, ModeId::Insert);
    type_keys(&mut editor, "X");
    assert_eq!(buffer_text(&editor), "Xabc\n");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(editor.mode, ModeId::Normal);
}

#[test]
fn append_after_cursor() {
    let (mut editor, _f) = open("ab\n");
    // 'l' to move to col 1, then 'a' (insert after) -> insert at col 2
    type_keys(&mut editor, "la");
    type_keys(&mut editor, "X");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "abX\n");
}

#[test]
fn open_line_below() {
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "o");
    type_keys(&mut editor, "new");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "first\nnew\nsecond\n");
}

#[test]
fn open_line_above() {
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "O");
    type_keys(&mut editor, "head");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "head\nfirst\nsecond\n");
}

#[test]
fn insert_enter_splits_line() {
    let (mut editor, _f) = open("abcd\n");
    type_keys(&mut editor, "ll"); // col 2 (on 'c')
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::Enter);
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "ab\ncd\n");
}

#[test]
fn backspace_joins_lines() {
    let (mut editor, _f) = open("ab\ncd\n");
    // Move to start of "cd" via j0
    type_keys(&mut editor, "j0");
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::Backspace);
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "abcd\n");
}

#[test]
fn colon_w_saves_to_file() {
    let (mut editor, file) = open("hello\n");
    type_keys(&mut editor, "A");
    type_keys(&mut editor, " world");
    press(&mut editor, KeyCode::Esc);
    type_keys(&mut editor, ":w");
    press(&mut editor, KeyCode::Enter);
    let on_disk = std::fs::read_to_string(file.path()).unwrap();
    assert_eq!(on_disk, "hello world\n");
}

#[test]
fn undo_redo_roundtrip() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "i");
    type_keys(&mut editor, "X");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "Xabc\n");
    type_keys(&mut editor, "u");
    assert_eq!(buffer_text(&editor), "abc\n");
    // C-r to redo
    mode::handle_key(&mut editor, Key::with(KeyCode::Char('r'), KeyMods::CTRL));
    assert_eq!(buffer_text(&editor), "Xabc\n");
}

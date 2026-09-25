//! `~` toggle-case operator: normal mode, with count, capped at end-of-line.

use std::io::Write;

use rtdvi::keymap::keys::Key;
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
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
fn tilde_lower_to_upper() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "~");
    assert_eq!(text(&editor), "Abc\n");
    // Single char: cursor advances onto the next char.
    assert_eq!(cursor(&editor), (0, 1));
}

#[test]
fn tilde_upper_to_lower() {
    let (mut editor, _f) = open("ABC\n");
    type_keys(&mut editor, "~");
    assert_eq!(text(&editor), "aBC\n");
}

#[test]
fn tilde_is_a_toggle_not_just_uppercase() {
    let (mut editor, _f) = open("aB\n");
    type_keys(&mut editor, "~"); // a -> A
    type_keys(&mut editor, "~"); // B -> b
    assert_eq!(text(&editor), "Ab\n");
}

#[test]
fn count_tilde_toggles_multiple_chars() {
    // The example from the request: 3~ on "test" -> "TESt".
    let (mut editor, _f) = open("test\n");
    type_keys(&mut editor, "3~");
    assert_eq!(text(&editor), "TESt\n");
    // Cursor lands on the char just past the last toggled one (the final t).
    assert_eq!(cursor(&editor), (0, 3));
}

#[test]
fn count_tilde_mixed_case_each_char_flips() {
    let (mut editor, _f) = open("tEsT\n");
    type_keys(&mut editor, "4~");
    assert_eq!(text(&editor), "TeSt\n");
}

#[test]
fn tilde_caps_at_end_of_line() {
    let (mut editor, _f) = open("ab\n");
    type_keys(&mut editor, "9~"); // count exceeds remaining chars
    assert_eq!(text(&editor), "AB\n");
    // Cursor parks on the last character (does not wrap to next line).
    assert_eq!(cursor(&editor), (0, 1));
}

#[test]
fn tilde_does_not_cross_into_next_line() {
    let (mut editor, _f) = open("ab\ncd\n");
    type_keys(&mut editor, "5~");
    assert_eq!(text(&editor), "AB\ncd\n");
    assert_eq!(cursor(&editor), (0, 1));
}

#[test]
fn tilde_at_interior_position() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "ll~"); // cursor on the first 'l'
    assert_eq!(text(&editor), "heLlo\n");
    assert_eq!(cursor(&editor), (0, 3));
}

#[test]
fn tilde_leaves_non_letters_unchanged() {
    let (mut editor, _f) = open("a1!b\n");
    type_keys(&mut editor, "4~");
    assert_eq!(text(&editor), "A1!B\n");
}

#[test]
fn tilde_undo_is_single_step() {
    let (mut editor, _f) = open("test\n");
    type_keys(&mut editor, "3~");
    assert_eq!(text(&editor), "TESt\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "test\n");
}

//! M2: cursor motions in normal mode.

use std::io::Write;

use rtdvi::keymap::keys::Key;
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn open_file_with(content: &str) -> (Editor, tempfile::NamedTempFile) {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content.as_bytes()).unwrap();
    file.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(file.path()).unwrap();
    editor.focus_single(id);
    (editor, file)
}

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}

fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

#[test]
fn hjkl_basic() {
    let (mut editor, _f) = open_file_with("hello\nworld\nthird\n");
    assert_eq!(cursor(&editor), (0, 0));
    type_keys(&mut editor, "l");
    assert_eq!(cursor(&editor), (0, 1));
    type_keys(&mut editor, "j");
    assert_eq!(cursor(&editor), (1, 1));
    type_keys(&mut editor, "h");
    assert_eq!(cursor(&editor), (1, 0));
    type_keys(&mut editor, "k");
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn move_right_clamped_at_end_of_line() {
    let (mut editor, _f) = open_file_with("hi\nworld\n");
    type_keys(&mut editor, "lll"); // try to go past 'i'
    assert_eq!(cursor(&editor), (0, 1)); // clamped to last col
}

#[test]
fn line_start_and_end() {
    let (mut editor, _f) = open_file_with("abcdef\n");
    type_keys(&mut editor, "$");
    assert_eq!(cursor(&editor), (0, 5));
    type_keys(&mut editor, "0");
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn gg_and_G() {
    let (mut editor, _f) = open_file_with("a\nbb\nccc\n");
    type_keys(&mut editor, "G");
    let (row, _) = cursor(&editor);
    // last *non-empty* line is row 2 ("ccc"); a trailing newline adds a virtual row.
    assert!(row >= 2);
    type_keys(&mut editor, "gg");
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn sticky_column_across_short_line() {
    let (mut editor, _f) = open_file_with("longline\nx\nlongline\n");
    type_keys(&mut editor, "$"); // col 7
    assert_eq!(cursor(&editor), (0, 7));
    type_keys(&mut editor, "j"); // line "x" only has col 0
    assert_eq!(cursor(&editor), (1, 0));
    type_keys(&mut editor, "j"); // back to longline -> sticky returns us to col 7
    assert_eq!(cursor(&editor), (2, 7));
}

#[test]
fn word_forward_skips_spaces() {
    let (mut editor, _f) = open_file_with("foo bar baz\n");
    type_keys(&mut editor, "w");
    assert_eq!(cursor(&editor), (0, 4)); // 'b' of bar
    type_keys(&mut editor, "w");
    assert_eq!(cursor(&editor), (0, 8)); // 'b' of baz
}

#[test]
fn word_backward() {
    let (mut editor, _f) = open_file_with("foo bar baz\n");
    type_keys(&mut editor, "$"); // on 'z'
    type_keys(&mut editor, "b");
    assert_eq!(cursor(&editor), (0, 8)); // start of "baz"
    type_keys(&mut editor, "b");
    assert_eq!(cursor(&editor), (0, 4)); // start of "bar"
}

#[test]
fn word_end() {
    let (mut editor, _f) = open_file_with("foo bar\n");
    type_keys(&mut editor, "e");
    assert_eq!(cursor(&editor), (0, 2)); // 'o' of foo
    type_keys(&mut editor, "e");
    assert_eq!(cursor(&editor), (0, 6)); // 'r' of bar
}

//! `J` — join lines, vim-style: single space at the seam, leading
//! whitespace of joined lines removed, counts, `)` and trailing-space rules.

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
fn j_joins_two_lines_with_a_space() {
    let (mut editor, _f) = open("hello\nworld\n");
    type_keys(&mut editor, "J");
    assert_eq!(text(&editor), "hello world\n");
    // Cursor lands on the inserted space (col 5).
    assert_eq!(cursor(&editor), (0, 5));
}

#[test]
fn j_strips_leading_whitespace_of_joined_line() {
    let (mut editor, _f) = open("foo\n    bar\n");
    type_keys(&mut editor, "J");
    assert_eq!(text(&editor), "foo bar\n");
}

#[test]
fn count_j_joins_n_lines() {
    // 3J joins three lines into one.
    let (mut editor, _f) = open("one\ntwo\nthree\nfour\n");
    type_keys(&mut editor, "3J");
    assert_eq!(text(&editor), "one two three\nfour\n");
    // Cursor on the seam before the last joined word ("three").
    assert_eq!(cursor(&editor), (0, 7));
}

#[test]
fn two_j_joins_two_lines_like_plain_j() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "2J");
    assert_eq!(text(&editor), "a b\nc\n");
}

#[test]
fn j_no_extra_space_when_line_ends_in_whitespace() {
    let (mut editor, _f) = open("foo \nbar\n");
    type_keys(&mut editor, "J");
    assert_eq!(text(&editor), "foo bar\n");
}

#[test]
fn j_no_space_before_close_paren() {
    let (mut editor, _f) = open("foo(\n)\n");
    type_keys(&mut editor, "J");
    assert_eq!(text(&editor), "foo()\n");
}

#[test]
fn j_on_last_line_is_a_noop() {
    let (mut editor, _f) = open("only\n");
    type_keys(&mut editor, "J");
    assert_eq!(text(&editor), "only\n");
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn j_joins_at_cursor_row_not_top() {
    let (mut editor, _f) = open("keep\nhello\nworld\n");
    type_keys(&mut editor, "j"); // move to row 1 ("hello")
    type_keys(&mut editor, "J");
    assert_eq!(text(&editor), "keep\nhello world\n");
    assert_eq!(cursor(&editor), (1, 5));
}

#[test]
fn j_count_clamps_at_end_of_buffer() {
    // 9J with only two lines below joins what's available without panicking.
    let (mut editor, _f) = open("a\nb\n");
    type_keys(&mut editor, "9J");
    assert_eq!(text(&editor), "a b\n");
}

#[test]
fn j_is_single_undo_step() {
    let (mut editor, _f) = open("one\ntwo\nthree\n");
    type_keys(&mut editor, "3J");
    assert_eq!(text(&editor), "one two three\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "one\ntwo\nthree\n");
}

#[test]
fn j_joins_empty_next_line_without_trailing_space() {
    let (mut editor, _f) = open("foo\n\nbar\n");
    type_keys(&mut editor, "3J");
    assert_eq!(text(&editor), "foo bar\n");
}

//! Yank operators (yy/yj/yk/yw/etc.) with counts.

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

// ---- yy / Y ----------------------------------------------------------------

#[test]
fn yy_yanks_current_line_linewise() {
    let (mut editor, _f) = open("hello\nworld\n");
    type_keys(&mut editor, "yy");
    assert_eq!(editor.unnamed_register.text, "hello\n");
    assert!(editor.unnamed_register.linewise);
    // Buffer unchanged, cursor stays put.
    assert_eq!(text(&editor), "hello\nworld\n");
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn capital_y_aliases_yy() {
    let (mut editor, _f) = open("hello\nworld\n");
    type_keys(&mut editor, "Y");
    assert_eq!(editor.unnamed_register.text, "hello\n");
    assert!(editor.unnamed_register.linewise);
}

#[test]
fn yy_p_pastes_line_below() {
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "yyp");
    assert_eq!(text(&editor), "first\nfirst\nsecond\n");
}

#[test]
fn count_yy_yanks_multiple_lines() {
    let (mut editor, _f) = open("a\nb\nc\nd\n");
    type_keys(&mut editor, "3yy");
    assert_eq!(editor.unnamed_register.text, "a\nb\nc\n");
    assert!(editor.unnamed_register.linewise);
    assert_eq!(text(&editor), "a\nb\nc\nd\n");
}

// ---- yj / yk ---------------------------------------------------------------

#[test]
fn yj_yanks_current_and_next_line() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "yj");
    assert_eq!(editor.unnamed_register.text, "a\nb\n");
    assert!(editor.unnamed_register.linewise);
}

#[test]
fn count_4yj_yanks_5_lines() {
    let (mut editor, _f) = open("a\nb\nc\nd\ne\nf\n");
    type_keys(&mut editor, "4yj");
    assert_eq!(editor.unnamed_register.text, "a\nb\nc\nd\ne\n");
}

#[test]
fn yk_yanks_current_and_previous_line() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "jyk"); // on row 1, yank rows 0-1
    assert_eq!(editor.unnamed_register.text, "a\nb\n");
}

#[test]
fn count_2yk_yanks_three_lines_up() {
    let (mut editor, _f) = open("a\nb\nc\nd\n");
    type_keys(&mut editor, "3j"); // row 3
    type_keys(&mut editor, "2yk"); // rows 1..=3
    assert_eq!(editor.unnamed_register.text, "b\nc\nd\n");
}

// ---- yw / yb / ye ----------------------------------------------------------

#[test]
fn yw_yanks_one_word_forward() {
    let (mut editor, _f) = open("foo bar baz\n");
    type_keys(&mut editor, "yw");
    assert_eq!(editor.unnamed_register.text, "foo ");
    assert!(!editor.unnamed_register.linewise);
    // Cursor stays at the start (yank doesn't move).
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn count_yw_yanks_multiple_words() {
    let (mut editor, _f) = open("foo bar baz qux\n");
    type_keys(&mut editor, "2yw");
    assert_eq!(editor.unnamed_register.text, "foo bar ");
}

#[test]
fn y3w_count_after_operator() {
    let (mut editor, _f) = open("foo bar baz qux\n");
    type_keys(&mut editor, "y3w");
    assert_eq!(editor.unnamed_register.text, "foo bar baz ");
}

#[test]
fn pre_and_post_count_multiply_on_yank() {
    let (mut editor, _f) = open("a b c d e f g h i\n");
    type_keys(&mut editor, "2y3w"); // 6 words
    assert_eq!(editor.unnamed_register.text, "a b c d e f ");
}

#[test]
fn yb_yanks_word_backward() {
    let (mut editor, _f) = open("foo bar baz\n");
    type_keys(&mut editor, "$yb"); // cursor at 'z', yank back to 'b' of baz
    // Cursor stays at end ($) after yank.
    assert!(editor.unnamed_register.text == "ba" || editor.unnamed_register.text == "baz");
}

#[test]
fn ye_yanks_to_word_end_inclusive() {
    let (mut editor, _f) = open("foobar baz\n");
    type_keys(&mut editor, "ye");
    assert_eq!(editor.unnamed_register.text, "foobar");
}

// ---- y$ / y0 / yG / ygg ----------------------------------------------------

#[test]
fn y_dollar_yanks_to_end_of_line() {
    let (mut editor, _f) = open("hello world\n");
    type_keys(&mut editor, "lly$");
    assert_eq!(editor.unnamed_register.text, "llo world");
}

#[test]
fn y_zero_yanks_to_start_of_line() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "llly0");
    assert_eq!(editor.unnamed_register.text, "hel");
}

#[test]
fn ygg_yanks_to_start_of_buffer() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "jjygg");
    assert_eq!(editor.unnamed_register.text, "a\nb\nc\n");
    assert!(editor.unnamed_register.linewise);
}

#[test]
fn capital_y_g_yanks_to_end_of_buffer() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "yG");
    assert_eq!(editor.unnamed_register.text, "a\nb\nc\n");
    assert!(editor.unnamed_register.linewise);
}

// ---- Behavior preserved ----------------------------------------------------

#[test]
fn yank_does_not_move_cursor() {
    let (mut editor, _f) = open("hello world\n");
    type_keys(&mut editor, "lll"); // col 3
    type_keys(&mut editor, "yw");
    assert_eq!(cursor(&editor), (0, 3));
}

#[test]
fn yank_does_not_dirty_buffer() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "yy");
    let id = editor.active_buffer_id().unwrap();
    assert!(!editor.buffers.get(&id).unwrap().is_dirty());
}

#[test]
fn yy_yanks_into_register_for_paste() {
    let (mut editor, _f) = open("alpha\nbeta\n");
    type_keys(&mut editor, "yy");
    type_keys(&mut editor, "jp"); // move to row 1, paste below
    assert_eq!(text(&editor), "alpha\nbeta\nalpha\n");
}

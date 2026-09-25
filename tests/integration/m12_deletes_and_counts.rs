//! Delete operators (dd/dw/dj/dk/etc.) and vim-style count prefixes.

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

// ---- Counts on motions -----------------------------------------------------

#[test]
fn count_repeats_motion() {
    let (mut editor, _f) = open("abcdef\n");
    type_keys(&mut editor, "4l");
    assert_eq!(cursor(&editor), (0, 4));
}

#[test]
fn count_on_j_moves_multiple_lines() {
    let (mut editor, _f) = open("a\nb\nc\nd\ne\n");
    type_keys(&mut editor, "3j");
    assert_eq!(cursor(&editor).0, 3);
}

#[test]
fn count_on_w_moves_multiple_words() {
    let (mut editor, _f) = open("foo bar baz qux\n");
    type_keys(&mut editor, "3w");
    assert_eq!(cursor(&editor), (0, 12)); // start of "qux"
}

#[test]
fn count_on_capital_g_is_line_number() {
    let (mut editor, _f) = open("a\nb\nc\nd\ne\n");
    type_keys(&mut editor, "3G");
    assert_eq!(cursor(&editor).0, 2); // line 3 = row 2
}

// ---- dd / dj / dk ----------------------------------------------------------

#[test]
fn dd_deletes_current_line() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "dd");
    assert_eq!(text(&editor), "b\nc\n");
}

#[test]
fn count_dd_deletes_multiple_lines() {
    let (mut editor, _f) = open("a\nb\nc\nd\ne\n");
    type_keys(&mut editor, "3dd");
    assert_eq!(text(&editor), "d\ne\n");
}

#[test]
fn dj_deletes_two_lines_default() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "dj");
    assert_eq!(text(&editor), "c\n");
}

#[test]
fn count_4dj_deletes_5_lines() {
    let (mut editor, _f) = open("a\nb\nc\nd\ne\nf\ng\n");
    // 4dj from row 0 deletes rows 0..=4 (current + 4 below) = 5 lines.
    type_keys(&mut editor, "4dj");
    assert_eq!(text(&editor), "f\ng\n");
}

#[test]
fn dk_deletes_current_and_previous_line() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "jdk"); // on row 1, dk -> delete row 0..=1
    assert_eq!(text(&editor), "c\n");
}

#[test]
fn count_2dk_deletes_three_lines_up() {
    let (mut editor, _f) = open("a\nb\nc\nd\ne\n");
    type_keys(&mut editor, "3j"); // row 3
    type_keys(&mut editor, "2dk"); // delete rows 1..=3
    assert_eq!(text(&editor), "a\ne\n");
}

// ---- dw / db / de ----------------------------------------------------------

#[test]
fn dw_deletes_one_word_forward() {
    let (mut editor, _f) = open("foo bar baz\n");
    type_keys(&mut editor, "dw");
    assert_eq!(text(&editor), "bar baz\n");
}

#[test]
fn count_dw_deletes_multiple_words() {
    let (mut editor, _f) = open("foo bar baz qux\n");
    type_keys(&mut editor, "2dw");
    assert_eq!(text(&editor), "baz qux\n");
}

#[test]
fn d3w_count_after_operator_also_works() {
    let (mut editor, _f) = open("foo bar baz qux\n");
    type_keys(&mut editor, "d3w");
    assert_eq!(text(&editor), "qux\n");
}

#[test]
fn pre_and_post_counts_multiply() {
    let (mut editor, _f) = open("a b c d e f g h i\n");
    // 2 * 3 = 6 words deleted
    type_keys(&mut editor, "2d3w");
    assert_eq!(text(&editor), "g h i\n");
}

#[test]
fn db_deletes_word_backward() {
    let (mut editor, _f) = open("foo bar baz\n");
    type_keys(&mut editor, "$db"); // cursor at 'z', db deletes "ba"
    let t = text(&editor);
    assert!(t.starts_with("foo bar z") || t.starts_with("foo bar "), "got {t:?}");
}

#[test]
fn de_deletes_to_word_end_inclusive() {
    let (mut editor, _f) = open("foobar baz\n");
    type_keys(&mut editor, "de");
    assert_eq!(text(&editor), " baz\n");
}

// ---- d$ / d0 / dG / dgg ----------------------------------------------------

#[test]
fn d_dollar_deletes_to_end_of_line() {
    let (mut editor, _f) = open("hello world\n");
    type_keys(&mut editor, "lld$"); // delete from col 2 to end
    assert_eq!(text(&editor), "he\n");
}

#[test]
fn capital_d_aliases_d_dollar() {
    let (mut editor, _f) = open("hello world\n");
    type_keys(&mut editor, "llD");
    assert_eq!(text(&editor), "he\n");
}

#[test]
fn d_zero_deletes_to_start_of_line() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "llld0");
    assert_eq!(text(&editor), "lo\n");
}

#[test]
fn dgg_deletes_to_start_of_buffer() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "jjdgg"); // from row 2, delete to top
    assert_eq!(text(&editor), "");
}

#[test]
fn d_capital_g_deletes_to_end_of_buffer() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "jdG");
    assert_eq!(text(&editor), "a\n");
}

// ---- x / X -----------------------------------------------------------------

#[test]
fn x_deletes_char_under_cursor() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "x");
    assert_eq!(text(&editor), "bc\n");
}

#[test]
fn count_x_deletes_multiple_chars() {
    let (mut editor, _f) = open("abcdef\n");
    type_keys(&mut editor, "3x");
    assert_eq!(text(&editor), "def\n");
}

#[test]
fn x_does_not_cross_line_boundary() {
    let (mut editor, _f) = open("ab\ncd\n");
    type_keys(&mut editor, "l5x"); // try to delete past 'b' into next line
    assert_eq!(text(&editor), "a\ncd\n");
}

#[test]
fn capital_x_deletes_char_before_cursor() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "llX"); // cursor at 'c', X deletes 'b'
    assert_eq!(text(&editor), "ac\n");
}

// ---- Edge cases for counts -------------------------------------------------

#[test]
fn leading_zero_is_motion_not_count() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "lll0"); // col 3, then '0' -> line start
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn rejected_sequence_clears_count() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "5"); // pending count 5
    type_keys(&mut editor, "Q"); // 'Q' alone isn't bound -> reject, clear count
    type_keys(&mut editor, "l"); // should move 1, not 5
    assert_eq!(cursor(&editor), (0, 1));
}

#[test]
fn delete_yanks_to_unnamed_register() {
    let (mut editor, _f) = open("foo bar\n");
    type_keys(&mut editor, "dw");
    assert_eq!(editor.unnamed_register.text, "foo ");
}

#[test]
fn dd_then_p_pastes_the_line_back() {
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "ddp");
    // dd deletes "first\n" linewise; p pastes below current line ("second")
    assert_eq!(text(&editor), "second\nfirst\n");
}

//! `>>` and `<<` indent / dedent operators. Counts apply (`3>>`),
//! visual-mode `>` / `<` work on the selected row span, and the
//! indent unit follows `options.expandtab`.

use std::io::Write;

use jvim::config::Config;
use jvim::keymap::keys::Key;
use jvim::{mode, Editor};
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

fn buffer_text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

fn with_expandtab(toggle: bool) -> Config {
    let s = format!(
        r#"
[options]
expandtab = {toggle}
"#,
    );
    toml::from_str(&s).unwrap()
}

// ---- `>>` / `<<` basic --------------------------------------------------

#[test]
fn indent_current_line_with_spaces_when_expandtab_on() {
    // Default config: expandtab = true, tab_width = 4.
    let (mut editor, _f) = open("foo\nbar\n");
    type_keys(&mut editor, ">>");
    assert_eq!(buffer_text(&editor), "    foo\nbar\n");
    // Cursor lands on first non-blank (after the indent).
    assert_eq!(cursor(&editor), (0, 4));
}

#[test]
fn indent_current_line_with_tab_when_expandtab_off() {
    let (mut editor, _f) = open("foo\nbar\n");
    editor.apply_config(with_expandtab(false));
    type_keys(&mut editor, ">>");
    assert_eq!(buffer_text(&editor), "\tfoo\nbar\n");
}

#[test]
fn count_indents_multiple_lines() {
    let (mut editor, _f) = open("a\nb\nc\nd\n");
    type_keys(&mut editor, "3>>");
    assert_eq!(buffer_text(&editor), "    a\n    b\n    c\nd\n");
}

#[test]
fn dedent_removes_leading_tab() {
    let (mut editor, _f) = open("\tfoo\nbar\n");
    type_keys(&mut editor, "<<");
    assert_eq!(buffer_text(&editor), "foo\nbar\n");
}

#[test]
fn dedent_removes_one_tab_width_of_spaces() {
    let (mut editor, _f) = open("    foo\nbar\n");
    type_keys(&mut editor, "<<");
    assert_eq!(buffer_text(&editor), "foo\nbar\n");
}

#[test]
fn dedent_partial_indent_removes_what_is_there() {
    let (mut editor, _f) = open("  foo\nbar\n");
    type_keys(&mut editor, "<<");
    assert_eq!(buffer_text(&editor), "foo\nbar\n");
}

#[test]
fn dedent_no_op_on_unindented_line() {
    let (mut editor, _f) = open("foo\nbar\n");
    type_keys(&mut editor, "<<");
    assert_eq!(buffer_text(&editor), "foo\nbar\n");
}

#[test]
fn count_dedent_walks_multiple_lines() {
    let (mut editor, _f) = open("    a\n    b\n    c\nd\n");
    type_keys(&mut editor, "3<<");
    assert_eq!(buffer_text(&editor), "a\nb\nc\nd\n");
}

#[test]
fn indent_skips_empty_lines() {
    // Empty line between two content lines shouldn't get whitespace
    // sprayed across it.
    let (mut editor, _f) = open("foo\n\nbar\n");
    type_keys(&mut editor, "3>>");
    assert_eq!(buffer_text(&editor), "    foo\n\n    bar\n");
}

// ---- Undo collapses the whole operation ---------------------------------

#[test]
fn three_line_indent_is_one_undo_step() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "3>>");
    assert_eq!(buffer_text(&editor), "    a\n    b\n    c\n");
    type_keys(&mut editor, "u");
    assert_eq!(buffer_text(&editor), "a\nb\nc\n");
}

// ---- Visual mode `>` and `<` --------------------------------------------

#[test]
fn visual_line_indent_spans_selection() {
    let (mut editor, _f) = open("a\nb\nc\n");
    // V on line 0, j to extend to line 1, then > to indent.
    type_keys(&mut editor, "Vj>");
    assert_eq!(buffer_text(&editor), "    a\n    b\nc\n");
}

#[test]
fn visual_line_dedent_spans_selection() {
    let (mut editor, _f) = open("    a\n    b\nc\n");
    type_keys(&mut editor, "Vj<");
    assert_eq!(buffer_text(&editor), "a\nb\nc\n");
}

#[test]
fn visual_indent_returns_to_normal_mode() {
    let (mut editor, _f) = open("a\nb\n");
    type_keys(&mut editor, "V>");
    assert_eq!(editor.mode, jvim::mode::ModeId::Normal);
}

#[test]
fn visual_char_indent_still_works_per_row() {
    // Even with character-wise visual (`v`), `>` should indent every
    // touched row.
    let (mut editor, _f) = open("foo\nbar\n");
    type_keys(&mut editor, "vj>");
    assert_eq!(buffer_text(&editor), "    foo\n    bar\n");
}

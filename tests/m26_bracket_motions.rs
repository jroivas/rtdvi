//! `%` match-bracket and `[[` / `]]` section motions.

use std::io::Write;

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

fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

// ---- % match bracket -------------------------------------------------------

#[test]
fn percent_jumps_from_open_to_close_paren() {
    let (mut editor, _f) = open("(abc)\n");
    // cursor at (0,0) on '('
    type_keys(&mut editor, "%");
    assert_eq!(cursor(&editor), (0, 4)); // ')'
}

#[test]
fn percent_jumps_from_close_to_open_paren() {
    let (mut editor, _f) = open("(abc)\n");
    type_keys(&mut editor, "$"); // on ')'
    type_keys(&mut editor, "%");
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn percent_handles_nested_brackets() {
    let (mut editor, _f) = open("(a(b)c)\n");
    type_keys(&mut editor, "%");
    // Outer '(' at col 0 matches outer ')' at col 6.
    assert_eq!(cursor(&editor), (0, 6));
}

#[test]
fn percent_works_for_curly_braces_across_lines() {
    let (mut editor, _f) = open("fn main() {\n    let x = 1;\n}\n");
    // Move to the '{' on line 0 (col 10).
    type_keys(&mut editor, "$"); // col 10 = '{'
    assert_eq!(cursor(&editor), (0, 10));
    type_keys(&mut editor, "%");
    // Should jump to the '}' on row 2 col 0.
    assert_eq!(cursor(&editor), (2, 0));
    type_keys(&mut editor, "%");
    // And back.
    assert_eq!(cursor(&editor), (0, 10));
}

#[test]
fn percent_works_for_square_brackets() {
    let (mut editor, _f) = open("[a,b,[c,d],e]\n");
    type_keys(&mut editor, "%");
    assert_eq!(cursor(&editor), (0, 12));
}

#[test]
fn percent_on_non_bracket_scans_forward_on_line() {
    let (mut editor, _f) = open("hello (world)\n");
    // cursor at (0,0) on 'h'. Scan forward should find '(' at col 6.
    type_keys(&mut editor, "%");
    assert_eq!(cursor(&editor), (0, 12)); // ')'
}

#[test]
fn percent_with_unmatched_bracket_is_noop() {
    let (mut editor, _f) = open("(hello\n");
    let before = cursor(&editor);
    type_keys(&mut editor, "%");
    assert_eq!(cursor(&editor), before);
}

#[test]
fn percent_on_line_without_any_bracket_is_noop() {
    let (mut editor, _f) = open("hello world\n");
    let before = cursor(&editor);
    type_keys(&mut editor, "%");
    assert_eq!(cursor(&editor), before);
}

// ---- [[ and ]] section motions --------------------------------------------

fn c_source() -> String {
    String::from(
        "// preamble\n\
         int main() {\n\
             return 0;\n\
         }\n\
         \n\
         int helper(int x) {\n\
             return x * 2;\n\
         }\n\
         \n\
         {\n\
             // anonymous block\n\
         }\n",
    )
}

#[test]
fn double_close_bracket_jumps_to_next_section() {
    let (mut editor, _f) = open(&c_source());
    type_keys(&mut editor, "]]");
    // First `{` at column 0 is on row 1 (`int main() {` — but '{' isn't at col 0).
    // Actually `int main() {` doesn't start with `{`. The next *section* (line
    // starting with `{`) is the anonymous block on row 9.
    assert_eq!(cursor(&editor).0, 9);
}

#[test]
fn double_close_with_count_jumps_multiple_sections() {
    // Build a buffer with three `{` lines.
    let content = "before\n{\n  a\n}\n{\n  b\n}\n{\n  c\n}\n";
    let (mut editor, _f) = open(content);
    type_keys(&mut editor, "2]]");
    // From row 0, second `{` at col 0 line is row 4.
    assert_eq!(cursor(&editor).0, 4);
}

#[test]
fn double_open_bracket_jumps_to_previous_section() {
    let content = "{\n  a\n}\n{\n  b\n}\n{\n  c\n}\n";
    let (mut editor, _f) = open(content);
    // Jump to the last `{` line.
    type_keys(&mut editor, "G"); // last line
    type_keys(&mut editor, "[[");
    // The previous `{` line at col 0 is row 6.
    assert_eq!(cursor(&editor).0, 6);
}

#[test]
fn double_close_bracket_at_eof_stops_at_last_line() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "]]");
    // No `{` lines; should land on the last line.
    let last = editor.buffers.get(&editor.active_buffer_id().unwrap()).unwrap().line_count().saturating_sub(1);
    assert_eq!(cursor(&editor).0, last);
}

#[test]
fn double_open_bracket_at_bof_stops_at_first_line() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "G"); // last line
    type_keys(&mut editor, "[[");
    assert_eq!(cursor(&editor).0, 0);
}

#[test]
fn percent_in_visual_mode_extends_selection() {
    let (mut editor, _f) = open("(abc)\n");
    type_keys(&mut editor, "v%"); // start visual, jump to ')'
    // Yank to capture the selection so we can verify range.
    type_keys(&mut editor, "y");
    assert_eq!(editor.unnamed_register.text, "(abc)");
}

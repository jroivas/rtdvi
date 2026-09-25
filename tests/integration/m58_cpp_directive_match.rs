//! `%` on C preprocessor conditionals rotates #if/#elif/#else/#endif,
//! matchit-style, skipping nested conditional blocks.

use std::io::Write;

use rtdvi::keymap::keys::Key;
use rtdvi::{mode, Editor};
use tempfile::Builder;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}

/// Open `content` as a `.c` file (so the filetype is `c`).
fn open_c(content: &str) -> (Editor, tempfile::NamedTempFile) {
    let mut f = Builder::new().suffix(".c").tempfile().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn row(editor: &Editor) -> usize {
    editor.active_window().unwrap().cursor.row
}
fn goto_row(editor: &mut Editor, target: usize) {
    let id = editor.active_buffer_id().unwrap();
    let w = editor
        .windows
        .get_mut(&editor.tabs[editor.active_tab].active)
        .unwrap();
    w.cursor.row = target;
    w.cursor.col = 0;
    let _ = id;
}

// The example from the request (rows are 0-indexed here; the prompt's
// line numbers are 1-based).
const SRC: &str = "\
#include <stdio.h>

#if MUL2
int test3(int x)
{
    return x * 2;
}
#elif MUL3
int test3(int x)
{
#if TWICE
    return x * 3 * 2;
#else
    return x * 3;
#endif
}
#else
int test3(int x)
{
    return x * 4;
}
#endif
";

#[test]
fn outer_group_rotates_skipping_nested() {
    // Rows: 2 #if MUL2, 7 #elif MUL3, 16 #else, 21 #endif.
    let (mut editor, _f) = open_c(SRC);

    goto_row(&mut editor, 2); // #if MUL2
    type_keys(&mut editor, "%");
    assert_eq!(row(&editor), 7, "#if -> #elif");

    type_keys(&mut editor, "%");
    // From #elif MUL3 we must reach the outer #else (row 16), NOT the nested
    // #else at row 12.
    assert_eq!(row(&editor), 16, "#elif -> outer #else (skips nested)");

    type_keys(&mut editor, "%");
    assert_eq!(row(&editor), 21, "#else -> #endif");

    type_keys(&mut editor, "%");
    assert_eq!(row(&editor), 2, "#endif -> opening #if (wraps)");
}

#[test]
fn nested_group_rotates_independently() {
    // Nested block: 10 #if TWICE, 12 #else, 14 #endif.
    let (mut editor, _f) = open_c(SRC);

    goto_row(&mut editor, 10); // #if TWICE
    type_keys(&mut editor, "%");
    assert_eq!(row(&editor), 12, "nested #if -> nested #else");

    type_keys(&mut editor, "%");
    assert_eq!(row(&editor), 14, "nested #else -> nested #endif");

    type_keys(&mut editor, "%");
    assert_eq!(row(&editor), 10, "nested #endif -> nested #if (wraps)");
}

#[test]
fn cursor_lands_on_hash_column() {
    let (mut editor, _f) = open_c("#if A\n    #endif\n");
    goto_row(&mut editor, 0);
    type_keys(&mut editor, "%");
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 1);
    assert_eq!(w.cursor.col, 4, "cursor lands on the '#', not column 0");
}

#[test]
fn plain_if_else_endif_cycle() {
    let (mut editor, _f) = open_c("#if X\na\n#else\nb\n#endif\n");
    type_keys(&mut editor, "%"); // on #if (row 0)
    assert_eq!(row(&editor), 2);
    type_keys(&mut editor, "%");
    assert_eq!(row(&editor), 4);
    type_keys(&mut editor, "%");
    assert_eq!(row(&editor), 0);
}

#[test]
fn brackets_still_match_on_non_directive_lines() {
    // `%` must keep its normal bracket behaviour off directive lines.
    let (mut editor, _f) = open_c("int f(int a)\n{\n}\n");
    // Cursor on the '(' of row 0.
    let w = editor
        .windows
        .get_mut(&editor.tabs[editor.active_tab].active)
        .unwrap();
    w.cursor.col = 5; // the '('
    type_keys(&mut editor, "%");
    let w = editor.active_window().unwrap();
    assert_eq!((w.cursor.row, w.cursor.col), (0, 11), "jumps to matching ')'");
}

#[test]
fn directive_handling_is_c_only() {
    // In a non-C buffer, a line starting with `#` is not treated as a
    // preprocessor directive (e.g. it's a comment/heading), so `%` does not
    // rotate — it falls back to bracket matching and stays put here.
    let mut f = Builder::new().suffix(".txt").tempfile().unwrap();
    f.write_all(b"#if A\n#endif\n").unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    type_keys(&mut editor, "%");
    assert_eq!(editor.active_window().unwrap().cursor.row, 0);
}

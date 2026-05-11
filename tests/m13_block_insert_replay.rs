//! Visual-block `I` and `A`: typed text is replayed into every selected row.

use std::io::Write;

use jvim::keymap::keys::{Key, KeyCode, KeyMods};
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

#[test]
fn insert_prefix_into_three_rows() {
    let (mut editor, _f) = open("alpha\nbeta\ngamma\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jj"); // rows 0-2
    type_keys(&mut editor, "I// ");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "// alpha\n// beta\n// gamma\n");
}

#[test]
fn insert_multiple_chars_into_inner_column() {
    let (mut editor, _f) = open("abcdef\nghijkl\nmnopqr\n");
    type_keys(&mut editor, "ll"); // col 2
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jj"); // rows 0-2, col 2
    type_keys(&mut editor, "IXY");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "abXYcdef\nghXYijkl\nmnXYopqr\n");
}

#[test]
fn append_into_three_rows() {
    let (mut editor, _f) = open("abc\ndef\nghi\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jjl"); // rows 0-2, cols 0-1
    type_keys(&mut editor, "AZ");
    press(&mut editor, KeyCode::Esc);
    // Right edge col = 1; insert at col 2 on each row.
    assert_eq!(text(&editor), "abZc\ndeZf\nghZi\n");
}

#[test]
fn append_pads_short_lines_with_spaces() {
    // Middle line is shorter than the right edge of the rectangle.
    // Right column of the rect = col 4 here (motion clamps `l` at end of
    // the shortest line, "gamma" → cap at col 4). Insert at col 5.
    let (mut editor, _f) = open("alphabet\nhi\ngamma\n");
    type_keys(&mut editor, "ll"); // col 2
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jjll"); // bottom row reaches col 4
    type_keys(&mut editor, "AX");
    press(&mut editor, KeyCode::Esc);
    // Row 0 "alphabet" col 5 -> "alphaXbet"; row 1 "hi" pad 3 -> "hi   X";
    // row 2 "gamma" col 5 = end-of-line -> "gammaX".
    assert_eq!(text(&editor), "alphaXbet\nhi   X\ngammaX\n");
}

#[test]
fn insert_skips_lines_shorter_than_left_edge() {
    // For `I` (insert at left), lines that don't reach the left col are skipped.
    // We can't normally select a block that includes a line shorter than the
    // left col (the rectangle's left would be past line end), but `I` is
    // robust to that case anyway.
    let (mut editor, _f) = open("longer line\nhi\nlonger again\n");
    type_keys(&mut editor, "llll"); // col 4
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jj");
    type_keys(&mut editor, "IZ");
    press(&mut editor, KeyCode::Esc);
    // Row 0 col 4 -> "longZer line"; row 1 width 2 < 4 -> skipped; row 2 "longZer again".
    assert_eq!(text(&editor), "longZer line\nhi\nlongZer again\n");
}

#[test]
fn esc_with_no_typing_leaves_buffer_unchanged() {
    let (mut editor, _f) = open("abc\ndef\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "I");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "abc\ndef\n");
}

#[test]
fn block_insert_count_x_compatible() {
    // <C-v>2j I "hello " <Esc> over 3 rows.
    let (mut editor, _f) = open("a\nb\nc\nd\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "2j"); // count works in visual modes too via motions
    type_keys(&mut editor, "I> ");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "> a\n> b\n> c\nd\n");
}

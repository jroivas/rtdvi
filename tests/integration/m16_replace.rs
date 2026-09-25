//! `r{char}` replace operator: normal mode with count, visual, visual-line,
//! visual-block.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode, KeyMods};
use rtdvi::{mode, Editor};
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
fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

// ---- Normal mode -----------------------------------------------------------

#[test]
fn rx_replaces_char_under_cursor() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "rZ");
    assert_eq!(text(&editor), "Zbc\n");
    // Cursor stays put (count == 1 case).
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn rx_at_interior_position() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "llrZ");
    assert_eq!(text(&editor), "heZlo\n");
}

#[test]
fn count_rx_replaces_multiple_chars() {
    let (mut editor, _f) = open("abcdef\n");
    type_keys(&mut editor, "3rX");
    assert_eq!(text(&editor), "XXXdef\n");
    // Cursor lands on the last replaced cell.
    assert_eq!(cursor(&editor), (0, 2));
}

#[test]
fn count_rx_clamps_to_end_of_line() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "10rX"); // count > line len
    // Vim's "refuse" behavior would leave it unchanged; we cap at remaining.
    assert_eq!(text(&editor), "XXX\n");
}

#[test]
fn r_esc_cancels_without_modifying() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "r");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "abc\n");
}

#[test]
fn r_enter_replaces_with_newline() {
    let (mut editor, _f) = open("abcd\n");
    type_keys(&mut editor, "ll");
    type_keys(&mut editor, "r");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(text(&editor), "ab\nd\n");
}

#[test]
fn r_undo_is_single_step() {
    let (mut editor, _f) = open("abcdef\n");
    type_keys(&mut editor, "3rX");
    assert_eq!(text(&editor), "XXXdef\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "abcdef\n");
}

// ---- Visual character-wise -------------------------------------------------

#[test]
fn visual_r_replaces_selection() {
    let (mut editor, _f) = open("hello world\n");
    type_keys(&mut editor, "v"); // start visual at col 0
    type_keys(&mut editor, "llll"); // extend to col 4
    type_keys(&mut editor, "rX");
    assert_eq!(text(&editor), "XXXXX world\n");
}

#[test]
fn visual_r_preserves_newlines_across_lines() {
    let (mut editor, _f) = open("abc\ndef\n");
    type_keys(&mut editor, "v");
    type_keys(&mut editor, "jl"); // include first newline + 2 chars of row 1
    type_keys(&mut editor, "rZ");
    assert_eq!(text(&editor), "ZZZ\nZZf\n");
}

#[test]
fn visual_r_returns_to_normal() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "v");
    type_keys(&mut editor, "l");
    type_keys(&mut editor, "rX");
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Normal);
}

#[test]
fn visual_r_undo_is_single_step() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "v");
    type_keys(&mut editor, "lll");
    type_keys(&mut editor, "rX");
    assert_eq!(text(&editor), "XXXXo\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "hello\n");
}

// ---- Visual-line -----------------------------------------------------------

#[test]
fn vline_r_replaces_full_lines() {
    let (mut editor, _f) = open("abc\ndef\n");
    type_keys(&mut editor, "V");
    type_keys(&mut editor, "j"); // both rows
    type_keys(&mut editor, "rX");
    assert_eq!(text(&editor), "XXX\nXXX\n");
}

// ---- Visual-block ----------------------------------------------------------

#[test]
fn vblock_r_replaces_rectangle() {
    let (mut editor, _f) = open("abcdef\nghijkl\nmnopqr\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jjll"); // rows 0-2, cols 0-2
    type_keys(&mut editor, "rZ");
    assert_eq!(text(&editor), "ZZZdef\nZZZjkl\nZZZpqr\n");
}

#[test]
fn vblock_r_undo_is_single_step() {
    let (mut editor, _f) = open("abc\ndef\nghi\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jjl");
    type_keys(&mut editor, "rZ");
    assert_eq!(text(&editor), "ZZc\nZZf\nZZi\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "abc\ndef\nghi\n");
}

#[test]
fn vblock_r_clears_selection_and_returns_to_normal() {
    let (mut editor, _f) = open("abc\ndef\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jl");
    type_keys(&mut editor, "rX");
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Normal);
    assert!(matches!(
        editor.active_window().unwrap().selection,
        rtdvi::cursor::Selection::None
    ));
}

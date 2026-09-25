//! Readline-style line editing in the `:` command line and `/` search prompt
//! (Ctrl-A/E/U/K/W/D, word motions, mid-line editing) — the terminal-agnostic
//! bindings that give macOS parity with Linux.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode, KeyMods};
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
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

// ---- command line ----------------------------------------------------------

#[test]
fn command_ctrl_a_e_move_to_ends() {
    let (mut editor, _f) = open("x\n");
    type_keys(&mut editor, ":hello world");
    assert_eq!(editor.command_line.cursor, editor.command_line.input.len());
    ctrl(&mut editor, 'a');
    assert_eq!(editor.command_line.cursor, 0);
    ctrl(&mut editor, 'e');
    assert_eq!(editor.command_line.cursor, editor.command_line.input.len());
}

#[test]
fn command_ctrl_a_then_insert_edits_mid_line() {
    let (mut editor, _f) = open("x\n");
    type_keys(&mut editor, ":world");
    ctrl(&mut editor, 'a'); // to start
    type_keys(&mut editor, "hello ");
    assert_eq!(editor.command_line.input, "hello world");
}

#[test]
fn command_ctrl_u_deletes_to_start() {
    let (mut editor, _f) = open("x\n");
    type_keys(&mut editor, ":set number");
    ctrl(&mut editor, 'u');
    assert_eq!(editor.command_line.input, "");
    assert_eq!(editor.command_line.cursor, 0);
}

#[test]
fn command_ctrl_w_deletes_previous_word() {
    let (mut editor, _f) = open("x\n");
    type_keys(&mut editor, ":edit /a/b/c");
    ctrl(&mut editor, 'w');
    assert_eq!(editor.command_line.input, "edit ");
}

#[test]
fn command_ctrl_k_deletes_to_end() {
    let (mut editor, _f) = open("x\n");
    type_keys(&mut editor, ":foo bar");
    ctrl(&mut editor, 'a'); // start
    ctrl(&mut editor, 'e'); // end (no-op distance)
    ctrl(&mut editor, 'a'); // back to start
    ctrl(&mut editor, 'k'); // delete to end
    assert_eq!(editor.command_line.input, "");
}

// ---- search prompt ---------------------------------------------------------

#[test]
fn search_ctrl_u_clears_prompt() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "/foobar");
    ctrl(&mut editor, 'u');
    assert_eq!(editor.search.prompt, "");
}

#[test]
fn search_supports_mid_line_editing() {
    // The search prompt had no cursor movement at all before.
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "/oo");
    ctrl(&mut editor, 'a'); // to start of prompt
    type_keys(&mut editor, "f"); // insert before "oo"
    assert_eq!(editor.search.prompt, "foo");
}

#[test]
fn search_ctrl_w_deletes_previous_word() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "/foo bar");
    ctrl(&mut editor, 'w');
    assert_eq!(editor.search.prompt, "foo ");
}

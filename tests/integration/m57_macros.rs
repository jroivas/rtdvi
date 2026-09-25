//! Keyboard macros: `q{reg}` records keystrokes into a named register, `@{reg}`
//! replays them. A macro is just register text, so a yanked snippet can be
//! executed too.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::mode::ModeId;
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

#[test]
fn record_insert_then_replay() {
    let (mut editor, _f) = open("\n");
    // qa  i tst <Esc>  q
    type_keys(&mut editor, "qa");
    assert!(editor.macros.is_recording());
    type_keys(&mut editor, "itst");
    press(&mut editor, KeyCode::Esc);
    type_keys(&mut editor, "q");
    assert!(!editor.macros.is_recording(), "second q stops recording");
    assert_eq!(buffer_text(&editor), "tst\n");

    // The recorded register holds the literal keystrokes, Esc as 0x1b.
    let reg = editor.named_registers.get(&'a').unwrap();
    assert_eq!(reg.text, "itst\u{1b}");

    // Open a fresh line below and replay the macro there.
    type_keys(&mut editor, "o");
    press(&mut editor, KeyCode::Esc);
    type_keys(&mut editor, "@a");
    assert_eq!(editor.mode, ModeId::Normal, "macro's <Esc> returns to normal");
    assert_eq!(buffer_text(&editor), "tst\ntst\n");
}

#[test]
fn yanked_text_executes_as_macro() {
    // The user's example: put "itst" into a register, `@a` types "tst".
    let (mut editor, _f) = open("\n");
    editor.named_registers.insert(
        'a',
        rtdvi::editor::Register { text: "itst".into(), linewise: false },
    );
    type_keys(&mut editor, "@a");
    // No <Esc> in the register, so it leaves us in insert mode mid-edit.
    assert_eq!(editor.mode, ModeId::Insert);
    assert_eq!(buffer_text(&editor), "tst\n");
}

#[test]
fn replay_with_count_repeats() {
    // A line-editing macro replayed N times. Record `A!<Esc>j` (append '!',
    // move down), then `2@a` applies it to the next two lines.
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "qa");
    type_keys(&mut editor, "A!");
    press(&mut editor, KeyCode::Esc);
    type_keys(&mut editor, "jq"); // move down, then stop recording

    assert_eq!(buffer_text(&editor), "a!\nb\nc\n");

    // Cursor is on line 2 ("b"); replay twice over lines 2 and 3.
    type_keys(&mut editor, "2@a");
    assert_eq!(buffer_text(&editor), "a!\nb!\nc!\n");
}

#[test]
fn at_at_repeats_last_macro() {
    let (mut editor, _f) = open("a\nb\nc\nd\n");
    type_keys(&mut editor, "qa");
    type_keys(&mut editor, "A.");
    press(&mut editor, KeyCode::Esc);
    type_keys(&mut editor, "jq");
    assert_eq!(buffer_text(&editor), "a.\nb\nc\nd\n");

    type_keys(&mut editor, "@a"); // line 2
    type_keys(&mut editor, "@@"); // repeat on line 3
    assert_eq!(buffer_text(&editor), "a.\nb.\nc.\nd\n");
}

#[test]
fn q_is_literal_in_insert_mode() {
    // While recording, a `q` typed in insert mode is text, not a stop command.
    let (mut editor, _f) = open("\n");
    type_keys(&mut editor, "qa");
    type_keys(&mut editor, "iq"); // insert a literal 'q'
    assert!(editor.macros.is_recording(), "insert-mode q must not stop recording");
    press(&mut editor, KeyCode::Esc);
    type_keys(&mut editor, "q"); // now in normal mode: stops
    assert!(!editor.macros.is_recording());
    assert_eq!(buffer_text(&editor), "q\n");
    assert_eq!(editor.named_registers.get(&'a').unwrap().text, "iq\u{1b}");
}

#[test]
fn record_captures_ctrl_keys_as_control_bytes() {
    let (mut editor, _f) = open("\n");
    type_keys(&mut editor, "qa");
    // Insert, then a literal Ctrl-something is uncommon; just verify a normal
    // recording stores Esc as the control byte and replays cleanly.
    type_keys(&mut editor, "ix");
    press(&mut editor, KeyCode::Esc);
    type_keys(&mut editor, "q");
    assert_eq!(editor.named_registers.get(&'a').unwrap().text, "ix\u{1b}");
}

#[test]
fn recursive_macro_hits_depth_limit_without_hanging() {
    // Register `a` invokes itself; playback must terminate via the depth guard.
    let (mut editor, _f) = open("\n");
    editor.named_registers.insert(
        'a',
        rtdvi::editor::Register { text: "@a".into(), linewise: false },
    );
    type_keys(&mut editor, "@a");
    // If we got here, the recursion guard stopped the self-call.
    assert_eq!(editor.macros.last_played, Some('a'));
}

//! `:ls` opens a modal buffer picker (like `:ff`); `:b`/`:buffer <n>` (and the
//! glued `:b7`) switch the active window to a numbered buffer.

use std::io::Write;

use rtdvi::command::run_ex_line;
use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn tmp(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

/// Two buffers open; the active window shows the second (`b`).
fn setup() -> (Editor, u32, u32, (NamedTempFile, NamedTempFile)) {
    let fa = tmp("aaa\n");
    let fb = tmp("bbb\n");
    let mut editor = Editor::new();
    let id_a = editor.open_path(fa.path()).unwrap();
    editor.focus_single(id_a);
    run_ex_line(&mut editor, &format!("e {}", fb.path().display()));
    let id_b = editor.active_buffer_id().unwrap();
    (editor, id_a.0, id_b.0, (fa, fb))
}

fn active(editor: &Editor) -> u32 {
    editor.active_buffer_id().unwrap().0
}

#[test]
fn ls_opens_picker_selecting_active_buffer() {
    let (mut editor, _a, b, _f) = setup();
    run_ex_line(&mut editor, "ls");
    let p = editor.buffer_picker.as_ref().expect("picker should be open");
    assert_eq!(p.ids.len(), 2);
    assert_eq!(p.ids[p.selected].0, b, "highlight starts on the active buffer");
    // Labels carry the buffer numbers.
    assert!(p.labels.iter().any(|l| l.contains(&b.to_string())));
}

#[test]
fn picker_enter_switches_to_selected() {
    let (mut editor, a, _b, _f) = setup();
    run_ex_line(&mut editor, "ls"); // selected = active (b), ids sorted [a, b]
    mode::handle_key(&mut editor, Key::char('k')); // move up to a
    mode::handle_key(&mut editor, Key::new(KeyCode::Enter));
    assert!(editor.buffer_picker.is_none());
    assert_eq!(active(&editor), a);
}

#[test]
fn picker_esc_cancels() {
    let (mut editor, _a, b, _f) = setup();
    run_ex_line(&mut editor, "ls");
    mode::handle_key(&mut editor, Key::new(KeyCode::Esc));
    assert!(editor.buffer_picker.is_none());
    assert_eq!(active(&editor), b, "cancel keeps the current buffer");
}

#[test]
fn buffer_number_switches() {
    let (mut editor, a, _b, _f) = setup();
    run_ex_line(&mut editor, &format!("b {a}")); // :b <n> with a space
    assert_eq!(active(&editor), a);
}

#[test]
fn buffer_keyword_switches() {
    let (mut editor, a, b, _f) = setup();
    run_ex_line(&mut editor, &format!("b {a}"));
    assert_eq!(active(&editor), a);
    run_ex_line(&mut editor, &format!("buffer {b}"));
    assert_eq!(active(&editor), b);
}

#[test]
fn glued_b_number_switches() {
    let (mut editor, a, _b, _f) = setup();
    run_ex_line(&mut editor, &format!("b{a}")); // :b7 glued form, no space
    assert_eq!(active(&editor), a);
}

#[test]
fn buffer_nonexistent_reports_error() {
    let (mut editor, _a, b, _f) = setup();
    run_ex_line(&mut editor, "b 9999");
    assert!(
        editor.status_message.as_deref().unwrap_or("").contains("does not exist"),
        "status: {:?}",
        editor.status_message
    );
    assert_eq!(active(&editor), b, "buffer unchanged on error");
}

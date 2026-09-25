//! `force_save` option: refuse to abandon unsaved buffers when switching or
//! closing, while still allowing writes and new splits. Off by default keeps
//! the permissive neovim-style behaviour.

use std::io::Write;

use rtdvi::command::run_ex_line;
use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn esc(editor: &mut Editor) {
    mode::handle_key(editor, Key::new(KeyCode::Esc));
}
fn dirty_edit(editor: &mut Editor) {
    type_keys(editor, "iX");
    esc(editor);
}
fn status(editor: &Editor) -> String {
    editor.status_message.clone().unwrap_or_default()
}
fn nwin(editor: &Editor) -> usize {
    editor.tabs[0].tree.windows().len()
}

fn tmp(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

/// Editor with file A open, plus file B opened into the same window (so both
/// buffers exist and the active window shows B).
fn two_buffers() -> (Editor, NamedTempFile, NamedTempFile) {
    let a = tmp("aaa\n");
    let b = tmp("bbb\n");
    let mut editor = Editor::new();
    let id_a = editor.open_path(a.path()).unwrap();
    editor.focus_single(id_a);
    run_ex_line(&mut editor, &format!("e {}", b.path().display()));
    (editor, a, b)
}

fn one_buffer() -> (Editor, NamedTempFile) {
    let f = tmp("line1\nline2\n");
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

// ---- switching ------------------------------------------------------------

#[test]
fn without_force_save_bnext_switches_freely() {
    let (mut editor, _a, _b) = two_buffers();
    dirty_edit(&mut editor); // B modified
    let before = editor.active_buffer_id();
    run_ex_line(&mut editor, "bn");
    assert_ne!(editor.active_buffer_id(), before, "default: should switch");
}

#[test]
fn force_save_blocks_bnext_on_modified_buffer() {
    let (mut editor, _a, _b) = two_buffers();
    editor.config.options.force_save = true;
    dirty_edit(&mut editor); // B modified, shown only here
    let before = editor.active_buffer_id();
    run_ex_line(&mut editor, "bn");
    assert_eq!(editor.active_buffer_id(), before, "should refuse to switch");
    assert!(status(&editor).contains("No write since last change"), "{}", status(&editor));
}

#[test]
fn force_save_allows_bnext_after_write() {
    let (mut editor, _a, _b) = two_buffers();
    editor.config.options.force_save = true;
    dirty_edit(&mut editor);
    run_ex_line(&mut editor, "w"); // save B
    let before = editor.active_buffer_id();
    run_ex_line(&mut editor, "bn");
    assert_ne!(editor.active_buffer_id(), before, "clean buffer switches");
}

#[test]
fn force_save_bnext_bang_overrides() {
    let (mut editor, _a, _b) = two_buffers();
    editor.config.options.force_save = true;
    dirty_edit(&mut editor);
    let before = editor.active_buffer_id();
    run_ex_line(&mut editor, "bn!");
    assert_ne!(editor.active_buffer_id(), before, "! forces the switch");
}

#[test]
fn force_save_blocks_edit_switch_on_modified_buffer() {
    let (mut editor, _a, _b) = two_buffers();
    let c = tmp("ccc\n");
    editor.config.options.force_save = true;
    dirty_edit(&mut editor); // B modified
    let before = editor.active_buffer_id();
    run_ex_line(&mut editor, &format!("e {}", c.path().display()));
    assert_eq!(editor.active_buffer_id(), before, "e should refuse to switch");
    assert!(status(&editor).contains("No write since last change"));
}

// ---- splitting ------------------------------------------------------------

#[test]
fn force_save_allows_opening_new_split_with_file() {
    let (mut editor, _f) = one_buffer();
    let b = tmp("bbb\n");
    editor.config.options.force_save = true;
    dirty_edit(&mut editor); // active buffer modified
    run_ex_line(&mut editor, &format!("vsplit {}", b.path().display()));
    assert_eq!(nwin(&editor), 2, "new split allowed despite unsaved buffer");
    let id = editor.active_buffer_id().unwrap();
    assert_eq!(editor.buffers.get(&id).unwrap().rope().to_string(), "bbb\n");
}

// ---- closing --------------------------------------------------------------

#[test]
fn force_save_allows_closing_extra_split_but_not_last() {
    let (mut editor, _f) = one_buffer();
    editor.config.options.force_save = true;
    dirty_edit(&mut editor); // modified
    run_ex_line(&mut editor, "vsplit"); // same buffer in two windows
    assert_eq!(nwin(&editor), 2);
    // Closing one split is fine — the buffer still shows in the other.
    run_ex_line(&mut editor, "close");
    assert_eq!(nwin(&editor), 1);
    // The last window of the modified buffer refuses to close.
    run_ex_line(&mut editor, "close");
    assert_eq!(nwin(&editor), 1, "last split should refuse to close");
    assert!(status(&editor).contains("No write since last change"));
}

#[test]
fn force_save_close_bang_overrides_last_split() {
    let (mut editor, _f) = one_buffer();
    editor.config.options.force_save = true;
    dirty_edit(&mut editor);
    run_ex_line(&mut editor, "vsplit");
    run_ex_line(&mut editor, "close"); // extra split
    run_ex_line(&mut editor, "close!"); // force the last one
    // The tab collapsed to a single window earlier; a forced close of the sole
    // remaining window is a no-op guard-wise but must not be blocked.
    assert!(!status(&editor).contains("No write"), "bang must not be blocked");
}

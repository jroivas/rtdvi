//! `:tab new|next|prev` dispatcher and gt/gT bindings.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
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

fn fresh() -> Editor {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    editor
}

#[test]
fn colon_tab_new_creates_a_tab() {
    let mut editor = fresh();
    assert_eq!(editor.tabs.len(), 1);
    type_keys(&mut editor, ":tab new");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs.len(), 2);
    assert_eq!(editor.active_tab, 1);
}

#[test]
fn colon_tab_next_and_prev_cycle() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tab new");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":tab new");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 2);
    type_keys(&mut editor, ":tab next");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 0);
    type_keys(&mut editor, ":tab prev");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 2);
}

#[test]
fn colon_tab_new_with_file_arg() {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"hello\n").unwrap();
    tmp.flush().unwrap();
    let mut editor = fresh();
    let cmd = format!(":tab new {}", tmp.path().display());
    type_keys(&mut editor, &cmd);
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs.len(), 2);
    let id = editor.active_buffer_id().unwrap();
    let text = editor.buffers.get(&id).unwrap().rope().to_string();
    assert_eq!(text, "hello\n");
}

#[test]
fn colon_tab_with_no_subcommand_errors_without_panic() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tab");
    press(&mut editor, KeyCode::Enter);
    // Status message reports usage; tab list unchanged.
    assert_eq!(editor.tabs.len(), 1);
    assert!(editor.status_message.is_some());
}

#[test]
fn colon_tab_unknown_subcommand_errors() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tab wat");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs.len(), 1);
    assert!(editor.status_message.is_some());
}

#[test]
fn gt_moves_to_next_tab() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 1);
    type_keys(&mut editor, "gt");
    assert_eq!(editor.active_tab, 0);
    type_keys(&mut editor, "gt");
    assert_eq!(editor.active_tab, 1);
}

#[test]
fn capital_gt_moves_to_previous_tab() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 2);
    type_keys(&mut editor, "gT");
    assert_eq!(editor.active_tab, 1);
    type_keys(&mut editor, "gT");
    assert_eq!(editor.active_tab, 0);
}

#[test]
fn original_single_word_commands_still_work() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":tabnext");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 0);
    type_keys(&mut editor, ":tabprev");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 1);
}

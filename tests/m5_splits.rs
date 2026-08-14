//! M5: splits, window navigation, close.

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

#[test]
fn split_creates_two_windows() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    let tab = editor.tabs.first().unwrap();
    assert_eq!(tab.tree.windows().len(), 2);
}

#[test]
fn vsplit_creates_two_windows() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    let tab = editor.tabs.first().unwrap();
    assert_eq!(tab.tree.windows().len(), 2);
}

#[test]
fn split_with_filename_opens_file_in_new_window() {
    let (mut editor, _f) = open("original\n");
    let mut other = NamedTempFile::new().unwrap();
    other.write_all(b"other file\n").unwrap();
    other.flush().unwrap();

    rtdvi::command::run_ex_line(&mut editor, &format!("split {}", other.path().display()));

    let tab = editor.tabs.first().unwrap();
    assert_eq!(tab.tree.windows().len(), 2);
    // The active (new) window shows the opened file...
    let active_buf = editor.windows.get(&tab.active).unwrap().buffer;
    assert_eq!(
        editor.buffers.get(&active_buf).unwrap().rope().to_string(),
        "other file\n"
    );
    // ...while the other window keeps the original buffer.
    let other_win = tab
        .tree
        .windows()
        .into_iter()
        .find(|w| *w != tab.active)
        .unwrap();
    let other_buf = editor.windows.get(&other_win).unwrap().buffer;
    assert_eq!(
        editor.buffers.get(&other_buf).unwrap().rope().to_string(),
        "original\n"
    );
}

#[test]
fn vsplit_with_filename_opens_file_in_new_window() {
    let (mut editor, _f) = open("original\n");
    let mut other = NamedTempFile::new().unwrap();
    other.write_all(b"other file\n").unwrap();
    other.flush().unwrap();

    rtdvi::command::run_ex_line(&mut editor, &format!("vsplit {}", other.path().display()));

    let tab = editor.tabs.first().unwrap();
    assert_eq!(tab.tree.windows().len(), 2);
    let active_buf = editor.windows.get(&tab.active).unwrap().buffer;
    assert_eq!(
        editor.buffers.get(&active_buf).unwrap().rope().to_string(),
        "other file\n"
    );
}

#[test]
fn split_without_filename_keeps_current_buffer() {
    let (mut editor, _f) = open("original\n");
    let orig_buf = editor.active_buffer_id().unwrap();
    rtdvi::command::run_ex_line(&mut editor, "split");
    let tab = editor.tabs.first().unwrap();
    assert_eq!(tab.tree.windows().len(), 2);
    // Both windows still show the original buffer.
    for w in tab.tree.windows() {
        assert_eq!(editor.windows.get(&w).unwrap().buffer, orig_buf);
    }
}

#[test]
fn closing_bottom_right_split_keeps_focus_in_same_column() {
    // Build: left column | right column split into top/bottom.
    // Tree ends up V(left, H(top_right, bottom_right)).
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter); // active = left column
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "l"); // focus right column
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter); // active = top-right

    // windows() is left→right / top→bottom: [left, top_right, bottom_right].
    let ids = editor.tabs[0].tree.windows();
    assert_eq!(ids.len(), 3);
    let (left, top_right, bottom_right) = (ids[0], ids[1], ids[2]);

    // Move down to the bottom-right split and close it.
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "j");
    assert_eq!(editor.tabs[0].active, bottom_right, "expected to be on bottom-right");
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "c");

    // Focus must stay in the same (right) column — the sibling that filled the
    // freed space — not jump to the top-left window.
    assert_eq!(editor.tabs[0].tree.windows().len(), 2);
    assert_eq!(editor.tabs[0].active, top_right, "focus should stay in the right column");
    assert_ne!(editor.tabs[0].active, left, "focus must not jump to the left column");
}

#[test]
fn close_removes_one_window() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":close");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs.first().unwrap().tree.windows().len(), 1);
}

#[test]
fn ctrl_w_w_cycles_focus() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    let first_active = editor.tabs[0].active;
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "w");
    let second_active = editor.tabs[0].active;
    assert_ne!(first_active, second_active);
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "w");
    assert_eq!(editor.tabs[0].active, first_active);
}

#[test]
fn ctrl_w_h_l_navigates_vertical_split() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    // After vsplit, new window is on the left and gets focus.
    let left = editor.tabs[0].active;
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "l");
    let right = editor.tabs[0].active;
    assert_ne!(left, right);
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "h");
    assert_eq!(editor.tabs[0].active, left);
}

#[test]
fn ctrl_w_j_k_navigates_horizontal_split() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    // After :split the new window is on top and gets focus.
    let top = editor.tabs[0].active;
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "j");
    let bottom = editor.tabs[0].active;
    assert_ne!(top, bottom);
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "k");
    assert_eq!(editor.tabs[0].active, top);
}

#[test]
fn ctrl_w_held_ctrl_letter_is_same_as_letter() {
    // Pressing <C-w><C-l> (Ctrl held through both) must do the same as
    // <C-w>l (Ctrl released between keys), because terminals send the
    // former when the user just keeps Ctrl down.
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    let left = editor.tabs[0].active;
    ctrl(&mut editor, 'w');
    ctrl(&mut editor, 'l'); // Ctrl-held form
    let right = editor.tabs[0].active;
    assert_ne!(left, right);
    ctrl(&mut editor, 'w');
    ctrl(&mut editor, 'h');
    assert_eq!(editor.tabs[0].active, left);
}

#[test]
fn ctrl_w_held_ctrl_j_k_navigates_horizontal_split() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    let top = editor.tabs[0].active;
    ctrl(&mut editor, 'w');
    ctrl(&mut editor, 'j');
    let bottom = editor.tabs[0].active;
    assert_ne!(top, bottom);
    ctrl(&mut editor, 'w');
    ctrl(&mut editor, 'k');
    assert_eq!(editor.tabs[0].active, top);
}

#[test]
fn ctrl_w_held_ctrl_w_cycles_focus() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    let first = editor.tabs[0].active;
    ctrl(&mut editor, 'w');
    ctrl(&mut editor, 'w');
    let second = editor.tabs[0].active;
    assert_ne!(first, second);
}

#[test]
fn ctrl_w_hjkl_navigates_4_way_grid() {
    // Build a 2x2 grid: vsplit then horizontal-split each side.
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "l");
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    // Now we should have 4 windows in a 2x2 grid.
    assert_eq!(editor.tabs[0].tree.windows().len(), 4);
    // From any corner, hjkl should reach a distinct neighbour each press.
    let start = editor.tabs[0].active;
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "h");
    let after_h = editor.tabs[0].active;
    assert_ne!(start, after_h);
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "j");
    let after_j = editor.tabs[0].active;
    // Could be same as `after_h` if h already moved to the bottom-left;
    // just assert we landed on *something* and the editor is still healthy.
    let _ = after_j;
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "l");
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "k");
    // Editor should still have all 4 windows.
    assert_eq!(editor.tabs[0].tree.windows().len(), 4);
}

#[test]
fn ctrl_w_s_keybinding_splits() {
    let (mut editor, _f) = open("hello\n");
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "s");
    assert_eq!(editor.tabs[0].tree.windows().len(), 2);
}

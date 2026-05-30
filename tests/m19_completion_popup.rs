//! Wildmenu-style completion popup: with multiple matches, the popup opens
//! immediately on the first Tab. Arrows navigate; Right/Enter accept; Left closes.

use std::fs;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;

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

fn fixture_dir() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("alpha.txt"), "").unwrap();
    fs::write(dir.path().join("apple.md"), "").unwrap();
    fs::write(dir.path().join("apricot.rs"), "").unwrap();
    dir
}

#[test]
fn first_tab_shows_popup_immediately_on_multiple_matches() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    // Multiple matches → popup opens on first Tab.
    assert!(comp.popup_visible);
}

#[test]
fn second_tab_advances_to_next_match() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    assert!(editor.command_line.input.ends_with("/alpha.txt"));
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert!(comp.popup_visible);
    assert!(editor.command_line.input.ends_with("/apple.md"));
}

#[test]
fn down_arrow_advances_selection_in_popup() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab); // popup visible, index=0 -> alpha.txt
    press(&mut editor, KeyCode::Down);
    assert!(editor.command_line.input.ends_with("/apple.md"));
    press(&mut editor, KeyCode::Down);
    assert!(editor.command_line.input.ends_with("/apricot.rs"));
}

#[test]
fn up_arrow_wraps_around() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab); // popup visible, index=0 -> alpha.txt
    press(&mut editor, KeyCode::Up); // wrap to last entry -> apricot.rs
    assert!(editor.command_line.input.ends_with("/apricot.rs"));
}

#[test]
fn right_accepts_and_closes_popup() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Down); // -> apple.md
    let kept = editor.command_line.input.clone();
    press(&mut editor, KeyCode::Right);
    assert_eq!(editor.command_line.input, kept);
    assert!(!editor.command_line.completion.as_ref().unwrap().popup_visible);
    // We're still in command mode (Right doesn't run).
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Command);
}

#[test]
fn left_closes_popup_without_running() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Down); // -> apple.md
    press(&mut editor, KeyCode::Left);
    // Popup gone, completion state cleared, still in command mode.
    assert!(editor.command_line.completion.is_none());
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Command);
}

#[test]
fn enter_in_popup_accepts_and_runs_command() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab); // popup visible, index=0 -> alpha.txt
    press(&mut editor, KeyCode::Down); // apple.md
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Normal);
    let id = editor.active_buffer_id().unwrap();
    let path = editor.buffers.get(&id).unwrap().path().unwrap();
    assert!(path.ends_with("apple.md"));
}

#[test]
fn typing_dismisses_popup_and_clears_completion() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Tab);
    type_keys(&mut editor, "p");
    assert!(editor.command_line.completion.is_none());
}

#[test]
fn esc_in_popup_exits_command_mode() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Esc);
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Normal);
    assert!(editor.command_line.completion.is_none());
}

#[test]
fn popup_renders_to_test_backend() {
    let dir = fixture_dir();
    let mut editor = fresh();
    type_keys(&mut editor, &format!(":e {}/a", dir.path().display()));
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Tab);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let area = *buf.area();
    let mut rendered = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            rendered.push_str(buf[(x, y)].symbol());
        }
        rendered.push('\n');
    }
    // The popup should contain all three filenames.
    assert!(rendered.contains("alpha.txt"), "frame:\n{rendered}");
    assert!(rendered.contains("apple.md"), "frame:\n{rendered}");
    assert!(rendered.contains("apricot.rs"), "frame:\n{rendered}");
}

#[test]
fn arrow_keys_without_popup_are_ignored() {
    let mut editor = fresh();
    type_keys(&mut editor, ":hello");
    let before = editor.command_line.input.clone();
    press(&mut editor, KeyCode::Up);
    press(&mut editor, KeyCode::Down);
    press(&mut editor, KeyCode::Left);
    press(&mut editor, KeyCode::Right);
    assert_eq!(editor.command_line.input, before);
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Command);
}

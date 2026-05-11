//! `:vi <file>` alias and Tab completion for file paths.

use std::fs;
use std::io::Write;

use jvim::keymap::keys::{Key, KeyCode};
use jvim::{mode, Editor};
use tempfile::{NamedTempFile, TempDir};

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

// ---- :vi alias -------------------------------------------------------------

#[test]
fn vi_opens_file_like_e() {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"hello from vi\n").unwrap();
    tmp.flush().unwrap();
    let mut editor = fresh();
    let cmd = format!(":vi {}", tmp.path().display());
    type_keys(&mut editor, &cmd);
    press(&mut editor, KeyCode::Enter);
    let id = editor.active_buffer_id().unwrap();
    let text = editor.buffers.get(&id).unwrap().rope().to_string();
    assert_eq!(text, "hello from vi\n");
}

#[test]
fn visual_alias_also_works() {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"x\n").unwrap();
    tmp.flush().unwrap();
    let mut editor = fresh();
    let cmd = format!(":visual {}", tmp.path().display());
    type_keys(&mut editor, &cmd);
    press(&mut editor, KeyCode::Enter);
    let id = editor.active_buffer_id().unwrap();
    assert_eq!(editor.buffers.get(&id).unwrap().rope().to_string(), "x\n");
}

// ---- Tab completion --------------------------------------------------------

/// Build a temp dir with deterministic contents.
fn fixture_dir() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("alpha.txt"), "").unwrap();
    fs::write(dir.path().join("apple.md"), "").unwrap();
    fs::write(dir.path().join("banana"), "").unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    fs::write(dir.path().join("nested").join("inner.txt"), "").unwrap();
    dir
}

#[test]
fn tab_completes_unique_match() {
    let dir = fixture_dir();
    let mut editor = fresh();
    let typed = format!(":e {}/ban", dir.path().display());
    type_keys(&mut editor, &typed);
    press(&mut editor, KeyCode::Tab);
    let expected = format!("e {}/banana", dir.path().display());
    assert_eq!(editor.command_line.input, expected);
}

#[test]
fn tab_cycles_through_multiple_matches() {
    let dir = fixture_dir();
    let mut editor = fresh();
    let typed = format!(":e {}/a", dir.path().display());
    type_keys(&mut editor, &typed);
    press(&mut editor, KeyCode::Tab);
    // First match (sorted): "alpha.txt"
    let after_first = editor.command_line.input.clone();
    assert!(
        after_first.ends_with("/alpha.txt"),
        "first tab gave {after_first:?}"
    );
    press(&mut editor, KeyCode::Tab);
    let after_second = editor.command_line.input.clone();
    assert!(
        after_second.ends_with("/apple.md"),
        "second tab gave {after_second:?}"
    );
    // Third tab wraps back to the first match.
    press(&mut editor, KeyCode::Tab);
    assert_eq!(editor.command_line.input, after_first);
}

#[test]
fn tab_appends_slash_for_directories() {
    let dir = fixture_dir();
    let mut editor = fresh();
    let typed = format!(":e {}/nes", dir.path().display());
    type_keys(&mut editor, &typed);
    press(&mut editor, KeyCode::Tab);
    assert!(
        editor.command_line.input.ends_with("/nested/"),
        "got {:?}",
        editor.command_line.input
    );
}

#[test]
fn tab_with_no_matches_is_noop() {
    let dir = fixture_dir();
    let mut editor = fresh();
    let typed = format!(":e {}/zzzz", dir.path().display());
    type_keys(&mut editor, &typed);
    let before = editor.command_line.input.clone();
    press(&mut editor, KeyCode::Tab);
    assert_eq!(editor.command_line.input, before);
    assert!(editor.command_line.completion.is_none());
}

#[test]
fn tab_at_command_name_does_not_complete() {
    // Cursor right after `:e` (no space) — partial would be the command itself.
    let mut editor = fresh();
    type_keys(&mut editor, ":e");
    press(&mut editor, KeyCode::Tab);
    assert_eq!(editor.command_line.input, "e");
}

#[test]
fn typing_after_tab_resets_cycle() {
    let dir = fixture_dir();
    let mut editor = fresh();
    let typed = format!(":e {}/a", dir.path().display());
    type_keys(&mut editor, &typed);
    press(&mut editor, KeyCode::Tab);
    assert!(editor.command_line.completion.is_some());
    // User types a char — should clear completion.
    type_keys(&mut editor, "p");
    assert!(editor.command_line.completion.is_none());
}

#[test]
fn backspace_after_tab_resets_cycle() {
    let dir = fixture_dir();
    let mut editor = fresh();
    let typed = format!(":e {}/a", dir.path().display());
    type_keys(&mut editor, &typed);
    press(&mut editor, KeyCode::Tab);
    assert!(editor.command_line.completion.is_some());
    press(&mut editor, KeyCode::Backspace);
    assert!(editor.command_line.completion.is_none());
}

#[test]
fn esc_clears_completion_and_exits() {
    let dir = fixture_dir();
    let mut editor = fresh();
    let typed = format!(":e {}/a", dir.path().display());
    type_keys(&mut editor, &typed);
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Esc);
    assert!(editor.command_line.completion.is_none());
    assert_eq!(editor.mode, jvim::mode::ModeId::Normal);
}

#[test]
fn enter_after_tab_runs_the_completed_command() {
    let dir = fixture_dir();
    let mut editor = fresh();
    let typed = format!(":e {}/ban", dir.path().display());
    type_keys(&mut editor, &typed);
    press(&mut editor, KeyCode::Tab); // -> .../banana
    press(&mut editor, KeyCode::Enter);
    let id = editor.active_buffer_id().unwrap();
    let path = editor.buffers.get(&id).unwrap().path().unwrap();
    assert!(path.ends_with("banana"), "got path {:?}", path);
}

#[test]
fn tab_completes_vi_command_too() {
    let dir = fixture_dir();
    let mut editor = fresh();
    let typed = format!(":vi {}/ban", dir.path().display());
    type_keys(&mut editor, &typed);
    press(&mut editor, KeyCode::Tab);
    assert!(editor.command_line.input.ends_with("/banana"));
}

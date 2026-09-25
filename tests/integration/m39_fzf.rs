//! `:ff` interactive fuzzy file finder.

use std::fs;
use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, Editor};
use tempfile::TempDir;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}

/// Build a workspace that matches the user's example so we can test
/// `:ff posix.c share` ranks `lib/vfs/access_layer/smb/smb_share_vfs_posix.c`
/// near the top.
fn make_workspace() -> TempDir {
    let dir = TempDir::new().unwrap();
    let files = [
        "src/main.rs",
        "src/lib.rs",
        "lib/vfs/access_layer/smb/smb_share_vfs_posix.c",
        "lib/vfs/access_layer/smb/smb_share_vfs_posix.h",
        "lib/vfs/access_layer/smb/smb_other.c",
        "lib/vfs/access_layer/cifs/cifs_share_vfs_posix.c",
        "lib/vfs/access_layer/cifs/cifs.c",
        "include/share.h",
        "tests/posix.c",
        "tests/test_share.c",
        "docs/README.md",
        "build/generated.c",
    ];
    for path in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, "").unwrap();
    }
    dir
}

use std::sync::Mutex;
static CWD_GUARD: Mutex<()> = Mutex::new(());

/// Set CWD to `dir` and return a guard that holds the mutex for the
/// lifetime of the test. Tests in this file all touch the process-wide
/// CWD, so they must serialize.
fn editor_in(dir: &TempDir) -> (Editor, std::sync::MutexGuard<'static, ()>) {
    let guard = CWD_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    std::env::set_current_dir(dir.path()).unwrap();
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    (editor, guard)
}

#[test]
fn ff_query_shows_popup_with_matches() {
    let dir = make_workspace();
    let (mut editor, _g) = editor_in(&dir);
    type_keys(&mut editor, ":ff posix.c share");
    // Popup should be visible with matches.
    let ff = editor.fzf_state.as_ref().expect("ff session active");
    assert!(!ff.matches.is_empty(), "expected matches");
    // The smb path containing both tokens should rank above the cifs
    // alternative for typical query weights.
    assert!(
        ff.matches.iter().any(|m| m == "lib/vfs/access_layer/smb/smb_share_vfs_posix.c"),
        "smb path missing from matches: {:?}",
        ff.matches
    );
    assert_eq!(
        ff.matches[0],
        "lib/vfs/access_layer/smb/smb_share_vfs_posix.c",
        "expected the smb path to be the top match; full list:\n{:#?}",
        ff.matches
    );
}

#[test]
fn ff_enter_opens_selected_file() {
    let dir = make_workspace();
    let (mut editor, _g) = editor_in(&dir);
    type_keys(&mut editor, ":ff readme");
    press(&mut editor, KeyCode::Enter);
    let id = editor.active_buffer_id().unwrap();
    let path = editor.buffers.get(&id).unwrap().path().unwrap();
    assert!(path.ends_with("docs/README.md"), "got {path:?}");
}

#[test]
fn ff_open_recorded_in_edit_history() {
    let dir = make_workspace();
    let (mut editor, _g) = editor_in(&dir);
    // Redirect the history file so the test never touches real user state.
    editor.history.file_path = dir.path().join("history");
    type_keys(&mut editor, ":ff readme");
    press(&mut editor, KeyCode::Enter);
    // The opened file is recorded as the equivalent `:vi <path>`.
    assert_eq!(
        editor.history.entries.last().map(String::as_str),
        Some("vi docs/README.md")
    );
    // Browsing `:vi ` then Up surfaces it as the first hit.
    type_keys(&mut editor, ":vi ");
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.command_line.input, "vi docs/README.md");
}

#[test]
fn ff_arrow_keys_navigate_popup() {
    let dir = make_workspace();
    let (mut editor, _g) = editor_in(&dir);
    type_keys(&mut editor, ":ff posix");
    let ff = editor.fzf_state.as_ref().unwrap();
    assert!(ff.matches.len() > 1);
    let first = ff.matches[0].clone();
    let second = ff.matches[1].clone();
    assert_eq!(ff.selected, 0);
    press(&mut editor, KeyCode::Down);
    assert_eq!(editor.fzf_state.as_ref().unwrap().selected, 1);
    press(&mut editor, KeyCode::Down);
    assert_eq!(editor.fzf_state.as_ref().unwrap().selected, 2);
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.fzf_state.as_ref().unwrap().selected, 1);
    // Enter opens the second entry, not the first.
    press(&mut editor, KeyCode::Enter);
    let id = editor.active_buffer_id().unwrap();
    let path = editor.buffers.get(&id).unwrap().path().unwrap();
    assert!(path.to_string_lossy().ends_with(&second));
    assert!(!path.to_string_lossy().ends_with(&first));
}

#[test]
fn ff_esc_cancels_without_opening() {
    let dir = make_workspace();
    let (mut editor, _g) = editor_in(&dir);
    let starting_buf = editor.active_buffer_id().unwrap();
    type_keys(&mut editor, ":ff posix");
    press(&mut editor, KeyCode::Esc);
    // Still on the scratch buffer.
    assert_eq!(editor.active_buffer_id().unwrap(), starting_buf);
    assert!(editor.fzf_state.is_none());
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Normal);
}

#[test]
fn ff_refines_matches_as_user_types_and_deletes() {
    let dir = make_workspace();
    let (mut editor, _g) = editor_in(&dir);
    type_keys(&mut editor, ":ff posix");
    let initial = editor.fzf_state.as_ref().unwrap().matches.len();
    assert!(initial > 1);
    // Refining with a second token narrows down.
    type_keys(&mut editor, " share");
    let refined = editor.fzf_state.as_ref().unwrap().matches.len();
    assert!(
        refined < initial,
        "adding ' share' should narrow matches: {refined} >= {initial}"
    );
    // Backspace the refinement away — count goes back up.
    for _ in 0..6 {
        press(&mut editor, KeyCode::Backspace);
    }
    let after_delete = editor.fzf_state.as_ref().unwrap().matches.len();
    assert!(
        after_delete >= refined,
        "backspace should re-widen matches: after_delete={after_delete} refined={refined}"
    );
}

#[test]
fn ff_with_no_query_lists_some_files() {
    let dir = make_workspace();
    let (mut editor, _g) = editor_in(&dir);
    type_keys(&mut editor, ":ff");
    let ff = editor.fzf_state.as_ref().unwrap();
    assert!(!ff.matches.is_empty());
}

#[test]
fn ff_does_not_trigger_for_unrelated_commands() {
    let dir = make_workspace();
    let (mut editor, _g) = editor_in(&dir);
    type_keys(&mut editor, ":file");
    assert!(editor.fzf_state.is_none());
}

#[test]
fn ff_bang_rebuilds_index() {
    let dir = make_workspace();
    let (mut editor, _g) = editor_in(&dir);
    // Trigger initial build.
    type_keys(&mut editor, ":ff");
    let before = editor.fzf_index.is_some();
    assert!(before);
    press(&mut editor, KeyCode::Esc);
    // Add a new file the cached index doesn't know about.
    let new_file = dir.path().join("brand_new.txt");
    let mut f = std::fs::File::create(&new_file).unwrap();
    f.write_all(b"hi").unwrap();
    // Plain `:ff brand` still uses the cached index, no result.
    type_keys(&mut editor, ":ff brand");
    let cached_misses = editor
        .fzf_state
        .as_ref()
        .unwrap()
        .matches
        .iter()
        .all(|m| !m.contains("brand_new"));
    assert!(cached_misses, "cached search wouldn't find brand_new yet");
    press(&mut editor, KeyCode::Esc);
    // Bang clears the cache; new file appears.
    type_keys(&mut editor, ":ff!brand");
    let bang_finds = editor
        .fzf_state
        .as_ref()
        .unwrap()
        .matches
        .iter()
        .any(|m| m.contains("brand_new"));
    assert!(bang_finds, "expected `:ff!brand` to find brand_new.txt");
}

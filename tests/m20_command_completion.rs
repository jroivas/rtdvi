//! Tab completion of ex command names.

use jvim::keymap::keys::{Key, KeyCode};
use jvim::{mode, Editor};

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
fn unique_command_prefix_fills_in_full_name() {
    // Only one registered name starts with "vspl".
    let mut editor = fresh();
    type_keys(&mut editor, ":vspl");
    press(&mut editor, KeyCode::Tab);
    assert_eq!(editor.command_line.input, "vsplit");
    // Single match: state is still set so a second Tab would open the popup
    // (it'd just be a one-row popup).
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert_eq!(comp.matches.len(), 1);
    assert_eq!(comp.matches[0], "vsplit");
}

#[test]
fn second_tab_on_unique_match_opens_popup() {
    let mut editor = fresh();
    type_keys(&mut editor, ":vspl");
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Tab);
    assert!(editor.command_line.completion.as_ref().unwrap().popup_visible);
}

#[test]
fn tab_on_multi_match_inserts_first_then_popup() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tab");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap().clone();
    assert!(comp.matches.len() > 1, "expected several tab* matches, got {:?}", comp.matches);
    // First Tab inserts the first match (alphabetical).
    assert_eq!(editor.command_line.input, comp.matches[0]);
    assert!(!comp.popup_visible);
    // Second Tab reveals the popup.
    press(&mut editor, KeyCode::Tab);
    assert!(editor.command_line.completion.as_ref().unwrap().popup_visible);
}

#[test]
fn down_arrow_cycles_command_matches() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tab");
    press(&mut editor, KeyCode::Tab);
    press(&mut editor, KeyCode::Tab); // popup visible
    let first = editor.command_line.input.clone();
    press(&mut editor, KeyCode::Down);
    assert_ne!(editor.command_line.input, first);
}

#[test]
fn enter_after_command_completion_runs_the_command() {
    let mut editor = fresh();
    type_keys(&mut editor, ":vspl");
    press(&mut editor, KeyCode::Tab); // -> vsplit
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs[0].tree.windows().len(), 2);
}

#[test]
fn matching_includes_aliases() {
    // `tabe` is an alias for tabnew; should appear in the match list.
    let mut editor = fresh();
    type_keys(&mut editor, ":tabe");
    press(&mut editor, KeyCode::Tab);
    let comp = editor.command_line.completion.as_ref().unwrap();
    assert!(
        comp.matches.iter().any(|m| m == "tabe" || m == "tabedit"),
        "matches: {:?}",
        comp.matches
    );
}

#[test]
fn no_matches_keeps_input_unchanged() {
    let mut editor = fresh();
    type_keys(&mut editor, ":zzzz");
    let before = editor.command_line.input.clone();
    press(&mut editor, KeyCode::Tab);
    assert_eq!(editor.command_line.input, before);
    assert!(editor.command_line.completion.is_none());
}

#[test]
fn typing_after_command_completion_clears_state() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tab");
    press(&mut editor, KeyCode::Tab);
    assert!(editor.command_line.completion.is_some());
    type_keys(&mut editor, "n");
    assert!(editor.command_line.completion.is_none());
}

#[test]
fn command_completion_does_not_interfere_with_file_completion() {
    // After the command is typed, completing a file arg still routes to file completion.
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("readme"), "").unwrap();
    let mut editor = fresh();
    let typed = format!(":e {}/rea", dir.path().display());
    type_keys(&mut editor, &typed);
    press(&mut editor, KeyCode::Tab);
    assert!(
        editor.command_line.input.ends_with("/readme"),
        "got {:?}",
        editor.command_line.input
    );
}

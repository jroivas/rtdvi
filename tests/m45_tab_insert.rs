//! Tab key in insert mode: `expandtab` (default `true`) substitutes
//! spaces to the next tab stop; `expandtab = false` inserts a literal
//! `\t`; Shift+Tab (`BackTab`) always inserts a literal `\t`.

use std::io::Write;

use rtdvi::config::Config;
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

// ---- Default: tab key inserts spaces -------------------------------------

#[test]
fn default_tab_inserts_spaces_to_next_stop() {
    // Default config: expandtab=true, tab_width=4.
    let (mut editor, _f) = open("\n");
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::Tab);
    // From col 0, tab fills four spaces.
    assert_eq!(buffer_text(&editor), "    \n");
}

#[test]
fn tab_pads_to_next_multiple_of_tab_width() {
    let (mut editor, _f) = open("\n");
    type_keys(&mut editor, "iab"); // cursor now at col 2
    press(&mut editor, KeyCode::Tab);
    // tab_width=4, col=2 → insert 2 spaces to reach col 4.
    assert_eq!(buffer_text(&editor), "ab  \n");
}

#[test]
fn tab_at_tab_stop_inserts_full_tab_width() {
    let (mut editor, _f) = open("\n");
    type_keys(&mut editor, "iabcd"); // cursor at col 4 (exactly on a stop)
    press(&mut editor, KeyCode::Tab);
    // col=4 → 4 spaces, lands on col 8.
    assert_eq!(buffer_text(&editor), "abcd    \n");
}

#[test]
fn custom_tab_width_drives_spacing() {
    let cfg: Config = toml::from_str(
        r#"
[options]
tab_width = 2
"#,
    )
    .unwrap();
    let (mut editor, _f) = open("\n");
    editor.apply_config(cfg);
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::Tab);
    assert_eq!(buffer_text(&editor), "  \n");
}

// ---- expandtab = false: literal tab --------------------------------------

#[test]
fn expandtab_off_inserts_literal_tab() {
    let cfg: Config = toml::from_str(
        r#"
[options]
expandtab = false
"#,
    )
    .unwrap();
    let (mut editor, _f) = open("\n");
    editor.apply_config(cfg);
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::Tab);
    assert_eq!(buffer_text(&editor), "\t\n");
}

// ---- Shift+Tab: always literal tab --------------------------------------

#[test]
fn shift_tab_inserts_literal_tab_even_with_expandtab() {
    // Default expandtab=true, but BackTab (Shift+Tab) bypasses it.
    let (mut editor, _f) = open("\n");
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::BackTab);
    assert_eq!(buffer_text(&editor), "\t\n");
}

#[test]
fn shift_tab_works_with_expandtab_off_too() {
    let cfg: Config = toml::from_str(
        r#"
[options]
expandtab = false
"#,
    )
    .unwrap();
    let (mut editor, _f) = open("\n");
    editor.apply_config(cfg);
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::BackTab);
    assert_eq!(buffer_text(&editor), "\t\n");
}

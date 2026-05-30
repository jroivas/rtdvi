//! `:highlight` / `:nohighlight` and `<leader>m` toggle on the word
//! under the cursor.

use std::io::Write;

use rtdvi::config::Config;
use rtdvi::highlights::PALETTE;
use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
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
    let mut f = NamedTempFile::with_suffix(".txt").unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

// ---- :highlight / :nohighlight --------------------------------------------

#[test]
fn highlight_command_adds_entry() {
    let (mut editor, _f) = open("foo bar foo\n");
    type_keys(&mut editor, ":highlight foo");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.highlights.entries.len(), 1);
    assert_eq!(editor.highlights.entries[0].pattern, "foo");
}

#[test]
fn highlight_command_toggles_off_on_second_call() {
    let (mut editor, _f) = open("foo bar foo\n");
    type_keys(&mut editor, ":highlight foo");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":highlight foo");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.highlights.is_empty());
}

#[test]
fn nohighlight_with_text_removes_one() {
    let (mut editor, _f) = open("foo bar baz\n");
    type_keys(&mut editor, ":highlight foo");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":highlight bar");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.highlights.entries.len(), 2);
    type_keys(&mut editor, ":nohighlight foo");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.highlights.entries.len(), 1);
    assert!(editor.highlights.entries.iter().all(|e| e.pattern != "foo"));
}

#[test]
fn nohighlight_with_no_args_clears_all() {
    let (mut editor, _f) = open("foo bar baz\n");
    type_keys(&mut editor, ":highlight foo");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":highlight bar");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":nohighlight");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.highlights.is_empty());
}

#[test]
fn highlight_palette_exhausts_then_drops_oldest() {
    let (mut editor, _f) = open("x\n");
    for i in 0..PALETTE.len() {
        type_keys(&mut editor, &format!(":highlight t{i}"));
        press(&mut editor, KeyCode::Enter);
    }
    assert_eq!(editor.highlights.entries.len(), PALETTE.len());
    let oldest_pat = editor.highlights.entries[0].pattern.clone();
    // One more — should evict the oldest.
    type_keys(&mut editor, ":highlight overflow");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.highlights.entries.len(), PALETTE.len());
    assert!(editor
        .highlights
        .entries
        .iter()
        .all(|e| e.pattern != oldest_pat));
}

#[test]
fn hl_and_nohl_short_aliases_work() {
    let (mut editor, _f) = open("x\n");
    type_keys(&mut editor, ":hl foo");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.highlights.entries.len(), 1);
    type_keys(&mut editor, ":nohl foo");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.highlights.is_empty());
}

#[test]
fn highlight_with_no_arg_reports_usage() {
    let (mut editor, _f) = open("x\n");
    type_keys(&mut editor, ":highlight");
    press(&mut editor, KeyCode::Enter);
    let status = editor.status_message.as_deref().unwrap_or("");
    assert!(status.contains("usage"), "got: {status:?}");
}

// ---- <leader>m toggle on word under cursor --------------------------------

#[test]
fn backslash_m_toggles_word_under_cursor() {
    // Cursor at (0, 0) sits on `foo` in "foo bar".
    let (mut editor, _f) = open("foo bar\n");
    type_keys(&mut editor, "\\m");
    assert_eq!(editor.highlights.entries.len(), 1);
    // The pattern is the word-bounded form.
    assert_eq!(editor.highlights.entries[0].pattern, r"\bfoo\b");
    // Press again to toggle off.
    type_keys(&mut editor, "\\m");
    assert!(editor.highlights.is_empty());
}

/// User's exact bug: `:highlight something` then cursor onto a
/// `something` instance and `<leader>m` should DE-highlight it,
/// not append a second entry.
#[test]
fn leader_m_removes_existing_literal_highlight() {
    let (mut editor, _f) = open("something else\n");
    type_keys(&mut editor, ":highlight something");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.highlights.entries.len(), 1);
    // Cursor at (0,0) sits on `something`. Pressing `\m` should remove
    // the literal entry rather than adding a `\bsomething\b` one.
    type_keys(&mut editor, "\\m");
    assert!(
        editor.highlights.is_empty(),
        "expected the existing highlight to be removed; entries: {:?}",
        editor
            .highlights
            .entries
            .iter()
            .map(|e| &e.pattern)
            .collect::<Vec<_>>()
    );
}

/// And the inverse — `<leader>m` first, then `:highlight` of the
/// same word — should toggle off, not duplicate.
#[test]
fn highlight_command_removes_existing_word_bounded_highlight() {
    let (mut editor, _f) = open("foo bar foo\n");
    // Sits on `foo` at (0,0).
    type_keys(&mut editor, "\\m");
    assert_eq!(editor.highlights.entries.len(), 1);
    assert_eq!(editor.highlights.entries[0].pattern, r"\bfoo\b");
    // `:highlight foo` should now remove the word-bounded entry.
    type_keys(&mut editor, ":highlight foo");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.highlights.is_empty());
}

/// `:nohighlight foo` should remove a highlight regardless of which
/// flavour created it.
#[test]
fn nohighlight_removes_word_bounded_entry() {
    let (mut editor, _f) = open("foo bar\n");
    type_keys(&mut editor, "\\m"); // word-bounded form
    assert_eq!(editor.highlights.entries.len(), 1);
    type_keys(&mut editor, ":nohighlight foo");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.highlights.is_empty());
}

#[test]
fn backslash_m_no_op_on_whitespace() {
    let (mut editor, _f) = open("   foo\n");
    type_keys(&mut editor, "\\m");
    assert!(editor.highlights.is_empty());
    let status = editor.status_message.as_deref().unwrap_or("");
    assert!(status.contains("no word"), "got: {status:?}");
}

// ---- Configurable leader --------------------------------------------------

#[test]
fn configured_leader_expands_in_user_keymap() {
    let toml = r#"
[options]
leader = ","
[[keymaps]]
mode = "normal"
keys = "<leader>m"
action = "highlight_toggle_word_under_cursor"
"#;
    let cfg: Config = toml::from_str(toml).unwrap();
    let (mut editor, _f) = open("hello\n");
    editor.apply_config(cfg);
    // Apply: ",m" should now also toggle highlight on the word under cursor.
    type_keys(&mut editor, ",m");
    assert_eq!(editor.highlights.entries.len(), 1);
    assert_eq!(editor.highlights.entries[0].pattern, r"\bhello\b");
}

// ---- Rendering ------------------------------------------------------------

#[test]
fn highlighted_cell_renders_with_palette_color() {
    use ratatui::style::Color;
    let (mut editor, _f) = open("foo bar foo\n");
    type_keys(&mut editor, ":highlight foo");
    press(&mut editor, KeyCode::Enter);
    let backend = TestBackend::new(40, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    // Row 0, col 0..3 = "foo" → background should be the first palette
    // colour (gold).
    let gold = PALETTE[0];
    let cell_bg = buf[(0, 0)].bg;
    assert_eq!(
        cell_bg, gold,
        "expected gold background on highlighted `f`; got {:?}",
        cell_bg
    );
    // Col 4 = ' ' (space between words), should NOT be highlighted.
    assert_ne!(buf[(4, 0)].bg, gold);
    // Col 5 = 'b' of "bar", not highlighted.
    assert_ne!(buf[(5, 0)].bg, gold);
    // Col 8..11 = second "foo" — highlighted again.
    assert_eq!(buf[(8, 0)].bg, gold);
    // Make sure the colour palette is actually being used (not Reset).
    assert_ne!(gold, Color::Reset);
}

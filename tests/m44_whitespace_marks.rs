//! `[options] highlight_trailing_whitespace` / `highlight_tabs`.
//!
//! When enabled, those cells get a red background. The flags are off
//! by default so existing buffers don't suddenly turn into Christmas
//! lights for users who didn't opt in.

use std::io::Write;

use jvim::config::Config;
use jvim::{ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;
use tempfile::NamedTempFile;

fn open_with(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::with_suffix(".txt").unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn render_bgs(editor: &mut Editor, w: u16, h: u16) -> Vec<Vec<Color>> {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let area = *buf.area();
    let mut rows = Vec::with_capacity(area.height as usize);
    for y in 0..area.height {
        let mut row = Vec::with_capacity(area.width as usize);
        for x in 0..area.width {
            row.push(buf[(x, y)].bg);
        }
        rows.push(row);
    }
    rows
}

// ---- Defaults: nothing is painted red ------------------------------------

#[test]
fn defaults_do_not_highlight_trailing_or_tabs() {
    let (mut editor, _f) = open_with("foo   \n\tbar\n");
    let bgs = render_bgs(&mut editor, 30, 4);
    // No red anywhere in the file content rows.
    for row in &bgs[..2] {
        for c in row {
            assert_ne!(*c, Color::Red, "row has red bg without opting in");
        }
    }
}

// ---- highlight_trailing_whitespace ---------------------------------------

#[test]
fn trailing_whitespace_lights_up_when_enabled() {
    let cfg: Config = toml::from_str(
        r#"
[options]
highlight_trailing_whitespace = true
"#,
    )
    .unwrap();
    let (mut editor, _f) = open_with("foo   \nbar\n");
    editor.apply_config(cfg);
    let bgs = render_bgs(&mut editor, 30, 4);
    // "foo" at cols 0-2 → not red. Cols 3,4,5 (three trailing spaces)
    // should be red.
    let row0 = &bgs[0];
    assert_ne!(row0[0], Color::Red);
    assert_ne!(row0[1], Color::Red);
    assert_ne!(row0[2], Color::Red);
    assert_eq!(row0[3], Color::Red, "col 3 should be red (trailing space)");
    assert_eq!(row0[4], Color::Red);
    assert_eq!(row0[5], Color::Red);
    // "bar" — no trailing space, no red.
    let row1 = &bgs[1];
    assert!(row1.iter().all(|c| *c != Color::Red));
}

#[test]
fn line_with_only_whitespace_is_all_red() {
    let cfg: Config = toml::from_str(
        r#"
[options]
highlight_trailing_whitespace = true
"#,
    )
    .unwrap();
    let (mut editor, _f) = open_with("   \nfoo\n");
    editor.apply_config(cfg);
    let bgs = render_bgs(&mut editor, 30, 4);
    // First three cells of row 0 are spaces — entire run counts as trailing.
    assert_eq!(bgs[0][0], Color::Red);
    assert_eq!(bgs[0][1], Color::Red);
    assert_eq!(bgs[0][2], Color::Red);
}

// ---- highlight_tabs ------------------------------------------------------

#[test]
fn tabs_light_up_when_enabled() {
    let cfg: Config = toml::from_str(
        r#"
[options]
highlight_tabs = true
"#,
    )
    .unwrap();
    let (mut editor, _f) = open_with("\tfoo\n  \tbar\n");
    editor.apply_config(cfg);
    let bgs = render_bgs(&mut editor, 30, 4);
    // Row 0 starts with a tab — it expands to tab_width=4 cells, all red.
    for x in 0..4 {
        assert_eq!(
            bgs[0][x],
            Color::Red,
            "row 0 col {x} should be red (tab cell)"
        );
    }
    // Row 0 col 4 onward is "foo" — not red.
    assert_ne!(bgs[0][4], Color::Red);

    // Row 1 has "  \t" — two spaces (not red unless trailing-ws is on),
    // then tab from col 2 to next tab stop (col 4 inclusive? actually
    // tab from col 2 advances to next multiple of 4 → col 4, so it
    // fills cols 2 and 3). Then "bar".
    assert_ne!(bgs[1][0], Color::Red); // leading space
    assert_ne!(bgs[1][1], Color::Red);
    assert_eq!(bgs[1][2], Color::Red); // tab expansion cells
    assert_eq!(bgs[1][3], Color::Red);
    assert_ne!(bgs[1][4], Color::Red); // 'b'
}

#[test]
fn both_flags_together_highlight_trailing_tab() {
    let cfg: Config = toml::from_str(
        r#"
[options]
highlight_trailing_whitespace = true
highlight_tabs = true
"#,
    )
    .unwrap();
    let (mut editor, _f) = open_with("hi\t\nbye\n");
    editor.apply_config(cfg);
    let bgs = render_bgs(&mut editor, 30, 4);
    // 'h', 'i' at cols 0, 1 — not red. Tab at col 2 → expands.
    assert_ne!(bgs[0][0], Color::Red);
    assert_ne!(bgs[0][1], Color::Red);
    assert_eq!(bgs[0][2], Color::Red); // tab cells
    assert_eq!(bgs[0][3], Color::Red);
}

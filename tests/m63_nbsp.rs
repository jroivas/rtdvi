//! `[options] nbsp_marker` — a configurable visual indicator for non-breaking
//! spaces (U+00A0), which otherwise render identically to a normal space.

use std::io::Write;

use rtdvi::config::Config;
use rtdvi::{ui, Editor};
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

/// (symbol, fg) at each cell of the first row.
fn row0(editor: &mut Editor, w: u16, h: u16) -> Vec<(String, Color)> {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..w).map(|x| (buf[(x, 0u16)].symbol().to_string(), buf[(x, 0u16)].fg)).collect()
}

const NBSP: &str = "\u{a0}";
const NBSP_FG: Color = Color::Rgb(0xe5, 0x9b, 0x4c);
const SPACE_FG: Color = Color::Rgb(0x55, 0x55, 0x55);

/// (symbol, fg, bg) at each cell of the first row.
fn row0_full(editor: &mut Editor, w: u16, h: u16) -> Vec<(String, Color, Color)> {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..w)
        .map(|x| {
            let c = &buf[(x, 0u16)];
            (c.symbol().to_string(), c.fg, c.bg)
        })
        .collect()
}

#[test]
fn nbsp_renders_as_blank_by_default() {
    let (mut editor, _f) = open_with(&format!("a{NBSP}b\n"));
    let cells = row0(&mut editor, 20, 4);
    // No marker configured → the NBSP char is shown as-is (not substituted).
    assert_eq!(cells[1].0, NBSP);
    assert_ne!(cells[1].1, NBSP_FG);
}

#[test]
fn nbsp_marker_substitutes_glyph_and_colour() {
    let cfg: Config = toml::from_str("[options]\nnbsp_marker = \"·\"\n").unwrap();
    let (mut editor, _f) = open_with(&format!("a{NBSP}b\n"));
    editor.apply_config(cfg);
    let cells = row0(&mut editor, 20, 4);
    assert_eq!(cells[0].0, "a");
    // Column 1 (the NBSP) now shows the marker glyph in the NBSP colour.
    assert_eq!(cells[1].0, "·");
    assert_eq!(cells[1].1, NBSP_FG);
    // A normal space is untouched.
    assert_eq!(cells[2].0, "b");

    // A real space stays a space, even with the marker on.
    let (mut editor2, _f2) = open_with("a b\n");
    editor2.apply_config(toml::from_str("[options]\nnbsp_marker = \"·\"\n").unwrap());
    let cells2 = row0(&mut editor2, 20, 4);
    assert_eq!(cells2[1].0, " ");
    assert_ne!(cells2[1].1, NBSP_FG);
}

#[test]
fn space_marker_substitutes_normal_spaces() {
    let (mut editor, _f) = open_with("a b\n");
    editor.apply_config(toml::from_str("[options]\nspace_marker = \"·\"\n").unwrap());
    let cells = row0(&mut editor, 20, 4);
    assert_eq!(cells[0].0, "a");
    assert_eq!(cells[1].0, "·"); // the space
    assert_eq!(cells[1].1, SPACE_FG);
    assert_eq!(cells[2].0, "b");
}

#[test]
fn space_and_nbsp_markers_are_distinct() {
    let (mut editor, _f) = open_with(&format!("a {NBSP}b\n")); // space then NBSP
    editor
        .apply_config(toml::from_str("[options]\nspace_marker = \"·\"\nnbsp_marker = \"␣\"\n").unwrap());
    let cells = row0(&mut editor, 20, 4);
    assert_eq!((cells[1].0.as_str(), cells[1].1), ("·", SPACE_FG)); // normal space
    assert_eq!((cells[2].0.as_str(), cells[2].1), ("␣", NBSP_FG)); // NBSP
}

#[test]
fn trailing_whitespace_red_wins_over_space_marker() {
    let (mut editor, _f) = open_with("ab  \n"); // two trailing spaces
    editor.apply_config(
        toml::from_str("[options]\nspace_marker = \"·\"\nhighlight_trailing_whitespace = true\n")
            .unwrap(),
    );
    let cells = row0_full(&mut editor, 20, 4);
    // Trailing space cell shows the marker glyph but with the trailing-red bg.
    assert_eq!(cells[2].0, "·");
    assert_eq!(cells[2].2, Color::Red);
}

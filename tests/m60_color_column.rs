//! `[options] color_column` — vim's `colorcolumn`. Highlights one or more
//! screen columns with a background ruler. Off by default.

use std::io::Write;

use rtdvi::config::Config;
use rtdvi::{ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;
use tempfile::NamedTempFile;

/// The background colour the ruler paints (mirrors `window_render::cc_bg`).
const CC: Color = Color::Rgb(64, 48, 48);

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

fn cfg_with(opts: &str) -> Config {
    toml::from_str(&format!("[options]\n{opts}\n")).unwrap()
}

#[test]
fn default_off_no_ruler() {
    let (mut editor, _f) = open_with("hello world\n");
    let bgs = render_bgs(&mut editor, 30, 4);
    for row in &bgs {
        for c in row {
            assert_ne!(*c, CC, "no ruler should be painted by default");
        }
    }
}

#[test]
fn absolute_column_paints_one_screen_column() {
    let (mut editor, _f) = open_with("hello world\n");
    editor.apply_config(cfg_with("color_column = \"5\""));
    let bgs = render_bgs(&mut editor, 30, 4);
    // 1-based column 5 → 0-based screen x = 4.
    assert_eq!(bgs[0][4], CC, "column 5 should be highlighted");
    assert_ne!(bgs[0][3], CC, "neighbouring columns untouched");
    assert_ne!(bgs[0][5], CC);
}

#[test]
fn ruler_extends_past_end_of_line() {
    // Short line; the ruler should still show in the empty cells beyond it.
    let (mut editor, _f) = open_with("hi\n");
    editor.apply_config(cfg_with("color_column = \"5\""));
    let bgs = render_bgs(&mut editor, 30, 4);
    assert_eq!(bgs[0][4], CC, "ruler shows past end-of-line on a real row");
}

#[test]
fn ruler_not_drawn_on_tilde_rows() {
    let (mut editor, _f) = open_with("only one line\n");
    editor.apply_config(cfg_with("color_column = \"3\""));
    let bgs = render_bgs(&mut editor, 30, 6);
    // Row 0 is the only real line → painted. Rows 1+ are `~` filler.
    assert_eq!(bgs[0][2], CC);
    assert_ne!(bgs[1][2], CC, "no ruler on the ~ rows below the buffer");
}

#[test]
fn multiple_columns() {
    let (mut editor, _f) = open_with("aaaaaaaaaaaaaaaaaaaa\n");
    editor.apply_config(cfg_with("color_column = \"3,7\""));
    let bgs = render_bgs(&mut editor, 30, 4);
    assert_eq!(bgs[0][2], CC); // column 3
    assert_eq!(bgs[0][6], CC); // column 7
    assert_ne!(bgs[0][4], CC);
}

#[test]
fn set_command_toggles_ruler_at_runtime() {
    use rtdvi::keymap::keys::{Key, KeyCode};
    use rtdvi::mode;
    let (mut editor, _f) = open_with("hello world\n");

    // `:set colorcolumn=5`
    for c in ":set colorcolumn=5".chars() {
        mode::handle_key(&mut editor, Key::char(c));
    }
    mode::handle_key(&mut editor, Key::new(KeyCode::Enter));
    assert_eq!(editor.config.options.color_column, "5");
    assert_eq!(render_bgs(&mut editor, 30, 4)[0][4], CC);

    // `:set nocolorcolumn` clears it.
    for c in ":set nocolorcolumn".chars() {
        mode::handle_key(&mut editor, Key::char(c));
    }
    mode::handle_key(&mut editor, Key::new(KeyCode::Enter));
    assert!(editor.config.options.color_column.is_empty());
    assert_ne!(render_bgs(&mut editor, 30, 4)[0][4], CC);
}

#[test]
fn relative_to_textwidth() {
    let (mut editor, _f) = open_with("xxxxxxxxxxxxxxxxxxxx\n");
    // textwidth 10, "+1" → column 11 → screen x 10.
    editor.apply_config(cfg_with("textwidth = 10\ncolor_column = \"+1\""));
    let bgs = render_bgs(&mut editor, 40, 4);
    assert_eq!(bgs[0][10], CC, "+1 over textwidth 10 → column 11 (x=10)");
    assert_ne!(bgs[0][9], CC);
}

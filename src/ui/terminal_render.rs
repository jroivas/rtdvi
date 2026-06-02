//! Render an embedded terminal's [`vt100`] screen grid into a window rect.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::terminal::Terminal;

pub fn render(term: &Terminal, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Ok(parser) = term.parser.lock() else {
        return;
    };
    let screen = parser.screen();
    let (grid_rows, grid_cols) = screen.size();
    let rows = area.height.min(grid_rows);
    let cols = area.width.min(grid_cols);

    let mut lines: Vec<Line> = Vec::with_capacity(area.height as usize);
    for row in 0..rows {
        let mut spans: Vec<Span> = Vec::new();
        let mut cur_text = String::new();
        let mut cur_style = Style::default();
        let mut col = 0u16;
        while col < cols {
            let cell = screen.cell(row, col);
            // A wide character occupies two grid columns; the second column is
            // a continuation cell with no contents of its own, so skip it (the
            // glyph from the first cell already advances the terminal cursor).
            if let Some(c) = cell {
                if c.is_wide_continuation() {
                    col += 1;
                    continue;
                }
            }
            let style = cell.map(cell_style).unwrap_or_default();
            let text = match cell {
                Some(c) if c.has_contents() => c.contents(),
                _ => " ".to_string(),
            };
            if style != cur_style && !cur_text.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut cur_text), cur_style));
            }
            cur_style = style;
            cur_text.push_str(&text);
            col += 1;
        }
        if !cur_text.is_empty() {
            spans.push(Span::styled(cur_text, cur_style));
        }
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// Place the hardware cursor at the terminal's reported cursor position.
pub fn set_cursor(term: &Terminal, frame: &mut Frame, area: Rect) {
    let Ok(parser) = term.parser.lock() else {
        return;
    };
    let screen = parser.screen();
    if screen.hide_cursor() {
        return;
    }
    let (row, col) = screen.cursor_position();
    if row >= area.height || col >= area.width {
        return;
    }
    frame.set_cursor_position((area.x + col, area.y + row));
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut fg = conv_color(cell.fgcolor());
    let mut bg = conv_color(cell.bgcolor());
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    let mut style = Style::default();
    if let Some(c) = fg {
        style = style.fg(c);
    }
    if let Some(c) = bg {
        style = style.bg(c);
    }
    if cell.bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.italic() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

fn conv_color(c: vt100::Color) -> Option<Color> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(Color::Indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

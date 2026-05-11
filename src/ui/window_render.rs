//! Render a single window's buffer slice, plus selection highlight.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::cursor::Selection;
use crate::text::width as twidth;
use crate::window::Window;
use crate::Editor;

pub fn render(editor: &Editor, window: &Window, frame: &mut Frame, area: Rect) {
    let Some(buffer) = editor.buffers.get(&window.buffer) else {
        return;
    };
    let tab_width = editor.config.options.tab_width;

    let height = area.height as usize;
    let mut lines = Vec::with_capacity(height);
    let show_number = editor.config.options.number;
    let gutter = if show_number {
        let max_line = window.top_line + height;
        let digits = num_digits(max_line.max(1));
        digits + 1
    } else {
        0
    };
    let text_width = area.width.saturating_sub(gutter as u16) as usize;

    let sel_style = Style::default()
        .bg(Color::Rgb(60, 80, 110))
        .add_modifier(Modifier::REVERSED);

    for row in 0..height {
        let line_idx = window.top_line + row;
        if line_idx >= buffer.line_count() {
            lines.push(Line::from(Span::styled(
                "~",
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }
        let line_text = buffer.line_string(line_idx);
        let mut spans = Vec::new();
        if show_number {
            let n = format!(
                "{:>width$} ",
                line_idx + 1,
                width = gutter.saturating_sub(1).max(1)
            );
            spans.push(Span::styled(n, Style::default().fg(Color::DarkGray)));
        }
        let (sel_start_col, sel_end_col) = selection_cols_for_row(window, buffer, line_idx, tab_width);
        spans.extend(line_spans(
            &line_text,
            window.left_col,
            text_width,
            tab_width,
            sel_start_col,
            sel_end_col,
            sel_style,
        ));
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// Return `(start_col, end_col)` of the selection within `line_idx`, in
/// display columns. `end_col` is *exclusive*. `None` means no selection on
/// this row, encoded as `(0, 0)` => no highlight.
fn selection_cols_for_row(
    window: &Window,
    buffer: &crate::buffer::Buffer,
    line_idx: usize,
    tab_width: usize,
) -> (usize, usize) {
    let line = buffer.line_string(line_idx);
    let line_width = twidth::line_display_width(&line, tab_width);
    match window.selection {
        Selection::None => (0, 0),
        Selection::Char { anchor } => {
            let cur = window.cursor;
            let (lo, hi) = if (anchor.row, anchor.col) <= (cur.row, cur.col) {
                (anchor, cur)
            } else {
                (cur, anchor)
            };
            if line_idx < lo.row || line_idx > hi.row {
                (0, 0)
            } else if lo.row == hi.row {
                (lo.col, hi.col + 1)
            } else if line_idx == lo.row {
                (lo.col, line_width + 1)
            } else if line_idx == hi.row {
                (0, hi.col + 1)
            } else {
                (0, line_width + 1)
            }
        }
        Selection::Line { anchor_row } => {
            let (lo, hi) = if anchor_row <= window.cursor.row {
                (anchor_row, window.cursor.row)
            } else {
                (window.cursor.row, anchor_row)
            };
            if line_idx < lo || line_idx > hi {
                (0, 0)
            } else {
                (0, line_width.max(1))
            }
        }
        Selection::Block { anchor } => {
            let cur = window.cursor;
            let (top, bot) = (anchor.row.min(cur.row), anchor.row.max(cur.row));
            let (left, right) = (anchor.col.min(cur.col), anchor.col.max(cur.col));
            if line_idx < top || line_idx > bot {
                (0, 0)
            } else {
                (left, right + 1)
            }
        }
    }
}

pub fn set_cursor(editor: &Editor, window: &Window, frame: &mut Frame, area: Rect) {
    let gutter = if editor.config.options.number {
        let max_line = window.top_line + area.height as usize;
        num_digits(max_line.max(1)) + 1
    } else {
        0
    };
    let screen_row = window.cursor.row.saturating_sub(window.top_line);
    let screen_col = window.cursor.col.saturating_sub(window.left_col);
    if screen_row >= area.height as usize {
        return;
    }
    let x = area.x + gutter as u16 + screen_col as u16;
    let y = area.y + screen_row as u16;
    frame.set_cursor_position((x, y));
}

/// Build one or more `Span`s for a single text line, splitting at the
/// selection boundary so the selected cells get reverse-video.
fn line_spans(
    line: &str,
    left_col: usize,
    width: usize,
    tab_width: usize,
    sel_start_col: usize,
    sel_end_col: usize,
    sel_style: Style,
) -> Vec<Span<'static>> {
    if width == 0 {
        return vec![];
    }
    let mut col = 0usize;
    let mut emitted = 0usize;
    // Cells emitted, paired with whether each cell is currently inside the selection.
    let mut current_text = String::new();
    let mut current_selected = false;
    let mut spans: Vec<Span<'static>> = Vec::new();

    let flush = |spans: &mut Vec<Span<'static>>, text: &mut String, selected: bool| {
        if !text.is_empty() {
            let style = if selected {
                sel_style
            } else {
                Style::default()
            };
            spans.push(Span::styled(std::mem::take(text), style));
        }
    };

    for (_b, g, _gc, w) in twidth::graphemes_with_cols(line, tab_width) {
        if col + w <= left_col {
            col += w;
            continue;
        }
        if emitted >= width {
            break;
        }
        let cells_in_sel = col >= sel_start_col && col < sel_end_col;
        if cells_in_sel != current_selected {
            flush(&mut spans, &mut current_text, current_selected);
            current_selected = cells_in_sel;
        }
        if g == "\t" {
            let start_skip = left_col.saturating_sub(col);
            let visible = w.saturating_sub(start_skip);
            let take = visible.min(width - emitted);
            for _ in 0..take {
                current_text.push(' ');
            }
            emitted += take;
        } else {
            if emitted + w > width {
                break;
            }
            current_text.push_str(g);
            emitted += w;
        }
        col += w;
    }
    flush(&mut spans, &mut current_text, current_selected);
    spans
}

fn num_digits(n: usize) -> usize {
    if n == 0 {
        1
    } else {
        (n as f64).log10().floor() as usize + 1
    }
}

//! Render a single window's buffer slice.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::buffer::Buffer;
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
        // Width of the largest line number we might show + 1 separator.
        let max_line = window.top_line + height;
        let digits = num_digits(max_line.max(1));
        digits + 1
    } else {
        0
    };
    let text_width = area.width.saturating_sub(gutter as u16) as usize;

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
        // Expand tabs and clip horizontally to the visible window.
        spans.push(expand_line_span(&line_text, window.left_col, text_width, tab_width));
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), area);

    // Render selection / cursor highlight in subsequent milestones; for M1
    // the cursor position is set via `set_cursor`.
    let _ = Modifier::REVERSED;
    let _ = buffer_unused(buffer);
}

#[inline]
fn buffer_unused(_b: &Buffer) {}

pub fn set_cursor(editor: &Editor, window: &Window, frame: &mut Frame, area: Rect) {
    let tab_width = editor.config.options.tab_width;
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
    let _ = tab_width;
    let x = area.x + gutter as u16 + screen_col as u16;
    let y = area.y + screen_row as u16;
    frame.set_cursor_position((x, y));
}

fn expand_line_span(line: &str, left_col: usize, width: usize, tab_width: usize) -> Span<'static> {
    if width == 0 {
        return Span::raw("");
    }
    // Walk graphemes, expanding tabs to spaces, skip until `left_col`, then
    // collect up to `width` display cells.
    let mut col = 0usize;
    let mut out = String::new();
    let mut emitted = 0usize;
    for (_b, g, _gc, w) in twidth::graphemes_with_cols(line, tab_width) {
        if col + w <= left_col {
            col += w;
            continue;
        }
        if emitted >= width {
            break;
        }
        if g == "\t" {
            // The first cell of the tab might be inside the left scroll region;
            // emit only the visible portion.
            let start_skip = left_col.saturating_sub(col);
            let visible = w.saturating_sub(start_skip);
            let take = visible.min(width - emitted);
            for _ in 0..take {
                out.push(' ');
            }
            emitted += take;
        } else {
            if emitted + w > width {
                break;
            }
            out.push_str(g);
            emitted += w;
        }
        col += w;
    }
    Span::raw(out)
}

fn num_digits(n: usize) -> usize {
    if n == 0 {
        1
    } else {
        (n as f64).log10().floor() as usize + 1
    }
}

//! Renders a "render buffer" — a non-editable buffer whose content is
//! pre-styled `ratatui` lines (built by a plugin via the render ABI). Unlike
//! `window_render`, there is no syntax/selection pipeline: the stored lines are
//! painted directly, scrolled by `window.top_line` / `window.left_col`.
//!
//! Navigation is exactly like a normal buffer — `hjkl`, `w/b/e`, `0/$`,
//! `gg/G`, `Ctrl-d/u`, etc. all move the (visible) cursor and the view follows
//! via `Window::scroll_into_view`. The buffer is just read-only and styled.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::text::width as twidth;
use crate::window::Window;
use crate::Editor;

pub fn render(editor: &Editor, window: &Window, frame: &mut Frame, area: Rect) {
    let Some(buffer) = editor.buffers.get(&window.buffer) else { return };
    let Some(content) = buffer.render_content() else { return };
    let src = &content.lines;
    let height = area.height as usize;

    // The link under the cursor, if any — highlight its row's link spans.
    let cursor_on_link = content
        .links
        .iter()
        .any(|l| l.line == window.cursor.row && (l.start_col..l.end_col).contains(&window.cursor.col));

    let mut out: Vec<Line> = Vec::with_capacity(height);
    for row in 0..height {
        let idx = window.top_line + row;
        match src.get(idx) {
            Some(line) => {
                let line = if cursor_on_link && idx == window.cursor.row {
                    highlight_links_on(line)
                } else {
                    line.clone()
                };
                // Horizontal scroll: drop the first `left_col` display columns;
                // the right edge is clipped by the Paragraph widget.
                out.push(clip_left(&line, window.left_col));
            }
            None => out.push(Line::from(Span::styled(
                "~",
                Style::default().fg(Color::DarkGray),
            ))),
        }
    }
    frame.render_widget(Paragraph::new(out), area);
}

/// Place the cursor for a render buffer: same row/col math as a normal buffer
/// (honoring scroll), but with no gutter since render buffers draw none.
pub fn set_cursor(window: &Window, frame: &mut Frame, area: Rect) {
    let sy = window.cursor.row.saturating_sub(window.top_line);
    let sx = window.cursor.col.saturating_sub(window.left_col);
    if sy < area.height as usize && sx < area.width as usize {
        frame.set_cursor_position((area.x + sx as u16, area.y + sy as u16));
    }
}

/// Drop the leading `left_col` display columns from a styled line, preserving
/// per-span styles. A character straddling the cut is dropped (kept simple).
fn clip_left(line: &Line<'static>, left_col: usize) -> Line<'static> {
    if left_col == 0 {
        return line.clone();
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut col = 0usize;
    for sp in &line.spans {
        let mut kept = String::new();
        for ch in sp.content.chars() {
            let g = ch.to_string();
            let w = twidth::grapheme_width(&g, col, 1).max(1);
            if col >= left_col {
                kept.push(ch);
            }
            col += w;
        }
        if !kept.is_empty() {
            spans.push(Span::styled(kept, sp.style));
        }
    }
    Line::from(spans)
}

/// Returns a copy of `line` with its underlined (link) spans reverse-video, so
/// the focused link stands out. Non-link spans are untouched.
fn highlight_links_on(line: &Line<'static>) -> Line<'static> {
    let spans: Vec<Span<'static>> = line
        .spans
        .iter()
        .map(|s| {
            if s.style.add_modifier.contains(Modifier::UNDERLINED) {
                Span::styled(s.content.clone(), s.style.add_modifier(Modifier::REVERSED))
            } else {
                s.clone()
            }
        })
        .collect();
    Line::from(spans)
}

//! Per-window statusline.
//!
//! Each window in a tab gets its own status row painted on the bottom of
//! its rect (vim's `laststatus=2` behaviour). The active window's
//! statusline is highlighted distinctly from inactive ones so it's
//! obvious where focus is in a multi-split layout.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::window::Window;
use crate::Editor;

pub fn render_for(
    editor: &Editor,
    window: &Window,
    is_active: bool,
    frame: &mut Frame,
    area: Rect,
) {
    let buffer = editor.buffers.get(&window.buffer);
    let mode_str = if is_active { editor.mode.short_name() } else { "" };
    let file = match buffer {
        Some(b) => b
            .path()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "[No Name]".to_string()),
        None => "[No Name]".to_string(),
    };
    let dirty = buffer.map(|b| b.is_dirty()).unwrap_or(false);
    let pos = format!(" {}:{} ", window.cursor.row + 1, window.cursor.col + 1);

    // Colour palette: the active window gets a brighter background, the
    // inactive ones get a muted one — same trick vim uses with the
    // StatusLine vs StatusLineNC highlight groups.
    let (bar_bg, bar_fg) = if is_active {
        (Color::White, Color::Black)
    } else {
        (Color::DarkGray, Color::Gray)
    };
    let bar_style = Style::default()
        .fg(bar_fg)
        .bg(bar_bg)
        .add_modifier(if is_active { Modifier::BOLD } else { Modifier::empty() });

    let mut spans = Vec::new();
    if is_active {
        spans.push(Span::styled(
            format!(" {mode_str} "),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        format!(" {file}{} ", if dirty { " [+]" } else { "" }),
        bar_style,
    ));
    // Right-side filler + cursor position.
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let total = area.width as usize;
    let pos_len = pos.chars().count();
    if total > used + pos_len {
        spans.push(Span::styled(" ".repeat(total - used - pos_len), bar_style));
    }
    spans.push(Span::styled(pos, bar_style));

    frame.render_widget(Paragraph::new(Line::from(spans)).style(bar_style), area);
}

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
    // Full path as the buffer holds it — relative when opened relatively,
    // absolute when opened absolutely. The basename was easier to read
    // but useless once you have several files with the same name in a
    // tree (e.g. multiple `mod.rs`).
    let file = match buffer {
        Some(b) => b
            .path()
            .map(|p| p.display().to_string())
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
    let mode_span_len = if is_active { mode_str.chars().count() + 2 } else { 0 };
    let dirty_marker = if dirty { " [+]" } else { "" };
    let pos_len = pos.chars().count();
    let total = area.width as usize;
    // Two surrounding spaces around the path.
    let file_chrome = 2 + dirty_marker.chars().count();
    let path_budget = total
        .saturating_sub(mode_span_len)
        .saturating_sub(pos_len)
        .saturating_sub(file_chrome);
    let truncated_file = truncate_left(&file, path_budget);

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
        format!(" {truncated_file}{dirty_marker} "),
        bar_style,
    ));
    // Right-side filler so the position sticks to the right edge.
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    if total > used + pos_len {
        spans.push(Span::styled(" ".repeat(total - used - pos_len), bar_style));
    }
    spans.push(Span::styled(pos, bar_style));

    frame.render_widget(Paragraph::new(Line::from(spans)).style(bar_style), area);
}

/// Drop characters from the LEFT of `s` until it fits in `max` columns,
/// prefixing `…` when truncation happens. Keeps the basename visible
/// in narrow split windows.
fn truncate_left(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".chars().take(max).collect();
    }
    // Skip (n - (max - 1)) leading chars, prefix `…`.
    let skip = n - (max - 1);
    let tail: String = s.chars().skip(skip).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_left_short_path_passes_through() {
        assert_eq!(truncate_left("foo.c", 10), "foo.c");
    }

    #[test]
    fn truncate_left_long_path_keeps_tail() {
        let path = "a/very/long/path/to/file.rs";
        let out = truncate_left(path, 12);
        assert_eq!(out.chars().count(), 12);
        assert!(out.starts_with('…'));
        assert!(out.ends_with("file.rs"));
    }

    #[test]
    fn truncate_left_max_one_returns_marker() {
        assert_eq!(truncate_left("hello", 1), "…");
    }
}

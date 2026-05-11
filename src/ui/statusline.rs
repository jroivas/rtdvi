//! One-line statusline at the bottom of the window area.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::Editor;

pub fn render(editor: &Editor, frame: &mut Frame, area: Rect) {
    let mode_str = editor.mode.short_name();
    let (file, dirty) = match editor.active_window().and_then(|w| editor.buffers.get(&w.buffer)) {
        Some(b) => (
            b.path()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "[No Name]".to_string()),
            b.is_dirty(),
        ),
        None => ("[No Name]".to_string(), false),
    };
    let pos = editor
        .active_window()
        .map(|w| format!(" {}:{} ", w.cursor.row + 1, w.cursor.col + 1))
        .unwrap_or_default();
    let style = Style::default()
        .fg(Color::Black)
        .bg(Color::White)
        .add_modifier(Modifier::BOLD);
    let left = format!(" {mode_str} ");
    let middle = format!(" {}{} ", file, if dirty { " [+]" } else { "" });
    let line = Line::from(vec![
        Span::styled(left, style),
        Span::raw(middle),
        Span::raw(" "),
        Span::raw(pos),
    ]);
    let para = Paragraph::new(line).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(para, area);
}

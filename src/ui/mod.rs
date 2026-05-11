//! TUI rendering. Top-level `render` takes the editor and a ratatui `Frame`;
//! sub-modules render specific zones.

pub mod cmdline;
pub mod statusline;
pub mod window_render;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::mode::ModeId;
use crate::Editor;

pub fn render(editor: &mut Editor, frame: &mut Frame) {
    let area = frame.area();
    let show_tabs = editor.tabs.len() > 1;
    let constraints: Vec<Constraint> = if show_tabs {
        vec![
            Constraint::Length(1), // tabline
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    let (tab_area, window_area, status_area, cmd_area) = if show_tabs {
        (Some(chunks[0]), chunks[1], chunks[2], chunks[3])
    } else {
        (None, chunks[0], chunks[1], chunks[2])
    };

    if let Some(rect) = tab_area {
        render_tabline(editor, frame, rect);
    }
    render_windows(editor, frame, window_area);
    statusline::render(editor, frame, status_area);
    cmdline::render(editor, frame, cmd_area);
}

fn render_tabline(editor: &Editor, frame: &mut Frame, area: Rect) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;
    let mut spans = Vec::new();
    for (idx, tab) in editor.tabs.iter().enumerate() {
        let name = editor
            .windows
            .get(&tab.active)
            .and_then(|w| editor.buffers.get(&w.buffer))
            .and_then(|b| b.path())
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "[No Name]".to_string());
        let label = format!(" {} {name} ", idx + 1);
        let style = if idx == editor.active_tab {
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_windows(editor: &mut Editor, frame: &mut Frame, area: Rect) {
    let Some(tab) = editor.tabs.get(editor.active_tab) else {
        return;
    };
    let active_win = tab.active;
    let layout = tab.tree.layout(area);

    // First pass: update each window's last-known viewport + scroll.
    for (wid, rect) in &layout {
        if let Some(w) = editor.windows.get_mut(wid) {
            w.viewport_h = rect.height;
            w.viewport_w = rect.width;
            w.scroll_into_view(0);
        }
    }

    // Second pass: paint.
    for (wid, rect) in &layout {
        if let Some(window) = editor.windows.get(wid) {
            window_render::render(editor, window, frame, *rect);
        }
    }

    // Active-window cursor.
    if editor.mode != ModeId::Command && editor.mode != ModeId::Search {
        if let Some((_, rect)) = layout.iter().find(|(w, _)| *w == active_win) {
            if let Some(window) = editor.windows.get(&active_win) {
                window_render::set_cursor(editor, window, frame, *rect);
            }
        }
    }
}

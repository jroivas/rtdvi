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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // window area
            Constraint::Length(1), // status line
            Constraint::Length(1), // cmdline / message
        ])
        .split(area);
    let window_area = chunks[0];
    let status_area = chunks[1];
    let cmd_area = chunks[2];

    render_windows(editor, frame, window_area);
    statusline::render(editor, frame, status_area);
    cmdline::render(editor, frame, cmd_area);
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

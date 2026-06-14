//! TUI rendering. Top-level `render` takes the editor and a ratatui `Frame`;
//! sub-modules render specific zones.

pub mod cmdline;
#[cfg(feature = "render-buffer")]
pub mod render_buffer;
pub mod statusline;
pub mod terminal_render;
pub mod window_render;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::mode::ModeId;
use crate::Editor;

/// True when a window's buffer is a non-editable render buffer.
fn is_render_buffer(editor: &Editor, window: &crate::window::Window) -> bool {
    editor
        .buffers
        .get(&window.buffer)
        .map(|b| !b.is_editable())
        .unwrap_or(false)
}

pub fn render(editor: &mut Editor, frame: &mut Frame) {
    let area = frame.area();
    let show_tabs = editor.tabs.len() > 1;
    let cmd_h = cmdline::height(editor);
    // Each window owns its own statusline (bottom row of its rect), so the
    // top-level layout no longer reserves a global statusline area — just
    // tabline (optional), window area, and cmdline.
    let constraints: Vec<Constraint> = if show_tabs {
        vec![
            Constraint::Length(1),     // tabline
            Constraint::Min(1),        // windows (incl. per-window statuslines)
            Constraint::Length(cmd_h), // cmdline
        ]
    } else {
        vec![Constraint::Min(1), Constraint::Length(cmd_h)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    let (tab_area, window_area, cmd_area) = if show_tabs {
        (Some(chunks[0]), chunks[1], chunks[2])
    } else {
        (None, chunks[0], chunks[1])
    };

    if let Some(rect) = tab_area {
        render_tabline(editor, frame, rect);
    }
    render_windows(editor, frame, window_area);
    cmdline::render(editor, frame, cmd_area);

    // Overlay the completion popup on top of everything else.
    if let Some(comp) = editor.command_line.completion.as_ref() {
        if comp.popup_visible && !comp.matches.is_empty() {
            render_completion_popup(frame, &comp.matches, comp.index, cmd_area);
        }
    }
    // `:ff` interactive popup. Same shape as the completion popup but
    // driven by the live fuzzy-search state rather than Tab cycling.
    if let Some(ff) = editor.fzf_state.as_ref() {
        if !ff.matches.is_empty() {
            render_completion_popup(frame, &ff.matches, ff.selected, cmd_area);
        }
    }
    // LSP location picker (gr / gd with multiple results).
    if let Some(picker) = editor.lsp_picker.as_ref() {
        if !picker.labels.is_empty() {
            render_completion_popup(frame, &picker.labels, picker.selected, cmd_area);
        }
    }
}

fn render_completion_popup(
    frame: &mut Frame,
    matches: &[String],
    selected: usize,
    cmd_area: Rect,
) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::Line;
    use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

    let max_height: u16 = 10;
    let item_count = matches.len() as u16;
    // +2 for top/bottom borders. Don't reach above the screen.
    let height = (item_count + 2).min(max_height + 2).min(cmd_area.y);
    if height < 3 {
        return; // not enough vertical room
    }
    // Width = widest match + 2 for borders. Cap to screen width.
    let widest = matches.iter().map(|s| s.chars().count()).max().unwrap_or(0) as u16;
    let width = (widest + 2).max(20).min(cmd_area.width);
    let x = cmd_area.x;
    let y = cmd_area.y.saturating_sub(height);
    let rect = Rect { x, y, width, height };

    // Wipe whatever was under the popup so nothing bleeds through.
    frame.render_widget(Clear, rect);

    let items: Vec<ListItem> = matches
        .iter()
        .map(|m| ListItem::new(Line::from(m.clone())))
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    state.select(Some(selected));
    frame.render_stateful_widget(list, rect, &mut state);
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
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Text};
    use ratatui::widgets::Paragraph;

    // Remember the region size so `:resize` can map a row/col delta to a ratio.
    editor.last_window_area = (area.width, area.height);

    let Some(tab) = editor.tabs.get(editor.active_tab) else {
        return;
    };
    let active_win = tab.active;
    let layout = tab.tree.layout(area);
    let borders = tab.tree.borders(area);

    // Each rect from the split tree is divided into:
    //   content area (height - 1 rows) → the buffer view
    //   status line (1 row)            → per-window statusline
    let split_for = |r: Rect| -> (Rect, Rect) {
        if r.height <= 1 {
            // Pathological — give status priority over a 1-row content area.
            (
                Rect { height: 0, ..r },
                Rect { y: r.y, height: r.height, ..r },
            )
        } else {
            (
                Rect {
                    height: r.height - 1,
                    ..r
                },
                Rect {
                    y: r.y + r.height - 1,
                    height: 1,
                    ..r
                },
            )
        }
    };

    // First pass: update each window's last-known viewport + scroll, using
    // the *content* rect (status row excluded). Terminal windows instead
    // resize their PTY/grid to the exact content rectangle.
    for (wid, rect) in &layout {
        let (content_rect, _) = split_for(*rect);
        let buf = editor.windows.get(wid).map(|w| w.buffer);
        if let Some(term) = buf.and_then(|b| editor.terminals.get_mut(&b)) {
            term.resize(content_rect.width, content_rect.height);
        } else if let Some(w) = editor.windows.get_mut(wid) {
            w.viewport_h = content_rect.height;
            w.viewport_w = content_rect.width;
            w.scroll_into_view(0);
        }
    }

    // Second pass: paint each window's content and its own statusline.
    for (wid, rect) in &layout {
        let (content_rect, status_rect) = split_for(*rect);
        if let Some(window) = editor.windows.get(wid) {
            if let Some(term) = editor.terminals.get(&window.buffer) {
                terminal_render::render(term, frame, content_rect);
            } else if is_render_buffer(editor, window) {
                #[cfg(feature = "render-buffer")]
                render_buffer::render(editor, window, frame, content_rect);
            } else {
                window_render::render(editor, window, frame, content_rect);
            }
            statusline::render_for(editor, window, *wid == active_win, frame, status_rect);
        }
    }

    // Active-window cursor (in the active window's *content* rect).
    if editor.mode != ModeId::Command && editor.mode != ModeId::Search {
        if let Some((_, rect)) = layout.iter().find(|(w, _)| *w == active_win) {
            let (content_rect, _) = split_for(*rect);
            if let Some(window) = editor.windows.get(&active_win) {
                if let Some(term) = editor.terminals.get(&window.buffer) {
                    terminal_render::set_cursor(term, frame, content_rect);
                } else if is_render_buffer(editor, window) {
                    #[cfg(feature = "render-buffer")]
                    render_buffer::set_cursor(window, frame, content_rect);
                } else {
                    window_render::set_cursor(editor, window, frame, content_rect);
                }
            }
        }
    }

    // Draw vertical split borders on top of everything else.
    let border_style = Style::default().fg(Color::DarkGray);
    for border in borders {
        if border.width == 0 || border.height == 0 {
            continue;
        }
        let lines: Text = (0..border.height)
            .map(|_| Line::from("│"))
            .collect::<Vec<_>>()
            .into();
        frame.render_widget(Paragraph::new(lines).style(border_style), border);
    }
}

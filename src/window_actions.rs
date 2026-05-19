//! Actions that manipulate windows: split, close, focus-nav.

use std::sync::Arc;

use ratatui::layout::Rect;

use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::ModeId;
use crate::window::{SplitAxis, Window};
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("split_horizontal", Arc::new(|ed| split_active(ed, SplitAxis::Horizontal)));
    reg.register("split_vertical", Arc::new(|ed| split_active(ed, SplitAxis::Vertical)));
    reg.register("close_window", Arc::new(|ed| {
        if let Err(msg) = close_active(ed, false) {
            ed.status_message = Some(msg);
        }
    }));
    reg.register("focus_left", Arc::new(|ed| focus_direction(ed, Dir::Left)));
    reg.register("focus_right", Arc::new(|ed| focus_direction(ed, Dir::Right)));
    reg.register("focus_up", Arc::new(|ed| focus_direction(ed, Dir::Up)));
    reg.register("focus_down", Arc::new(|ed| focus_direction(ed, Dir::Down)));
    reg.register("focus_next", Arc::new(focus_next));
    reg.register("equalize_splits", Arc::new(equalize_splits));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    use ModeId::Normal;
    // Each "command" has both forms: release-Ctrl-then-letter (`<C-w>l`)
    // and hold-Ctrl-through-both-keys (`<C-w><C-l>`). Vim does the same.
    let bindings = [
        ("<C-w>h", "focus_left"),
        ("<C-w><C-h>", "focus_left"),
        ("<C-w>j", "focus_down"),
        ("<C-w><C-j>", "focus_down"),
        ("<C-w>k", "focus_up"),
        ("<C-w><C-k>", "focus_up"),
        ("<C-w>l", "focus_right"),
        ("<C-w><C-l>", "focus_right"),
        ("<C-w>w", "focus_next"),
        ("<C-w><C-w>", "focus_next"),
        ("<C-w>s", "split_horizontal"),
        ("<C-w><C-s>", "split_horizontal"),
        ("<C-w>v", "split_vertical"),
        ("<C-w><C-v>", "split_vertical"),
        ("<C-w>c", "close_window"),
        ("<C-w><C-c>", "close_window"),
        ("<C-w>=", "equalize_splits"),
    ];
    for (seq, action) in bindings {
        reg.bind(Normal, seq, Action::Builtin(action)).unwrap();
    }
    // Vim-style tab navigation: gt -> next tab, gT -> previous tab.
    reg.bind(Normal, "gt", Action::Ex("tabnext".into())).unwrap();
    reg.bind(Normal, "gT", Action::Ex("tabprev".into())).unwrap();
}

pub fn split_active(editor: &mut Editor, axis: SplitAxis) {
    let Some(tab_idx) = (editor.active_tab < editor.tabs.len()).then_some(editor.active_tab) else {
        return;
    };
    let active = editor.tabs[tab_idx].active;
    let (buffer, cursor, top_line, left_col) = match editor.windows.get(&active) {
        Some(w) => (w.buffer, w.cursor, w.top_line, w.left_col),
        None => return,
    };

    let new_id = editor.new_window_id();
    let mut new_win = Window::new(new_id, buffer);
    new_win.cursor = cursor;
    new_win.top_line = top_line;
    new_win.left_col = left_col;
    editor.windows.insert(new_id, new_win);

    let tab = &mut editor.tabs[tab_idx];
    let old_tree = std::mem::replace(&mut tab.tree, crate::window::SplitTree::Leaf(active));
    tab.tree = old_tree.split_leaf(active, axis, new_id);
    tab.active = new_id;
}

pub fn close_active(editor: &mut Editor, bang: bool) -> Result<(), String> {
    let Some(tab_idx) = (editor.active_tab < editor.tabs.len()).then_some(editor.active_tab) else {
        return Ok(());
    };
    let active = editor.tabs[tab_idx].active;

    if !bang {
        let dirty = editor
            .windows
            .get(&active)
            .and_then(|w| editor.buffers.get(&w.buffer))
            .map(|b| b.is_dirty())
            .unwrap_or(false);
        if dirty {
            return Err("E37: No write since last change (add ! to override)".into());
        }
    }

    let tab = &mut editor.tabs[tab_idx];
    let old_tree = std::mem::replace(&mut tab.tree, crate::window::SplitTree::Leaf(active));
    match old_tree.remove_leaf(active) {
        Some(new_tree) => {
            let first = new_tree.windows().first().copied();
            tab.tree = new_tree;
            if let Some(w) = first {
                tab.active = w;
            }
            editor.windows.remove(&active);
        }
        None => {
            editor.tabs.remove(tab_idx);
            editor.windows.remove(&active);
            if editor.tabs.is_empty() {
                editor.should_quit = true;
            } else {
                editor.active_tab = editor.active_tab.min(editor.tabs.len() - 1);
            }
        }
    }
    Ok(())
}

#[derive(Copy, Clone)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Move focus exactly one column / row in `dir`. Two-step algorithm:
///   1. Find the candidate column (for L/R) or row (for U/D) whose edge is
///      immediately adjacent to the active window — never skip past closer
///      columns to land in a further one.
///   2. Within that adjacent strip, pick the window whose perpendicular
///      range contains the cursor's screen position. On a boundary line,
///      pick the rect above (for L/R) or to the left (for U/D), matching
///      vim's `<C-w>` behaviour.
fn focus_direction(editor: &mut Editor, dir: Dir) {
    let Some(tab) = editor.tabs.get(editor.active_tab) else {
        return;
    };
    let active = tab.active;
    // Synthetic layout: only relative positions matter for navigation, so
    // a fixed 1000×1000 canvas keeps the math integer-friendly.
    let layout = tab
        .tree
        .layout(Rect { x: 0, y: 0, width: 1000, height: 1000 });
    let Some(active_rect) = layout.iter().find(|(w, _)| *w == active).map(|(_, r)| *r) else {
        return;
    };

    // Map cursor screen position into synthetic coordinates so we can pick
    // the right target row/column.
    let (x_ref, y_ref) = cursor_synthetic_position(editor, active, active_rect);

    // Step 1: keep only candidates strictly in `dir` from the active rect.
    let candidates: Vec<(crate::window::WindowId, Rect)> = layout
        .iter()
        .filter(|(w, r)| {
            *w != active
                && match dir {
                    Dir::Left => r.x + r.width <= active_rect.x,
                    Dir::Right => r.x >= active_rect.x + active_rect.width,
                    Dir::Up => r.y + r.height <= active_rect.y,
                    Dir::Down => r.y >= active_rect.y + active_rect.height,
                }
        })
        .copied()
        .collect();
    if candidates.is_empty() {
        return;
    }

    // Step 2: pick the edge of the *nearest* column/row in `dir`.
    let nearest_edge: u16 = match dir {
        Dir::Right => candidates.iter().map(|(_, r)| r.x).min().unwrap(),
        Dir::Left => candidates.iter().map(|(_, r)| r.x + r.width).max().unwrap(),
        Dir::Up => candidates.iter().map(|(_, r)| r.y + r.height).max().unwrap(),
        Dir::Down => candidates.iter().map(|(_, r)| r.y).min().unwrap(),
    };
    let same_strip: Vec<(crate::window::WindowId, Rect)> = candidates
        .into_iter()
        .filter(|(_, r)| match dir {
            Dir::Right => r.x == nearest_edge,
            Dir::Left => r.x + r.width == nearest_edge,
            Dir::Up => r.y + r.height == nearest_edge,
            Dir::Down => r.y == nearest_edge,
        })
        .collect();
    if same_strip.is_empty() {
        return;
    }

    // Step 3: within that strip, pick the rect that contains the cursor's
    // reference position. On a boundary, prefer the rect *above* (L/R) or
    // *to the left* (U/D): we look for the largest `r.y` (or `r.x`) that's
    // still strictly less than the reference, then fall back to the
    // topmost / leftmost rect when the reference is at the very start.
    let chosen: Option<crate::window::WindowId> = match dir {
        Dir::Left | Dir::Right => {
            let y = y_ref;
            same_strip
                .iter()
                .filter(|(_, r)| r.y <= y)
                .max_by_key(|(_, r)| r.y)
                .or_else(|| same_strip.iter().min_by_key(|(_, r)| r.y))
                .map(|(w, _)| *w)
        }
        Dir::Up | Dir::Down => {
            let x = x_ref;
            same_strip
                .iter()
                .filter(|(_, r)| r.x <= x)
                .max_by_key(|(_, r)| r.x)
                .or_else(|| same_strip.iter().min_by_key(|(_, r)| r.x))
                .map(|(w, _)| *w)
        }
    };

    if let (Some(next), Some(tab)) = (chosen, editor.tabs.get_mut(editor.active_tab)) {
        tab.active = next;
    }
}

/// Cursor position projected into the synthetic 1000×1000 layout, used as
/// a reference point when choosing which target row/column to land in.
fn cursor_synthetic_position(
    editor: &Editor,
    active: crate::window::WindowId,
    active_rect: Rect,
) -> (u16, u16) {
    let Some(window) = editor.windows.get(&active) else {
        return (
            active_rect.x + active_rect.width / 2,
            active_rect.y + active_rect.height / 2,
        );
    };
    let viewport_h = window.viewport_h.max(1) as f32;
    let viewport_w = window.viewport_w.max(1) as f32;
    let screen_row = window.cursor.row.saturating_sub(window.top_line) as f32;
    let screen_col = window.cursor.col.saturating_sub(window.left_col) as f32;
    let y = active_rect.y as f32 + (screen_row / viewport_h) * active_rect.height as f32;
    let x = active_rect.x as f32 + (screen_col / viewport_w) * active_rect.width as f32;
    (
        (x as u16).min(active_rect.x + active_rect.width),
        (y as u16).min(active_rect.y + active_rect.height),
    )
}

fn equalize_splits(editor: &mut Editor) {
    if let Some(tab) = editor.tabs.get_mut(editor.active_tab) {
        tab.tree.equalize();
    }
}

fn focus_next(editor: &mut Editor) {
    let Some(tab) = editor.tabs.get_mut(editor.active_tab) else {
        return;
    };
    let order = tab.tree.windows();
    let Some(pos) = order.iter().position(|w| *w == tab.active) else {
        return;
    };
    let next = order[(pos + 1) % order.len()];
    tab.active = next;
}

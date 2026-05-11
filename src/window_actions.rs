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
    reg.register("close_window", Arc::new(close_active));
    reg.register("focus_left", Arc::new(|ed| focus_direction(ed, Dir::Left)));
    reg.register("focus_right", Arc::new(|ed| focus_direction(ed, Dir::Right)));
    reg.register("focus_up", Arc::new(|ed| focus_direction(ed, Dir::Up)));
    reg.register("focus_down", Arc::new(|ed| focus_direction(ed, Dir::Down)));
    reg.register("focus_next", Arc::new(focus_next));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    use ModeId::Normal;
    let bindings = [
        ("<C-w>h", "focus_left"),
        ("<C-w>j", "focus_down"),
        ("<C-w>k", "focus_up"),
        ("<C-w>l", "focus_right"),
        ("<C-w>w", "focus_next"),
        ("<C-w>s", "split_horizontal"),
        ("<C-w>v", "split_vertical"),
        ("<C-w>c", "close_window"),
    ];
    for (seq, action) in bindings {
        reg.bind(Normal, seq, Action::Builtin(action)).unwrap();
    }
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

pub fn close_active(editor: &mut Editor) {
    let Some(tab_idx) = (editor.active_tab < editor.tabs.len()).then_some(editor.active_tab) else {
        return;
    };
    let active = editor.tabs[tab_idx].active;
    let tab = &mut editor.tabs[tab_idx];
    let old_tree = std::mem::replace(&mut tab.tree, crate::window::SplitTree::Leaf(active));
    match old_tree.remove_leaf(active) {
        Some(new_tree) => {
            // Pick a new active window — first leaf in the remaining tree.
            let first = new_tree.windows().first().copied();
            tab.tree = new_tree;
            if let Some(w) = first {
                tab.active = w;
            }
            editor.windows.remove(&active);
        }
        None => {
            // Removed the last window in this tab. Drop the tab entirely.
            editor.tabs.remove(tab_idx);
            editor.windows.remove(&active);
            if editor.tabs.is_empty() {
                editor.should_quit = true;
            } else {
                editor.active_tab = editor.active_tab.min(editor.tabs.len() - 1);
            }
        }
    }
}

#[derive(Copy, Clone)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

fn focus_direction(editor: &mut Editor, dir: Dir) {
    let Some(tab) = editor.tabs.get(editor.active_tab) else {
        return;
    };
    let active = tab.active;
    // Use a synthetic full-window area so we can compare relative positions.
    // The actual size only matters for which window is "adjacent" — we use
    // the last-known viewport of the active window's renders as a proxy.
    // Even simpler: use a unit grid based on tree structure.
    let layout = tab.tree.layout(Rect { x: 0, y: 0, width: 1000, height: 1000 });
    let Some(active_rect) = layout.iter().find(|(w, _)| *w == active).map(|(_, r)| *r) else {
        return;
    };
    let mut best: Option<(crate::window::WindowId, u32)> = None;
    for (wid, rect) in &layout {
        if *wid == active {
            continue;
        }
        let adjacent = match dir {
            Dir::Left => rect.x + rect.width <= active_rect.x,
            Dir::Right => rect.x >= active_rect.x + active_rect.width,
            Dir::Up => rect.y + rect.height <= active_rect.y,
            Dir::Down => rect.y >= active_rect.y + active_rect.height,
        };
        if !adjacent {
            continue;
        }
        // Score by perpendicular distance to the active rect's center.
        let score = match dir {
            Dir::Left | Dir::Right => {
                let ac = active_rect.y + active_rect.height / 2;
                let rc = rect.y + rect.height / 2;
                (ac as i32 - rc as i32).unsigned_abs()
            }
            Dir::Up | Dir::Down => {
                let ac = active_rect.x + active_rect.width / 2;
                let rc = rect.x + rect.width / 2;
                (ac as i32 - rc as i32).unsigned_abs()
            }
        };
        if best.map_or(true, |(_, s)| score < s) {
            best = Some((*wid, score));
        }
    }
    if let (Some((next, _)), Some(tab)) = (best, editor.tabs.get_mut(editor.active_tab)) {
        tab.active = next;
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

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
    reg.register("focus_left", Arc::new(|ed| { focus_direction(ed, Dir::Left); after_focus(ed); }));
    reg.register("focus_right", Arc::new(|ed| { focus_direction(ed, Dir::Right); after_focus(ed); }));
    reg.register("focus_up", Arc::new(|ed| { focus_direction(ed, Dir::Up); after_focus(ed); }));
    reg.register("focus_down", Arc::new(|ed| { focus_direction(ed, Dir::Down); after_focus(ed); }));
    reg.register("focus_next", Arc::new(|ed| { focus_next(ed); after_focus(ed); }));
    reg.register("equalize_splits", Arc::new(equalize_splits));
    reg.register("maximize_height", Arc::new(|ed| maximize_active(ed, SplitAxis::Horizontal)));
    reg.register("maximize_width", Arc::new(|ed| maximize_active(ed, SplitAxis::Vertical)));
    reg.register("move_to_new_tab", Arc::new(move_to_new_tab));
    reg.register("file_info", Arc::new(show_file_info));
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
        ("<C-w>_", "maximize_height"),
        ("<C-w><C-_>", "maximize_height"),
        ("<C-w>|", "maximize_width"),
        ("<C-w>T", "move_to_new_tab"),
    ];
    for (seq, action) in bindings {
        reg.bind(Normal, seq, Action::Builtin(action)).unwrap();
    }
    reg.bind(Normal, "<C-g>", Action::Builtin("file_info")).unwrap();
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

    // Where the closing window's cursor sits, in synthetic layout coords. After
    // the sibling collapses into the freed space, whichever window now covers
    // this point becomes focused — so focus stays in the same column/strip and,
    // when a whole column collapses, follows the split-navigation rules
    // (respecting resized, non-even split ratios).
    let ref_point = {
        let tab = &editor.tabs[tab_idx];
        tab.tree
            .layout(SYNTH_CANVAS)
            .iter()
            .find(|(w, _)| *w == active)
            .map(|(_, r)| cursor_synthetic_position(editor, active, *r))
    };

    let tab = &mut editor.tabs[tab_idx];
    let old_tree = std::mem::replace(&mut tab.tree, crate::window::SplitTree::Leaf(active));
    match old_tree.remove_leaf(active) {
        Some(new_tree) => {
            let next = ref_point
                .and_then(|pt| window_at_point(&new_tree, pt))
                .or_else(|| new_tree.windows().first().copied());
            tab.tree = new_tree;
            if let Some(w) = next {
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

/// Remove a specific window (by id) from whatever tab owns it, collapsing
/// the split into its sibling. Used to auto-close terminal windows when the
/// job exits — no dirty check, since terminals have nothing to save.
pub fn remove_window(editor: &mut Editor, win_id: crate::window::WindowId) {
    let Some(tab_idx) = editor
        .tabs
        .iter()
        .position(|t| t.tree.windows().contains(&win_id))
    else {
        editor.windows.remove(&win_id);
        return;
    };
    let tab = &mut editor.tabs[tab_idx];
    let old_tree = std::mem::replace(&mut tab.tree, crate::window::SplitTree::Leaf(win_id));
    match old_tree.remove_leaf(win_id) {
        Some(new_tree) => {
            let first = new_tree.windows().first().copied();
            tab.tree = new_tree;
            if tab.active == win_id {
                if let Some(w) = first {
                    tab.active = w;
                }
            }
            editor.windows.remove(&win_id);
        }
        None => {
            editor.tabs.remove(tab_idx);
            editor.windows.remove(&win_id);
            if editor.tabs.is_empty() {
                editor.should_quit = true;
            } else {
                editor.active_tab = editor.active_tab.min(editor.tabs.len() - 1);
            }
        }
    }
}

/// Synthetic canvas for layout math: only relative positions matter, so a
/// fixed 1000×1000 rectangle keeps everything integer-friendly.
const SYNTH_CANVAS: Rect = Rect { x: 0, y: 0, width: 1000, height: 1000 };

/// The window covering `point` in `tree`'s synthetic layout — the leaf that
/// now occupies the spot a just-closed window vacated. Prefers the rect that
/// contains the point (using the actual, possibly-resized split ratios); on a
/// boundary or miss, falls back to the nearest rect centre.
fn window_at_point(tree: &crate::window::SplitTree, point: (u16, u16)) -> Option<crate::window::WindowId> {
    let (x, y) = point;
    let layout = tree.layout(SYNTH_CANVAS);
    layout
        .iter()
        .find(|(_, r)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
        .map(|(w, _)| *w)
        .or_else(|| {
            layout
                .iter()
                .min_by_key(|(_, r)| {
                    let cx = r.x as i32 + r.width as i32 / 2;
                    let cy = r.y as i32 + r.height as i32 / 2;
                    let dx = cx - x as i32;
                    let dy = cy - y as i32;
                    dx * dx + dy * dy
                })
                .map(|(w, _)| *w)
        })
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
    let layout = tab.tree.layout(SYNTH_CANVAS);
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

/// `<C-w>_` (Horizontal) / `<C-w>|` (Vertical): grow the active window along
/// `axis` to the maximum the layout allows, leaving the cross axis untouched.
fn maximize_active(editor: &mut Editor, axis: SplitAxis) {
    if let Some(tab) = editor.tabs.get_mut(editor.active_tab) {
        let target = tab.active;
        tab.tree.maximize(target, axis);
    }
}

/// `<C-w>T`: move the active window to its own new tab page, collapsing the
/// split it leaves behind. No-op when it is already the only window in the tab
/// (it would just move to an identical layout), matching vim.
fn move_to_new_tab(editor: &mut Editor) {
    let Some(tab_idx) = (editor.active_tab < editor.tabs.len()).then_some(editor.active_tab) else {
        return;
    };
    // Only one window in this tab → nothing to peel off.
    if editor.tabs[tab_idx].tree.windows().len() < 2 {
        return;
    }
    let active = editor.tabs[tab_idx].active;

    // Detach `active` from the current tab, collapsing its split into the
    // sibling, but keep the Window itself alive in `editor.windows`.
    let tab = &mut editor.tabs[tab_idx];
    let old_tree = std::mem::replace(&mut tab.tree, crate::window::SplitTree::Leaf(active));
    if let Some(new_tree) = old_tree.remove_leaf(active) {
        if let Some(w) = new_tree.windows().first().copied() {
            tab.active = w;
        }
        tab.tree = new_tree;
    }

    // Drop it into a fresh tab placed right after the current one, and focus it.
    let new_idx = tab_idx + 1;
    editor.tabs.insert(new_idx, crate::tab::Tab::single(active));
    editor.active_tab = new_idx;
}

/// Resize the active window by `delta` (rows for `Horizontal`, columns for
/// `Vertical`). Returns `true` if a matching split was found and adjusted.
/// A no-op (returns `false`) when the window has no split along that axis.
pub fn resize_active(editor: &mut Editor, axis: SplitAxis, delta: i32) -> bool {
    let (w, h) = editor.last_window_area;
    if w == 0 || h == 0 {
        return false;
    }
    let area = ratatui::layout::Rect { x: 0, y: 0, width: w, height: h };
    let Some(tab) = editor.tabs.get_mut(editor.active_tab) else {
        return false;
    };
    let target = tab.active;
    tab.tree.resize(target, axis, delta, area)
}

/// The active window's current content size `(width, height)` from the last
/// render — used to turn an absolute `:resize N` into a delta.
pub fn active_window_size(editor: &Editor) -> Option<(u16, u16)> {
    let tab = editor.tabs.get(editor.active_tab)?;
    let w = editor.windows.get(&tab.active)?;
    Some((w.viewport_w, w.viewport_h))
}

/// After any focus change, re-sync the editor mode: focusing a terminal
/// window enters Terminal-job mode, leaving one returns to Normal.
fn after_focus(editor: &mut Editor) {
    crate::mode::sync_mode_for_active(editor);
}

/// Move focus by a vim direction letter (`h`/`j`/`k`/`l`) or `w` for next.
/// Used by Terminal-job mode's `<C-w>` window commands.
pub fn focus_dir(editor: &mut Editor, c: char) {
    match c {
        'h' => focus_direction(editor, Dir::Left),
        'j' => focus_direction(editor, Dir::Down),
        'k' => focus_direction(editor, Dir::Up),
        'l' => focus_direction(editor, Dir::Right),
        'w' => focus_next(editor),
        _ => {}
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

/// `<C-g>` — show full path, line count, and cursor position, vim-style.
fn show_file_info(editor: &mut Editor) {
    let Some(win) = editor.active_window() else { return };
    let buf_id = win.buffer;
    let cur_line = win.cursor.row + 1;
    let Some(buf) = editor.buffers.get(&buf_id) else { return };

    let path = buf
        .path()
        .and_then(|p| p.canonicalize().ok().or_else(|| Some(p.to_path_buf())))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| buf.display_name());
    let dirty = if buf.is_dirty() { " [Modified]" } else { "" };
    let total = buf.line_count();
    let pct = if total == 0 {
        "--".to_string()
    } else {
        format!("{}%", (cur_line * 100).div_ceil(total))
    };
    editor.status_message = Some(format!(
        "\"{path}\"{dirty}  line {cur_line} of {total}  --{pct}--"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::{SplitAxis, SplitTree, WindowId};

    fn leaf(n: u32) -> Box<SplitTree> {
        Box::new(SplitTree::Leaf(WindowId(n)))
    }

    #[test]
    fn point_lands_in_containing_rect() {
        // Two columns: left = win 1, right = win 2 (even vertical split).
        let tree = SplitTree::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.5,
            first: leaf(1),
            second: leaf(2),
        };
        assert_eq!(window_at_point(&tree, (250, 500)), Some(WindowId(1)));
        assert_eq!(window_at_point(&tree, (750, 500)), Some(WindowId(2)));
    }

    #[test]
    fn point_respects_resized_ratio() {
        // Top 60% = win 1, bottom 40% = win 2 (a resized horizontal split).
        let tree = SplitTree::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.6,
            first: leaf(1),
            second: leaf(2),
        };
        // y=550 sits in the top 60% (0..600) → win 1.
        assert_eq!(window_at_point(&tree, (500, 550)), Some(WindowId(1)));
        // y=650 sits in the bottom 40% (600..1000) → win 2, not the midpoint.
        assert_eq!(window_at_point(&tree, (500, 650)), Some(WindowId(2)));
    }

    #[test]
    fn boundary_point_falls_back_to_nearest_centre() {
        let tree = SplitTree::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            first: leaf(1),
            second: leaf(2),
        };
        // y=1000 is outside every half-open rect; nearest centre is win 2.
        assert_eq!(window_at_point(&tree, (500, 1000)), Some(WindowId(2)));
    }
}

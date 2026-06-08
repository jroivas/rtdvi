//! Interaction for non-editable render buffers (see [`crate::buffer::RenderContent`]).
//!
//! A render buffer is a modal viewer: most keys still scroll/move the cursor
//! (the rope holds the plain text), but a few are special — `Enter` follows the
//! link under the cursor, `<Tab>`/`<S-Tab>` jump between links, and `q` closes
//! the view. These are dispatched ahead of the normal keymap (and ahead of the
//! macro `q` handler) by [`handle_key`].

use crate::keymap::{Key, KeyCode};
use crate::Editor;

/// Handle a key for the active render-buffer window. Returns `true` if the key
/// was consumed (so the caller stops); `false` lets normal motions/scroll run.
/// Only call when the active buffer is a render buffer and mode is Normal.
pub fn handle_key(editor: &mut Editor, key: Key) -> bool {
    match key.code {
        KeyCode::Enter if key.mods.is_empty() => {
            follow_link(editor);
            true
        }
        KeyCode::Tab if key.mods.is_empty() => {
            focus_link(editor, true);
            true
        }
        KeyCode::BackTab => {
            focus_link(editor, false);
            true
        }
        // `gx` is a common "follow" alias; accept a lone `x` is too aggressive,
        // so only Enter/Tab are bound. `q` closes the viewer (ahead of macros).
        KeyCode::Char('q') if key.mods.is_empty() => {
            let _ = crate::window_actions::close_active(editor, true);
            true
        }
        _ => false,
    }
}

/// True when the active buffer is a render buffer.
pub fn active_is_render(editor: &Editor) -> bool {
    editor
        .active_buffer_id()
        .and_then(|id| editor.buffers.get(&id))
        .map(|b| !b.is_editable())
        .unwrap_or(false)
}

/// Move the cursor onto the next (`forward`) or previous link's first column,
/// wrapping around. No-op when the buffer has no links.
fn focus_link(editor: &mut Editor, forward: bool) {
    let Some((row, col, mut links)) = editor
        .active_buffer_id()
        .and_then(|id| editor.buffers.get(&id))
        .and_then(|b| b.render_content())
        .map(|c| {
            let w = editor.active_window().map(|w| w.cursor).unwrap_or_default();
            (
                w.row,
                w.col,
                c.links
                    .iter()
                    .map(|l| (l.line, l.start_col))
                    .collect::<Vec<_>>(),
            )
        })
    else {
        return;
    };
    if links.is_empty() {
        return;
    }
    links.sort_unstable();
    let cur = (row, col);
    let target = if forward {
        links.iter().find(|&&p| p > cur).copied().unwrap_or(links[0])
    } else {
        links.iter().rev().find(|&&p| p < cur).copied().unwrap_or(*links.last().unwrap())
    };
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = target.0;
        w.cursor.col = target.1;
        w.cursor.sticky_col = target.1;
    }
}

/// Follow the link under the cursor: resolve its target relative to the page's
/// base dir, open the file, and re-run the producing command so the linked
/// page renders in the same pane. Non-file targets report a status message.
fn follow_link(editor: &mut Editor) {
    let (row, col) = match editor.active_window() {
        Some(w) => (w.cursor.row, w.cursor.col),
        None => return,
    };
    let Some(content) = editor
        .active_buffer_id()
        .and_then(|id| editor.buffers.get(&id))
        .and_then(|b| b.render_content())
    else {
        return;
    };
    let Some(link) = content
        .links
        .iter()
        .find(|l| l.line == row && (l.start_col..l.end_col).contains(&col))
    else {
        editor.status_message = Some("no link under cursor".into());
        return;
    };
    let target = link.target.clone();
    let producer = content.producer.clone();
    let base_dir = content.base_dir.clone();
    let source_window = content.source_window;

    // Only local files are navigable in v1; other targets are reported.
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || target.contains("://") {
        editor.status_message = Some(format!("external link: {target}"));
        return;
    }
    if target.starts_with('#') {
        editor.status_message = Some(format!("anchor links not supported: {target}"));
        return;
    }
    let Some(producer) = producer else {
        editor.status_message = Some("cannot follow link (no producer)".into());
        return;
    };

    let rel = target.strip_prefix("./").unwrap_or(&target);
    let path = {
        let p = std::path::Path::new(rel);
        if p.is_absolute() {
            p.to_path_buf()
        } else if let Some(base) = &base_dir {
            base.join(p)
        } else {
            p.to_path_buf()
        }
    };
    if !path.is_file() {
        editor.status_message = Some(format!("not a file: {}", path.display()));
        return;
    }

    let bid = match editor.open_path(&path) {
        Ok(b) => b,
        Err(e) => {
            editor.status_message = Some(format!("open {}: {e}", path.display()));
            return;
        }
    };

    // Point a source window at the opened file and make it active, so the
    // producer reads it; its OpenRenderBuffer then reuses this render window.
    let src_win = source_window.filter(|wid| editor.windows.contains_key(wid));
    let src_win = match src_win {
        Some(wid) => wid,
        None => {
            // Source window gone — make a sibling to host the file.
            crate::window_actions::split_active(editor, crate::window::SplitAxis::Horizontal);
            match editor.tabs.get(editor.active_tab).map(|t| t.active) {
                Some(wid) => wid,
                None => return,
            }
        }
    };
    if let Some(w) = editor.windows.get_mut(&src_win) {
        w.buffer = bid;
        w.cursor = crate::cursor::Cursor::default();
        w.top_line = 0;
        w.left_col = 0;
    }
    if let Some(tab) = editor.tabs.get_mut(editor.active_tab) {
        tab.active = src_win;
    }
    crate::command::run_ex_line(editor, &producer);
}

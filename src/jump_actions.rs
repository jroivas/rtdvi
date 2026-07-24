//! Jumplist navigation (`<C-o>` / `<C-i>`) and "search word under cursor"
//! (`*`, `#`, `£`).

use std::sync::Arc;

use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::ModeId;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("jump_back", Arc::new(jump_back));
    reg.register("jump_forward", Arc::new(jump_forward));
    reg.register("search_word_under_cursor", Arc::new(search_word_under_cursor));
    reg.register(
        "search_word_under_cursor_backward",
        Arc::new(search_word_under_cursor_backward),
    );
    reg.register("center_cursor", Arc::new(center_cursor));
    reg.register("scroll_cursor_top", Arc::new(scroll_cursor_top));
    reg.register("scroll_cursor_bottom", Arc::new(scroll_cursor_bottom));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    let bindings = [
        ("<C-o>", "jump_back"),
        // Terminals usually send <Tab> for Ctrl-I, so we bind that.
        ("<Tab>", "jump_forward"),
        ("*", "search_word_under_cursor"),
        ("#", "search_word_under_cursor_backward"),
        // Vim treats `£` (char 163) as an alias for `#`: search backward.
        ("£", "search_word_under_cursor_backward"),
        // zz / zt / zb — anchor the cursor's line to mid / top / bottom
        // of the window.
        ("zz", "center_cursor"),
        ("zt", "scroll_cursor_top"),
        ("zb", "scroll_cursor_bottom"),
    ];
    for (seq, action) in bindings {
        reg.bind(ModeId::Normal, seq, Action::Builtin(action)).unwrap();
    }
}

// ---- zz / zt / zb ---------------------------------------------------------

fn center_cursor(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.center_on_cursor();
    }
}

fn scroll_cursor_top(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.scroll_top_to_cursor();
    }
}

fn scroll_cursor_bottom(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.scroll_bottom_to_cursor();
    }
}

// ---- <C-o> / <C-i> --------------------------------------------------------

fn jump_back(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some(current) = editor.current_jump_entry() else {
        return;
    };
    if let Some(entry) = editor.jumplist.back(current) {
        editor.jumplist_goto(entry);
    } else {
        editor.status_message = Some("Already at oldest jump".into());
    }
}

fn jump_forward(editor: &mut Editor) {
    let _ = editor.take_count();
    if let Some(entry) = editor.jumplist.forward() {
        editor.jumplist_goto(entry);
    } else {
        editor.status_message = Some("Already at newest jump".into());
    }
}

// ---- * / # / £ ------------------------------------------------------------

/// `*` — search forward for the word under the cursor.
fn search_word_under_cursor(editor: &mut Editor) {
    search_word(editor, true);
}

fn search_word_under_cursor_backward(editor: &mut Editor) {
    search_word(editor, false);
}

fn search_word(editor: &mut Editor, forward: bool) {
    let _ = editor.take_count();
    let Some(word) = editor.word_under_cursor() else {
        editor.status_message = Some("No word under cursor".into());
        return;
    };
    // Build a `\bword\b` regex so `foo` doesn't match `foobar`.
    let pattern = format!(r"\b{}\b", regex::escape(&word));
    if let Err(e) = editor.search.set_pattern(&pattern) {
        editor.status_message = Some(format!("E: bad pattern: {e}"));
        return;
    }
    editor.search.direction_forward = forward;
    // Record where we are NOW so <C-o> can come back.
    editor.jumplist_record_here();
    // Don't start AT the cursor (we'd hit the current symbol); skip past it.
    crate::search_actions::jump_match(editor, forward, false);
}


//! `!` — shell filter actions.
//!
//! `!` in Visual / VisualLine mode: stash the selected line range and enter
//! command mode pre-filled with `!`. Typing `sort -u` and pressing Enter
//! then pipes those lines through `sh -c "sort -u"` and replaces them.
//!
//! `!!` in Normal mode: same but for the cursor's current line only.

use std::sync::Arc;

use crate::cursor::Selection;
use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("shell_filter", Arc::new(shell_filter));
    reg.register("shell_filter_line", Arc::new(shell_filter_line));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    reg.bind(ModeId::Visual, "!", Action::Builtin("shell_filter")).unwrap();
    reg.bind(ModeId::VisualLine, "!", Action::Builtin("shell_filter")).unwrap();
    reg.bind(ModeId::Normal, "!!", Action::Builtin("shell_filter_line")).unwrap();
}

fn shell_filter(editor: &mut Editor) {
    let Some(w) = editor.active_window() else { return };
    let cur_row = w.cursor.row;
    let (first, last) = match w.selection {
        Selection::Line { anchor_row } => (anchor_row.min(cur_row), anchor_row.max(cur_row)),
        Selection::Char { anchor } => (anchor.row.min(cur_row), anchor.row.max(cur_row)),
        Selection::Block { anchor } => (anchor.row.min(cur_row), anchor.row.max(cur_row)),
        Selection::None => (cur_row, cur_row),
    };
    editor.shell_filter_range = Some((first, last));
    if let Some(w) = editor.active_window_mut() {
        w.selection = Selection::None;
    }
    editor.command_line.input = "!".into();
    editor.command_line.cursor = 1;
    switch_mode(editor, ModeId::Command);
}

fn shell_filter_line(editor: &mut Editor) {
    let row = editor.active_window().map(|w| w.cursor.row).unwrap_or(0);
    editor.shell_filter_range = Some((row, row));
    editor.command_line.input = "!".into();
    editor.command_line.cursor = 1;
    switch_mode(editor, ModeId::Command);
}

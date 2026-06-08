//! Modes.
//!
//! v1 keeps modes pragmatic: a `ModeId` tag on `Editor` and a free-function
//! `handle_key` dispatcher per mode. A trait-based dispatch was considered
//! but creates borrow-checker pain when modes mutate the editor. The
//! [`ModeId`] is the seam — plugins later can replace key handling per
//! mode through the keymap registry without needing to add new modes.

pub mod command;
pub mod insert;
pub mod normal;
pub mod search;
pub mod terminal;
pub mod visual;
pub mod visual_block;
pub mod visual_line;

use crate::keymap::{Key, KeyCode};
use crate::Editor;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModeId {
    Normal,
    Insert,
    Visual,
    VisualLine,
    VisualBlock,
    Command,
    Search,
    /// Terminal-job mode: keystrokes are forwarded to the embedded
    /// terminal's child process. Entered when a terminal window is focused.
    Terminal,
}

impl ModeId {
    pub fn short_name(self) -> &'static str {
        match self {
            ModeId::Normal => "NORMAL",
            ModeId::Insert => "INSERT",
            ModeId::Visual => "VISUAL",
            ModeId::VisualLine => "V-LINE",
            ModeId::VisualBlock => "V-BLOCK",
            ModeId::Command => "COMMAND",
            ModeId::Search => "SEARCH",
            ModeId::Terminal => "TERMINAL",
        }
    }
}

pub fn handle_key(editor: &mut Editor, key: Key) {
    // Render buffers are modal viewers: they claim Enter/Tab/q (link follow,
    // link nav, close) ahead of everything else — including the macro `q`
    // handler — while letting motions/scroll fall through.
    if editor.mode == ModeId::Normal && crate::render_actions::active_is_render(editor) {
        if crate::render_actions::handle_key(editor, key) {
            return;
        }
    }
    // Macro control (`q`/`@`) and recording capture sit above mode dispatch so
    // they see every keystroke and stay orthogonal to per-mode key handling.
    if crate::macros::pre_dispatch(editor, key) {
        return;
    }
    match editor.mode {
        ModeId::Normal => normal::handle_key(editor, key),
        ModeId::Insert => insert::handle_key(editor, key),
        ModeId::Visual => visual::handle_key(editor, key),
        ModeId::VisualLine => visual_line::handle_key(editor, key),
        ModeId::VisualBlock => visual_block::handle_key(editor, key),
        ModeId::Command => command::handle_key(editor, key),
        ModeId::Search => search::handle_key(editor, key),
        ModeId::Terminal => terminal::handle_key(editor, key),
    }
}

/// Re-sync the editor mode after a focus change: a focused terminal window
/// puts us in Terminal-job mode; leaving one drops back to Normal. Called
/// after window-focus navigation and after a terminal is reaped.
pub fn sync_mode_for_active(editor: &mut Editor) {
    let is_term = editor.active_is_terminal();
    if is_term && editor.mode != ModeId::Terminal {
        editor.terminal_window_cmd = false;
        switch_mode(editor, ModeId::Terminal);
    } else if !is_term && editor.mode == ModeId::Terminal {
        switch_mode(editor, ModeId::Normal);
    }
}

/// Handle a bracketed-paste chunk (whole pasted text at once). Inserted
/// verbatim, so it never triggers per-character autoindent/brace-dedent.
/// Only acts in Insert and Command modes; in other modes a stray paste is
/// ignored rather than executed as commands.
pub fn handle_paste(editor: &mut Editor, text: &str) {
    if text.is_empty() {
        return;
    }
    // Terminals deliver line breaks in bracketed paste as CR (`\r`) or CRLF,
    // not LF. Normalise to `\n` so line splitting and per-line cursor
    // advancement work — otherwise a multi-line paste leaves the cursor stuck
    // on the first line.
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    match editor.mode {
        ModeId::Insert => insert::insert_paste(editor, &normalized),
        ModeId::Command => {
            // Drop control chars (newlines/tabs) — the command line is one row.
            let cur = editor.command_line.cursor;
            let clean: String = normalized.chars().filter(|c| !c.is_control()).collect();
            editor.command_line.input.insert_str(cur, &clean);
            editor.command_line.cursor = cur + clean.len();
        }
        _ => {}
    }
}

/// Enter the command line from a visual mode, prefilled with the `'<,'>`
/// range (vim's behaviour when you press `:` over a selection). The
/// selection's inclusive line range is stashed in `last_visual_range` so a
/// ranged ex command (`:'<,'>s/…/…/`) can resolve it, and the selection is
/// cleared as we drop into Command mode.
pub fn enter_command_from_visual(editor: &mut Editor) {
    if let Some(w) = editor.active_window() {
        let cur = w.cursor.row;
        let other = match w.selection {
            crate::cursor::Selection::Char { anchor } => anchor.row,
            crate::cursor::Selection::Block { anchor } => anchor.row,
            crate::cursor::Selection::Line { anchor_row } => anchor_row,
            crate::cursor::Selection::None => cur,
        };
        editor.last_visual_range = Some((cur.min(other), cur.max(other)));
    }
    if let Some(w) = editor.active_window_mut() {
        w.selection = crate::cursor::Selection::None;
    }
    editor.clear_pending_count();
    editor.command_line.clear();
    editor.command_line.input = "'<,'>".to_string();
    editor.command_line.cursor = editor.command_line.input.len();
    switch_mode(editor, ModeId::Command);
}

/// Switch modes, emitting `ModeChanged`. Free function so callers don't
/// need to borrow `editor` twice.
pub fn switch_mode(editor: &mut Editor, to: ModeId) {
    let from = editor.mode;
    if from == to {
        return;
    }
    // Render buffers are non-editable: refuse to enter Insert on one.
    if to == ModeId::Insert {
        let editable = editor
            .active_buffer_id()
            .and_then(|id| editor.buffers.get(&id))
            .map(|b| b.is_editable())
            .unwrap_or(true);
        if !editable {
            editor.status_message = Some("E21: buffer is not modifiable".into());
            return;
        }
    }
    editor.mode = to;
    crate::event::emit(editor, crate::event::Event::ModeChanged { from, to });
}

/// Try to consume `key` as a count digit. Returns true if the key was a
/// digit and got absorbed into the editor's pending count.
///
/// - When `pending_keys` is empty, digits go into `pending_count_pre`.
/// - When `pending_keys` is a known operator prefix (`d` / `c` / `y`) and
///   `allow_after_operator` is true, digits go into `pending_count_post`.
///   Visual modes pass `false` because they have no operator-pending state.
/// - A leading `0` is always the line-start motion, never a count digit.
pub fn try_accumulate_count(editor: &mut Editor, key: Key, allow_after_operator: bool) -> bool {
    if !key.mods.is_empty() {
        return false;
    }
    let KeyCode::Char(c) = key.code else {
        return false;
    };
    if !c.is_ascii_digit() {
        return false;
    }
    let d = c.to_digit(10).unwrap() as usize;
    let at_start = editor.pending_keys.is_empty();
    let at_op = allow_after_operator && is_operator_prefix(&editor.pending_keys);

    let leading_zero_at_start =
        at_start && d == 0 && editor.pending_count_pre.is_none();
    let leading_zero_after_op =
        at_op && d == 0 && editor.pending_count_post.is_none();

    if at_start && !leading_zero_at_start {
        editor.pending_count_pre = Some(editor.pending_count_pre.unwrap_or(0) * 10 + d);
        return true;
    }
    if at_op && !leading_zero_after_op {
        editor.pending_count_post = Some(editor.pending_count_post.unwrap_or(0) * 10 + d);
        return true;
    }
    false
}

fn is_operator_prefix(keys: &[Key]) -> bool {
    keys.len() == 1
        && keys[0].mods.is_empty()
        && matches!(
            keys[0].code,
            KeyCode::Char('d') | KeyCode::Char('c') | KeyCode::Char('y')
        )
}

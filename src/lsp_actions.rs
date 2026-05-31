//! Keymap-bound LSP actions: `gd` (definition), `K` (hover),
//! `[d` / `]d` (prev / next diagnostic).

use std::path::Path;
use std::sync::Arc;

use lsp_types::Url;

use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::ModeId;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("lsp_goto_definition", Arc::new(goto_definition));
    reg.register("lsp_goto_declaration", Arc::new(goto_declaration));
    reg.register("lsp_goto_implementation", Arc::new(goto_implementation));
    reg.register("lsp_goto_type_definition", Arc::new(goto_type_definition));
    reg.register("lsp_references", Arc::new(references));
    reg.register("lsp_hover", Arc::new(hover));
    reg.register("lsp_diagnostic_next", Arc::new(diagnostic_next));
    reg.register("lsp_diagnostic_prev", Arc::new(diagnostic_prev));
    reg.register("lsp_diagnostic_at_cursor", Arc::new(diagnostic_at_cursor));
    reg.register("lsp_rename_prompt", Arc::new(rename_prompt));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    let bindings = [
        ("gd", "lsp_goto_definition"),
        ("gD", "lsp_goto_declaration"),
        ("gi", "lsp_goto_implementation"),
        ("gf", "lsp_goto_type_definition"),
        ("gr", "lsp_references"),
        ("K", "lsp_hover"),
        ("]d", "lsp_diagnostic_next"),
        ("[d", "lsp_diagnostic_prev"),
        // Leader bindings use the default leader `\`.
        ("\\h", "lsp_diagnostic_at_cursor"),
        ("\\rn", "lsp_rename_prompt"),
    ];
    for (seq, action) in bindings {
        reg.bind(ModeId::Normal, seq, Action::Builtin(action)).unwrap();
    }
}

// ---- helpers ---------------------------------------------------------------

fn active_buffer_uri_and_pos(editor: &Editor) -> Option<(String, u32, u32, String)> {
    let win = editor.active_window()?;
    let buf_id = win.buffer;
    let buf = editor.buffers.get(&buf_id)?;
    let path = buf.path()?;
    let uri = Url::from_file_path(path).ok()?.to_string();
    let filetype = editor.syntax_for(buf_id).filetype.to_string();
    Some((uri, win.cursor.row as u32, win.cursor.col as u32, filetype))
}

// ---- single/multi-location jumps -------------------------------------------

/// Helper for all four `goto_*` actions. Requests all matching locations;
/// jumps directly when there is exactly one, shows the picker when there
/// are multiple, reports "no <label>" when the server returns nothing.
fn location_jump<F>(editor: &mut Editor, label: &str, request: F)
where
    F: FnOnce(&mut crate::lsp::Client, &str, u32, u32) -> Vec<(String, u32, u32)>,
{
    let _ = editor.take_count();
    let Some((uri, line, character, filetype)) = active_buffer_uri_and_pos(editor) else {
        editor.status_message = Some("LSP: no buffer".into());
        return;
    };
    let Some(path_buf) = Url::parse(&uri).ok().and_then(|u| u.to_file_path().ok()) else {
        return;
    };
    let locations = {
        let Some(client) = editor.lsp.find_for(&filetype, &path_buf) else {
            editor.status_message = Some(format!("LSP: no client for {filetype}"));
            return;
        };
        request(client, &uri, line, character)
    };
    match locations.len() {
        0 => editor.status_message = Some(format!("LSP: no {label}")),
        1 => {
            let (t_uri, t_line, t_char) = locations.into_iter().next().unwrap();
            editor.jumplist_record_here();
            open_uri_at(editor, &t_uri, t_line as usize, t_char as usize);
        }
        _ => {
            editor.jumplist_record_here();
            editor.lsp_picker = Some(crate::editor::LocationPicker::new(locations));
        }
    }
}

fn goto_definition(editor: &mut Editor) {
    location_jump(editor, "definition", |c, u, l, ch| c.goto_definition(u, l, ch));
}

fn goto_declaration(editor: &mut Editor) {
    location_jump(editor, "declaration", |c, u, l, ch| c.goto_declaration(u, l, ch));
}

fn goto_implementation(editor: &mut Editor) {
    location_jump(editor, "implementation", |c, u, l, ch| c.goto_implementation(u, l, ch));
}

fn goto_type_definition(editor: &mut Editor) {
    location_jump(editor, "type definition", |c, u, l, ch| c.goto_type_definition(u, l, ch));
}

// ---- references ------------------------------------------------------------

/// `gr` — fetch every reference site. Shows a picker when there are
/// multiple results so the user can choose; jumps directly for a single hit.
fn references(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some((uri, line, character, filetype)) = active_buffer_uri_and_pos(editor) else {
        editor.status_message = Some("LSP: no buffer".into());
        return;
    };
    let Some(path_buf) = Url::parse(&uri).ok().and_then(|u| u.to_file_path().ok()) else {
        return;
    };
    let locs = {
        let Some(client) = editor.lsp.find_for(&filetype, &path_buf) else {
            editor.status_message = Some(format!("LSP: no client for {filetype}"));
            return;
        };
        client.references(&uri, line, character, true)
    };
    match locs.len() {
        0 => editor.status_message = Some("LSP: no references".into()),
        1 => {
            let (t_uri, t_line, t_char) = locs.into_iter().next().unwrap();
            editor.jumplist_record_here();
            open_uri_at(editor, &t_uri, t_line as usize, t_char as usize);
        }
        _ => {
            editor.jumplist_record_here();
            editor.lsp_picker = Some(crate::editor::LocationPicker::new(locs));
        }
    }
}

// ---- diagnostic at cursor --------------------------------------------------

fn diagnostic_at_cursor(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cur_row = win.cursor.row as u32;
    let Some(path) = editor.buffers.get(&buf_id).and_then(|b| b.path()) else {
        return;
    };
    let uri = match Url::from_file_path(path) {
        Ok(u) => u.to_string(),
        Err(_) => return,
    };
    let mut msg: Option<String> = None;
    for client in editor.lsp.clients.values() {
        for d in client.diagnostics.for_uri(&uri) {
            if d.range.start.line <= cur_row && cur_row <= d.range.end.line {
                let sev = match d.severity {
                    Some(lsp_types::DiagnosticSeverity::ERROR) => "error",
                    Some(lsp_types::DiagnosticSeverity::WARNING) => "warning",
                    Some(lsp_types::DiagnosticSeverity::INFORMATION) => "info",
                    Some(lsp_types::DiagnosticSeverity::HINT) => "hint",
                    _ => "diag",
                };
                msg = Some(format!("{sev}: {}", d.message.lines().next().unwrap_or("")));
                break;
            }
        }
        if msg.is_some() {
            break;
        }
    }
    editor.status_message = Some(msg.unwrap_or_else(|| "LSP: no diagnostic at cursor".into()));
}

// ---- K (hover) -------------------------------------------------------------

fn hover(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some((uri, line, character, filetype)) = active_buffer_uri_and_pos(editor) else {
        return;
    };
    let path_buf = match Url::parse(&uri).ok().and_then(|u| u.to_file_path().ok()) {
        Some(p) => p,
        None => return,
    };
    let text = {
        let Some(client) = editor.lsp.find_for(&filetype, &path_buf) else {
            return;
        };
        client.hover(&uri, line, character)
    };
    if let Some(t) = text {
        // First line only — the cmdline area is one row tall in v1.
        let first = t.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        editor.status_message = Some(first.to_string());
    } else {
        editor.status_message = Some("LSP: no hover".into());
    }
}

// ---- diagnostic navigation -------------------------------------------------

fn diagnostic_next(editor: &mut Editor) {
    diagnostic_jump(editor, true);
}
fn diagnostic_prev(editor: &mut Editor) {
    diagnostic_jump(editor, false);
}

fn diagnostic_jump(editor: &mut Editor, forward: bool) {
    let _ = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cur_row = win.cursor.row as u32;
    let Some(path) = editor.buffers.get(&buf_id).and_then(|b| b.path()) else {
        return;
    };
    let uri = match Url::from_file_path(path) {
        Ok(u) => u.to_string(),
        Err(_) => return,
    };
    let filetype = editor.syntax_for(buf_id).filetype.to_string();

    // Gather diagnostic line numbers from every client that knows this URI.
    let mut lines: Vec<u32> = Vec::new();
    for client in editor.lsp.clients.values() {
        for d in client.diagnostics.for_uri(&uri) {
            lines.push(d.range.start.line);
        }
        let _ = filetype.clone(); // silence
    }
    if lines.is_empty() {
        editor.status_message = Some("LSP: no diagnostics".into());
        return;
    }
    lines.sort();
    lines.dedup();
    let target = if forward {
        lines.iter().find(|&&l| l > cur_row).copied().or_else(|| lines.first().copied())
    } else {
        lines.iter().rev().find(|&&l| l < cur_row).copied().or_else(|| lines.last().copied())
    };
    let Some(target) = target else { return };
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = target as usize;
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
    }
}

pub fn open_uri_at(editor: &mut Editor, uri: &str, line: usize, character: usize) {
    let Ok(parsed) = Url::parse(uri) else { return };
    let Ok(path) = parsed.to_file_path() else { return };
    let path: &Path = path.as_path();
    // If a buffer for this path already exists, reuse it.
    let existing = editor
        .buffers
        .iter()
        .find(|(_, b)| b.path() == Some(path))
        .map(|(id, _)| *id);
    let buf_id = match existing {
        Some(id) => id,
        None => match editor.open_path(path) {
            Ok(id) => id,
            Err(e) => {
                editor.status_message = Some(format!("LSP: open {} failed: {e}", path.display()));
                return;
            }
        },
    };
    if let Some(w) = editor.active_window_mut() {
        w.buffer = buf_id;
        w.cursor.row = line;
        w.cursor.col = character;
        w.cursor.sticky_col = character;
        w.left_col = 0;
        // Land the target line in the middle of the window (vim's `zz`
        // after a jump). Without this, large jumps would leave the
        // cursor pinned to the bottom row when `scroll_into_view` runs.
        w.center_on_cursor();
    }
    editor.lsp_did_open(buf_id);
}

// ---- rename prompt ---------------------------------------------------------

/// Enter command mode pre-filled with `LspRename ` so the user can type
/// the new name and press Enter — same feel as vim's `<leader>rn`.
fn rename_prompt(editor: &mut Editor) {
    let _ = editor.take_count();
    editor.command_line.clear();
    editor.command_line.input = "LspRename ".to_string();
    editor.command_line.cursor = editor.command_line.input.len();
    crate::mode::switch_mode(editor, crate::mode::ModeId::Command);
}

//! Applying LSP `WorkspaceEdit` results to the editor.
//!
//! Used by `:LspRename`. The `WorkspaceEdit` shape is a map of
//! `file URI → Vec<TextEdit>`; for each file we either edit the in-memory
//! buffer (preferred — keeps cursor / view sane) or fall back to reading,
//! editing, and writing the file on disk if it isn't currently open.
//!
//! Per file: edits are sorted in REVERSE document order so the byte
//! offsets we compute for later edits stay valid as earlier edits shrink
//! / grow the rope.

use lsp_types::{TextEdit, Url, WorkspaceEdit};

use crate::Editor;

/// Send a `textDocument/rename` request using the active buffer's cursor
/// position. Returns the server's `WorkspaceEdit` (or `None` on failure).
pub fn request_rename(editor: &mut Editor, new_name: &str) -> Option<WorkspaceEdit> {
    let win = editor.active_window()?;
    let buf_id = win.buffer;
    let cur = win.cursor;
    let buf = editor.buffers.get(&buf_id)?;
    let path = buf.path()?;
    let uri = Url::from_file_path(path).ok()?.to_string();
    let filetype = editor.syntax_for(buf_id).filetype.to_string();
    let path_buf = path.to_path_buf();
    // Pick the first server that advertises rename support.
    let client = editor.lsp.client_for(&filetype, &path_buf, |c| c.rename)?;
    client.rename(&uri, cur.row as u32, cur.col as u32, new_name)
}

/// Apply `we` and return how many files were touched.
pub fn apply_workspace_edit(editor: &mut Editor, we: &WorkspaceEdit) -> usize {
    let mut affected = 0;
    if let Some(changes) = &we.changes {
        for (uri, edits) in changes {
            if apply_edits_to_uri(editor, uri, edits) {
                affected += 1;
            }
        }
    }
    // `document_changes` is the newer / richer form. v1 ignores annotations
    // and treats every entry as a plain text edit on a known document.
    if let Some(doc_changes) = &we.document_changes {
        use lsp_types::DocumentChanges;
        match doc_changes {
            DocumentChanges::Edits(edits) => {
                for tde in edits {
                    let uri = &tde.text_document.uri;
                    let edits: Vec<TextEdit> = tde
                        .edits
                        .iter()
                        .filter_map(|e| match e {
                            lsp_types::OneOf::Left(te) => Some(te.clone()),
                            lsp_types::OneOf::Right(annotated) => {
                                Some(annotated.text_edit.clone())
                            }
                        })
                        .collect();
                    if apply_edits_to_uri(editor, uri, &edits) {
                        affected += 1;
                    }
                }
            }
            DocumentChanges::Operations(_) => {
                // Create/rename/delete file ops aren't handled in v1.
            }
        }
    }
    affected
}

fn apply_edits_to_uri(editor: &mut Editor, uri: &Url, edits: &[TextEdit]) -> bool {
    let Ok(path) = uri.to_file_path() else {
        return false;
    };
    // Prefer an already-open buffer over editing the file on disk; that
    // way cursor position, undo history, and unsaved state are preserved.
    let existing = editor
        .buffers
        .iter()
        .find(|(_, b)| b.path() == Some(path.as_path()))
        .map(|(id, _)| *id);
    let buf_id = match existing {
        Some(id) => id,
        None => match editor.open_path(&path) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("lsp rename: open {} failed: {e}", path.display());
                return false;
            }
        },
    };

    // Sort edits in reverse order — process bottom-of-file first so each
    // earlier edit's char index stays valid.
    let mut sorted = edits.to_vec();
    sorted.sort_by(|a, b| {
        let aa = (a.range.start.line, a.range.start.character);
        let bb = (b.range.start.line, b.range.start.character);
        bb.cmp(&aa)
    });

    let buf = match editor.buffers.get_mut(&buf_id) {
        Some(b) => b,
        None => return false,
    };
    buf.begin_transaction();
    for edit in sorted {
        if let Some((start, end)) = lsp_range_to_chars(buf, edit.range) {
            let _ = buf.replace(start..end, &edit.new_text);
        }
    }
    buf.end_transaction();
    true
}

fn lsp_range_to_chars(buf: &crate::buffer::Buffer, range: lsp_types::Range) -> Option<(usize, usize)> {
    let start = lsp_position_to_char(buf, range.start)?;
    let end = lsp_position_to_char(buf, range.end)?;
    Some((start, end))
}

fn lsp_position_to_char(buf: &crate::buffer::Buffer, pos: lsp_types::Position) -> Option<usize> {
    let row = pos.line as usize;
    if row >= buf.line_count() {
        return None;
    }
    let line_start = buf.line_to_char(row);
    let line = buf.line_string(row);
    // LSP positions are in UTF-16 code units. For ASCII text (the common
    // case) this is equivalent to character counts. We approximate by
    // counting Rust chars — close enough for clangd / rust-analyzer on
    // typical source where identifiers are ASCII.
    let mut chars_into_line = 0usize;
    let target = pos.character as usize;
    for c in line.chars() {
        if chars_into_line >= target {
            break;
        }
        chars_into_line += 1;
        let _ = c;
    }
    Some(line_start + chars_into_line)
}

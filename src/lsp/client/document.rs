//! Text-document lifecycle notifications (didOpen/didChange/didSave/
//! didClose) and pull-diagnostics handling.

use lsp_types::notification::Notification;
use lsp_types::{TextDocumentItem, Url, VersionedTextDocumentIdentifier};
use serde_json::Value;

use super::Client;

/// Convert a byte offset in `text` to an LSP [`Position`] (line, character).
/// Character count uses Rust `char` boundaries, which for ASCII-only source
/// matches both UTF-16 code units (the LSP spec) and visual columns.
fn byte_offset_to_position(text: &str, byte_offset: usize) -> lsp_types::Position {
    let prefix = &text[..byte_offset.min(text.len())];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() as u32;
    let last_newline = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let character = prefix[last_newline..].chars().count() as u32;
    lsp_types::Position { line, character }
}

/// Compute an incremental change event between two document texts.
///
/// Finds the smallest contiguous range of bytes that covers all differences
/// between `old` and `new` and returns an LSP [`TextDocumentContentChangeEvent`]
/// appropriate for `TextDocumentSyncKind::Incremental`.
///
/// Returns `None` when the diff is too complex (non-contiguous edits, odd
/// UTF-8 boundaries) — the caller should fall back to full-text sync.
fn compute_incremental_change(old: &str, new: &str) -> Option<lsp_types::TextDocumentContentChangeEvent> {
    if old == new {
        return None;
    }

    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();
    let min_len = old.len().min(new.len());

    // Leading identical prefix
    let first_diff = (0..min_len)
        .find(|&i| old_bytes[i] != new_bytes[i])
        .unwrap_or(min_len);

    // Trailing identical suffix
    let (mut lo, mut ln) = (old.len(), new.len());
    while lo > first_diff && ln > first_diff {
        if old_bytes[lo - 1] != new_bytes[ln - 1] {
            break;
        }
        lo -= 1;
        ln -= 1;
    }

    // Guard against non-UTF-8-safe slice boundaries — fall back to full sync.
    if !old.is_char_boundary(first_diff) || !old.is_char_boundary(lo) {
        return None;
    }
    if !new.is_char_boundary(first_diff) || !new.is_char_boundary(ln) {
        return None;
    }

    let start_pos = byte_offset_to_position(old, first_diff);
    let end_pos = byte_offset_to_position(old, lo);
    let replacement = &new[first_diff..ln];

    Some(lsp_types::TextDocumentContentChangeEvent {
        range: Some(lsp_types::Range { start: start_pos, end: end_pos }),
        range_length: None,
        text: replacement.to_string(),
    })
}

impl Client {
    pub fn did_open(&mut self, uri: &str, language_id: &str, text: &str) {
        if !self.ready {
            return;
        }
        let version = 1;
        self.open_versions.insert(uri.to_string(), version);
        self.open_texts.insert(uri.to_string(), text.to_string());
        // Reset accepted-version tracking so fresh diagnostics from this
        // new open session are never blocked by a counter left from before.
        self.diagnostics.reset_uri(uri);
        tracing::info!("lsp({}): did_open uri={} version={}", self.name, uri, version);
        let Ok(parsed) = Url::parse(uri) else { return };
        let params = lsp_types::DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: parsed,
                language_id: language_id.into(),
                version,
                text: text.into(),
            },
        };
        let _ = self.send_notification(
            lsp_types::notification::DidOpenTextDocument::METHOD,
            serde_json::to_value(params).unwrap(),
        );
        self.pull_diagnostics(uri);
    }

    pub fn did_change(&mut self, uri: &str, new_text: &str) {
        if !self.ready {
            return;
        }
        let Ok(parsed) = Url::parse(uri) else { return };
        let version = {
            let v = self.open_versions.entry(uri.to_string()).or_insert(1);
            *v += 1;
            *v
        };

        // Try incremental sync first: send only the changed range.
        if let Some(old_text) = self.open_texts.get(uri) {
            if let Some(change) = compute_incremental_change(old_text, new_text) {
                let params = lsp_types::DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: parsed,
                        version,
                    },
                    content_changes: vec![change],
                };
                tracing::debug!(
                    "lsp({}): did_change uri={} version={} (incremental)",
                    self.name, uri, version
                );
                let _ = self.send_notification(
                    lsp_types::notification::DidChangeTextDocument::METHOD,
                    serde_json::to_value(params).unwrap(),
                );
                self.open_texts.insert(uri.to_string(), new_text.to_string());
                self.pull_diagnostics(uri);
                return;
            }
        }

        // Fall back to full-text sync if we couldn't compute an incremental
        // change (e.g. initial open, unknown URI, or complex multi-region diff).
        let params = lsp_types::DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: parsed,
                version,
            },
            content_changes: vec![lsp_types::TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: new_text.into(),
            }],
        };
        tracing::info!("lsp({}): did_change uri={} version={}", self.name, uri, version);
        let _ = self.send_notification(
            lsp_types::notification::DidChangeTextDocument::METHOD,
            serde_json::to_value(params).unwrap(),
        );
        self.open_texts.insert(uri.to_string(), new_text.to_string());
        self.pull_diagnostics(uri);
    }

    /// Send a `textDocument/diagnostic` pull request (LSP 3.17). Fire-and-forget:
    /// the id→uri mapping is recorded so `handle` can match the async response.
    /// No-op if the server doesn't support pull diagnostics.
    pub fn pull_diagnostics(&mut self, uri: &str) {
        if !self.ready || !self.capabilities.diagnostic {
            return;
        }
        let Ok(parsed) = Url::parse(uri) else { return };
        let id = self.next_id();
        let params = serde_json::json!({
            "textDocument": { "uri": parsed }
        });
        if self.send_request_raw(id, "textDocument/diagnostic", params).is_ok() {
            self.pending_diagnostics.insert(id, uri.to_string());
        }
    }

    /// Parse a `DocumentDiagnosticReport` response and store its diagnostics.
    pub(super) fn apply_pull_diagnostics(&mut self, uri: &str, result: Value) {
        // RelatedUnchangedDocumentDiagnosticReport: nothing changed, keep current.
        let kind = result.get("kind").and_then(|k| k.as_str()).unwrap_or("full");
        if kind == "unchanged" {
            return;
        }
        if let Some(items) = result.get("items") {
            if let Ok(diags) = serde_json::from_value::<Vec<lsp_types::Diagnostic>>(items.clone()) {
                tracing::info!(
                    "lsp({}): pull diagnostics uri={} count={}",
                    self.name,
                    uri.rsplit('/').next().unwrap_or(uri),
                    diags.len()
                );
                // Pull reports carry no version — pass None (always accepted).
                self.diagnostics.set(uri.to_string(), None, diags);
            }
        }
        // Inter-file dependencies: diagnostics for OTHER documents this edit
        // affected come back under `relatedDocuments`.
        if let Some(related) = result.get("relatedDocuments").and_then(|r| r.as_object()) {
            for (ruri, report) in related {
                if report.get("kind").and_then(|k| k.as_str()) == Some("unchanged") {
                    continue;
                }
                if let Some(items) = report.get("items") {
                    if let Ok(diags) =
                        serde_json::from_value::<Vec<lsp_types::Diagnostic>>(items.clone())
                    {
                        self.diagnostics.set(ruri.clone(), None, diags);
                    }
                }
            }
        }
    }

    pub fn did_save(&mut self, uri: &str) {
        if !self.ready {
            return;
        }
        let Ok(parsed) = Url::parse(uri) else { return };
        let params = lsp_types::DidSaveTextDocumentParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
            text: None,
        };
        let _ = self.send_notification(
            lsp_types::notification::DidSaveTextDocument::METHOD,
            serde_json::to_value(params).unwrap(),
        );
    }

    pub fn did_close(&mut self, uri: &str) {
        if !self.ready {
            return;
        }
        let Ok(parsed) = Url::parse(uri) else { return };
        let params = lsp_types::DidCloseTextDocumentParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
        };
        let _ = self.send_notification(
            lsp_types::notification::DidCloseTextDocument::METHOD,
            serde_json::to_value(params).unwrap(),
        );
        self.open_versions.remove(uri);
        self.open_texts.remove(uri);
    }
}

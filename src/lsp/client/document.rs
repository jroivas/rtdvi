//! Text-document lifecycle notifications (didOpen/didChange/didSave/
//! didClose) and pull-diagnostics handling.

use lsp_types::notification::Notification;
use lsp_types::{TextDocumentItem, Url, VersionedTextDocumentIdentifier};
use serde_json::Value;

use super::Client;

impl Client {
    pub fn did_open(&mut self, uri: &str, language_id: &str, text: &str) {
        if !self.ready {
            return;
        }
        let version = 1;
        self.open_versions.insert(uri.to_string(), version);
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
        // Full-text sync (TextDocumentSyncKind::FULL). Simpler than
        // incremental and works with every server. Optimisable later.
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
    }
}

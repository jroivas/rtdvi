//! Native LSP client.
//!
//! Architecture: one [`Client`] per configured server. A background reader
//! thread continuously parses framed JSON-RPC messages off the server's
//! stdout and pushes them onto an mpsc channel. The main thread writes
//! requests/notifications synchronously to the server's stdin and drains
//! the channel each render tick to consume notifications (mainly
//! diagnostics).
//!
//! Synchronous request flow: `request(method, params, timeout)` writes the
//! request, then polls the channel — handling any notifications that
//! arrive in the meantime — until either the matching response shows up
//! or `timeout` elapses. This blocks the UI for the duration of the
//! request; clangd typically answers `definition` / `hover` in <50 ms.

pub mod client;
pub mod manager;
pub mod transport;

pub use client::{Client, ServerCapabilities};
pub use manager::{LspConfig, Manager};

use lsp_types::Diagnostic;
use std::collections::HashMap;

/// Diagnostics collected from `textDocument/publishDiagnostics`, keyed by
/// the URI the server reported them for.
#[derive(Default, Debug, Clone)]
pub struct DiagnosticStore {
    pub by_uri: HashMap<String, Vec<Diagnostic>>,
}

impl DiagnosticStore {
    pub fn set(&mut self, uri: String, diags: Vec<Diagnostic>) {
        if diags.is_empty() {
            self.by_uri.remove(&uri);
        } else {
            self.by_uri.insert(uri, diags);
        }
    }

    pub fn for_uri(&self, uri: &str) -> &[Diagnostic] {
        self.by_uri.get(uri).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn count(&self) -> usize {
        self.by_uri.values().map(|v| v.len()).sum()
    }
}

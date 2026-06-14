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
use std::path::PathBuf;

/// The common preamble for a position-based LSP request on the active buffer:
/// the file `uri`, the on-disk `path` (servers are picked per path), the
/// cursor `line`/`character`, and the buffer `filetype`. Built by
/// [`crate::Editor::active_lsp_context`].
#[derive(Clone, Debug)]
pub struct RequestContext {
    pub uri: String,
    pub path: PathBuf,
    pub line: u32,
    pub character: u32,
    pub filetype: String,
}

/// Diagnostics collected from `textDocument/publishDiagnostics`, keyed by
/// the URI the server reported them for.
///
/// Versions (LSP 3.15 `publishDiagnostics.version`) are tracked to discard
/// stale notifications. clangd does a fast parse followed by a slower
/// semantic pass and may send two `publishDiagnostics` for the same URI in
/// quick succession; without version filtering the second (possibly emptier)
/// response overwrites the first and errors disappear.
#[derive(Default, Debug, Clone)]
pub struct DiagnosticStore {
    pub by_uri: HashMap<String, Vec<Diagnostic>>,
    /// Highest document version we have accepted diagnostics for, per URI.
    /// `None` means we haven't received any versioned notification yet.
    accepted_version: HashMap<String, i32>,
}

impl DiagnosticStore {
    /// Apply a `publishDiagnostics` notification.
    ///
    /// If `version` is `Some(v)` and `v` is strictly less than the version
    /// we have already accepted for this URI, the notification is silently
    /// dropped — it came from an older analysis pass that lost the race.
    pub fn set(&mut self, uri: String, version: Option<i32>, diags: Vec<Diagnostic>) {
        if let Some(v) = version {
            let accepted = self.accepted_version.get(&uri).copied().unwrap_or(i32::MIN);
            if v < accepted {
                tracing::info!(
                    "diag: dropped stale v={v} (accepted={accepted}) for {}",
                    uri.rsplit('/').next().unwrap_or(&uri)
                );
                return;
            }
            self.accepted_version.insert(uri.clone(), v);
        }
        tracing::info!(
            "diag: stored {} diagnostics (v={:?}) for {}",
            diags.len(),
            version,
            uri.rsplit('/').next().unwrap_or(&uri)
        );
        if diags.is_empty() {
            self.by_uri.remove(&uri);
        } else {
            self.by_uri.insert(uri, diags);
        }
    }

    /// Clear all diagnostics and version state for `uri`.
    /// Call when `didOpen` is sent so stale version counters from a previous
    /// session don't prevent fresh diagnostics from being accepted.
    pub fn reset_uri(&mut self, uri: &str) {
        self.by_uri.remove(uri);
        self.accepted_version.remove(uri);
    }

    pub fn for_uri(&self, uri: &str) -> &[Diagnostic] {
        self.by_uri.get(uri).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn count(&self) -> usize {
        self.by_uri.values().map(|v| v.len()).sum()
    }
}

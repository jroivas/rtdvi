//! One [`Client`] = one connection to a running LSP server.

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use lsp_types::{
    notification::Notification,
    request::Request,
    ClientCapabilities, InitializeParams, InitializeResult,
    PublishDiagnosticsParams, TextDocumentClientCapabilities, TextDocumentItem,
    Url, VersionedTextDocumentIdentifier,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::transport;
use super::DiagnosticStore;

/// LSP messages we parse on the way in. We use [`Value`] for params/results
/// so the same channel can carry every notification and response without
/// pre-committing to a fixed schema.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum InboundMessage {
    Response {
        #[allow(dead_code)]
        jsonrpc: String,
        id: Value,
        #[serde(default)]
        result: Option<Value>,
        #[serde(default)]
        error: Option<Value>,
    },
    Notification {
        #[allow(dead_code)]
        jsonrpc: String,
        method: String,
        #[serde(default)]
        params: Value,
    },
}

#[derive(Debug, Serialize)]
struct OutboundRequest<'a> {
    jsonrpc: &'a str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Serialize)]
struct OutboundNotification<'a> {
    jsonrpc: &'a str,
    method: &'a str,
    params: Value,
}

/// Server-advertised capabilities. We only keep the bits we actually act on.
#[derive(Debug, Clone, Default)]
pub struct ServerCapabilities {
    pub definition: bool,
    pub hover: bool,
    pub references: bool,
}

pub struct Client {
    name: String,
    root_dir: Option<PathBuf>,
    #[allow(dead_code)]
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<InboundMessage>,
    next_id: u64,
    capabilities: ServerCapabilities,
    /// Diagnostics published by the server, keyed by URI.
    pub diagnostics: DiagnosticStore,
    /// Map of URI → last-sent document version, so `didChange` always uses
    /// an incrementing version number.
    open_versions: HashMap<String, i32>,
    /// True after the `initialized` notification has been sent.
    ready: bool,
}

impl Client {
    /// Spawn a server given a command + args and an optional workspace root.
    pub fn spawn(name: &str, cmd: &[String], root_dir: Option<PathBuf>) -> std::io::Result<Self> {
        if cmd.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "lsp: empty command",
            ));
        }
        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (tx, rx) = mpsc::channel::<InboundMessage>();
        let name_owned = name.to_string();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let raw = match transport::read_message(&mut reader) {
                    Ok(r) => r,
                    Err(_) => break,
                };
                let msg: InboundMessage = match serde_json::from_slice(&raw) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("lsp({}): bad message: {e}", name_owned);
                        continue;
                    }
                };
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });

        let mut client = Self {
            name: name.to_string(),
            root_dir,
            child,
            stdin,
            rx,
            next_id: 0,
            capabilities: ServerCapabilities::default(),
            diagnostics: DiagnosticStore::default(),
            open_versions: HashMap::new(),
            ready: false,
        };
        client.initialize()?;
        Ok(client)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn root_dir(&self) -> Option<&Path> {
        self.root_dir.as_deref()
    }
    pub fn capabilities(&self) -> &ServerCapabilities {
        &self.capabilities
    }

    /// Drain any messages the reader thread has queued. Call from the
    /// editor's main loop each tick to pick up async notifications
    /// (`publishDiagnostics`, etc.).
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle(msg);
        }
    }

    fn handle(&mut self, msg: InboundMessage) {
        match msg {
            InboundMessage::Notification { method, params, .. } => {
                if method == lsp_types::notification::PublishDiagnostics::METHOD {
                    if let Ok(p) =
                        serde_json::from_value::<PublishDiagnosticsParams>(params.clone())
                    {
                        self.diagnostics.set(p.uri.to_string(), p.diagnostics);
                    }
                } else {
                    tracing::debug!("lsp({}): notification {method}", self.name);
                }
            }
            InboundMessage::Response { id, result, error, .. } => {
                tracing::debug!(
                    "lsp({}): unsolicited response id={id:?} ok={} err={}",
                    self.name,
                    result.is_some(),
                    error.is_some(),
                );
            }
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn send_raw(&mut self, payload: &[u8]) -> std::io::Result<()> {
        transport::write_message(&mut self.stdin, payload)?;
        self.stdin.flush()
    }

    fn send_request_raw(&mut self, id: u64, method: &str, params: Value) -> std::io::Result<()> {
        let body = OutboundRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let payload = serde_json::to_vec(&body).unwrap();
        self.send_raw(&payload)
    }

    fn send_notification(&mut self, method: &str, params: Value) -> std::io::Result<()> {
        let body = OutboundNotification {
            jsonrpc: "2.0",
            method,
            params,
        };
        let payload = serde_json::to_vec(&body).unwrap();
        self.send_raw(&payload)
    }

    /// Synchronously send a request and block for up to `timeout` waiting
    /// for the matching response. Notifications that arrive in the
    /// meantime are processed normally. Returns the `result` value, or
    /// `None` on timeout/error.
    pub fn request_sync(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Option<Value> {
        let id = self.next_id();
        if let Err(e) = self.send_request_raw(id, method, params) {
            tracing::warn!("lsp({}): send failed: {e}", self.name);
            return None;
        }
        let target = Value::from(id);
        let deadline = Instant::now() + timeout;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            let msg = match self.rx.recv_timeout(remaining) {
                Ok(m) => m,
                Err(_) => break,
            };
            match msg {
                InboundMessage::Response { id: rid, result, error, .. } if rid == target => {
                    if error.is_some() {
                        tracing::warn!(
                            "lsp({}): response error for {method}: {:?}",
                            self.name,
                            error
                        );
                        return None;
                    }
                    return result;
                }
                other => self.handle(other),
            }
        }
        tracing::warn!("lsp({}): request {method} timed out", self.name);
        None
    }

    /// Initialize handshake. Sends `initialize` request and `initialized`
    /// notification. Captures the server's reported capabilities.
    fn initialize(&mut self) -> std::io::Result<()> {
        let root_uri = self
            .root_dir
            .as_ref()
            .and_then(|p| Url::from_file_path(p).ok());
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            #[allow(deprecated)]
            root_uri: root_uri.clone(),
            #[allow(deprecated)]
            root_path: None,
            initialization_options: None,
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities::default()),
                ..Default::default()
            },
            trace: None,
            workspace_folders: None,
            client_info: Some(lsp_types::ClientInfo {
                name: "jvim".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            locale: None,
            work_done_progress_params: Default::default(),
        };
        let result = self.request_sync(
            lsp_types::request::Initialize::METHOD,
            serde_json::to_value(params).unwrap(),
            Duration::from_secs(10),
        );
        if let Some(v) = result {
            if let Ok(parsed) = serde_json::from_value::<InitializeResult>(v) {
                let caps = parsed.capabilities;
                self.capabilities.definition = caps.definition_provider.is_some();
                self.capabilities.hover = caps.hover_provider.is_some();
                self.capabilities.references = caps.references_provider.is_some();
            }
        }
        self.send_notification(
            lsp_types::notification::Initialized::METHOD,
            serde_json::json!({}),
        )?;
        self.ready = true;
        Ok(())
    }

    pub fn did_open(&mut self, uri: &str, language_id: &str, text: &str) {
        if !self.ready {
            return;
        }
        let version = 1;
        self.open_versions.insert(uri.to_string(), version);
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
        let _ = self.send_notification(
            lsp_types::notification::DidChangeTextDocument::METHOD,
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

    /// Request a definition jump. Returns `(uri, line, character)` for the
    /// first location in the response, or `None` if the server didn't
    /// return a useful answer.
    pub fn goto_definition(&mut self, uri: &str, line: u32, character: u32) -> Option<(String, u32, u32)> {
        if !self.capabilities.definition {
            return None;
        }
        let Ok(parsed_uri) = Url::parse(uri) else {
            return None;
        };
        let params = lsp_types::GotoDefinitionParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: parsed_uri },
                position: lsp_types::Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let result = self.request_sync(
            lsp_types::request::GotoDefinition::METHOD,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(1500),
        )?;
        // The response is `Location | Location[] | LocationLink[] | null`.
        // Pick the first.
        if let Ok(loc) = serde_json::from_value::<lsp_types::Location>(result.clone()) {
            return Some((loc.uri.to_string(), loc.range.start.line, loc.range.start.character));
        }
        if let Ok(locs) = serde_json::from_value::<Vec<lsp_types::Location>>(result.clone()) {
            return locs
                .into_iter()
                .next()
                .map(|l| (l.uri.to_string(), l.range.start.line, l.range.start.character));
        }
        if let Ok(links) = serde_json::from_value::<Vec<lsp_types::LocationLink>>(result) {
            return links.into_iter().next().map(|l| {
                (
                    l.target_uri.to_string(),
                    l.target_selection_range.start.line,
                    l.target_selection_range.start.character,
                )
            });
        }
        None
    }

    /// Request hover text. Returns a plain-text excerpt suitable for the
    /// statusline.
    pub fn hover(&mut self, uri: &str, line: u32, character: u32) -> Option<String> {
        if !self.capabilities.hover {
            return None;
        }
        let Ok(parsed) = Url::parse(uri) else { return None };
        let params = lsp_types::HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
                position: lsp_types::Position { line, character },
            },
            work_done_progress_params: Default::default(),
        };
        let result = self.request_sync(
            lsp_types::request::HoverRequest::METHOD,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(1500),
        )?;
        let hover: lsp_types::Hover = serde_json::from_value(result).ok()?;
        Some(hover_text(&hover))
    }

    /// Best-effort shutdown — send `shutdown` request then `exit` notif.
    pub fn shutdown(&mut self) {
        let _ = self.request_sync(
            lsp_types::request::Shutdown::METHOD,
            Value::Null,
            Duration::from_millis(500),
        );
        let _ = self.send_notification(lsp_types::notification::Exit::METHOD, Value::Null);
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Best-effort: ask the server to exit, then reap the child.
        let _ = self.send_notification(lsp_types::notification::Exit::METHOD, Value::Null);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn hover_text(hover: &lsp_types::Hover) -> String {
    use lsp_types::{HoverContents, MarkedString};
    match &hover.contents {
        HoverContents::Scalar(MarkedString::String(s)) => s.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(l)) => l.value.clone(),
        HoverContents::Array(arr) => arr
            .iter()
            .map(|s| match s {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(l) => l.value.clone(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        HoverContents::Markup(m) => m.value.clone(),
    }
}

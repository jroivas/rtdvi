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
    Url, VersionedTextDocumentIdentifier, WorkspaceClientCapabilities, WorkspaceFolder,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::transport;
use super::DiagnosticStore;

/// LSP messages we parse on the way in. We use [`Value`] for params/results
/// so the same channel can carry every notification and response without
/// pre-committing to a fixed schema.
///
/// Order matters for `#[serde(untagged)]`: `ServerRequest` must come before
/// `Response` because both carry an `id` field; the presence of `method`
/// distinguishes them.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum InboundMessage {
    /// Server→client request (e.g. `workspace/configuration`). Has both
    /// `id` and `method`; the client must reply with a matching response.
    ServerRequest {
        #[allow(dead_code)]
        jsonrpc: String,
        id: Value,
        method: String,
        #[serde(default)]
        params: Value,
    },
    /// Response to one of our requests.
    Response {
        #[allow(dead_code)]
        jsonrpc: String,
        id: Value,
        #[serde(default)]
        result: Option<Value>,
        #[serde(default)]
        error: Option<Value>,
    },
    /// Server-initiated notification (no `id`).
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
    pub declaration: bool,
    pub implementation: bool,
    pub type_definition: bool,
    pub rename: bool,
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
    pub fn spawn(
        name: &str,
        cmd: &[String],
        root_dir: Option<PathBuf>,
        init_options: Option<serde_json::Value>,
    ) -> std::io::Result<Self> {
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
        client.initialize(init_options)?;
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
            InboundMessage::ServerRequest { id, method, params, .. } => {
                // The server is asking us something; we must reply or it blocks.
                tracing::debug!("lsp({}): server request {method} id={id:?}", self.name);
                let result = match method.as_str() {
                    // workspace/configuration: return null for every requested
                    // item so the server uses its built-in defaults.
                    "workspace/configuration" => {
                        let n = params
                            .get("items")
                            .and_then(|v| v.as_array())
                            .map(|a| a.len())
                            .unwrap_or(1);
                        Value::Array(vec![Value::Null; n])
                    }
                    // workspace/workspaceFolders: return the folders we know about.
                    "workspace/workspaceFolders" => {
                        match &self.root_dir {
                            Some(p) => {
                                if let Ok(uri) = Url::from_file_path(p) {
                                    let name = p.file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| "workspace".into());
                                    serde_json::json!([{"uri": uri.to_string(), "name": name}])
                                } else {
                                    Value::Null
                                }
                            }
                            None => Value::Null,
                        }
                    }
                    // All other server requests (registerCapability, etc.) → null.
                    _ => Value::Null,
                };
                self.send_response(id, result);
            }
            InboundMessage::Notification { method, params, .. } => {
                if method == lsp_types::notification::PublishDiagnostics::METHOD {
                    if let Ok(p) =
                        serde_json::from_value::<PublishDiagnosticsParams>(params.clone())
                    {
                        let n = p.diagnostics.len();
                        let sevs: Vec<_> = p.diagnostics.iter().map(|d| {
                            match d.severity {
                                Some(lsp_types::DiagnosticSeverity::ERROR) => format!("E@{}", d.range.start.line),
                                Some(lsp_types::DiagnosticSeverity::WARNING) => format!("W@{}", d.range.start.line),
                                Some(lsp_types::DiagnosticSeverity::INFORMATION) => format!("I@{}", d.range.start.line),
                                Some(lsp_types::DiagnosticSeverity::HINT) => format!("H@{}", d.range.start.line),
                                _ => format!("?@{}", d.range.start.line),
                            }
                        }).collect();
                        tracing::info!(
                            "lsp({}): publishDiagnostics uri={} version={:?} count={} sevs={:?}",
                            self.name,
                            p.uri.path(),
                            p.version,
                            n,
                            sevs,
                        );
                        self.diagnostics.set(p.uri.to_string(), p.version, p.diagnostics);
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

    fn send_response(&mut self, id: Value, result: Value) {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        let payload = serde_json::to_vec(&body).unwrap();
        let _ = self.send_raw(&payload);
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
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    tracing::warn!("lsp({}): server exited while waiting for {method}", self.name);
                    return None;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
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
    fn initialize(&mut self, init_options: Option<serde_json::Value>) -> std::io::Result<()> {
        let root_uri = self
            .root_dir
            .as_ref()
            .and_then(|p| Url::from_file_path(p).ok());

        // workspace/configuration and workspaceFolders are both declared so
        // servers like rust-analyzer can request settings and enumerate roots.
        let workspace_folders: Option<Vec<WorkspaceFolder>> =
            root_uri.as_ref().map(|uri| {
                let name = self
                    .root_dir
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "workspace".into());
                vec![WorkspaceFolder { uri: uri.clone(), name }]
            });

        let params = InitializeParams {
            process_id: Some(std::process::id()),
            #[allow(deprecated)]
            root_uri: root_uri.clone(),
            #[allow(deprecated)]
            root_path: None,
            initialization_options: init_options,
            capabilities: ClientCapabilities {
                workspace: Some(WorkspaceClientCapabilities {
                    configuration: Some(true),
                    workspace_folders: Some(true),
                    ..Default::default()
                }),
                text_document: Some(TextDocumentClientCapabilities {
                    synchronization: Some(lsp_types::TextDocumentSyncClientCapabilities {
                        dynamic_registration: Some(false),
                        will_save: Some(false),
                        will_save_wait_until: Some(false),
                        did_save: Some(true),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            trace: None,
            workspace_folders,
            client_info: Some(lsp_types::ClientInfo {
                name: "rtdvi".into(),
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
        let result = result.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                format!("lsp({}): server did not respond to initialize", self.name),
            )
        })?;
        if let Ok(parsed) = serde_json::from_value::<InitializeResult>(result) {
            let caps = parsed.capabilities;
            self.capabilities.definition = caps.definition_provider.is_some();
            self.capabilities.hover = caps.hover_provider.is_some();
            self.capabilities.references = caps.references_provider.is_some();
            self.capabilities.declaration = caps.declaration_provider.is_some();
            self.capabilities.implementation = caps.implementation_provider.is_some();
            self.capabilities.type_definition = caps.type_definition_provider.is_some();
            self.capabilities.rename = caps.rename_provider.is_some();
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

    /// Request a definition jump. Returns `(uri, line, character)` for the
    /// first location in the response, or `None` if the server didn't
    /// return a useful answer.
    pub fn goto_definition(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.definition {
            return Vec::new();
        }
        self.request_all_locations(
            lsp_types::request::GotoDefinition::METHOD,
            uri,
            line,
            character,
        )
    }

    /// Common shape for definition/declaration/implementation/typeDefinition.
    /// All four return `Location | Location[] | LocationLink[]`; we collect
    /// every location so the caller can show a picker when multiple exist.
    fn request_all_locations(
        &mut self,
        method: &'static str,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        let Ok(parsed) = Url::parse(uri) else {
            return Vec::new();
        };
        let params = lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
            position: lsp_types::Position { line, character },
        };
        let Some(result) = self.request_sync(
            method,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(1500),
        ) else {
            return Vec::new();
        };
        all_locations(result)
    }

    pub fn goto_declaration(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.declaration {
            return Vec::new();
        }
        self.request_all_locations(
            lsp_types::request::GotoDeclaration::METHOD,
            uri,
            line,
            character,
        )
    }

    pub fn goto_implementation(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.implementation {
            return Vec::new();
        }
        self.request_all_locations(
            lsp_types::request::GotoImplementation::METHOD,
            uri,
            line,
            character,
        )
    }

    pub fn goto_type_definition(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.type_definition {
            return Vec::new();
        }
        self.request_all_locations(
            lsp_types::request::GotoTypeDefinition::METHOD,
            uri,
            line,
            character,
        )
    }

    /// `textDocument/references`. Returns every location the server
    /// reports (across files). `include_declaration` controls whether the
    /// declaration site itself is part of the result.
    pub fn references(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.references {
            return Vec::new();
        }
        let Ok(parsed) = Url::parse(uri) else {
            return Vec::new();
        };
        let params = lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
                position: lsp_types::Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext { include_declaration },
        };
        let Some(result) = self.request_sync(
            lsp_types::request::References::METHOD,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(2000),
        ) else {
            return Vec::new();
        };
        let locs: Vec<lsp_types::Location> =
            serde_json::from_value(result).unwrap_or_default();
        locs.into_iter()
            .map(|l| (l.uri.to_string(), l.range.start.line, l.range.start.character))
            .collect()
    }

    /// `textDocument/rename`. Returns the resulting `WorkspaceEdit` for
    /// the caller to apply, or `None` if the server refuses or times out.
    pub fn rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Option<lsp_types::WorkspaceEdit> {
        if !self.capabilities.rename {
            return None;
        }
        let Ok(parsed) = Url::parse(uri) else {
            return None;
        };
        let params = lsp_types::RenameParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
                position: lsp_types::Position { line, character },
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        };
        let result = self.request_sync(
            lsp_types::request::Rename::METHOD,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(3000),
        )?;
        serde_json::from_value(result).ok()
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

/// Extract `(uri, line, character)` from any of the three shapes that
/// definition/declaration/implementation/typeDefinition can return.
fn all_locations(result: Value) -> Vec<(String, u32, u32)> {
    if let Ok(loc) = serde_json::from_value::<lsp_types::Location>(result.clone()) {
        return vec![(loc.uri.to_string(), loc.range.start.line, loc.range.start.character)];
    }
    if let Ok(locs) = serde_json::from_value::<Vec<lsp_types::Location>>(result.clone()) {
        return locs
            .into_iter()
            .map(|l| (l.uri.to_string(), l.range.start.line, l.range.start.character))
            .collect();
    }
    if let Ok(links) = serde_json::from_value::<Vec<lsp_types::LocationLink>>(result) {
        return links
            .into_iter()
            .map(|l| {
                (
                    l.target_uri.to_string(),
                    l.target_selection_range.start.line,
                    l.target_selection_range.start.character,
                )
            })
            .collect();
    }
    Vec::new()
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

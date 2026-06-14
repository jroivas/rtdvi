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
    PublishDiagnosticsParams, TextDocumentClientCapabilities,
    Url, WorkspaceClientCapabilities, WorkspaceFolder,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::transport;
mod document;
mod requests;

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
    /// Server supports pull diagnostics (`textDocument/diagnostic`, LSP 3.17).
    /// Modern rust-analyzer only delivers native diagnostics this way.
    pub diagnostic: bool,
    /// Server supports `textDocument/rangeFormatting`. Used by `gq` on code.
    pub range_formatting: bool,
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
    /// In-flight `textDocument/diagnostic` pull requests: request id → URI.
    /// Responses are matched back to their document in `handle`.
    pending_diagnostics: HashMap<u64, String>,
    /// URIs whose pull diagnostics need (re)fetching on the next `poll`. A
    /// pull right after an edit often returns a `ContentModified` /
    /// `ServerCancelled` error ("retrigger"); we re-queue here and retry on
    /// the next tick until the server answers with a real report.
    pull_retry: std::collections::HashSet<String>,
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
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take().expect("stderr");
        let (tx, rx) = mpsc::channel::<InboundMessage>();
        let name_owned = name.to_string();
        // Stderr reader: drain continuously (so the server never blocks on a
        // full stderr pipe) but only surface warnings/errors to the log,
        // so a chatty server can't flood the log file.
        let stderr_name = name.to_string();
        thread::spawn(move || {
            use std::io::BufRead;
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if l.contains("WARN") || l.contains("ERROR") || l.contains("error") {
                            tracing::warn!("lsp({stderr_name}) stderr: {l}");
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let raw = match transport::read_message(&mut reader) {
                    Ok(r) => r,
                    Err(_) => break,
                };
                // Trace inbound message methods at debug (enable with RTDVI_LOG).
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) {
                    let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("<response>");
                    let id = v.get("id").map(|i| i.to_string()).unwrap_or_default();
                    tracing::debug!("lsp({name_owned}) <- method={method} id={id}");
                }
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
            pending_diagnostics: HashMap::new(),
            pull_retry: std::collections::HashSet::new(),
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
        // Retry any pull-diagnostics that the server asked us to retrigger.
        // Only fire for URIs with no in-flight request, so we don't pile up.
        if !self.pull_retry.is_empty() {
            let inflight: std::collections::HashSet<&String> =
                self.pending_diagnostics.values().collect();
            let due: Vec<String> = self
                .pull_retry
                .iter()
                .filter(|u| !inflight.contains(u))
                .cloned()
                .collect();
            for uri in due {
                self.pull_retry.remove(&uri);
                self.pull_diagnostics(&uri);
            }
        }
    }

    fn handle(&mut self, msg: InboundMessage) {
        match msg {
            InboundMessage::ServerRequest { id, method, params, .. } => {
                // The server is asking us something; we must reply or it blocks.
                tracing::info!("lsp({}): server request {method} id={id:?}", self.name);
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
                // After acking a diagnostic-refresh request, re-pull every open
                // document so freshly-computed diagnostics are fetched.
                if method == "workspace/diagnostic/refresh" {
                    let uris: Vec<String> = self.open_versions.keys().cloned().collect();
                    for uri in uris {
                        self.pull_diagnostics(&uri);
                    }
                }
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
                // Is this the response to a pull-diagnostics request we sent?
                if let Some(num) = id.as_u64() {
                    if let Some(uri) = self.pending_diagnostics.remove(&num) {
                        if let Some(result) = result {
                            self.apply_pull_diagnostics(&uri, result);
                        } else if let Some(err) = &error {
                            // ContentModified (-32801) / ServerCancelled (-32802):
                            // the document changed while computing — retrigger.
                            // Re-queue for the next poll tick so we keep asking
                            // until the server returns a real report.
                            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                            tracing::info!(
                                "lsp({}): diagnostic pull error code={code} for {} — retrying",
                                self.name,
                                uri.rsplit('/').next().unwrap_or(&uri)
                            );
                            self.pull_retry.insert(uri);
                        }
                        return;
                    }
                }
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
        // Keep a copy for the post-`initialized` didChangeConfiguration push.
        let init_options_for_config = init_options.clone();
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
                    // Declare publishDiagnostics support (flycheck results are
                    // still pushed this way).
                    publish_diagnostics: Some(lsp_types::PublishDiagnosticsClientCapabilities {
                        related_information: Some(true),
                        version_support: Some(true),
                        tag_support: Some(lsp_types::TagSupport {
                            value_set: vec![
                                lsp_types::DiagnosticTag::UNNECESSARY,
                                lsp_types::DiagnosticTag::DEPRECATED,
                            ],
                        }),
                        code_description_support: Some(true),
                        data_support: Some(true),
                    }),
                    // Declare PULL diagnostics support (LSP 3.17). rust-analyzer
                    // delivers native (syntax/type) diagnostics only via pull —
                    // without this we never see errors on unsaved edits.
                    diagnostic: Some(lsp_types::DiagnosticClientCapabilities {
                        dynamic_registration: Some(false),
                        related_document_support: Some(true),
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
            self.capabilities.diagnostic = caps.diagnostic_provider.is_some();
            self.capabilities.range_formatting =
                caps.document_range_formatting_provider.is_some();
        }
        self.send_notification(
            lsp_types::notification::Initialized::METHOD,
            serde_json::json!({}),
        )?;
        self.ready = true;

        // Push the same options via workspace/didChangeConfiguration. neovim
        // does this and several rust-analyzer settings only fully activate
        // once this notification arrives — initializationOptions alone is not
        // always enough to enable live (non-save) diagnostics.
        if let Some(opts) = init_options_for_config.clone() {
            self.send_notification(
                lsp_types::notification::DidChangeConfiguration::METHOD,
                serde_json::json!({ "settings": opts }),
            )?;
        }
        Ok(())
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


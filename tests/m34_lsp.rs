//! LSP integration test. A small Python script stands in for clangd: it
//! reads framed JSON-RPC messages on stdin and writes responses on stdout.
//! This lets us exercise the whole client pipeline without depending on
//! whether clangd is installed.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use jvim::config::Config;
use jvim::lsp::{Client, LspConfig, Manager};
use jvim::Editor;
use tempfile::{NamedTempFile, TempDir};

/// Write a small Python script that implements just enough of LSP to
/// answer `initialize`, return a definition, and emit a diagnostic for a
/// `didOpen`. Returns the path to the script.
fn mock_lsp_server() -> NamedTempFile {
    let script = r#"#!/usr/bin/env python3
import sys, json, os, time, threading

def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode("utf-8").rstrip("\r\n")
        if line == "":
            break
        if ":" in line:
            k, v = line.split(":", 1)
            headers[k.strip().lower()] = v.strip()
    n = int(headers["content-length"])
    body = sys.stdin.buffer.read(n)
    return json.loads(body)

def write_message(obj):
    data = json.dumps(obj).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(data)}\r\n\r\n".encode())
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()

while True:
    msg = read_message()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "capabilities": {
                    "definitionProvider": True,
                    "hoverProvider": True,
                }
            }
        })
    elif method == "textDocument/didOpen":
        uri = msg["params"]["textDocument"]["uri"]
        # Emit a diagnostic on line 1.
        write_message({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": uri,
                "diagnostics": [{
                    "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 4}},
                    "severity": 1,
                    "message": "mock error",
                }]
            }
        })
    elif method == "textDocument/definition":
        uri = msg["params"]["textDocument"]["uri"]
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "uri": uri,
                "range": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 11}},
            }
        })
    elif method == "textDocument/hover":
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "contents": {"kind": "plaintext", "value": "mock hover content"}
            }
        })
    elif method == "shutdown":
        write_message({"jsonrpc": "2.0", "id": msg["id"], "result": None})
    elif method == "exit":
        break
"#;
    let mut tmp = NamedTempFile::with_suffix(".py").unwrap();
    tmp.write_all(script.as_bytes()).unwrap();
    tmp.flush().unwrap();
    let metadata = std::fs::metadata(tmp.path()).unwrap();
    let mut perms = metadata.permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(tmp.path(), perms).unwrap();
    tmp
}

fn mock_config(script_path: &std::path::Path) -> LspConfig {
    LspConfig {
        name: "mock".into(),
        cmd: vec!["python3".into(), script_path.display().to_string()],
        filetypes: vec!["c".into(), "cpp".into()],
        root_markers: vec![".git".into()],
    }
}

#[test]
fn client_initialize_reports_capabilities() {
    let script = mock_lsp_server();
    let cfg = mock_config(script.path());
    let client = Client::spawn(&cfg.name, &cfg.cmd, None).expect("spawn mock");
    let caps = client.capabilities();
    assert!(caps.definition);
    assert!(caps.hover);
    drop(client); // Drop sends `exit` to the mock.
}

#[test]
fn diagnostics_arrive_via_publish_diagnostics_notification() {
    let script = mock_lsp_server();
    let cfg = mock_config(script.path());
    let mut client = Client::spawn(&cfg.name, &cfg.cmd, None).expect("spawn mock");
    client.did_open("file:///tmp/test.c", "c", "int x;\nbad line\n");

    // The reader thread is async — drain for up to a second.
    let deadline = Instant::now() + Duration::from_secs(2);
    while client.diagnostics.count() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        client.poll();
    }
    assert!(
        client.diagnostics.count() >= 1,
        "no diagnostics arrived within timeout"
    );
}

#[test]
fn manager_spawns_one_client_per_filetype_and_root() {
    let script = mock_lsp_server();
    let cfg = mock_config(script.path());
    let mut mgr = Manager {
        configs: vec![cfg],
        clients: std::collections::HashMap::new(),
    };

    // Two distinct workspaces — should get two clients.
    let ws_a = TempDir::new().unwrap();
    std::fs::create_dir_all(ws_a.path().join(".git")).unwrap();
    std::fs::write(ws_a.path().join("foo.c"), "").unwrap();
    let ws_b = TempDir::new().unwrap();
    std::fs::create_dir_all(ws_b.path().join(".git")).unwrap();
    std::fs::write(ws_b.path().join("foo.c"), "").unwrap();

    let _ = mgr.ensure("c", &ws_a.path().join("foo.c"));
    let _ = mgr.ensure("c", &ws_b.path().join("foo.c"));
    assert_eq!(mgr.clients.len(), 2, "expected two clients for two workspaces");

    // Re-asking for the same workspace shouldn't spawn another one.
    let _ = mgr.ensure("c", &ws_a.path().join("foo.c"));
    assert_eq!(mgr.clients.len(), 2);

    mgr.shutdown_all();
}

#[test]
fn editor_open_path_auto_starts_lsp_and_sends_did_open() {
    // Build an editor and replace its default clangd config with our mock.
    let script = mock_lsp_server();
    let cfg = mock_config(script.path());

    let workspace = TempDir::new().unwrap();
    std::fs::create_dir_all(workspace.path().join(".git")).unwrap();
    let file_path = workspace.path().join("test.c");
    std::fs::write(&file_path, "int main() {\nreturn 0;\n}\n").unwrap();

    let mut editor = Editor::new();
    editor.lsp.configs = vec![cfg];
    let _id = editor.open_path(&file_path).unwrap();

    // Wait for diagnostics to arrive.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        editor.lsp_poll();
        let any_diags = editor
            .lsp
            .clients
            .values()
            .any(|c| c.diagnostics.count() > 0);
        if any_diags {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let total: usize = editor.lsp.clients.values().map(|c| c.diagnostics.count()).sum();
    assert!(total >= 1, "no diagnostics arrived");
}

#[test]
fn gd_action_jumps_to_definition_from_mock() {
    use jvim::keymap::keys::Key;

    let script = mock_lsp_server();
    let cfg = mock_config(script.path());
    let workspace = TempDir::new().unwrap();
    std::fs::create_dir_all(workspace.path().join(".git")).unwrap();
    let file_path = workspace.path().join("test.c");
    std::fs::write(&file_path, "int x = 1;\nint y;\nfoo();\n").unwrap();

    let mut editor = Editor::new();
    editor.lsp.configs = vec![cfg];
    let id = editor.open_path(&file_path).unwrap();
    editor.focus_single(id);

    // Trigger gd. The mock returns row 3 col 7 in the same URI.
    jvim::mode::handle_key(&mut editor, Key::char('g'));
    jvim::mode::handle_key(&mut editor, Key::char('d'));

    let cur = editor.active_window().unwrap().cursor;
    assert_eq!(cur.row, 3);
    assert_eq!(cur.col, 7);
}

#[test]
fn config_lsp_server_block_parses() {
    let toml = r#"
[lsp.clangd]
cmd = ["clangd", "-j=1"]
filetypes = ["c", "cpp"]
root_markers = [".git"]
"#;
    let cfg: Config = toml::from_str(toml).unwrap();
    let entry = cfg.lsp.get("clangd").expect("clangd block");
    assert_eq!(entry.cmd, vec!["clangd", "-j=1"]);
    assert_eq!(entry.filetypes, vec!["c", "cpp"]);
}

#[test]
fn apply_config_replaces_default_clangd() {
    let toml = r#"
[lsp.clangd]
cmd = ["my-custom-clangd"]
filetypes = ["c"]
"#;
    let cfg: Config = toml::from_str(toml).unwrap();
    let mut editor = Editor::new();
    editor.apply_config(cfg);
    let clangd = editor.lsp.configs.iter().find(|c| c.name == "clangd").unwrap();
    assert_eq!(clangd.cmd, vec!["my-custom-clangd"]);
}

#[test]
fn find_root_walks_up_to_marker() {
    let tmp = TempDir::new().unwrap();
    let nested = tmp.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(tmp.path().join("a").join(".git")).unwrap();
    let file = nested.join("foo.c");
    std::fs::write(&file, "").unwrap();
    let root = Manager::find_root(&file, &[".git".into()]).expect("root");
    assert_eq!(root, tmp.path().join("a"));
}

#[test]
fn find_root_returns_none_when_no_marker_present() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("foo.c");
    std::fs::write(&file, "").unwrap();
    let root = Manager::find_root(&file, &[".this-does-not-exist".into()]);
    assert!(root.is_none());
}

// Make `PathBuf` import non-dead.
#[allow(dead_code)]
fn _silence_unused(_: PathBuf) {}

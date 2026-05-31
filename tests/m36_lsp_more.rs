//! Extra LSP coverage: references / declaration / implementation /
//! typeDefinition / rename / diagnostic-at-cursor.

use std::io::Write;
use std::time::{Duration, Instant};

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::lsp::{Client, LspConfig};
use rtdvi::{mode, Editor};
use tempfile::{NamedTempFile, TempDir};

fn mock_lsp_server() -> NamedTempFile {
    let script = r#"#!/usr/bin/env python3
import sys, json

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

def loc(uri, line, char, end_line=None, end_char=None):
    return {
        "uri": uri,
        "range": {
            "start": {"line": line, "character": char},
            "end": {"line": end_line if end_line is not None else line,
                    "character": end_char if end_char is not None else char + 3},
        },
    }

while True:
    msg = read_message()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        write_message({
            "jsonrpc": "2.0", "id": msg["id"],
            "result": {"capabilities": {
                "definitionProvider": True,
                "declarationProvider": True,
                "implementationProvider": True,
                "typeDefinitionProvider": True,
                "referencesProvider": True,
                "hoverProvider": True,
                "renameProvider": True,
            }}
        })
    elif method == "textDocument/didOpen":
        uri = msg["params"]["textDocument"]["uri"]
        write_message({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {"uri": uri, "diagnostics": [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
                "severity": 1, "message": "the diagnostic",
            }]},
        })
    elif method == "textDocument/declaration":
        uri = msg["params"]["textDocument"]["uri"]
        write_message({"jsonrpc": "2.0", "id": msg["id"], "result": loc(uri, 1, 4)})
    elif method == "textDocument/implementation":
        uri = msg["params"]["textDocument"]["uri"]
        write_message({"jsonrpc": "2.0", "id": msg["id"], "result": loc(uri, 2, 6)})
    elif method == "textDocument/typeDefinition":
        uri = msg["params"]["textDocument"]["uri"]
        write_message({"jsonrpc": "2.0", "id": msg["id"], "result": loc(uri, 3, 2)})
    elif method == "textDocument/references":
        uri = msg["params"]["textDocument"]["uri"]
        write_message({"jsonrpc": "2.0", "id": msg["id"],
                       "result": [loc(uri, 5, 0), loc(uri, 6, 0), loc(uri, 7, 0)]})
    elif method == "textDocument/rename":
        uri = msg["params"]["textDocument"]["uri"]
        new_name = msg["params"]["newName"]
        write_message({"jsonrpc": "2.0", "id": msg["id"],
                       "result": {"changes": {uri: [{
                           "range": {"start": {"line": 0, "character": 4},
                                     "end": {"line": 0, "character": 7}},
                           "newText": new_name,
                       }]}}})
    elif method == "textDocument/hover":
        write_message({"jsonrpc": "2.0", "id": msg["id"],
                       "result": {"contents": {"kind": "plaintext", "value": "hover txt"}}})
    elif method == "shutdown":
        write_message({"jsonrpc": "2.0", "id": msg["id"], "result": None})
    elif method == "exit":
        break
"#;
    let mut tmp = NamedTempFile::with_suffix(".py").unwrap();
    tmp.write_all(script.as_bytes()).unwrap();
    tmp.flush().unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(tmp.path()).unwrap().permissions();
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
        init_options: None,
    }
}

fn setup_editor(content: &str) -> (Editor, TempDir, std::path::PathBuf, NamedTempFile) {
    let script = mock_lsp_server();
    let cfg = mock_config(script.path());
    let workspace = TempDir::new().unwrap();
    std::fs::create_dir_all(workspace.path().join(".git")).unwrap();
    let file_path = workspace.path().join("test.c");
    std::fs::write(&file_path, content).unwrap();

    let mut editor = Editor::new();
    editor.lsp.configs = vec![cfg];
    let id = editor.open_path(&file_path).unwrap();
    editor.focus_single(id);
    (editor, workspace, file_path, script)
}

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}

// ---- Capability detection -------------------------------------------------

#[test]
fn client_picks_up_all_new_capabilities() {
    let script = mock_lsp_server();
    let cfg = mock_config(script.path());
    let client = Client::spawn(&cfg.name, &cfg.cmd, None, None).unwrap();
    let c = client.capabilities();
    assert!(c.definition);
    assert!(c.declaration);
    assert!(c.implementation);
    assert!(c.type_definition);
    assert!(c.references);
    assert!(c.hover);
    assert!(c.rename);
}

// ---- Single-location jumps ------------------------------------------------

#[test]
fn capital_gd_jumps_to_declaration() {
    let (mut editor, _ws, _path, _s) = setup_editor("int x;\nint y;\nint z;\n");
    type_keys(&mut editor, "gD");
    let cur = editor.active_window().unwrap().cursor;
    assert_eq!(cur.row, 1);
    assert_eq!(cur.col, 4);
}

#[test]
fn gi_jumps_to_implementation() {
    let (mut editor, _ws, _path, _s) = setup_editor("int x;\nint y;\nint z;\nint w;\n");
    type_keys(&mut editor, "gi");
    let cur = editor.active_window().unwrap().cursor;
    assert_eq!(cur.row, 2);
    assert_eq!(cur.col, 6);
}

#[test]
fn gf_jumps_to_type_definition() {
    let (mut editor, _ws, _path, _s) = setup_editor("int x;\nint y;\nint z;\nint w;\n");
    type_keys(&mut editor, "gf");
    let cur = editor.active_window().unwrap().cursor;
    assert_eq!(cur.row, 3);
    assert_eq!(cur.col, 2);
}

// ---- References -----------------------------------------------------------

#[test]
fn gr_shows_picker_for_multiple_references() {
    let (mut editor, _ws, _path, _s) =
        setup_editor("a\nb\nc\nd\ne\nfoo\nbar\nbaz\n");
    type_keys(&mut editor, "gr");
    // With multiple results the picker should open (no jump yet).
    let picker = editor.lsp_picker.as_ref().expect("picker should be open");
    assert_eq!(picker.locations.len(), 3);
    assert_eq!(picker.selected, 0);
    // Accept the first item with Enter — should jump to row 5, col 0.
    press(&mut editor, KeyCode::Enter);
    assert!(editor.lsp_picker.is_none(), "picker should close after Enter");
    let cur = editor.active_window().unwrap().cursor;
    assert_eq!(cur.row, 5);
    assert_eq!(cur.col, 0);
}

// ---- Diagnostic at cursor -------------------------------------------------

#[test]
fn lsp_diagnostic_command_shows_current_line_diagnostic() {
    let (mut editor, _ws, _path, _s) = setup_editor("int x = 1;\nreturn 0;\n");
    // Wait for the publishDiagnostics to arrive.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        editor.lsp_poll();
        if editor.lsp.clients.values().any(|c| c.diagnostics.count() > 0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    type_keys(&mut editor, ":LspDiagnostic");
    press(&mut editor, KeyCode::Enter);
    let status = editor.status_message.as_deref().unwrap_or("");
    assert!(
        status.contains("the diagnostic"),
        "status: {status:?}"
    );
    assert!(status.starts_with("error"), "status: {status:?}");
}

// ---- Rename ---------------------------------------------------------------

#[test]
fn lsp_rename_replaces_text_in_active_buffer() {
    let (mut editor, _ws, _path, _s) = setup_editor("int foo = 1;\n");
    type_keys(&mut editor, ":LspRename bar");
    press(&mut editor, KeyCode::Enter);
    let id = editor.active_buffer_id().unwrap();
    let text = editor.buffers.get(&id).unwrap().rope().to_string();
    // The mock rewrites chars 4..7 ("foo") with the new name.
    assert_eq!(text, "int bar = 1;\n");
    let status = editor.status_message.as_deref().unwrap_or("");
    assert!(status.contains("renamed"), "status: {status:?}");
}

#[test]
fn lsp_rename_is_single_undo_step() {
    let (mut editor, _ws, _path, _s) = setup_editor("int foo = 1;\n");
    type_keys(&mut editor, ":LspRename bar");
    press(&mut editor, KeyCode::Enter);
    // One `u` reverts the rename.
    mode::handle_key(&mut editor, Key::char('u'));
    let id = editor.active_buffer_id().unwrap();
    let text = editor.buffers.get(&id).unwrap().rope().to_string();
    assert_eq!(text, "int foo = 1;\n");
}

#[test]
fn lsp_rename_requires_new_name() {
    let (mut editor, _ws, _path, _s) = setup_editor("int foo;\n");
    type_keys(&mut editor, ":LspRename");
    press(&mut editor, KeyCode::Enter);
    let status = editor.status_message.as_deref().unwrap_or("");
    assert!(status.contains("usage"), "status: {status:?}");
}

//! `gd` and friends land the target line in the middle of the window
//! instead of at the bottom edge. `zz` / `zt` / `zb` also work directly.

use std::io::Write;
use std::time::{Duration, Instant};

use rtdvi::keymap::keys::Key;
use rtdvi::lsp::LspConfig;
use rtdvi::{mode, Editor};
use tempfile::{NamedTempFile, TempDir};

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}

fn long_content(n: usize) -> String {
    (0..n).map(|i| format!("line {i}\n")).collect()
}

#[test]
fn zz_centres_cursor_in_window() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    tmp.write_all(long_content(200).as_bytes()).unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    // Pretend a render happened so viewport_h is set to a real value.
    {
        let w_id = editor.tabs[0].active;
        editor.windows.get_mut(&w_id).unwrap().viewport_h = 20;
    }
    type_keys(&mut editor, "50G"); // jump to line 50 = row 49
    type_keys(&mut editor, "zz");
    let w = editor.active_window().unwrap();
    // Cursor row 49, viewport 20 → top_line = 49 - 10 = 39.
    assert_eq!(w.top_line, 39, "expected 39, got {}", w.top_line);
}

#[test]
fn zt_puts_cursor_at_top() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    tmp.write_all(long_content(200).as_bytes()).unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    {
        let w_id = editor.tabs[0].active;
        editor.windows.get_mut(&w_id).unwrap().viewport_h = 20;
    }
    type_keys(&mut editor, "50G");
    type_keys(&mut editor, "zt");
    let w = editor.active_window().unwrap();
    assert_eq!(w.top_line, 49); // cursor row IS the top row
}

#[test]
fn zb_puts_cursor_at_bottom() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    tmp.write_all(long_content(200).as_bytes()).unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    {
        let w_id = editor.tabs[0].active;
        editor.windows.get_mut(&w_id).unwrap().viewport_h = 20;
    }
    type_keys(&mut editor, "50G");
    type_keys(&mut editor, "zb");
    let w = editor.active_window().unwrap();
    // Cursor 49, viewport 20 → top_line = 49 - 19 = 30.
    assert_eq!(w.top_line, 30);
}

// ---- counted {n}G / {n}gg centre the target -------------------------------

fn editor_with_lines(n: usize, viewport_h: u16) -> (Editor, NamedTempFile) {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    tmp.write_all(long_content(n).as_bytes()).unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    let w_id = editor.tabs[0].active;
    editor.windows.get_mut(&w_id).unwrap().viewport_h = viewport_h;
    (editor, tmp)
}

#[test]
fn count_g_centres_target() {
    let (mut editor, _f) = editor_with_lines(200, 20);
    type_keys(&mut editor, "100G"); // row 99
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 99);
    // Centered: top_line = 99 - 20/2 = 89.
    assert_eq!(w.top_line, 89, "expected centred top_line 89, got {}", w.top_line);
}

#[test]
fn count_g_near_end_clamps_to_last_page() {
    // 200 lines (rows 0..=199), viewport 20.
    let (mut editor, _f) = editor_with_lines(200, 20);
    type_keys(&mut editor, "198G"); // row 197 — near the end
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 197);
    // Centering (187) would show blank past EOF, so it clamps so the final
    // line (199) sits on the bottom row: top_line = 199 - 19 = 180.
    assert_eq!(w.top_line, 180, "expected clamped top_line 180, got {}", w.top_line);
    // The last line stays visible — no blank space below it.
    assert!(w.top_line + 20 - 1 >= 199);
}

#[test]
fn plain_g_to_last_line_pins_to_bottom() {
    // No count: `G` lands the last line without over-centring (clamped).
    let (mut editor, _f) = editor_with_lines(200, 20);
    // Prime a centred jump first, then plain G.
    type_keys(&mut editor, "100G");
    type_keys(&mut editor, "G"); // last line = row 199
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 199);
}

#[test]
fn count_gg_centres_like_count_g() {
    let (mut editor, _f) = editor_with_lines(200, 20);
    type_keys(&mut editor, "100gg"); // row 99
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 99);
    assert_eq!(w.top_line, 89);
}

// ---- gd via mock LSP centres the target -----------------------------------

fn mock_lsp_server_returning(line: u32) -> NamedTempFile {
    let script = format!(
        r#"#!/usr/bin/env python3
import sys, json
def read_message():
    headers = {{}}
    while True:
        line = sys.stdin.buffer.readline()
        if not line: return None
        line = line.decode("utf-8").rstrip("\r\n")
        if line == "": break
        if ":" in line:
            k, v = line.split(":", 1)
            headers[k.strip().lower()] = v.strip()
    n = int(headers["content-length"])
    return json.loads(sys.stdin.buffer.read(n))
def write_message(obj):
    data = json.dumps(obj).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {{len(data)}}\r\n\r\n".encode())
    sys.stdout.buffer.write(data); sys.stdout.buffer.flush()
TARGET = {line}
while True:
    msg = read_message()
    if msg is None: break
    method = msg.get("method")
    if method == "initialize":
        write_message({{"jsonrpc":"2.0","id":msg["id"],"result":{{"capabilities":{{"definitionProvider":True}}}}}})
    elif method == "textDocument/definition":
        uri = msg["params"]["textDocument"]["uri"]
        write_message({{"jsonrpc":"2.0","id":msg["id"],"result":{{
            "uri": uri,
            "range": {{"start": {{"line": TARGET, "character": 0}},
                       "end":   {{"line": TARGET, "character": 0}}}}}}}})
    elif method == "shutdown":
        write_message({{"jsonrpc":"2.0","id":msg["id"],"result":None}})
    elif method == "exit":
        break
"#,
        line = line
    );
    let mut tmp = NamedTempFile::with_suffix(".py").unwrap();
    tmp.write_all(script.as_bytes()).unwrap();
    tmp.flush().unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(tmp.path()).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(tmp.path(), perms).unwrap();
    tmp
}

#[test]
fn gd_lands_target_in_middle_of_viewport() {
    let script = mock_lsp_server_returning(120);
    let cfg = LspConfig {
        name: "mock".into(),
        cmd: vec!["python3".into(), script.path().display().to_string()],
        filetypes: vec!["c".into()],
        root_markers: vec![".git".into()],
        init_options: None,
    };

    let workspace = TempDir::new().unwrap();
    std::fs::create_dir_all(workspace.path().join(".git")).unwrap();
    let file = workspace.path().join("test.c");
    std::fs::write(&file, long_content(300)).unwrap();

    let mut editor = Editor::new();
    editor.lsp.configs = vec![cfg];
    let id = editor.open_path(&file).unwrap();
    editor.focus_single(id);

    // Give the mock a moment to come up and process didOpen.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        editor.lsp_poll();
        if !editor.lsp.clients.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Set a known viewport height before triggering the jump.
    {
        let w_id = editor.tabs[0].active;
        editor.windows.get_mut(&w_id).unwrap().viewport_h = 30;
    }
    type_keys(&mut editor, "gd");
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 120, "cursor should be at line 120 (the target)");
    // Centered: top_line = 120 - 30/2 = 105.
    assert_eq!(
        w.top_line, 105,
        "expected viewport centred on cursor (top_line=105); got {}",
        w.top_line
    );
}

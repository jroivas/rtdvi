//! `:lsp {restart|status|stop}` — server-management command. These tests
//! don't require a real language server: a scratch buffer has no path so no
//! server is spawned, exercising the empty-state paths.

use rtdvi::command::run_ex_line;
use rtdvi::Editor;

fn fresh() -> Editor {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    editor
}
fn status(editor: &Editor) -> String {
    editor.status_message.clone().unwrap_or_default()
}

#[test]
fn lsp_restart_with_no_servers_reports_none() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "lsp restart");
    assert!(status(&editor).contains("restarted"));
    assert!(status(&editor).contains("no servers"));
    assert!(editor.lsp.clients.is_empty());
}

#[test]
fn lsp_with_no_subcommand_defaults_to_restart() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "lsp");
    assert!(status(&editor).contains("restarted"));
}

#[test]
fn lsp_status_with_no_servers() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "lsp status");
    assert!(status(&editor).contains("no servers running"));
}

#[test]
fn lsp_stop_clears_clients() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "lsp stop");
    assert!(editor.lsp.clients.is_empty());
    assert!(status(&editor).contains("stopped"));
}

#[test]
fn lsp_unknown_subcommand_errors() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "lsp frobnicate");
    // BadArgs surfaces as a status/error message mentioning the bad word.
    assert!(status(&editor).contains("frobnicate") || status(&editor).contains("unknown"));
}

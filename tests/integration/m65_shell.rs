//! `:sh` / `:shell` raise the flag the render loop watches to suspend the
//! editor and drop to an interactive shell.

use rtdvi::command::run_ex_line;
use rtdvi::Editor;

fn fresh() -> Editor {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    editor
}

#[test]
fn sh_sets_pending_shell() {
    let mut editor = fresh();
    assert!(!editor.pending_shell);
    run_ex_line(&mut editor, "sh");
    assert!(editor.pending_shell);
}

#[test]
fn shell_alias_sets_pending_shell() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "shell");
    assert!(editor.pending_shell);
}

#[test]
fn pending_shell_defaults_false() {
    let editor = fresh();
    assert!(!editor.pending_shell);
}

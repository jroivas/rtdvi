//! Generic dotted-path config access: `:set options.tab_width = 8` and
//! `:get options.tab_width`.

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
fn set_dotted_path_number_with_spaces() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "set options.tab_width = 8");
    assert_eq!(editor.config.options.tab_width, 8);
    assert!(status(&editor).contains("options.tab_width = 8"), "{}", status(&editor));
}

#[test]
fn set_dotted_path_number_without_spaces() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "set options.tab_width=3");
    assert_eq!(editor.config.options.tab_width, 3);
}

#[test]
fn set_dotted_path_bool() {
    let mut editor = fresh();
    assert!(!editor.config.options.number);
    run_ex_line(&mut editor, "set options.number = true");
    assert!(editor.config.options.number);
    run_ex_line(&mut editor, "set options.number = off");
    assert!(!editor.config.options.number);
}

#[test]
fn set_dotted_path_string() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "set options.leader = ,");
    assert_eq!(editor.config.options.leader, ",");
}

#[test]
fn get_dotted_path_reports_value() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "set options.tab_width = 8");
    run_ex_line(&mut editor, "get options.tab_width");
    assert_eq!(status(&editor), "options.tab_width = 8");
}

#[test]
fn get_bare_name_falls_back_to_options() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "get tab_width");
    assert_eq!(status(&editor), "options.tab_width = 4"); // default
}

#[test]
fn set_bad_value_is_rejected_and_unchanged() {
    let mut editor = fresh();
    let before = editor.config.options.tab_width;
    run_ex_line(&mut editor, "set options.tab_width = notanumber");
    assert_eq!(editor.config.options.tab_width, before, "config must be unchanged");
    assert!(status(&editor).contains("integer") || status(&editor).contains("number"),
        "got: {}", status(&editor));
}

#[test]
fn set_unknown_path_errors() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "set options.nope = 1");
    assert!(status(&editor).contains("unknown config path"), "got: {}", status(&editor));
}

#[test]
fn set_out_of_range_is_rejected() {
    // tab_width is usize; a negative value must fail deserialization back into
    // Config, leaving the value untouched.
    let mut editor = fresh();
    let before = editor.config.options.tab_width;
    run_ex_line(&mut editor, "set options.tab_width = -1");
    assert_eq!(editor.config.options.tab_width, before);
    assert!(status(&editor).contains("set options.tab_width"), "got: {}", status(&editor));
}

#[test]
fn named_shortcut_still_works() {
    // Bare-name form is unaffected by the dotted-path handling.
    let mut editor = fresh();
    run_ex_line(&mut editor, "set number");
    assert!(editor.config.options.number);
    run_ex_line(&mut editor, "set tab_width=2");
    assert_eq!(editor.config.options.tab_width, 2);
}

//! M48: JSON config support + the `:config` ex command suite
//! (`show`, `path`, `convert` / `conv`, `load`).

use std::io::Write;

use jvim::command::run_ex_line;
use jvim::config::loader::{self, Format};
use jvim::Editor;
use tempfile::NamedTempFile;

// ---- JSON parsing --------------------------------------------------------

#[test]
fn loader_parses_json_file() {
    let mut tmp = NamedTempFile::with_suffix(".json").unwrap();
    tmp.write_all(
        br#"{
            "options": {
                "tab_width": 7,
                "expandtab": false
            }
        }"#,
    )
    .unwrap();
    tmp.flush().unwrap();
    let cfg = loader::load_or_default(tmp.path()).unwrap();
    assert_eq!(cfg.options.tab_width, 7);
    assert!(!cfg.options.expandtab);
}

#[test]
fn loader_rejects_unknown_extension() {
    let mut tmp = NamedTempFile::with_suffix(".yaml").unwrap();
    tmp.write_all(b"options:\n  tab_width: 2\n").unwrap();
    tmp.flush().unwrap();
    assert!(loader::load_or_default(tmp.path()).is_err());
}

#[test]
fn serialize_round_trips_through_both_formats() {
    let mut cfg = jvim::config::Config::default();
    cfg.options.tab_width = 6;
    cfg.options.expandtab = false;

    let toml_text = loader::serialize(&cfg, Format::Toml).unwrap();
    let from_toml = loader::parse(&toml_text, Format::Toml).unwrap();
    assert_eq!(from_toml.options.tab_width, 6);
    assert!(!from_toml.options.expandtab);

    let json_text = loader::serialize(&cfg, Format::Json).unwrap();
    let from_json = loader::parse(&json_text, Format::Json).unwrap();
    assert_eq!(from_json.options.tab_width, 6);
    assert!(!from_json.options.expandtab);
}

// ---- :config show --------------------------------------------------------

fn active_buffer_text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

#[test]
fn config_show_opens_a_new_split_with_the_config() {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    let before = editor.windows.len();
    run_ex_line(&mut editor, "config show");
    // A new window should have appeared (the split).
    assert_eq!(editor.windows.len(), before + 1);
    // The active window now points at the new scratch buffer.
    let text = active_buffer_text(&editor);
    assert!(text.contains("tab_width"), "got buffer: {text}");
    assert!(text.contains("4"), "got buffer: {text}");
}

#[test]
fn config_show_respects_format_argument() {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    run_ex_line(&mut editor, "config show json");
    let text = active_buffer_text(&editor);
    // JSON output uses quoted keys.
    assert!(text.contains("\"tab_width\""), "expected JSON: {text}");
}

#[test]
fn config_show_sets_filetype_for_highlighting() {
    // The scratch buffer should carry a syntax override matching the
    // chosen format so syntax highlighting can pick it up.
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    run_ex_line(&mut editor, "config show json");
    let buf_id = editor.active_buffer_id().unwrap();
    let buf = editor.buffers.get(&buf_id).unwrap();
    assert_eq!(buf.syntax_override(), Some("json"));
}

#[test]
fn config_show_rejects_unknown_format() {
    let mut editor = Editor::new();
    run_ex_line(&mut editor, "config show xml");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains("unknown format"), "got: {msg}");
}

// ---- :config path --------------------------------------------------------

#[test]
fn config_path_with_defaults_says_no_file_loaded() {
    let mut editor = Editor::new();
    run_ex_line(&mut editor, "config path");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains("loaded:"), "got: {msg}");
    assert!(msg.contains("defaults") || msg.contains("<defaults"),
            "should mention defaults: {msg}");
    assert!(msg.contains("searched:"), "got: {msg}");
}

#[test]
fn config_path_shows_loaded_file() {
    let mut tmp = NamedTempFile::with_suffix(".toml").unwrap();
    tmp.write_all(b"[options]\ntab_width = 8\n").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    editor.config_path = Some(tmp.path().to_path_buf());
    run_ex_line(&mut editor, "config path");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains(&tmp.path().display().to_string()),
            "should include loaded path: {msg}");
}

// ---- :config convert / conv ---------------------------------------------

#[test]
fn config_convert_writes_json_file() {
    let tmp = NamedTempFile::with_suffix(".json").unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp); // we want the path, not a file handle yet
    let mut editor = Editor::new();
    editor.config.options.tab_width = 9;
    run_ex_line(&mut editor, &format!("config convert json {}", path.display()));
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("\"tab_width\""), "expected JSON: {body}");
    assert!(body.contains("9"), "value missing: {body}");
}

#[test]
fn config_conv_is_alias_for_convert() {
    let tmp = NamedTempFile::with_suffix(".toml").unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    let mut editor = Editor::new();
    editor.config.options.tab_width = 3;
    run_ex_line(&mut editor, &format!("config conv toml {}", path.display()));
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("tab_width"), "got: {body}");
    assert!(body.contains("3"), "got: {body}");
}

#[test]
fn config_convert_swaps_extension_of_loaded_path_by_default() {
    // Loaded `config.toml` somewhere; `:config convert json` should write
    // `config.json` next to it.
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("config.toml");
    std::fs::write(&toml_path, "[options]\ntab_width = 5\n").unwrap();

    let mut editor = Editor::new();
    editor.config = loader::load_or_default(&toml_path).unwrap();
    editor.config_path = Some(toml_path.clone());

    run_ex_line(&mut editor, "config convert json");
    let expected = toml_path.with_extension("json");
    assert!(expected.exists(), "expected file at {}", expected.display());
    let body = std::fs::read_to_string(&expected).unwrap();
    assert!(body.contains("\"tab_width\""), "got: {body}");
}

#[test]
fn config_convert_rejects_unknown_format() {
    let mut editor = Editor::new();
    run_ex_line(&mut editor, "config convert xml");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains("unknown format"), "got: {msg}");
}

// ---- :config load --------------------------------------------------------

#[test]
fn config_load_applies_new_file() {
    let mut tmp = NamedTempFile::with_suffix(".json").unwrap();
    tmp.write_all(br#"{"options": {"tab_width": 11}}"#).unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    assert_eq!(editor.config.options.tab_width, 4); // default
    run_ex_line(&mut editor, &format!("config load {}", tmp.path().display()));
    assert_eq!(editor.config.options.tab_width, 11);
    assert_eq!(editor.config_path.as_deref(), Some(tmp.path()));
}

#[test]
fn config_load_with_no_arg_reloads_current_path() {
    let mut tmp = NamedTempFile::with_suffix(".toml").unwrap();
    tmp.write_all(b"[options]\ntab_width = 2\n").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    editor.config_path = Some(tmp.path().to_path_buf());
    editor.config = loader::load_or_default(tmp.path()).unwrap();
    assert_eq!(editor.config.options.tab_width, 2);

    // Edit the file externally.
    std::fs::write(tmp.path(), b"[options]\ntab_width = 13\n").unwrap();
    run_ex_line(&mut editor, "config load");
    assert_eq!(editor.config.options.tab_width, 13);
}

#[test]
fn config_load_without_path_or_loaded_errors() {
    let mut editor = Editor::new();
    run_ex_line(&mut editor, "config load");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains("no config"), "got: {msg}");
}

// ---- Dispatch sanity -----------------------------------------------------

#[test]
fn bare_config_command_reports_usage() {
    let mut editor = Editor::new();
    run_ex_line(&mut editor, "config");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains("usage"), "got: {msg}");
}

#[test]
fn config_unknown_subcommand_errors() {
    let mut editor = Editor::new();
    run_ex_line(&mut editor, "config wiggle");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains("unknown sub-command"), "got: {msg}");
}

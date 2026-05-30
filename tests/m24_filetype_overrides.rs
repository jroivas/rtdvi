//! Filetype overrides: TOML config globs, MIME type translation, vim alias
//! normalisation, and `:set syntax=…` manual override.

use std::io::Write;

use rtdvi::config::Config;
use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::syntax::{detect_filetype_for, normalize_filetype, FiletypeOverrides};
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}

// ---- normalize_filetype ---------------------------------------------------

#[test]
fn vim_alias_c_plus_plus_normalizes_to_cpp() {
    assert_eq!(normalize_filetype("c++"), "cpp");
    assert_eq!(normalize_filetype("cpp"), "cpp");
    assert_eq!(normalize_filetype("CPP"), "cpp");
}

#[test]
fn mime_text_markdown_normalizes_to_markdown() {
    assert_eq!(normalize_filetype("text/markdown"), "markdown");
    assert_eq!(normalize_filetype("text/x-c++src"), "cpp");
    assert_eq!(normalize_filetype("text/x-rust"), "rust");
    assert_eq!(normalize_filetype("application/json"), "json");
}

#[test]
fn unrecognised_strings_pass_through_lowercased() {
    assert_eq!(normalize_filetype("Strange"), "strange");
    assert_eq!(normalize_filetype("application/x-magic"), "application/x-magic");
}

// ---- FiletypeOverrides glob matching --------------------------------------

#[test]
fn glob_overrides_match_by_basename() {
    let mut map = std::collections::HashMap::new();
    map.insert("*.cpp".to_string(), "c++".to_string());
    map.insert("*.md".to_string(), "text/markdown".to_string());
    let ov = FiletypeOverrides::from_map(&map);
    assert_eq!(
        detect_filetype_for(std::path::Path::new("/tmp/foo.cpp"), &ov),
        "cpp"
    );
    assert_eq!(
        detect_filetype_for(std::path::Path::new("readme.md"), &ov),
        "markdown"
    );
}

#[test]
fn longer_glob_beats_shorter_glob() {
    let mut map = std::collections::HashMap::new();
    map.insert("*.toml".to_string(), "toml".to_string());
    map.insert("Cargo.toml".to_string(), "rust".to_string());
    let ov = FiletypeOverrides::from_map(&map);
    assert_eq!(
        detect_filetype_for(std::path::Path::new("Cargo.toml"), &ov),
        "rust"
    );
    assert_eq!(
        detect_filetype_for(std::path::Path::new("config.toml"), &ov),
        "toml"
    );
}

#[test]
fn builtin_detection_still_works_without_overrides() {
    let ov = FiletypeOverrides::default();
    assert_eq!(
        detect_filetype_for(std::path::Path::new("foo.rs"), &ov),
        "rust"
    );
    assert_eq!(
        detect_filetype_for(std::path::Path::new("Cargo.toml"), &ov),
        "toml"
    );
}

#[test]
fn mime_guess_fallback_covers_extra_extensions() {
    // Lots of extensions live in mime_guess's table that aren't in rtdvi's
    // built-in (e.g. `.xml`). It returns an `application/xml` which we may
    // not have a direct mapping for — but the test ensures the path
    // doesn't crash and returns *something*.
    let ov = FiletypeOverrides::default();
    let ft = detect_filetype_for(std::path::Path::new("foo.xml"), &ov);
    assert!(!ft.is_empty());
}

// ---- Config integration ---------------------------------------------------

#[test]
fn config_filetypes_applied_via_apply_config() {
    let toml = r#"
[filetypes]
"*.cpp" = "c++"
"*.md" = "text/markdown"
"#;
    let cfg: Config = toml::from_str(toml).unwrap();
    let mut editor = Editor::new();
    editor.apply_config(cfg);
    assert_eq!(editor.config.filetypes.get("*.cpp"), Some(&"c++".to_string()));
}

#[test]
fn editor_syntax_for_uses_config_overrides() {
    // Build an editor with a config that maps *.foo -> rust.
    let toml = r#"
[filetypes]
"*.foo" = "rust"
"#;
    let cfg: Config = toml::from_str(toml).unwrap();
    let mut editor = Editor::new();
    editor.apply_config(cfg);
    // Save a buffer to a file with the .foo extension.
    let mut tmp = NamedTempFile::with_suffix(".foo").unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    tmp.flush().unwrap();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);

    let syn = editor.syntax_for(id);
    assert_eq!(syn.filetype, "rust");
}

// ---- :set syntax override --------------------------------------------------

#[test]
fn set_syntax_overrides_per_buffer() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "let x = 1;").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    // Default detection -> rust.
    assert_eq!(editor.syntax_for(id).filetype, "rust");
    // Override to c.
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.syntax_for(id).filetype, "c");
}

#[test]
fn set_ft_alias_works() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "x").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    type_keys(&mut editor, ":set ft=python");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.syntax_for(id).filetype, "python");
}

#[test]
fn set_syntax_accepts_vim_alias() {
    let mut tmp = NamedTempFile::with_suffix(".txt").unwrap();
    writeln!(tmp, "x").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    // `c++` is the user-facing name; we normalise to cpp internally.
    type_keys(&mut editor, ":set syntax=c++");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.syntax_for(id).filetype, "cpp");
}

#[test]
fn set_syntax_accepts_mime_type() {
    let mut tmp = NamedTempFile::with_suffix(".txt").unwrap();
    writeln!(tmp, "x").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    type_keys(&mut editor, ":set syntax=text/markdown");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.syntax_for(id).filetype, "markdown");
}

#[test]
fn set_syntax_off_clears_override() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "x").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.syntax_for(id).filetype, "c");
    type_keys(&mut editor, ":set syntax=off");
    press(&mut editor, KeyCode::Enter);
    // Falls back to extension-based detection.
    assert_eq!(editor.syntax_for(id).filetype, "rust");
}

#[test]
fn set_unknown_option_just_reports_message() {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    type_keys(&mut editor, ":set wibble=42");
    press(&mut editor, KeyCode::Enter);
    assert!(
        editor.status_message.as_deref().unwrap_or("").contains("ignoring"),
        "got: {:?}",
        editor.status_message
    );
}

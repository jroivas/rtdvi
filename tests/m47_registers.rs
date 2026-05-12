//! Named registers via the `"<letter>` prefix and the special
//! system-clipboard register (`q` by default). The system-clipboard
//! flow is exercised through a fake `clipboard_copy_cmd` / `_paste_cmd`
//! pair that pipes into / out of a tempfile, so the test doesn't
//! depend on wl-copy / xclip being installed.

use std::io::Write;

use jvim::config::Config;
use jvim::keymap::keys::{Key, KeyCode};
use jvim::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}

fn open(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn buffer_text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

// ---- Named register basics -----------------------------------------------

#[test]
fn yank_to_named_register_stores_separately_from_unnamed() {
    let (mut editor, _f) = open("alpha\nbeta\n");
    // "ayy yanks current line into register a.
    type_keys(&mut editor, "\"ayy");
    let a = editor.named_registers.get(&'a').cloned();
    assert!(a.is_some(), "register a should be populated");
    assert_eq!(a.unwrap().text, "alpha\n");
    // Unnamed register also got the content (vim convention).
    assert_eq!(editor.unnamed_register.text, "alpha\n");

    // Move to next line and yank into b.
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "\"byy");
    assert_eq!(
        editor.named_registers.get(&'b').unwrap().text,
        "beta\n"
    );
    // Register a is untouched.
    assert_eq!(
        editor.named_registers.get(&'a').unwrap().text,
        "alpha\n"
    );
}

#[test]
fn paste_from_named_register() {
    let (mut editor, _f) = open("alpha\nbeta\n");
    type_keys(&mut editor, "\"ayy"); // a = "alpha\n"
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "\"byy"); // b = "beta\n"
    // Move back to top and paste from a.
    type_keys(&mut editor, "gg");
    type_keys(&mut editor, "\"ap"); // line-wise paste of "alpha\n" below line 0
    assert_eq!(buffer_text(&editor), "alpha\nalpha\nbeta\n");

    // Cursor is now on the newly inserted "alpha" at line 1.
    // Pasting b below that lands beta on line 2, giving two of each.
    type_keys(&mut editor, "\"bp");
    assert_eq!(buffer_text(&editor), "alpha\nalpha\nbeta\nbeta\n");
}

#[test]
fn plain_p_still_uses_unnamed_after_named_yank() {
    // After "ayy the unnamed register also got updated; a plain p
    // pastes from it.
    let (mut editor, _f) = open("alpha\nbeta\n");
    type_keys(&mut editor, "\"ayy");
    type_keys(&mut editor, "p");
    assert_eq!(buffer_text(&editor), "alpha\nalpha\nbeta\n");
}

#[test]
fn delete_into_named_register() {
    let (mut editor, _f) = open("foo\nbar\nbaz\n");
    type_keys(&mut editor, "\"add"); // delete current line into a
    assert_eq!(buffer_text(&editor), "bar\nbaz\n");
    assert_eq!(editor.named_registers.get(&'a').unwrap().text, "foo\n");
}

#[test]
fn register_prefix_is_one_shot() {
    // After "ayy, the next plain yy should NOT keep writing to a.
    let (mut editor, _f) = open("alpha\nbeta\n");
    type_keys(&mut editor, "\"ayy"); // a = alpha
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "yy"); // unnamed only — a untouched
    assert_eq!(editor.unnamed_register.text, "beta\n");
    assert_eq!(editor.named_registers.get(&'a').unwrap().text, "alpha\n");
}

#[test]
fn unknown_prefix_letter_cancels_cleanly() {
    let (mut editor, _f) = open("alpha\n");
    type_keys(&mut editor, "\""); // arm
    press(&mut editor, KeyCode::Esc); // bail
    // Now plain yy should still work and target unnamed.
    type_keys(&mut editor, "yy");
    assert_eq!(editor.unnamed_register.text, "alpha\n");
    assert!(editor.named_registers.is_empty());
}

#[test]
fn visual_mode_yank_into_named_register() {
    let (mut editor, _f) = open("alpha\nbeta\n");
    // V to enter visual-line, "ay to yank into a.
    type_keys(&mut editor, "V");
    type_keys(&mut editor, "\"ay");
    assert_eq!(editor.named_registers.get(&'a').unwrap().text, "alpha\n");
}

// ---- System clipboard register --------------------------------------------
//
// We stub the clipboard with `tee` (copy) and `cat` (paste) over a
// tempfile, so the test runs on any Unix without depending on wl-copy.

fn make_clipboard_config(path: &std::path::Path) -> Config {
    // Write a snippet of TOML that points clipboard_copy_cmd at
    // `sh -c "cat > <path>"` and clipboard_paste_cmd at `cat <path>`.
    // Using `sh -c` keeps the command list simple.
    let s = format!(
        r#"
[options]
clipboard_copy_cmd  = ["sh", "-c", "cat > {p}"]
clipboard_paste_cmd = ["cat", "{p}"]
"#,
        p = path.display()
    );
    toml::from_str(&s).unwrap()
}

#[test]
fn yank_to_q_writes_to_system_clipboard() {
    let clip = NamedTempFile::new().unwrap();
    let (mut editor, _f) = open("alpha\nbeta\n");
    editor.apply_config(make_clipboard_config(clip.path()));

    type_keys(&mut editor, "\"qyy"); // yank line 1 to "clipboard"

    let contents = std::fs::read_to_string(clip.path()).unwrap();
    assert_eq!(contents, "alpha\n");

    // And q should NOT be in the named_registers map — it bypasses
    // in-memory storage entirely.
    assert!(editor.named_registers.get(&'q').is_none());

    // Unnamed register still got it (so plain `p` still works locally).
    assert_eq!(editor.unnamed_register.text, "alpha\n");
}

#[test]
fn paste_from_q_reads_system_clipboard() {
    let clip = NamedTempFile::new().unwrap();
    std::fs::write(clip.path(), "from-the-outside\n").unwrap();
    let (mut editor, _f) = open("alpha\n");
    editor.apply_config(make_clipboard_config(clip.path()));

    type_keys(&mut editor, "\"qp");
    // Linewise (trailing newline) so it pastes below current line.
    assert_eq!(buffer_text(&editor), "alpha\nfrom-the-outside\n");
}

#[test]
fn configurable_register_letter() {
    let clip = NamedTempFile::new().unwrap();
    let s = format!(
        r#"
[options]
system_clipboard_register = "c"
clipboard_copy_cmd  = ["sh", "-c", "cat > {p}"]
clipboard_paste_cmd = ["cat", "{p}"]
"#,
        p = clip.path().display()
    );
    let cfg: Config = toml::from_str(&s).unwrap();
    let (mut editor, _f) = open("hello\n");
    editor.apply_config(cfg);
    type_keys(&mut editor, "\"cyy");
    let contents = std::fs::read_to_string(clip.path()).unwrap();
    assert_eq!(contents, "hello\n");
    // The default register letter 'q' is no longer special — it
    // becomes a normal in-memory named register.
    type_keys(&mut editor, "\"qyy");
    assert!(editor.named_registers.get(&'q').is_some());
}

#[test]
fn uppercase_config_letter_works_too() {
    // User wrote `system_clipboard_register = "C"`. The runtime
    // comparison should fold case so `"cyy` still routes externally.
    let clip = NamedTempFile::new().unwrap();
    let s = format!(
        r#"
[options]
system_clipboard_register = "C"
clipboard_copy_cmd  = ["sh", "-c", "cat > {p}"]
clipboard_paste_cmd = ["cat", "{p}"]
"#,
        p = clip.path().display()
    );
    let cfg: Config = toml::from_str(&s).unwrap();
    let (mut editor, _f) = open("hello\n");
    editor.apply_config(cfg);
    type_keys(&mut editor, "\"cyy");
    assert_eq!(std::fs::read_to_string(clip.path()).unwrap(), "hello\n");
}

#[test]
fn disabling_via_space_treats_q_as_normal_register() {
    let s = r#"
[options]
system_clipboard_register = " "
"#;
    let cfg: Config = toml::from_str(s).unwrap();
    let (mut editor, _f) = open("hello\n");
    editor.apply_config(cfg);
    type_keys(&mut editor, "\"qyy");
    // With the integration disabled, q becomes a normal in-memory slot.
    assert!(editor.named_registers.get(&'q').is_some());
    assert_eq!(
        editor.named_registers.get(&'q').unwrap().text,
        "hello\n"
    );
}

#[test]
fn missing_clipboard_tool_surfaces_status_message() {
    let s = r#"
[options]
clipboard_copy_cmd = ["this-command-does-not-exist-jvim"]
"#;
    let cfg: Config = toml::from_str(s).unwrap();
    let (mut editor, _f) = open("hello\n");
    editor.apply_config(cfg);
    type_keys(&mut editor, "\"qyy");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains("clipboard"), "got status: {msg:?}");
}

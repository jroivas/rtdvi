//! Yank / paste registers.
//!
//! Two layers:
//!
//! - **In-memory named registers** (`a`..`z`). Selected with `"<letter>`
//!   before an operator: `"ayy`, `"add`, `"ap`. The unnamed register
//!   (`"" `) is the destination when no prefix is typed and the implicit
//!   source of plain `p`.
//! - **System clipboard** — one configurable register letter
//!   (default `q`) routes yank/paste through `wl-copy`/`wl-paste`
//!   (Wayland), `xclip` (X11), or `pbcopy`/`pbpaste` (macOS). No new
//!   library deps; we shell out.
//!
//! The pre-trie hook `try_consume_key` intercepts `"<letter>` so the
//! keymap never sees the prefix, which keeps the register selection
//! orthogonal to every existing key binding.
//!
//! `store` / `read_for_paste` are the two public funnels — every
//! yank/delete/paste action goes through them so register routing
//! lives in exactly one place.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::config::Config;
use crate::editor::Register;
use crate::keymap::{Key, KeyCode};
use crate::Editor;

/// State of the `"<letter>` prefix.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pending {
    /// No prefix active.
    #[default]
    None,
    /// User typed `"`; next keystroke is the register letter (or cancel).
    Awaiting,
    /// Register selected; next operator/paste uses it.
    Selected(char),
}

#[derive(Default, Debug)]
pub struct Registers {
    /// `a`..`z` — explicit named registers, in-memory only.
    pub named: HashMap<char, Register>,
    /// Current `"<letter>` prefix state.
    pub pending: Pending,
}

impl Registers {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Pre-trie hook. Returns `true` when the key has been consumed by the
/// register-prefix machinery (either started the prefix or supplied
/// the letter), `false` to let the normal key handler proceed.
///
/// Gated to start ONLY when the trie is idle — no half-typed operator
/// (`d` waiting for a motion), so `"` inside a custom binding can still
/// be matched as a literal `"`.
pub fn try_consume_key(editor: &mut Editor, key: Key) -> bool {
    match editor.registers.pending {
        Pending::Awaiting => {
            if let KeyCode::Char(c) = key.code {
                if c.is_ascii_alphabetic() && key.mods.is_empty() {
                    editor.registers.pending = Pending::Selected(c.to_ascii_lowercase());
                    return true;
                }
            }
            // Anything else — bail out of the prefix without firing it.
            editor.registers.pending = Pending::None;
            editor.status_message = Some("register: cancelled".into());
            return true;
        }
        Pending::None | Pending::Selected(_) => {}
    }
    // `"` at a clean dispatch point starts a register prefix. Don't
    // interpose if the user is mid-sequence — e.g. inside a custom
    // binding that contains a literal `"`.
    if matches!(key.code, KeyCode::Char('"'))
        && key.mods.is_empty()
        && editor.pending_keys.is_empty()
    {
        editor.registers.pending = Pending::Awaiting;
        return true;
    }
    false
}

/// Consume the current `Selected` register, returning the letter. After
/// this call, `pending` is `None`. Call once per operator execution.
pub fn take_pending(editor: &mut Editor) -> Option<char> {
    match std::mem::replace(&mut editor.registers.pending, Pending::None) {
        Pending::Selected(c) => Some(c),
        _ => None,
    }
}

/// Yank / delete writer. The unnamed register is **always** updated
/// (vim behaviour). When a named register is selected, the content
/// also goes there — or, for the system-clipboard register, gets piped
/// to the external tool.
pub fn store(editor: &mut Editor, text: String, linewise: bool) {
    let entry = Register {
        text: text.clone(),
        linewise,
    };
    let pending = take_pending(editor);
    let sys_reg = editor.config.options.system_clipboard_register;
    if let Some(name) = pending {
        if name == sys_reg && sys_reg != ' ' {
            if let Err(msg) = write_system_clipboard(&editor.config, &text) {
                editor.status_message = Some(format!("clipboard: {msg}"));
            } else {
                editor.status_message = Some(format!("yanked → system clipboard ({} bytes)", text.len()));
            }
        } else {
            editor.named_registers.insert(name, entry.clone());
        }
    }
    editor.unnamed_register = entry;
}

/// Paste reader. Returns the register content the next paste should
/// use, consuming the `"<letter>` prefix if one was set.
pub fn read_for_paste(editor: &mut Editor) -> Register {
    let pending = take_pending(editor);
    let sys_reg = editor.config.options.system_clipboard_register;
    match pending {
        None => editor.unnamed_register.clone(),
        Some(name) if name == sys_reg && sys_reg != ' ' => match read_system_clipboard(&editor.config) {
            Ok(text) => {
                // Heuristic: trailing newline ⇒ line-wise paste, matching
                // vim's `*` / `+` semantics.
                let linewise = text.ends_with('\n');
                Register { text, linewise }
            }
            Err(msg) => {
                editor.status_message = Some(format!("clipboard: {msg}"));
                Register::default()
            }
        },
        Some(name) => editor
            .named_registers
            .get(&name)
            .cloned()
            .unwrap_or_default(),
    }
}

// ---- System clipboard plumbing -------------------------------------------

fn detect_copy_cmd() -> Vec<String> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return vec!["wl-copy".into()];
    }
    if std::env::var("DISPLAY").is_ok() {
        return vec![
            "xclip".into(),
            "-selection".into(),
            "clipboard".into(),
        ];
    }
    if cfg!(target_os = "macos") {
        return vec!["pbcopy".into()];
    }
    vec!["wl-copy".into()]
}

fn detect_paste_cmd() -> Vec<String> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return vec!["wl-paste".into(), "--no-newline".into()];
    }
    if std::env::var("DISPLAY").is_ok() {
        return vec![
            "xclip".into(),
            "-selection".into(),
            "clipboard".into(),
            "-o".into(),
        ];
    }
    if cfg!(target_os = "macos") {
        return vec!["pbpaste".into()];
    }
    vec!["wl-paste".into(), "--no-newline".into()]
}

fn write_system_clipboard(config: &Config, text: &str) -> Result<(), String> {
    let cmd = config
        .options
        .clipboard_copy_cmd
        .clone()
        .unwrap_or_else(detect_copy_cmd);
    let (program, args) = cmd
        .split_first()
        .ok_or_else(|| "clipboard_copy_cmd is empty".to_string())?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {program}: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("write {program}: {e}"))?;
    }
    let status = child
        .wait()
        .map_err(|e| format!("wait {program}: {e}"))?;
    if !status.success() {
        return Err(format!("{program} exited {status}"));
    }
    Ok(())
}

fn read_system_clipboard(config: &Config) -> Result<String, String> {
    let cmd = config
        .options
        .clipboard_paste_cmd
        .clone()
        .unwrap_or_else(detect_paste_cmd);
    let (program, args) = cmd
        .split_first()
        .ok_or_else(|| "clipboard_paste_cmd is empty".to_string())?;
    let output = Command::new(program)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("spawn {program}: {e}"))?;
    if !output.status.success() {
        return Err(format!("{program} exited {}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("utf8: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_default_is_none() {
        let r = Registers::new();
        assert!(matches!(r.pending, Pending::None));
        assert!(r.named.is_empty());
    }
}

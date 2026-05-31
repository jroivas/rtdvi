//! User configuration.
//!
//! Two sections: `[options]` (runtime knobs like `tab_width`) and a list of
//! `[[keymaps]]` entries that bind sequences to action names. Both are
//! applied at startup by [`crate::Editor::apply_config`].
//!
//! The on-disk format is **TOML or JSON** — picked by file extension. See
//! [`loader`] for the discovery rules and `:config` for runtime tooling
//! (show / path / convert / load).

pub mod loader;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub keymaps: Vec<KeymapEntry>,
    /// Glob-pattern → filetype/MIME mappings. Each glob is matched against
    /// the basename of the buffer's path. Right-hand side may be either a
    /// short type name (e.g. `c++`, `rust`) or a MIME type (`text/markdown`).
    /// Tried before the built-in detector.
    #[serde(default)]
    pub filetypes: std::collections::HashMap<String, String>,
    /// `[lsp.NAME]` blocks — one per language server. Each block carries
    /// the command, claimed filetypes, and workspace-root markers.
    #[serde(default)]
    pub lsp: std::collections::HashMap<String, LspServerConfig>,
    /// Plugins to load at startup. Each entry is either a bare name string
    /// or a table with a `name` key and arbitrary option fields.
    #[serde(default)]
    pub plugins: Vec<crate::plugin::config::PluginEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LspServerConfig {
    pub cmd: Vec<String>,
    #[serde(default)]
    pub filetypes: Vec<String>,
    #[serde(default = "default_root_markers")]
    pub root_markers: Vec<String>,
}

fn default_root_markers() -> Vec<String> {
    vec![".git".into()]
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Options {
    #[serde(default = "default_tab_width")]
    pub tab_width: usize,
    /// When true, pressing Tab in insert mode inserts spaces up to the
    /// next multiple of `tab_width`. When false, inserts a literal `\t`.
    /// Shift+Tab always inserts a literal `\t` regardless of this flag.
    /// Default: `true`.
    #[serde(default = "default_expandtab")]
    pub expandtab: bool,
    /// Copy the indentation of the previous line when opening a new line
    /// (Enter, `o`, `O`). Default: `true`.
    #[serde(default = "default_true")]
    pub autoindent: bool,
    /// Language-aware smart indent on top of `autoindent`: adds an extra
    /// level after `{`, control keywords (`if`/`for`/`while`/…), and
    /// Python/Lua `:` / `then`; auto-dedents `{` and `}` on blank lines;
    /// snaps Backspace to the previous tab stop when in leading whitespace.
    /// Requires `autoindent = true` to have any effect. Default: `true`.
    #[serde(default = "default_true")]
    pub smartindent: bool,
    #[serde(default = "default_number")]
    pub number: bool,
    /// The leader key — `<leader>` in user keymap entries expands to
    /// this character. Vim's default is `\`. Set to e.g. `","` or
    /// `" "` to taste.
    #[serde(default = "default_leader")]
    pub leader: String,
    /// Paint trailing whitespace cells (the run of spaces/tabs after
    /// the last non-whitespace char on a line) with a red background.
    /// Default `false`.
    #[serde(default)]
    pub highlight_trailing_whitespace: bool,
    /// Paint every tab character anywhere in the line with a red
    /// background. Useful for spaces-only projects where stray tabs
    /// are bugs. Default `false`.
    #[serde(default)]
    pub highlight_tabs: bool,
    /// The register letter that routes yank/paste through the system
    /// clipboard instead of an in-memory slot. `"<this>yy` copies the
    /// current line to the OS clipboard; `"<this>p` pastes from it.
    /// Default: `'q'` (vim's `q` is for macros, which we don't have).
    /// Set to a different letter, or `' '` to disable the integration.
    #[serde(default = "default_system_clipboard_register")]
    pub system_clipboard_register: char,
    /// Command and args that copy stdin to the system clipboard.
    /// `None` means auto-detect at runtime: `wl-copy` under Wayland,
    /// `xclip -selection clipboard` under X11, `pbcopy` on macOS.
    #[serde(default)]
    pub clipboard_copy_cmd: Option<Vec<String>>,
    /// Command and args that print the system clipboard to stdout.
    /// `None` means auto-detect (`wl-paste --no-newline`, `xclip -o`,
    /// `pbpaste`).
    #[serde(default)]
    pub clipboard_paste_cmd: Option<Vec<String>>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            tab_width: default_tab_width(),
            expandtab: default_expandtab(),
            autoindent: default_true(),
            smartindent: default_true(),
            number: default_number(),
            leader: default_leader(),
            highlight_trailing_whitespace: false,
            highlight_tabs: false,
            system_clipboard_register: default_system_clipboard_register(),
            clipboard_copy_cmd: None,
            clipboard_paste_cmd: None,
        }
    }
}

fn default_system_clipboard_register() -> char {
    'q'
}

fn default_true() -> bool {
    true
}
fn default_tab_width() -> usize {
    4
}
fn default_expandtab() -> bool {
    true
}
fn default_number() -> bool {
    false
}
fn default_leader() -> String {
    "\\".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeymapEntry {
    pub mode: String,
    pub keys: String,
    pub action: String,
}

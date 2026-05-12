//! TOML configuration.
//!
//! Two sections: `[options]` (runtime knobs like `tab_width`) and a list of
//! `[[keymaps]]` entries that bind sequences to action names. Both are
//! applied at startup by [`crate::Editor::apply_config`].

pub mod loader;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
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
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct Options {
    #[serde(default = "default_tab_width")]
    pub tab_width: usize,
    #[serde(default = "default_expandtab")]
    pub expandtab: bool,
    #[serde(default = "default_number")]
    pub number: bool,
    /// The leader key — `<leader>` in user keymap entries expands to
    /// this character. Vim's default is `\`. Set to e.g. `","` or
    /// `" "` to taste.
    #[serde(default = "default_leader")]
    pub leader: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            tab_width: default_tab_width(),
            expandtab: default_expandtab(),
            number: default_number(),
            leader: default_leader(),
        }
    }
}

fn default_tab_width() -> usize {
    4
}
fn default_expandtab() -> bool {
    false
}
fn default_number() -> bool {
    false
}
fn default_leader() -> String {
    "\\".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeymapEntry {
    pub mode: String,
    pub keys: String,
    pub action: String,
}

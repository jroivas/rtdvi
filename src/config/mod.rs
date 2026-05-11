//! TOML configuration. Stub — implemented in M10.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub keymaps: Vec<KeymapEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Options {
    #[serde(default = "default_tab_width")]
    pub tab_width: usize,
    #[serde(default = "default_expandtab")]
    pub expandtab: bool,
    #[serde(default = "default_number")]
    pub number: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            tab_width: default_tab_width(),
            expandtab: default_expandtab(),
            number: default_number(),
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

#[derive(Debug, Clone, Deserialize)]
pub struct KeymapEntry {
    pub mode: String,
    pub keys: String,
    pub action: String,
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One entry in the `plugins = [...]` config array.
/// Either a plain name string or a table with a `name` key plus arbitrary options.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PluginEntry {
    Simple(String),
    Table(PluginTableEntry),
}

impl PluginEntry {
    pub fn name(&self) -> &str {
        match self {
            PluginEntry::Simple(n) => n,
            PluginEntry::Table(t) => &t.name,
        }
    }

    pub fn options(&self) -> HashMap<String, String> {
        match self {
            PluginEntry::Simple(_) => HashMap::new(),
            PluginEntry::Table(t) => t.options.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginTableEntry {
    pub name: String,
    #[serde(flatten)]
    pub options: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_string_entry() {
        let toml = r#"plugins = ["wordcount"]"#;
        #[derive(Deserialize)]
        struct Cfg { plugins: Vec<PluginEntry> }
        let cfg: Cfg = toml::from_str(toml).unwrap();
        assert_eq!(cfg.plugins[0].name(), "wordcount");
        assert!(cfg.plugins[0].options().is_empty());
    }

    #[test]
    fn table_entry_with_options() {
        let toml = r#"plugins = [{name = "formatter", cmd = "rustfmt", on_save = "true"}]"#;
        #[derive(Deserialize)]
        struct Cfg { plugins: Vec<PluginEntry> }
        let cfg: Cfg = toml::from_str(toml).unwrap();
        assert_eq!(cfg.plugins[0].name(), "formatter");
        assert_eq!(cfg.plugins[0].options().get("cmd").unwrap(), "rustfmt");
    }

    #[test]
    fn mixed_array() {
        let toml = r#"plugins = ["a", {name = "b", x = "1"}]"#;
        #[derive(Deserialize)]
        struct Cfg { plugins: Vec<PluginEntry> }
        let cfg: Cfg = toml::from_str(toml).unwrap();
        assert_eq!(cfg.plugins[0].name(), "a");
        assert_eq!(cfg.plugins[1].name(), "b");
    }
}

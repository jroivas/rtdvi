//! Owns one [`Client`] per server config and matches buffers to clients.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::Client;

#[derive(Debug, Clone)]
pub struct LspConfig {
    /// Logical name (`"clangd"`, `"rust-analyzer"`, …). Also keys the
    /// running-clients map.
    pub name: String,
    /// Program + args, e.g. `["clangd", "-j=2", "--background-index"]`.
    pub cmd: Vec<String>,
    /// Filetypes this server claims. Buffer's filetype must match for the
    /// server to be auto-started.
    pub filetypes: Vec<String>,
    /// Files / directories that, when found by walking up from the buffer's
    /// path, identify the workspace root. Defaults to `.git`.
    pub root_markers: Vec<String>,
}

impl LspConfig {
    /// Built-in default for clangd. Returned when the user hasn't supplied
    /// their own `[lsp.clangd]` block.
    pub fn clangd_default() -> Self {
        Self {
            name: "clangd".into(),
            cmd: vec![
                "clangd".into(),
                "-j=2".into(),
                "--background-index".into(),
                "--background-index-priority=low".into(),
                "--malloc-trim".into(),
                "--pch-storage=disk".into(),
            ],
            filetypes: vec!["c".into(), "cpp".into(), "objc".into(), "objcpp".into()],
            root_markers: vec![
                ".git".into(),
                "compile_commands.json".into(),
                "compile_flags.txt".into(),
            ],
        }
    }
}

#[derive(Default)]
pub struct Manager {
    pub configs: Vec<LspConfig>,
    /// Active clients keyed by `(server_name, root_dir)` so two buffers
    /// from the same workspace share one server.
    pub clients: HashMap<(String, Option<PathBuf>), Client>,
}

impl Manager {
    pub fn new() -> Self {
        Self {
            configs: vec![LspConfig::clangd_default()],
            clients: HashMap::new(),
        }
    }

    /// Replace `configs` with what came in from TOML. Default clangd config
    /// is kept ONLY if the user didn't define their own.
    pub fn apply_user_configs(&mut self, user: Vec<LspConfig>) {
        let mut merged = Vec::new();
        let mut have_clangd = false;
        for cfg in user {
            if cfg.name == "clangd" {
                have_clangd = true;
            }
            merged.push(cfg);
        }
        if !have_clangd {
            merged.push(LspConfig::clangd_default());
        }
        self.configs = merged;
    }

    /// Find the first config that claims `filetype`.
    pub fn config_for_filetype(&self, filetype: &str) -> Option<&LspConfig> {
        self.configs
            .iter()
            .find(|c| c.filetypes.iter().any(|f| f == filetype))
    }

    /// Walk up from `path` looking for any of the named root markers.
    pub fn find_root(path: &Path, markers: &[String]) -> Option<PathBuf> {
        let mut cur = if path.is_dir() {
            Some(path.to_path_buf())
        } else {
            path.parent().map(|p| p.to_path_buf())
        };
        while let Some(dir) = cur {
            for m in markers {
                if dir.join(m).exists() {
                    return Some(dir);
                }
            }
            cur = dir.parent().map(|p| p.to_path_buf());
        }
        None
    }

    /// Ensure a client is running for the given filetype + path. Spawns
    /// one if necessary; otherwise returns the existing one. Returns
    /// `None` if no config claims this filetype.
    pub fn ensure(&mut self, filetype: &str, path: &Path) -> Option<&mut Client> {
        let cfg = self.config_for_filetype(filetype)?.clone();
        let root = Self::find_root(path, &cfg.root_markers);
        let key = (cfg.name.clone(), root.clone());
        if !self.clients.contains_key(&key) {
            match Client::spawn(&cfg.name, &cfg.cmd, root.clone()) {
                Ok(c) => {
                    self.clients.insert(key.clone(), c);
                }
                Err(e) => {
                    tracing::warn!("lsp: spawn {} failed: {e}", cfg.name);
                    return None;
                }
            }
        }
        self.clients.get_mut(&key)
    }

    /// Find an already-running client for `filetype` + `path`. Doesn't
    /// spawn anything.
    pub fn find_for(&mut self, filetype: &str, path: &Path) -> Option<&mut Client> {
        let cfg = self.config_for_filetype(filetype)?;
        let root = Self::find_root(path, &cfg.root_markers);
        let key = (cfg.name.clone(), root);
        self.clients.get_mut(&key)
    }

    /// Drain pending messages from every running client. Call once per
    /// editor tick so diagnostics get picked up.
    pub fn poll_all(&mut self) {
        for client in self.clients.values_mut() {
            client.poll();
        }
    }

    pub fn shutdown_all(&mut self) {
        for client in self.clients.values_mut() {
            client.shutdown();
        }
        self.clients.clear();
    }
}

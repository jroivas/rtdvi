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
    /// Optional `initializationOptions` sent verbatim in the LSP `initialize`
    /// request. Server-specific; `None` sends no options.
    pub init_options: Option<serde_json::Value>,
}

impl LspConfig {
    /// Built-in default for clangd. Returned when the user hasn't supplied
    /// their own `[lsp.clangd]` block.
    pub fn clangd_default() -> Self {
        let mut cmd: Vec<String> = vec![
            "clangd".into(),
            "-j=2".into(),
            "--background-index".into(),
            "--background-index-priority=low".into(),
        ];
        // `--malloc-trim` releases freed memory back to the OS via glibc's
        // `malloc_trim()`. clangd on macOS is built without it (no glibc), so
        // passing the flag makes it exit with an error — omit it there.
        if !cfg!(target_os = "macos") {
            cmd.push("--malloc-trim".into());
        }
        cmd.push("--pch-storage=disk".into());
        Self {
            name: "clangd".into(),
            cmd,
            filetypes: vec!["c".into(), "cpp".into(), "objc".into(), "objcpp".into()],
            root_markers: vec![
                ".git".into(),
                "compile_commands.json".into(),
                "compile_flags.txt".into(),
            ],
            init_options: None,
        }
    }

    /// Built-in default for rust-analyzer.
    pub fn rust_analyzer_default() -> Self {
        Self {
            name: "rust-analyzer".into(),
            cmd: vec!["rust-analyzer".into()],
            filetypes: vec!["rust".into()],
            root_markers: vec!["Cargo.toml".into(), ".git".into()],
            // `diagnostics.experimental.enable` makes rust-analyzer compute
            // extra diagnostics in-memory; `checkOnSave` keeps cargo-check
            // diagnostics flowing. Both together is the combination that
            // actually produced live diagnostics in testing.
            init_options: Some(serde_json::json!({
                "diagnostics": {
                    "enable": true,
                    "experimental": { "enable": true }
                },
                "checkOnSave": { "enable": true },
                "check": { "command": "check" }
            })),
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
            configs: vec![LspConfig::clangd_default(), LspConfig::rust_analyzer_default()],
            clients: HashMap::new(),
        }
    }

    /// Replace `configs` with what came in from TOML. Built-in defaults for
    /// clangd and rust-analyzer are kept unless the user defined their own.
    pub fn apply_user_configs(&mut self, user: Vec<LspConfig>) {
        let mut merged = Vec::new();
        let mut have_clangd = false;
        let mut have_ra = false;
        for cfg in user {
            match cfg.name.as_str() {
                "clangd" => have_clangd = true,
                "rust-analyzer" => have_ra = true,
                _ => {}
            }
            merged.push(cfg);
        }
        if !have_clangd {
            merged.push(LspConfig::clangd_default());
        }
        if !have_ra {
            merged.push(LspConfig::rust_analyzer_default());
        }
        self.configs = merged;
    }

    /// All configs that claim `filetype`, in config order.
    pub fn configs_for_filetype(&self, filetype: &str) -> Vec<&LspConfig> {
        self.configs
            .iter()
            .filter(|c| c.filetypes.iter().any(|f| f == filetype))
            .collect()
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

    /// The `(name, root)` client keys for every config claiming `filetype`
    /// at `path`. Order follows config order.
    fn keys_for(&self, filetype: &str, path: &Path) -> Vec<(String, Option<PathBuf>)> {
        self.configs_for_filetype(filetype)
            .into_iter()
            .map(|c| (c.name.clone(), Self::find_root(path, &c.root_markers)))
            .collect()
    }

    /// Spawn (if not already running) every server that claims `filetype`
    /// for `path`. Servers whose binary is missing or that fail to start are
    /// skipped silently — so configuring pyright+ruff but only installing one
    /// just runs the installed one, and installing both runs both.
    pub fn ensure_all(&mut self, filetype: &str, path: &Path) {
        let matching: Vec<LspConfig> = self
            .configs_for_filetype(filetype)
            .into_iter()
            .cloned()
            .collect();
        for cfg in matching {
            let root = Self::find_root(path, &cfg.root_markers);
            let key = (cfg.name.clone(), root.clone());
            if self.clients.contains_key(&key) {
                continue;
            }
            match Client::spawn(&cfg.name, &cfg.cmd, root.clone(), cfg.init_options.clone()) {
                Ok(c) => {
                    self.clients.insert(key, c);
                }
                Err(e) => {
                    // Missing binary / spawn failure: skip this server, keep
                    // any others. The editor keeps working without it.
                    tracing::warn!("lsp: spawn {} failed: {e}", cfg.name);
                }
            }
        }
    }

    /// All running clients claiming `filetype` for `path`. Used to broadcast
    /// `did_open` / `did_change` / `did_save` to every matching server.
    pub fn clients_for(&mut self, filetype: &str, path: &Path) -> Vec<&mut Client> {
        let keys: std::collections::HashSet<(String, Option<PathBuf>)> =
            self.keys_for(filetype, path).into_iter().collect();
        self.clients
            .iter_mut()
            .filter(|(k, _)| keys.contains(*k))
            .map(|(_, c)| c)
            .collect()
    }

    /// The first running client claiming `filetype` for `path` whose
    /// capabilities satisfy `pred`. Used for single-answer requests
    /// (`gd`, `hover`, `references`, `rename`) so a lint-only server with no
    /// `definitionProvider` is skipped in favour of one that supports it.
    pub fn client_for<F>(&mut self, filetype: &str, path: &Path, pred: F) -> Option<&mut Client>
    where
        F: Fn(&crate::lsp::ServerCapabilities) -> bool,
    {
        let keys = self.keys_for(filetype, path);
        let chosen = keys
            .into_iter()
            .find(|k| self.clients.get(k).is_some_and(|c| pred(c.capabilities())))?;
        self.clients.get_mut(&chosen)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clangd_malloc_trim_gated_on_platform() {
        let cmd = LspConfig::clangd_default().cmd;
        let has_trim = cmd.iter().any(|a| a == "--malloc-trim");
        if cfg!(target_os = "macos") {
            assert!(!has_trim, "--malloc-trim must not be passed to clangd on macOS");
        } else {
            assert!(has_trim, "--malloc-trim should be passed to clangd on non-macOS");
        }
        // The rest of the baseline flags are always present.
        assert_eq!(cmd.first().map(String::as_str), Some("clangd"));
        assert!(cmd.iter().any(|a| a == "--pch-storage=disk"));
    }
}

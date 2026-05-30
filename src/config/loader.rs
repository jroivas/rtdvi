//! Discover, parse, and serialise the user's config.
//!
//! Two on-disk formats are supported: **TOML** (preferred / canonical)
//! and **JSON**. Format is picked from the file extension. At startup
//! rtdvi looks for `config.toml` first, then `config.json`, at each of
//! the standard locations — so the user can choose either without any
//! per-system config flag.

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::Config;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("toml emit: {0}")]
    TomlEmit(#[from] toml::ser::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

/// On-disk serialisation format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Toml,
    Json,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Toml => "toml",
            Format::Json => "json",
        }
    }

    pub fn from_path(p: &Path) -> Option<Self> {
        match p.extension().and_then(|e| e.to_str())?.to_ascii_lowercase().as_str() {
            "toml" => Some(Format::Toml),
            "json" => Some(Format::Json),
            _ => None,
        }
    }

    /// Accept user-typed names like `toml`, `TOML`, `json`, `JSON`.
    pub fn parse_name(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "toml" => Some(Format::Toml),
            "json" => Some(Format::Json),
            _ => None,
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.extension())
    }
}

/// The ordered list of locations rtdvi looks at on startup. Each entry
/// is "config.<ext>" at one of:
///
/// 1. `$RTDVI_CONFIG` (verbatim — single entry; extension drives format)
/// 2. `$XDG_CONFIG_HOME/rtdvi/config.{toml,json}`
/// 3. `$HOME/.config/rtdvi/config.{toml,json}`
///
/// At a given directory `.toml` is checked before `.json`, so a user
/// with both files gets TOML.
pub fn search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("RTDVI_CONFIG") {
        out.push(PathBuf::from(p));
        return out;
    }
    let mut dirs = Vec::new();
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        dirs.push(PathBuf::from(x).join("rtdvi"));
    }
    if let Ok(h) = std::env::var("HOME") {
        dirs.push(PathBuf::from(h).join(".config").join("rtdvi"));
    }
    for d in &dirs {
        out.push(d.join("config.toml"));
        out.push(d.join("config.json"));
    }
    out
}

/// First existing entry from [`search_paths`].
pub fn find_existing() -> Option<(PathBuf, Format)> {
    for p in search_paths() {
        if p.exists() {
            let fmt = Format::from_path(&p).unwrap_or(Format::Toml);
            return Some((p, fmt));
        }
    }
    None
}

/// **Deprecated alias** — returns the preferred default *write* path
/// (TOML at the canonical location). Kept so existing callers compile.
/// New code should use [`search_paths`] / [`find_existing`].
pub fn default_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RTDVI_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("rtdvi").join("config.toml"))
}

/// Parse the given text in the given format.
pub fn parse(text: &str, format: Format) -> Result<Config, ConfigError> {
    match format {
        Format::Toml => Ok(toml::from_str(text)?),
        Format::Json => Ok(serde_json::from_str(text)?),
    }
}

/// Serialise the config to a string in the given format. TOML output
/// uses the pretty multi-line form; JSON output is indented.
pub fn serialize(config: &Config, format: Format) -> Result<String, ConfigError> {
    match format {
        Format::Toml => Ok(toml::to_string_pretty(config)?),
        Format::Json => Ok(serde_json::to_string_pretty(config)?),
    }
}

/// Load and parse the file at `path`. Format is inferred from the
/// extension; an unknown extension is rejected. Returns
/// `Ok(Config::default())` if the file doesn't exist (no config = defaults).
pub fn load_or_default(path: &Path) -> Result<Config, ConfigError> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let format = Format::from_path(path).ok_or_else(|| {
        ConfigError::Other(format!(
            "config: cannot infer format from extension of {}",
            path.display()
        ))
    })?;
    let text = std::fs::read_to_string(path)?;
    parse(&text, format)
}

/// Write `config` to `path` in the format inferred from its extension.
/// Creates the parent directory if needed.
pub fn write_to_path(config: &Config, path: &Path) -> Result<(), ConfigError> {
    let format = Format::from_path(path).ok_or_else(|| {
        ConfigError::Other(format!(
            "config: cannot infer format from extension of {}",
            path.display()
        ))
    })?;
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let text = serialize(config, format)?;
    std::fs::write(path, text)?;
    Ok(())
}

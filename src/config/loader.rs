//! Discover and load the TOML config.

use std::path::PathBuf;

use thiserror::Error;

use super::Config;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),
}

/// Return the user's config path:
/// 1. `$JVIM_CONFIG` if set,
/// 2. else `$XDG_CONFIG_HOME/jvim/config.toml` if `XDG_CONFIG_HOME` is set,
/// 3. else `$HOME/.config/jvim/config.toml`.
pub fn default_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("JVIM_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("jvim").join("config.toml"))
}

/// Load and parse a config file. Returns `Ok(Config::default())` if the path
/// does not exist (no config = defaults).
pub fn load_or_default(path: &std::path::Path) -> Result<Config, ConfigError> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(path)?;
    let cfg: Config = toml::from_str(&text)?;
    Ok(cfg)
}

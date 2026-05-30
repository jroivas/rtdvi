use std::path::PathBuf;

/// Resolve the filesystem path for a plugin by name.
///
/// Search order:
///   1. `$RTDVI_PLUGIN_DIR/<name>.wasm`
///   2. `$HOME/.local/rtdvi/plugins/<name>.wasm`
///   3. `./<name>.wasm` (fallback — useful during development)
pub fn plugin_path(name: &str) -> PathBuf {
    if let Ok(dir) = std::env::var("RTDVI_PLUGIN_DIR") {
        return PathBuf::from(dir).join(format!("{name}.wasm"));
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("rtdvi")
            .join("plugins")
            .join(format!("{name}.wasm"));
    }
    PathBuf::from(format!("{name}.wasm"))
}

/// Resolve the filesystem path for a plugin with an arbitrary extension.
pub fn plugin_path_with_ext(name: &str, ext: &str) -> PathBuf {
    if let Ok(dir) = std::env::var("RTDVI_PLUGIN_DIR") {
        return PathBuf::from(dir).join(format!("{name}{ext}"));
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("rtdvi")
            .join("plugins")
            .join(format!("{name}{ext}"));
    }
    PathBuf::from(format!("{name}{ext}"))
}

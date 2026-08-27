//! `:config` — show / load / convert the user config.

use std::path::PathBuf;
use crate::command::{ArgCompletion, CommandError, ExArgs, ExCommand};
use crate::Editor;

pub(super) struct ConfigCmd;

impl ExCommand for ConfigCmd {
    fn name(&self) -> &'static str {
        "config"
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        use crate::config::loader::{self, Format};
        let Some(sub) = args.words.first().map(|s| s.to_string()) else {
            return Err(CommandError::BadArgs(
                "usage: :config {show|path|convert|conv|load|reload} [args]".into(),
            ));
        };
        let rest: Vec<String> = args.words.iter().skip(1).cloned().collect();
        match sub.as_str() {
            "show" => {
                let fmt = match rest.first() {
                    Some(s) => Format::parse_name(s).ok_or_else(|| {
                        CommandError::BadArgs(format!(
                            "config show: unknown format {s:?} (expected toml/json)"
                        ))
                    })?,
                    None => editor
                        .config_path
                        .as_deref()
                        .and_then(Format::from_path)
                        .unwrap_or(Format::Toml),
                };
                let text = loader::serialize(&editor.config, fmt)
                    .map_err(|e| CommandError::Failed(format!("config show: {e}")))?;
                open_config_view(editor, &text, fmt);
            }
            "path" => {
                let mut out = String::new();
                match editor.config_path.as_ref() {
                    Some(p) => out.push_str(&format!("loaded: {}\n", p.display())),
                    None => out.push_str("loaded: <defaults — no config file found>\n"),
                }
                out.push_str("searched:\n");
                for p in loader::search_paths() {
                    out.push_str(&format!(
                        "  {} ({})\n",
                        p.display(),
                        if p.exists() { "exists" } else { "missing" }
                    ));
                }
                editor.status_message = Some(out.trim_end().to_string());
            }
            "convert" | "conv" => {
                let Some(fmt_arg) = rest.first() else {
                    return Err(CommandError::BadArgs(
                        "usage: :config convert <toml|json> [path]".into(),
                    ));
                };
                let fmt = Format::parse_name(fmt_arg).ok_or_else(|| {
                    CommandError::BadArgs(format!(
                        "config convert: unknown format {fmt_arg:?} (expected toml/json)"
                    ))
                })?;
                let dest = if let Some(p) = rest.get(1) {
                    PathBuf::from(p)
                } else {
                    // Default destination: same directory as the currently
                    // loaded file, switching the extension. Falls back to
                    // the canonical default-write path.
                    let base = editor
                        .config_path
                        .clone()
                        .unwrap_or_else(|| {
                            loader::default_path().unwrap_or_else(|| PathBuf::from("config.toml"))
                        });
                    base.with_extension(fmt.extension())
                };
                match loader::write_to_path(&editor.config, &dest) {
                    Ok(()) => {
                        editor.status_message = Some(format!(
                            "config: wrote {} ({fmt})",
                            dest.display()
                        ));
                    }
                    Err(e) => return Err(CommandError::Failed(format!("config convert: {e}"))),
                }
            }
            // `reload` is `load` with no argument by intent — reload the active
            // file — but both accept an optional explicit path.
            "load" | "reload" => {
                let target = if let Some(p) = rest.first() {
                    PathBuf::from(p)
                } else {
                    match editor.config_path.clone() {
                        Some(p) => p,
                        None => {
                            return Err(CommandError::Failed(format!(
                                "config {sub}: no config currently loaded, pass a path"
                            )));
                        }
                    }
                };
                match loader::load_or_default(&target) {
                    Ok(cfg) => {
                        editor.apply_config(cfg);
                        editor.config_path = Some(target.clone());
                        editor.status_message =
                            Some(format!("config: loaded {}", target.display()));
                    }
                    Err(e) => return Err(CommandError::Failed(format!("config {sub}: {e}"))),
                }
            }
            other => {
                return Err(CommandError::BadArgs(format!(
                    "config: unknown sub-command {other:?} (expected show/path/convert/load/reload)"
                )));
            }
        }
        Ok(())
    }
    fn complete_arg(&self, idx: usize, before: &[String]) -> ArgCompletion {
        match idx {
            1 => ArgCompletion::Enum(&["conv", "convert", "load", "path", "reload", "show"]),
            2 => match before.first().map(String::as_str) {
                Some("show" | "convert" | "conv") => ArgCompletion::Enum(&["json", "toml"]),
                Some("load" | "reload") => ArgCompletion::Path,
                _ => ArgCompletion::None,
            },
            3 => match before.first().map(String::as_str) {
                Some("convert" | "conv") => ArgCompletion::Path,
                _ => ArgCompletion::None,
            },
            _ => ArgCompletion::None,
        }
    }
}

/// Create a scratch buffer containing `text`, open it in a fresh
/// vertical split, and apply the matching filetype so syntax
/// highlighting kicks in (when a `toml.vim` / `json.vim` is available
/// on the system).
fn open_config_view(editor: &mut Editor, text: &str, fmt: crate::config::loader::Format) {
    let buf_id = editor.open_scratch();
    if let Some(buf) = editor.buffers.get_mut(&buf_id) {
        let _ = buf.insert(0, text);
        buf.set_syntax_override(Some(match fmt {
            crate::config::loader::Format::Toml => "toml".to_string(),
            crate::config::loader::Format::Json => "json".to_string(),
        }));
    }
    crate::window_actions::split_active(editor, crate::window::SplitAxis::Vertical);
    if let Some(w) = editor.active_window_mut() {
        w.buffer = buf_id;
        w.cursor = crate::cursor::Cursor::default();
        w.top_line = 0;
        w.left_col = 0;
        w.selection = crate::cursor::Selection::None;
    }
    editor.status_message = Some(format!("config: showing ({fmt}) in new split"));
}


//! Option commands: `:set`, `:get`, `:colorscheme`.

use crate::command::{ArgCompletion, CommandError, ExArgs, ExCommand};
use crate::Editor;
use serde_json::Value;

/// `:set <option>[=<value>]` — runtime configuration.
///
/// Boolean shorthands (vim-compatible, `no` prefix disables):
///   `number`/`nu`           — line numbers in gutter
///   `autoindent`/`ai`       — copy indent of previous line on new line
///   `smartindent`/`si`      — language-aware extra indent rules
///   `expandtab`/`et`        — Tab key inserts spaces
///   `force_save`/`fs`       — block switching/closing away from unsaved buffers
///
/// Value options:
///   `tabstop=N`/`ts=N`      — tab display width and indent size
///   `shiftwidth=N`/`sw=N`   — indent size for >> and smartindent (alias for tabstop)
///   `syntax=NAME`/`ft=NAME` — per-buffer filetype override
pub(super) struct Set;
impl ExCommand for Set {
    fn name(&self) -> &'static str {
        "set"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["setlocal", "se"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        if args.words.is_empty() {
            return Err(CommandError::BadArgs("usage: :set option=value".into()));
        }
        // Generic dotted-path form: `:set options.tab_width = 8` (spaces
        // optional). Any config field addressable by a `a.b.c` path can be set;
        // the value is coerced to the field's existing type. Bare-name options
        // (`:set number`, `:set tab_width=8`) fall through to the shortcuts.
        let raw = args.raw.trim();
        if let Some((lhs, rhs)) = raw.split_once('=') {
            let path = lhs.trim();
            if path.contains('.') {
                return set_config_path(editor, path, rhs.trim());
            }
        }
        for word in &args.words {
            let (key, value) = match word.split_once('=') {
                Some(p) => p,
                None => {
                    match word.as_str() {
                        "syntax" | "syn" | "filetype" | "ft" => {
                            let cur = editor
                                .active_buffer_id()
                                .and_then(|b| editor.buffers.get(&b))
                                .and_then(|b| b.syntax_override())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "<auto>".into());
                            editor.status_message = Some(format!("syntax={cur}"));
                        }
                        "number" | "nu" => {
                            editor.config.options.number = true;
                            editor.status_message = Some("number".into());
                        }
                        "nonumber" | "nonu" => {
                            editor.config.options.number = false;
                            editor.status_message = Some("nonumber".into());
                        }
                        "autoindent" | "ai" => {
                            editor.config.options.autoindent = true;
                            editor.status_message = Some("autoindent".into());
                        }
                        "noautoindent" | "noai" => {
                            editor.config.options.autoindent = false;
                            editor.status_message = Some("noautoindent".into());
                        }
                        "smartindent" | "si" => {
                            editor.config.options.smartindent = true;
                            editor.status_message = Some("smartindent".into());
                        }
                        "nosmartindent" | "nosi" => {
                            editor.config.options.smartindent = false;
                            editor.status_message = Some("nosmartindent".into());
                        }
                        "paste" => {
                            editor.config.options.paste = true;
                            editor.status_message = Some("paste".into());
                        }
                        "nopaste" => {
                            editor.config.options.paste = false;
                            editor.status_message = Some("nopaste".into());
                        }
                        "force_save" | "fs" => {
                            editor.config.options.force_save = true;
                            editor.status_message = Some("force_save".into());
                        }
                        "noforce_save" | "nofs" => {
                            editor.config.options.force_save = false;
                            editor.status_message = Some("noforce_save".into());
                        }
                        "expandtab" | "et" => {
                            editor.config.options.expandtab = true;
                            editor.status_message = Some("expandtab".into());
                        }
                        "noexpandtab" | "noet" => {
                            editor.config.options.expandtab = false;
                            editor.status_message = Some("noexpandtab".into());
                        }
                        "nocolorcolumn" | "nocc" => {
                            editor.config.options.color_column.clear();
                            editor.status_message = Some("nocolorcolumn".into());
                        }
                        _ => {
                            editor.status_message =
                                Some(format!("set: unknown option '{word}'"));
                        }
                    }
                    continue;
                }
            };
            match key {
                "syntax" | "syn" | "filetype" | "ft" => {
                    let Some(buf_id) = editor.active_buffer_id() else {
                        return Err(CommandError::Failed("no active buffer".into()));
                    };
                    // `on`/`auto` → auto-detect (no override).
                    // `off`/`none`/empty → disable highlighting entirely.
                    // anything else → force that filetype.
                    let (override_val, msg) = match value.trim().to_ascii_lowercase().as_str() {
                        "on" | "auto" => (None, "syntax=auto (auto-detect)".to_string()),
                        "off" | "none" | "" => {
                            (Some("off".to_string()), "syntax=off (disabled)".to_string())
                        }
                        _ => {
                            let n = crate::syntax::normalize_filetype(value);
                            (Some(n.clone()), format!("syntax set to '{n}'"))
                        }
                    };
                    if let Some(buf) = editor.buffers.get_mut(&buf_id) {
                        buf.set_syntax_override(override_val);
                    }
                    editor.invalidate_syntax_cache(Some(buf_id));
                    editor.status_message = Some(msg);
                }
                "number" | "nu" => {
                    editor.config.options.number = parse_bool(value);
                    editor.status_message = Some(format!(
                        "{}number", if editor.config.options.number { "" } else { "no" }
                    ));
                }
                "autoindent" | "ai" => {
                    editor.config.options.autoindent = parse_bool(value);
                    editor.status_message = Some(format!(
                        "{}autoindent", if editor.config.options.autoindent { "" } else { "no" }
                    ));
                }
                "smartindent" | "si" => {
                    editor.config.options.smartindent = parse_bool(value);
                    editor.status_message = Some(format!(
                        "{}smartindent", if editor.config.options.smartindent { "" } else { "no" }
                    ));
                }
                "expandtab" | "et" => {
                    editor.config.options.expandtab = parse_bool(value);
                    editor.status_message = Some(format!(
                        "{}expandtab", if editor.config.options.expandtab { "" } else { "no" }
                    ));
                }
                "force_save" | "fs" => {
                    editor.config.options.force_save = parse_bool(value);
                    editor.status_message = Some(format!(
                        "{}force_save", if editor.config.options.force_save { "" } else { "no" }
                    ));
                }
                "tabstop" | "ts" | "tab_width" | "shiftwidth" | "sw" => {
                    match value.parse::<usize>() {
                        Ok(n) if n >= 1 => {
                            editor.config.options.tab_width = n;
                            editor.status_message = Some(format!("tabstop={n}"));
                        }
                        _ => {
                            editor.status_message =
                                Some(format!("set: tabstop must be a positive integer, got '{value}'"));
                        }
                    }
                }
                "colorcolumn" | "cc" | "color_column" => {
                    // `:set cc=` (empty value) turns the ruler off.
                    editor.config.options.color_column = value.to_string();
                    editor.status_message = Some(format!("colorcolumn={value}"));
                }
                "nbsp" | "nbsp_marker" => {
                    // `:set nbsp=` (empty value) hides the marker again.
                    editor.config.options.nbsp_marker = value.to_string();
                    editor.status_message = Some(format!("nbsp_marker={value}"));
                }
                "space" | "space_marker" => {
                    // `:set space=` (empty value) hides the marker again.
                    editor.config.options.space_marker = value.to_string();
                    editor.status_message = Some(format!("space_marker={value}"));
                }
                _ => {
                    editor.status_message =
                        Some(format!("set: unknown option '{key}'"));
                }
            }
        }
        Ok(())
    }
}

/// Parse a bool from a `:set option=<value>` string.
/// Accepts: `on`/`off`, `true`/`false`, `1`/`0`, `yes`/`no`.
fn parse_bool(s: &str) -> bool {
    matches!(s, "on" | "true" | "1" | "yes")
}

/// `:get <config.path>` — show the current value of any config field addressed
/// by a dotted path (e.g. `:get options.tab_width`). A bare name with no `.`
/// falls back to `options.<name>` for convenience.
pub(super) struct Get;
impl ExCommand for Get {
    fn name(&self) -> &'static str {
        "get"
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let Some(path) = args.first() else {
            return Err(CommandError::BadArgs("usage: :get <config.path>".into()));
        };
        get_config_path(editor, path)
    }
    fn complete_arg(&self, idx: usize, _before: &[String]) -> ArgCompletion {
        if idx == 1 {
            ArgCompletion::Dynamic(config_path_candidates)
        } else {
            ArgCompletion::None
        }
    }
}

/// Convert a dotted config path (`options.tab_width`) to a JSON Pointer
/// (`/options/tab_width`), escaping `~` and `/` per RFC 6901.
fn to_json_pointer(path: &str) -> String {
    let mut out = String::new();
    for seg in path.split('.') {
        out.push('/');
        out.push_str(&seg.replace('~', "~0").replace('/', "~1"));
    }
    out
}

/// Render a JSON value for display: strings bare, everything else as compact
/// JSON (`8`, `false`, `["a","b"]`).
fn display_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Coerce the user-typed `s` to a JSON value matching the type of `existing`,
/// so unquoted input works (`8` → number, `on` → bool, `foo` → string).
fn coerce_value(s: &str, existing: &Value) -> Result<Value, String> {
    match existing {
        Value::Bool(_) => match s.to_ascii_lowercase().as_str() {
            "true" | "on" | "1" | "yes" => Ok(Value::Bool(true)),
            "false" | "off" | "0" | "no" => Ok(Value::Bool(false)),
            _ => Err(format!("expected a boolean (on/off), got {s:?}")),
        },
        Value::Number(n) if n.is_f64() && n.as_i64().is_none() => s
            .parse::<f64>()
            .map(|f| serde_json::json!(f))
            .map_err(|_| format!("expected a number, got {s:?}")),
        Value::Number(_) => s
            .parse::<i64>()
            .map(|i| serde_json::json!(i))
            .map_err(|_| format!("expected an integer, got {s:?}")),
        Value::String(_) => Ok(Value::String(s.to_string())),
        // null (an unset optional) or a composite: accept raw JSON, else string.
        _ => serde_json::from_str(s).or_else(|_| Ok(Value::String(s.to_string()))),
    }
}

/// `:set <path> = <value>` — set any config field addressed by a dotted path.
/// Round-trips the config through JSON so it works for every field without
/// per-option wiring; the whole config is re-validated on the way back, so a
/// bad value (wrong type / out of range) is rejected with a readable error and
/// nothing changes.
fn set_config_path(editor: &mut Editor, path: &str, value_str: &str) -> Result<(), CommandError> {
    let mut root = serde_json::to_value(&editor.config)
        .map_err(|e| CommandError::Failed(format!("set: {e}")))?;
    let ptr = to_json_pointer(path);
    let slot = root
        .pointer_mut(&ptr)
        .ok_or_else(|| CommandError::BadArgs(format!("set: unknown config path {path:?}")))?;
    let new_val = coerce_value(value_str, slot).map_err(CommandError::BadArgs)?;
    let shown = display_value(&new_val);
    *slot = new_val;

    let new_config: crate::config::Config = serde_json::from_value(root)
        .map_err(|e| CommandError::Failed(format!("set {path}: {e}")))?;
    editor.config = new_config;
    editor.status_message = Some(format!("{path} = {shown}"));
    Ok(())
}

fn get_config_path(editor: &mut Editor, path: &str) -> Result<(), CommandError> {
    let root = serde_json::to_value(&editor.config)
        .map_err(|e| CommandError::Failed(format!("get: {e}")))?;
    // Try the path as given, then `options.<name>` for a bare name.
    let candidates = if path.contains('.') {
        vec![path.to_string()]
    } else {
        vec![path.to_string(), format!("options.{path}")]
    };
    for p in candidates {
        if let Some(v) = root.pointer(&to_json_pointer(&p)) {
            editor.status_message = Some(format!("{p} = {}", display_value(v)));
            return Ok(());
        }
    }
    Err(CommandError::BadArgs(format!("get: unknown config path {path:?}")))
}

/// Tab-completion candidates for a config path: the top-level keys plus the
/// `options.*` leaf names (the common case).
fn config_path_candidates(editor: &Editor, prefix: &str) -> Vec<String> {
    let Ok(root) = serde_json::to_value(&editor.config) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Value::Object(top) = &root {
        for (k, v) in top {
            out.push(k.clone());
            if k == "options" {
                if let Value::Object(opts) = v {
                    for ok in opts.keys() {
                        out.push(format!("options.{ok}"));
                    }
                }
            }
        }
    }
    out.retain(|c| c.starts_with(prefix));
    out.sort();
    out
}

pub(super) struct ColorScheme;
impl ExCommand for ColorScheme {
    fn name(&self) -> &'static str {
        "colorscheme"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["colo"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let Some(name) = args.first() else {
            // No arg = report current name.
            let cur = if editor.colorscheme.name.is_empty() {
                "<default>"
            } else {
                editor.colorscheme.name.as_str()
            };
            editor.status_message = Some(format!("colorscheme: {cur}"));
            return Ok(());
        };
        match crate::colorscheme::load(name) {
            Ok(scheme) => {
                editor.status_message = Some(format!("colorscheme {name} loaded"));
                editor.colorscheme = scheme;
                Ok(())
            }
            Err(e) => Err(CommandError::Failed(format!("E185: Cannot find color scheme '{name}': {e}"))),
        }
    }
}

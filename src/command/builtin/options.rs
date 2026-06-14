//! Option commands: `:set`, `:colorscheme`.

use crate::command::{CommandError, ExArgs, ExCommand};
use crate::Editor;

/// `:set <option>[=<value>]` — runtime configuration.
///
/// Boolean shorthands (vim-compatible, `no` prefix disables):
///   `number`/`nu`           — line numbers in gutter
///   `autoindent`/`ai`       — copy indent of previous line on new line
///   `smartindent`/`si`      — language-aware extra indent rules
///   `expandtab`/`et`        — Tab key inserts spaces
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

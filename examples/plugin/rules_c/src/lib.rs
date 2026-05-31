//! rules_c — C / C++ indent rules plugin for rtdvi.
//!
//! ## What this adds over the built-in smartindent
//!
//! The built-in layer handles the common cases (copy indent, `{`-open,
//! control-keyword +1, `}`-close). This plugin takes over completely for C
//! and C++, adding the cases the built-in misses:
//!
//! * `case LABEL:` and `default:` lines → indent the case body +1.
//! * Strips `//` line comments before matching end-of-line patterns, so
//!   `x = 1; // {` doesn't falsely trigger block-indent.
//! * Reads `tab_width` and `expandtab` from rtdvi options so the plugin
//!   respects whatever indent width the user has configured.
//!
//! ## Build
//!
//!   rustup target add wasm32-unknown-unknown
//!   cargo build --target wasm32-unknown-unknown --release
//!   cp target/wasm32-unknown-unknown/release/rules_c.wasm \
//!      ~/.local/rtdvi/plugins/
//!
//! ## Enable in ~/.config/rtdvi/config.toml
//!
//!   plugins = ["rules_c"]
//!
//! Once loaded, rtdvi calls this plugin's `compute_indent` for every new line
//! opened in a C or C++ buffer, bypassing the built-in smartindent for those
//! filetypes.

// WASM is single-threaded; static mut access is safe here.
#![allow(static_mut_refs)]

use std::alloc::Layout;

// ── rtdvi ABI imports ─────────────────────────────────────────────────────────

#[link(wasm_import_module = "rtdvi")]
extern "C" {
    /// Register this plugin as the indent provider for `filetype`.
    fn rtdvi_register_indent_provider(ptr: i32, len: i32) -> i32;
    /// Write line `row` of `buf_id` into the buffer at `out_ptr`.
    /// Returns byte count, or -1 on error.
    fn rtdvi_get_line(buf_id: i32, row: i32, out_ptr: i32, max_len: i32) -> i32;
    /// Read editor option `key` into `out_ptr`. Returns byte count.
    fn rtdvi_get_option_str(key_ptr: i32, key_len: i32, out_ptr: i32, max_len: i32) -> i32;
}

// ── Memory exports (required by rtdvi to pass data into the plugin) ───────────

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    if size <= 0 {
        return 0;
    }
    match Layout::from_size_align(size as usize, 8) {
        Ok(layout) => unsafe { std::alloc::alloc(layout) as i32 },
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: i32, size: i32) {
    if ptr == 0 || size <= 0 {
        return;
    }
    if let Ok(layout) = Layout::from_size_align(size as usize, 8) {
        unsafe { std::alloc::dealloc(ptr as *mut u8, layout) }
    }
}

// ── Static scratch buffers (WASM is single-threaded — no races) ───────────────

static mut LINE_BUF: [u8; 4096] = [0u8; 4096];
static mut OPT_BUF: [u8; 64] = [0u8; 64];

// ── Plugin lifecycle ──────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn rtdvi_init(_cfg_ptr: i32, _cfg_len: i32) -> i32 {
    for ft in ["c", "cpp"] {
        let b = ft.as_bytes();
        unsafe {
            rtdvi_register_indent_provider(b.as_ptr() as i32, b.len() as i32);
        }
    }
    0
}

// ── Indent provider export ────────────────────────────────────────────────────

/// Compute the indent string for the line that will be inserted after
/// `prev_row` in `buf_id`. Writes the result to `result_ptr` (a buffer
/// allocated by the host inside this plugin's linear memory).
/// Returns byte count.
#[no_mangle]
pub extern "C" fn compute_indent(
    buf_id: i32,
    prev_row: i32,
    result_ptr: i32,
    result_max: i32,
) -> i32 {
    let indent = compute_c_indent(buf_id, prev_row);
    let bytes = indent.as_bytes();
    let n = bytes.len().min(result_max as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), result_ptr as *mut u8, n);
    }
    n as i32
}

// ── C indent logic ────────────────────────────────────────────────────────────

fn compute_c_indent(buf_id: i32, prev_row: i32) -> String {
    if prev_row < 0 {
        return String::new();
    }

    let line = read_line(buf_id, prev_row);
    let unit = indent_unit();

    let trimmed_line = line.trim_end_matches('\n').trim_end_matches('\r');
    let base = leading_whitespace(trimmed_line).to_string();

    // Strip line comments before examining the effective end of the line.
    let effective = strip_line_comment(trimmed_line.trim_end());

    // Preprocessor directives: no indent change.
    if effective.trim_start().starts_with('#') {
        return base;
    }

    // Opening brace at end → body is one level in.
    if effective.ends_with('{') {
        return format!("{base}{unit}");
    }

    // `case X:` or `default:` label → case body is one level in.
    if is_case_or_default_label(effective) {
        return format!("{base}{unit}");
    }

    // Control-keyword line ending with `)` and no trailing `{` or `;` → +1.
    if is_c_control_line(effective) {
        return format!("{base}{unit}");
    }

    // Bare `else` (possibly after `}`) → body is one level in.
    if ends_with_word(effective, "else") {
        return format!("{base}{unit}");
    }

    // Everything else (statements, closing braces, blank lines…) → same level.
    base
}

// ── Pattern helpers ───────────────────────────────────────────────────────────

/// True if `s` is a `case X:` or `default:` label.
fn is_case_or_default_label(s: &str) -> bool {
    let t = s.trim_start();
    if t == "default:" {
        return true;
    }
    if (t.starts_with("case ") || t.starts_with("case\t")) && t.ends_with(':') {
        return true;
    }
    false
}

/// True if `s` is a C control-keyword line whose condition ends with `)` —
/// indicating the body follows on the next line without a `{`.
fn is_c_control_line(s: &str) -> bool {
    if !s.ends_with(')') {
        return false;
    }
    let t = s.trim_start();
    t.starts_with("if ")
        || t.starts_with("if(")
        || t.starts_with("for ")
        || t.starts_with("for(")
        || t.starts_with("while ")
        || t.starts_with("while(")
        || t.starts_with("else if ")
        || t.starts_with("else if(")
        || t.starts_with("switch ")
        || t.starts_with("switch(")
}

/// True if `s` ends with `word` as a complete word (not a suffix of a longer
/// identifier).
fn ends_with_word(s: &str, word: &str) -> bool {
    if !s.ends_with(word) {
        return false;
    }
    let rest = &s[..s.len() - word.len()];
    rest.is_empty() || !rest.ends_with(|c: char| c.is_alphanumeric() || c == '_')
}

/// Strip a `//`-style line comment from the end of `s`. Does not handle block
/// comments or comments inside strings — good enough for indent heuristics.
fn strip_line_comment(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut in_str = false;
    let mut escape = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if escape {
            escape = false;
        } else if b == b'\\' && in_str {
            escape = true;
        } else if b == b'"' {
            in_str = !in_str;
        } else if !in_str && b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            return s[..i].trim_end();
        }
        i += 1;
    }
    s
}

/// Extract the leading spaces/tabs from `line`.
fn leading_whitespace(line: &str) -> &str {
    let n = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len());
    &line[..n]
}

// ── Host interaction helpers ──────────────────────────────────────────────────

fn read_line(buf_id: i32, row: i32) -> String {
    // Use addr_of_mut! to avoid creating a `&mut` reference to `static mut`.
    let ptr = std::ptr::addr_of_mut!(LINE_BUF) as *mut u8;
    let n = unsafe { rtdvi_get_line(buf_id, row, ptr as i32, LINE_BUF.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, n as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn indent_unit() -> String {
    let tw = get_usize_option("tab_width", 4);
    let et = get_bool_option("expandtab", true);
    if et { " ".repeat(tw) } else { "\t".to_string() }
}

fn get_usize_option(key: &str, default: usize) -> usize {
    let ptr = std::ptr::addr_of_mut!(OPT_BUF) as *mut u8;
    let n = unsafe {
        rtdvi_get_option_str(key.as_ptr() as i32, key.len() as i32, ptr as i32, OPT_BUF.len() as i32)
    };
    if n <= 0 {
        return default;
    }
    let s = unsafe { std::str::from_utf8(std::slice::from_raw_parts(ptr, n as usize)).unwrap_or("") };
    s.parse().unwrap_or(default)
}

fn get_bool_option(key: &str, default: bool) -> bool {
    let ptr = std::ptr::addr_of_mut!(OPT_BUF) as *mut u8;
    let n = unsafe {
        rtdvi_get_option_str(key.as_ptr() as i32, key.len() as i32, ptr as i32, OPT_BUF.len() as i32)
    };
    if n <= 0 {
        return default;
    }
    let s = unsafe { std::str::from_utf8(std::slice::from_raw_parts(ptr, n as usize)).unwrap_or("") };
    matches!(s, "true" | "1")
}

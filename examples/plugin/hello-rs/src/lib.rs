//! hello-rs — minimal jvim plugin in Rust.
//!
//! ## Build
//!
//!   rustup target add wasm32-unknown-unknown
//!   cargo build --target wasm32-unknown-unknown --release
//!   cp target/wasm32-unknown-unknown/release/hello.wasm ~/.local/jvim/plugins/
//!
//! ## Enable in ~/.config/jvim/config.toml
//!
//!   plugins = ["hello"]
//!
//! ## Commands
//!
//!   :plugin hello.greet()         — "Hello from Rust! (hello-rs plugin)"
//!   :plugin hello.greet("World")  — "Hello World from Rust! (hello-rs plugin)"
//!   :plugin hello.wordcount()     — count words in the active buffer
//!   <leader>h                     — same as :plugin hello.greet() (bound in jvim_init)

use std::alloc::Layout;
use std::slice;

// ── jvim host imports ─────────────────────────────────────────────────────────
//
// The `wasm_import_module` attribute sets the WASM module name to "jvim".
// Without it the linker would put these in "env", which the host does not provide.

#[link(wasm_import_module = "jvim")]
extern "C" {
    fn jvim_log(ptr: i32, len: i32);
    fn jvim_set_status(ptr: i32, len: i32);
    fn jvim_register_command(ptr: i32, len: i32) -> i32;
    fn jvim_bind_key(
        mode_ptr: i32,
        mode_len: i32,
        keys_ptr: i32,
        keys_len: i32,
        fn_ptr: i32,
        fn_len: i32,
    ) -> i32;
    fn jvim_active_buffer_id() -> i32;
    fn jvim_line_count(buf_id: i32) -> i32;
    fn jvim_get_line(buf_id: i32, row: i32, out_ptr: i32, max_len: i32) -> i32;
}

// ── Memory exports (required by the host to pass strings into the plugin) ─────

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

// ── Internal helpers ──────────────────────────────────────────────────────────

fn log(msg: &str) {
    unsafe { jvim_log(msg.as_ptr() as i32, msg.len() as i32) }
}

fn set_status(msg: &str) {
    unsafe { jvim_set_status(msg.as_ptr() as i32, msg.len() as i32) }
}

fn register_command(name: &str) {
    unsafe { jvim_register_command(name.as_ptr() as i32, name.len() as i32) };
}

fn bind_key(mode: &str, keys: &str, func: &str) {
    unsafe {
        jvim_bind_key(
            mode.as_ptr() as i32,
            mode.len() as i32,
            keys.as_ptr() as i32,
            keys.len() as i32,
            func.as_ptr() as i32,
            func.len() as i32,
        )
    };
}

/// Read line `row` from buffer `buf_id` into a new String.
fn get_line(buf_id: i32, row: i32) -> String {
    let mut buf = vec![0u8; 4096];
    let n = unsafe { jvim_get_line(buf_id, row, buf.as_mut_ptr() as i32, buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..n as usize]).into_owned()
}

// ── Plugin lifecycle ──────────────────────────────────────────────────────────

/// Called once when the plugin loads. Config options arrive as a JSON object.
/// Return 0 to accept the plugin; any other value skips it.
#[no_mangle]
pub extern "C" fn jvim_init(cfg_ptr: i32, cfg_len: i32) -> i32 {
    let _cfg = unsafe {
        let bytes = slice::from_raw_parts(cfg_ptr as *const u8, cfg_len as usize);
        std::str::from_utf8(bytes).unwrap_or("{}")
    };
    log("hello-rs: loaded");

    register_command("greet");
    register_command("wordcount");

    // Bind <leader>h → :plugin hello.greet()
    // The function argument format is "pluginname.functionname".
    bind_key("normal", "<leader>h", "hello.greet");

    0
}

/// Called for every command registered via jvim_register_command.
/// `name` is the command name; `args` is a JSON array (e.g. `[]` or `["World"]`).
#[no_mangle]
pub extern "C" fn run_command(
    name_ptr: i32,
    name_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> i32 {
    let name = unsafe {
        let bytes = slice::from_raw_parts(name_ptr as *const u8, name_len as usize);
        std::str::from_utf8(bytes).unwrap_or("")
    };
    let args = unsafe {
        let bytes = slice::from_raw_parts(args_ptr as *const u8, args_len as usize);
        std::str::from_utf8(bytes).unwrap_or("[]")
    };

    match name {
        "greet" => cmd_greet(args),
        "wordcount" => cmd_wordcount(),
        other => {
            log(&format!("hello-rs: unknown command {other:?}"));
            -1
        }
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

fn cmd_greet(args: &str) -> i32 {
    // Treat missing and empty string the same — both show the default greeting.
    let msg = match first_string_arg(args).filter(|s| !s.is_empty()) {
        Some(who) => format!("Hello {who} from Rust! (hello-rs plugin)"),
        None => "Hello from Rust! (hello-rs plugin)".to_string(),
    };
    set_status(&msg);
    0
}

/// Extract the first JSON string from the args array the host passes in.
///
/// The host sends a JSON array, e.g.:
///   `[]`               → None
///   `["World"]`        → Some("World")
///   `["Hello, there"]` → Some("Hello, there")   (commas inside quotes are fine)
///
/// Scans for the opening `"`, then walks bytes until the matching closing `"`
/// respecting `\"` escapes, so this handles commas and other special chars inside
/// the string without pulling in a JSON library.
fn first_string_arg(json: &str) -> Option<String> {
    let bytes = json.as_bytes();
    // Find the opening quote of the first string element.
    let open = bytes.iter().position(|&b| b == b'"')? + 1;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2, // skip the escaped character
            b'"' => {
                let s = std::str::from_utf8(&bytes[open..i]).ok()?;
                return Some(
                    s.replace("\\n", "\n")
                        .replace("\\t", "\t")
                        .replace("\\\"", "\"")
                        .replace("\\\\", "\\"),
                );
            }
            _ => i += 1,
        }
    }
    None
}

fn cmd_wordcount() -> i32 {
    let buf_id = unsafe { jvim_active_buffer_id() };
    if buf_id < 0 {
        set_status("wordcount: no active buffer");
        return 0;
    }
    let line_count = unsafe { jvim_line_count(buf_id) };
    if line_count < 0 {
        set_status("wordcount: buffer not found");
        return 0;
    }

    let mut words: usize = 0;
    for row in 0..line_count {
        words += get_line(buf_id, row).split_whitespace().count();
    }

    let lines = line_count as usize;
    set_status(&format!("{words} words, {lines} lines"));
    0
}

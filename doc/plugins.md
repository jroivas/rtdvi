# Plugins

rtdvi plugins run inside a WebAssembly sandbox. Every plugin is a `.wasm` binary
that imports a small set of host functions (the "rtdvi ABI") and exports a handful
of functions that rtdvi calls at runtime.

Lua plugins are a special case: a WASM plugin called `mlua-wasm` embeds a full
Lua 5.4 VM and acts as a *plugin manager*, loading `.lua` files on your behalf.

---

## Runtime compatibility

rtdvi supports two WASM runtimes (see [building.md](building.md)). Not all plugin
types work with both:

| Plugin type | `runtime-wasmtime` | `runtime-wasmi` |
|-------------|-------------------|-----------------|
| WAT / plain Rust (`wasm32-unknown-unknown`) | ✓ | ✓ |
| `wasm32-unknown-emscripten` without WASM EH | ✓ | ✓ |
| `mlua-wasm` and any plugin using WASM exceptions | ✓ | ✗ |

wasmi 1.0.9 does not implement the WASM exceptions proposal. Attempting to load
an EH-using plugin under wasmi produces:

```
plugin "mlua_wasm": invalid WASM: exceptions proposal not enabled
    (plugin uses WASM exceptions; build rtdvi with runtime-wasmtime)
```

---

## Architecture

```
rtdvi  ←──────────────────────────────────────────────────────────  .wasm plugin
       ─── rtdvi_init / run_command / on_event ──────────────────►
       ◄── rtdvi_log / rtdvi_set_status / rtdvi_register_command ───

rtdvi  ←─────────────────  mlua-wasm.wasm  ←──────────────────  hello.lua
       ─── load_plugin ──►               ─── Lua 5.4 VM ──────►

rtdvi  ←───────────────  rules_c.wasm  (indent provider for C / C++)
       ─── compute_indent(buf_id, prev_row, ptr, max) → n ───────►
       ◄── rtdvi_register_indent_provider("c") ────────────────────
```

All plugin code runs inside the WASM sandbox.  Plugins cannot access the host
filesystem, network, or OS directly.  The only interface is the rtdvi ABI.

---

## WASM ABI Reference

### Required imports (module `"rtdvi"`)

These are the functions rtdvi exposes to every plugin.  All string arguments are
passed as `(ptr: i32, len: i32)` pairs pointing into the plugin's linear memory.

| Function | Signature | Description |
|----------|-----------|-------------|
| `rtdvi_log` | `(ptr, len)` | Write a line to the plugin log (shown on error) |
| `rtdvi_set_status` | `(ptr, len)` | Set the editor status bar message |
| `rtdvi_register_command` | `(ptr, len) → i32` | Register a named ex command; returns 0 on success |
| `rtdvi_register_plugin_manager` | `(ptr, len) → i32` | Declare this plugin as the loader for a file extension (e.g. `".lua"`) |
| `rtdvi_register_indent_provider` | `(ptr, len) → i32` | Declare this plugin as the indent provider for a filetype (e.g. `"c"`). rtdvi will call the plugin's `compute_indent` export instead of its built-in smartindent for that filetype. |
| `rtdvi_bind_key` | `(mode_ptr,len, keys_ptr,len, fn_ptr,len) → i32` | Bind a key sequence to a plugin function |
| `rtdvi_active_buffer_id` | `() → i32` | ID of the currently active buffer |
| `rtdvi_active_window_id` | `() → i32` | ID of the currently active window |
| `rtdvi_line_count` | `(buf_id) → i32` | Number of lines in a buffer |
| `rtdvi_get_line` | `(buf_id, row, out_ptr, max_len) → i32` | Copy a line into plugin memory; returns byte count |
| `rtdvi_get_cursor` | `(win_id) → i64` | Packed `(row << 32 | col)` cursor position |
| `rtdvi_set_cursor` | `(win_id, row, col) → i32` | Move cursor (deferred until after WASM call returns) |
| `rtdvi_insert_text` | `(buf_id, char_pos, ptr, len) → i32` | Insert text at a character position (deferred) |
| `rtdvi_delete_text` | `(buf_id, start, end) → i32` | Delete character range (deferred) |
| `rtdvi_get_option_str` | `(key_ptr, key_len, out_ptr, max_len) → i32` | Read a string editor option |
| `rtdvi_render_span` | `(text_ptr,len, fg, bg, attrs, size, target_ptr,target_len) → i32` | Append a styled segment to the render line under construction (see [Render buffers](#render-buffers)) |
| `rtdvi_render_newline` | `()` | Finish the current render line and start a new one |
| `rtdvi_render_image` | `(path_ptr,len, alt_ptr,len) → i32` | Place an image on its own line; local files render as half-block art, otherwise a labelled placeholder |
| `rtdvi_render_open` | `(title_ptr, title_len) → i32` | Open the accumulated lines as a non-editable render buffer in a split |

Mutations (`rtdvi_set_cursor`, `rtdvi_insert_text`, `rtdvi_delete_text`,
`rtdvi_render_open`) are *deferred*: they are queued and applied to the editor
after the WASM call returns, so there is no borrow conflict with the read-only
snapshot.

### Render buffers

A **render buffer** is a non-editable buffer whose content is pre-styled lines
(text + colour + emphasis + size + links), painted directly instead of going
through the syntax pipeline. It is how a plugin shows *rendered* output —
markdown, help, an HTML view — rather than editable text. Build it with three
host calls:

1. `rtdvi_render_span(text, fg, bg, attrs, size, target)` — append one styled
   segment to the current line. Encoding:
   - `fg` / `bg`: `0xRRGGBB`, or `-1` for "no colour".
   - `attrs`: bitset — `1` bold, `2` italic, `4` underline, `8` reverse,
     `16` strikethrough.
   - `size`: advisory scale — `0` body, `1..=6` heading levels. The TUI can't
     change glyph size, so it approximates larger sizes with emphasis; the value
     is preserved for richer frontends.
   - `target` (`target_len > 0`): makes the segment a **followable link** to
     that target.
2. `rtdvi_render_newline()` — end the line, begin the next.
3. `rtdvi_render_image(path, alt)` — place an image on its own line. It is
   decoded and rendered as truecolor **half-block (`▀`) art**, sized to the
   window. **Local** files are resolved against the source document's directory;
   **remote** `http(s)` images are fetched over the network (blocking, with a
   timeout + size cap, and cached per URL so re-renders don't refetch) when
   `options.fetch_remote_images` is on (the default). An unreadable or
   fetch-failed image falls back to a `🖼 alt` placeholder. Decoding/fetching
   runs in the trusted core, not the sandboxed plugin.
4. `rtdvi_render_open(title)` — open the accumulated lines in a split (reusing an
   existing render window if there is one, so following a link replaces the page
   in place).

**Following links.** In a render buffer, `<Enter>` follows the link under the
cursor, `<Tab>` / `<S-Tab>` jump between links, and `q` closes the view. A link
to a local file is resolved relative to the source file's directory; the editor
opens it and re-runs the producing command, so the linked page renders in the
same pane (browser-style). `http(s)` URLs and `#anchors` are reported in the
status line (not opened) in this version. Normal motions (`j`/`k`/`Ctrl-d`/…)
still scroll.

The worked example is `examples/plugin/markdown` — `:md` renders the current
buffer as styled markdown with followable `[text](file.md)` links.

### WASI

rtdvi also provides `wasi_snapshot_preview1.random_get` (used internally by
Rust's `HashMap` to seed its hasher).  All other WASI imports are **stubbed as
traps** — if a plugin calls them it will crash.  Do not rely on WASI I/O, clock,
or process-control functions.

### Required exports

Every WASM plugin must export:

| Export | Signature | Description |
|--------|-----------|-------------|
| `memory` | linear memory | The plugin's address space |
| `alloc` | `(size: i32) → i32` | Allocate `size` bytes; return pointer |
| `dealloc` | `(ptr: i32, size: i32)` | Free a previous allocation |
| `rtdvi_init` | `(cfg_ptr: i32, cfg_len: i32) → i32` | Called once on load; return 0 to accept, non-zero to reject |

Optional exports (only needed for specific features):

| Export | Signature | Description |
|--------|-----------|-------------|
| `run_command` | `(name_ptr,len, args_ptr,len) → i32` | Dispatch a registered ex command |
| `on_event` | `(json_ptr: i32, json_len: i32)` | Receive editor events as JSON |
| `load_plugin` | `(name_ptr,len, content_ptr,len) → i32` | (Plugin managers) load a sub-plugin from source text |
| `unload_plugin` | `(name_ptr: i32, name_len: i32) → i32` | (Plugin managers) unload a previously loaded sub-plugin |
| `compute_indent` | `(buf_id: i32, prev_row: i32, result_ptr: i32, result_max: i32) → i32` | (Indent providers) compute the indent for the new line following `prev_row`. Write the indent string to `result_ptr` (allocated by the host via `alloc`). Return byte count, or 0 for empty indent, or -1 on error. |

### String passing convention

When rtdvi calls an export it writes strings into the plugin's memory via
`alloc` + direct write, then passes `(ptr, len)`.  When a plugin calls an
import it passes a pointer into its own static data section or a
`alloc`-returned region.  Strings are **not** null-terminated; always use the
length parameter.

---

## Building a Plugin

### WAT — minimal, no toolchain needed

Write the module in WebAssembly Text format and assemble it with `wat2wasm`:

```wat
(module
  (import "rtdvi" "rtdvi_register_command"
          (func $reg (param i32 i32) (result i32)))
  (import "rtdvi" "rtdvi_set_status"
          (func $status (param i32 i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "greet")             ;; offset 0, len 5
  (data (i32.const 8) "Hello from WAT!")   ;; offset 8, len 15

  (func (export "alloc") (param i32) (result i32) (i32.const 128))
  (func (export "dealloc") (param i32 i32))

  (func (export "rtdvi_init") (param i32 i32) (result i32)
    (drop (call $reg (i32.const 0) (i32.const 5)))
    (i32.const 0))

  (func (export "run_command")
    (param i32 i32 i32 i32) (result i32)
    (call $status (i32.const 8) (i32.const 15))
    (i32.const 0))
)
```

```sh
wat2wasm hello.wat -o hello.wasm
cp hello.wasm ~/.local/rtdvi/plugins/
```

See `examples/plugin/hello-wasm/hello.wat` for the full annotated example.

### Rust — `wasm32-unknown-unknown`

For pure Rust plugins with no C dependencies, target `wasm32-unknown-unknown`.
This is the simplest target: no libc, no emscripten, no WASM exception handling.

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]
```

```rust
// src/lib.rs
#[link(wasm_import_module = "rtdvi")]
extern "C" {
    fn rtdvi_log(ptr: i32, len: i32);
    fn rtdvi_register_command(ptr: i32, len: i32) -> i32;
}

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 { /* bump allocator */ }
#[no_mangle]
pub extern "C" fn dealloc(_ptr: i32, _size: i32) {}

#[no_mangle]
pub extern "C" fn rtdvi_init(_cfg_ptr: i32, _cfg_len: i32) -> i32 {
    let name = b"greet";
    unsafe { rtdvi_register_command(name.as_ptr() as i32, name.len() as i32) };
    0
}
```

```sh
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/myplugin.wasm ~/.local/rtdvi/plugins/
```

See `examples/plugin/hello-rs/` for the full example including a word-count
command that reads buffer lines via `rtdvi_get_line`.

### Rust — `wasm32-unknown-emscripten` (C/Lua dependencies)

If your plugin links C code (e.g. Lua via mlua), you must target
`wasm32-unknown-emscripten`.  This is significantly more complex — read the
[Known Pitfalls](#known-pitfalls) section before starting.

`.cargo/config.toml` for the plugin crate:
```toml
[build]
target = "wasm32-unknown-emscripten"

[target.wasm32-unknown-emscripten]
rustflags = [
    "-C", "panic=abort",
    "-C", "link-args=-sERROR_ON_UNDEFINED_SYMBOLS=0 -sSUPPORT_LONGJMP=wasm",
]
```

Post-processing with `wasm-opt` is required to translate old-format WASM EH
(produced by the Rust sysroot) to new-format (required by wasmtime 45+):
```sh
wasm-opt \
  --translate-to-exnref \
  --enable-exception-handling \
  --enable-reference-types \
  plugin.wasm -o plugin.wasm
```

See `examples/plugin/mlua-wasm/Makefile` for the full build recipe.

---

## Lua Plugins via mlua-wasm

`mlua-wasm` is a WASM plugin that embeds Lua 5.4 and acts as a plugin manager
for `.lua` files.  Load it first, then load your Lua plugins:

```toml
# ~/.config/rtdvi/config.toml
plugins = [
  "mlua_wasm",    # loads the Lua manager first
  "hello",        # tries hello.wasm, then hello.lua via mlua_wasm
]
```

### Supported Lua API (`vim.*`)

mlua-wasm exposes a Neovim-compatible subset:

```lua
-- Logging / status
print(msg)                            -- sends to rtdvi log
vim.notify(msg [, level])             -- sets status bar

-- Key bindings
vim.keymap.set(mode, lhs, fn, opts)

-- Commands
vim.api.nvim_create_user_command(name, fn, opts)

-- Buffer / window
vim.api.nvim_get_current_buf()
vim.api.nvim_buf_get_lines(buf, start, end, strict)
vim.api.nvim_buf_set_lines(buf, start, end, strict, lines)
vim.api.nvim_get_current_win()
vim.api.nvim_win_get_cursor(win)      -- returns {row, col} (1-based row)
vim.api.nvim_win_set_cursor(win, {row, col})

-- Options (read-only)
vim.o.tab_width                       -- via __index metatable
vim.o.leader

-- Ex commands (stub, not yet dispatched to rtdvi)
vim.cmd(str)
vim.api.nvim_command(str)
```

Unimplemented `vim.api.*` calls return a no-op function and log a warning.

### Example Lua plugin

```lua
-- hello.lua
vim.api.nvim_create_user_command("Greet", function(args)
  vim.notify("Hello from Lua!")
end, {})

vim.keymap.set("normal", "<leader>h", function()
  vim.notify("Hello from <leader>h!")
end)
```

---

## Configuration

Plugins are listed in `~/.config/rtdvi/config.toml`:

```toml
plugins = [
  "wordcount",                      # simple name — looks for wordcount.wasm
  "mlua_wasm",                      # Lua plugin manager
  "hello",                          # tries hello.wasm, then hello.lua
  {name = "formatter", cmd = "rustfmt"},  # table form with options
]
```

Options in table form are passed to `rtdvi_init` as a JSON object:
`rtdvi_init` receives `{"cmd":"rustfmt"}` in the config buffer.

### Plugin search path

For a plugin named `foo`, rtdvi looks for `foo.wasm` in order:

1. `$RTDVI_PLUGIN_DIR/foo.wasm`
2. `$HOME/.local/rtdvi/plugins/foo.wasm`
3. `./foo.wasm` (current working directory — useful during development)

---

## Known Pitfalls

These are real issues encountered while building `mlua-wasm` (the Lua engine
WASM plugin).  They apply to any Rust plugin targeting `wasm32-unknown-emscripten`.

### 1. `thread_local!` crashes — TLS base is never initialised

**What happens**: Using `thread_local! { static FOO: ... }` in a WASM plugin
causes a trap the first time it is accessed.

**Why**: emscripten's thread-local storage uses a global pointer `__tls_base`
that is written by emscripten's startup code (`_start` / `main`).  When wasmtime
calls an exported function directly (bypassing the emscripten runtime), this
pointer is never set.  Any TLS access dereferences address 0 → out-of-bounds
memory trap.

**Fix**: Use `static mut` instead.  WASM is single-threaded, so there are no
data races.

```rust
// BAD — crashes
thread_local! {
    static STATE: RefCell<Option<MyState>> = RefCell::new(None);
}

// GOOD — single-threaded WASM, static mut is safe
static mut STATE: Option<MyState> = None;
```

### 2. `HashMap::new()` traps — WASI `random_get` is needed

**What happens**: Creating a `HashMap` (or `HashSet`, or anything that uses
Rust's default `RandomState`) traps during the hash seed call.

**Why**: `HashMap::new()` seeds its hasher via `RandomState::new()`, which calls
`getrandom()`, which on the emscripten WASM target calls the WASI function
`wasi_snapshot_preview1.random_get`.  rtdvi stubs all unknown WASI imports as
traps.

**Fix**: rtdvi explicitly provides `random_get` in its ABI (`src/plugin/abi.rs`)
with a deterministic fill.  No plugin-side change is needed — just be aware that
other WASI functions (`fd_write`, `clock_time_get`, etc.) are still stubs.  Do
not write plugins that call those.

### 3. C++ `invoke_*` trampolines trap — use `-fwasm-exceptions`

**What happens**: Any indirect C++ function call traps with an import error on
`env.invoke_vii` (or similar).

**Why**: emscripten's default C++ exception handling wraps every indirect call
with a JS trampoline (`invoke_*`).  These trampolines are imports that require a
JavaScript host to implement the catch/resume semantics.  rtdvi is a native Rust
host, not a JS engine.

**Why it is triggered by Lua specifically**: `mlua-sys` compiles Lua as C++ with
`-fexceptions` and patches `luaconf.h` to use `throw/try` for Lua error recovery.
Every call to a potentially-throwing function — including Lua's internal
`(*f)(L, ud)` indirect dispatch in `luaD_rawrunprotected` — gets wrapped.

**Fix**: Build the C/C++ code with `-fwasm-exceptions`:

```makefile
EMCC_CFLAGS="-fwasm-exceptions" cargo build
```

This switches C++ exception handling from JS trampolines to native WASM EH
instructions.  No `invoke_*` imports are generated.  Combined with
`--translate-to-exnref` the resulting binary loads correctly in wasmtime.

### 4. wasm-opt `--strip-eh` corrupts the binary without `--enable-exception-handling`

**What happens**: A WASM binary that previously parsed now fails with "invalid
WASM: failed to parse WebAssembly module".

**Why**: If the input binary contains exception-handling instructions (EH)
and you run `wasm-opt --strip-eh` *without* `--enable-exception-handling`,
wasm-opt fails to parse the EH section correctly and emits a malformed binary.

**Fix**: Always enable the proposal when the input may contain EH:

```sh
wasm-opt --strip-eh \
  --enable-exception-handling \
  --enable-reference-types \
  plugin.wasm -o plugin.wasm
```

### 5. Old-format WASM EH rejected by wasmtime 45+

**What happens**: wasmtime fails to parse the module with a message about
unsupported EH opcodes.

**Why**: There are two incompatible WASM exception-handling formats:

| Format | Opcodes | Produced by |
|--------|---------|-------------|
| Phase 3 (old) | `try` / `catch` / `throw` blocks | LLVM ≤ 17 / pre-LLVM-18 Rust sysroot |
| Phase 4 (new) | `try_table` + `exnref` | LLVM 18+ with `-fwasm-exceptions` |

wasmtime 45 with `wasm_exceptions(true)` supports **only the new format**.
The Rust sysroot for `wasm32-unknown-emscripten` in Rust ≤ 1.82 produces
old-format EH.  Even with LLVM 21+ in rustc, the *pre-compiled* std sysroot
artifacts may be old-format.

**Fix**: Run `wasm-opt --translate-to-exnref` after the build to convert
old-format EH to new-format:

```sh
wasm-opt \
  --translate-to-exnref \
  --enable-exception-handling \
  --enable-reference-types \
  plugin.wasm -o plugin.wasm
```

The wasmtime engine must be created with `wasm_exceptions(true)` to parse the
result (rtdvi does this automatically in `src/plugin/runtime.rs`).

### 6. `SUPPORT_LONGJMP=0` breaks Lua's error recovery

**What happens**: Lua init traps immediately — `rtdvi_init` never returns.

**Why**: Lua's protected-call mechanism (`lua_pcall`) relies on C `setjmp` /
`longjmp` for error recovery.  With `SUPPORT_LONGJMP=0`, emscripten compiles
longjmp as a no-op or an abort stub.  Even with `StdLib::NONE`, mlua calls
`lua_pcall` during Lua state initialisation.  The first longjmp traps.

**Fix**: Use `SUPPORT_LONGJMP=wasm` to implement longjmp via WASM EH:

```toml
# .cargo/config.toml
[target.wasm32-unknown-emscripten]
rustflags = ["-C", "link-args=-sSUPPORT_LONGJMP=wasm"]
```

Combined with the `--translate-to-exnref` post-processing step, Lua errors
propagate correctly without needing a JavaScript host.

---

## What to Avoid in WASM Plugins

Quick reference for `wasm32-unknown-emscripten` Rust plugins:

- **Do not** use `thread_local!` — TLS base is never initialised by wasmtime.
  Use `static mut` instead.
- **Do not** call WASI functions other than `random_get`.  `fd_write`,
  `clock_time_get`, `proc_exit`, and all other WASI imports trap.
- **Do not** use `SUPPORT_LONGJMP=0` if your plugin uses Lua or any C library
  that relies on `setjmp`/`longjmp`.
- **Do not** link C++ code without `-fwasm-exceptions` (emscripten default) if
  you want to run under rtdvi's native WASM host.
- **Do not** run `wasm-opt --strip-eh` without `--enable-exception-handling`
  on binaries that contain EH sections.
- **Do** run `wasm-opt --translate-to-exnref --enable-exception-handling`
  after any emscripten build to ensure the binary uses the EH format wasmtime
  expects.
- **Do** build with `panic=abort` — unwinding Rust panics across WASM/host
  boundaries is not supported.

---

## Example: `rules_c` — indent provider for C / C++

`examples/plugin/rules_c/` is the reference implementation of an indent
provider. It targets `wasm32-unknown-unknown` (pure Rust, no Emscripten, no
WASM exceptions) and works with both `runtime-wasmtime` and `runtime-wasmi`.

### What it handles

On top of the built-in smartindent, `rules_c` adds:

- **`case LABEL:` and `default:`** lines → indent the case body one level in.
- **`//` comment stripping** — patterns are matched against the code before any
  trailing `//` comment, so `x++; // open {` doesn't falsely trigger
  block-indent.
- **`tab_width` / `expandtab` aware** — reads the host options at call time so
  the plugin respects `:set tabstop=2` etc. without restart.

### Build

```sh
cd examples/plugin/rules_c
rustup target add wasm32-unknown-unknown   # once
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/rules_c.wasm \
   ~/.local/rtdvi/plugins/
```

### Enable

```toml
# ~/.config/rtdvi/config.toml
plugins = ["rules_c"]
```

Once loaded, rtdvi routes all indent computations for C and C++ buffers
through this plugin. The built-in smartindent is bypassed for those filetypes
but still applies for all others.

### Implementing your own language rules plugin

`rules_c` is the canonical template. The minimal contract is:

1. Call `rtdvi_register_indent_provider("lang")` for each filetype during
   `rtdvi_init`.
2. Export `compute_indent(buf_id, prev_row, result_ptr, result_max) → i32`.
3. Inside `compute_indent`, use `rtdvi_get_line` to read context lines,
   compute the indent string, write it to `result_ptr`, return byte count.

No Emscripten, no C, no WASM exceptions needed — pure `wasm32-unknown-unknown`
Rust is sufficient for any language whose indent rules can be expressed as
pattern matching on surrounding lines.

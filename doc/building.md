# Building rtdvi

## Requirements

- **Rust** 1.75 or newer (anything from 2024 will do). Install via
  [rustup.rs](https://rustup.rs).
- **Linux / macOS** terminal. Windows isn't tested but most of the
  stack (`crossterm`, `ropey`) is cross-platform — it'll likely need
  small tweaks.
- **Optional but recommended**:
  - System `vim` install — provides default colorschemes and the
    syntax files rtdvi parses. Typically `/usr/share/vim/vim*/colors/`
    and `/usr/share/vim/vim*/syntax/` on Linux.
  - `clangd` on PATH — auto-spawned for C/C++ buffers if present.

## Build

```sh
cargo build --release
```

The binary lands at `target/release/rtdvi`.

## Feature flags

rtdvi has three optional features that change which engines are compiled in.
Exactly one WASM runtime must be selected — the two are mutually exclusive.

### WASM runtime (pick one)

| Feature | Default | Description |
|---------|---------|-------------|
| `runtime-wasmtime` | ✓ | JIT-compiled WASM via [wasmtime](https://wasmtime.dev). Higher plugin performance, larger binary (~22 MB release), heavier build (~140 extra crates). |
| `runtime-wasmi` | | Interpreted WASM via [wasmi](https://github.com/wasmi-labs/wasmi). Smaller binary, far fewer dependencies, slower plugin execution. Good choice if you don't run compute-heavy plugins. Plugins that use WASM exceptions (e.g. `mlua-wasm`) cannot load — plain Rust plugins (`wasm32-unknown-unknown`) work fine. |

### Lua engine (additive)

| Feature | Default | Description |
|---------|---------|-------------|
| `lua-engine` | | Enables in-process Lua 5.4 scripting via [mlua](https://github.com/mlua-rs/mlua) (vendored, no system Lua needed). Can be combined with either WASM runtime. |

### Common build variants

```sh
# Default — wasmtime JIT, no Lua
cargo build --release

# Lighter build — wasmi interpreter, no Lua
cargo build --release --no-default-features --features runtime-wasmi

# wasmtime + Lua
cargo build --release --features lua-engine

# wasmi + Lua  (smallest full-featured build)
cargo build --release --no-default-features --features runtime-wasmi,lua-engine
```

Omitting both runtimes or enabling both are compile errors.

## Install

```sh
cargo install --path .
```

Drops `rtdvi` into `~/.cargo/bin/` (make sure that's on your PATH).

## Run

```sh
rtdvi path/to/file
rtdvi          # scratch buffer
```

## Logging

rtdvi writes a log to `./editor.log` in the working directory. Verbosity
is controlled by `$RTDVI_LOG`:

```sh
RTDVI_LOG=debug rtdvi foo.c
```

Useful when troubleshooting LSP or colorscheme load failures —
non-fatal errors are silenced from the UI but logged.

## Tests

```sh
cargo test           # all unit + integration tests (debug)
cargo test --release # release-mode timings for the perf tests
```

The integration tests use `ratatui::backend::TestBackend` to render
into an in-memory grid and assert on the resulting cells, so they
exercise the entire pipeline including the renderer.

## Layout

```
src/
  main.rs              CLI, terminal setup, event loop
  editor.rs            the Editor aggregate (state, registries)
  buffer.rs            ropey-backed text buffer, undo
  cursor.rs            Cursor + Selection types
  window.rs / tab.rs   split tree, tab pages
  mode/                normal/insert/visual/visual_line/visual_block/command/search
  keymap/              Key, KeyMods, KeyTrie, ActionRegistry
  command/             ExCommand trait + builtin commands
  completion.rs        :command-line tab completion + popup
  colorscheme.rs       vim .vim colorscheme parser
  syntax.rs            filetype detection + syntax engine
  lsp/                 LSP client / manager / transport
  ui/                  rendering (ratatui)
  motion.rs            cursor motions
  edit_actions.rs      i / a / o / O / u / <C-r>
  delete_actions.rs    dd / dw / dj / dk / d$ / d0 / dG / dgg / x / X
  yank_actions.rs      yy / yj / yk / yw / y$ / yG / Y
  visual_actions.rs    v / V / <C-v>, d / y / c, block I / A replay
  replace_actions.rs   r{c}
  bracket_actions.rs   % match, ]] [[ section motions
  search_actions.rs    /, ?, n, N
  window_actions.rs    <C-w>… split / focus / equalise
  lsp_actions.rs       gd / K / [d / ]d
text/width.rs          display-column math (CJK, tabs)
```

## Dependencies

| Crate              | Feature flag | Why |
|--------------------|-------------|-----|
| `ropey`            | always | rope-backed text buffer |
| `ratatui`          | always | TUI rendering |
| `crossterm`        | always | terminal I/O backend |
| `regex`            | always | search, syntax patterns |
| `unicode-width`    | always | display-column math |
| `unicode-segmentation` | always | grapheme iteration |
| `serde`, `toml`    | always | config |
| `serde_json`       | always | LSP messages |
| `lsp-types` 0.95   | always | LSP message structs (pinned: 0.97 swapped `Url`→`Uri`) |
| `mime_guess`       | always | extra filetype detection |
| `clap`             | always | CLI args |
| `thiserror` / `anyhow` | always | error types |
| `tracing` + appender | always | logging |
| `wasmtime`         | `runtime-wasmtime` | JIT WASM engine (~140 transitive crates) |
| `wasmi`            | `runtime-wasmi` | interpreter WASM engine (much lighter) |
| `mlua`             | `lua-engine` | in-process Lua 5.4 (vendored) |
| `tempfile`         | dev-only | test fixtures |
| `wat`              | dev-only | WAT text-format parsing in tests |

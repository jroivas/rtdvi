# Building jvim

## Requirements

- **Rust** 1.75 or newer (anything from 2024 will do). Install via
  [rustup.rs](https://rustup.rs).
- **Linux / macOS** terminal. Windows isn't tested but most of the
  stack (`crossterm`, `ropey`) is cross-platform — it'll likely need
  small tweaks.
- **Optional but recommended**:
  - System `vim` install — provides default colorschemes and the
    syntax files jvim parses. Typically `/usr/share/vim/vim*/colors/`
    and `/usr/share/vim/vim*/syntax/` on Linux.
  - `clangd` on PATH — auto-spawned for C/C++ buffers if present.

## Build

```sh
cargo build --release
```

The binary lands at `target/release/jvim`.

## Install

```sh
cargo install --path .
```

Drops `jvim` into `~/.cargo/bin/` (make sure that's on your PATH).

## Run

```sh
jvim path/to/file
jvim          # scratch buffer
```

## Logging

jvim writes a log to `./editor.log` in the working directory. Verbosity
is controlled by `$JVIM_LOG`:

```sh
JVIM_LOG=debug jvim foo.c
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

| Crate              | Why |
|--------------------|-----|
| `ropey`            | rope-backed text buffer |
| `ratatui`          | TUI rendering |
| `crossterm`        | terminal I/O backend |
| `regex`            | search, syntax patterns |
| `unicode-width`    | display-column math |
| `unicode-segmentation` | grapheme iteration |
| `serde`, `toml`    | config |
| `serde_json`       | LSP messages |
| `lsp-types` 0.95   | LSP message structs (pinned: 0.97 swapped `Url`→`Uri`) |
| `mime_guess`       | extra filetype detection |
| `clap`             | CLI args |
| `thiserror` / `anyhow` | error types |
| `tracing` + appender | logging |
| `tempfile`         | test fixtures (dev-only) |

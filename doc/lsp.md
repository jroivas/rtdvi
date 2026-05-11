# LSP

jvim ships with a **native LSP client**. By default it auto-spawns
clangd on C/C++ buffers. Add `[lsp.NAME]` blocks in your config to
register more servers.

## Default setup

Opening a `.c`/`.cpp`/`.h`/`.hpp` file in a directory whose ancestors
contain any of `.git`, `compile_commands.json`, or `compile_flags.txt`
triggers:

```
clangd -j=2 --background-index --background-index-priority=low
        --malloc-trim --pch-storage=disk
```

If `clangd` isn't on PATH, the spawn fails silently and the editor
runs without LSP for that buffer. Check `editor.log` for the warning.

## Keybindings

| Keys      | Action |
|-----------|--------|
| `gd`      | Go to definition. Opens the target file at the returned position, reusing an existing buffer if one matches. |
| `K`       | Hover. First non-empty line of the response is shown in the cmdline. |
| `]d`      | Next diagnostic line in this buffer (wraps). |
| `[d`      | Previous diagnostic line (wraps). |

All four work in Normal mode.

## Diagnostics

When a server publishes `textDocument/publishDiagnostics`, jvim:

- Stores them on the client (keyed by URI).
- Renders a **gutter column** with a one-char severity marker on
  every diagnostic line:
  - `!` red — Error
  - `?` yellow — Warning
  - `i` blue — Information
  - `h` cyan — Hint

When multiple diagnostics fall on the same line, the highest-severity
marker wins.

The gutter only appears when there's at least one diagnostic — it
doesn't clutter unhighlighted buffers.

## Configuration

```toml
[lsp.clangd]
cmd = ["clangd", "-j=2", "--background-index"]
filetypes = ["c", "cpp", "objc", "objcpp"]
root_markers = [".git", "compile_commands.json", "compile_flags.txt"]

[lsp.rust-analyzer]
cmd = ["rust-analyzer"]
filetypes = ["rust"]
# root_markers defaults to [".git"]

[lsp.pyright]
cmd = ["pyright-langserver", "--stdio"]
filetypes = ["python"]
root_markers = [".git", "pyproject.toml", "setup.py"]
```

Defining your own `[lsp.clangd]` **replaces** the built-in default
(useful for tweaking flags). Other servers are additive.

## Architecture

One **`Client`** per `(server_name, workspace_root)` pair:

```
   crossterm event loop (main thread)
        │
        ▼
   Editor::lsp_did_open(buffer)
        │
        ├── Manager::ensure(filetype, path)
        │       │
        │       ▼
        │    Client::spawn(cmd) ──── std::process::Command
        │                                 ├─ stdin  ─── synchronous writes from main thread
        │                                 ├─ stdout ─── reader thread
        │                                 └─ stderr ─── /dev/null
        │
        └── Client::did_open(uri, language_id, text)
                                                 ▼
                                         mpsc::channel
                                                 ▼
                                         Editor::lsp_poll()  ←─── called every render
```

A background **reader thread** parses framed JSON-RPC messages off
stdout and sends them as `InboundMessage` values down an mpsc
channel. The main thread drains the channel on every render
(`Editor::lsp_poll`), turning notifications into editor state
(diagnostics).

For requests like `gd` / `K`, the main thread blocks on the channel
for up to a configurable timeout (1.5s by default), processing any
notifications that arrive in the meantime, until the matching
response shows up.

## Message flow per buffer

1. `:e foo.c` opens the buffer.
2. `Editor::lsp_did_open` finds (or spawns) the right client, sends
   `textDocument/didOpen` with the file's text and language ID.
3. Server starts indexing. Diagnostics trickle in via
   `publishDiagnostics`.
4. User presses `gd` → `Client::goto_definition(uri, line, col)` →
   synchronous `textDocument/definition` request → first location
   in the response → cursor jumps.

## `didChange`

The plumbing exists (`Editor::lsp_did_change`) but **isn't yet wired
to every buffer edit**. Until that lands, the server sees the
on-disk content from `didOpen` and gets a fresh view on `:e` / `:w`.
For typical C/C++ workflows this is acceptable — diagnostics refresh
on save.

## Shutdown

When the editor exits, every running client gets `shutdown` + `exit`
notifications and the child is reaped. If you `:q!` quickly, the
process kill in `Drop` ensures nothing's left hanging.

## Adding more actions

The existing actions live in
[`src/lsp_actions.rs`](../src/lsp_actions.rs). Each follows the same
pattern: read cursor position + URI, find the client, fire a
synchronous request, act on the result.

Easy follow-ups (same plumbing, ~20 lines each):

- `gi` → `textDocument/implementation`
- `gD` → `textDocument/declaration`
- `gr` → `textDocument/references` (needs a list UI)
- `<leader>rn` → `textDocument/rename` (needs `WorkspaceEdit` apply)
- `<leader>gf` → `textDocument/formatting` (needs `TextEdit` apply)

## Testing without clangd

`tests/m34_lsp.rs` uses a tiny Python script as a mock LSP server.
It implements just enough of the protocol to respond to `initialize`,
emit diagnostics on `didOpen`, return canned `definition` and `hover`
results, and exit cleanly on `shutdown` / `exit`. Useful as a
template if you want to test your own server-specific behaviour
locally.

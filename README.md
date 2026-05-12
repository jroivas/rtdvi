# jvim

A small modal text editor in the spirit of vim, written in Rust. Covers
the everyday vim flow without trying to be a feature-complete clone:
modal editing, splits, tabs, visual modes (including block), file-path
and command-name completion, a real colorscheme + syntax layer that
reuses your system's vim files, and a native LSP client (clangd by
default).

## Highlights

- **Modal editing** — Normal, Insert, Visual (`v`), Visual-line (`V`),
  Visual-block (`<C-v>`), Command (`:`), Search (`/` `?`).
- **Counts** that compose: `4dj`, `2d3w`, `3yy`, `5<C-f>`, …
- **Operators** with motions: `d{motion}`, `y{motion}`, `c{motion}`,
  plus `r` (replace), `x` / `X`.
- **Visual-block insert/append** that replays across rows on `<Esc>`,
  collapsed into a single undo step.
- **Undo transactions** — block ops and full insert sessions are one
  undo entry each.
- **Splits + tabs** with `<C-w>hjkl`, `<C-w>=`, `gt`/`gT`, `:split`,
  `:vsplit`, `:tab new|next|prev`.
- **Per-window statusline**, distinct style for the active window,
  full file path with smart truncation.
- **Search** with `/` `?` `n` `N` (regex), `*` / `£` / `#` for the
  word under the cursor.
- **Jumplist** with `<C-o>` / `<Tab>` to hop back / forward through
  the places you've been. See [`doc/jumplist.md`](doc/jumplist.md).
- **Page motions** — `<C-f>` / `<C-u>` / `<PageDown>` / `<PageUp>`,
  count-aware.
- **Scroll anchors** — `zz` / `zt` / `zb` to center / top-align /
  bottom-align the cursor's line.
- **Persistent text highlights** — `:highlight <text>` and `\m` on
  the word under the cursor, ten-colour palette, oldest evicted when
  full. See [`doc/highlights.md`](doc/highlights.md).
- **Whitespace marks** — opt-in `highlight_trailing_whitespace` and
  `highlight_tabs` paint problematic whitespace red. See
  [`doc/whitespace-marks.md`](doc/whitespace-marks.md).
- **Spaces by default** — `<Tab>` in insert mode inserts spaces to the
  next tab stop (`expandtab = true` by default). `<Shift-Tab>` always
  inserts a literal tab regardless of the setting.
- **Named registers + system clipboard** — `"ayy` / `"ap` for vim-style
  named registers (`a`..`z`). One letter (`q` by default) routes
  through `wl-copy`/`wl-paste` so copy-paste between jvim and the rest
  of your desktop just works. See
  [`doc/registers.md`](doc/registers.md).
- **Fuzzy file finder** — `:ff` interactive popup over the working
  directory, gitignore-aware, multi-token AND. See
  [`doc/fzf.md`](doc/fzf.md).
- **Colorscheme support** — reads `~/.config/jvim/colors/<name>.vim`,
  `./colors/`, and `/usr/share/vim/vim*/colors/`. `:colorscheme` / `:colo`.
- **Syntax highlighting** that picks up `syn keyword` from
  `/usr/share/vim/vim*/syntax/<lang>.vim` plus a built-in regex layer
  (strings, numbers, comments, function calls, preprocessor lines).
- **Filetype detection** — extension + basename table, MIME mapping,
  config glob overrides, `:set syntax=…` override.
- **Tab completion** in the `:` command line for filenames AND command
  names. Tab tab opens a navigable popup; arrows pick, Enter accepts.
  Directories descend on subsequent `<Tab>`.
- **Native LSP client** — auto-spawns clangd for `c`/`cpp`/`objc`/`objcpp`
  by default, shows diagnostic markers in the gutter; `gd` / `gD` /
  `gi` / `gf` (type) / `gr` jumps, `K` hover, `]d` / `[d` next/prev
  diagnostic, `:LspRename`, `:LspReferences`, `:LspDiagnostic`.
  Configurable per server.
- **Configurable leader key** — `options.leader` (default `\`)
  expands `<leader>` in user `[[keymaps]]` entries.
- **TOML config** at `~/.config/jvim/config.toml` (also honoured at
  `$XDG_CONFIG_HOME/jvim/config.toml` or `$JVIM_CONFIG`).
- **380+ tests** cover modes, motions, edits, splits, search,
  completion, LSP plumbing, colorscheme + syntax layers,
  highlights, jumplist, fuzzy finder, whitespace marks, and render
  output via ratatui's `TestBackend`.

## Quick start

Requirements: Rust 1.75+ (anything reasonably recent). Optional:
clangd on PATH for C/C++ LSP. System `vim` install for the bundled
syntax + colorscheme files.

```sh
cargo build --release
./target/release/jvim path/to/file.c
```

Open a file:

```sh
jvim test.c              # auto-detects filetype, loads syntax + clangd
jvim                     # scratch buffer
```

The first time you open a C/C++ file inside a directory with a
`compile_commands.json` (or `.git`), clangd is spawned in the
background. Diagnostics appear as `!` / `?` markers in the left gutter.

## Cheat sheet (the bits you'll reach for daily)

```
Normal-mode motions
  h j k l               left / down / up / right (count-aware)
  w b e                 word forward / back / to end
  0 $                   line start / end
  gg G                  first / last line (or "5G" to line 5)
  %                     matching bracket
  ]] [[                 next / previous section (filetype-aware)
  /pat ?pat n N         search forward / backward, repeat
  *  £  #               search word under cursor (£ = *, # = backward)
  <C-f> <C-u>           page down / page up
  zz zt zb              centre / top / bottom align current line
  <C-o>  <Tab>          jumplist back / forward

Editing
  i a I A o O           enter insert (at various positions)
  r{c}                  replace char(s) — count-aware, works in visual modes too
  x X                   delete char under / before cursor
  dd dj dk dw d$ d0     delete line / down / up / word / to EOL / to BOL
  dG dgg                delete to end / start of file
  yy yj yk yw y$        yank line / down / up / word / to EOL
  p                     paste below (line-wise) or after (char-wise)
  "ayy "ap              named register (a..z): yank to / paste from
  "qyy "qp              system clipboard via wl-copy / wl-paste
  u <C-r>               undo / redo
  4dj  2d3w  3yy        counts compose vim-style

Visual
  v V <C-v>             visual char / line / block
  d y c r{c}            delete / yank / change / replace selection
  I A                   block insert at left edge / append at right edge
                        (typed text replays across every selected row)

Splits / tabs / windows
  :split  :vsplit       horizontal / vertical split
  <C-w>h j k l          move focus (held-Ctrl form works too)
  <C-w>=                equalise every split
  :tabnew  gt  gT       new tab / next / prev
  :tab new|next|prev    space-separated alternatives

Ex command line
  :w :wq :q :q!         write / write+quit / quit / force quit
  :e <file>  :vi <file> open file
  :bnext :bprev         cycle buffers
  :set syntax=c++       per-buffer filetype override
  :colorscheme desert   load color scheme
  :ff [query]           fuzzy file finder (:ff! busts the cache)
  :highlight <text>     toggle persistent text highlight (:hl)
  :nohighlight          clear all highlights (:nohl)
  <Tab>                 file or command completion (first match)
  <Tab><Tab>            open completion popup; arrow keys pick

LSP (when a server is running for this filetype)
  gd                    go to definition
  gD                    go to declaration
  gi                    go to implementation
  gf                    go to type definition
  gr                    list references
  K                     hover
  ]d  [d                next / previous diagnostic
  :LspRename <new>      rename symbol under cursor
  :LspReferences        list references
  :LspDiagnostic        show diagnostic at cursor

Highlights
  \m                    toggle highlight on word under cursor (<leader>m)
```

## Configuration

Drop a TOML file at `~/.config/jvim/config.toml`:

```toml
[options]
tab_width = 4
number = true
leader = ","
highlight_trailing_whitespace = true
highlight_tabs = false

[filetypes]
"*.cpp" = "c++"
"*.md"  = "text/markdown"
"Cargo.toml" = "rust"

[[keymaps]]
mode = "normal"
keys = "<leader>w"
action = "delete_word_forward"

[[keymaps]]
mode = "normal"
keys = "<leader>m"
action = "highlight_toggle_word_under_cursor"

[lsp.clangd]
cmd = ["clangd", "-j=2", "--background-index"]
filetypes = ["c", "cpp", "objc", "objcpp"]
root_markers = [".git", "compile_commands.json"]
```

See [`doc/configuration.md`](doc/configuration.md) for the full schema.

## Documentation

In `doc/`:

- [building.md](doc/building.md) — build, dependencies, where things live
- [configuration.md](doc/configuration.md) — TOML config reference
- [modes.md](doc/modes.md) — how the modal state machine works
- [motions.md](doc/motions.md) — every motion + count semantics
- [editing.md](doc/editing.md) — operators (`d`/`y`/`c`/`r`), `x`/`X`, undo
- [visual-mode.md](doc/visual-mode.md) — `v` / `V` / `<C-v>`, block replay
- [splits-and-tabs.md](doc/splits-and-tabs.md) — windows, navigation, equalise
- [command-line.md](doc/command-line.md) — `:` commands, tab completion popup
- [jumplist.md](doc/jumplist.md) — `<C-o>` / `<Tab>` jump back / forward
- [registers.md](doc/registers.md) — named registers, system clipboard
- [highlights.md](doc/highlights.md) — `:highlight` and `\m`, palette, toggling
- [whitespace-marks.md](doc/whitespace-marks.md) — trailing-whitespace and tab marks
- [fzf.md](doc/fzf.md) — `:ff` fuzzy file finder
- [colorschemes.md](doc/colorschemes.md) — `:colorscheme`, search paths
- [syntax-highlighting.md](doc/syntax-highlighting.md) — how filetype + highlighting work
- [lsp.md](doc/lsp.md) — clangd setup, supported actions, configuring more servers
- [extending.md](doc/extending.md) — for hacking on jvim itself

## Status

Working day-to-day editor for the author's flow. Known limitations:

- `:s` (substitute), macros, registers beyond unnamed, named marks,
  folds, tree-sitter — out of scope.
- LSP `textDocument/formatting`, code actions, and signature help
  aren't wired up yet (rename / references / hover / goto-* are).
- `didChange` isn't pushed to LSP servers on every keystroke yet —
  diagnostics refresh on `:w` / `:e`.

PRs and bug reports welcome.

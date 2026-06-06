# Registers (yank / paste)

rtdvi has named registers like vim — and one of them is wired to the
system clipboard, so copy/paste between rtdvi and the rest of your
desktop doesn't need a plugin or extra keystrokes.

## In-memory named registers

Prefix any yank, delete, or paste with `"<letter>` to target a named
register. Letters `a`..`z` (case-insensitive — upper folds to lower)
all work.

```
"ayy   yank current line into register a
"add   delete current line into register a
"ap    paste register a after the cursor
"by3w  yank three words into register b
"bd$   delete to end-of-line into register b
```

The **unnamed** register (vim's `""`) is always updated as well, so a
plain `p` pastes whatever you most recently yanked or deleted —
regardless of which named slot it also landed in.

Visual mode works the same way: select with `v` / `V` / `<C-v>`, then
`"ay` or `"ad`.

If you start a prefix and bail (`"` then `<Esc>` or any non-letter),
the register selection is cancelled and a status message says so.

## System clipboard register

One letter — `q` by default, configurable — is special. Anything
yanked into it gets piped to `wl-copy` (Wayland), `xclip` (X11), or
`pbcopy` (macOS). Anything pasted from it reads the same clipboard
via `wl-paste` / `xclip -o` / `pbpaste`.

```
"qyy   yank current line to the system clipboard
"qd$   delete-and-cut to end-of-line
"qp    paste from the system clipboard
```

Why this design:

- **Internal clipboard is the default.** Most yanks don't need to
  leave the editor; routing them all to wl-copy clutters the system
  clipboard.
- **Same `"<letter>` syntax everywhere.** No `+` / `*` distinction
  from vim — you don't have to remember a separate notation for the
  system clipboard.
- **No conflict with macros.** `q` is also vim's macro-record key, and
  rtdvi has [keyboard macros](macros.md) — but the two don't collide:
  **bare `q`** records a macro, while **`"q`** (quote-prefixed) selects
  the clipboard register. Change it via
  `options.system_clipboard_register` if you'd prefer a different
  letter.

The unnamed register still updates after `"qyy`, so a subsequent
plain `p` pastes locally without re-reading the system clipboard.

## Clipboard tools

By default rtdvi probes your environment:

| Environment | Copy command | Paste command |
|-------------|--------------|---------------|
| `$WAYLAND_DISPLAY` set | `wl-copy` | `wl-paste --no-newline` |
| `$DISPLAY` set         | `xclip -selection clipboard` | `xclip -selection clipboard -o` |
| macOS                  | `pbcopy` | `pbpaste` |

Override with explicit `[options]` entries when the defaults don't
fit (e.g. you prefer `wl-copy --primary`, or you're on a server with
`xsel` installed instead of `xclip`):

```toml
[options]
clipboard_copy_cmd  = ["wl-copy"]
clipboard_paste_cmd = ["wl-paste", "--no-newline"]
```

If the configured copy tool isn't on `PATH`, the yank still updates
the local unnamed register, but a status-line message reports the
failure so you don't silently lose data.

## Linewise heuristic

Internal yanks remember whether they were line-wise (`yy`) or
character-wise (`yw`), so `p` knows whether to paste on the next
line or after the cursor.

The system clipboard has no metadata, so rtdvi falls back to a
heuristic on `"qp`: **text ending in `\n` is treated as line-wise**.
Same as vim's `*` / `+` behaviour.

## Implementation

- [`src/registers.rs`](../src/registers.rs) — `Registers` state, the
  `"<letter>` pre-trie hook (`try_consume_key`), and the
  `store` / `read_for_paste` funnels every yank/delete/paste passes
  through.
- [`src/editor.rs`](../src/editor.rs) — `unnamed_register` (the
  default destination) and `named_registers` (the `a`..`z` map).
- [`src/yank_actions.rs`](../src/yank_actions.rs),
  [`src/delete_actions.rs`](../src/delete_actions.rs),
  [`src/visual_actions.rs`](../src/visual_actions.rs) — call sites.

There's exactly one place in the code that decides whether a yank
goes to memory or the OS clipboard: `registers::store`. Adding more
special destinations (a search register, a numbered ring) is purely
additive there.

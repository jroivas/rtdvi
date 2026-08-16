# Modes

rtdvi is modal. Every keystroke is interpreted by the **current mode's**
handler.

## Mode summary

| Mode          | Enter with     | Leave with        | What it does |
|---------------|----------------|-------------------|--------------|
| Normal        | (default; `<Esc>` from anywhere) | (stays in normal) | Motions, operators, ex command entry |
| Insert        | `i` `a` `I` `A` `o` `O` (or `c`) | `<Esc>` | Types characters into the buffer |
| Replace       | `R`            | `<Esc>` | Overtype: typed chars overwrite existing ones |
| Visual        | `v`            | `<Esc>` / operator | Character-wise selection |
| Visual-line   | `V`            | `<Esc>` / operator | Line-wise selection |
| Visual-block  | `<C-v>`        | `<Esc>` / operator | Rectangular selection |
| Command       | `:`            | `<Esc>` / `<CR>`   | Ex command line |
| Search        | `/` `?`        | `<Esc>` / `<CR>`   | Forward/backward search |

The current mode is shown as a coloured badge on the **active window's**
statusline.

## How dispatch works

The Normal and three Visual modes all route through the keymap trie
(see [`src/keymap/trie.rs`](../src/keymap/trie.rs)). Each mode owns a
key→action mapping in [`Editor.keymap`](../src/keymap/mod.rs). The
mode's `handle_key` pushes the typed key onto the pending sequence,
asks the trie to resolve, and either dispatches the matched action,
waits for more keys, or rejects.

Count digits are intercepted by `try_accumulate_count` in
[`src/mode/mod.rs`](../src/mode/mod.rs) before keys reach the trie.

Insert mode bypasses the trie for typed characters — every printable
key inserts at the cursor. A few special keys (`<Esc>`, `<Enter>`,
`<Backspace>`, `<Tab>`) are handled directly. **Replace mode** (`R`)
works the same way but overwrites the character under the cursor
instead of inserting; `<Backspace>` restores what was overtyped. See
[editing.md](editing.md#replace-mode-r).

Command and Search modes own their own line editors (the `:` prompt
and `/` prompt at the bottom of the screen). Both support the same
readline-style keys — see [command-line.md](command-line.md#line-editing).

## Pending state in Normal mode

Three pieces of state are tracked between keystrokes:

- `pending_keys: Vec<Key>` — partial key sequence being assembled.
- `pending_count_pre: Option<usize>` — count typed before an operator
  (the `4` in `4dj`).
- `pending_count_post: Option<usize>` — count typed between operator
  and motion (the `3` in `d3w`). Multiplied with `pre` when fired.
- `pending_replace: bool` — true when `r` has been pressed and the next
  key is the replacement character.
- `pending_block_insert: Option<PendingBlockInsert>` — set by `I`/`A`
  in visual-block to drive cross-row replay on `<Esc>`.

Any rejection (`Resolve::None`) clears both `pending_keys` and the
counts so the user can start over cleanly.

## Mode transitions

Every mode change emits a `ModeChanged` event on the editor's event
bus, so future listeners (a status-aware plugin layer) can react.

## Esc semantics

- Insert / Replace → Normal: cursor steps left by 1 (vim convention),
  and any open undo transaction commits.
- Visual modes → Normal: selection is cleared.
- Command / Search → Normal: input is dropped; mode returns.
- Visual-block insert/append: replays typed text across the rectangle
  BEFORE switching to Normal, so the replay is part of the same
  insert session and ends up as a single undo entry.

## Looking for a key binding?

- See [motions.md](motions.md), [editing.md](editing.md),
  [visual-mode.md](visual-mode.md), [splits-and-tabs.md](splits-and-tabs.md),
  [registers.md](registers.md), [macros.md](macros.md),
  [command-line.md](command-line.md), [lsp.md](lsp.md).
- Or grep `src/*_actions.rs` for `bind_default_keys` to see every
  default binding.

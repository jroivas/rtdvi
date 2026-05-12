# Editing

## Insert mode entry

| Keys | Where the cursor lands |
|------|------------------------|
| `i`  | Before current char |
| `a`  | After current char |
| `I`  | Beginning of current line |
| `A`  | End of current line |
| `o`  | New blank line below, cursor on it |
| `O`  | New blank line above, cursor on it |

Each entry **opens an undo transaction** that closes on `<Esc>`. The
whole session — every typed character, every backspace — becomes ONE
undo entry (matches vim).

In insert mode:
- Typed chars insert at the cursor.
- `<Enter>` splits the line.
- `<Backspace>` deletes the char before the cursor, joining lines at
  column 0.
- `<Tab>` inserts **spaces up to the next multiple of `tab_width`**
  by default. Set `options.expandtab = false` to insert a literal
  `\t` instead.
- `<Shift-Tab>` **always inserts a literal `\t`**, regardless of
  `expandtab` — the manual escape hatch for files (Makefiles, Go)
  that need a real tab character.
- `<Esc>` returns to Normal; cursor steps left by 1 (vim convention).

## Delete operators

All count-aware. Pre-operator count × post-operator count multiplies.

| Keys      | Action |
|-----------|--------|
| `dd`      | Delete current line (linewise) |
| `dj`      | Delete current line + line below (= 2 lines; `4dj` = 5 lines) |
| `dk`      | Delete current line + line above |
| `dw`      | Delete to start of next word |
| `db`      | Delete back to previous word start |
| `de`      | Delete through end of current word (inclusive) |
| `d$` / `D`| Delete to end of line |
| `d0`      | Delete to start of line |
| `dG`      | Delete to end of buffer (or to line N if count given) |
| `dgg`     | Delete to start of buffer (or to line N) |
| `x`       | Delete char under cursor (`3x` = 3 chars, capped at EOL) |
| `X`       | Delete char before cursor |

Deleted text goes into the **unnamed register** by default, so `p`
pastes it back. Prefix with `"<letter>` to target a named register
(`"ayy`, `"ap`) or the system clipboard (`"qyy` / `"qp` — see
[registers.md](registers.md)).

## Yank operators

Identical ranges to the delete operators, but the buffer is not
modified and the cursor stays put.

| Keys     | Action |
|----------|--------|
| `yy` / `Y` | Yank current line (linewise) |
| `yj`     | Yank current + line below |
| `yk`     | Yank current + line above |
| `yw`     | Yank to start of next word |
| `yb`     | Yank back to previous word start |
| `ye`     | Yank through end of current word (inclusive) |
| `y$`     | Yank to end of line |
| `y0`     | Yank to start of line |
| `yG`     | Yank to end of buffer |
| `ygg`    | Yank to start of buffer |

## Paste

| Keys | Action |
|------|--------|
| `p`  | Paste below current line (linewise) OR after cursor (charwise), depending on how the register was filled. |

## Change

| Keys      | Action |
|-----------|--------|
| `c{motion}` | Delete the motion's range, then enter insert mode. |
| `cc`      | (= `c$` for the line) — currently not bound separately; use `dd` + `i`. |

In visual modes, `c` deletes the selection and drops into Insert. For
visual-block, that's the start of a block-replace session.

## Replace

| Keys      | Action |
|-----------|--------|
| `r{c}`    | Replace char under cursor with `{c}`. Cursor stays put. |
| `5rX`     | Replace 5 chars (capped at EOL). |
| `r<Enter>`| Replace with a newline (splits the line). |
| `r<Tab>`  | Replace with a tab. |
| `r<Esc>`  | Cancel — no change. |

In **visual / visual-line** modes: `r{c}` replaces every selected
character with `{c}`, preserving newlines, in a single undo step.

In **visual-block** mode: `r{c}` replaces every cell of the rectangle
with `{c}`, again as a single undo step.

## Undo / redo

| Keys    | Action |
|---------|--------|
| `u`     | Undo last transaction |
| `<C-r>` | Redo |

A **transaction** can include many edits — see
[visual-mode.md](visual-mode.md) for block ops, which collapse the
whole rectangle change (top-row typing + cross-row replay) into one
entry.

## Counts

Counts compose vim-style:

```
3yy        → yank 3 lines
4dj        → delete 5 lines (current + 4 below)
2d3w       → delete 6 words
5rX        → replace 5 chars with X
3p         → paste 3 times (count on `p` isn't implemented yet)
```

(`p` doesn't currently respect a count — single paste only.)

## File save

| Ex command | Action |
|------------|--------|
| `:w`       | Write current buffer to disk |
| `:w <path>`| Save as |
| `:wq`      | Write then quit |
| `:q!`      | Quit discarding changes |

See [command-line.md](command-line.md) for the full ex command surface.

## Behaviour notes

- All delete / change / replace operations go through `Buffer::insert`
  / `delete` / `replace`. Each call appends to the current undo
  transaction (or creates a one-edit transaction if none is open).
- Inside an insert session, the per-char inserts are appended to the
  transaction opened by the entry action (`i`/`a`/etc.). One `u`
  reverts the whole session.
- Block delete and block insert/append each manage their own
  transaction, nested cleanly so block change (`c`) wraps both into a
  single step.
- Cursor positioning after undo lands on the start of the FIRST edit
  in the transaction — i.e. the "top" of the change.

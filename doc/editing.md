# Editing

## Insert mode entry

| Keys | Where the cursor lands |
|------|------------------------|
| `i`  | Before current char |
| `a`  | After current char |
| `I`  | Beginning of current line |
| `A`  | End of current line |
| `o`  | New line below, cursor on it (continues a comment leader — see below) |
| `O`  | New line above, cursor on it (continues a comment leader — see below) |

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

## Comment continuation

When you open a line inside a comment — with `o`, `O`, or `<Enter>` — the
comment leader is carried onto the new line, so you don't retype it:

```
/**                 press o here
 * Multiline          → new line starts with " * "
 */
```

Recognised in C-family filetypes (`c`, `cpp`, `java`, `javascript`,
`typescript`, `go`, `rust`):

- **Block comments** `/* … */` — the opening `/*` and middle `*` lines
  continue with an aligned `* `. Continuation stops once the line
  closes the block (`*/`), and a single-line `/* … */` doesn't continue
  at all.
- **Line comments** `//` — continues with `// `. In Rust, the doc
  markers `///` and `//!` continue as themselves.

The leader is only recognised at the **start** of the line, so a
trailing comment (`x = 1;  // note`) does not trigger continuation. This
is built in (no `comments`/`formatoptions` config, no runtime files) and
is gated on `options.smartindent`, like the other smart-indent rules.

## Open-paren alignment

In C-family filetypes and Rust, pressing `<Enter>` on a line with an
**unclosed `(`** aligns the continuation just past that paren — under
the first argument — like vim's default `cindent`:

```
int something(int val1,      press <Enter> here
              int val2)       → continuation aligns under "int val1"
```

Alignment kicks in only when there is content after the open paren (a
bare trailing `foo(` still gets a normal one-level indent), it targets
the innermost unclosed paren, and it ignores parens inside string/char
literals or after a `//` comment. Gated on `options.smartindent`.

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
| `dl` / `d<Space>` | Delete the char under the cursor (like `x`; `3dl` = 3 chars) |
| `dh`      | Delete the char before the cursor (like `X`) |
| `d$` / `D`| Delete to end of line |
| `d0`      | Delete to start of line |
| `dG`      | Delete to end of buffer (or to line N if count given) |
| `dgg`     | Delete to start of buffer (or to line N) |
| `x`       | Delete char under cursor (`3x` = 3 chars, capped at EOL) |
| `X`       | Delete char before cursor |

## Indent / dedent

| Keys | Action |
|------|--------|
| `>>` | Indent current line by one shiftwidth (`3>>` = three lines) |
| `<<` | Dedent current line by one shiftwidth |
| `>` (in visual) | Indent every line touched by the selection |
| `<` (in visual) | Dedent every line touched by the selection |

The indent unit follows `options.expandtab`:
- `expandtab = true` (default) → `tab_width` spaces are inserted
- `expandtab = false` → a literal `\t` is inserted

Dedent removes up to one shiftwidth's worth of leading whitespace,
counting display columns: a leading tab counts as `tab_width`
columns immediately, so a single `<<` removes either one tab or up
to `tab_width` spaces (whichever is at the start). Lines with less
than a full shiftwidth of indent lose what they have.

All affected lines collapse into a single undo entry.

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
| `yl` / `y<Space>` | Yank the char under the cursor (`3yl` = 3 chars) |
| `yh`     | Yank the char before the cursor |
| `y$`     | Yank to end of line |
| `y0`     | Yank to start of line |
| `yG`     | Yank to end of buffer |
| `ygg`    | Yank to start of buffer |

## Paste

| Keys | Action |
|------|--------|
| `p`  | Paste below current line (linewise) OR after cursor (charwise), depending on how the register was filled. |
| `P`  | Paste **above** current line (linewise) OR **before** cursor (charwise) — the same register, placed on the other side. |

Both honour the `"<letter>` register prefix (`"aP`, `"qp`, …).

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

## Replace mode (`R`)

`R` enters **Replace (overtype) mode** — the statusline shows
`REPLACE`. Typed characters **overwrite** the character under the cursor
and advance; once the cursor passes the old end of line, further
characters extend the line as in insert mode.

| Keys | Action |
|------|--------|
| `R`  | Enter Replace mode |
| *(any char)* | Overwrite the char under the cursor, advance |
| `<Backspace>` | Restore the overtyped character (or delete an appended one); rejoins on a broken line |
| `<Enter>` | Break the line at the cursor |
| `<Esc>` / `<C-c>` | Leave to Normal; cursor steps left one column |

`<Backspace>` walks back through your changes, putting the original
characters back; once you backspace past where you started it just
moves left without destroying untouched text. The whole session is a
single undo step. Replace mode is refused on read-only render buffers,
like Insert.

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

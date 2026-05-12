# Motions

Every motion is **count-aware**. Type a number first, then the motion:
`5j` moves down 5 lines, `3w` advances 3 words.

## Basic cursor movement

| Keys              | Action |
|-------------------|--------|
| `h` / `<Left>`    | Left one cell |
| `l` / `<Right>`   | Right one cell |
| `j` / `<Down>`    | Down one line (sticky column) |
| `k` / `<Up>`      | Up one line |
| `0`               | Line start (column 0) |
| `$`               | Line end (sticky to EOL on subsequent `j`/`k`) |
| `gg`              | First line. `5gg` → line 5. |
| `G`               | Last line. `5G` → line 5. |

The **sticky column** is the desired display column when moving
vertically through shorter lines. Vim's behaviour: after `$`, every
`j`/`k` lands on the EOL of the next line until you type a horizontal
motion that resets the sticky column.

## Word motions

| Keys | Action |
|------|--------|
| `w`  | Start of next word |
| `b`  | Start of previous word |
| `e`  | End of current/next word |

Word boundaries follow vim's "keyword vs punctuation vs blank"
classes. CJK characters work; tabs expand to display columns.

## Paging

| Keys              | Action |
|-------------------|--------|
| `<C-f>` / `<PageDown>` | Down one screen (minus 2 lines of overlap) |
| `<C-u>` / `<PageUp>`   | Up one screen |

Counts work: `3<C-f>` jumps three pages.

The cursor lands at column 0 of the new top line, and `top_line`
follows so the viewport scrolls (not just the cursor).

## Scroll anchors

| Keys | Action |
|------|--------|
| `zz` | Centre the current line in the window |
| `zt` | Scroll so the current line is at the top |
| `zb` | Scroll so the current line is at the bottom |

These move the **viewport** around the cursor — the cursor itself
doesn't move. Useful after a long jump (`gd`, `<C-o>`, `:5G`, …) to
get the line you care about into reading position.

## Jumplist

jvim keeps a chronological list of "interesting" cursor positions so
you can hop back to where you came from.

| Keys     | Action |
|----------|--------|
| `<C-o>`  | Jump back to the previous entry |
| `<Tab>`  | Jump forward (terminals send `<Tab>` for Ctrl-I) |

Entries are pushed automatically before:

- `gd` / `gD` / `gi` / `gf` / `gr` (LSP navigation)
- `:ff` opening a result
- `*` / `£` / `#` (word-under-cursor search)
- absolute-line `gg` / `G` jumps (e.g. `42G`)
- `/` / `?` accepting a search match

Same-line jumps are coalesced into the most recent entry; the list
caps at 100 entries (oldest evicted).

## Symbol search

| Keys | Action |
|------|--------|
| `*`  | Search forward for the word under the cursor (word-bounded) |
| `£`  | Same as `*`. Bound for keyboards where `£` is easier to reach than `*`. |
| `#`  | Search backward for the word under the cursor |

The pattern is `\bword\b` with word characters being
`[A-Za-z0-9_]` — the same definition `w`/`b`/`e` use. The cursor
jumps to the first hit and the pattern is recorded for `n`/`N`.

## Bracket and section motions

| Keys | Action |
|------|--------|
| `%`  | Jump to matching `()`, `[]`, `{}`. If the cursor isn't on a bracket, scans the rest of the line for the first one. |
| `]]` | Next section (next item / function declaration). |
| `[[` | Previous section. |

Section rules are **filetype-aware**:

- **Rust** (`.rs`): stops on `fn` / `struct` / `enum` / `impl` /
  `trait` / `mod` / `use` / `const` / `static` / `type` /
  `macro_rules!` at any indent, with optional `pub`, `pub(crate)`,
  `async`, `unsafe`, `extern "C"` prefixes. So methods inside `impl`
  blocks are stops. `if`/`while`/closure/`Self {}` blocks are NOT.
- **Python** (`.py`): `def`, `async def`, `class` at any indent.
- **C / C++** / **Java** / **Go** / **JS** / **TS**: line at column 0
  that contains `{` (function definition) OR begins with `{` at col 0.
- Other languages fall back to vim's generic "line begins with `{`".

`]]` / `[[` are count-aware too.

## Search

| Keys      | Action |
|-----------|--------|
| `/pat<CR>` | Forward search for `pat` (full regex via the `regex` crate). |
| `?pat<CR>` | Backward search. |
| `n`        | Next match in the current direction. |
| `N`        | Previous match. |
| `<Esc>` (while typing) | Cancel the prompt, no pattern stored. |

The pattern wraps around the end / beginning of the buffer.

Matched cells are NOT visually highlighted in the viewport in v1; the
cursor just jumps. (Adding match highlighting is a small extension on
top of the search action.)

## Counts in detail

Counts compose vim-style. The number typed BEFORE the operator/motion
and the number typed BETWEEN operator and motion are MULTIPLIED:

```
3j        → cursor.row += 3
4dj       → delete current + 4 lines below (5 lines total)
2d3w      → 6 words deleted
5G        → goto line 5 (absolute, not relative)
```

A leading `0` is always the line-start motion, not a count digit.

## Visual modes

All motions above work in `v`, `V`, and `<C-v>` modes too. They extend
the selection instead of just moving the cursor.

## Implementation notes

- Motions register actions in [`src/motion.rs`](../src/motion.rs).
  Each is a named action (`move_left`, `word_forward`, …) bound via
  the default keymap; user TOML keymaps can rebind any of them.
- `goto_first_line` / `goto_last_line` (the `gg` and `G` handlers)
  interpret the count as an absolute line number, distinct from the
  step-repeat semantics of the rest.
- The viewport scrolls via `Window::scroll_into_view` at render time,
  so motion code doesn't have to think about it.

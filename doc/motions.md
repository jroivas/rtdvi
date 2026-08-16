# Motions

Every motion is **count-aware**. Type a number first, then the motion:
`5j` moves down 5 lines, `3w` advances 3 words.

## Basic cursor movement

| Keys              | Action |
|-------------------|--------|
| `h` / `<Left>`    | Left one **grapheme** (jumps over a whole tab or CJK char in one press) |
| `l` / `<Right>`   | Right one **grapheme** |
| `j` / `<Down>`    | Down one line (sticky column) |
| `k` / `<Up>`      | Up one line |
| `0`               | Line start (column 0) |
| `$`               | Line end (sticky to EOL on subsequent `j`/`k`) |
| `gg`              | First line. `5gg` → line 5. |
| `G`               | Last line. `5G` → line 5. |

A **counted** `{n}G` / `{n}gg` centres the destination line in the
window (like `zz`), clamping near the buffer end so no blank space
shows past the last line. Plain `gg`/`G` scroll minimally as before.

The **sticky column** is the desired display column when moving
vertically through shorter lines. Vim's behaviour: after `$`, every
`j`/`k` lands on the EOL of the next line until you type a horizontal
motion that resets the sticky column.

### Multi-cell graphemes (tabs, CJK)

A tab character with `tab_width = 4` displays as four cells, but it's
still **one** grapheme. `h` and `l` treat it as a single step:

```
"\tdata"     →  shown as "    data"
                cols     0123 4567
l from col 0  → col 4 (on 'd'), not col 1
h from col 4  → col 0 (start of tab)
```

If you land **inside** a tab via vertical motion (sticky column = 2,
say), `l` snaps forward to the end of the tab and `h` snaps back to
its start. The cursor never gets stuck mid-grapheme after a motion.

CJK and other wide characters work the same way — `l` over `漢字`
moves two display cells in one press.

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

rtdvi keeps a chronological list of "interesting" cursor positions so
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
| `#`  | Search backward for the word under the cursor |
| `£`  | Same as `#` — search **backward**. Vim aliases `£` (char 163) to `#`. |

The pattern is `\bword\b` with word characters being
`[A-Za-z0-9_]` — the same definition `w`/`b`/`e` use. The cursor
jumps to the first hit, the match is centred in the window, and the
pattern is recorded both for `n`/`N` and in the persistent search
history (so `/`<Up> can recall it).

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
| `/<CR>` / `?<CR>` | Empty pattern → repeat the **last** search (from `/`, `?`, or `*`/`#`/`£`) forward / backward. |
| `n`        | Next match in the current direction. |
| `N`        | Previous match. |
| `<Esc>` / `<C-c>` (while typing) | Cancel the prompt; the cursor snaps back to where the search started. |

The pattern wraps around the end / beginning of the buffer, and every
accepted match is **centred** in the window (clamped near the buffer
end), the same as a counted `{n}G`.

### Incremental search (incsearch)

Searching is incremental: as you type or amend the pattern, the cursor
previews the first match **from where the search started** and the view
scrolls (centred) to show it. Editing the pattern — more characters,
`<Backspace>`, or history browse — re-searches from that same origin,
so the preview always reflects the first hit for the current text. An
empty or no-match pattern leaves the view at the origin, and cancelling
(`<Esc>` / `<C-c>`) restores it exactly. Pressing `<Enter>` commits the
jump (and records the origin on the [jumplist](#jumplist)).

The `/`/`?` prompt itself supports the full readline editing set — see
[command-line.md](command-line.md#line-editing).

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

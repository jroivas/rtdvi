# Visual mode

Three flavours, identical operators on top.

## Entering

| Keys     | Mode           | Selection shape |
|----------|----------------|-----------------|
| `v`      | Visual         | Character-wise (anchor → cursor) |
| `V`      | Visual-line    | Whole lines from anchor to cursor |
| `<C-v>`  | Visual-block   | Rectangle (anchor.row..cursor.row, anchor.col..cursor.col) |

Press `<Esc>` to leave any visual mode without acting.

## Operators on selection

| Keys | Action |
|------|--------|
| `d` / `x` | Delete the selection. Goes into the unnamed register. |
| `y`  | Yank to unnamed register. |
| `c`  | Delete, then drop into Insert. |
| `r{c}` | Replace every selected char with `{c}`. Newlines kept. |

After `d` / `y` / `r` you return to Normal automatically.

## Motions inside visual modes

Every cursor motion (`hjkl`, `wbe`, `0$`, `gg`/`G`, `]]`/`[[`, `%`,
counts) extends the selection. `<C-v>5j` selects 5 rows down.

## Visual-block specifics

`<C-v>` is where the real power lives. Beyond the operators above:

| Keys | Action |
|------|--------|
| `I{text}<Esc>` | Insert `{text}` at the **left edge** of every selected row |
| `A{text}<Esc>` | Append `{text}` at the **right edge** of every row |

The flow:

1. `<C-v>` — anchor at the cursor.
2. Extend with motions: `jj` for 3 rows, `5l` for 6 cols wide, etc.
3. Press `I` (or `A`).
4. Cursor jumps to the top-left (or right edge + 1) of the rectangle.
5. Insert mode opens; type whatever you want.
6. `<Esc>` — typed text is **replayed across every other selected row**.

### Padding rules

- **`I`** (insert at left): if a row is shorter than the left column of
  the rectangle, the row is skipped (no spaces added).
- **`A`** (append at right): if a row is shorter than the right edge,
  it's padded with spaces to reach the target column before the typed
  text is inserted.

This matches vim's classic block-insert behaviour.

### Block undo is one step

The entire block operation — top-row typing PLUS the cross-row replay
— is wrapped in a **single undo transaction**. One `u` reverts the
whole rectangle. `<C-r>` re-applies it.

Same for visual-block `d` / `x`: the per-row deletes are coalesced.

### Block change

`c` from visual-block opens a transaction that spans:
1. The block delete (each row's slice).
2. The follow-up Insert session.

Press `<Esc>` to commit, `u` to revert everything in one step.

## Paste

`p` in Normal mode pastes whatever's in the unnamed register:

- **Linewise** register (filled by `yy`/`dd`/`Y`/etc.): paste inserts a
  new line BELOW the current one.
- **Charwise** register: paste inserts after the cursor on the current
  line.
- A block-yank/delete fills the register with newline-joined rows;
  pasting drops it back in as charwise text (no rectangular paste in
  v1).

## Selection rendering

The active selection paints with a reverse-video / blue background.
Selection wins over syntax highlighting on overlapping cells — useful
when picking out a selected token visually.

## Counts in visual modes

Counts are accumulated in visual modes the same as in normal mode. So
`<C-v>3j5l` selects a 4×6 rectangle (3+1 rows, 5+1 cols).

There's no operator-pending count concept in visual modes (the
operators fire immediately), so only the "before motion" count
applies.

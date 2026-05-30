# Fuzzy file finder (`:ff`)

Interactive file finder over the current working directory.

## Usage

```
:ff             open with empty query — lists everything
:ff foo         filter to paths matching "foo"
:ff src lex     multi-token AND: paths matching BOTH "src" and "lex"
:ff!            rebuild the index, then open empty
:ff! query      rebuild the index, then filter
```

The popup updates **live** as you type. The selected row is shown
inverted. Keys:

| Keys | Effect |
|------|--------|
| `<Down>` | Move selection down |
| `<Up>`   | Move selection up |
| `<Enter>` | Open the selected file (records a jumplist entry) |
| `<Esc>`  | Cancel — don't open anything |
| any printable / `<Backspace>` | Refine the query; selection auto-resets |

The current cursor position is pushed onto the
[jumplist](motions.md#jumplist) before the new file opens, so
`<C-o>` always brings you back to where you started the search.

## Scoring

Behind the scenes:

- Walks the working directory with the `ignore` crate, so
  `.gitignore`, `.ignore`, and hidden-file rules from `git config`
  all apply automatically.
- Skips files over 50 MB and caps the index at 100k entries.
- Each query is split on whitespace into AND-tokens. Every token has
  to match (fuzzy, via `SkimMatcherV2`); the scores are summed and a
  small penalty proportional to path length is subtracted so shorter
  paths float to the top.

## Indexing and `:ff!`

The index is built lazily on first `:ff` and reused for subsequent
invocations within the same session. Use `:ff!` to bust the cache
when you've added new files outside the editor and don't want to
restart.

The bang takes effect once — the moment the input transitions into
the banged form. Typing more characters after `:ff!` doesn't keep
rebuilding the index; it just refines the query against the freshly
rebuilt one.

## Why not call it `:fzf`?

`fzf` is a separate, well-known product. `:ff` is rtdvi's built-in
finder; it doesn't shell out and doesn't depend on `fzf` being
installed.

## Implementation

- [`src/fzf.rs`](../src/fzf.rs) — `Index::build` (the walker) and
  `search` (the scorer).
- [`src/mode/command.rs`](../src/mode/command.rs) — the cmdline
  integration: `parse_ff_query`, `ff_refresh_if_active`, `ff_nav`,
  `ff_accept_and_open`.
- The popup itself is rendered alongside the regular completion
  popup in `ui/cmdline.rs`.

# Syntax highlighting

jvim runs **two layers** of pattern matching against every visible
line per render:

1. A **built-in regex layer** with hand-tuned patterns per language
   (comments, strings, numbers, function calls, preprocessor
   directives).
2. A **vim `syn keyword` layer** parsed out of
   `/usr/share/vim/vim*/syntax/<filetype>.vim`.

Plus an overlay for the active `/` search pattern.

The result: opening a `.c` file with no extra setup gives keyword,
type, comment, string, number, function-call, and `#include`
highlighting using your system's existing vim files.

## Filetype detection

For every buffer, jvim picks a filetype in this order:

1. Per-buffer override from `:set syntax=…` / `:set ft=…`.
2. User glob from `[filetypes]` in config (`*.cpp = "c++"`, …).
3. Built-in basename/extension table:
   - **Names**: `Makefile`, `makefile`, `GNUmakefile`, `Dockerfile`,
     `Cargo.toml`, `Cargo.lock`, `.gitignore`, `.gitconfig`.
   - **Extensions**: `.rs`, `.py`, `.md` / `.markdown`, `.toml`,
     `.yaml` / `.yml`, `.json`, `.c`, `.h`, `.cpp` / `.cc` / `.cxx`
     / `.hpp` / `.hh` / `.hxx`, `.js` / `.mjs` / `.cjs`, `.ts`,
     `.sh` / `.bash`, `.vim`, `.html` / `.htm`, `.css`, `.go`,
     `.lua`, `.rb`, `.java`, `.tex`.
4. `mime_guess` extension fallback, mapped through a MIME → filetype
   table.
5. `"generic"` last resort.

Names are normalised: vim aliases like `c++` / `cxx` / `cppsrc` →
`cpp`. MIME types like `text/markdown` → `markdown`, `text/x-c++src`
→ `cpp`. Unknown values pass through lowercased so a `:set
syntax=foo` still loads `/usr/share/vim/vim*/syntax/foo.vim` if
present.

## Built-in regex rules

Per filetype, jvim has a small set of high-priority patterns. Selected
examples (see [`src/syntax.rs`](../src/syntax.rs) for the full list):

| Filetype     | Rules                                                 |
|--------------|-------------------------------------------------------|
| `c` / `cpp`  | `"…"` String, `'c'` Character, numbers, `name(`  Function (capture group), `#include` etc. PreProc, `//…` & `/* … */` Comment |
| `rust` / `go` / `java` / `js` / `ts` / `css` | strings, numbers, function calls, `//…` & `/*…*/` |
| `python` / `sh` / `ruby` / `toml` / `yaml` / `make` / `dockerfile` | strings (both quote styles), numbers, function calls, `#…` Comment |
| `markdown`   | `# heading…` Title, `` `code` `` String, `**bold**` Special |
| `lua`        | strings, numbers, function calls, `--…` Comment |
| `html`       | `<!--…-->` Comment, double-quoted strings |
| `vim`        | strings, numbers, `^"…$` Comment |
| `tex`        | `%…` Comment, `\foo` Keyword |
| `json`       | strings, numbers |
| `generic`    | strings (both quotes), numbers, `//…` and `#…` comments |

Function-call detection uses a capture group on `\b([A-Za-z_]\w*)\s*\(`
so only the identifier (not the trailing `(`) gets the Function color.

## Vim `syn keyword` layer

For any filetype where `/usr/share/vim/vim*/syntax/<filetype>.vim`
exists, jvim:

1. Reads the file once on first use (cached per buffer).
2. Extracts `syn keyword GROUP word1 word2 …` directives, ignoring
   options like `contained`, `nextgroup=…`, `skipwhite`.
3. Extracts `hi [def] link FROM TO` directives.
4. Compiles **one alternation regex per group** with word boundaries
   (`\bword1|word2|…\b`).
5. Resolves the final color via the link chain: e.g. `cKeyword` →
   `Keyword` (link in `c.vim`) → `Statement` (default `SynLink`) → a
   real `Statement` style in the colorscheme.

What jvim **doesn't** do:

- `syn region`, `syn match` — vim's regex flavour differs from PCRE
  and would need a translator.
- `contained` / `containedin` / `nextgroup` / `transparent` —
  contextual matching.
- Embedded `syntax include` directives (e.g. C inside Vim
  doc-comments).

This means some highlighting is missing compared to real vim:
multi-line strings, function definitions vs calls, preprocessor
arguments. The built-in regex layer fills in the most useful gaps
(`#include`, function calls).

## Priority

Within a single line, higher-priority rules **paint over** lower ones:

1. Vim keyword regexes — priority 0 (lowest).
2. Built-in regex rules — priority = position in the rule list, so
   rules listed later in `builtin_rules` win.
3. Active search match — overlay applied last by the renderer.

This is why `// fn foo` highlights as Comment (the `Comment` rule is
later in the list than keywords), even though `fn` is a Rust keyword.

## Manual override

```
:set syntax=c++       — force c++ syntax for this buffer
:set syntax=text/markdown — MIME type works too
:set syntax=off       — disable (back to auto-detection)
:set ft=python        — alias for `syntax`
```

The override is per-buffer; it doesn't affect other buffers and isn't
persisted.

## Performance

Compiling per-language regexes from a system syntax file is
**expensive** (the C `c.vim` has 100+ `syn keyword` lines). jvim
caches the compiled `Syntax` per buffer on the editor:

- First render of a buffer → builds the syntax engine.
- Every subsequent render → reuses the cached `Arc<Syntax>`.
- `:set syntax=…` and `:e <new file>` invalidate the relevant cache
  entry so the next render rebuilds.

Per-render cost on a 30-line viewport with cached syntax: well under
a millisecond.

## Adding rules

Built-in rules live in `builtin_rules(filetype)` in
[`src/syntax.rs`](../src/syntax.rs). Each rule is a
`(regex, group_name, Option<capture_index>)` tuple. Add a row, rebuild.

Vim keyword pickup is automatic — adding more `syn keyword` lines to
a system syntax file (or dropping one into `./syntax/<filetype>.vim`
or `~/.config/jvim/syntax/<filetype>.vim`) immediately affects jvim.

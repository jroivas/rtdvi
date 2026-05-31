# Syntax highlighting

rtdvi runs **two layers** of pattern matching against every visible
line per render:

1. A **built-in layer** with keyword lists and regex patterns for
   every supported language. Always active — no external files needed.
2. A **vim `syn keyword` layer** parsed out of
   `/usr/share/vim/vim*/syntax/<filetype>.vim`. Optional — extends
   the built-in layer with the long tail of language keywords.

Plus an overlay for the active `/` search pattern.

The result: opening a `.rs` or `.py` file with no extra setup gives
full keyword, type, comment, string, number, and function-call
highlighting from the built-in layer alone. Installing a vim runtime
package adds any keywords not yet in the built-in list.

## Without a vim installation

The built-in layer is self-contained — rtdvi works correctly with no
system vim present. To install just the syntax and colorscheme files
without the vim editor:

| Distro | Command |
|--------|---------|
| Debian / Ubuntu | `sudo apt-get install vim-runtime` |
| Fedora / RHEL | `sudo dnf install vim-filesystem vim-common` |
| Arch Linux | `sudo pacman -S vim-runtime` |
| openSUSE | `sudo zypper install vim-data` |
| Alpine | `sudo apk add vim` |
| macOS (Homebrew) | `brew install vim` |

After installing, rtdvi picks up the files automatically — no
config change needed.

## Filetype detection

For every buffer, rtdvi picks a filetype in this order:

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

## Built-in rules

Per filetype, rtdvi ships keyword lists and regex patterns. The keyword
lists cover the complete reserved-word vocabulary for each language;
the regex patterns handle the constructs that keywords can't express.

| Filetype | Keywords | Regex patterns |
|----------|----------|----------------|
| `c` | full C11 keyword set | strings, chars, numbers, function calls, `#…` PreProc, `//` & `/*…*/` comments |
| `cpp` | full C++23 keyword set (incl. `concept`, `co_await`, `consteval`, …) | same as c |
| `rust` | all reserved + future-reserved words | primitive types (`i32`, `u8`, `bool`, …) as Type, strings, numbers, function calls, `//` & `/*…*/` comments |
| `go` | full keyword set | builtin identifiers (`len`, `make`, `nil`, `true`, …), types, strings, numbers, function calls, `//` & `/*…*/` comments |
| `python` | full keyword set incl. `async`/`await`, `match`/`case` | strings (both quotes), numbers, function calls, `#…` comments |
| `javascript` | full keyword set incl. `async`/`await`, `import`/`export` | strings (both quotes), numbers, function calls, `//` & `/*…*/` comments |
| `typescript` | JS keywords + `interface`, `type`, `enum`, `readonly`, `declare`, … | same as javascript |
| `java` | full keyword set incl. `record`, `sealed`, `permits` | strings, chars, numbers, function calls, `//` & `/*…*/` comments |
| `sh` | shell control keywords (`if`/`then`/`fi`, `for`/`do`/`done`, …) | strings (both quotes), numbers, function calls, `#…` comments |
| `lua` | full keyword set | strings (both quotes), numbers, function calls, `--…` comments |
| `ruby` | full keyword set | strings (both quotes), numbers, function calls, `#…` comments |
| `toml` | `true`, `false`, `inf`, `nan` | strings (both quotes), numbers, `#…` comments |
| `yaml` | `true`, `false`, `yes`, `no`, `on`, `off`, `null` | strings (both quotes), numbers, `#…` comments |
| `json` | `true`, `false`, `null` | strings, numbers |
| `markdown` | — | `# heading…` Title, `` `code` `` String, `**bold**` Special |
| `vim` | common ex-commands as Statement | strings, numbers, `^"…$` comments |
| `html` | — | `<!--…-->` comments, double-quoted strings |
| `tex` | — | `%…` comments, `\command` Keyword |
| `make` / `dockerfile` / `gitconfig` | — | strings (both quotes), numbers, `#…` comments |
| `css` | — | strings (both quotes), numbers, `/*…*/` comments |
| `generic` | — | strings (both quotes), numbers, `//…` and `#…` comments |

Function-call detection uses a capture group on `\b([A-Za-z_]\w*)\s*\(`
so only the identifier (not the trailing `(`) gets the Function color.

## Vim `syn keyword` layer (optional enhancement)

When a vim runtime is installed, the built-in keyword lists are
supplemented with everything in the corresponding `.vim` syntax file.
For any filetype where `/usr/share/vim/vim*/syntax/<filetype>.vim`
exists, rtdvi:

1. Reads the file once on first use (cached per buffer).
2. Extracts `syn keyword GROUP word1 word2 …` directives, ignoring
   options like `contained`, `nextgroup=…`, `skipwhite`.
3. Extracts `hi [def] link FROM TO` directives.
4. Compiles **one alternation regex per group** with word boundaries
   (`\bword1|word2|…\b`).
5. Resolves the final color via the link chain: e.g. `cKeyword` →
   `Keyword` (link in `c.vim`) → `Statement` (default `SynLink`) → a
   real `Statement` style in the colorscheme.

What rtdvi **doesn't** do:

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
**expensive** (the C `c.vim` has 100+ `syn keyword` lines). rtdvi
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
or `~/.config/rtdvi/syntax/<filetype>.vim`) immediately affects rtdvi.

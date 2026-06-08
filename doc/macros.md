# Keyboard macros

Record a sequence of keystrokes once, then replay it as many times as
you like — vim's `q` / `@`, working exactly the same way.

## Recording

| Keys        | Action |
|-------------|--------|
| `q{reg}`    | Start recording into register `{reg}` (`a`–`z` or `0`–`9`) |
| `q`         | Stop recording (only in Normal mode) |

While recording, the active window's statusline shows `recording @{reg}`
so you always know you're capturing. Everything you type between the
opening `q{reg}` and the closing `q` is stored — including mode
switches, inserted text, and `<Esc>`.

```
qa          start recording into register a
0wcwfoo<Esc>  jump to first word, change it to "foo"
q           stop
```

## Playback

| Keys        | Action |
|-------------|--------|
| `@{reg}`    | Play the macro in register `{reg}` once |
| `@@`        | Replay the most recently played macro |
| `N@{reg}`   | Play it `N` times (e.g. `10@a`) |

```
@a          run macro a once
5@a         run it five more times
@@          run it once more
```

A count multiplies as you'd expect, so a macro that edits one line and
moves down can be applied to a whole block with a single `N@`.

## Macros *are* registers

A macro is nothing more than keystrokes stored as **text** in a named
register — the same `a`–`z` registers you yank and paste with (see
[registers.md](registers.md)). Two consequences fall straight out of
this, just like vim:

- **A recorded macro can be pasted.** Record into `a`, then `"ap`
  drops the literal keystrokes into your buffer — handy for editing a
  macro by hand.
- **Any yanked text can be executed as a macro.** Yank some text into
  `a` (`"ayy`), then `@a` feeds that text back as if you'd typed it.
  So putting `itst<Esc>` in a register and pressing `@a` types `tst`.

Special keys are stored as their control bytes — `<Esc>` as `0x1b`,
`<CR>` as `\r`, `<Tab>` as `\t`, and `Ctrl`-letters as their C0 control
code — so a round-tripped macro replays faithfully. Keys with no compact
text form (arrows, function keys) are dropped from the recording; use
the equivalent motions (`h`/`j`/`k`/`l`, `w`, `0`, `$`, …) for portable
macros.

## Interaction with the clipboard register

The system-clipboard register defaults to `q` as well, but the two
never collide because they're reached differently:

- **bare `q`** starts/stops macro *recording*;
- **`"q`** (quote-prefixed) selects the system-clipboard *register* for
  the next yank/delete/paste.

You can still record into register `q` itself (`qq … q`, replay with
`@q`); that in-memory register is independent of the OS clipboard. If
you'd rather free the letter up entirely, change
`options.system_clipboard_register` — see [registers.md](registers.md).

## Notes

- Recording and the closing `q` only act in **Normal mode**. A `q`
  typed while inserting text is just a literal `q`, so macros that
  insert the letter record correctly.
- A macro that invokes itself (`@a` stored inside register `a`) is
  stopped by a recursion-depth guard rather than looping forever.
- Register letters fold to lowercase, so `qA` records into `a`.
  (Vim's *append-to-register* uppercase form is not implemented.)

## Implementation

[`src/macros.rs`](../src/macros.rs) holds the whole feature: a single
`pre_dispatch` hook at the top of `mode::handle_key` owns the `q` / `@`
state machine and captures keystrokes into the active recording.
Storage shares `editor.named_registers` with yank/paste, which is what
makes the macro/register duality work.

# ved — the verbose ed

A drop-in compatible clone of [ed](https://www.gnu.org/software/ed/), the original Unix line editor, written in pure-stdlib Rust. ved adds friendly errors, command confirmations, long-form command aliases, and a built-in help reference, while preserving strict compatibility so any script written for real ed runs against ved unchanged.

The purpose is twofold. First, a more learner-friendly on-ramp to the ed/sed/grep/vim command lineage, since ed's terseness is a feature for veterans but a wall for newcomers. Second, a Rust learning project for the author. ved should remain small, dependency-free, and faithful to ed's spirit. ed itself is around 5,000 lines of C; ved should land somewhere comparable.

## Design decisions

| Decision | Choice |
|---|---|
| Language | Rust, edition 2024, pure stdlib (no dependencies, ever) |
| Interface | CLI, line-oriented. No TUI. |
| ed compatibility | Strict drop-in. Scripts written for real ed must work unchanged. |
| Long-form aliases | Long names allowed for commands without inline delimited arguments: `append`, `insert`, `change`, `delete`, `print`, `write`, `quit`, `read`, `edit`, `help`. Substitute, global, and inverse global stay short-only because their `/foo/bar/` argument syntax makes long names ambiguous. (This is "option C" from the design discussion.) |
| Help system | `H` (matching ed's help-mode toggle) and `help` (long form) both print a command reference |
| Errors | Full English replacing ed's bare `?`. Example: `? unknown command: foo` |
| Confirmations | Echo what each command did. Example: `deleted 3 lines (5-7)`, `wrote 142 bytes to config.txt` |
| Prompt flag | `-p` (matching ed) plus `--prompt` long form. Both `--prompt VALUE` and `--prompt=VALUE` styles accepted. |
| Line numbering | Use ed's existing `n` command. No extension needed. |
| Build output | Single static binary. No runtime, no shared libraries. |

## Build plan (slices)

Each slice is a complete working program with one more capability than the previous. Update the status column as work progresses.

| # | Slice | Status |
|---|---|---|
| 1 | Cargo project, REPL loop, `-p`/`--prompt` flag | **done** |
| 2 | Buffer + `a` (append) + `.` + `p` + `,p` | **done** |
| 3 | Addresses (`1p`, `$p`, `2,4p`) + current-line tracking + `n` (numbered print) | **done** |
| 4 | `w` (write) and `q` (quit), with double-q dance for unsaved changes | **done** |
| 5a | BRE regex engine (`src/bre.rs`, hand-written, pure stdlib) | **done** |
| 5b | Substitute: `s/old/new/[g]` wired up to the BRE engine | **done** |
| 6 | `d` (delete) and `i` (insert) | **done** |
| 7 | `g/pattern/cmd` and `v/pattern/cmd` (grep ancestor) | **done** |
| 8 | Long-form command aliases (`append`, `write`, `quit`, etc.) | **done** |
| 9 | Friendly errors + confirmations + `Q` (force quit) | **done** |
| 10 | `H`/`help` command reference | **done** |
| 11 | Open existing files, `e` (edit), `r` (read) | **done** |

## Build and run

```sh
cd nursery/ved
cargo build
cargo run -- -p '* '
```

The `--` separates cargo's flags from ved's flags so cargo doesn't try to interpret `-p` as one of its own. Type `q`, `quit`, or Ctrl-D to exit.

For an optimized release build:

```sh
cargo build --release
./target/release/ved -p '* '
```

## Architectural notes

Slice 1 was structured to make slice 2 a localized change. The `dispatch` function in `src/main.rs` was a four-line match and grew into a six-arm match in slice 2, plus a new `Buffer` module and two new `Action` variants (`EnterInputMode`, `Error`). The REPL loop now tracks an `input_mode` flag and owns the `Buffer`.

The `Buffer` lives in `src/buffer.rs` as its own module. Lines are 1-indexed externally to match ed's address syntax. The `current` field tracks ed's "current line" concept; 0 is the sentinel for an empty buffer (the case where `a` inserts before the first line).

Slice 3 introduced `src/address.rs` with `Address`, `Spec`, and `Range` types plus a parser and resolver. `dispatch` is now two-stage: peel any address spec off the front of the line, then look at the command letter and run the matching handler. Bare Enter is special-cased at the very top of dispatch as the equivalent of `+1p`. Slice 3 commands: `p`, `n`, address-only (`5<Enter>` jumps to line 5 and prints), `a`, `q`/`quit`.

Slice 3 also moved `dispatch` from `&Buffer` to `&mut Buffer`. The reason: `p` and `n` update ed's "current line" as a side effect, and threading that through the `Action` enum was uglier than just letting dispatch mutate. Slice 2's note that slice 6 would be first to need this was a guess that didn't survive contact with reality — fine.

Slice 4 added the modified flag, the remembered filename, and the double-q dance. The buffer gained `modified: bool` and `filename: Option<String>` plus the methods to manage them. `Spec::resolve_or_whole` was added so `w` defaults to the whole buffer; both `resolve` variants share a private `resolve_with(default)` helper.

Dispatch now has two stages: first try exact-match no-arg commands (`q`, `quit`, `a`, `p`, `n`), then fall through to commands-with-arguments (currently just `w`, but slices 5+ will add more here). The REPL holds a `quit_warned` flag and applies the unsaved-changes policy when dispatch returns `Action::Quit` — first quit on a modified buffer warns and sets the flag, second quit (or any quit on a clean buffer) actually exits. Any non-quit Action resets the flag, which is how an intervening command clears the warning.

`w`'s confirmation uses ved's friendly format: `wrote 142 bytes to foo.txt`. Real ed prints just the byte count. Slice 9 will add an `H`-style toggle so users can opt back to ed-compat output for scripting.

Errors flow through `Action::Error(String)`. The REPL prints them as `? <message>` (ved's friendly format). Slice 9 will systematize this — possibly adding an `H` toggle so users can suppress messages and behave like real ed — but the routing channel already exists.

No external crates are planned. This is deliberate. Manual argument parsing, manual command parsing, manual buffer management, all stdlib. The result should be a single fast-starting binary that compiles in under a second and has zero supply-chain footprint.

## Slice 5: the BRE engine

ed uses BRE (POSIX Basic Regular Expressions), and ved's strict drop-in compatibility goal means the substitute command needs a BRE engine. Pure-stdlib Rust has no regex support, so we're building one.

**Why hand-write it instead of pulling a crate.** The Rust ecosystem's main regex crate is RE2-style and doesn't support BRE syntax (escaped parens for groups, no `+`/`?` metachars, etc.) — even if we relaxed the no-deps rule, we'd still need a BRE→ERE translator. There IS prior art in `posix-regex` (a no_std POSIX regex parser from the Redox OS project, ~30k downloads), but the whole point of ved is learning Rust, and a regex engine is a famously beautiful CS exercise. We build our own.

**Where it lives.** `src/bre.rs` as a module of ved. Not a separate crate, not a path dependency, just a module — keeps ved's "single binary, zero dependencies" claim intact. If the engine turns out well and feels publishable, extracting it later from a single file to its own crate is mechanical.

**Slice 5a — engine.** Bottom-up implementation of the BRE subset that ed users actually use:

1. Literal character matching
2. `.` (any single character)
3. `*` (zero or more of the previous)
4. `^` and `$` anchors
5. Bracket expressions: `[abc]`, `[^abc]`, `[a-z]`, `[a-zA-Z0-9_]`
6. `\(...\)` capturing groups
7. `\1`-`\9` backreferences (in the pattern AND in the replacement string for `s`)

Skipped for now: POSIX character classes (`[[:alpha:]]`), `\{n,m\}` bounded repetition, `\<` `\>` word boundaries. Add later if we want them.

**Slice 5b — substitute command.** Once the engine works, wire it up:

- Parse `s` arguments: any-char delimiter, `s/old/new/`, `s/old/new/g`, address-prefix support (`1,$s/old/new/g`)
- Apply the engine to the addressed range, do the substitution(s), update the buffer
- Handle the ed conventions: `&` in replacement = whole match, `\1`-`\9` = backreferences, `\&` = literal ampersand

**Open question for after slice 5a works.** Is `bre` worth publishing as a standalone crate on crates.io? The name `bre` is taken (a Luau runtime, unrelated). Possible names: `posix-bre`, `ed-regex`, `bre-rs`, `pure-bre`. Decision deferred until the engine is real. If we extract it, the goal would be a small, well-documented crate aimed at people writing ed-style or sed-style tools who want pure-Rust BRE without pulling in the Redox no_std stack.

## Open questions

None at the moment. Slices 2 through 4 are execution work. New questions will accumulate here as they arise.

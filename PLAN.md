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
| 2 | Buffer + `a` (append) + `.` + `p` + `,p` | todo |
| 3 | Addresses (`1p`, `$p`, `2,4p`) + current-line tracking + `n` (numbered print) | todo |
| 4 | `w` (write) and `q` (quit), with double-q dance for unsaved changes | todo |
| 5 | Substitute: `s/old/new/[g]` | todo |
| 6 | `d` (delete) and `i` (insert) | todo |
| 7 | `g/pattern/cmd` and `v/pattern/cmd` (grep ancestor) | todo |
| 8 | Long-form command aliases (`append`, `write`, `quit`, etc.) | todo |
| 9 | Friendly errors + confirmations layer | todo |
| 10 | `H`/`help` command reference | todo |
| 11 | Open existing files (`ved filename` argument) | todo |

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

Slice 1 is structured to make slice 2 a localized change. The `dispatch` function in `src/main.rs` is currently a four-line match, and it's the only place that needs to grow into a real ed command parser. The surrounding REPL loop and the `Action` enum already provide the contract between the parser and the loop.

When adding the buffer in slice 2, expect to add new `Action` variants (`EnterInputMode`, `BufferModified`, `Error(String)`, etc.). Rust's exhaustive `match` checking will guide every call site that needs updating, which is the main reason the dispatch result is a discriminated union rather than, say, a tuple of strings.

No external crates are planned. This is deliberate. Manual argument parsing, manual command parsing, manual buffer management, all stdlib. The result should be a single fast-starting binary that compiles in under a second and has zero supply-chain footprint.

## Open questions

None at the moment. Slices 2 through 4 are execution work. New questions will accumulate here as they arise.

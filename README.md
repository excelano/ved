# ved — the verbose ed

A drop-in compatible clone of [ed](https://www.gnu.org/software/ed/), the original Unix line editor, written in pure-stdlib Rust. ved adds friendly error messages, command confirmations, long-form command aliases, and a built-in help reference while preserving strict compatibility so any script written for real ed runs against ved unchanged.

## Why

ed's one-character error messages and silent operations make it notoriously hard to learn. ved keeps ed's interface and behavior but tells you what happened: `deleted 3 lines (2-4)` instead of silence, `? no match` instead of `?`, and `help` prints a command reference without leaving the editor. If you already know ed, ved works exactly the same. If you're learning, ved explains what's going on.

## Build and install

ved requires only a Rust toolchain (1.85+ for edition 2024). No external crates, no C dependencies, no runtime.

```sh
cd ved
cargo build --release
```

The binary is at `target/release/ved`. To install system-wide:

```sh
sudo make install            # installs to /usr/local/bin
sudo make install PREFIX=/usr  # or specify a different prefix
```

To run without installing:

```sh
cargo run -- -p '* '
```

The `--` separates cargo's flags from ved's. `-p '* '` sets the command prompt (ed has no prompt by default).

## Quick start

```
$ ved myfile.txt           # open a file
135                        # ved prints the byte count
* ,p                      # print all lines
Hello world
This is a test file
* 1s/Hello/Goodbye/       # substitute on line 1
Goodbye world
* g/test/n                # print lines matching "test" with numbers
2	This is a test file
* w                        # write back to the file
37
* q                        # quit
```

## Commands

| Command | Long form | Description |
|---|---|---|
| `[.]a` | `append` | Append text after the addressed line (end with `.`) |
| `[.]i` | `insert` | Insert text before the addressed line (end with `.`) |
| `[.,.]d` | `delete` | Delete the addressed lines |
| `[.,.]p` | `print` | Print the addressed lines |
| `[.,.]n` | `number` | Print with line numbers |
| `[.,.]s/re/new/[g]` | | Substitute: replace regex matches in addressed lines |
| `[.,.]g/re/cmd` | | Global: run a command on every line matching a regex |
| `[.,.]v/re/cmd` | | Inverse global: run a command on lines NOT matching |
| `[.,.]w [file]` | | Write the addressed lines (default: all) to a file |
| `e [file]` | | Edit: replace the buffer with a file's contents |
| `r [file]` | | Read: append a file's contents after the addressed line |
| `q` | `quit` | Quit (warns on unsaved changes; repeat to force) |
| `Q` | | Quit unconditionally |
| `H` | `help` | Print the command reference |

## Addresses

Addresses specify which lines a command operates on. Most commands default to the current line (`.`).

| Address | Meaning |
|---|---|
| `5` | Line 5 |
| `.` | Current line |
| `$` | Last line |
| `+3` / `-1` | Relative to current line |
| `,` | All lines (shorthand for `1,$`) |
| `;` | Current line to end (shorthand for `.,$`) |
| `2,7` | Lines 2 through 7 |

## Regular expressions

ved implements POSIX Basic Regular Expressions (BRE) with a hand-written engine. No external regex library.

| Syntax | Meaning |
|---|---|
| `.` | Any single character |
| `*` | Zero or more of the preceding element |
| `^` / `$` | Start / end of line anchors |
| `[abc]` | Character class: matches a, b, or c |
| `[^abc]` | Negated class: matches anything except a, b, c |
| `[a-z]` | Range: matches any lowercase letter |
| `\(...\)` | Capturing group |
| `\1` ... `\9` | Backreference to captured group |

In replacement strings: `&` expands to the whole match, `\1`-`\9` expand to captured groups, `\&` is a literal ampersand.

## Implementation

2,006 lines of Rust across four modules, zero dependencies, 76 tests.

| Module | Lines | Purpose |
|---|---|---|
| `main.rs` | 753 | REPL, command dispatch, substitute/global/write/read |
| `bre.rs` | 929 | BRE regex engine: compiler, matcher, replacement expander |
| `address.rs` | 208 | Address parser and resolver |
| `buffer.rs` | 116 | Line buffer with current-line tracking |

The BRE engine started as a translation of Rob Pike's ~30-line recursive matcher from "The Practice of Programming," then grew bottom-up through a compile step (inspired by Ken Thompson's original ed), bracket expressions, capturing groups, and backreferences. The full history is in the git log.

## License

MIT. See [LICENSE](LICENSE).

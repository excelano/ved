# ved — the verbose ed

A verbose clone of [ed](https://www.gnu.org/software/ed/), the original Unix line editor, written in pure-stdlib Rust. ved adds friendly error messages, command confirmations, long-form command aliases, and a built-in help reference while preserving close compatibility so most scripts written for real ed run against ved unchanged.

**Full tutorial:** [https://excelano.com/ved/tutorial/](https://excelano.com/ved/tutorial/)

## Why

ed's one-character error messages and silent operations make it notoriously hard to learn. ved keeps ed's interface and behavior but tells you what happened: `deleted 3 lines (2-4)` instead of silence, `? no match` instead of `?`, and `help` prints a command reference without leaving the editor. If you already know ed, ved will be immediately familiar. If you're learning, ved explains what's going on.

## Install

### Debian and Ubuntu

Add the [Excelano apt repository](https://excelano.com/apt/) once (one-time setup):

```sh
curl -fsSL https://excelano.com/apt/setup.sh | sudo sh
```

Then install it, so `apt upgrade` keeps it current:

```sh
sudo apt install ved
```

Both amd64 and arm64 packages ship with every release.

### Windows

With [WinGet](https://learn.microsoft.com/windows/package-manager/), so `winget upgrade` keeps it current:

```powershell
winget install Excelano.ved
```

### Cargo

On macOS, or anywhere with a Rust toolchain, install the latest release from [crates.io](https://crates.io/crates/ved), and run the same command again to upgrade:

```sh
cargo install ved
```

There is no Homebrew formula and no macOS download. If you installed ved from the `excelano/tap` Homebrew tap, it will not be upgraded; remove it with `brew uninstall ved` and `brew untap excelano/tap`, then install with Cargo.

### Build from source

ved requires only a Rust toolchain at least as new as the `rust-version` in `Cargo.toml`. No external crates, no C dependencies, no runtime.

```sh
cd ved
cargo build --release
```

The binary is at `target/release/ved`. To install system-wide:

```sh
make build                        # build as your user (needs cargo)
sudo make install                 # copy binary to /usr/local/bin
sudo make install PREFIX=/usr     # or specify a different prefix
```

To run without installing:

```sh
cargo run -- -p '* '
```

The `--` separates cargo's flags from ved's. `-p '* '` sets the command prompt (ed has no prompt by default).

## Quick start

A session with the prompt set to `* `, on a two-line file:

```
$ ved -p '* ' myfile.txt
32
* ,p
Hello world
This is a test file
* 1s/Hello/Goodbye/
Goodbye world
* g/test/n
2	This is a test file
* w
wrote 34 bytes to myfile.txt
* q
```

ved prints the file's size when it opens it, confirms each change, and says how many bytes it wrote. With `-s`, it prints only what the commands ask for, as `ed -s` does.

## Commands

| Command | Long form | Description |
|---|---|---|
| `[.]a` | `append` | Append text after the addressed line (end with `.`) |
| `[.]i` | `insert` | Insert text before the addressed line (end with `.`) |
| `[.,.]c` | `change` | Replace the addressed lines with new text (end with `.`) |
| `[.,.]d` | `delete` | Delete the addressed lines |
| `[.,.]p` | `print` | Print the addressed lines |
| `[.,.]n` | `number` | Print with line numbers |
| `[.,.]l` | `list` | Print with non-printing bytes as `\NNN` octal, ending `$` |
| `[.,.+1]j` | `join` | Join the addressed lines into one (default: `.` and next) |
| `[.,.]m DEST` | | Move the addressed lines to after DEST (0 = top) |
| `[.,.]t DEST` | | Copy the addressed lines to after DEST (0 = top) |
| `[.,.]s/re/new/[g]` | | Substitute: replace regex matches in addressed lines |
| `[.,.]s` | | Repeat the last substitute (pattern, replacement, flags) |
| `[1,$]g/re/cmd` | | Global: run a command on every line matching a regex |
| `[1,$]v/re/cmd` | | Inverse global: run a command on lines NOT matching |
| `[1,$]w [file]` | `write` | Write the addressed lines to a file |
| `[1,$]wq [file]` | | Write, then quit |
| `e [file]` | `edit` | Edit: replace the buffer with a file's contents (warns on unsaved changes; repeat to force) |
| `E [file]` | | Edit, discarding unsaved changes |
| `[$]r [file]` | `read` | Read: append a file's contents after the addressed line |
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
| `/re/` | Next line matching `re`, searching forward (wraps at the end) |
| `?re?` | Next line matching `re`, searching backward (wraps at the start) |

A search address's closing delimiter can be omitted if the pattern runs to the end of the line, matching ed: `/re` alone (no command letter after it) searches and prints, the same as `5` alone jumps to line 5 and prints it. Compound offsets like `$-5` or `5-3` are not supported — write them as two commands (`$`, then `-5`) instead.

## Regular expressions

ved implements the core of POSIX Basic Regular Expressions (BRE) with its own engine. No external regex library.

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
| `\0NN` | Octal byte literal (3-digit, leading zero, range `\000`-`\077`) |

In replacement strings: `&` expands to the whole match, `\1`-`\9` expand to captured groups, `\&` is a literal ampersand, `\0NN` inserts the corresponding byte. The leading-zero requirement keeps `\1`-`\9` reserved for backreferences; `\037` is the ASCII unit separator, `\011` is tab, `\033` is escape.

## As your default editor

ed was the original Unix `$EDITOR`, and because ved follows ed's interface it
serves the same role: it takes the file as an argument, writes your changes back
when you `w`, and exits zero on `q`. That is exactly the contract `git`,
`crontab -e`, and `sudoedit` expect, so ved slots straight in:

```sh
export EDITOR=ved
```

`EDITOR` names the line-editor slot, which is where ved belongs — tools reach for
it as the fallback when no full-screen editor is set. As with ed, ved opens to a
silent prompt rather than printing the file, so `,p` is the first thing to type
when git hands you a commit-message template: it prints the buffer, you edit it
with the usual ed commands, then `w` and `q`. Quitting without writing leaves the
file untouched, and git declines the empty commit.

For the `VISUAL` slot — the interactive, cursor-driven editor — [nved](https://github.com/excelano/nved)
is the companion: a descendant of ved that edits the printed block in place
without taking over the screen or disturbing your scrollback.

## Implementation

Pure-stdlib Rust, one module per concern. Zero dependencies, no external crates, no C.

### Limitations

These limitations are inherited from ed and intentional, since changing them would make ved not-an-ed-clone:

- **Newline-only record model.** Lines are delimited by `\n`. A `\r` before the `\n` is the last character of the line, as in ed, so a file with CRLF endings is written back with them, and `l` shows the `\r` as `\015`. The ASCII information separators (RS, GS, FS, US) used in some text formats as record separators do *not* create addressable lines — a file using RS to separate records but no newlines loads as a single ved line. Use `l` to make embedded separators visible within a line.
- **UTF-8 only.** Files are read via `std::fs::read_to_string`, which rejects invalid UTF-8. ved is a text editor, not a binary editor. UTF-8 text with multi-byte characters works; arbitrary binary does not. If a file carries a UTF-16 BOM or looks like UTF-7 (the `+ACI-` escape from Scoutbook exports), ved prints a warning with the `iconv` command needed to convert it. A UTF-16 file does not load. A file with the UTF-7 marker is valid UTF-8 and loads and saves as it is, because ved cannot tell a UTF-7 export from UTF-8 text that happens to contain `+ACI-`.

## License

MIT. See [LICENSE](LICENSE).

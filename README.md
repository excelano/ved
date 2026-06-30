# ved — the verbose ed

A drop-in compatible clone of [ed](https://www.gnu.org/software/ed/), the original Unix line editor, written in pure-stdlib Rust. ved adds friendly error messages, command confirmations, long-form command aliases, and a built-in help reference while preserving strict compatibility so any script written for real ed runs against ved unchanged.

**Full tutorial:** [https://excelano.com/ved/tutorial/](https://excelano.com/ved/tutorial/)

## Why

ed's one-character error messages and silent operations make it notoriously hard to learn. ved keeps ed's interface and behavior but tells you what happened: `deleted 3 lines (2-4)` instead of silence, `? no match` instead of `?`, and `help` prints a command reference without leaving the editor. If you already know ed, ved works exactly the same. If you're learning, ved explains what's going on.

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

### Homebrew

On macOS or Linux, tap and trust the repository once — Homebrew gates third-party taps behind explicit trust (one-time setup):

```sh
brew tap excelano/tap
brew trust excelano/tap
```

Then install it, so `brew upgrade` keeps it current:

```sh
brew install ved
```

### Prebuilt binary (Linux and macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/excelano/ved/main/install.sh | sh
```

The installer downloads the right tarball for your platform from the GitHub release, verifies its checksum, and drops the binary into `~/.cargo/bin` (or the equivalent on Windows). If `ved` isn't found on your `PATH` after installation, ensure `~/.cargo/bin` is on it. Releases also ship raw tarballs (`ved-*.tar.xz` / `.zip`) for manual installation. To uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/excelano/ved/main/uninstall.sh | sh
```

That removes the binary from `~/.cargo/bin`; ved stores nothing else on disk. You can also just `rm ~/.cargo/bin/ved`.

### Windows

In PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/excelano/ved/releases/latest/download/ved-installer.ps1 | iex"
```

### Cargo

If you have a Rust toolchain, install the latest release from [crates.io](https://crates.io/crates/ved):

```sh
cargo install ved
```

### Build from source

ved requires only a Rust toolchain (1.85+ for edition 2024). No external crates, no C dependencies, no runtime.

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
| `\0NN` | Octal byte literal (3-digit, leading zero, range `\000`-`\077`) |

In replacement strings: `&` expands to the whole match, `\1`-`\9` expand to captured groups, `\&` is a literal ampersand, `\0NN` inserts the corresponding byte. The leading-zero requirement keeps `\1`-`\9` reserved for backreferences; `\037` is the ASCII unit separator, `\011` is tab, `\033` is escape.

## As your default editor

ed was the original Unix `$EDITOR`, and because ved is drop-in compatible it
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

For the full-screen `VISUAL` slot, [nved](https://github.com/excelano/nved) is the
companion — a cursor-driven descendant of ved that edits the printed block in
place.

## Implementation

2,684 lines of Rust across four modules, zero dependencies, 119 tests.

| Module | Lines | Purpose |
|---|---|---|
| `main.rs` | 1260 | REPL, command dispatch, substitute/global/write/read |
| `bre.rs` | 1057 | BRE regex engine: compiler, matcher, replacement expander |
| `address.rs` | 236 | Address parser and resolver |
| `buffer.rs` | 131 | Line buffer with current-line tracking |

The BRE engine started as a translation of Rob Pike's ~30-line recursive matcher from "The Practice of Programming," then grew bottom-up through a compile step (inspired by Ken Thompson's original ed), bracket expressions, capturing groups, and backreferences. The full history is in the git log.

### Limitations worth knowing

Two limitations are inherited from ed and intentional, since changing them would make ved not-an-ed-clone:

- **Newline-only record model.** Lines are delimited by `\n` (or `\r\n`). The ASCII information separators (RS, GS, FS, US) used in some text formats as record separators do *not* create addressable lines — a file using RS to separate records but no newlines loads as a single ved line. Use `l` to make embedded separators visible within a line.
- **UTF-8 only.** Files are read via `std::fs::read_to_string`, which rejects invalid UTF-8. ved is a text editor, not a binary editor. UTF-8 text with multi-byte characters works; arbitrary binary does not. If a file looks like UTF-7 (the `+ACI-` escape from Scoutbook exports) or carries a UTF-16 BOM, ved prints a warning at startup with the `iconv` command needed to convert it, and `w` refuses to overwrite the original (saving edits as UTF-8 would silently corrupt the source). `w <newname>` to a different file still works as an escape valve.

## License

MIT. See [LICENSE](LICENSE).

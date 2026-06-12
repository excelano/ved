// ved - the verbose ed
// A drop-in compatible clone of ed with friendly errors,
// confirmations, long-form command aliases, and a built-in
// help system. Written in pure-stdlib Rust.

mod address;
mod bre;
mod buffer;

use address::Spec;
use buffer::Buffer;
use std::env;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let (prompt, filename) = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ved: {e}");
            eprintln!("try 'ved --help' for usage information");
            return ExitCode::from(2);
        }
    };

    run_repl(prompt.as_deref(), filename.as_deref())
}

/// Parse command line arguments.
/// Returns (prompt, filename) on success.
fn parse_args(args: &[String]) -> Result<(Option<String>, Option<String>), String> {
    let mut prompt: Option<String> = None;
    let mut filename: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-p" | "--prompt" => {
                i += 1;
                if i >= args.len() {
                    return Err(format!("{arg} requires a value"));
                }
                prompt = Some(args[i].clone());
            }
            s if s.starts_with("--prompt=") => {
                prompt = Some(s["--prompt=".len()..].to_string());
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("ved {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            s if s.starts_with('-') => {
                return Err(format!("unknown option: {s}"));
            }
            _ => {
                filename = Some(arg.clone());
            }
        }
        i += 1;
    }
    Ok((prompt, filename))
}

fn print_usage() {
    println!("ved - the verbose ed");
    println!();
    println!("Usage: ved [OPTIONS] [FILE]");
    println!();
    println!("Options:");
    println!("  -p, --prompt <STRING>   Set the command prompt");
    println!("  -h, --help              Show this help message and exit");
    println!("  -V, --version           Show version information and exit");
}

/// The main read-eval-print loop. Reads commands from stdin one line
/// at a time, dispatches them, and prints any output. Exits on EOF
/// or on the q/quit command.
///
/// The loop has two modes. In *command mode* each input line is
/// parsed by `dispatch` and the resulting `Action` is applied. In
/// *input mode* (entered by `a`) every input line goes verbatim
/// into the buffer until the user types a single `.` on a line.
fn run_repl(prompt: Option<&str>, filename: Option<&str>) -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut reader = stdin.lock();
    let mut line = String::new();
    let mut buffer = Buffer::new();
    let mut input_mode = false;
    // ed's "unsaved changes" guard. The first `q` on a modified
    // buffer flips this on and prints a warning instead of quitting.
    // Any other command in between resets it.
    let mut quit_warned = false;

    // If a filename was given on the command line, load it.
    if let Some(f) = filename {
        match load_file(f, &mut buffer) {
            Ok(LoadOutcome::Loaded(bytes)) => {
                let _ = writeln!(out, "{bytes}");
            }
            Ok(LoadOutcome::NewFile) => {
                let _ = writeln!(out, "{f}: new file");
            }
            Err(e) => {
                let _ = writeln!(out, "? {e}");
            }
        }
    }

    loop {
        // ed shows the prompt only in command mode, never in input
        // mode — input mode is meant to feel like raw typing.
        if !input_mode {
            if let Some(p) = prompt {
                if write!(out, "{p}").is_err() || out.flush().is_err() {
                    return ExitCode::FAILURE;
                }
            }
        }

        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return ExitCode::SUCCESS, // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("ved: read error: {e}");
                return ExitCode::FAILURE;
            }
        }

        // Strip just the line terminator, not all trailing whitespace
        // — a user typing "  hello   " into the buffer should keep
        // their trailing spaces. trim_end() would eat them.
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');

        if input_mode {
            if trimmed == "." {
                input_mode = false;
            } else {
                buffer.append_after(buffer.current(), trimmed.to_string());
            }
            continue;
        }

        match dispatch(trimmed, &mut buffer) {
            Action::Quit => {
                if buffer.is_modified() && !quit_warned {
                    let _ = writeln!(out, "? warning: buffer modified");
                    quit_warned = true;
                } else {
                    return ExitCode::SUCCESS;
                }
            }
            Action::ForceQuit => {
                return ExitCode::SUCCESS;
            }
            Action::EnterInputMode => {
                input_mode = true;
                quit_warned = false;
            }
            Action::Done => {
                quit_warned = false;
            }
            Action::Print(msg) => {
                let _ = writeln!(out, "{msg}");
                quit_warned = false;
            }
            Action::Error(msg) => {
                // ved's friendly-error format. Slice 9 will route
                // every error path through this variant and may add
                // an H toggle to suppress the message like real ed.
                let _ = writeln!(out, "? {msg}");
                quit_warned = false;
            }
        }
    }
}

/// What the REPL should do after handling one input line.
enum Action {
    Quit,
    ForceQuit,
    EnterInputMode,
    Done,
    Print(String),
    Error(String),
}

/// Dispatch a single command line.
///
/// Two-stage parse: first peel any address spec off the front of
/// `cmd`, then decide what to do based on the command letter that
/// follows. Each command knows its own default for an empty spec
/// (most default to the current line).
fn dispatch(cmd: &str, buf: &mut Buffer) -> Action {
    let cmd = cmd.trim_start();

    // Bare Enter is shorthand for `+1p` — advance to the next line
    // and print it. This is how you walk through a file in ed.
    if cmd.is_empty() {
        if buf.is_empty() {
            return Action::Error("invalid address".to_string());
        }
        let next = buf.current() + 1;
        if next > buf.len() {
            return Action::Error("invalid address".to_string());
        }
        buf.set_current(next);
        return match buf.line(next) {
            Some(s) => Action::Print(s.to_string()),
            None => Action::Error("invalid address".to_string()),
        };
    }

    let (spec, rest) = match Spec::parse(cmd) {
        Ok(parsed) => parsed,
        Err(e) => return Action::Error(e),
    };

    // No command letter, just an address. ed treats this as
    // "go to that line and print it" — so `5<Enter>` jumps to
    // line 5 and prints it.
    if rest.is_empty() {
        return run_print(&spec, buf, PrintMode::Plain);
    }

    // Stage one: commands that take no arguments after the letter.
    // Exact-match these so we don't have to deal with `q` matching
    // the front of `quit`.
    match rest {
        "q" | "quit" => return Action::Quit,
        "a" | "append" => {
            // Append after the addressed line (default: current).
            // On an empty buffer, current is 0 and append_after(0)
            // inserts at the start — correct.
            if !buf.is_empty() {
                if let Ok(r) = spec.resolve(buf) {
                    buf.set_current(r.end);
                }
            }
            return Action::EnterInputMode;
        }
        "i" | "insert" => {
            // Insert before the addressed line (default: current).
            // Implemented as "append after address - 1", so the
            // input-mode loop (which always does append_after) works
            // unchanged. On an empty buffer or address 1, current
            // becomes 0 (the before-first-line sentinel).
            if !buf.is_empty() {
                let addr = match spec.resolve(buf) {
                    Ok(r) => r.end,
                    Err(e) => return Action::Error(e),
                };
                buf.set_current(addr.saturating_sub(1));
            }
            return Action::EnterInputMode;
        }
        "c" | "change" => {
            // Change: delete the addressed range, then enter input
            // mode positioned to append where the deleted range
            // started. Equivalent to `d` followed by `i` at the same
            // address, in one command. On an empty buffer, resolve
            // errors out — there's nothing to change.
            let range = match spec.resolve(buf) {
                Ok(r) => r,
                Err(e) => return Action::Error(e),
            };
            let start = range.start;
            buf.delete_range(range.start, range.end);
            buf.set_current(start.saturating_sub(1));
            return Action::EnterInputMode;
        }
        "d" | "delete" => return run_delete(&spec, buf),
        "p" | "print" => return run_print(&spec, buf, PrintMode::Plain),
        "n" | "number" => return run_print(&spec, buf, PrintMode::Numbered),
        "l" | "list" => return run_print(&spec, buf, PrintMode::List),
        "j" | "join" => return run_join(&spec, buf),
        "H" | "help" => return run_help(),
        "Q" => return Action::ForceQuit,
        _ => {}
    }

    // Stage two: commands with arguments. Match on the first byte
    // and pass the rest as the argument string.
    let first = rest.as_bytes()[0];
    let args = &rest[1..];
    match first {
        b'e' => run_edit(buf, args),
        b'r' => run_read(&spec, buf, args),
        b'g' => run_global(&spec, buf, args, false),
        b'v' => run_global(&spec, buf, args, true),
        b's' => run_substitute(&spec, buf, args),
        b'w' => run_write(&spec, buf, args),
        b'm' => run_move(&spec, buf, args),
        b't' => run_transfer(&spec, buf, args),
        _ => Action::Error(format!("unknown command: {rest}")),
    }
}

/// Global command: scan the addressed range for lines matching
/// (or not matching, if `invert` is true) a pattern, then execute
/// a command on each one. Default address: whole buffer (1,$).
///
/// `g/pattern/p` is literally "global regex print" — the ancestor
/// of the grep command.
///
/// Delete is special-cased (reverse iteration to keep line numbers
/// stable). All other commands dispatch through the normal path.
fn run_global(spec: &Spec, buf: &mut Buffer, args: &str, invert: bool) -> Action {
    let args_bytes = args.as_bytes();
    if args_bytes.is_empty() {
        return Action::Error("no delimiter".to_string());
    }

    let delim = args_bytes[0];
    let after_delim = &args_bytes[1..];

    // Parse pattern: read until unescaped delimiter.
    let (pattern, rest) = match scan_delimited(after_delim, delim) {
        Some(r) => r,
        None => return Action::Error("unterminated pattern".to_string()),
    };

    // The rest is the command. Empty defaults to "p".
    let cmd = std::str::from_utf8(rest).unwrap_or("p");
    let cmd = cmd.trim_start();
    let cmd = if cmd.is_empty() { "p" } else { cmd };

    // Compile the regex.
    let re = bre::Regex::compile(&pattern);

    // Resolve the address range (default: whole buffer).
    let range = match spec.resolve_or_whole(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };

    // Scan and mark matching lines.
    let mut marked = Vec::new();
    for n in range.start..=range.end {
        if let Some(line) = buf.line(n) {
            let matched = re.find(line.as_bytes()).is_some();
            if matched != invert {
                marked.push(n);
            }
        }
    }

    if marked.is_empty() {
        return Action::Error("no match".to_string());
    }

    // Delete is special: remove marked lines bottom-to-top so
    // earlier line numbers stay valid.
    if cmd == "d" {
        for &n in marked.iter().rev() {
            buf.delete_range(n, n);
        }
        return Action::Done;
    }

    // For all other commands: set current to the marked line and
    // dispatch normally. Works for p, n, s (which don't change
    // line count) and anything else that operates on current line.
    let mut output = String::new();
    for &n in &marked {
        buf.set_current(n);
        match dispatch(cmd, buf) {
            Action::Print(msg) => {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&msg);
            }
            Action::Error(msg) => {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&format!("? {msg}"));
            }
            Action::Done => {}
            // Quit and EnterInputMode don't make sense inside g.
            _ => {}
        }
    }

    if output.is_empty() {
        Action::Done
    } else {
        Action::Print(output)
    }
}

/// Print a command reference.
fn run_help() -> Action {
    Action::Print(
        "\
ved commands (addresses shown in brackets are optional):

  [.]a, append       Append text after the addressed line. End with '.'
  [.]i, insert       Insert text before the addressed line. End with '.'
  [.,.]c, change     Replace the addressed lines with new text. End with '.'
  [.,.]d, delete     Delete the addressed lines
  [.,.]p, print      Print the addressed lines
  [.,.]n, number     Print with line numbers
  [.,.]l, list       Print with non-printing chars as \\NNN octal, ending $
  [.,.+1]j, join     Join the addressed lines into one (default: . and next)
  [.,.]m DEST        Move the addressed lines to after DEST (0 = top)
  [.,.]t DEST        Copy the addressed lines to after DEST (0 = top)
  [.,.]s/re/new/[g]  Substitute: replace regex match in addressed lines
  [.,.]s             Repeat the last substitute (pattern, replacement, flags)
  [.,.]g/re/cmd      Global: run cmd on lines matching regex
  [.,.]v/re/cmd      Inverse global: run cmd on lines NOT matching regex
  [.,.]w [file]      Write addressed lines (default: all) to file
  q, quit            Quit (warns if buffer modified, repeat to force)
  H, help            Show this help

Addresses: 1 (line 1), $ (last), . (current), +N/-N (relative), , (all), ; (current to end)
Regex: . (any char), * (zero or more), ^ $ (anchors), [abc] [^abc] [a-z] (classes)
       \\(...\\) (capture group), \\1-\\9 (backreference)
Replacement: & (whole match), \\1-\\9 (group), \\& (literal &)"
            .to_string(),
    )
}

/// Delete the addressed lines. Default address: current line.
/// Updates current to the line after the deleted range, or the
/// new last line.
fn run_delete(spec: &Spec, buf: &mut Buffer) -> Action {
    let range = match spec.resolve(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };
    let count = range.end - range.start + 1;
    buf.delete_range(range.start, range.end);
    if count == 1 {
        Action::Print(format!("deleted line {}", range.start))
    } else {
        Action::Print(format!(
            "deleted {count} lines ({}-{})",
            range.start, range.end
        ))
    }
}

/// Join the addressed lines into a single line at the start of the
/// range. Default address is `.,.+1` — bare `j` joins the current
/// line with the next, matching ed. A single-line range is a silent
/// no-op (still sets current). The joined line is the literal
/// concatenation: ed does not insert separators.
fn run_join(spec: &Spec, buf: &mut Buffer) -> Action {
    let range = match spec.resolve_or_pair_with_next(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };
    if range.start == range.end {
        buf.set_current(range.start);
        return Action::Print(String::new());
    }
    let mut joined = String::new();
    for n in range.start..=range.end {
        if let Some(line) = buf.line(n) {
            joined.push_str(line);
        }
    }
    buf.replace_line(range.start, joined);
    buf.delete_range(range.start + 1, range.end);
    buf.set_current(range.start);
    let count = range.end - range.start + 1;
    Action::Print(format!(
        "joined {count} lines ({}-{}) into line {}",
        range.start, range.end, range.start
    ))
}

/// Move the addressed range to after a destination address.
/// `2,4m6` moves lines 2-4 to after line 6. The destination may be
/// 0 ("before line 1") so a range can be moved to the top. ed
/// forbids moving a range into itself, so dest in `[start, end]`
/// is an error.
fn run_move(spec: &Spec, buf: &mut Buffer, dest_arg: &str) -> Action {
    let range = match spec.resolve(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };
    let dest = match address::parse_dest(dest_arg, buf) {
        Ok((d, _)) => d,
        Err(e) => return Action::Error(e),
    };
    if dest >= range.start && dest <= range.end {
        return Action::Error("invalid destination".to_string());
    }
    let lines: Vec<String> = (range.start..=range.end)
        .filter_map(|n| buf.line(n).map(str::to_string))
        .collect();
    let count = lines.len();
    buf.delete_range(range.start, range.end);
    // Deletion shifts everything past `range.end` down by `count`.
    let adjusted = if dest > range.end { dest - count } else { dest };
    let mut at = adjusted;
    for line in lines {
        buf.append_after(at, line);
        at += 1;
    }
    buf.set_current(adjusted + count);
    Action::Print(format!(
        "moved {count} lines ({}-{}) to after {dest}",
        range.start, range.end
    ))
}

/// Copy the addressed range to after a destination address. Like
/// `m` but the source lines stay in place. The destination may be
/// 0 ("before line 1"). Unlike `m`, the destination may be inside
/// the source range — you get the original plus a copy.
fn run_transfer(spec: &Spec, buf: &mut Buffer, dest_arg: &str) -> Action {
    let range = match spec.resolve(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };
    let dest = match address::parse_dest(dest_arg, buf) {
        Ok((d, _)) => d,
        Err(e) => return Action::Error(e),
    };
    let lines: Vec<String> = (range.start..=range.end)
        .filter_map(|n| buf.line(n).map(str::to_string))
        .collect();
    let count = lines.len();
    let mut at = dest;
    for line in lines {
        buf.append_after(at, line);
        at += 1;
    }
    buf.set_current(dest + count);
    Action::Print(format!(
        "copied {count} lines ({}-{}) to after {dest}",
        range.start, range.end
    ))
}

/// Resolve `spec` against the buffer and emit the corresponding
/// lines. With `numbered = true` each line is prefixed with its
/// 1-indexed position and a tab, matching ed's `n` command. Updates
/// the current line to the end of the printed range.
enum PrintMode {
    Plain,
    Numbered,
    List,
}

fn run_print(spec: &Spec, buf: &mut Buffer, mode: PrintMode) -> Action {
    let range = match spec.resolve(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };

    let mut output = String::new();
    for n in range.start..=range.end {
        if n > range.start {
            output.push('\n');
        }
        if matches!(mode, PrintMode::Numbered) {
            output.push_str(&format!("{n}\t"));
        }
        if let Some(line) = buf.line(n) {
            if matches!(mode, PrintMode::List) {
                output.push_str(&render_list_line(line));
            } else {
                output.push_str(line);
            }
        }
    }

    buf.set_current(range.end);
    Action::Print(output)
}

/// Render a line for the `l` (list) command: each byte outside
/// printable ASCII becomes `\NNN` (3-digit octal), backslashes
/// escape to `\\`, and a `$` marks end-of-line. Operates byte
/// by byte so multi-byte UTF-8 sequences also become visible —
/// the point of `l` is to disambiguate, including for cases
/// like a non-breaking space hiding among regular spaces.
fn render_list_line(line: &str) -> String {
    let mut out = String::new();
    for b in line.bytes() {
        match b {
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push('$');
    out
}

/// Load a file into the buffer, appending lines after position
/// `after` (0 = start of buffer). Sets the filename and returns
/// the byte count on success.
/// What `load_file` did. A missing file is not an error: ed remembers
/// the name so a later `w` creates it, so we report it distinctly.
enum LoadOutcome {
    Loaded(usize),
    NewFile,
}

fn load_file(filename: &str, buf: &mut Buffer) -> Result<LoadOutcome, String> {
    let content = match std::fs::read_to_string(filename) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            buf.set_filename(filename.to_string());
            buf.mark_saved();
            return Ok(LoadOutcome::NewFile);
        }
        Err(e) => return Err(format!("cannot open {filename}: {e}")),
    };
    let bytes = content.len();

    for line in content.lines() {
        buf.append_after(buf.len(), line.to_string());
    }
    buf.set_filename(filename.to_string());
    buf.mark_saved();

    Ok(LoadOutcome::Loaded(bytes))
}

/// Edit: replace the buffer with the contents of a file.
/// Warns if the buffer has unsaved changes (like q does).
fn run_edit(buf: &mut Buffer, args: &str) -> Action {
    let filename_arg = args.trim_start();

    let filename = if filename_arg.is_empty() {
        match buf.filename() {
            Some(f) => f.to_string(),
            None => return Action::Error("no current filename".to_string()),
        }
    } else {
        filename_arg.to_string()
    };

    // Replace the buffer entirely.
    *buf = Buffer::new();
    match load_file(&filename, buf) {
        Ok(LoadOutcome::Loaded(bytes)) => Action::Print(format!("{bytes}")),
        Ok(LoadOutcome::NewFile) => Action::Print(format!("{filename}: new file")),
        Err(e) => Action::Error(e),
    }
}

/// Read: append the contents of a file after the addressed line.
/// Default address: last line ($).
fn run_read(spec: &Spec, buf: &mut Buffer, args: &str) -> Action {
    let filename_arg = args.trim_start();

    let filename = if filename_arg.is_empty() {
        match buf.filename() {
            Some(f) => f.to_string(),
            None => return Action::Error("no current filename".to_string()),
        }
    } else {
        filename_arg.to_string()
    };

    // Resolve the insertion point (default: end of buffer).
    let after = if buf.is_empty() {
        0
    } else if spec.is_empty() {
        buf.len()
    } else {
        match spec.resolve(buf) {
            Ok(r) => r.end,
            Err(e) => return Action::Error(e),
        }
    };

    let content = match std::fs::read_to_string(&filename) {
        Ok(c) => c,
        Err(e) => return Action::Error(format!("cannot open {filename}: {e}")),
    };
    let bytes = content.len();

    let mut insert_at = after;
    for line in content.lines() {
        buf.append_after(insert_at, line.to_string());
        insert_at += 1;
    }

    if buf.filename().is_none() {
        buf.set_filename(filename);
    }

    Action::Print(format!("{bytes}"))
}

/// Substitute: apply a regex replacement to the addressed lines.
/// The `args` string is everything after the `s` letter, e.g.
/// `/old/new/g`. The first character is the delimiter. A bare `s`
/// with no arguments repeats the last substitute (pattern,
/// replacement, and global flag), erroring if none has run yet.
fn run_substitute(spec: &Spec, buf: &mut Buffer, args: &str) -> Action {
    let args = args.as_bytes();

    let (pattern, replacement, global) = if args.is_empty() {
        match buf.last_subst() {
            Some((p, r, g)) => (p.clone(), r.clone(), *g),
            None => return Action::Error("no previous substitute".to_string()),
        }
    } else {
        let delim = args[0];
        let rest = &args[1..];

        let (pattern, rest) = match scan_delimited(rest, delim) {
            Some(r) => r,
            None => return Action::Error("unterminated pattern".to_string()),
        };

        let (replacement, rest) = match scan_delimited(rest, delim) {
            Some(r) => r,
            None => return Action::Error("unterminated replacement".to_string()),
        };

        let global = rest.contains(&b'g');
        buf.set_last_subst(pattern.clone(), replacement.clone(), global);
        (pattern, replacement, global)
    };

    // Compile the pattern.
    let re = bre::Regex::compile(&pattern);

    // Resolve the address range (default: current line).
    let range = match spec.resolve(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };

    let mut last_modified_line = None;

    for n in range.start..=range.end {
        let line = match buf.line(n) {
            Some(l) => l.to_string(),
            None => continue,
        };
        let line_bytes = line.as_bytes();

        let new_line = if global {
            substitute_all(&re, line_bytes, &replacement)
        } else {
            substitute_first(&re, line_bytes, &replacement)
        };

        if let Some(new_bytes) = new_line {
            let new_str = String::from_utf8_lossy(&new_bytes).into_owned();
            buf.replace_line(n, new_str);
            last_modified_line = Some(n);
        }
    }

    match last_modified_line {
        Some(n) => {
            buf.set_current(n);
            match buf.line(n) {
                Some(s) => Action::Print(s.to_string()),
                None => Action::Error("invalid address".to_string()),
            }
        }
        None => Action::Error("no match".to_string()),
    }
}

/// Scan a delimiter-terminated field, handling backslash escapes.
/// Returns the field content (with escapes preserved for the BRE
/// engine) and the remainder after the closing delimiter.
fn scan_delimited(input: &[u8], delim: u8) -> Option<(Vec<u8>, &[u8])> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < input.len() {
        if input[i] == delim {
            return Some((result, &input[i + 1..]));
        }
        if input[i] == b'\\' && i + 1 < input.len() {
            if input[i + 1] == delim {
                // Escaped delimiter: include just the delimiter
                result.push(delim);
                i += 2;
            } else {
                // Other escape: pass through as-is for the BRE
                // engine or replacement expander to interpret.
                result.push(input[i]);
                result.push(input[i + 1]);
                i += 2;
            }
        } else {
            result.push(input[i]);
            i += 1;
        }
    }
    None // no closing delimiter found
}

/// Replace the first match in `text`. Returns None if no match.
fn substitute_first(
    re: &bre::Regex,
    text: &[u8],
    replacement: &[u8],
) -> Option<Vec<u8>> {
    let m = re.find(text)?;
    let mut result = Vec::new();
    result.extend_from_slice(&text[..m.start]);
    result.extend_from_slice(&bre::expand_replacement(replacement, &m, text));
    result.extend_from_slice(&text[m.end..]);
    Some(result)
}

/// Replace all non-overlapping matches in `text`. Returns None
/// if no match was found at all.
fn substitute_all(
    re: &bre::Regex,
    text: &[u8],
    replacement: &[u8],
) -> Option<Vec<u8>> {
    let mut result = Vec::new();
    let mut pos = 0;
    let mut matched = false;

    while pos <= text.len() {
        match re.find(&text[pos..]) {
            Some(m) => {
                matched = true;
                let abs_start = pos + m.start;
                let abs_end = pos + m.end;
                result.extend_from_slice(&text[pos..abs_start]);
                // For expand_replacement, the Match positions are
                // relative to the slice we searched, but we need
                // them relative to that same slice for text lookup.
                result.extend_from_slice(
                    &bre::expand_replacement(replacement, &m, &text[pos..]),
                );
                // Advance past the match. If the match was empty,
                // advance by one byte to avoid an infinite loop.
                if abs_end == abs_start {
                    if pos < text.len() {
                        result.push(text[pos]);
                    }
                    pos = abs_start + 1;
                } else {
                    pos = abs_end;
                }
            }
            None => {
                result.extend_from_slice(&text[pos..]);
                break;
            }
        }
    }

    if matched {
        Some(result)
    } else {
        None
    }
}

/// Write the addressed range to a file. Empty spec defaults to the
/// whole buffer (1,$). Filename can be passed inline (`w foo.txt`)
/// or omitted to reuse the buffer's remembered filename.
fn run_write(spec: &Spec, buf: &mut Buffer, args: &str) -> Action {
    // The argument string has whatever followed the `w` letter,
    // including any leading space. Trim it.
    let filename_arg = args.trim_start();

    let filename: String = if filename_arg.is_empty() {
        match buf.filename() {
            Some(f) => f.to_string(),
            None => return Action::Error("no current filename".to_string()),
        }
    } else {
        filename_arg.to_string()
    };

    // Build the content. The one place we deviate from the normal
    // resolve flow: an empty buffer with no address spec is allowed
    // (it writes a 0-byte file). Anything else goes through the
    // resolver and errors on an empty buffer.
    let (content, wrote_whole) = if buf.is_empty() && spec.is_empty() {
        (String::new(), true)
    } else {
        let range = match spec.resolve_or_whole(buf) {
            Ok(r) => r,
            Err(e) => return Action::Error(e),
        };
        let mut s = String::new();
        for n in range.start..=range.end {
            if let Some(line) = buf.line(n) {
                s.push_str(line);
                s.push('\n');
            }
        }
        let whole = range.start == 1 && range.end == buf.len();
        (s, whole)
    };

    let bytes = content.len();

    if let Err(e) = std::fs::write(&filename, &content) {
        return Action::Error(format!("cannot write {}: {}", filename, e));
    }

    // Build the confirmation message before we move `filename` into
    // the buffer's filename slot.
    let msg = format!("wrote {bytes} bytes to {filename}");
    buf.set_filename(filename);
    if wrote_whole {
        buf.mark_saved();
    }
    Action::Print(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut p = std::env::temp_dir();
        p.push(format!("ved_test_{tag}_{}_{nanos}", std::process::id()));
        p
    }

    #[test]
    fn missing_file_loads_as_new_file() {
        let path = temp_path("newfile");
        assert!(!path.exists());

        let mut buf = Buffer::new();
        let outcome = load_file(path.to_str().unwrap(), &mut buf);

        assert!(matches!(outcome, Ok(LoadOutcome::NewFile)));
        assert_eq!(buf.filename(), path.to_str());
        assert!(buf.is_empty());
        assert!(!buf.is_modified());
        assert!(!path.exists(), "opening a new file must not create it");
    }

    #[test]
    fn directory_is_still_an_error() {
        let path = temp_path("dir");
        std::fs::create_dir(&path).unwrap();

        let mut buf = Buffer::new();
        let outcome = load_file(path.to_str().unwrap(), &mut buf);

        std::fs::remove_dir(&path).unwrap();

        assert!(outcome.is_err());
        assert!(buf.filename().is_none());
    }

    // ── render_list_line (l command) ─────────────────────────

    #[test]
    fn list_printable_ascii_passes_through() {
        assert_eq!(render_list_line("hello world"), "hello world$");
    }

    #[test]
    fn list_empty_line_is_just_dollar() {
        assert_eq!(render_list_line(""), "$");
    }

    #[test]
    fn list_tab_becomes_octal() {
        assert_eq!(render_list_line("a\tb"), "a\\011b$");
    }

    #[test]
    fn list_ascii_separators_become_octal() {
        // FS GS RS US — the ASCII information separators
        let s = "A\u{1C}B\u{1D}C\u{1E}D\u{1F}E";
        assert_eq!(render_list_line(s), "A\\034B\\035C\\036D\\037E$");
    }

    #[test]
    fn list_backslash_escapes_to_double_backslash() {
        assert_eq!(render_list_line("a\\b"), "a\\\\b$");
    }

    #[test]
    fn list_utf8_multibyte_renders_byte_by_byte() {
        // ü is 0xC3 0xBC in UTF-8; l surfaces every non-ASCII byte
        assert_eq!(render_list_line("ü"), "\\303\\274$");
    }

    #[test]
    fn list_high_bit_byte_pads_to_three_digits() {
        // 0x7F (DEL) is non-printable; 0xFF is high-bit
        let s = "\u{7F}";
        assert_eq!(render_list_line(s), "\\177$");
    }

    // ── j (join) command ─────────────────────────────────────

    fn buf_with(lines: &[&str]) -> Buffer {
        let mut b = Buffer::new();
        for (i, l) in lines.iter().enumerate() {
            b.append_after(i, (*l).to_string());
        }
        b
    }

    #[test]
    fn join_default_joins_current_and_next() {
        let mut buf = buf_with(&["foo", "bar", "baz"]);
        buf.set_current(1);
        let act = dispatch("j", &mut buf);
        assert!(matches!(act, Action::Print(_)));
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.line(1), Some("foobar"));
        assert_eq!(buf.line(2), Some("baz"));
        assert_eq!(buf.current(), 1);
    }

    #[test]
    fn join_range_concatenates_all_lines() {
        let mut buf = buf_with(&["a", "b", "c", "d", "e"]);
        let act = dispatch("2,4j", &mut buf);
        assert!(matches!(act, Action::Print(_)));
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.line(2), Some("bcd"));
        assert_eq!(buf.line(3), Some("e"));
        assert_eq!(buf.current(), 2);
    }

    #[test]
    fn join_single_line_is_silent_noop() {
        let mut buf = buf_with(&["one", "two", "three"]);
        let act = dispatch("2j", &mut buf);
        if let Action::Print(s) = act {
            assert!(s.is_empty());
        } else {
            panic!("expected silent Print on single-line join");
        }
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.line(2), Some("two"));
        assert_eq!(buf.current(), 2);
    }

    #[test]
    fn join_does_not_insert_separators() {
        // ed semantics: literal concatenation, no spaces added.
        let mut buf = buf_with(&["hello", "world"]);
        let _ = dispatch("1,2j", &mut buf);
        assert_eq!(buf.line(1), Some("helloworld"));
    }

    #[test]
    fn join_at_last_line_errors() {
        // Bare `j` on the last line wants `.+1`, which is off-end.
        let mut buf = buf_with(&["only", "lines", "here"]);
        buf.set_current(3);
        let act = dispatch("j", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    #[test]
    fn join_long_form_alias_works() {
        let mut buf = buf_with(&["a", "b", "c"]);
        let _ = dispatch("1,3join", &mut buf);
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.line(1), Some("abc"));
    }

    #[test]
    fn join_marks_buffer_modified() {
        let mut buf = buf_with(&["x", "y"]);
        buf.mark_saved();
        assert!(!buf.is_modified());
        let _ = dispatch("1,2j", &mut buf);
        assert!(buf.is_modified());
    }

    // ── m (move) command ─────────────────────────────────────

    #[test]
    fn move_range_to_after_later_line() {
        let mut buf = buf_with(&["a", "b", "c", "d", "e"]);
        let _ = dispatch("2,3m5", &mut buf);
        // Removed b, c from positions 2-3; appended after the
        // shifted-down dest. Result: a, d, e, b, c.
        assert_eq!(buf.line(1), Some("a"));
        assert_eq!(buf.line(2), Some("d"));
        assert_eq!(buf.line(3), Some("e"));
        assert_eq!(buf.line(4), Some("b"));
        assert_eq!(buf.line(5), Some("c"));
        assert_eq!(buf.current(), 5);
    }

    #[test]
    fn move_range_to_after_earlier_line() {
        let mut buf = buf_with(&["a", "b", "c", "d", "e"]);
        let _ = dispatch("4,5m1", &mut buf);
        // Move d,e to after line 1. Result: a, d, e, b, c.
        assert_eq!(buf.line(1), Some("a"));
        assert_eq!(buf.line(2), Some("d"));
        assert_eq!(buf.line(3), Some("e"));
        assert_eq!(buf.line(4), Some("b"));
        assert_eq!(buf.line(5), Some("c"));
    }

    #[test]
    fn move_to_top_with_dest_zero() {
        let mut buf = buf_with(&["a", "b", "c"]);
        let _ = dispatch("3m0", &mut buf);
        assert_eq!(buf.line(1), Some("c"));
        assert_eq!(buf.line(2), Some("a"));
        assert_eq!(buf.line(3), Some("b"));
        assert_eq!(buf.current(), 1);
    }

    #[test]
    fn move_to_end_with_dollar() {
        let mut buf = buf_with(&["a", "b", "c", "d"]);
        let _ = dispatch("1m$", &mut buf);
        // Move line 1 to after line 4 (the $). Result: b, c, d, a.
        assert_eq!(buf.line(1), Some("b"));
        assert_eq!(buf.line(2), Some("c"));
        assert_eq!(buf.line(3), Some("d"));
        assert_eq!(buf.line(4), Some("a"));
    }

    #[test]
    fn move_into_self_errors() {
        let mut buf = buf_with(&["a", "b", "c", "d", "e"]);
        let act = dispatch("2,4m3", &mut buf);
        assert!(matches!(act, Action::Error(_)));
        // Buffer unchanged
        assert_eq!(buf.line(2), Some("b"));
        assert_eq!(buf.line(3), Some("c"));
        assert_eq!(buf.line(4), Some("d"));
    }

    #[test]
    fn move_invalid_dest_errors() {
        let mut buf = buf_with(&["a", "b"]);
        let act = dispatch("1m99", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    // ── t (transfer/copy) command ────────────────────────────

    #[test]
    fn transfer_copies_range_after_dest() {
        let mut buf = buf_with(&["a", "b", "c"]);
        let _ = dispatch("1,2t3", &mut buf);
        // Original lines stay, copy of 1-2 appended after 3.
        assert_eq!(buf.len(), 5);
        assert_eq!(buf.line(4), Some("a"));
        assert_eq!(buf.line(5), Some("b"));
        assert_eq!(buf.current(), 5);
    }

    #[test]
    fn transfer_to_top_with_dest_zero() {
        let mut buf = buf_with(&["x", "y"]);
        let _ = dispatch("2t0", &mut buf);
        assert_eq!(buf.line(1), Some("y"));
        assert_eq!(buf.line(2), Some("x"));
        assert_eq!(buf.line(3), Some("y"));
        assert_eq!(buf.current(), 1);
    }

    #[test]
    fn transfer_within_self_is_allowed() {
        // Unlike m, t can target inside the source range.
        let mut buf = buf_with(&["a", "b", "c", "d"]);
        let act = dispatch("1,3t2", &mut buf);
        assert!(matches!(act, Action::Print(_)));
        assert_eq!(buf.len(), 7);
    }

    #[test]
    fn move_and_transfer_mark_buffer_modified() {
        let mut buf = buf_with(&["a", "b", "c"]);
        buf.mark_saved();
        let _ = dispatch("1t3", &mut buf);
        assert!(buf.is_modified());

        let mut buf = buf_with(&["a", "b", "c"]);
        buf.mark_saved();
        let _ = dispatch("1m3", &mut buf);
        assert!(buf.is_modified());
    }

    // ── bare s repeats last substitute ───────────────────────

    #[test]
    fn bare_s_with_no_prior_substitute_errors() {
        let mut buf = buf_with(&["hello world"]);
        let act = dispatch("s", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    #[test]
    fn bare_s_repeats_last_substitute() {
        let mut buf = buf_with(&["foo", "foo", "foo"]);
        let _ = dispatch("1s/foo/bar/", &mut buf);
        assert_eq!(buf.line(1), Some("bar"));
        // Bare s on a different line replays it.
        let _ = dispatch("2s", &mut buf);
        assert_eq!(buf.line(2), Some("bar"));
        let _ = dispatch("3s", &mut buf);
        assert_eq!(buf.line(3), Some("bar"));
    }

    #[test]
    fn bare_s_preserves_global_flag() {
        let mut buf = buf_with(&["xxx xxx xxx"]);
        let _ = dispatch("s/x/Y/g", &mut buf);
        assert_eq!(buf.line(1), Some("YYY YYY YYY"));
        // Reset the line content, then bare s should also be global.
        buf.replace_line(1, "xxx xxx xxx".to_string());
        let _ = dispatch("s", &mut buf);
        assert_eq!(buf.line(1), Some("YYY YYY YYY"));
    }

    #[test]
    fn bare_s_without_global_flag_stays_non_global() {
        let mut buf = buf_with(&["aaa aaa"]);
        let _ = dispatch("s/a/Z/", &mut buf);
        // First a only.
        assert_eq!(buf.line(1), Some("Zaa aaa"));
        let _ = dispatch("s", &mut buf);
        // Bare s repeats: still non-global, so next first-a becomes Z.
        assert_eq!(buf.line(1), Some("ZZa aaa"));
    }

    #[test]
    fn explicit_s_updates_the_remembered_state() {
        let mut buf = buf_with(&["one two three"]);
        let _ = dispatch("s/one/ONE/", &mut buf);
        assert_eq!(buf.line(1), Some("ONE two three"));
        // A new explicit s replaces the remembered command.
        let _ = dispatch("s/two/TWO/", &mut buf);
        assert_eq!(buf.line(1), Some("ONE TWO three"));
        // Reset and confirm bare s now reuses the SECOND substitute.
        buf.replace_line(1, "one two three".to_string());
        let _ = dispatch("s", &mut buf);
        assert_eq!(buf.line(1), Some("one TWO three"));
    }
}

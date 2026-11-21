// ved - the verbose ed
// A verbose clone of ed with friendly errors,
// confirmations, long-form command aliases, and a built-in
// help system. Written in pure-stdlib Rust.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

mod address;
mod bre;
mod buffer;
mod enc;

use address::Spec;
use buffer::{Buffer, Substitution};
use std::env;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let options = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ved: {e}");
            eprintln!("try 'ved --help' for usage information");
            return ExitCode::from(2);
        }
    };

    run_repl(&options)
}

struct Options {
    prompt: Option<String>,
    filename: Option<String>,
    silent: bool,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut prompt: Option<String> = None;
    let mut filename: Option<String> = None;
    let mut silent = false;
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
            "-s" | "--script" => silent = true,
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
    Ok(Options {
        prompt,
        filename,
        silent,
    })
}

fn print_usage() {
    println!("ved - the verbose ed");
    println!();
    println!("Usage: ved [OPTIONS] [FILE]");
    println!();
    println!("Options:");
    println!("  -p, --prompt <STRING>   Set the command prompt");
    println!(
        "  -s, --script            Print only what commands ask for: no byte counts or confirmations"
    );
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
fn run_repl(options: &Options) -> ExitCode {
    let prompt = options.prompt.as_deref();
    let filename = options.filename.as_deref();
    let silent = options.silent;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut reader = stdin.lock();
    let mut line = String::new();
    let mut buffer = Buffer::new();
    let mut input_mode = false;
    // ed's "unsaved changes" guard. The first `q` or `e` on a modified
    // buffer flips this on and prints a warning instead of acting. A
    // second `q` or `e` straight after it goes ahead; any other command
    // in between resets it.
    let mut modified_warned = false;
    let mut had_error = false;
    let exit = |had_error: bool| {
        if had_error {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    };

    // If a filename was given on the command line, load it.
    if let Some(f) = filename {
        match load_file(f, &mut buffer) {
            Ok(LoadOutcome::Loaded(bytes)) => {
                emit(&mut out, &Action::Confirm(format!("{bytes}")), silent);
            }
            Ok(LoadOutcome::NewFile) => {
                emit(&mut out, &Action::Confirm(format!("{f}: new file")), silent);
            }
            Err(e) => {
                let _ = writeln!(out, "? {e}");
                had_error = true;
            }
        }
    }

    loop {
        // ed shows the prompt only in command mode, never in input
        // mode — input mode is meant to feel like raw typing.
        if !input_mode
            && let Some(p) = prompt
            && (write!(out, "{p}").is_err() || out.flush().is_err())
        {
            return ExitCode::FAILURE;
        }

        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                input_mode = false;
                if buffer.is_modified() && !modified_warned {
                    let _ = writeln!(out, "? warning: buffer modified");
                    modified_warned = true;
                    had_error = true;
                    continue;
                }
                return exit(had_error);
            }
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

        let action = match dispatch(trimmed, &mut buffer) {
            Action::EditModified(args) if modified_warned => run_edit(&mut buffer, &args),
            other => other,
        };
        match action {
            Action::EditModified(_) => {
                let _ = writeln!(out, "? warning: buffer modified");
                modified_warned = true;
                had_error = true;
            }
            Action::Quit => {
                if buffer.is_modified() && !modified_warned {
                    let _ = writeln!(out, "? warning: buffer modified");
                    modified_warned = true;
                    had_error = true;
                } else {
                    return exit(had_error);
                }
            }
            Action::ForceQuit => {
                return exit(had_error);
            }
            Action::ConfirmAndQuit(msg) => {
                emit(&mut out, &Action::Confirm(msg), silent);
                return exit(had_error);
            }
            Action::EnterInputMode => {
                input_mode = true;
                modified_warned = false;
            }
            Action::Done => {
                modified_warned = false;
            }
            output => {
                had_error |= emit(&mut out, &output, silent);
                modified_warned = false;
            }
        }
    }
}

/// What the REPL should do after handling one input line.
enum Action {
    Quit,
    ForceQuit,
    ConfirmAndQuit(String),
    /// The results of the commands `g` ran, in order.
    Batch(Vec<Action>),
    /// `e` on a modified buffer, carrying its argument. The loop
    /// warns the first time and runs the edit the second.
    EditModified(String),
    EnterInputMode,
    Done,
    /// Output the user asked for, such as the lines `p` prints.
    Print(String),
    /// What ved says about a command it carried out, such as a byte
    /// count. `-s` suppresses these.
    Confirm(String),
    Error(String),
}

/// Write an output action and return whether it reported an error.
fn emit(out: &mut impl Write, action: &Action, silent: bool) -> bool {
    match action {
        Action::Print(msg) => {
            let _ = writeln!(out, "{msg}");
            false
        }
        Action::Confirm(msg) => {
            if !silent {
                let _ = writeln!(out, "{msg}");
            }
            false
        }
        Action::Error(msg) => {
            let _ = writeln!(out, "? {msg}");
            true
        }
        Action::Batch(actions) => actions
            .iter()
            .fold(false, |err, a| emit(out, a, silent) | err),
        _ => false,
    }
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
            match spec.resolve_target(buf) {
                Ok(n) => buf.set_current(n),
                Err(e) => return Action::Error(e),
            }
            return Action::EnterInputMode;
        }
        "i" | "insert" => {
            // Insert before the target is append after the line above
            // it, so the input-mode loop always appends. 0i and 1i
            // both insert at the top, as in ed.
            match spec.resolve_target(buf) {
                Ok(n) => buf.set_current(n.saturating_sub(1)),
                Err(e) => return Action::Error(e),
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

    // Stage two: commands with arguments. Take the command letter as
    // a character and pass the rest as the argument string. Splitting
    // with the iterator rather than a byte offset keeps a multi-byte
    // first character from slicing `rest` off a char boundary.
    let (first, args) = match split_command(rest) {
        Some(parts) => parts,
        None => return Action::Error(format!("unknown command: {rest}")),
    };
    match first {
        'e' if buf.is_modified() => Action::EditModified(args.to_string()),
        'e' | 'E' => run_edit(buf, args),
        'r' => run_read(&spec, buf, args),
        'g' => run_global(&spec, buf, args, false),
        'v' => run_global(&spec, buf, args, true),
        's' => run_substitute(&spec, buf, args),
        'w' => match args.strip_prefix('q') {
            Some(file) if file.is_empty() || file.starts_with(char::is_whitespace) => {
                match run_write(&spec, buf, file) {
                    Action::Confirm(msg) => Action::ConfirmAndQuit(msg),
                    other => other,
                }
            }
            _ => run_write(&spec, buf, args),
        },
        'm' => run_move(&spec, buf, args),
        't' => run_transfer(&spec, buf, args),
        _ => Action::Error(format!("unknown command: {rest}")),
    }
}

/// Split a stage-two command into its letter and its argument string.
///
/// `e`, `r` and `w` take a filename, so the letter must be followed by
/// whitespace or the end of the line: `wfoo` is an error, as in ed, and
/// cannot write to a file named `foo`. That rule is what makes the long
/// forms `edit`, `read` and `write` unambiguous. `wq` is the one
/// exception, handled in the `w` arm.
fn split_command(rest: &str) -> Option<(char, &str)> {
    for (long, letter) in [("edit", 'e'), ("read", 'r'), ("write", 'w')] {
        if let Some(args) = rest.strip_prefix(long)
            && (args.is_empty() || args.starts_with(char::is_whitespace))
        {
            return Some((letter, args));
        }
    }
    let mut chars = rest.chars();
    let first = chars.next()?;
    let args = chars.as_str();
    let takes_filename = matches!(first, 'e' | 'E' | 'r' | 'w');
    let is_wq = first == 'w'
        && args
            .strip_prefix('q')
            .is_some_and(|f| f.is_empty() || f.starts_with(char::is_whitespace));
    if takes_filename && !is_wq && !(args.is_empty() || args.starts_with(char::is_whitespace)) {
        return None;
    }
    Some((first, args))
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
    if !valid_delimiter(delim) {
        return Action::Error("invalid delimiter".to_string());
    }
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
    let re = bre::Regex::compile(pattern);

    // Resolve the address range (default: whole buffer).
    let range = match spec.resolve_or_whole(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };

    // Mark every matching line, then visit the marks. A mark belongs to
    // its line, so a command that inserts, deletes or moves lines
    // leaves the remaining marks on the lines they were set on.
    buf.clear_marks();
    let mut any = false;
    for n in range.start..=range.end {
        if let Some(line) = buf.line(n)
            && re.find(line.as_bytes()).is_some() != invert
        {
            buf.set_mark(n, true);
            any = true;
        }
    }
    if !any {
        return Action::Error("no match".to_string());
    }

    // A plain `d` deletes only the marked lines, so delete them bottom
    // to top, where each removal is cheap.
    if cmd == "d" {
        let marked: Vec<usize> = std::iter::from_fn(|| buf.take_mark()).collect();
        for &n in marked.iter().rev() {
            buf.delete_range(n, n);
        }
        if let Some(&last) = marked.last() {
            let after_last = last + 1 - marked.len();
            buf.set_current(after_last.min(buf.len()));
        }
        return Action::Done;
    }

    // Anything else runs once per marked line, with that line current.
    // The first error stops the loop, as in ed.
    let mut results = Vec::new();
    while let Some(n) = buf.take_mark() {
        buf.set_current(n);
        match dispatch(cmd, buf) {
            Action::Error(msg) => {
                buf.clear_marks();
                results.push(Action::Error(msg));
                break;
            }
            result @ (Action::Print(_) | Action::Confirm(_) | Action::Batch(_)) => {
                results.push(result)
            }
            _ => {}
        }
    }

    match results.len() {
        0 => Action::Done,
        1 => results.pop().unwrap(),
        _ => Action::Batch(results),
    }
}

/// Print a command reference.
fn run_help() -> Action {
    Action::Print(
        "\
ved commands (addresses shown in brackets are optional):

  [.]a, append          Append text after the addressed line. End with '.'
  [.]i, insert          Insert text before the addressed line. End with '.'
  [.,.]c, change        Replace the addressed lines with new text. End with '.'
  [.,.]d, delete        Delete the addressed lines
  [.,.]p, print         Print the addressed lines
  [.,.]n, number        Print with line numbers
  [.,.]l, list          Print with non-printing chars as \\NNN octal, ending $
  [.,.+1]j, join        Join the addressed lines into one (default: . and next)
  [.,.]m DEST           Move the addressed lines to after DEST (0 = top)
  [.,.]t DEST           Copy the addressed lines to after DEST (0 = top)
  [.,.]s/re/new/[g]     Substitute: replace regex match in addressed lines
  [.,.]s                Repeat the last substitute (pattern, replacement, flags)
  [1,$]g/re/cmd         Global: run cmd on lines matching regex
  [1,$]v/re/cmd         Inverse global: run cmd on lines NOT matching regex
  [1,$]w, write [file]  Write the addressed lines to file
  [1,$]wq [file]        Write, then quit
  [$]r, read [file]     Read file in after the addressed line
  e, edit [file]        Replace the buffer with file (warns if buffer modified)
  E [file]              Replace the buffer with file, discarding changes
  q, quit               Quit (warns if buffer modified, repeat to force)
  Q                     Quit, discarding changes
  H, help               Show this help

Addresses: 1 (line 1), $ (last), . (current), +N/-N (relative), , (all), ; (current to end),
           /re/ (next matching line), ?re? (previous matching line)
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
        Action::Confirm(format!("deleted line {}", range.start))
    } else {
        Action::Confirm(format!(
            "deleted {count} lines ({}-{})",
            range.start, range.end
        ))
    }
}

/// Join the addressed lines into a single line at the start of the
/// range. Default address is `.,.+1` — bare `j` joins the current
/// line with the next, matching ed. A single-line range is a silent
/// no-op that leaves the current line alone. The joined line is the literal
/// concatenation: ed does not insert separators.
fn run_join(spec: &Spec, buf: &mut Buffer) -> Action {
    let range = match spec.resolve_or_pair_with_next(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };
    if range.start == range.end {
        return Action::Done;
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
    Action::Confirm(format!(
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
    let count = range.end - range.start + 1;
    let first = buf.move_range(range.start, range.end, dest);
    buf.set_current(first + count - 1);
    Action::Confirm(format!(
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
    for (at, line) in (dest..).zip(lines) {
        buf.append_after(at, line);
    }
    buf.set_current(dest + count);
    Action::Confirm(format!(
        "copied {count} lines ({}-{}) to after {dest}",
        range.start, range.end
    ))
}

enum PrintMode {
    Plain,
    Numbered,
    List,
}

/// Print the addressed lines in `mode` and make the last one current.
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
        if let Some(line) = buf.line(n) {
            output.push_str(&format_line(n, line, &mode));
        }
    }

    buf.set_current(range.end);
    Action::Print(output)
}

fn format_line(n: usize, line: &str, mode: &PrintMode) -> String {
    match mode {
        PrintMode::Plain => line.to_string(),
        PrintMode::Numbered => format!("{n}\t{line}"),
        PrintMode::List => render_list_line(line),
    }
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
    enc::warn_if_non_utf8(filename);
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

    for line in split_lines(&content) {
        buf.append_after(buf.len(), line.to_string());
    }
    buf.set_filename(filename.to_string());
    buf.mark_saved();

    Ok(LoadOutcome::Loaded(bytes))
}

/// Split file contents into lines on `\n` alone. A `\r` before the
/// `\n` stays in the line, as it does in ed, so a file with CRLF endings
/// is written back with them.
fn split_lines(content: &str) -> std::str::SplitTerminator<'_, char> {
    content.split_terminator('\n')
}

/// Edit: replace the buffer with the contents of a file. The file is
/// loaded into a new buffer, so a failed load leaves the old one as it
/// was. The unsaved-changes check is the caller's: `e` goes through the
/// loop's warning, `E` does not.
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

    let mut fresh = Buffer::new();
    let action = match load_file(&filename, &mut fresh) {
        Ok(LoadOutcome::Loaded(bytes)) => Action::Confirm(format!("{bytes}")),
        Ok(LoadOutcome::NewFile) => Action::Confirm(format!("{filename}: new file")),
        Err(e) => return Action::Error(e),
    };
    *buf = fresh;
    action
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

    enc::warn_if_non_utf8(&filename);
    let content = match std::fs::read_to_string(&filename) {
        Ok(c) => c,
        Err(e) => return Action::Error(format!("cannot open {filename}: {e}")),
    };
    let bytes = content.len();

    for (insert_at, line) in (after..).zip(split_lines(&content)) {
        buf.append_after(insert_at, line.to_string());
    }

    if buf.filename().is_none() {
        buf.set_filename(filename);
    }

    Action::Confirm(format!("{bytes}"))
}

/// Substitute: apply a regex replacement to the addressed lines.
/// The `args` string is everything after the `s` letter, e.g.
/// `/old/new/g`. The first character is the delimiter. A bare `s`
/// with no arguments repeats the last substitute (pattern,
/// replacement, and global flag), erroring if none has run yet.
fn run_substitute(spec: &Spec, buf: &mut Buffer, args: &str) -> Action {
    let args = args.as_bytes();
    let range = match spec.resolve(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };

    let (subst, mode) = if args.is_empty() {
        match buf.last_subst() {
            Some(last) => (last.clone(), None),
            None => return Action::Error("no previous substitute".to_string()),
        }
    } else {
        let delim = args[0];
        if !valid_delimiter(delim) {
            return Action::Error("invalid delimiter".to_string());
        }
        let rest = &args[1..];

        let (pattern, rest) = match scan_delimited(rest, delim) {
            Some(r) => r,
            None => return Action::Error("unterminated pattern".to_string()),
        };

        let (replacement, rest) = scan_delimited(rest, delim).unwrap_or((rest, &[]));

        let replacement = if replacement == b"%" {
            match buf.last_subst() {
                Some(last) => last.replacement.clone(),
                None => return Action::Error("no previous substitute".to_string()),
            }
        } else {
            replacement.to_vec()
        };
        let (nth, global, mode) = match parse_suffix(rest) {
            Ok(s) => s,
            Err(e) => return Action::Error(e),
        };
        let subst = Substitution {
            pattern: pattern.to_vec(),
            replacement,
            nth,
            global,
        };
        buf.set_last_subst(subst.clone());
        (subst, mode)
    };

    let re = bre::Regex::compile(&subst.pattern);

    let mut last_modified_line = None;

    for n in range.start..=range.end {
        let line = match buf.line(n) {
            Some(l) => l.to_string(),
            None => continue,
        };
        let line_bytes = line.as_bytes();

        let new_line = substitute(&re, line_bytes, &subst.replacement, subst.nth, subst.global);

        if let Some(new_bytes) = new_line {
            // Always valid UTF-8: `line_bytes` and `replacement` are
            // both valid UTF-8 (they come from `str`), the BRE
            // engine's `.` and bracket atoms consume whole
            // characters, and every match/group/octal-escape byte
            // range therefore lands on a character boundary.
            let new_str = String::from_utf8(new_bytes).expect("substitution result is valid UTF-8");
            buf.replace_line(n, new_str);
            last_modified_line = Some(n);
        }
    }

    match last_modified_line {
        Some(n) => {
            buf.set_current(n);
            match buf.line(n) {
                Some(line) => match mode {
                    Some(mode) => Action::Print(format_line(n, line, &mode)),
                    None => Action::Confirm(line.to_string()),
                },
                None => Action::Error("invalid address".to_string()),
            }
        }
        None => Action::Error("no match".to_string()),
    }
}

/// POSIX allows any character but space and newline. The fields are
/// scanned byte by byte, so ved also requires it to be one byte.
fn valid_delimiter(delim: u8) -> bool {
    delim.is_ascii() && !matches!(delim, b' ' | b'\n')
}

/// Split `input` at the first unescaped `delim`, returning the field
/// before it and the rest after it, or None if there is no closing
/// delimiter. Escapes are skipped, not interpreted: `\.` stays `\.`,
/// so the pattern compiler and the replacement expander, which know
/// what an escape means, see what the user typed. An escaped delimiter
/// reaches them as backslash-delimiter, which both read as the literal
/// character.
fn scan_delimited(input: &[u8], delim: u8) -> Option<(&[u8], &[u8])> {
    let mut i = 0;
    while i < input.len() {
        if input[i] == delim {
            return Some((&input[..i], &input[i + 1..]));
        }
        if input[i] == b'\\' && i + 1 < input.len() {
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

/// Parse what follows the replacement in `s/re/new/`: a count, `g`,
/// and at most one of `p`, `l` and `n`, each at most once. A count and
/// `g` together are an error, as in GNU ed; POSIX leaves them
/// unspecified.
fn parse_suffix(rest: &[u8]) -> Result<(usize, bool, Option<PrintMode>), String> {
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    let nth = if digits == 0 {
        1
    } else {
        match std::str::from_utf8(&rest[..digits])
            .ok()
            .and_then(|d| d.parse().ok())
        {
            Some(n) if n > 0 => n,
            _ => return Err("invalid count".to_string()),
        }
    };
    let mut global = false;
    let mut mode = None;
    for &b in &rest[digits..] {
        match b {
            b'g' if !global && digits == 0 => global = true,
            b'p' | b'l' | b'n' if mode.is_none() => {
                mode = Some(match b {
                    b'l' => PrintMode::List,
                    b'n' => PrintMode::Numbered,
                    _ => PrintMode::Plain,
                })
            }
            _ => return Err(format!("unknown suffix: {}", String::from_utf8_lossy(rest))),
        }
    }
    Ok((nth, global, mode))
}

/// Replace matches of `re` in `text`: the `nth` match (counting from 1),
/// and with `global` every match after it too. Returns None if nothing
/// was replaced.
///
/// Searching resumes in the whole line rather than in the slice after
/// the last match, so `^` can match only at the start of the line. An
/// empty match at the position where the previous match ended is not a
/// new match, which is POSIX's rule: `s/a*/X/g` on `baaac` gives
/// `XbXcX`, with one X for `aaa` and none for the nothing after it.
fn substitute(
    re: &bre::Regex,
    text: &[u8],
    replacement: &[u8],
    nth: usize,
    global: bool,
) -> Option<Vec<u8>> {
    let mut result = Vec::new();
    let mut copied = 0;
    let mut search = 0;
    let mut prev_end = None;
    let mut seen = 0;
    let mut replaced = false;

    while search <= text.len() {
        let Some(m) = re.find_at(text, search) else {
            break;
        };
        let step = if m.start == m.end {
            bre::char_width(&text[m.start..]).max(1)
        } else {
            0
        };
        if m.start == m.end && prev_end == Some(m.start) {
            search = m.start + step;
            continue;
        }
        seen += 1;
        if seen >= nth {
            result.extend_from_slice(&text[copied..m.start]);
            result.extend_from_slice(&bre::expand_replacement(replacement, &m, text));
            copied = m.end;
            replaced = true;
            if !global {
                break;
            }
        }
        prev_end = Some(m.end);
        search = m.end + step;
    }

    if !replaced {
        return None;
    }
    result.extend_from_slice(&text[copied..]);
    Some(result)
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
        return Action::Error(format!("cannot write {filename}: {e}"));
    }

    // Build the confirmation message before we move `filename` into
    // the buffer's filename slot.
    let msg = format!("wrote {bytes} bytes to {filename}");
    buf.set_filename(filename);
    if wrote_whole {
        buf.mark_saved();
    }
    Action::Confirm(msg)
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

    // ── CLI argument parsing ───────────────────────────────────
    // -h/--help and -V/--version aren't covered here: both call
    // std::process::exit directly inside parse_args, which would
    // kill the test binary rather than return.

    #[test]
    fn parse_args_with_nothing_returns_no_prompt_or_filename() {
        let Options {
            prompt, filename, ..
        } = parse_args(&[]).unwrap();
        assert_eq!(prompt, None);
        assert_eq!(filename, None);
    }

    #[test]
    fn parse_args_prompt_flag_takes_the_next_arg() {
        let args = ["-p".to_string(), "custom> ".to_string()];
        let Options {
            prompt, filename, ..
        } = parse_args(&args).unwrap();
        assert_eq!(prompt, Some("custom> ".to_string()));
        assert_eq!(filename, None);
    }

    #[test]
    fn parse_args_long_prompt_flag_is_the_same_as_short() {
        let args = ["--prompt".to_string(), "custom> ".to_string()];
        let Options { prompt, .. } = parse_args(&args).unwrap();
        assert_eq!(prompt, Some("custom> ".to_string()));
    }

    #[test]
    fn parse_args_prompt_equals_form() {
        let args = ["--prompt=custom> ".to_string()];
        let Options { prompt, .. } = parse_args(&args).unwrap();
        assert_eq!(prompt, Some("custom> ".to_string()));
    }

    #[test]
    fn parse_args_prompt_flag_with_no_value_errors() {
        let args = ["-p".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn parse_args_unknown_flag_errors() {
        let args = ["--nope".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn parse_args_bare_word_is_a_filename() {
        let args = ["notes.txt".to_string()];
        let Options {
            prompt, filename, ..
        } = parse_args(&args).unwrap();
        assert_eq!(prompt, None);
        assert_eq!(filename, Some("notes.txt".to_string()));
    }

    #[test]
    fn parse_args_prompt_and_filename_together() {
        let args = ["-p".to_string(), "> ".to_string(), "notes.txt".to_string()];
        let Options {
            prompt, filename, ..
        } = parse_args(&args).unwrap();
        assert_eq!(prompt, Some("> ".to_string()));
        assert_eq!(filename, Some("notes.txt".to_string()));
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

    // ── w (write) command ─────────────────────────────────────

    #[test]
    fn write_command_writes_buffer_and_remembers_filename() {
        let path = temp_path("write_explicit");
        let mut buf = buf_with(&["foo", "bar"]);
        let act = dispatch(&format!("w {}", path.to_str().unwrap()), &mut buf);
        assert!(matches!(act, Action::Confirm(ref s) if s.contains("wrote")));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "foo\nbar\n",
            "written content"
        );
        assert_eq!(buf.filename(), path.to_str());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn bare_write_reuses_remembered_filename() {
        let path = temp_path("write_bare");
        let mut buf = buf_with(&["only"]);
        let _ = dispatch(&format!("w {}", path.to_str().unwrap()), &mut buf);
        buf.replace_line(1, "changed".to_string());
        let act = dispatch("w", &mut buf);
        assert!(matches!(act, Action::Confirm(_)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "changed\n");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn wq_writes_the_remembered_file_and_quits() {
        let path = temp_path("wq_remembered");
        let mut buf = buf_with(&["foo"]);
        buf.set_filename(path.to_str().unwrap().to_string());
        let act = dispatch("wq", &mut buf);
        assert!(matches!(act, Action::ConfirmAndQuit(ref s) if s.contains("wrote")));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo\n");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn wq_with_a_filename_writes_that_file_and_quits() {
        let path = temp_path("wq_named");
        let mut buf = buf_with(&["foo"]);
        let act = dispatch(&format!("wq {}", path.to_str().unwrap()), &mut buf);
        assert!(matches!(act, Action::ConfirmAndQuit(_)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo\n");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn wq_that_cannot_write_does_not_quit() {
        let mut buf = buf_with(&["foo"]);
        let act = dispatch("wq", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    #[test]
    fn write_letter_glued_to_a_word_is_an_error_not_a_filename() {
        let mut buf = buf_with(&["foo"]);
        for cmd in ["wfoo", "efoo", "rfoo", "writer", "wqx"] {
            let act = dispatch(cmd, &mut buf);
            assert!(matches!(act, Action::Error(_)), "{cmd} should be an error");
        }
        assert_eq!(buf.len(), 1);
    }

    #[test]
    fn long_form_write_edit_and_read_name_the_file_that_follows() {
        let path = temp_path("long_forms");
        let p = path.to_str().unwrap();
        let mut buf = buf_with(&["foo"]);
        assert!(matches!(
            dispatch(&format!("write {p}"), &mut buf),
            Action::Confirm(_)
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo\n");
        assert!(matches!(
            dispatch(&format!("read {p}"), &mut buf),
            Action::Confirm(_)
        ));
        assert_eq!(buf.len(), 2);
        assert!(matches!(
            dispatch(&format!("write {p}"), &mut buf),
            Action::Confirm(_)
        ));
        assert!(matches!(
            dispatch(&format!("edit {p}"), &mut buf),
            Action::Confirm(_)
        ));
        assert_eq!(buf.len(), 2);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn write_with_no_filename_and_none_remembered_errors() {
        let mut buf = buf_with(&["x"]);
        let act = dispatch("w", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    #[test]
    fn write_of_a_partial_range_does_not_mark_the_buffer_saved() {
        // A fresh buffer starts modified (append_after sets it).
        // Writing only line 1 leaves the rest of the buffer
        // unsaved, so that flag must survive the write — only a
        // whole-buffer write is allowed to clear it.
        let path = temp_path("write_partial");
        let mut buf = buf_with(&["a", "b", "c"]);
        assert!(buf.is_modified());
        let _ = dispatch(&format!("1w {}", path.to_str().unwrap()), &mut buf);
        assert!(buf.is_modified());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn write_of_the_whole_buffer_marks_it_saved() {
        let path = temp_path("write_whole");
        let mut buf = buf_with(&["a", "b", "c"]);
        assert!(buf.is_modified());
        let _ = dispatch(&format!("w {}", path.to_str().unwrap()), &mut buf);
        assert!(!buf.is_modified());
        std::fs::remove_file(&path).unwrap();
    }

    // ── r (read) command ──────────────────────────────────────

    #[test]
    fn read_command_inserts_file_contents_after_the_address() {
        let path = temp_path("read_insert");
        std::fs::write(&path, "x\ny\n").unwrap();
        let mut buf = buf_with(&["foo", "bar"]);
        let act = dispatch(&format!("1r {}", path.to_str().unwrap()), &mut buf);
        assert!(matches!(act, Action::Confirm(_)));
        assert_eq!(buf.line(1), Some("foo"));
        assert_eq!(buf.line(2), Some("x"));
        assert_eq!(buf.line(3), Some("y"));
        assert_eq!(buf.line(4), Some("bar"));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn read_with_no_filename_and_none_remembered_errors() {
        let mut buf = buf_with(&["x"]);
        let act = dispatch("r", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    #[test]
    fn read_of_a_missing_file_errors() {
        let path = temp_path("read_missing");
        let mut buf = buf_with(&["x"]);
        let act = dispatch(&format!("r {}", path.to_str().unwrap()), &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    // ── e (edit) command ──────────────────────────────────────

    #[test]
    fn edit_command_replaces_the_buffer_with_file_contents() {
        let path = temp_path("edit_replace");
        std::fs::write(&path, "new1\nnew2\n").unwrap();
        let mut buf = buf_with(&["old1", "old2", "old3"]);
        buf.mark_saved();
        let act = dispatch(&format!("e {}", path.to_str().unwrap()), &mut buf);
        assert!(matches!(act, Action::Confirm(_)));
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.line(1), Some("new1"));
        assert_eq!(buf.line(2), Some("new2"));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn edit_with_no_filename_and_none_remembered_errors() {
        let mut buf = buf_with(&["x"]);
        buf.mark_saved();
        let act = dispatch("e", &mut buf);
        assert!(matches!(act, Action::Error(_)));
        // A failed edit must not touch the buffer.
        assert_eq!(buf.line(1), Some("x"));
    }

    #[test]
    fn edit_of_a_missing_file_wipes_the_buffer_as_a_new_file() {
        let path = temp_path("edit_missing");
        let mut buf = buf_with(&["old1"]);
        buf.mark_saved();
        let act = dispatch(&format!("e {}", path.to_str().unwrap()), &mut buf);
        assert!(matches!(act, Action::Confirm(ref s) if s.contains("new file")));
        assert!(buf.is_empty());
    }

    fn lines_of(buf: &Buffer) -> Vec<&str> {
        (1..=buf.len()).filter_map(|n| buf.line(n)).collect()
    }

    #[test]
    fn comma_on_an_empty_buffer_is_address_zero_for_a_and_i() {
        for cmd in [",a", ",i", ";a", "$a", ".a"] {
            let mut buf = Buffer::new();
            assert!(
                matches!(dispatch(cmd, &mut buf), Action::EnterInputMode),
                "{cmd}"
            );
            assert_eq!(buf.current(), 0, "{cmd}");
        }
        for cmd in ["1a", "1,$a", ",p"] {
            let mut buf = Buffer::new();
            assert!(matches!(dispatch(cmd, &mut buf), Action::Error(_)), "{cmd}");
        }
    }

    #[test]
    fn a_relative_address_can_reach_line_zero_where_zero_is_allowed() {
        let mut buf = buf_with(&["a1", "b2", "a3"]);
        buf.set_current(1);
        assert!(matches!(dispatch("+1m-1", &mut buf), Action::Confirm(_)));
        assert_eq!(lines_of(&buf), ["b2", "a1", "a3"]);
        let mut buf = buf_with(&["a1", "b2"]);
        buf.set_current(1);
        assert!(matches!(dispatch("-1a", &mut buf), Action::EnterInputMode));
        assert_eq!(buf.current(), 0);
        let mut buf = buf_with(&["a1", "b2"]);
        buf.set_current(1);
        assert!(matches!(dispatch("-1p", &mut buf), Action::Error(_)));
        assert!(matches!(dispatch("-2t0", &mut buf), Action::Error(_)));
    }

    #[test]
    fn global_delete_leaves_the_line_after_the_last_deletion_current() {
        let mut buf = buf_with(&["a1", "b2", "a3", "b4", "a5", "ab6"]);
        dispatch("g/a/d", &mut buf);
        assert_eq!(buf.line(buf.current()), Some("b4"));
        let mut buf = buf_with(&["a1", "b2", "a3", "b4"]);
        dispatch("g/a/d", &mut buf);
        assert_eq!(buf.line(buf.current()), Some("b4"));
        let mut buf = buf_with(&["a1", "a2"]);
        dispatch("g/a/d", &mut buf);
        assert_eq!(buf.current(), 0);
    }

    #[test]
    fn global_relative_delete_hits_the_lines_before_the_marked_ones() {
        let mut buf = buf_with(&["x", "a1", "y", "a2"]);
        dispatch("g/a/-1d", &mut buf);
        assert_eq!(lines_of(&buf), ["a1", "a2"]);
    }

    #[test]
    fn global_join_follows_its_marks_after_each_join() {
        let mut buf = buf_with(&["a1", "b1", "a2", "b2"]);
        dispatch("g/a/j", &mut buf);
        assert_eq!(lines_of(&buf), ["a1b1", "a2b2"]);
    }

    #[test]
    fn global_insert_above_a_mark_does_not_shift_it_onto_another_line() {
        let mut buf = buf_with(&["a1", "a2"]);
        dispatch("g/a/t0", &mut buf);
        assert_eq!(lines_of(&buf), ["a2", "a1", "a1", "a2"]);
    }

    #[test]
    fn global_stops_at_the_first_failing_command() {
        let mut buf = buf_with(&["a1", "b", "a2"]);
        let act = dispatch("g/a/-1d", &mut buf);
        assert!(matches!(act, Action::Error(_)));
        assert_eq!(lines_of(&buf), ["a1", "b", "a2"]);
        assert_eq!(buf.take_mark(), None, "marks are cleared after an error");
    }

    #[test]
    fn move_carries_a_marked_line_with_it() {
        let mut buf = buf_with(&["a", "b", "c"]);
        buf.set_mark(3, true);
        dispatch("3m0", &mut buf);
        assert_eq!(lines_of(&buf), ["c", "a", "b"]);
        assert_eq!(buf.take_mark(), Some(1));
    }

    #[test]
    fn global_reverses_a_file_with_move_to_zero() {
        let mut buf = buf_with(&["1", "2", "3", "4"]);
        dispatch("g/^/m0", &mut buf);
        assert_eq!(lines_of(&buf), ["4", "3", "2", "1"]);
    }

    #[test]
    fn append_and_insert_accept_the_addresses_ed_accepts() {
        // Each case: command, the line input mode appends after.
        for (cmd, after) in [
            ("0a", 0),
            ("$a", 3),
            ("2a", 2),
            ("1,2a", 2),
            ("3,1a", 1),
            ("a", 1),
        ] {
            let mut buf = buf_with(&["one", "two", "three"]);
            buf.set_current(1);
            assert!(
                matches!(dispatch(cmd, &mut buf), Action::EnterInputMode),
                "{cmd}"
            );
            assert_eq!(buf.current(), after, "{cmd}");
        }
        for (cmd, after) in [("0i", 0), ("1i", 0), ("3i", 2)] {
            let mut buf = buf_with(&["one", "two", "three"]);
            assert!(
                matches!(dispatch(cmd, &mut buf), Action::EnterInputMode),
                "{cmd}"
            );
            assert_eq!(buf.current(), after, "{cmd}");
        }
    }

    #[test]
    fn append_and_insert_reject_an_address_past_the_end() {
        for cmd in ["5a", "9,3a", "5i"] {
            let mut buf = buf_with(&["one", "two", "three"]);
            buf.set_current(1);
            assert!(matches!(dispatch(cmd, &mut buf), Action::Error(_)), "{cmd}");
            assert_eq!(buf.current(), 1, "{cmd} must not move the current line");
        }
        let mut empty = Buffer::new();
        assert!(matches!(dispatch("a", &mut empty), Action::EnterInputMode));
        assert!(matches!(dispatch("1a", &mut empty), Action::Error(_)));
    }

    #[test]
    fn an_escaped_delimiter_is_the_literal_character() {
        // Each case: line, command, result. Expected values are GNU
        // ed's output for the same input.
        for (line, cmd, want) in [
            ("a.b", "s.a\\..X.", "Xb"),
            ("x*y", "s*x\\*y*Z*", "Z"),
            ("abc", "s&a&\\&&", "&bc"),
            ("a/b", "s/\\//-/", "a-b"),
            ("abc", "s/b/\\n/", "anc"),
        ] {
            let mut buf = buf_with(&[line]);
            dispatch(cmd, &mut buf);
            assert_eq!(buf.line(1), Some(want), "{cmd} on {line}");
        }
        let mut buf = buf_with(&["axb"]);
        assert!(matches!(dispatch("s.a\\..X.", &mut buf), Action::Error(_)));
        assert_eq!(buf.line(1), Some("axb"));
    }

    #[test]
    fn a_utf8_file_with_the_utf7_marker_loads_and_saves_in_place() {
        let path = temp_path("utf7_marker");
        std::fs::write(&path, "see +ACI- here\n").unwrap();
        let mut buf = Buffer::new();
        load_file(path.to_str().unwrap(), &mut buf).unwrap();
        dispatch("s/see/saw/", &mut buf);
        assert!(matches!(dispatch("w", &mut buf), Action::Confirm(_)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "saw +ACI- here\n");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn crlf_line_endings_survive_a_load_and_a_write() {
        let path = temp_path("crlf");
        std::fs::write(&path, "one\r\ntwo\r\n").unwrap();
        let mut buf = Buffer::new();
        load_file(path.to_str().unwrap(), &mut buf).unwrap();
        assert_eq!(buf.line(1), Some("one\r"));
        assert!(matches!(dispatch("w", &mut buf), Action::Confirm(_)));
        assert_eq!(std::fs::read(&path).unwrap(), b"one\r\ntwo\r\n");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_missing_final_newline_still_ends_the_last_line() {
        assert_eq!(split_lines("a\nb").collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(split_lines("a\nb\n").collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(split_lines("").count(), 0);
    }

    #[test]
    fn edit_on_a_modified_buffer_asks_the_loop_to_warn() {
        let mut buf = buf_with(&["unsaved"]);
        let act = dispatch("e other.txt", &mut buf);
        assert!(matches!(act, Action::EditModified(ref a) if a.trim() == "other.txt"));
        assert_eq!(buf.line(1), Some("unsaved"));
    }

    #[test]
    fn capital_e_edits_a_modified_buffer_without_asking() {
        let path = temp_path("capital_e");
        std::fs::write(&path, "new\n").unwrap();
        let mut buf = buf_with(&["unsaved"]);
        let act = dispatch(&format!("E {}", path.to_str().unwrap()), &mut buf);
        assert!(matches!(act, Action::Confirm(_)));
        assert_eq!(buf.line(1), Some("new"));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn edit_that_fails_to_load_leaves_the_buffer_alone() {
        let dir = std::env::temp_dir();
        let mut buf = buf_with(&["unsaved"]);
        let act = dispatch(&format!("E {}", dir.to_str().unwrap()), &mut buf);
        assert!(matches!(act, Action::Error(_)));
        assert_eq!(buf.line(1), Some("unsaved"));
        assert!(buf.is_modified());
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
        assert!(matches!(act, Action::Confirm(_)));
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.line(1), Some("foobar"));
        assert_eq!(buf.line(2), Some("baz"));
        assert_eq!(buf.current(), 1);
    }

    #[test]
    fn join_range_concatenates_all_lines() {
        let mut buf = buf_with(&["a", "b", "c", "d", "e"]);
        let act = dispatch("2,4j", &mut buf);
        assert!(matches!(act, Action::Confirm(_)));
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.line(2), Some("bcd"));
        assert_eq!(buf.line(3), Some("e"));
        assert_eq!(buf.current(), 2);
    }

    #[test]
    fn join_single_line_is_silent_noop() {
        let mut buf = buf_with(&["one", "two", "three"]);
        let act = dispatch("2j", &mut buf);
        assert!(matches!(act, Action::Done));
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.line(2), Some("two"));
        assert_eq!(buf.current(), 3, "the current line is unchanged");
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

    // ── d (delete) command ────────────────────────────────────

    #[test]
    fn delete_single_line_reports_singular_message() {
        let mut buf = buf_with(&["a", "b", "c"]);
        let act = dispatch("2d", &mut buf);
        assert!(matches!(act, Action::Confirm(ref s) if s == "deleted line 2"));
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.line(1), Some("a"));
        assert_eq!(buf.line(2), Some("c"));
    }

    #[test]
    fn delete_range_reports_count_and_bounds() {
        let mut buf = buf_with(&["a", "b", "c", "d"]);
        let act = dispatch("2,3d", &mut buf);
        assert!(matches!(act, Action::Confirm(ref s) if s == "deleted 2 lines (2-3)"));
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn delete_long_form_alias_works() {
        let mut buf = buf_with(&["a", "b"]);
        let _ = dispatch("1delete", &mut buf);
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.line(1), Some("b"));
    }

    // ── g/v (global / inverse global) command ──────────────────

    #[test]
    fn global_runs_print_on_every_matching_line() {
        let mut buf = buf_with(&["foo", "bar", "foo2"]);
        let act = dispatch("g/foo/p", &mut buf);
        let Action::Batch(results) = act else {
            panic!("expected one result per matching line");
        };
        assert!(
            matches!(&results[..], [Action::Print(a), Action::Print(b)] if a == "foo" && b == "foo2")
        );
    }

    #[test]
    fn global_defaults_to_print_when_no_command_is_given() {
        let mut buf = buf_with(&["foo", "bar"]);
        let act = dispatch("g/foo/", &mut buf);
        assert!(matches!(act, Action::Print(ref s) if s == "foo"));
    }

    #[test]
    fn inverse_global_v_runs_on_lines_that_do_not_match() {
        let mut buf = buf_with(&["foo", "bar", "foo2"]);
        let act = dispatch("v/foo/p", &mut buf);
        assert!(matches!(act, Action::Print(ref s) if s == "bar"));
    }

    #[test]
    fn global_delete_removes_matching_lines() {
        let mut buf = buf_with(&["foo", "bar", "foo2", "baz"]);
        let act = dispatch("g/foo/d", &mut buf);
        assert!(matches!(act, Action::Done));
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.line(1), Some("bar"));
        assert_eq!(buf.line(2), Some("baz"));
    }

    #[test]
    fn global_with_no_matches_errors() {
        let mut buf = buf_with(&["x", "y"]);
        let act = dispatch("g/zzz/p", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    #[test]
    fn global_with_no_delimiter_errors() {
        let mut buf = buf_with(&["x"]);
        let act = dispatch("g", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    #[test]
    fn global_unterminated_pattern_errors() {
        let mut buf = buf_with(&["x"]);
        let act = dispatch("g/unterminated", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    // ── H (help) command ──────────────────────────────────────

    #[test]
    fn help_command_prints_command_summary() {
        let mut buf = buf_with(&["a"]);
        let act = dispatch("H", &mut buf);
        assert!(matches!(act, Action::Print(ref s) if s.contains("ved commands")));
    }

    #[test]
    fn help_mentions_every_command_the_dispatcher_accepts() {
        let Action::Print(help) = run_help() else {
            panic!("help should print");
        };
        for cmd in [
            "a", "i", "c", "d", "p", "n", "l", "j", "m", "t", "s", "g", "v", "w", "wq", "r", "e",
            "E", "q", "Q", "H",
        ] {
            let listed = help.lines().any(|l| {
                l.trim_start()
                    .trim_start_matches(|c| "[.,+1$]".contains(c))
                    .starts_with(cmd)
            });
            assert!(listed, "help does not list {cmd}");
        }
    }

    #[test]
    fn help_long_form_alias_works() {
        let mut buf = buf_with(&["a"]);
        let act = dispatch("help", &mut buf);
        assert!(matches!(act, Action::Print(_)));
    }

    // ── a/i/c (append/insert/change) pre-input-mode state ──────
    // These hand off to Action::EnterInputMode, so the actual text
    // entry isn't unit-testable here; what dispatch itself is
    // responsible for is positioning `current` correctly before
    // that handoff, which is what these check.

    #[test]
    fn append_positions_current_at_the_addressed_line() {
        let mut buf = buf_with(&["x", "y"]);
        let act = dispatch("1a", &mut buf);
        assert!(matches!(act, Action::EnterInputMode));
        assert_eq!(buf.current(), 1);
    }

    #[test]
    fn insert_positions_current_before_the_addressed_line() {
        let mut buf = buf_with(&["x", "y"]);
        let act = dispatch("2i", &mut buf);
        assert!(matches!(act, Action::EnterInputMode));
        assert_eq!(buf.current(), 1);
    }

    #[test]
    fn insert_at_line_one_positions_current_at_the_sentinel() {
        let mut buf = buf_with(&["x", "y"]);
        let _ = dispatch("1i", &mut buf);
        assert_eq!(buf.current(), 0);
    }

    #[test]
    fn change_deletes_the_range_and_positions_before_it() {
        let mut buf = buf_with(&["a", "b", "c"]);
        let act = dispatch("1,2c", &mut buf);
        assert!(matches!(act, Action::EnterInputMode));
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.line(1), Some("c"));
        assert_eq!(buf.current(), 0);
    }

    // ── Q (force quit) command ─────────────────────────────────

    #[test]
    fn force_quit_returns_force_quit_regardless_of_modified_state() {
        let mut buf = buf_with(&["x"]);
        assert!(matches!(dispatch("Q", &mut buf), Action::ForceQuit));
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
        assert!(matches!(act, Action::Confirm(_)));
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
    fn substitute_without_its_closing_delimiter_runs_to_the_end_of_the_line() {
        let mut buf = buf_with(&["abc"]);
        assert!(matches!(dispatch("s/a/b", &mut buf), Action::Confirm(ref l) if l == "bbc"));
        let mut buf = buf_with(&["abc"]);
        dispatch("s/b/", &mut buf);
        assert_eq!(buf.line(1), Some("ac"));
        let mut buf = buf_with(&["abc"]);
        assert!(matches!(dispatch("s/a", &mut buf), Action::Error(_)));
    }

    #[test]
    fn substitute_count_replaces_only_that_match() {
        for (cmd, want) in [("s/a/X/2", "aXa"), ("s/a/X/3", "aaX"), ("s/a/X/2p", "aXa")] {
            let mut buf = buf_with(&["aaa"]);
            assert!(
                matches!(
                    dispatch(cmd, &mut buf),
                    Action::Print(_) | Action::Confirm(_)
                ),
                "{cmd}"
            );
            assert_eq!(buf.line(1), Some(want), "{cmd}");
        }
        let mut buf = buf_with(&["aaa"]);
        assert!(matches!(dispatch("s/a/X/4", &mut buf), Action::Error(_)));
    }

    #[test]
    fn substitute_print_suffixes_choose_how_the_line_is_shown() {
        let mut buf = buf_with(&["aaa"]);
        assert!(matches!(dispatch("s/a/X/n", &mut buf), Action::Print(ref l) if l == "1\tXaa"));
        let mut buf = buf_with(&["aaa"]);
        assert!(matches!(dispatch("s/a/X/l", &mut buf), Action::Print(ref l) if l == "Xaa$"));
        let mut buf = buf_with(&["aaa"]);
        assert!(matches!(dispatch("s/a/X/gp", &mut buf), Action::Print(ref l) if l == "XXX"));
    }

    #[test]
    fn substitute_rejects_suffixes_it_does_not_understand() {
        for cmd in [
            "s/a/X/z", "s/a/X/gq", "s/a/X/2g", "s/a/X/g2", "s/a/X/0", "s/a/X/pp", "s/a/X/ g",
            "s/a/X/gg",
        ] {
            let mut buf = buf_with(&["aaa"]);
            assert!(matches!(dispatch(cmd, &mut buf), Action::Error(_)), "{cmd}");
            assert_eq!(buf.line(1), Some("aaa"), "{cmd}");
        }
    }

    #[test]
    fn bare_s_repeats_the_count_too() {
        let mut buf = buf_with(&["aaa", "aaa"]);
        dispatch("1s/a/X/2", &mut buf);
        dispatch("2s", &mut buf);
        assert_eq!(buf.line(2), Some("aXa"));
    }

    #[test]
    fn percent_alone_is_the_previous_replacement() {
        let mut buf = buf_with(&["ab"]);
        dispatch("s/a/X/", &mut buf);
        dispatch("s/b/%/", &mut buf);
        assert_eq!(buf.line(1), Some("XX"));
        dispatch("s/X/a%/", &mut buf);
        assert_eq!(buf.line(1), Some("a%X"));
        let mut fresh = buf_with(&["ab"]);
        assert!(matches!(dispatch("s/a/%/", &mut fresh), Action::Error(_)));
        assert_eq!(fresh.line(1), Some("ab"));
    }

    #[test]
    fn backslash_can_be_the_delimiter() {
        let mut buf = buf_with(&["a"]);
        dispatch("s\\a\\b\\", &mut buf);
        assert_eq!(buf.line(1), Some("b"));
    }

    #[test]
    fn a_delimiter_that_is_not_one_ascii_byte_is_an_error_not_a_panic() {
        for cmd in [
            "s\u{e9}*\u{d7}j\\",
            "s\u{e9}a\u{e9}b\u{e9}",
            "g\u{e9}a\u{e9}p",
            "s a b ",
        ] {
            let mut buf = buf_with(&["a"]);
            assert!(matches!(dispatch(cmd, &mut buf), Action::Error(_)), "{cmd}");
            assert_eq!(buf.line(1), Some("a"), "{cmd}");
        }
    }

    #[test]
    fn substitute_with_an_invalid_address_is_not_remembered() {
        let mut buf = buf_with(&["aaa", "bbb"]);
        assert!(matches!(dispatch("9s/a/X/g", &mut buf), Action::Error(_)));
        assert!(matches!(dispatch("1s", &mut buf), Action::Error(_)));
        assert_eq!(buf.line(1), Some("aaa"));
    }

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

    // ── substitute over multi-byte UTF-8 characters ────────────
    // (issue #13: `.` and bracket expressions in bre.rs now consume
    // a whole UTF-8 character, matching GNU ed's counts, closing the
    // gap #10 had worked around with a boundary check.)

    #[test]
    fn substitute_dot_global_on_multibyte_line_matches_by_character() {
        let mut buf = buf_with(&["café"]);
        let act = dispatch("s/./Y/g", &mut buf);
        assert!(!matches!(act, Action::Error(_)));
        // Four characters, not café's five bytes.
        assert_eq!(buf.line(1), Some("YYYY"));
    }

    #[test]
    fn substitute_does_not_panic_when_search_would_slide_into_a_character() {
        // A failed match attempt starting exactly on a multi-byte
        // character must not fall through to the search trying the
        // bytes inside it as further starting offsets: that used to
        // panic run_substitute's UTF-8 rebuild (a real bug caught
        // while closing out issue #13, distinct from the atom-width
        // fix itself — the failure lived in bre::Regex::find's
        // search loop).
        let mut buf = buf_with(&["\u{20ac}yq"]); // "€yq"
        let act = dispatch("s/\\(..\\)y/[\\1]/", &mut buf);
        assert!(matches!(act, Action::Error(_)));
        assert_eq!(buf.line(1), Some("\u{20ac}yq"));
    }

    #[test]
    fn global_substitute_with_star_that_can_match_empty_preserves_characters() {
        // `[a-z]*` matches zero-width at é, and stepping past an empty
        // match has to move a whole character, not a byte. The empty
        // match right after `caf` is not a new match. sed gives XéX.
        let mut buf = buf_with(&["café"]);
        let act = dispatch("s/[a-z]*/X/g", &mut buf);
        assert!(!matches!(act, Action::Error(_)));
        assert_eq!(buf.line(1), Some("XéX"));
    }

    #[test]
    fn substitute_global_matches_caret_once_and_skips_adjacent_empty_matches() {
        // Expected values are sed's: GNU ed refuses the empty-match
        // cases with "Infinite substitution loop".
        for (line, cmd, want) in [
            ("aaa", "s/^a/X/g", "Xaa"),
            ("baaac", "s/a*/X/g", "XbXcX"),
            ("abc", "s/x*/-/g", "-a-b-c-"),
            ("abc", "s/b*/-/g", "-a-c-"),
            ("bca", "s/a*$/E/g", "bcE"),
            ("abc", "s/$/!/g", "abc!"),
        ] {
            let mut buf = buf_with(&[line]);
            dispatch(cmd, &mut buf);
            assert_eq!(buf.line(1), Some(want), "{cmd} on {line}");
        }
    }

    // ── search addresses: /pattern/ and ?pattern? ─────────────
    // (issue #7)

    #[test]
    fn forward_search_finds_next_matching_line() {
        let mut buf = buf_with(&["alpha", "beta", "gamma"]);
        buf.set_current(1);
        let act = dispatch("/beta/p", &mut buf);
        assert!(matches!(act, Action::Print(ref s) if s == "beta"));
        assert_eq!(buf.current(), 2);
    }

    #[test]
    fn backward_search_wraps_past_the_top() {
        let mut buf = buf_with(&["alpha", "beta", "gamma"]);
        buf.set_current(1);
        let act = dispatch("?gamma?p", &mut buf);
        assert!(matches!(act, Action::Print(ref s) if s == "gamma"));
        assert_eq!(buf.current(), 3);
    }

    #[test]
    fn forward_search_wraps_past_the_bottom() {
        let mut buf = buf_with(&["alpha", "beta", "gamma"]);
        buf.set_current(3);
        let act = dispatch("/alpha/p", &mut buf);
        assert!(matches!(act, Action::Print(ref s) if s == "alpha"));
        assert_eq!(buf.current(), 1);
    }

    #[test]
    fn search_can_match_current_line_on_full_wraparound() {
        // Only the current line matches, so the search has to go all
        // the way around the buffer and back to it.
        let mut buf = buf_with(&["alpha", "beta"]);
        buf.set_current(1);
        let act = dispatch("/alpha/p", &mut buf);
        assert!(matches!(act, Action::Print(ref s) if s == "alpha"));
    }

    #[test]
    fn search_with_no_match_errors() {
        let mut buf = buf_with(&["alpha", "beta"]);
        buf.set_current(1);
        let act = dispatch("/nope/p", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    #[test]
    fn search_address_with_no_command_letter_prints() {
        // A bare address with nothing after it defaults to "go there
        // and print", same as any other address form.
        let mut buf = buf_with(&["alpha", "beta"]);
        buf.set_current(1);
        let act = dispatch("/beta", &mut buf);
        assert!(matches!(act, Action::Print(ref s) if s == "beta"));
    }

    #[test]
    fn search_pattern_can_escape_its_own_delimiter() {
        let mut buf = buf_with(&["foo/bar", "baz"]);
        buf.set_current(1);
        let act = dispatch("/foo\\/bar/p", &mut buf);
        assert!(matches!(act, Action::Print(ref s) if s == "foo/bar"));
    }

    #[test]
    fn search_address_works_as_a_range_endpoint() {
        let mut buf = buf_with(&["one", "two", "three", "four"]);
        buf.set_current(1);
        let act = dispatch("/two/,/four/p", &mut buf);
        assert!(matches!(act, Action::Print(_)));
    }

    #[test]
    fn search_on_empty_buffer_errors_not_panics() {
        let mut buf = Buffer::new();
        let act = dispatch("/x/p", &mut buf);
        assert!(matches!(act, Action::Error(_)));
    }

    #[test]
    fn dot_after_literal_consumes_the_whole_next_character() {
        // "f." matches 'f' plus the whole of é (not just its first
        // byte), so the replacement swallows both.
        let mut buf = buf_with(&["café"]);
        let act = dispatch("s/f./Z/", &mut buf);
        assert!(!matches!(act, Action::Error(_)));
        assert_eq!(buf.line(1), Some("caZ"));
    }

    #[test]
    fn substitute_whole_multibyte_character_still_works() {
        let mut buf = buf_with(&["café"]);
        let act = dispatch("s/é/e/", &mut buf);
        assert!(!matches!(act, Action::Error(_)));
        assert_eq!(buf.line(1), Some("cafe"));
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

    // ── non-ASCII input ──────────────────────────────────────

    #[test]
    fn non_ascii_command_letter_is_an_error_not_a_panic() {
        let mut buf = buf_with(&["a", "b"]);
        // The command letter is taken as a character, so a multi-byte
        // one reports an unknown command instead of slicing `rest`
        // off a char boundary.
        assert!(matches!(dispatch("\u{e9}", &mut buf), Action::Error(_)));
        assert!(matches!(
            dispatch("\u{4e16}\u{754c}", &mut buf),
            Action::Error(_)
        ));
        // With an address in front, and with trailing arguments.
        assert!(matches!(dispatch("1\u{e9}", &mut buf), Action::Error(_)));
        assert!(matches!(
            dispatch("1,2\u{e9}foo", &mut buf),
            Action::Error(_)
        ));
        // The buffer is untouched by any of them.
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.line(1), Some("a"));
    }
}

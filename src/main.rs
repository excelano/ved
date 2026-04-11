// ved - the verbose ed
// A drop-in compatible clone of ed with friendly errors,
// confirmations, long-form command aliases, and a built-in
// help system. Written in pure-stdlib Rust.

mod address;
mod buffer;

use address::Spec;
use buffer::Buffer;
use std::env;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let prompt = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ved: {e}");
            eprintln!("try 'ved --help' for usage information");
            return ExitCode::from(2);
        }
    };

    run_repl(prompt.as_deref())
}

/// Parse command line arguments.
/// Returns the prompt string (if any) on success.
fn parse_args(args: &[String]) -> Result<Option<String>, String> {
    let mut prompt: Option<String> = None;
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
            other => {
                return Err(format!("unknown argument: {other}"));
            }
        }
        i += 1;
    }
    Ok(prompt)
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
fn run_repl(prompt: Option<&str>) -> ExitCode {
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
            Action::EnterInputMode => {
                input_mode = true;
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
    EnterInputMode,
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
        return run_print(&spec, buf, false);
    }

    // Stage one: commands that take no arguments after the letter.
    // Exact-match these so we don't have to deal with `q` matching
    // the front of `quit`.
    match rest {
        "q" | "quit" => return Action::Quit,
        "a" => return Action::EnterInputMode,
        "p" => return run_print(&spec, buf, false),
        "n" => return run_print(&spec, buf, true),
        _ => {}
    }

    // Stage two: commands with arguments. Match on the first byte
    // and pass the rest as the argument string.
    let first = rest.as_bytes()[0];
    let args = &rest[1..];
    match first {
        b'w' => run_write(&spec, buf, args),
        _ => Action::Error(format!("unknown command: {rest}")),
    }
}

/// Resolve `spec` against the buffer and emit the corresponding
/// lines. With `numbered = true` each line is prefixed with its
/// 1-indexed position and a tab, matching ed's `n` command. Updates
/// the current line to the end of the printed range.
fn run_print(spec: &Spec, buf: &mut Buffer, numbered: bool) -> Action {
    let range = match spec.resolve(buf) {
        Ok(r) => r,
        Err(e) => return Action::Error(e),
    };

    let mut output = String::new();
    for n in range.start..=range.end {
        if n > range.start {
            output.push('\n');
        }
        if numbered {
            output.push_str(&format!("{n}\t"));
        }
        if let Some(line) = buf.line(n) {
            output.push_str(line);
        }
    }

    buf.set_current(range.end);
    Action::Print(output)
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

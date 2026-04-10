// ved - the verbose ed
// A drop-in compatible clone of ed with friendly errors,
// confirmations, long-form command aliases, and a built-in
// help system. Written in pure-stdlib Rust.

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
fn run_repl(prompt: Option<&str>) -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut reader = stdin.lock();
    let mut line = String::new();

    loop {
        if let Some(p) = prompt {
            if write!(out, "{p}").is_err() || out.flush().is_err() {
                return ExitCode::FAILURE;
            }
        }

        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return ExitCode::SUCCESS, // EOF
            Ok(_) => match dispatch(line.trim_end()) {
                Action::Continue => {}
                Action::Quit => return ExitCode::SUCCESS,
                Action::Print(msg) => {
                    let _ = writeln!(out, "{msg}");
                }
            },
            Err(e) => {
                eprintln!("ved: read error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
}

/// What the REPL should do after handling one input line.
/// Slice 2 will grow this to include buffer mutations.
enum Action {
    Continue,
    Quit,
    Print(String),
}

/// Dispatch a single command line. For slice 1 the only commands
/// we recognize are q/quit and the empty line. Everything else gets
/// a friendly "unknown command" reply.
fn dispatch(cmd: &str) -> Action {
    match cmd {
        "" => Action::Continue,
        "q" | "quit" => Action::Quit,
        other => Action::Print(format!("? unknown command: {other}")),
    }
}

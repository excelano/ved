// Run the ved binary on a script and check what it printed, the file it
// left, and its exit status.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

struct Run {
    stdout: String,
    status: i32,
    file: String,
}

fn run(args: &[&str], content: &str, script: &str) -> Run {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir: PathBuf = std::env::temp_dir().join(format!("ved_cli_{}_{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("f.txt"), content).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ved"))
        .args(args)
        .arg("f.txt")
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let file = std::fs::read_to_string(dir.join("f.txt")).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    Run {
        stdout: String::from_utf8(out.stdout).unwrap(),
        status: out.status.code().unwrap(),
        file,
    }
}

#[test]
fn a_clean_script_exits_zero() {
    let r = run(&[], "one\n", "s/one/two/\nw\nq\n");
    assert_eq!(r.status, 0);
    assert_eq!(r.file, "two\n");
}

#[test]
fn any_error_makes_the_exit_status_nonzero() {
    let r = run(&[], "one\n", "zz\n1p\nq\n");
    assert_eq!(r.status, 1);
    assert!(
        r.stdout.contains("one"),
        "commands after the error still run"
    );
}

#[test]
fn the_modified_buffer_warning_counts_as_an_error() {
    let r = run(&[], "one\n", "a\nX\n.\nq\nq\n");
    assert_eq!(r.status, 1);
    assert_eq!(r.file, "one\n");
}

#[test]
fn wq_saves_and_exits() {
    let r = run(&[], "one\n", "s/one/ONE/\nwq\n");
    assert_eq!(r.status, 0);
    assert_eq!(r.file, "ONE\n");
}

#[test]
fn end_of_input_on_a_modified_buffer_warns_like_q() {
    let r = run(&[], "one\n", "a\nX\n.\n");
    assert!(r.stdout.contains("warning: buffer modified"));
    assert_eq!(r.status, 1);
    assert_eq!(r.file, "one\n");
}

#[test]
fn end_of_input_on_a_saved_buffer_exits_quietly() {
    let r = run(&[], "one\n", "a\nX\n.\nw\n");
    assert!(!r.stdout.contains("warning"));
    assert_eq!(r.status, 0);
    assert_eq!(r.file, "one\nX\n");
}

#[test]
fn script_flag_prints_only_what_the_script_asks_for() {
    let r = run(&["-s"], "hello\n", ",p\nd\nw\nq\n");
    assert_eq!(r.stdout, "hello\n");
    assert_eq!(r.file, "");
}

#[test]
fn script_flag_keeps_s_quiet_unless_it_has_a_print_suffix() {
    let r = run(&["-s"], "aaa\n", "s/a/X/\ns/a/Y/p\nw\nq\n");
    assert_eq!(r.stdout, "XYa\n");
    assert_eq!(r.file, "XYa\n");
}

#[test]
fn script_flag_still_reports_errors() {
    let r = run(&["-s"], "one\n", "zz\nq\n");
    assert_eq!(r.stdout, "? unknown command: zz\n");
    assert_eq!(r.status, 1);
}

#[test]
fn without_the_script_flag_ved_confirms_each_command() {
    let r = run(&[], "hello\n", ",p\nd\nw\nq\n");
    assert_eq!(
        r.stdout,
        "6\nhello\ndeleted line 1\nwrote 0 bytes to f.txt\n"
    );
}

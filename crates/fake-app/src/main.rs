//! A program that stands in for an app the hub starts. It appends one line of JSON (its argument list, its
//! working folder, its process id) to `fake-app.log` in the folder of its own file, and ends. The log is next
//! to the program, so that every test, which has a program of its own in a folder of its own, reads only its
//! own starts.

use std::io::Write;

fn main() {
    let exe = std::env::current_exe().expect("own path");
    let log = exe.with_file_name("fake-app.log");
    let line = serde_json::json!({
        "argv": std::env::args().skip(1).collect::<Vec<_>>(),
        "cwd": std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
        "pid": std::process::id(),
    });
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .expect("log file");
    writeln!(out, "{line}").expect("write");
}

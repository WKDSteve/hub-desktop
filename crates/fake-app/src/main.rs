//! A program that stands in for an app the hub starts. It appends one line of JSON (its argument list, its
//! working folder, its process id) to `fake-app.log` in the folder of its own file, and ends. The log is next
//! to the program, so that every test, which has a program of its own in a folder of its own, reads only its
//! own starts. It also names the variables of its environment that end in `_STAY` (what hub-desktop sets for a hub it
//! starts with itself).
//!
//! For the runner of the updates it also takes a few options, anywhere in its arguments: `--say <text>` prints a
//! line to stdout, `--err <text>` one to stderr, `--sleep <ms>` waits, `--exit <code>` ends with that code,
//! `--env <name>` prints `<name>=<value>` (or `<unset>`), `--mark <file>` writes that file, `--spawn <file>` starts
//! itself in the background (without waiting) to write `<file>` after 1.5 s: a process the step left behind.
//! Every other argument is only logged.
//!
//! On Windows it is a GUI-subsystem program: started with a console of its own (`Window::NewConsole`, what the hub
//! does for a server like ai-desktop) it still gets none, so the tests open no console window. On a self-hosted
//! runner such a window took the focus on every run. Its stdout and stderr still reach a pipe the caller hands it.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::io::Write;

fn main() {
    let exe = std::env::current_exe().expect("own path");
    let log = exe.with_file_name("fake-app.log");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let line = serde_json::json!({
        "argv": args,
        "cwd": std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
        "pid": std::process::id(),
        // The variables a starter set to tell a hub to stay (`<APP>_STAY`), by name.
        "stay": stay_vars(),
    });
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .expect("log file");
    writeln!(out, "{line}").expect("write");

    let mut code = 0;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = || rest.clone().next().cloned().unwrap_or_default();
        match arg.as_str() {
            "--say" => println!("{}", value()),
            "--err" => eprintln!("{}", value()),
            "--sleep" => std::thread::sleep(std::time::Duration::from_millis(
                value().parse().unwrap_or(0),
            )),
            "--exit" => code = value().parse().unwrap_or(1),
            "--mark" => std::fs::write(value(), b"alive").expect("mark"),
            "--spawn" => {
                // The null device for its streams: it holds no pipe of the step, it only outlives it. Never waited
                // for: a process left behind is what the tests of the update runner look for.
                #[allow(clippy::zombie_processes)]
                std::process::Command::new(&exe)
                    .args(["--sleep", "1500", "--mark", &value()])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .expect("spawn");
            }
            "--env" => println!(
                "{}={}",
                value(),
                std::env::var(value()).unwrap_or_else(|_| "<unset>".to_owned())
            ),
            _ => continue,
        }
        rest.next();
    }
    std::process::exit(code);
}

/// The names of the variables that end in `_STAY`, sorted.
fn stay_vars() -> Vec<String> {
    let mut names: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| name.ends_with("_STAY"))
        .collect();
    names.sort();
    names
}

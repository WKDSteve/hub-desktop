//! A program that stands in for an app the hub starts. It appends one line of JSON (its argument list, its
//! working folder, its process id) to `fake-app.log` in the folder of its own file, and ends. The log is next
//! to the program, so that every test, which has a program of its own in a folder of its own, reads only its
//! own starts.
//!
//! For the runner of the updates it also takes a few options, anywhere in its arguments: `--say <text>` prints a
//! line to stdout, `--err <text>` one to stderr, `--sleep <ms>` waits, `--exit <code>` ends with that code,
//! `--env <name>` prints `<name>=<value>` (or `<unset>`).
//! Every other argument is only logged.

use std::io::Write;

fn main() {
    let exe = std::env::current_exe().expect("own path");
    let log = exe.with_file_name("fake-app.log");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let line = serde_json::json!({
        "argv": args,
        "cwd": std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
        "pid": std::process::id(),
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

//! `hub-desktop serve | start | open | stop | install | uninstall | --version`; without an argument (a
//! double click, or the link that `install` makes) it starts the hub in the background and opens the browser.
//!
//! The release build is a window program (appkit `docs/PROCESSES.md`, "A hub is a window program"): Windows
//! opens no console window for the link or a double click, and from a terminal it attaches to the terminal's
//! console. A debug build stays a console program.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use appkit_core::console::{self, Console};
use hub_desktop::app::{self, ServeOptions};
use hub_desktop::background::{self, UninstallOptions};

const USAGE: &str = "usage:
  hub-desktop                              (no argument, as a double click) start the hub in the
                                           background and open the browser (a debug build runs it in
                                           its own console window); a hub that runs already is not
                                           started twice, the browser opens on it
  hub-desktop serve [--port <1024-65535>]  run the hub until Ctrl-C
  hub-desktop start [--open]               run the hub in the background (no window; log in the
                                           config folder); --open also opens the browser
  hub-desktop stop                         stop the hub that runs in the background
  hub-desktop install [--shortcut]         copy this program to a fixed place (a build then does
                                           not fight the running hub for the file); the Start
                                           menu gets a link, --shortcut adds one
                                           on the desktop
  hub-desktop uninstall [--purge-settings] stop the hub and take away what install made;
                                           --purge-settings also removes the settings; the program
                                           file is left for the script
  hub-desktop open [--launch]              print a one-time pairing link for the running hub
                                           (--launch also opens it in the default browser)
  hub-desktop --version";

fn parse_port(args: &[String]) -> Result<Option<u16>, String> {
    match args {
        [] => Ok(None),
        [flag, value] if flag == "--port" => value
            .parse::<u16>()
            .ok()
            .filter(|p| *p >= 1024)
            .map(Some)
            .ok_or_else(|| format!("--port must be a number from 1024 to 65535, not {value:?}")),
        _ => Err(USAGE.to_owned()),
    }
}

/// A console that was opened by a double click closes with the program: an error is read before that.
fn wait_for_enter() {
    use std::io::{BufRead, IsTerminal, Write};
    if std::io::stdin().is_terminal() {
        eprint!("Press Enter to close this window.");
        let _ = std::io::stderr().flush();
        let _ = std::io::stdin().lock().lines().next();
    }
}

/// The verb the arguments name. Apart from `main` so that its future lives on the heap: the main thread
/// of Windows has 1 MB of stack, and the future of all the verbs together is too big for it in a debug
/// build.
async fn run(args: Vec<String>, console: Console) -> Result<(), String> {
    match args.first().map(String::as_str) {
        // A console of its own (a debug build) is a window the hub can run in; a window program has none.
        None if console == Console::Own => app::click().await,
        None => app::click_in_background().await,
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("serve") => match parse_port(&args[1..]) {
            Ok(port) => app::serve(ServeOptions { port, open: false }).await,
            Err(e) => Err(e),
        },
        Some("start") if args.len() == 1 || (args.len() == 2 && args[1] == "--open") => {
            match background::start().await {
                Ok(text) => {
                    print!("{text}");
                    if args.len() == 2 {
                        match app::open().await {
                            Ok(url) => app::deliver(&url),
                            Err(e) => Err(e),
                        }
                    } else {
                        Ok(())
                    }
                }
                Err(e) => Err(e),
            }
        }
        Some("install") if args.len() == 1 || (args.len() == 2 && args[1] == "--shortcut") => {
            background::install(background::install_options(args.len() == 2))
                .await
                .map(|t| print!("{t}"))
        }
        Some("uninstall")
            if args.len() == 1 || (args.len() == 2 && args[1] == "--purge-settings") =>
        {
            let opts = UninstallOptions {
                purge_settings: args.len() == 2,
            };
            background::uninstall(opts).await.map(|t| print!("{t}"))
        }
        Some("stop") if args.len() == 1 => background::stop().await.map(|t| print!("{t}")),
        Some("open") if args.len() == 1 => app::open().await.map(|url| println!("{url}")),
        Some("open") if args.len() == 2 && args[1] == "--launch" => match app::open().await {
            Ok(url) => {
                println!("{url}");
                app::launch(&url)
            }
            Err(e) => Err(e),
        },
        Some("--version" | "-V") if args.len() == 1 => {
            println!("{}", hub_desktop::version_line());
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}

#[tokio::main]
async fn main() {
    // Before the first line is written.
    let console = console::attach_parent();
    console::report_panics("hub-desktop");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let by_click = args.is_empty();
    if let Err(message) = Box::pin(run(args, console)).await {
        // stderr, or a message box when nobody reads it (the link of a window program).
        console::report("hub-desktop", &message);
        // In the console of a terminal the shell reads the same keys: only a console of its own waits.
        if by_click && console == Console::Own {
            wait_for_enter();
        }
        std::process::exit(1);
    }
}

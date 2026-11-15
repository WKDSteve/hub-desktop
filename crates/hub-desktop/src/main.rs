//! `hub-desktop serve | tray | start | open | stop | autostart | install | uninstall | --version`; without an
//! argument (a double click, or the link that `install` makes) it starts the hub in the background (on Windows
//! with its icon in the notification area) and opens the browser.
//!
//! The release build is a window program (appkit `docs/PROCESSES.md`, "A hub is a window program"): Windows
//! opens no console window for the link or a double click, and from a terminal it attaches to the terminal's
//! console. A debug build stays a console program.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use appkit_core::console::{self, Console};
use appkit_hub::background::Autostart;
use hub_desktop::app::{self, ServeOptions};
use hub_desktop::background::{self, UninstallOptions};

const USAGE: &str = "usage:
  hub-desktop                              (no argument, as a double click) start the hub in the
                                           background and open the browser (a debug build runs it in
                                           its own console window); a hub that runs already is not
                                           started twice, the browser opens on it
  hub-desktop serve [--port <1024-65535>]  run the hub until Ctrl-C
  hub-desktop tray                         run the hub with its icon in the notification area
                                           (Windows): open the page, open and stop the other hubs,
                                           quit; it starts the hubs of start_with_hub in config.json
  hub-desktop start [--open]               run the hub in the background (on Windows: tray; log in
                                           the config folder); --open also opens the browser
  hub-desktop stop                         stop the hub that runs in the background
  hub-desktop autostart on|off|status      start hub-desktop with your login; on takes over the
                                           login entries of tasks, rules and testing (their hubs
                                           then start with hub-desktop: start_with_hub)
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
        Some("autostart") if args.len() == 2 => {
            let what = match args[1].as_str() {
                "on" => Autostart::On,
                "off" => Autostart::Off,
                "status" => Autostart::Status,
                _ => return Err(USAGE.to_owned()),
            };
            let dir = appkit_hub::instance::config_folder(
                &hub_desktop::config::Env::from_process(),
                &hub_desktop::IDENTITY,
            )?;
            hub_desktop::login::autostart(&dir, what, &hub_desktop::login::SystemLogin)
                .map(|t| print!("{t}"))
        }
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

fn main() {
    // Before the first line is written.
    let console = console::attach_parent();
    console::report_panics("hub-desktop");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let by_click = args.is_empty();
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => fail(&format!("no runtime: {e}"), false, console),
    };
    // The icon needs the main thread for the messages of the system; the hub runs on the runtime's.
    let result = if args.len() == 1 && args[0] == "tray" {
        tray(rt)
    } else {
        rt.block_on(Box::pin(run(args, console)))
    };
    if let Err(message) = result {
        fail(&message, by_click, console);
    }
}

#[cfg(windows)]
fn tray(rt: tokio::runtime::Runtime) -> Result<(), String> {
    hub_desktop::tray::run(rt)
}

#[cfg(not(windows))]
fn tray(_rt: tokio::runtime::Runtime) -> Result<(), String> {
    Err(
        "the icon in the notification area is built for Windows only: run `hub-desktop serve`"
            .to_owned(),
    )
}

fn fail(message: &str, by_click: bool, console: Console) -> ! {
    // stderr, or a message box when nobody reads it (the link of a window program).
    console::report("hub-desktop", message);
    // In the console of a terminal the shell reads the same keys: only a console of its own waits.
    if by_click && console == Console::Own {
        wait_for_enter();
    }
    std::process::exit(1);
}

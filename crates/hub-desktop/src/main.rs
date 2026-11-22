//! The commands of hub-desktop: those every hub app shares (`appkit_hub::cli`: `start`, `stop`/`quit`, `restart`,
//! `status`, `open`, `link`, `logs`, `paths`, `version`, `help`) and its own (`serve`, `tray`, `autostart`, `install`,
//! `uninstall`). Without an argument (a double click, or the link that `install` makes) it starts the hub in the
//! background (on Windows with its icon in the notification area) and opens the browser.
//!
//! Exit codes: 0 done, 1 failed (`status`: the hub does not run), 2 wrong arguments.
//!
//! The release build is a window program (appkit `docs/PROCESSES.md`, "A hub is a window program"): Windows
//! opens no console window for the link or a double click, and from a terminal it attaches to the terminal's
//! console. A debug build stays a console program.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use appkit_core::console::{self, Console};
use appkit_hub::background::Autostart;
use appkit_hub::cli::{self, Parsed, Verb};
use hub_desktop::app::{self, ServeOptions};
use hub_desktop::background::{self, UninstallOptions};

/// The command in the terminal.
const COMMAND: &str = "hub-desktop";

/// The commands of hub-desktop that are not common to the hub apps, with what they do.
const OWN_USAGE: [(&str, &str); 5] = [
    (
        "serve",
        "  hub-desktop serve [--port <1024-65535>]  run the hub in this terminal until Ctrl-C\n",
    ),
    (
        "tray",
        "  hub-desktop tray                         run the hub with its icon in the notification area
                                           (Windows): open the page, open and stop the other hubs,
                                           quit; it starts the hubs of start_with_hub in config.json\n",
    ),
    (
        "autostart",
        "  hub-desktop autostart on|off|status      start hub-desktop with your login; on takes over the
                                           login entries of tasks, rules and testing (their hubs
                                           then start with hub-desktop: start_with_hub)\n",
    ),
    (
        "install",
        "  hub-desktop install [--shortcut]         copy this program to a fixed place (a build then does
                                           not fight the running hub for the file) and make the
                                           command `hub-desktop` for every new terminal; the Start
                                           menu gets a link, --shortcut adds one on the desktop\n",
    ),
    (
        "uninstall",
        "  hub-desktop uninstall [--purge-settings] stop the hub and take away what install made;
                                           --purge-settings also removes the settings; the program
                                           file is left for the script\n",
    ),
];

/// The text of `help`: the common commands, then the own ones, then what the background hub does.
fn usage() -> String {
    let mut out = String::from("usage:\n");
    out.push_str(&cli::usage_lines(COMMAND));
    for (_, lines) in OWN_USAGE {
        out.push_str(lines);
    }
    out.push_str(
        "\nOn Windows the hub in the background has its icon in the notification area (tray); elsewhere it ends
itself 10 minutes after the last page closed, unless it starts with your login or keep_running is set
in config.json. A hub that `start --open` or `open` started is stopped again when the page cannot be
opened. Exit codes: 0 done, 1 failed (status: the hub does not run), 2 wrong arguments.",
    );
    out
}

/// What one command does, for `help <command>`.
fn help_of(command: &str) -> Option<String> {
    cli::help(command, COMMAND).or_else(|| {
        OWN_USAGE
            .iter()
            .find(|(name, _)| *name == command)
            .map(|(_, lines)| lines.trim_end().to_owned())
    })
}

/// Why the program ends without having done its work, and with which exit code.
struct Failed {
    message: String,
    code: i32,
}

impl Failed {
    /// Wrong arguments: the usage, exit code 2.
    fn usage(message: String) -> Self {
        Self {
            message,
            code: cli::USAGE_EXIT,
        }
    }
}

impl From<String> for Failed {
    /// A command that failed: exit code 1.
    fn from(message: String) -> Self {
        Self { message, code: 1 }
    }
}

fn parse_port(args: &[String]) -> Result<Option<u16>, Failed> {
    match args {
        [] => Ok(None),
        [flag, value] if flag == "--port" => value
            .parse::<u16>()
            .ok()
            .filter(|p| *p >= 1024)
            .map(Some)
            .ok_or_else(|| {
                Failed::usage(format!(
                    "--port must be a number from 1024 to 65535, not {value:?}"
                ))
            }),
        _ => Err(Failed::usage(usage())),
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

/// The command the arguments name; the exit code when it ran. Apart from `main` so that its future lives on the
/// heap: the main thread of Windows has 1 MB of stack, and the future of all the commands together is too big for it
/// in a debug build.
async fn run(args: Vec<String>, console: Console) -> Result<i32, Failed> {
    let verb = match cli::parse(&args) {
        Parsed::Common(verb) => verb,
        Parsed::Usage(text) => return Err(Failed::usage(text)),
        Parsed::Own => return run_own(&args).await.map(|()| 0),
    };
    match verb {
        // A console of its own (a debug build) is a window the hub can run in; a window program has none.
        Verb::Click if console == Console::Own => app::click().await?,
        Verb::Click => app::click_in_background().await?,
        Verb::Start { open: false } => print!("{}", background::start().await?),
        Verb::Start { open: true } | Verb::Open => app::start_and_open().await?,
        Verb::Stop => print!("{}", background::stop().await?),
        Verb::Restart { open } => {
            print!("{}", background::stop().await?);
            if open {
                app::start_and_open().await?;
            } else {
                print!("{}", background::start().await?);
            }
        }
        Verb::Status { json } => {
            let status = cli::status(&background::APP, hub_desktop::VERSION).await;
            if json {
                println!("{}", status.json());
            } else {
                print!("{}", status.text());
            }
            return Ok(status.exit_code());
        }
        Verb::Link => println!("{}", app::open().await?),
        Verb::Logs { lines, follow } => cli::logs(&background::APP, lines, follow).await?,
        Verb::Paths => print!("{}", cli::paths(&background::APP)),
        Verb::Version => println!("{}", hub_desktop::version_line()),
        Verb::Help(None) => println!("{}", usage()),
        Verb::Help(Some(command)) => match help_of(&command) {
            Some(text) => println!("{text}"),
            None => {
                return Err(Failed::usage(format!(
                    "no command {command:?}\n\n{}",
                    usage()
                )));
            }
        },
    }
    Ok(0)
}

/// The commands of hub-desktop itself (`tray` runs in `main`, on the main thread).
async fn run_own(args: &[String]) -> Result<(), Failed> {
    match args.first().map(String::as_str) {
        Some("serve") => {
            let port = parse_port(&args[1..])?;
            app::serve(ServeOptions {
                port,
                open: false,
                alone: true,
            })
            .await?;
        }
        Some("install") if args.len() == 1 || (args.len() == 2 && args[1] == "--shortcut") => {
            print!(
                "{}",
                background::install(background::install_options(args.len() == 2)).await?
            );
        }
        Some("uninstall")
            if args.len() == 1 || (args.len() == 2 && args[1] == "--purge-settings") =>
        {
            let opts = UninstallOptions {
                purge_settings: args.len() == 2,
            };
            print!("{}", background::uninstall(opts).await?);
        }
        Some("autostart") if args.len() == 2 => {
            let what = match args[1].as_str() {
                "on" => Autostart::On,
                "off" => Autostart::Off,
                "status" => Autostart::Status,
                _ => return Err(Failed::usage(usage())),
            };
            let dir = appkit_hub::instance::config_folder(
                &hub_desktop::config::Env::from_process(),
                &hub_desktop::IDENTITY,
            )?;
            print!(
                "{}",
                hub_desktop::login::autostart(&dir, what, &hub_desktop::login::SystemLogin)?
            );
        }
        _ => return Err(Failed::usage(usage())),
    }
    Ok(())
}

fn main() {
    // First, before the runtime and the console: on Linux and macOS this program is also the watcher of the process
    // group of each child it starts (started with `--appkit-child-watcher`, it only waits and never returns here);
    // docs/PROCESSES.md of appkit.
    appkit_core::child::watcher_main();
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
        Err(e) => fail(&format!("no runtime: {e}"), 1, false, console),
    };
    // The icon needs the main thread for the messages of the system; the hub runs on the runtime's.
    let result = if args.len() == 1 && args[0] == "tray" {
        tray(rt).map(|()| 0).map_err(Failed::from)
    } else {
        rt.block_on(Box::pin(run(args, console)))
    };
    match result {
        Ok(0) => {}
        // `status` of a hub that does not run: said on stdout already, only the exit code is left.
        Ok(code) => std::process::exit(code),
        Err(failed) => fail(&failed.message, failed.code, by_click, console),
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

fn fail(message: &str, code: i32, by_click: bool, console: Console) -> ! {
    // stderr, or a message box when nobody reads it (the link of a window program).
    console::report("hub-desktop", message);
    // In the console of a terminal the shell reads the same keys: only a console of its own waits.
    if by_click && console == Console::Own {
        wait_for_enter();
    }
    std::process::exit(code);
}

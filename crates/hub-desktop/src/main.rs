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
use hub_desktop::family_cli;

/// The command in the terminal.
const COMMAND: &str = "hub-desktop";

/// The commands of hub-desktop that are not common to the hub apps, with what they do.
const OWN_USAGE: [(&str, &str); 9] = [
    (
        "start",
        "  hub-desktop start --all                  also start the hubs of the apps of the kit that are
                                           installed (tasks, rules, testing; each in the background,
                                           without a page; one that is not set up waits for its
                                           first start in its page)\n",
    ),
    (
        "stop",
        "  hub-desktop stop --all                   also ask the hubs of the other apps that run to stop\n",
    ),
    (
        "apps",
        "  hub-desktop apps [--json]                every app of the catalog: installed, running, port,
                                           start with the login, its command in the terminal, and
                                           the version of a hub that runs\n",
    ),
    (
        "cli",
        "  hub-desktop cli status [--json]          the commands of the family in the terminal (`tasks-desktop`,
                                           ...): one per app that is installed, whether its starter is
                                           as it should be, and whether the folder is on the PATH; exit
                                           code 1 when something is not
  hub-desktop cli repair                   make the commands of all apps that are installed and take away
                                           those of apps that are gone (`install` does the same step)\n",
    ),
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

/// What one command does, for `help <command>`: the common text, the own lines, or both (`start --all` goes with
/// `start`).
fn help_of(command: &str) -> Option<String> {
    let own = OWN_USAGE
        .iter()
        .find(|(name, _)| *name == command)
        .map(|(_, lines)| lines.trim_end().to_owned());
    match (cli::help(command, COMMAND), own) {
        (Some(common), Some(own)) => Some(format!("{common}\n{own}")),
        (common, own) => common.or(own),
    }
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
        Parsed::Own => return Box::pin(run_own(&args)).await,
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

/// `start --all`, `stop --all`, `apps`, `cli status` and `cli repair`: what looks after the apps of the family (see
/// `family_cli`). A function of its own, so that the futures it makes are not in the frame of the commands that run for
/// long: in a debug build every future a function makes has its room in the frame for as long as the function runs, and
/// the main thread of Windows has 1 MB.
async fn run_family(args: &[String]) -> Result<i32, Failed> {
    match args {
        [start, all] if start == "start" && all == "--all" => {
            // hub-desktop itself first (the icon starts the hubs of `start_with_hub`), then the other hubs of the kit.
            print!("{}", Box::pin(background::start()).await?);
            let env = hub_desktop::config::Env::from_process();
            print!("{}", Box::pin(family_cli::start_all(&env)).await?);
        }
        [stop, all] if stop == "stop" && all == "--all" => {
            // The others first: they are asked, and the hub that is left says so when it goes.
            let env = hub_desktop::config::Env::from_process();
            print!("{}", Box::pin(family_cli::stop_all(&env)).await?);
            print!("{}", Box::pin(background::stop()).await?);
        }
        [apps] if apps == "apps" => print_apps(false).await,
        [apps, json] if apps == "apps" && json == "--json" => print_apps(true).await,
        [cli, repair] if cli == "cli" && repair == "repair" => {
            let env = hub_desktop::config::Env::from_process();
            let found = Box::pin(family_cli::wanted(&env)).await;
            let said = family_cli::repair(&env, &found, false)?;
            if said.is_empty() {
                println!("nothing to make here (a test of the program asks for no change to the machine)");
            } else {
                print!("{said}");
            }
        }
        [cli, status] if cli == "cli" && status == "status" => return cli_status(false).await,
        [cli, status, json] if cli == "cli" && status == "status" && json == "--json" => {
            return cli_status(true).await;
        }
        _ => return Err(Failed::usage(usage())),
    }
    Ok(0)
}

/// The table `apps`, as text or as a document.
async fn print_apps(json: bool) {
    let env = hub_desktop::config::Env::from_process();
    let rows = Box::pin(family_cli::apps(&env)).await;
    if json {
        println!("{}", serde_json::json!({ "apps": rows }));
    } else {
        print!("{}", family_cli::apps_text(&rows));
    }
}

/// `cli status`: the report, and exit code 1 when a command is not as it should be.
async fn cli_status(json: bool) -> Result<i32, Failed> {
    let env = hub_desktop::config::Env::from_process();
    let found = Box::pin(family_cli::wanted(&env)).await;
    let report = family_cli::status(&env, &found);
    if json {
        let mut doc = serde_json::to_value(&report).unwrap_or_default();
        doc["ok"] = report.healthy().into();
        println!("{doc}");
    } else {
        print!("{}", report.text());
    }
    Ok(i32::from(!report.healthy()))
}

/// The commands of hub-desktop itself (`tray` runs in `main`, on the main thread); the exit code when one ran.
async fn run_own(args: &[String]) -> Result<i32, Failed> {
    match args.first().map(String::as_str) {
        // What looks after the whole family has a function of its own: its futures would sit in the frame of this one,
        // which `serve` and `install` run in.
        Some("start" | "stop" | "apps" | "cli") => return Box::pin(run_family(args)).await,
        Some("serve") => {
            let port = parse_port(&args[1..])?;
            // The commands that run long are boxed where they are made, as in the other apps of the family: in a
            // debug build the future of a command made inline has its room in the frame of this function, and the
            // main thread of Windows has 1 MB (the family commands overflowed it here once, see `run_family`).
            Box::pin(app::serve(ServeOptions {
                port,
                open: false,
                alone: true,
            }))
            .await?;
        }
        Some("install") if args.len() == 1 || (args.len() == 2 && args[1] == "--shortcut") => {
            print!(
                "{}",
                Box::pin(background::install(background::install_options(
                    args.len() == 2
                )))
                .await?
            );
        }
        Some("uninstall")
            if args.len() == 1 || (args.len() == 2 && args[1] == "--purge-settings") =>
        {
            let opts = UninstallOptions {
                purge_settings: args.len() == 2,
            };
            print!("{}", Box::pin(background::uninstall(opts)).await?);
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
    Ok(0)
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

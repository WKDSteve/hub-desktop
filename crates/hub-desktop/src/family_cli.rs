//! The commands of the apps of the family in the terminal, seen from hub-desktop: `install` makes `tasks-desktop`,
//! `rules-desktop`, ... a command for every app it finds on the machine (the starters of `appkit_hub::cli_entry`), `cli
//! status` and `cli repair` look at and mend them, `apps` is the table of what is on the machine, and `start --all` /
//! `stop --all` start and stop the hubs of the kit together.
//!
//! The apps of the kit make their own command when they are installed; this is the other way round, so that whoever
//! installs hub-desktop has all of them (an app that was installed before the kit had commands, one whose install was
//! run by a script that did not know of them). A starter is the same file either way: `cli_entry` writes both, and one
//! that is there with the same text is left as it is.
//!
//! What is looked at is the catalog (`catalog`): the apps built on the kit, and docmap-desktop and ai-desktop, whose
//! setups make their commands too. ai-desktop gets none on the workstation, where it is never installed
//! (`MASCHINE-03`). A file in the starter folder that is not a starter (it lacks the mark of `cli_entry`) is never
//! overwritten or removed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use appkit_core::family;
use appkit_core::registry::HubRecord;
use appkit_hub::cli_entry::{self, UserPath};
use appkit_hub::instance;
use serde::Serialize;

use crate::catalog::{Entry, CATALOG};
use crate::config::Env;
use crate::detect::{AppState, Detector, Run};
use crate::launch::{Browser, LaunchConfig, Launcher, Outcome, Request};
use crate::machine::Role;

/// The command of hub-desktop itself.
pub const OWN_COMMAND: &str = "hub-desktop";

/// The command an app of the catalog gets: its name for an app built on the kit (`tasks-desktop`), and for the two apps
/// whose setups make a command of their own the name of the app. None: the app has no command (a web variant, a program
/// of somebody else).
#[must_use]
pub fn command_of(entry: &Entry) -> Option<&'static str> {
    match entry.id {
        "docmap" | "ai" => Some(entry.repo.name),
        _ => entry.kit_identity().map(|identity| identity.app),
    }
}

/// A path as a shell reads it. The detection resolves every link on the way to a program, and on Windows that gives
/// the verbatim form `\\?\C:\...`; `cmd` cannot run a program by that name, and a starter could not name it by
/// `%LOCALAPPDATA%`. The prefix of a drive path is taken off; a share (`\\?\UNC\...`) is left as it is.
#[must_use]
pub fn plain_path(path: PathBuf) -> PathBuf {
    let plain = path
        .to_string_lossy()
        .strip_prefix(r"\\?\")
        .filter(|rest| {
            let b = rest.as_bytes();
            b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\'
        })
        .map(str::to_owned);
    plain.map_or(path, PathBuf::from)
}

/// An app that is there, and the command it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// The id of the app in the catalog (`tasks`; `hub` for hub-desktop itself).
    pub id: &'static str,
    pub command: &'static str,
    /// The program the starter runs.
    pub program: PathBuf,
}

/// Where the starters are made and which `PATH` is changed, as `appkit_hub::background::install` decides it for the
/// command of the app itself: nowhere when a test of the program asks for no changes to the machine
/// (`HUB_DESKTOP_NO_LOGIN_SYNC`) and names no family folder of its own (`APPKIT_FAMILY_DIR`); there the starters go and
/// the `PATH` of the user stays; on Windows the `PATH` of the user, elsewhere none (`~/.local/bin` is on it already).
/// `None`: nothing is made.
fn path_store(env: &Env) -> Option<Option<&'static dyn UserPath>> {
    let tests = env.get(&crate::IDENTITY.var("NO_LOGIN_SYNC")).is_some();
    if tests && env.get(family::FAMILY_DIR_VAR).is_none() {
        return None;
    }
    if tests || !cfg!(windows) {
        return Some(None);
    }
    Some(Some(&cli_entry::SystemPath))
}

/// The apps of the family that are installed here, with the command each gets: hub-desktop itself first (when its
/// installed program is there), then the catalog in its order. A look at files and ports, nothing is started.
pub async fn wanted(env: &Env) -> Vec<Wanted> {
    let mut found = Vec::new();
    if let Ok(own) = crate::background::installed_program(env)
        && own.is_file()
    {
        found.push(Wanted {
            id: "hub",
            command: OWN_COMMAND,
            program: own,
        });
    }
    let role = Role::of(env);
    let detector = Detector::new(env.clone(), None);
    for state in detector.apps().await.iter() {
        let (Some(command), Some(program)) = (command_of(state.entry), state.native.clone()) else {
            continue;
        };
        let program = plain_path(program);
        // The ai family is never on the workstation, also not as a command (`MASCHINE-03`).
        if state.entry.id == "ai" && !role.installs() {
            continue;
        }
        found.push(Wanted {
            id: state.entry.id,
            command,
            program,
        });
    }
    found
}

/// What a command of the family is in the starter folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Starter {
    /// There, with the text a repair would write.
    Ok,
    /// Not there.
    Missing,
    /// There (a starter), but not as a repair would write it: it names another program, or one file of the two is
    /// missing.
    Stale,
    /// A file of that name that is not a starter: it is left as it is.
    Foreign,
    /// A starter of an app that is not installed (any more): a repair takes it away.
    Orphan,
}

impl Starter {
    fn word(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Missing => "missing",
            Self::Stale => "stale",
            Self::Foreign => "foreign",
            Self::Orphan => "orphan",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Ok => "",
            Self::Missing => "`hub-desktop cli repair` makes it",
            Self::Stale => "it names another program: `hub-desktop cli repair` writes it again",
            Self::Foreign => "a file of that name that is not a starter: left as it is",
            Self::Orphan => "its app is not installed: `hub-desktop cli repair` takes it away",
        }
    }
}

/// One command of the family.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    /// The id of the app (`tasks`; `hub` for hub-desktop itself); none for the starter of an app that is gone.
    pub app: Option<&'static str>,
    pub command: &'static str,
    pub program: Option<String>,
    pub starter: Starter,
}

/// What `cli status` found.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// The folder of the starters; none: the machine has no place for them.
    pub dir: Option<String>,
    /// Whether that folder is on the `PATH` of this terminal.
    pub on_path: bool,
    pub rows: Vec<Row>,
}

impl Report {
    /// Whether every command is as a repair would make it, there is nothing left of an app that is gone, and the
    /// terminal finds the folder.
    #[must_use]
    pub fn healthy(&self) -> bool {
        self.dir.is_some() && self.on_path && self.rows.iter().all(|r| r.starter == Starter::Ok)
    }

    /// The lines for the person.
    #[must_use]
    pub fn text(&self) -> String {
        let Some(dir) = &self.dir else {
            return "no folder for the commands (no LOCALAPPDATA or HOME)\n".to_owned();
        };
        let mut out = format!(
            "commands in {dir} (on the PATH of this terminal: {})\n",
            if self.on_path { "yes" } else { "no" }
        );
        if self.rows.is_empty() {
            out.push_str("  no app of the family is installed here\n");
        }
        for row in &self.rows {
            let note = row.starter.note();
            let note = if note.is_empty() {
                String::new()
            } else {
                format!("  ({note})")
            };
            out.push_str(&format!(
                "  {:<16}{:<9}{note}\n",
                row.command,
                row.starter.word()
            ));
        }
        if !self.on_path {
            out.push_str(
                "the folder is not on the PATH of this terminal: open a new terminal (or `hub-desktop cli repair` puts it on the PATH of your user)\n",
            );
        }
        out
    }
}

/// Whether `dir` is one of the folders of the `PATH` of `env` (Windows compares without case and without a
/// trailing separator).
fn on_path(env: &Env, dir: &Path) -> bool {
    let Some(path) = env.get("PATH") else {
        return false;
    };
    let plain = |p: &Path| {
        let text = p.to_string_lossy();
        let text = text.trim_end_matches(['\\', '/']);
        if cfg!(windows) {
            text.replace('/', "\\").to_ascii_lowercase()
        } else {
            text.to_owned()
        }
    };
    let want = plain(dir);
    std::env::split_paths(path).any(|p| plain(&p) == want)
}

/// The commands that are ours to take away when their app is gone: every command of the catalog, hub-desktop's own
/// excepted.
fn known_commands() -> Vec<&'static str> {
    CATALOG.iter().filter_map(command_of).collect()
}

/// How the files of one command are, against what a repair would write.
fn starter_of(env: &Env, dir: &Path, command: &str, program: &Path) -> Starter {
    let mut states = Vec::new();
    for file in cli_entry::starter_files(dir, command) {
        let expected = if file.extension().is_some_and(|x| x == "cmd") {
            cli_entry::cmd_text(command, program, env)
        } else {
            cli_entry::sh_text(command, program)
        };
        states.push(match std::fs::read(&file) {
            Err(_) => Starter::Missing,
            Ok(bytes) if expected.as_ref().is_ok_and(|e| e.as_bytes() == bytes) => Starter::Ok,
            Ok(bytes) if has_mark(&bytes) => Starter::Stale,
            Ok(_) => Starter::Foreign,
        });
    }
    if states.contains(&Starter::Foreign) {
        Starter::Foreign
    } else if states.iter().all(|s| *s == Starter::Missing) {
        Starter::Missing
    } else if states.iter().all(|s| *s == Starter::Ok) {
        Starter::Ok
    } else {
        Starter::Stale
    }
}

fn has_mark(bytes: &[u8]) -> bool {
    bytes
        .windows(cli_entry::MARK.len())
        .any(|w| w == cli_entry::MARK.as_bytes())
}

/// Look at the commands of the family against `wanted` (see [`wanted`]).
#[must_use]
pub fn status(env: &Env, wanted: &[Wanted]) -> Report {
    let Some(dir) = cli_entry::starter_dir(env) else {
        return Report {
            dir: None,
            on_path: false,
            rows: Vec::new(),
        };
    };
    let mut rows: Vec<Row> = wanted
        .iter()
        .map(|w| Row {
            app: Some(w.id),
            command: w.command,
            program: Some(w.program.display().to_string()),
            starter: starter_of(env, &dir, w.command, &w.program),
        })
        .collect();
    for command in known_commands() {
        if wanted.iter().any(|w| w.command == command) {
            continue;
        }
        let ours = cli_entry::starter_files(&dir, command)
            .iter()
            .any(|f| std::fs::read(f).is_ok_and(|b| has_mark(&b)));
        if ours {
            rows.push(Row {
                app: None,
                command,
                program: None,
                starter: Starter::Orphan,
            });
        }
    }
    Report {
        on_path: on_path(env, &dir),
        dir: Some(dir.display().to_string()),
        rows,
    }
}

/// Make the commands of `wanted` (`others_only`: not that of hub-desktop, which its own install made) and take away
/// the starters of apps that are gone. Returns the lines for the person; what fails is said in them and in the error,
/// the rest is done.
///
/// # Errors
///
/// The commands that could not be made or taken away, after all the others were.
pub fn repair(env: &Env, wanted: &[Wanted], others_only: bool) -> Result<String, String> {
    let Some(path) = path_store(env) else {
        return Ok(String::new());
    };
    let mut out = String::new();
    let mut failed = Vec::new();
    for w in wanted
        .iter()
        .filter(|w| !(others_only && w.command == OWN_COMMAND))
    {
        match cli_entry::install(w.command, &w.program, env, path) {
            Ok(text) => out.push_str(&text),
            Err(e) => failed.push(format!("{}: {e}", w.command)),
        }
    }
    for row in status(env, wanted)
        .rows
        .iter()
        .filter(|r| r.starter == Starter::Orphan)
    {
        match cli_entry::uninstall(row.command, env, path) {
            Ok(text) => out.push_str(&text),
            Err(e) => failed.push(format!("{}: {e}", row.command)),
        }
    }
    if failed.is_empty() {
        Ok(out)
    } else {
        print!("{out}");
        Err(failed.join("; "))
    }
}

/// The step of `install` after the command of hub-desktop: the commands of the other apps that are there. The install
/// is done when this fails, so it says so and does not fail.
pub async fn after_install(env: &Env) -> String {
    if path_store(env).is_none() {
        return String::new();
    }
    let found = wanted(env).await;
    let others = found.iter().filter(|w| w.command != OWN_COMMAND).count();
    match repair(env, &found, true) {
        Ok(text) if others == 0 && text.is_empty() => String::new(),
        Ok(text) => format!("commands of the other apps found here:\n{text}"),
        Err(e) => {
            let line = format!("not all commands of the other apps could be made: {e}\n");
            eprint!("{line}");
            line
        }
    }
}

/// One app of the table `apps`.
#[derive(Debug, Clone, Serialize)]
pub struct AppRow {
    pub id: &'static str,
    pub name: &'static str,
    pub installed: bool,
    pub native: bool,
    pub web: bool,
    /// `running`, `stopped`, `port_busy`; none: the app has no way to be looked at.
    pub run: Option<&'static str>,
    pub port: Option<u16>,
    /// The version of the hub that runs (an app that does not run has none here: nothing is started to ask).
    pub version: Option<String>,
    pub autostart: Option<bool>,
    /// The command of the app in the terminal; none: it has none, or it is not installed.
    pub command: Option<Starter>,
}

fn run_word(run: Run) -> &'static str {
    match run {
        Run::Running => "running",
        Run::Stopped => "stopped",
        Run::PortBusy => "port_busy",
    }
}

/// The detector the commands look through: the web folder of `config.json`, and the start with the login unless a
/// test of the program asks for no look at the machine.
fn detector_of(env: &Env) -> Detector {
    let web_root = instance::config_folder(env, &crate::IDENTITY)
        .ok()
        .and_then(|dir| crate::config::load_file(&dir).ok())
        .and_then(|file| file.app.web_root);
    let detector = Detector::new(env.clone(), web_root);
    if env.get(&crate::IDENTITY.var("NO_LOGIN_SYNC")).is_some() {
        return detector;
    }
    let reading = env.clone();
    detector.with_autostart(move |identity| {
        appkit_hub::loginstate::is_on(identity, &reading, &appkit_hub::loginstate::SystemLogin)
    })
}

async fn running_record(env: &Env, state: &AppState) -> Option<HubRecord> {
    let identity = state.entry.kit_identity()?;
    let dir = instance::config_folder(env, identity).ok()?;
    instance::running_hub(&dir, identity).await
}

/// What is on the machine, app by app (the table `apps`).
pub async fn apps(env: &Env) -> Vec<AppRow> {
    let detector = detector_of(env);
    let report = status(env, &wanted(env).await);
    let mut rows = Vec::new();
    for state in detector.apps().await.iter() {
        let record = if state.run == Some(Run::Running) {
            running_record(env, state).await
        } else {
            None
        };
        let command = command_of(state.entry).and_then(|name| {
            report
                .rows
                .iter()
                .find(|r| r.command == name && r.app == Some(state.entry.id))
                .map(|r| r.starter)
        });
        rows.push(AppRow {
            id: state.entry.id,
            name: state.entry.name,
            installed: state.installed(),
            native: state.native.is_some(),
            web: state.web.is_some(),
            run: state.run.map(run_word),
            port: record.as_ref().map(|r| r.port).or(state.port),
            version: record.map(|r| r.version),
            autostart: state.autostart,
            command,
        });
    }
    rows
}

/// The table as text, one line per app.
#[must_use]
pub fn apps_text(rows: &[AppRow]) -> String {
    let head = [
        "app",
        "installed",
        "running",
        "port",
        "autostart",
        "command",
        "version",
    ];
    let cell = |r: &AppRow| -> [String; 7] {
        let word = |o: Option<&str>| o.unwrap_or("-").to_owned();
        [
            r.id.to_owned(),
            match (r.native, r.web) {
                (true, true) => "native+web",
                (true, false) => "native",
                (false, true) => "web",
                (false, false) => "no",
            }
            .to_owned(),
            word(
                r.run
                    .map(|w| if w == "port_busy" { "port busy" } else { w }),
            ),
            r.port.map_or_else(|| "-".to_owned(), |p| p.to_string()),
            match r.autostart {
                Some(true) => "on",
                Some(false) => "off",
                None => "-",
            }
            .to_owned(),
            word(r.command.map(Starter::word)),
            word(r.version.as_deref()),
        ]
    };
    let table: Vec<[String; 7]> = rows.iter().map(cell).collect();
    let mut widths = head.map(str::len);
    for row in &table {
        for (w, c) in widths.iter_mut().zip(row) {
            *w = (*w).max(c.len());
        }
    }
    let line = |cells: Vec<&str>| {
        let mut s = String::new();
        for (c, w) in cells.iter().zip(widths) {
            s.push_str(&format!("{c:<w$}  "));
        }
        format!("{}\n", s.trim_end())
    };
    let mut out = line(head.to_vec());
    for row in &table {
        out.push_str(&line(row.iter().map(String::as_str).collect()));
    }
    out
}

fn outcome_word(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Started => "started in the background",
        Outcome::Running => "runs already",
        Outcome::Starting => "a start is on its way",
        Outcome::SetupRequired => "not set up yet: its first start is in its page (`open`)",
        Outcome::Stopping => "asked to stop",
        Outcome::NotRunning => "does not run",
        Outcome::Opened => "opened",
    }
}

fn launcher_of(env: &Env) -> (Arc<Detector>, Arc<Launcher>) {
    let detector = Arc::new(detector_of(env));
    let launcher = Arc::new(Launcher::new(
        Arc::clone(&detector),
        LaunchConfig::new(None, Browser::System),
    ));
    (detector, launcher)
}

/// `start --all`: the hubs of the apps of the kit that are installed, each in the background without a page (what
/// hub-desktop does for `start_with_hub` at the login). One that is not set up waits for its first start in its page.
/// The start of hub-desktop itself is the caller's. Returns the lines for the person.
///
/// # Errors
///
/// The apps that could not be started, after all the others were asked.
pub async fn start_all(env: &Env) -> Result<String, String> {
    let (detector, launcher) = launcher_of(env);
    let mut out = String::new();
    let mut failed = Vec::new();
    for state in detector.kit_hubs().await {
        match launcher.launch(state.entry.id, Request::Background).await {
            Ok(outcome) => {
                out.push_str(&format!("{}: {}\n", state.entry.id, outcome_word(outcome)))
            }
            Err(e) => failed.push(format!("{}: {e:?}", state.entry.id)),
        }
    }
    if failed.is_empty() {
        Ok(out)
    } else {
        print!("{out}");
        Err(failed.join("; "))
    }
}

/// `stop --all`: the hubs of the apps of the kit that run are asked to stop (`<program> stop`, the app ends its own
/// hub). The stop of hub-desktop itself is the caller's. Returns the lines for the person.
///
/// # Errors
///
/// The apps that could not be asked, after all the others were.
pub async fn stop_all(env: &Env) -> Result<String, String> {
    let (detector, launcher) = launcher_of(env);
    let mut out = String::new();
    let mut failed = Vec::new();
    for state in detector.kit_hubs().await {
        match launcher.stop(state.entry.id).await {
            Ok(outcome) => {
                out.push_str(&format!("{}: {}\n", state.entry.id, outcome_word(outcome)))
            }
            Err(e) => failed.push(format!("{}: {e:?}", state.entry.id)),
        }
    }
    if failed.is_empty() {
        Ok(out)
    } else {
        print!("{out}");
        Err(failed.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_apps_that_have_a_command_are_those_of_the_kit_and_the_two_setups() {
        let names: Vec<&str> = CATALOG.iter().filter_map(command_of).collect();
        for expected in [
            "tasks-desktop",
            "rules-desktop",
            "testing-desktop",
            "docmap-desktop",
            "ai-desktop",
        ] {
            assert!(names.contains(&expected), "{expected}: {names:?}");
        }
        // hub-desktop itself is not in the catalog; an app of somebody else has no command.
        assert!(!names.contains(&OWN_COMMAND));
        assert!(!names.contains(&"streamtap"), "{names:?}");
    }

    /// The detection resolves links, and Windows then names a drive path `\\?\C:\...`: a starter must not carry that.
    #[test]
    fn the_verbatim_prefix_of_a_drive_path_is_taken_off_and_nothing_else() {
        let plain = |s: &str| plain_path(PathBuf::from(s)).display().to_string();
        assert_eq!(
            plain(r"\\?\C:\Users\x\tasks-hub.exe"),
            r"C:\Users\x\tasks-hub.exe"
        );
        assert_eq!(plain(r"\\?\d:\a"), r"d:\a");
        // A share, a device, a path that is plain already, and a prefix that holds no drive stay as they are.
        assert_eq!(
            plain(r"\\?\UNC\server\share\x.exe"),
            r"\\?\UNC\server\share\x.exe"
        );
        assert_eq!(plain(r"\\.\COM1"), r"\\.\COM1");
        assert_eq!(plain(r"C:\Users\x.exe"), r"C:\Users\x.exe");
        assert_eq!(plain("/home/x/tasks-hub"), "/home/x/tasks-hub");
        assert_eq!(plain(r"\\?\C:"), r"\\?\C:");
    }

    #[test]
    fn the_folder_is_on_the_path_as_a_shell_compares_it() {
        let dir = if cfg!(windows) {
            PathBuf::from(r"C:\Users\x\AppData\Local\appkit-family\bin")
        } else {
            PathBuf::from("/home/x/.local/bin")
        };
        let joined = |parts: &[&Path]| std::env::join_paths(parts).unwrap();
        let other = if cfg!(windows) {
            PathBuf::from(r"C:\other")
        } else {
            PathBuf::from("/other")
        };
        let with = Env::from_pairs([("PATH", joined(&[&other, &dir]))]);
        assert!(on_path(&with, &dir));
        let without = Env::from_pairs([("PATH", joined(&[&other]))]);
        assert!(!on_path(&without, &dir));
        assert!(!on_path(&Env::from_pairs([("X", "y")]), &dir));
        if cfg!(windows) {
            let upper = Env::from_pairs([(
                "PATH",
                std::ffi::OsString::from(format!("{}\\", dir.display().to_string().to_uppercase())),
            )]);
            assert!(on_path(&upper, &dir));
        }
    }

    #[test]
    fn the_table_has_one_line_per_app_with_aligned_columns() {
        let row = |id: &'static str, native: bool, run: Option<&'static str>, command| AppRow {
            id,
            name: "n",
            installed: native,
            native,
            web: false,
            run,
            port: run.filter(|r| *r == "running").map(|_| 17615),
            version: run.filter(|r| *r == "running").map(|_| "1.2.3".to_owned()),
            autostart: native.then_some(false),
            command,
        };
        let text = apps_text(&[
            row("tasks", true, Some("running"), Some(Starter::Ok)),
            row("rules", false, None, None),
            row("ai", true, Some("port_busy"), Some(Starter::Missing)),
        ]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "{text}");
        assert!(
            lines[0].starts_with("app ") && lines[0].contains("installed  running"),
            "{text}"
        );
        assert!(lines[1].starts_with("tasks"), "{text}");
        assert!(
            lines[1].contains("running")
                && lines[1].contains("17615")
                && lines[1].contains("1.2.3")
        );
        assert!(lines[2].contains("no") && lines[2].contains('-'), "{text}");
        assert!(
            lines[3].contains("port busy") && lines[3].contains("missing"),
            "{text}"
        );
        // The columns start at the same place in every line.
        let at = |l: &str, word: &str| l.find(word).unwrap();
        assert_eq!(at(lines[0], "installed"), at(lines[1], "native"));
    }

    #[test]
    fn a_report_is_healthy_only_when_everything_is_ok_and_the_terminal_finds_the_folder() {
        let row = |starter| Row {
            app: Some("tasks"),
            command: "tasks-desktop",
            program: None,
            starter,
        };
        let report = |on_path, starters: &[Starter]| Report {
            dir: Some("/d".to_owned()),
            on_path,
            rows: starters.iter().map(|s| row(*s)).collect(),
        };
        assert!(report(true, &[Starter::Ok]).healthy());
        assert!(report(true, &[]).healthy());
        assert!(!report(false, &[Starter::Ok]).healthy());
        for bad in [
            Starter::Missing,
            Starter::Stale,
            Starter::Foreign,
            Starter::Orphan,
        ] {
            assert!(!report(true, &[Starter::Ok, bad]).healthy(), "{bad:?}");
        }
        let none = Report {
            dir: None,
            on_path: false,
            rows: Vec::new(),
        };
        assert!(!none.healthy());
        assert!(none.text().contains("no folder"));
    }
}

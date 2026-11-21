//! The self-hosted runners of this machine: which of them runs, and a runner that is not running while a push of its
//! repository waits for it. Plan `ALL/commit-fluss-wartezeit-senken-agenten-an-die-ci-anbinden`, task
//! `hub-desktop/runner-zustand-im-tray-und-runner-beim-push-starten`.
//!
//! The one source of truth is `runners.ps1` of the nvim config (`$NVIM_CONFIG_DIR/scripts/runners.ps1`): `status all`
//! says per runner whether its process runs and what GitHub says, `ensure <repo>` starts one (detached, with the
//! guard against foreign code) and waits until GitHub sees it. The hub runs it through `pwsh` by argv, the one
//! exception to "never PowerShell" of the updates: a second way to look at the runners would drift from the one
//! the person and `:MyPlugins runner` use.
//!
//! One round (in the rhythm of the CI watcher, `ci_watch_interval_min`): `status all`; for each runner that is not
//! running, `testing ci remote --json $REPOS_DIR/<repo>`: a run in `waiting_runner` is a push that waits for it.
//! Such a runner is started without asking only with `runners.start_when_waiting: true` in `config.json`; otherwise
//! the icon says so and a click starts it. Never on the workstation (nor on a machine whose role is unclear), and
//! never in a hub of a test (`ciwatch::allowed`).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::ciwatch::{self, Answer, Ask, Call, Programs};
use crate::config::Env;
use crate::machine::Role;

/// `status all` asks GitHub once per runner (several at a time); this is plenty.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(120);
/// `ensure` runs the tool check and waits up to 90 s for GitHub.
pub const ENSURE_TIMEOUT: Duration = Duration::from_secs(240);
/// Why the runners are not looked at on this machine.
pub const NOT_HERE: &str = "the runners are looked at on the private machine only";

/// How `runners.ps1` is run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runners {
    pub pwsh: PathBuf,
    pub script: PathBuf,
}

impl Runners {
    /// `pwsh` on the `PATH` and `$NVIM_CONFIG_DIR/scripts/runners.ps1`, on the private machine only.
    ///
    /// # Errors
    ///
    /// What is missing, or [`NOT_HERE`], as a fixed sentence.
    pub fn find(env: &Env, role: Role) -> Result<Self, &'static str> {
        if role != Role::Private {
            return Err(NOT_HERE);
        }
        let pwsh = crate::update::on_path(env, "pwsh").ok_or("pwsh is not on the PATH")?;
        let script = env
            .path("NVIM_CONFIG_DIR")
            .and_then(crate::paths::usable_root)
            .map(|d| d.join("scripts").join("runners.ps1"))
            .filter(|f| f.is_file())
            .ok_or("no scripts/runners.ps1 in NVIM_CONFIG_DIR")?;
        Ok(Self { pwsh, script })
    }

    fn call(&self, args: &[&str]) -> Call {
        let mut all: Vec<OsString> = ["-NoProfile", "-NonInteractive", "-File"]
            .iter()
            .map(OsString::from)
            .collect();
        all.push(self.script.as_os_str().to_owned());
        all.extend(args.iter().map(OsString::from));
        let cwd = self
            .script
            .parent()
            .and_then(std::path::Path::parent)
            .map_or_else(|| self.script.clone(), std::path::Path::to_path_buf);
        Call {
            program: self.pwsh.clone(),
            args: all,
            cwd,
        }
    }

    /// `runners.ps1 status all`.
    #[must_use]
    pub fn status_call(&self) -> Call {
        self.call(&["status", "all"])
    }

    /// `runners.ps1 ensure <repo>`, for a name `status` gave.
    #[must_use]
    pub fn ensure_call(&self, repo: &str) -> Call {
        self.call(&["ensure", repo])
    }
}

/// One runner as `status` tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runner {
    pub repo: String,
    /// Its process runs on this machine.
    pub running: bool,
    /// What GitHub says: `online`, `offline`, `missing`, `unknown` (anything else is `unknown`).
    pub github: &'static str,
    pub busy: bool,
}

/// The runners of a `status all` answer. A machine without runners (exit 2, "no runners on this machine") has none.
///
/// # Errors
///
/// The script ran out of time or gave no line of status.
pub fn parse_status(answer: &Answer) -> Result<Vec<Runner>, &'static str> {
    let text = String::from_utf8_lossy(&answer.stdout);
    if answer.code.is_none() {
        return Err("runners.ps1 did not end in time");
    }
    if answer.code == Some(2)
        && text
            .lines()
            .any(|l| l.starts_with("no runners on this machine"))
    {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let (Some(repo), Some(local), Some(github)) = (
            words.first(),
            words.get(1).and_then(|w| w.strip_prefix("local=")),
            words.get(2).and_then(|w| w.strip_prefix("github=")),
        ) else {
            continue;
        };
        if !ciwatch::plain_repo(repo) || !matches!(local, "running" | "stopped") {
            continue;
        }
        out.push(Runner {
            repo: (*repo).to_owned(),
            running: local == "running",
            github: match github {
                "online" => "online",
                "offline" => "offline",
                "missing" => "missing",
                _ => "unknown",
            },
            busy: words.get(3) == Some(&"busy"),
        });
    }
    // Exit 1 is "not every runner is in the wanted state": the lines say which. Anything else without a line is no
    // status at all.
    if out.is_empty() && !matches!(answer.code, Some(0 | 1)) {
        return Err("runners.ps1 gave no status");
    }
    Ok(out)
}

/// How a start of a runner ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    /// `ensure` ended with 0: it runs and GitHub sees it.
    Online,
    /// It ended otherwise (or not in time): the log has the reason.
    Failed,
}

/// What the runners are at after a round.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Board {
    pub runners: Vec<Runner>,
    /// The repositories whose runner is not running while a push waits for it.
    pub waiting: BTreeSet<String>,
    /// The starts of this round (without asking) or of a click.
    pub started: BTreeMap<String, Started>,
    /// What kept the round from the runners (a fixed sentence).
    pub problem: Option<&'static str>,
}

fn status(ask: &dyn Ask, runners: &Runners) -> Result<Vec<Runner>, &'static str> {
    match ask.ask(&runners.status_call(), STATUS_TIMEOUT) {
        Ok(answer) => parse_status(&answer),
        Err(_) => Err("pwsh did not start"),
    }
}

fn ensure(ask: &dyn Ask, runners: &Runners, repo: &str) -> Started {
    match ask.ask(&runners.ensure_call(repo), ENSURE_TIMEOUT) {
        Ok(answer) if answer.code == Some(0) => Started::Online,
        _ => Started::Failed,
    }
}

/// One round: the status, the pushes that wait for a runner that is not running, and (with `auto`) their start.
/// `programs` asks the hosted CI (none: the waiting pushes are not seen, the status still is).
pub fn round(ask: &dyn Ask, runners: &Runners, programs: Option<&Programs>, auto: bool) -> Board {
    let mut board = Board::default();
    match status(ask, runners) {
        Ok(list) => board.runners = list,
        Err(why) => {
            board.problem = Some(why);
            return board;
        }
    }
    if let Some(programs) = programs {
        for runner in board.runners.iter().filter(|r| !r.running) {
            let dir = programs.repos.join(&runner.repo);
            if !dir.join(".git").exists() {
                continue;
            }
            let waits = ask
                .ask(&programs.remote(&dir), ciwatch::ASK_TIMEOUT)
                .ok()
                .and_then(|a| ciwatch::parse_remote(&a.stdout).ok())
                .is_some_and(|r| r.waiting_runner);
            if waits {
                board.waiting.insert(runner.repo.clone());
            }
        }
    }
    if auto && !board.waiting.is_empty() {
        for repo in board.waiting.clone() {
            board
                .started
                .insert(repo.clone(), ensure(ask, runners, &repo));
        }
        // What runs now.
        if let Ok(list) = status(ask, runners) {
            board.runners = list;
            board
                .waiting
                .retain(|repo| board.runners.iter().any(|r| &r.repo == repo && !r.running));
        }
    }
    board
}

/// The runners of the hub: how to ask, what the last round saw, and the starts on a click.
pub struct RunnerWatch {
    ask: Arc<dyn Ask>,
    runners: Runners,
    programs: Option<Programs>,
    board: Mutex<Option<Board>>,
    /// The runners a start is on its way for (a second click does nothing).
    starting: Mutex<BTreeSet<String>>,
}

impl RunnerWatch {
    #[must_use]
    pub fn new(ask: Arc<dyn Ask>, runners: Runners, programs: Option<Programs>) -> Self {
        Self {
            ask,
            runners,
            programs,
            board: Mutex::new(None),
            starting: Mutex::new(BTreeSet::new()),
        }
    }

    /// What the last round saw (none: no round has ended).
    #[must_use]
    pub fn board(&self) -> Option<Board> {
        self.board.lock().ok().and_then(|b| b.clone())
    }

    /// The runners a start is on its way for.
    #[must_use]
    pub fn starting(&self) -> BTreeSet<String> {
        self.starting.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Run a round now (it waits for programs: a thread of the blocking pool) and keep what it saw.
    pub fn refresh(&self, auto: bool) -> Board {
        let board = round(
            self.ask.as_ref(),
            &self.runners,
            self.programs.as_ref(),
            auto,
        );
        for (repo, started) in &board.started {
            say_start(repo, *started);
        }
        if let Some(why) = board.problem {
            eprintln!("runners: {why}");
        }
        if let Ok(mut kept) = self.board.lock() {
            *kept = Some(board.clone());
        }
        board
    }

    /// Start the runner of `repo` on a click: only a runner the last round saw, that does not run, and that no
    /// start is on its way for. It waits for `ensure` (a thread of the blocking pool), then looks again.
    ///
    /// # Errors
    ///
    /// Why nothing was started (a fixed sentence).
    pub fn start(&self, repo: &str) -> Result<Started, &'static str> {
        let known = self
            .board()
            .is_some_and(|b| b.runners.iter().any(|r| r.repo == repo && !r.running));
        if !known {
            return Err("no runner of that name is stopped here");
        }
        {
            let mut starting = self.starting.lock().map_err(|_| "the runners are locked")?;
            if !starting.insert(repo.to_owned()) {
                return Err("a start of that runner is on its way");
            }
        }
        let started = ensure(self.ask.as_ref(), &self.runners, repo);
        say_start(repo, started);
        let mut board = self.refresh(false);
        board.started.insert(repo.to_owned(), started);
        if let Ok(mut kept) = self.board.lock() {
            *kept = Some(board);
        }
        if let Ok(mut starting) = self.starting.lock() {
            starting.remove(repo);
        }
        Ok(started)
    }
}

fn say_start(repo: &str, started: Started) {
    match started {
        Started::Online => println!("runners: {repo} started, GitHub sees it"),
        Started::Failed => eprintln!(
            "runners: the start of {repo} did not end well (runners.ps1 status {repo} says more)"
        ),
    }
}

/// What the loop reads before every round: the minutes between two rounds and whether a waiting push starts its
/// runner without asking.
pub type Settings = Arc<dyn Fn() -> (u64, bool) + Send + Sync>;

/// A round `first` after the start, then one whenever the interval (`minute` times the minutes of the settings)
/// has passed, the settings read every `tick`, until the task is ended.
pub async fn watch(
    watch: Arc<RunnerWatch>,
    settings: Settings,
    first: Duration,
    tick: Duration,
    minute: Duration,
) {
    tokio::time::sleep(first).await;
    let mut last: Option<tokio::time::Instant> = None;
    loop {
        let (minutes, auto) = settings();
        let every = minute.saturating_mul(u32::try_from(minutes).unwrap_or(u32::MAX));
        if last.is_none_or(|t| t.elapsed() >= every) {
            let w = Arc::clone(&watch);
            let _ = tokio::task::spawn_blocking(move || w.refresh(auto)).await;
            last = Some(tokio::time::Instant::now());
        }
        tokio::time::sleep(tick).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(code: i32, text: &str) -> Answer {
        Answer {
            code: Some(code),
            stdout: text.as_bytes().to_vec(),
        }
    }

    #[test]
    fn the_status_lines_of_runners_ps1_are_read() {
        let text = "hub-desktop    local=running  github=online busy\r\n\
                    tasks.nvim     local=stopped  github=offline\n\
                    env: tasks.nvim lacks CARGO_BUILD_JOBS\n\
                    ../evil        local=stopped  github=online\n\
                    weird          local=maybe    github=online\n\
                    rules.nvim     local=stopped  github=whatever\n";
        let got = parse_status(&answer(1, text)).unwrap();
        assert_eq!(
            got,
            [
                Runner {
                    repo: "hub-desktop".into(),
                    running: true,
                    github: "online",
                    busy: true
                },
                Runner {
                    repo: "tasks.nvim".into(),
                    running: false,
                    github: "offline",
                    busy: false
                },
                Runner {
                    repo: "rules.nvim".into(),
                    running: false,
                    github: "unknown",
                    busy: false
                },
            ]
        );
    }

    #[test]
    fn a_machine_without_runners_has_none_and_a_broken_answer_is_a_problem() {
        let none = "no runners on this machine: C:\\Users\\x\\actions-runners does not exist\n";
        assert_eq!(parse_status(&answer(2, none)), Ok(Vec::new()));
        assert_eq!(parse_status(&answer(0, "")), Ok(Vec::new()));
        assert_eq!(
            parse_status(&answer(2, "unknown runner: x (no run.cmd)\n")),
            Err("runners.ps1 gave no status")
        );
        assert_eq!(
            parse_status(&Answer {
                code: None,
                stdout: Vec::new()
            }),
            Err("runners.ps1 did not end in time")
        );
    }
}

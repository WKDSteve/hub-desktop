//! The CI watcher: the verdict of the hosted CI goes into the finished tasks whose `ci` is still `pending` (or
//! `red`), through the tasks command, never into a file. Plan `ALL/commit-fluss-wartezeit-senken-agenten-an-die-ci-anbinden`,
//! task `hub-desktop/ci-beobachter-schreibt-urteil-in-die-task`.
//!
//! One round:
//! 1. `tasks list --done --ci=pending,red --format=ids` (one quick call; nothing listed ends the round), then
//!    `tasks call finished` (the contract of tasks.nvim, newest first, page by page until every listed id is found,
//!    at most [`MAX_PAGES`]): their `commits` (`repo@sha`) and `ci`.
//! 2. Per repository of their commits (`$REPOS_DIR/<repo>`, a git checkout): `testing ci remote --json <dir>`, the
//!    CI run of the head of origin/main (testing.nvim, read-only).
//! 3. Per commit: is it part of that head (`git merge-base --is-ancestor`)? Then it is green when the run is
//!    `success` on every system, red when the run ended `failure` or `rejected`; anything else leaves it pending.
//!    A run `queued` (or waiting for its runner) for longer than [`QUEUED_HINT_AFTER`] is a hint, never red.
//! 4. A task is green when every commit is, red as soon as one is (a red task turns green again once a later head
//!    that contains it is green): `tasks set <id> ci=<word> --by=hub-desktop`.
//!
//! On unless `config.json` says `ci_watch: false`, never in a hub of a test ([`allowed`]). What it is at is
//! [`Watcher::view`] (`GET /api/hub/ci`, the section "Testing" of the page) and [`Watcher::hint_count`] (the icon).
//!
//! The state of each system is `overall` of `testing ci remote`, never the raw run of the head: for a repository
//! that lands through `testing ci land` the run of `main` has no legs (`already-verified`), the systems ran on the
//! check branch `ci/**`, and testing.nvim reads them from there.
//!
//! The first round runs [`FIRST_ROUND`] after the start, the next ones `ci_watch_interval_min` minutes apart (5,
//! 1 to 60); the settings are read again every [`TICK`], so a new interval or the switch counts without a new start.
//!
//! Programs are started by argv only, from what [`Programs::find`] found (no path from a request or from
//! `config.json`): nvim on the `PATH` with the scripts of the checkouts of tasks.nvim and testing.nvim below
//! `$REPOS_DIR`, git on the `PATH`, the vault of `TASKS_VAULT`. Every word of a foreign document that reaches a
//! decision is one of a fixed list.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use appkit_core::child::{self, Environment, Leftovers, Start};
use serde_json::Value;

use crate::config::Env;

/// How many pages of 100 finished tasks a round reads at most to find the listed ones.
pub const MAX_PAGES: u64 = 10;
/// The first round runs this long after the hub started (its start stays quick), the next ones [`EVERY`] apart.
pub const FIRST_ROUND: Duration = Duration::from_secs(60);
/// The minutes between two rounds (`ci_watch_interval_min` of `config.json`): the default and the bounds.
pub const INTERVAL_DEFAULT_MIN: u64 = 5;
pub const INTERVAL_MIN: u64 = 1;
pub const INTERVAL_MAX: u64 = 60;
/// How often the loop reads the settings again: a shorter interval or a switch counts within this time.
pub const TICK: Duration = Duration::from_secs(30);
/// One minute of the interval (the test gives a shorter one).
pub const MINUTE: Duration = Duration::from_secs(60);
/// A run that has waited this long for a runner is a hint.
pub const QUEUED_HINT_AFTER: Duration = Duration::from_secs(30 * 60);
/// How long one call may take.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(90);
/// How much one answer may be.
pub const MAX_ANSWER: usize = 8 * 1024 * 1024;
/// Who writes the verdict, as the task's history names it.
pub const BY: &str = "hub-desktop";

/// One program call: an argv in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
}

/// How a call ended: its exit code (`None`: ended by its time) and stdout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
}

/// Runs the calls of a round. The program has [`SystemAsk`]; a test has one that answers.
pub trait Ask: Send + Sync {
    /// Run `call` to its end, or end it after `timeout`.
    ///
    /// # Errors
    ///
    /// The program could not be started or its output not read.
    fn ask(&self, call: &Call, timeout: Duration) -> Result<Answer, String>;
}

/// The programs of the machine through `appkit_core::child` (an argv, stdin empty, no window, bound to the hub,
/// named in its log), nobody answering a question.
pub struct SystemAsk;

impl Ask for SystemAsk {
    fn ask(&self, call: &Call, timeout: Duration) -> Result<Answer, String> {
        let run = child::Run {
            start: Start::new("hub-desktop ci watch", &call.program)
                .args(&call.args)
                .cwd(&call.cwd)
                .env(Environment::Inherited {
                    set: vec![
                        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
                        ("GH_PROMPT_DISABLED".into(), "1".into()),
                    ],
                    remove: vec!["PSModulePath".into()],
                }),
            stdin: Vec::new(),
            timeout,
            max_stdout: MAX_ANSWER,
            max_stderr: MAX_ANSWER,
            leftovers: Leftovers::End,
        };
        let output = crate::update::block_on(child::run(run))?
            .map_err(|e| format!("{} did not answer: {e}", file_name(&call.program)))?;
        Ok(Answer {
            code: output.code,
            stdout: output.stdout,
        })
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What the watcher needs, found once from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Programs {
    pub nvim: PathBuf,
    pub git: PathBuf,
    pub tasks_lua: PathBuf,
    pub testing_lua: PathBuf,
    pub vault: PathBuf,
    pub repos: PathBuf,
}

impl Programs {
    /// nvim and git on the `PATH` (absolute folders), `$REPOS_DIR/tasks.nvim/scripts/tasks.lua` and
    /// `$REPOS_DIR/testing.nvim/scripts/testing.lua`, the vault of `TASKS_VAULT`.
    ///
    /// # Errors
    ///
    /// What is missing, as a fixed sentence.
    pub fn find(env: &Env) -> Result<Self, &'static str> {
        let nvim = crate::update::on_path(env, "nvim").ok_or("nvim is not on the PATH")?;
        let git = crate::update::on_path(env, "git").ok_or("git is not on the PATH")?;
        let repos = env
            .path("REPOS_DIR")
            .and_then(crate::paths::usable_root)
            .filter(|d| d.is_dir())
            .ok_or("REPOS_DIR names no folder")?;
        let tasks_lua = repos.join("tasks.nvim").join("scripts").join("tasks.lua");
        if !tasks_lua.is_file() {
            return Err("no checkout of tasks.nvim in REPOS_DIR");
        }
        let testing_lua = repos
            .join("testing.nvim")
            .join("scripts")
            .join("testing.lua");
        if !testing_lua.is_file() {
            return Err("no checkout of testing.nvim in REPOS_DIR");
        }
        let vault = env
            .path("TASKS_VAULT")
            .and_then(crate::paths::usable_root)
            .filter(|d| d.is_dir())
            .ok_or("TASKS_VAULT names no folder")?;
        Ok(Self {
            nvim,
            git,
            tasks_lua,
            testing_lua,
            vault,
            repos,
        })
    }

    fn nvim_call(&self, script: &Path, args: &[OsString], cwd: &Path) -> Call {
        let mut all: Vec<OsString> = ["-n", "-i", "NONE", "--headless", "-u", "NONE", "-l"]
            .iter()
            .map(OsString::from)
            .collect();
        all.push(script.as_os_str().to_owned());
        all.extend(args.iter().cloned());
        Call {
            program: self.nvim.clone(),
            args: all,
            cwd: cwd.to_path_buf(),
        }
    }

    fn tasks(&self, args: &[&str]) -> Call {
        let mut all = vec![OsString::from(format!("--vault={}", self.vault.display()))];
        all.extend(args.iter().map(OsString::from));
        self.nvim_call(&self.tasks_lua, &all, &self.vault)
    }

    fn remote(&self, dir: &Path) -> Call {
        let args = [
            OsString::from("ci"),
            "remote".into(),
            "--json".into(),
            dir.as_os_str().to_owned(),
        ];
        self.nvim_call(&self.testing_lua, &args, dir)
    }

    fn git(&self, dir: &Path, args: &[&str]) -> Call {
        let mut all = vec![OsString::from("-C"), dir.as_os_str().to_owned()];
        all.extend(args.iter().map(OsString::from));
        Call {
            program: self.git.clone(),
            args: all,
            cwd: dir.to_path_buf(),
        }
    }
}

/// A verdict the watcher writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    Green,
    Red,
}

impl Verdict {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Red => "red",
        }
    }
}

/// A finished task the watcher looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    pub id: String,
    /// `pending` or `red` (what the task says now).
    pub ci: String,
    pub commits: Vec<(String, String)>,
}

/// A repository name of `repo@sha`: letters, digits, `.`, `_`, `-`, not `.` or `..`.
fn plain_repo(repo: &str) -> bool {
    !repo.is_empty()
        && repo.len() <= 100
        && repo != "."
        && repo != ".."
        && repo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn hex(text: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A task id of the vault: `<area>/<slug>`, plain words, no way out of the vault.
fn plain_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && *p != "."
                && *p != ".."
                && !p.starts_with('-')
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

/// The finished tasks of a `tasks.finished` document whose `ci` is `pending` or `red` and that name commits.
///
/// # Errors
///
/// Not such a document.
pub fn parse_finished(text: &[u8]) -> Result<(Vec<Watched>, Option<u64>), &'static str> {
    let doc: Value = serde_json::from_slice(text).map_err(|_| "the tasks command gave no JSON")?;
    if doc.get("kind").and_then(Value::as_str) != Some("tasks.finished") {
        return Err("the tasks command gave no tasks.finished document");
    }
    let mut out = Vec::new();
    for item in doc
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| plain_id(id))
        else {
            continue;
        };
        let Some(ci) = item
            .get("ci")
            .and_then(Value::as_str)
            .filter(|ci| matches!(*ci, "pending" | "red"))
        else {
            continue;
        };
        let commits: Vec<(String, String)> = item
            .get("commits")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|c| c.split_once('@'))
            .filter(|(repo, sha)| plain_repo(repo) && hex(sha, 7, 40))
            .map(|(repo, sha)| (repo.to_owned(), sha.to_ascii_lowercase()))
            .collect();
        if commits.is_empty() {
            continue;
        }
        out.push(Watched {
            id: id.to_owned(),
            ci: ci.to_owned(),
            commits,
        });
    }
    let next = doc.get("next_offset").and_then(Value::as_u64);
    Ok((out, next))
}

/// The ids of `tasks list ... --format=ids`, one a line; a line that is no plain id is left out.
#[must_use]
pub fn parse_ids(text: &[u8]) -> BTreeSet<String> {
    String::from_utf8_lossy(text)
        .lines()
        .map(str::trim)
        .filter(|id| plain_id(id))
        .map(str::to_owned)
        .collect()
}

/// The CI run of the head of origin/main, from `testing-ci-remote/1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub sha: String,
    pub overall: State,
    /// When the run was created (seconds since 1970), when known.
    pub created: Option<u64>,
}

/// The state of a run, as far as the watcher tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// `success`: every job of every system.
    Success,
    /// `failure`, `rejected`: done and red.
    Red,
    /// `queued`, `waiting_runner`, `no_runner`: waiting for a runner.
    Waiting,
    /// `running`, `cancelled` (a newer run took over), `skipped`, `no_run`, `unknown`, anything else.
    Open,
    /// `no_github`: there is no hosted CI.
    NoGithub,
}

impl State {
    fn of(word: &str) -> Self {
        match word {
            "success" => Self::Success,
            "failure" | "rejected" => Self::Red,
            "queued" | "waiting_runner" | "no_runner" => Self::Waiting,
            "no_github" => Self::NoGithub,
            _ => Self::Open,
        }
    }
}

/// The first repository of a `testing-ci-remote/1` document.
///
/// # Errors
///
/// Not such a document, or no commit in it.
pub fn parse_remote(text: &[u8]) -> Result<Remote, &'static str> {
    let doc: Value = serde_json::from_slice(text).map_err(|_| "testing gave no JSON")?;
    if doc.get("schema").and_then(Value::as_str) != Some("testing-ci-remote/1") {
        return Err("testing gave no testing-ci-remote/1 document");
    }
    let repo = doc
        .get("repos")
        .and_then(Value::as_array)
        .and_then(|r| r.first())
        .ok_or("the document names no repository")?;
    let overall = State::of(repo.get("overall").and_then(Value::as_str).unwrap_or(""));
    let sha = repo.get("sha").and_then(Value::as_str).unwrap_or("");
    if overall == State::NoGithub {
        return Ok(Remote {
            sha: String::new(),
            overall,
            created: None,
        });
    }
    if !hex(sha, 40, 40) {
        return Err("the document names no commit of origin/main");
    }
    let created = repo
        .get("run")
        .and_then(|r| r.get("created_at"))
        .and_then(Value::as_str)
        .and_then(parse_utc);
    Ok(Remote {
        sha: sha.to_ascii_lowercase(),
        overall,
        created,
    })
}

/// Days since 1970-01-01 of a date of the proleptic Gregorian calendar (H. Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DDTHH:MM:SSZ` as seconds since 1970.
fn parse_utc(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| text.get(r)?.parse::<i64>().ok();
    let days = days_from_civil(num(0..4)?, num(5..7)?, num(8..10)?);
    let secs = days * 86_400 + num(11..13)? * 3600 + num(14..16)? * 60 + num(17..19)?;
    u64::try_from(secs).ok()
}

/// What one commit is, for its task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitState {
    Green,
    Red,
    Pending,
}

/// A hint of a round: something a person may want to know, never a verdict.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hint {
    /// The run of origin/main has waited for a runner for this many minutes.
    Queued { repo: String, minutes: u64 },
    /// The checkout does not know the head of origin/main that GitHub names (it was not fetched).
    Behind { repo: String },
    /// No checkout of the repository below `REPOS_DIR`.
    NoCheckout { repo: String },
    /// The repository has no hosted CI (its origin is not on GitHub).
    NoGithub { repo: String },
    /// A call gave no usable answer (a fixed sentence).
    NoAnswer { repo: String, why: &'static str },
}

/// The outcome of a round.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Round {
    /// The verdicts written, by task id.
    pub written: BTreeMap<String, Verdict>,
    /// The verdicts the tasks command refused to write.
    pub not_written: BTreeMap<String, Verdict>,
    pub hints: BTreeSet<Hint>,
    /// A step that kept the round from looking at the tasks at all (a fixed sentence).
    pub problem: Option<&'static str>,
}

/// The verdict of a task from the states of its commits: red as soon as one is red, green when all are green.
#[must_use]
pub fn task_verdict(states: &[CommitState]) -> Option<Verdict> {
    if states.contains(&CommitState::Red) {
        Some(Verdict::Red)
    } else if !states.is_empty() && states.iter().all(|s| *s == CommitState::Green) {
        Some(Verdict::Green)
    } else {
        None
    }
}

/// What a repository's head says about the commits.
enum Head {
    Known(Remote),
    Unknown,
}

/// One round: read, judge, write (see the module).
pub fn round(ask: &dyn Ask, programs: &Programs, now: SystemTime) -> Round {
    let mut out = Round::default();
    // Which finished tasks wait for a verdict (or are red): one quick call, mostly with an empty answer.
    let listed = match ask.ask(
        &programs.tasks(&["list", "--done", "--ci=pending,red", "--format=ids"]),
        ASK_TIMEOUT,
    ) {
        Ok(answer) if answer.code == Some(0) => parse_ids(&answer.stdout),
        Ok(_) => {
            out.problem = Some("the tasks command failed");
            return out;
        }
        Err(_) => {
            out.problem = Some("the tasks command did not start");
            return out;
        }
    };
    if listed.is_empty() {
        return out;
    }
    // Their commits, from the contract: newest first, until every listed id is found.
    let mut watched = Vec::new();
    let mut offset = 0u64;
    for _ in 0..MAX_PAGES {
        let params = format!(r#"{{"limit":100,"offset":{offset}}}"#);
        let call = programs.tasks(&["call", "finished", &format!("--params={params}")]);
        let page = match ask.ask(&call, ASK_TIMEOUT) {
            Ok(answer) if answer.code == Some(0) => parse_finished(&answer.stdout),
            Ok(_) => Err("the tasks command failed"),
            Err(_) => Err("the tasks command did not start"),
        };
        match page {
            Ok((items, next)) => {
                watched.extend(items.into_iter().filter(|t| listed.contains(&t.id)));
                if watched.len() >= listed.len() {
                    break;
                }
                match next {
                    Some(n) if n > offset => offset = n,
                    _ => break,
                }
            }
            Err(why) => {
                out.problem = Some(why);
                return out;
            }
        }
    }
    if watched.is_empty() {
        return out;
    }
    let mut heads: BTreeMap<String, Head> = BTreeMap::new();
    for task in &watched {
        let mut states = Vec::new();
        for (repo, sha) in &task.commits {
            let head = heads
                .entry(repo.clone())
                .or_insert_with(|| ask_head(ask, programs, repo, now, &mut out.hints));
            states.push(match head {
                Head::Known(remote) => judge(ask, programs, repo, sha, remote, &mut out.hints),
                Head::Unknown => CommitState::Pending,
            });
        }
        let Some(verdict) = task_verdict(&states) else {
            continue;
        };
        // A red task gets red again only from a new red: it stays as it is; it turns green.
        if task.ci == verdict.word() {
            continue;
        }
        let word = format!("ci={}", verdict.word());
        let by = format!("--by={BY}");
        let call = programs.tasks(&["set", &task.id, &word, &by]);
        match ask.ask(&call, ASK_TIMEOUT) {
            Ok(answer) if answer.code == Some(0) => {
                out.written.insert(task.id.clone(), verdict);
            }
            _ => {
                out.not_written.insert(task.id.clone(), verdict);
            }
        }
    }
    out
}

/// Why the watcher does not run (a fixed sentence the page and the console show).
pub const OFF_BY_SETTINGS: &str = "config.json turns it off (ci_watch: false)";
/// A hub of a test (a config folder of its own, no browser, no login entries) never writes into a vault.
pub const OFF_IN_TESTS: &str = "the hub runs for a test";

/// The variables (of hub-desktop) a test sets and a person's hub does not: with any of them set the watcher never
/// runs, whatever `config.json` says, so that a test that starts the real program with the environment of the
/// person (`REPOS_DIR`, `TASKS_VAULT`) cannot write into the person's vault.
#[must_use]
pub fn test_markers() -> [String; 3] {
    ["CONFIG_DIR", "NO_LAUNCH", "NO_LOGIN_SYNC"].map(|v| crate::IDENTITY.var(v))
}

/// Whether the watcher may run at all: never in a hub of a test, whatever `config.json` says. (Whether it is
/// switched on is read before every round: [`Schedule::on`].)
///
/// # Errors
///
/// [`OFF_IN_TESTS`].
pub fn allowed(env: &Env) -> Result<(), &'static str> {
    if test_markers().iter().any(|v| env.get(v).is_some()) {
        return Err(OFF_IN_TESTS);
    }
    Ok(())
}

/// What the settings say for the next round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub on: bool,
    pub interval_min: u64,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            on: true,
            interval_min: INTERVAL_DEFAULT_MIN,
        }
    }
}

/// Whether `minutes` is an interval the settings may name (1 to 60).
#[must_use]
pub fn interval_ok(minutes: u64) -> bool {
    (INTERVAL_MIN..=INTERVAL_MAX).contains(&minutes)
}

/// The schedule of the keys of `config.json` (`ci_watch`, `ci_watch_interval_min`), and a note when the interval
/// is outside the bounds (then it is the default).
#[must_use]
pub fn schedule_of(on: Option<bool>, interval: Option<u64>) -> (Schedule, Option<String>) {
    let (interval_min, note) = match interval {
        None => (INTERVAL_DEFAULT_MIN, None),
        Some(m) if interval_ok(m) => (m, None),
        Some(m) => (
            INTERVAL_DEFAULT_MIN,
            Some(format!(
                "ci_watch_interval_min {m} is not {INTERVAL_MIN} to {INTERVAL_MAX}; the CI watcher takes {INTERVAL_DEFAULT_MIN}"
            )),
        ),
    };
    (
        Schedule {
            on: on != Some(false),
            interval_min,
        },
        note,
    )
}

/// The settings as the page shows them, with the bounds of the field.
#[must_use]
pub fn settings_view(schedule: Schedule) -> Value {
    serde_json::json!({
        "on": schedule.on,
        "interval_min": schedule.interval_min,
        "min": INTERVAL_MIN,
        "max": INTERVAL_MAX,
        "default": INTERVAL_DEFAULT_MIN,
    })
}

/// What the watcher is at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// It does not run (a fixed sentence).
    Off(&'static str),
    /// It runs; no round has ended yet.
    Waiting,
    /// The last round, and when it ended (seconds since 1970).
    Ran { at: u64, round: Round },
}

/// What the watcher is at, for the page (`/api/hub/ci`) and the icon.
#[derive(Debug)]
pub struct Watcher {
    seen: std::sync::Mutex<Seen>,
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new(Seen::Waiting)
    }
}

impl Watcher {
    #[must_use]
    pub fn new(seen: Seen) -> Self {
        Self {
            seen: std::sync::Mutex::new(seen),
        }
    }

    /// A watcher that does not run, for this reason.
    #[must_use]
    pub fn off(why: &'static str) -> Self {
        Self::new(Seen::Off(why))
    }

    #[must_use]
    pub fn seen(&self) -> Seen {
        self.seen
            .lock()
            .map(|s| s.clone())
            .unwrap_or(Seen::Off("the state of the watcher is lost"))
    }

    /// The last round (none: no round has ended, or the watcher does not run).
    #[must_use]
    pub fn last(&self) -> Option<Round> {
        match self.seen() {
            Seen::Ran { round, .. } => Some(round),
            Seen::Off(_) | Seen::Waiting => None,
        }
    }

    /// How many things of the last round a person may want to look at: its hints, the verdicts the tasks command
    /// did not take, and a problem that kept it from the tasks.
    #[must_use]
    pub fn hint_count(&self) -> usize {
        self.last().map_or(0, |r| {
            r.hints.len() + r.not_written.len() + usize::from(r.problem.is_some())
        })
    }

    /// The document of `/api/hub/ci`: no path of the machine is in it, only repository names, task ids and fixed
    /// words.
    #[must_use]
    pub fn view(&self) -> Value {
        match self.seen() {
            Seen::Off(why) => serde_json::json!({ "state": "off", "why": why }),
            Seen::Waiting => serde_json::json!({ "state": "waiting" }),
            Seen::Ran { at, round } => {
                let verdicts = |m: &BTreeMap<String, Verdict>| -> Vec<Value> {
                    m.iter()
                        .map(|(id, v)| serde_json::json!({ "id": id, "ci": v.word() }))
                        .collect()
                };
                serde_json::json!({
                    "state": "ran",
                    "at": at,
                    "written": verdicts(&round.written),
                    "not_written": verdicts(&round.not_written),
                    "hints": round.hints.iter().map(hint_view).collect::<Vec<_>>(),
                    "problem": round.problem,
                })
            }
        }
    }
}

fn hint_view(hint: &Hint) -> Value {
    match hint {
        Hint::Queued { repo, minutes } => {
            serde_json::json!({ "kind": "queued", "repo": repo, "minutes": minutes })
        }
        Hint::Behind { repo } => serde_json::json!({ "kind": "behind", "repo": repo }),
        Hint::NoCheckout { repo } => serde_json::json!({ "kind": "no_checkout", "repo": repo }),
        Hint::NoGithub { repo } => serde_json::json!({ "kind": "no_github", "repo": repo }),
        Hint::NoAnswer { repo, why } => {
            serde_json::json!({ "kind": "no_answer", "repo": repo, "why": why })
        }
    }
}

/// The line of the console (`hub.log`) for a round: the verdicts, and the hints when they changed.
fn say(round: &Round, before: Option<&Round>) {
    for (id, verdict) in &round.written {
        println!("ci watch: {id} is {}", verdict.word());
    }
    for (id, verdict) in &round.not_written {
        eprintln!(
            "ci watch: the tasks command did not take ci={} for {id}",
            verdict.word()
        );
    }
    if let Some(problem) = round.problem {
        eprintln!("ci watch: {problem}");
    }
    if before.is_none_or(|b| b.hints != round.hints) {
        for hint in &round.hints {
            println!("ci watch: {hint:?}");
        }
    }
}

/// What the loop reads before every round (the settings of `config.json`, read again each time).
pub type Settings = std::sync::Arc<dyn Fn() -> Schedule + Send + Sync>;

/// Runs the first round `first` after the start, then one whenever `interval_min` times `minute` has passed since
/// the last, until the task is ended (the end of the hub). The settings are read every `tick`: a switch or a new
/// interval counts from the next look on, without a new start. Switched off it shows [`OFF_BY_SETTINGS`] and runs
/// nothing; switched on again its next round runs at once. A round runs on a thread of the blocking pool: it waits
/// for programs.
pub async fn watch(
    watcher: std::sync::Arc<Watcher>,
    ask: std::sync::Arc<dyn Ask>,
    programs: Programs,
    settings: Settings,
    first: Duration,
    tick: Duration,
    minute: Duration,
) {
    tokio::time::sleep(first).await;
    let mut last: Option<tokio::time::Instant> = None;
    loop {
        let schedule = settings();
        if !schedule.on {
            if let Ok(mut seen) = watcher.seen.lock()
                && *seen != Seen::Off(OFF_BY_SETTINGS)
            {
                println!("ci watch: off ({OFF_BY_SETTINGS})");
                *seen = Seen::Off(OFF_BY_SETTINGS);
            }
            last = None;
            tokio::time::sleep(tick).await;
            continue;
        }
        let every = minute.saturating_mul(u32::try_from(schedule.interval_min).unwrap_or(u32::MAX));
        if last.is_none_or(|t| t.elapsed() >= every) {
            if let Ok(mut seen) = watcher.seen.lock()
                && matches!(*seen, Seen::Off(_))
            {
                *seen = Seen::Waiting;
            }
            let (a, p) = (std::sync::Arc::clone(&ask), programs.clone());
            let done =
                tokio::task::spawn_blocking(move || round(a.as_ref(), &p, SystemTime::now()))
                    .await
                    .unwrap_or_else(|_| Round {
                        problem: Some("the round broke off"),
                        ..Round::default()
                    });
            last = Some(tokio::time::Instant::now());
            let at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Ok(mut seen) = watcher.seen.lock() {
                let before = match &*seen {
                    Seen::Ran { round, .. } => Some(round),
                    Seen::Off(_) | Seen::Waiting => None,
                };
                say(&done, before);
                *seen = Seen::Ran { at, round: done };
            }
        }
        tokio::time::sleep(tick).await;
    }
}

fn ask_head(
    ask: &dyn Ask,
    programs: &Programs,
    repo: &str,
    now: SystemTime,
    hints: &mut BTreeSet<Hint>,
) -> Head {
    let dir = programs.repos.join(repo);
    if !dir.join(".git").exists() {
        hints.insert(Hint::NoCheckout {
            repo: repo.to_owned(),
        });
        return Head::Unknown;
    }
    let remote = match ask.ask(&programs.remote(&dir), ASK_TIMEOUT) {
        // A red CI ends `ci remote` with exit 1 and the document all the same: the document decides.
        Ok(answer) => parse_remote(&answer.stdout),
        Err(_) => Err("testing did not start"),
    };
    match remote {
        Ok(remote) if remote.overall == State::NoGithub => {
            hints.insert(Hint::NoGithub {
                repo: repo.to_owned(),
            });
            Head::Unknown
        }
        Ok(remote) => {
            if remote.overall == State::Waiting
                && let Some(created) = remote.created
            {
                let now = now
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let waited = now.saturating_sub(created);
                if waited >= QUEUED_HINT_AFTER.as_secs() {
                    hints.insert(Hint::Queued {
                        repo: repo.to_owned(),
                        minutes: waited / 60,
                    });
                }
            }
            Head::Known(remote)
        }
        Err(why) => {
            hints.insert(Hint::NoAnswer {
                repo: repo.to_owned(),
                why,
            });
            Head::Unknown
        }
    }
}

fn judge(
    ask: &dyn Ask,
    programs: &Programs,
    repo: &str,
    sha: &str,
    remote: &Remote,
    hints: &mut BTreeSet<Hint>,
) -> CommitState {
    let dir = programs.repos.join(repo);
    let head_commit = format!("{}^{{commit}}", remote.sha);
    let known = ask
        .ask(
            &programs.git(&dir, &["cat-file", "-e", &head_commit]),
            ASK_TIMEOUT,
        )
        .is_ok_and(|a| a.code == Some(0));
    if !known {
        hints.insert(Hint::Behind {
            repo: repo.to_owned(),
        });
        return CommitState::Pending;
    }
    let contained = ask
        .ask(
            &programs.git(&dir, &["merge-base", "--is-ancestor", sha, &remote.sha]),
            ASK_TIMEOUT,
        )
        .is_ok_and(|a| a.code == Some(0));
    if !contained {
        return CommitState::Pending;
    }
    match remote.overall {
        // A later run that contains it and succeeded on every system speaks for it as well.
        State::Success => CommitState::Green,
        // Red is the commit's own run only: a later push that broke the CI says nothing against an older commit
        // the watcher had not judged yet (it stays pending until a green head contains it).
        State::Red if remote.sha.starts_with(sha) => CommitState::Red,
        State::Red | State::Waiting | State::Open | State::NoGithub => CommitState::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_of_github_are_read() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2026-11-21T00:59:27Z"), Some(1_795_222_767));
        assert_eq!(parse_utc("2000-02-29T00:00:00Z"), Some(951_782_400));
        assert_eq!(parse_utc("2026-11-21 00:59:27"), None);
        assert_eq!(parse_utc("2026-1x-21T00:59:27Z"), None);
    }

    #[test]
    fn listed_ids_are_plain() {
        let ids = parse_ids(b"demo/a\r\n../x/y\n\nother/b\n--force/x\n");
        assert_eq!(ids.into_iter().collect::<Vec<_>>(), ["demo/a", "other/b"]);
    }

    #[test]
    fn ids_and_repos_are_plain_words() {
        assert!(plain_id("hub-desktop/ci-beobachter"));
        assert!(!plain_id("../x/y"));
        assert!(!plain_id("a/b/c"));
        assert!(!plain_id("a/--force"));
        assert!(plain_repo("tasks.nvim"));
        assert!(!plain_repo(".."));
        assert!(!plain_repo("a/b"));
    }

    #[test]
    fn a_task_is_red_at_once_and_green_only_whole() {
        use CommitState::{Green, Pending, Red};
        assert_eq!(task_verdict(&[Green, Green]), Some(Verdict::Green));
        assert_eq!(task_verdict(&[Green, Pending]), None);
        assert_eq!(task_verdict(&[Pending, Red]), Some(Verdict::Red));
        assert_eq!(task_verdict(&[]), None);
    }

    #[test]
    fn the_watcher_runs_unless_turned_off_and_never_for_a_test() {
        let person = Env::from_pairs([("REPOS_DIR", "C:/r"), ("TASKS_VAULT", "C:/v")]);
        assert_eq!(allowed(&person), Ok(()));
        for marker in [
            "HUB_DESKTOP_CONFIG_DIR",
            "HUB_DESKTOP_NO_LAUNCH",
            "HUB_DESKTOP_NO_LOGIN_SYNC",
        ] {
            for value in ["1", "C:/t"] {
                let test = Env::from_pairs([("TASKS_VAULT", "C:/v"), (marker, value)]);
                assert_eq!(allowed(&test), Err(OFF_IN_TESTS), "{marker}={value:?}");
            }
        }
    }

    #[test]
    fn the_interval_is_1_to_60_minutes_and_the_default_outside() {
        assert_eq!(schedule_of(None, None), (Schedule::default(), None));
        assert_eq!(
            schedule_of(Some(false), Some(1)),
            (
                Schedule {
                    on: false,
                    interval_min: 1
                },
                None
            )
        );
        assert_eq!(schedule_of(Some(true), Some(60)).0.interval_min, 60);
        for out in [0, 61, u64::MAX] {
            let (schedule, note) = schedule_of(None, Some(out));
            assert_eq!(schedule, Schedule::default(), "{out}");
            let note = note.unwrap();
            assert!(
                note.contains(&out.to_string()) && note.contains("takes 5"),
                "{note}"
            );
        }
        assert!(interval_ok(1) && interval_ok(60) && !interval_ok(0) && !interval_ok(61));
        assert_eq!(
            settings_view(Schedule {
                on: false,
                interval_min: 7
            }),
            serde_json::json!({"on": false, "interval_min": 7, "min": 1, "max": 60, "default": 5})
        );
    }

    #[test]
    fn the_view_names_the_state_and_counts_what_to_look_at() {
        let off = Watcher::off(OFF_IN_TESTS);
        assert_eq!(
            off.view(),
            serde_json::json!({"state": "off", "why": OFF_IN_TESTS})
        );
        assert_eq!(off.hint_count(), 0);
        assert_eq!(
            Watcher::default().view(),
            serde_json::json!({"state": "waiting"})
        );
        let mut round = Round::default();
        round.written.insert("a/b".into(), Verdict::Green);
        round.not_written.insert("a/c".into(), Verdict::Red);
        round.hints.insert(Hint::Queued {
            repo: "x".into(),
            minutes: 42,
        });
        round.hints.insert(Hint::NoAnswer {
            repo: "y".into(),
            why: "testing gave no JSON",
        });
        round.problem = Some("the tasks command failed");
        let ran = Watcher::new(Seen::Ran { at: 7, round });
        assert_eq!(ran.hint_count(), 4);
        assert_eq!(
            ran.view(),
            serde_json::json!({
                "state": "ran",
                "at": 7,
                "written": [{"id": "a/b", "ci": "green"}],
                "not_written": [{"id": "a/c", "ci": "red"}],
                "hints": [
                    {"kind": "queued", "repo": "x", "minutes": 42},
                    {"kind": "no_answer", "repo": "y", "why": "testing gave no JSON"},
                ],
                "problem": "the tasks command failed",
            })
        );
    }

    #[test]
    fn the_states_of_testing_are_told_apart() {
        assert_eq!(State::of("success"), State::Success);
        assert_eq!(State::of("failure"), State::Red);
        assert_eq!(State::of("rejected"), State::Red);
        for word in [
            "cancelled",
            "running",
            "skipped",
            "no_run",
            "unknown",
            "anything",
        ] {
            assert_eq!(State::of(word), State::Open, "{word}");
        }
        for word in ["queued", "waiting_runner", "no_runner"] {
            assert_eq!(State::of(word), State::Waiting, "{word}");
        }
    }
}

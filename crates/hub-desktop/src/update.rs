//! Updating an app from the hub: the native build from its release, the web variant from its clone.
//!
//! What runs is decided by the catalog and the detection, never by a request: a request names an app
//! (or all of them) and nothing else. The programs are `git` and `gh`, found once on the `PATH` when the hub
//! starts, and the downloaded build itself once it is checked. Each run is a child of the hub through
//! `appkit_core::child` ([`SystemRunner`]): an argv with a working folder, no shell, no console input or window,
//! bound to the hub (it ends with the hub), named in its log, and ended with what it started when its time
//! ([`UpdateConfig::step_timeout`]) is up. No PowerShell and no script of the release is run.
//!
//! - **Web variant** (a clone of `tasks-web`, ...): only a clone on `main` without local changes is touched,
//!   and only by `git pull --ff-only` (no stash, no reset, no rebase). The page is then checked against the
//!   `SHA256SUMS` of the clone, as `start-tasks.cmd /update` does.
//! - **Native build** (an app with a release): only on the private machine ([`Role::installs`]). `gh`
//!   downloads `SHA256SUMS` and the file of the catalog from the rolling build `latest` (the channel the update
//!   badge compares with; `gh` brings its own sign-in for a private repository). The hub checks the file
//!   against `SHA256SUMS` and runs nothing that does not match. Then `gh
//!   attestation verify` says whether the file was built by the rolling workflow of its repository
//!   ([`verdict`]): a file whose attestation does not verify is never run; one that could not be checked (a
//!   build from before attestations, no network, not signed in) is installed with a note, unless
//!   [`UpdateConfig::require_attestation`] says otherwise. The hub is the one checker that was installed before
//!   the release it checks; it writes no Sigstore of its own. Then the hub runs the checked file as the catalog
//!   says ([`catalog::Setup`]): a program of the kit with `install`, which stops, replaces and restarts it (a copy
//!   beside, then a move: a failure leaves the old program); a setup with its own window. What it starts on
//!   purpose (the hub of the app, the app at the end of a setup) stays when it ends. A program that serves a page
//!   (ai-desktop) and runs is refused, the hub cannot stop it.
//!
//! One update at a time, in a queue: "Update all" puts every app that has something to update into it, and
//! a failure of one goes on with the next.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_core::child::{self, Environment, Leftovers, ProcError, Start};
use serde::Serialize;

use crate::catalog::{self, Entry, Hint, Native, Setup};
use crate::config::Env;
use crate::detect::{AppState, Detector, Run};
use crate::launch::Launcher;
use crate::machine::Role;
use crate::sums::{self, Check};

/// How many lines of output a job keeps (the last ones), and how long one line may be.
pub const MAX_LINES: usize = 40;
pub const MAX_LINE: usize = 300;

/// A `SHA256SUMS` is a few lines; a larger one is no list of sums.
const MAX_SUMS: u64 = 64 * 1024;

/// The workflow that builds and attests the rolling build `latest` (appkit `release-kit`), below
/// `<owner>/<repo>/.github/workflows/`.
pub const ROLLING_WORKFLOW: &str = "rolling.yml";

/// What `gh attestation verify` came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The file was built by the workflow of its repository.
    Attested,
    /// `gh` says the attestation is wrong: the file is not what that workflow built.
    Wrong,
    /// It could not be checked (no attestation, no network, not signed in, out of time): the line that says
    /// why, if there is one.
    NotChecked(Option<String>),
}

/// The verdict of a run of `gh attestation verify`. Only what `gh` itself calls a failed verification is
/// [`Verdict::Wrong`], as in the install scripts of the release kit: anything else is no "no".
#[must_use]
pub fn verdict(ran: &Ran) -> Verdict {
    if ran.ok() {
        return Verdict::Attested;
    }
    let said = ran.lines.join("\n").to_lowercase();
    let wrong = [
        "verification failed",
        "mismatch",
        "does not match",
        "doesn't match",
    ]
    .iter()
    .any(|sign| said.contains(sign));
    if wrong && ran.code.is_some() {
        Verdict::Wrong
    } else {
        Verdict::NotChecked(ran.lines.iter().find(|l| !l.trim().is_empty()).cloned())
    }
}

/// The list of the sums of a release (appkit `release-kit`).
pub const SUMS_FILE: &str = "SHA256SUMS";

/// How much one output stream of a step may write before the step is ended: a download or an install says a
/// few lines.
pub const MAX_OUTPUT: usize = 4 * 1024 * 1024;

/// Who starts the steps, as the log of the hub names it.
const CALLER: &str = "hub-desktop update";

/// One program run of an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    /// What happens to a process the step started and left running when it ended by itself: `End` (a call of
    /// git or gh), `Keep` (an install, which brings the hub of its app back, or a setup that starts its app).
    /// When the time is up, everything it started is ended either way.
    pub leftovers: Leftovers,
}

impl Step {
    fn new(program: &Path, args: &[&std::ffi::OsStr], cwd: &Path) -> Self {
        Self {
            program: program.to_path_buf(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            cwd: cwd.to_path_buf(),
            leftovers: Leftovers::End,
        }
    }

    /// The same step, leaving running what it started on purpose.
    fn keeping(mut self) -> Self {
        self.leftovers = Leftovers::Keep;
        self
    }

    /// The line a job shows for the step: the name of the program and its arguments, a path of the machine as
    /// the name of its file (the page gets no folder of the machine).
    fn shown(&self) -> String {
        let name = self
            .program
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let args: Vec<_> = self
            .args
            .iter()
            .map(|a| {
                let path = Path::new(a);
                match path.file_name() {
                    Some(file) if path.is_absolute() => file.to_string_lossy(),
                    _ => a.to_string_lossy(),
                }
            })
            .collect();
        clean_line(format!("$ {name} {}", args.join(" ")).as_bytes())
    }
}

/// How a step ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ran {
    /// The exit code; `None`: it was ended because its time was up (or by a signal).
    pub code: Option<i32>,
    /// What it printed, stdout and stderr, cleaned ([`clean_line`]).
    pub lines: Vec<String>,
}

impl Ran {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// Runs the steps of an update. The program has [`SystemRunner`]; a test has one that answers.
pub trait Runner: Send + Sync {
    /// Run `step` to its end, or end it after `timeout`.
    ///
    /// # Errors
    ///
    /// The program could not be started.
    fn run(&self, step: &Step, timeout: Duration) -> Result<Ran, String>;

    /// As [`Runner::run`], and every line goes to `on_line` as soon as it is there: an install that takes minutes shows
    /// what it says while it runs. A runner without a live view gives its lines at the end.
    ///
    /// # Errors
    ///
    /// The program could not be started.
    fn run_lines(
        &self,
        step: &Step,
        timeout: Duration,
        on_line: &(dyn Fn(String) + Send + Sync),
    ) -> Result<Ran, String> {
        let ran = self.run(step, timeout)?;
        for line in &ran.lines {
            on_line(line.clone());
        }
        Ok(ran)
    }
}

/// A line of output as the page gets it: text, without escape sequences and control characters (a tab
/// is a space), at most [`MAX_LINE`] characters.
#[must_use]
pub fn clean_line(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // An escape sequence of a terminal (`ESC [ ... letter`, or `ESC` and one character).
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            } else {
                chars.next();
            }
            continue;
        }
        if c == '\t' {
            out.push(' ');
        } else if !c.is_control() {
            out.push(c);
        }
        if out.chars().count() >= MAX_LINE {
            break;
        }
    }
    out.trim_end().to_owned()
}

/// The last [`MAX_LINES`] lines.
#[derive(Default)]
struct Tail(VecDeque<String>);

impl Tail {
    fn push(&mut self, line: String) {
        if self.0.len() == MAX_LINES {
            self.0.pop_front();
        }
        self.0.push_back(line);
    }
}

/// The programs of the machine, as children of the hub through `appkit_core::child`: an argv, stdin empty, the
/// output in pipes, no console window, bound to the hub (a job object on Windows, a process group elsewhere: a step
/// whose time is up ends with everything it started, and a hub that ends leaves no step behind), its start named
/// in the log of the hub.
pub struct SystemRunner;

impl SystemRunner {
    /// The environment of a step: the hub's, and nobody answers a question (git and gh must fail instead of
    /// asking). `PSModulePath` is not handed on: a program that starts Windows PowerShell would load the
    /// modules of PowerShell 7 with it (a hub started from a pwsh terminal); git and gh do not read it.
    fn environment() -> Environment {
        let set = [
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GH_PROMPT_DISABLED", "1"),
            ("GH_NO_UPDATE_NOTIFIER", "1"),
        ];
        Environment::Inherited {
            set: set
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            remove: vec!["PSModulePath".into()],
        }
    }
}

/// Run `future` to its end on a thread of its own with a runtime of its own, so that a caller may block inside
/// a runtime (`spawn_blocking`) and outside of one (a test).
pub(crate) fn block_on<T: Send>(
    future: impl std::future::Future<Output = T> + Send,
) -> Result<T, String> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("update-step".to_owned())
            .spawn_scoped(scope, || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map(|runtime| runtime.block_on(future))
                    .map_err(|e| format!("no runtime for the step: {e}"))
            })
            .map_err(|e| format!("no thread for the step: {e}"))?
            .join()
            .map_err(|_| "the step broke off".to_owned())?
    })
}

impl Runner for SystemRunner {
    fn run(&self, step: &Step, timeout: Duration) -> Result<Ran, String> {
        self.run_lines(step, timeout, &|_| {})
    }

    fn run_lines(
        &self,
        step: &Step,
        timeout: Duration,
        on_line: &(dyn Fn(String) + Send + Sync),
    ) -> Result<Ran, String> {
        if !step.program.is_absolute() || !step.program.is_file() {
            return Err(format!(
                "not started: {} is not an absolute path to a file",
                step.program.display()
            ));
        }
        if !step.cwd.is_absolute() || !step.cwd.is_dir() {
            return Err(format!(
                "not started: {} is not an absolute path to a folder",
                step.cwd.display()
            ));
        }
        let run = child::Run {
            start: Start::new(CALLER, &step.program)
                .args(&step.args)
                .cwd(&step.cwd)
                .env(Self::environment()),
            stdin: Vec::new(),
            timeout,
            max_stdout: MAX_OUTPUT,
            max_stderr: MAX_OUTPUT,
            leftovers: step.leftovers,
        };
        let tail = Mutex::new(Tail::default());
        let ended = block_on(child::run_lines(run, |_, line| {
            let line = clean_line(line.as_bytes());
            on_line(line.clone());
            if let Ok(mut tail) = tail.lock() {
                tail.push(line);
            }
        }))?;
        let mut tail = tail.into_inner().unwrap_or_default();
        let code = match ended {
            Ok(code) => code,
            Err(ProcError::Spawn(e)) => {
                return Err(format!("could not start {}: {e}", step.program.display()));
            }
            Err(failure) => {
                let why = match failure {
                    ProcError::Timeout(_) => "the step took too long",
                    ProcError::TooLarge { .. } => "the step wrote too much",
                    _ => "its output could not be read",
                };
                tail.push(format!("(ended: {why})"));
                None
            }
        };
        Ok(Ran {
            code,
            lines: tail.0.into(),
        })
    }
}

/// The programs an update runs, found when the hub starts.
#[derive(Debug, Clone, Default)]
pub struct Tools {
    pub git: Option<PathBuf>,
    pub gh: Option<PathBuf>,
}

impl Tools {
    /// `git` and `gh` in a folder of the `PATH` that is an absolute folder of this machine.
    #[must_use]
    pub fn find(env: &Env) -> Self {
        Self {
            git: on_path(env, "git"),
            gh: on_path(env, "gh"),
        }
    }
}

/// The program `name` (with the suffix of programs of this system) in the first folder of the `PATH` that is an
/// absolute folder of this machine and has it.
pub(crate) fn on_path(env: &Env, name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    env.get("PATH")
        .map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter_map(crate::paths::usable_root)
        .map(|d| d.join(&file))
        .find(|f| f.is_file())
}

/// What an update may do on this machine.
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    pub role: Role,
    pub tools: Tools,
    /// Where a release is downloaded to (a folder of its own per run, removed afterwards); none: no native
    /// update.
    pub work: Option<PathBuf>,
    /// How long one step may take: a download, a pull, an install.
    pub step_timeout: Duration,
    /// How long the check of an attestation may take; out of time is "not checked".
    pub attest_timeout: Duration,
    /// A build whose attestation could not be checked is not installed (`update.require_attestation` in
    /// `config.json`; off: it is installed with a note).
    pub require_attestation: bool,
}

impl UpdateConfig {
    #[must_use]
    pub fn new(role: Role, tools: Tools, work: Option<PathBuf>) -> Self {
        Self {
            role,
            tools,
            work,
            step_timeout: Duration::from_secs(600),
            attest_timeout: Duration::from_secs(60),
            require_attestation: false,
        }
    }

    /// With `update.require_attestation` of `config.json`.
    #[must_use]
    pub fn requiring_attestation(mut self, require: bool) -> Self {
        self.require_attestation = require;
        self
    }

    /// Nothing can be updated: no programs, no folder, not the private machine.
    #[must_use]
    pub fn none() -> Self {
        Self::new(Role::Unclear, Tools::default(), None)
    }
}

/// Whether a part of an app can be updated: `Ok`, or the code of why not.
pub type Can = Result<(), &'static str>;

/// What an update of an app would touch: its native build and its web variant, where they are installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub native: Option<Can>,
    pub web: Option<Can>,
}

impl Plan {
    #[must_use]
    pub fn of(state: &AppState, cfg: &UpdateConfig) -> Self {
        let entry = state.entry;
        let native = (entry.native.is_some() && state.native.is_some()).then(|| {
            match cfg.role {
                Role::Workstation => return Err("workstation"),
                Role::Unclear => return Err("role_unclear"),
                Role::Private => {}
            }
            if !cfg!(windows) {
                return Err("windows_only");
            }
            if entry.install.hint != Hint::Release {
                return Err("no_release");
            }
            if entry.install.file.is_none() {
                return Err("no_release_file");
            }
            // A setup of the catalog, and `install` only for a program of the kit.
            match (entry.install.setup, entry.native) {
                (Some(Setup::Program), Some(Native::AppKitHub { .. }))
                | (Some(Setup::Nsis | Setup::Inno), _) => {}
                _ => return Err("no_installer"),
            }
            if cfg.tools.gh.is_none() {
                return Err("no_gh");
            }
            if cfg.work.is_none() {
                return Err("no_work_folder");
            }
            Ok(())
        });
        let web = state.web.is_some().then(|| {
            if cfg.tools.git.is_none() {
                return Err("no_git");
            }
            Ok(())
        });
        Self { native, web }
    }

    /// Whether anything of it can be updated.
    #[must_use]
    pub fn any(&self) -> bool {
        self.native == Some(Ok(())) || self.web == Some(Ok(()))
    }

    /// Why nothing can: the first reason, or that nothing of the app is installed.
    #[must_use]
    pub fn why_not(&self) -> &'static str {
        [self.native, self.web]
            .into_iter()
            .flatten()
            .find_map(Result::err)
            .unwrap_or("not_installed")
    }
}

fn can_word(can: Option<Can>) -> Option<&'static str> {
    can.map(|c| c.err().unwrap_or("ready"))
}

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    /// Every part that could be updated was.
    Done,
    /// A part failed (what the others did is in `parts`).
    Failed,
    /// Nothing could be done on this machine (all parts were refused).
    Skipped,
}

/// What came of one part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The clone moved to a newer commit.
    Updated,
    /// The clone had the newest commit already.
    UpToDate,
    /// The build of the release was installed.
    Installed,
    /// Not tried here (the machine, a missing program): the code says why.
    Skipped(&'static str),
    Failed(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PartView {
    /// `native` or `web`.
    pub part: &'static str,
    /// `ok`, `skipped` or `failed`.
    pub outcome: &'static str,
    /// `updated`, `up_to_date`, `installed`, or the code of why not.
    pub result: &'static str,
}

impl PartView {
    fn of(part: &'static str, outcome: Outcome) -> Self {
        let (outcome, result) = match outcome {
            Outcome::Updated => ("ok", "updated"),
            Outcome::UpToDate => ("ok", "up_to_date"),
            Outcome::Installed => ("ok", "installed"),
            Outcome::Skipped(code) => ("skipped", code),
            Outcome::Failed(code) => ("failed", code),
        };
        Self {
            part,
            outcome,
            result,
        }
    }
}

/// An update of one app as the page gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobView {
    pub state: JobState,
    /// What it does now (`downloading`, `pulling`, ...), while it runs.
    pub step: Option<&'static str>,
    pub parts: Vec<PartView>,
    /// The last lines of what the steps printed.
    pub lines: Vec<String>,
    /// The page of the web variant changed: an open one should be reloaded.
    pub reload: bool,
    /// What the check of the attestation of the native build came to: `attested`, `not_checked` or `failed`;
    /// none: it did not get that far, or there is no native part.
    pub attestation: Option<&'static str>,
}

impl JobView {
    fn queued() -> Self {
        Self {
            state: JobState::Queued,
            step: None,
            parts: Vec::new(),
            lines: Vec::new(),
            reload: false,
            attestation: None,
        }
    }

    fn active(&self) -> bool {
        matches!(self.state, JobState::Queued | JobState::Running)
    }
}

/// What an app can do about updates here, and its last job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateView {
    pub id: &'static str,
    /// `ready`, the code of why not, or `null` when the native app is not installed.
    pub native: Option<&'static str>,
    pub web: Option<&'static str>,
    pub job: Option<JobView>,
}

/// What a request for an update came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Queued {
    Queued,
    /// An update of the app is queued or running already.
    Already,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateError {
    UnknownApp,
    /// Nothing of the app can be updated here: the code says why.
    NotUpdatable(&'static str),
}

#[derive(Default)]
struct Inner {
    jobs: BTreeMap<&'static str, JobView>,
    queue: VecDeque<&'static str>,
    working: bool,
}

/// Removes a download folder when the run ends, however it ends.
struct Scrap(PathBuf);

impl Drop for Scrap {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Updater {
    detector: Arc<Detector>,
    launcher: Arc<Launcher>,
    cfg: UpdateConfig,
    runner: Arc<dyn Runner>,
    inner: Mutex<Inner>,
}

impl Updater {
    #[must_use]
    pub fn new(
        detector: Arc<Detector>,
        launcher: Arc<Launcher>,
        cfg: UpdateConfig,
        runner: Arc<dyn Runner>,
    ) -> Self {
        Self {
            detector,
            launcher,
            cfg,
            runner,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// An updater that can update nothing (the tests of other routes).
    #[must_use]
    pub fn disabled(detector: Arc<Detector>, launcher: Arc<Launcher>) -> Self {
        Self::new(
            detector,
            launcher,
            UpdateConfig::none(),
            Arc::new(SystemRunner),
        )
    }

    /// Every app that is listed, with what can be updated and its last job.
    pub async fn views(&self) -> Vec<UpdateView> {
        let apps = self.detector.apps().await;
        let jobs = self
            .inner
            .lock()
            .map(|inner| inner.jobs.clone())
            .unwrap_or_default();
        apps.iter()
            .map(|state| {
                let plan = Plan::of(state, &self.cfg);
                UpdateView {
                    id: state.entry.id,
                    native: can_word(plan.native),
                    web: can_word(plan.web),
                    job: jobs.get(state.entry.id).cloned(),
                }
            })
            .collect()
    }

    /// Queue an update of one app.
    ///
    /// # Errors
    ///
    /// [`UpdateError`]: nothing was queued.
    pub async fn request(self: &Arc<Self>, id: &str) -> Result<Queued, UpdateError> {
        let entry = catalog::find(id)
            .filter(|e| !self.detector.hides(e.id))
            .ok_or(UpdateError::UnknownApp)?;
        let plan = Plan::of(&self.detector.look_one(entry).await, &self.cfg);
        if !plan.any() {
            return Err(UpdateError::NotUpdatable(plan.why_not()));
        }
        Ok(if self.enqueue(&[entry.id]) == 1 {
            Queued::Queued
        } else {
            Queued::Already
        })
    }

    /// Queue an update of every listed app that has something to update; the number newly queued.
    pub async fn request_all(self: &Arc<Self>) -> usize {
        let apps = self.detector.apps().await;
        let ids: Vec<&'static str> = apps
            .iter()
            .filter(|state| Plan::of(state, &self.cfg).any())
            .map(|state| state.entry.id)
            .collect();
        self.enqueue(&ids)
    }

    fn enqueue(self: &Arc<Self>, ids: &[&'static str]) -> usize {
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        let mut added = 0;
        for id in ids {
            if inner.jobs.get(id).is_some_and(JobView::active) {
                continue;
            }
            inner.jobs.insert(id, JobView::queued());
            inner.queue.push_back(id);
            added += 1;
        }
        if added > 0 && !inner.working {
            inner.working = true;
            let this = Arc::clone(self);
            tokio::spawn(async move { this.work().await });
        }
        added
    }

    /// One job after the other, until the queue is empty.
    async fn work(self: Arc<Self>) {
        loop {
            let next = {
                let Ok(mut inner) = self.inner.lock() else {
                    return;
                };
                match inner.queue.pop_front() {
                    Some(id) => id,
                    None => {
                        inner.working = false;
                        return;
                    }
                }
            };
            self.run_app(next).await;
        }
    }

    fn change(&self, id: &'static str, f: impl FnOnce(&mut JobView)) {
        if let Ok(mut inner) = self.inner.lock()
            && let Some(job) = inner.jobs.get_mut(id)
        {
            f(job);
        }
    }

    async fn run_app(&self, id: &'static str) {
        self.change(id, |job| {
            job.state = JobState::Running;
            job.step = Some("looking");
        });
        let parts = match catalog::find(id).filter(|e| !self.detector.hides(e.id)) {
            Some(entry) => self.parts_of(entry).await,
            None => vec![PartView::of("native", Outcome::Failed("not_found"))],
        };
        let state = if parts.iter().any(|p| p.outcome == "failed") {
            JobState::Failed
        } else if parts.iter().all(|p| p.outcome == "skipped") {
            JobState::Skipped
        } else {
            JobState::Done
        };
        let reload = parts
            .iter()
            .any(|p| p.part == "web" && p.result == "updated");
        self.change(id, |job| {
            job.state = state;
            job.step = None;
            job.parts = parts;
            job.reload = reload;
        });
        // The next look sees the new build.
        self.detector.invalidate();
    }

    async fn parts_of(&self, entry: &'static Entry) -> Vec<PartView> {
        let state = self.detector.look_one(entry).await;
        let plan = Plan::of(&state, &self.cfg);
        let mut parts = Vec::new();
        if let Some(can) = plan.native {
            let outcome = match can {
                Ok(()) => self.native(entry, &state).await,
                Err(code) => Outcome::Skipped(code),
            };
            parts.push(PartView::of("native", outcome));
            let shown = parts.clone();
            self.change(entry.id, |job| job.parts = shown);
        }
        if let Some(can) = plan.web {
            let outcome = match can {
                Ok(()) => self.web(entry, &state).await,
                Err(code) => Outcome::Skipped(code),
            };
            parts.push(PartView::of("web", outcome));
        }
        if parts.is_empty() {
            parts.push(PartView::of("native", Outcome::Skipped("not_installed")));
        }
        parts
    }

    fn say(&self, id: &'static str, line: String) {
        self.change(id, |job| {
            if job.lines.len() == MAX_LINES {
                job.lines.remove(0);
            }
            job.lines.push(line);
        });
    }

    /// Run one step, its lines into the job. `Err`: it could not be started (said in the lines).
    async fn step(&self, id: &'static str, word: &'static str, step: Step) -> Result<Ran, ()> {
        self.step_in(id, word, step, self.cfg.step_timeout).await
    }

    /// As [`Self::step`], with the time the step may take.
    async fn step_in(
        &self,
        id: &'static str,
        word: &'static str,
        step: Step,
        timeout: Duration,
    ) -> Result<Ran, ()> {
        self.change(id, |job| job.step = Some(word));
        self.say(id, step.shown());
        let runner = Arc::clone(&self.runner);
        let (send, mut said) = tokio::sync::mpsc::unbounded_channel::<String>();
        let running = tokio::task::spawn_blocking(move || {
            runner.run_lines(&step, timeout, &|line| {
                let _ = send.send(line);
            })
        });
        // Ends when the step does: the sender goes with the closure.
        while let Some(line) = said.recv().await {
            self.say(id, line);
        }
        let ran = running
            .await
            .unwrap_or_else(|_| Err("the step stopped unexpectedly".to_owned()));
        match ran {
            Ok(ran) => Ok(ran),
            Err(why) => {
                // Why it could not start may hold a path: the console of the hub has it, the page a sentence.
                eprintln!("update {id}: {why}");
                self.say(id, "(the program could not be started)".to_owned());
                Err(())
            }
        }
    }

    /// `git pull --ff-only` in the clone of the web variant, then the check of the page.
    async fn web(&self, entry: &'static Entry, state: &AppState) -> Outcome {
        let (Some(git), Some(web), Some(dir)) = (
            self.cfg.tools.git.clone(),
            entry.web,
            state
                .web
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf),
        ) else {
            return Outcome::Failed("not_ready");
        };
        let id = entry.id;
        if !dir.join(".git").exists() {
            return Outcome::Failed("not_a_clone");
        }
        let git_step = |args: &[&str]| {
            let args: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
            Step::new(&git, &args, &dir)
        };
        let Ok(branch) = self
            .step(
                id,
                "checking",
                git_step(&["symbolic-ref", "--quiet", "--short", "HEAD"]),
            )
            .await
        else {
            return Outcome::Failed("git_failed");
        };
        if !branch.ok() || branch.lines.last().map(String::as_str) != Some("main") {
            return Outcome::Failed("not_on_main");
        }
        let Ok(status) = self
            .step(
                id,
                "checking",
                git_step(&["status", "--porcelain", "--untracked-files=no"]),
            )
            .await
        else {
            return Outcome::Failed("git_failed");
        };
        if !status.ok() {
            return Outcome::Failed("git_failed");
        }
        if status.lines.iter().any(|l| !l.trim().is_empty()) {
            return Outcome::Failed("dirty");
        }
        let head = |ran: Result<Ran, ()>| {
            ran.ok()
                .filter(Ran::ok)
                .and_then(|r| r.lines.last().cloned())
        };
        let before = head(
            self.step(id, "checking", git_step(&["rev-parse", "HEAD"]))
                .await,
        );
        match self
            .step(id, "pulling", git_step(&["pull", "--ff-only"]))
            .await
        {
            Ok(pull) if pull.ok() => {}
            _ => return Outcome::Failed("pull_failed"),
        }
        let after = head(
            self.step(id, "checking", git_step(&["rev-parse", "HEAD"]))
                .await,
        );
        self.change(id, |job| job.step = Some("verifying"));
        match read_sums(&dir) {
            None => self.say(
                id,
                format!("{SUMS_FILE} is missing: {} is not checked.", web.entry),
            ),
            Some(text) => match sums::check(&dir, &text, web.entry) {
                Ok(Check::Matches) => self.say(id, format!("{}: matches {SUMS_FILE}.", web.entry)),
                Ok(Check::NotListed) => {
                    self.say(
                        id,
                        format!("{SUMS_FILE} names no {}: it is not checked.", web.entry),
                    );
                }
                Ok(Check::Differs) | Err(_) => return Outcome::Failed("sums_mismatch"),
            },
        }
        match (before, after) {
            (Some(b), Some(a)) if b == a => Outcome::UpToDate,
            _ => Outcome::Updated,
        }
    }

    /// The rolling build of the release: download, check, install.
    async fn native(&self, entry: &'static Entry, state: &AppState) -> Outcome {
        let (Some(gh), Some(work), Some(file), Some(setup)) = (
            self.cfg.tools.gh.clone(),
            self.cfg.work.clone(),
            entry.install.file,
            entry.install.setup,
        ) else {
            return Outcome::Failed("not_ready");
        };
        let id = entry.id;
        if matches!(entry.native, Some(Native::LoopbackServer { .. }))
            && state.run == Some(Run::Running)
        {
            return Outcome::Failed("close_first");
        }
        // No click starts the app while its program is replaced.
        let Some(_held) = self.launcher.reserve(id) else {
            return Outcome::Failed("busy");
        };
        let dir = work.join(format!("{id}-{}", unique()));
        let _ = std::fs::remove_dir_all(&dir);
        if std::fs::create_dir_all(&dir).is_err() {
            return Outcome::Failed("no_work_folder");
        }
        let _scrap = Scrap(dir.clone());
        let repo = format!("{}/{}", entry.repo.owner, entry.repo.name);
        let download = Step::new(
            &gh,
            &[
                "release".as_ref(),
                "download".as_ref(),
                "latest".as_ref(),
                "-R".as_ref(),
                repo.as_ref(),
                "-p".as_ref(),
                SUMS_FILE.as_ref(),
                "-p".as_ref(),
                file.as_ref(),
                "-D".as_ref(),
                dir.as_os_str(),
                "--clobber".as_ref(),
            ],
            &dir,
        );
        match self.step(id, "downloading", download).await {
            Ok(ran) if ran.ok() => {}
            _ => return Outcome::Failed("download_failed"),
        }
        self.change(id, |job| job.step = Some("verifying"));
        let Some(text) = read_sums(&dir) else {
            return Outcome::Failed("checksum_failed");
        };
        if sums::check(&dir, &text, file) != Ok(Check::Matches) {
            self.say(
                id,
                format!("{file} does not match {SUMS_FILE}: nothing is run."),
            );
            return Outcome::Failed("checksum_failed");
        }
        self.say(id, format!("{file}: matches {SUMS_FILE}."));
        let build = dir.join(file);
        if let Err(code) = self.attest(id, &gh, &repo, &build, &dir).await {
            return Outcome::Failed(code);
        }
        self.install(id, setup, &build, &dir).await
    }

    /// Run the checked file of a release, as its [`Setup`] says.
    async fn install(&self, id: &'static str, setup: Setup, build: &Path, dir: &Path) -> Outcome {
        let ran = match setup {
            Setup::Program => {
                // A file that does not run as a program is not asked to install itself.
                match self
                    .step(
                        id,
                        "checking",
                        Step::new(build, &["--version".as_ref()], dir),
                    )
                    .await
                {
                    Ok(ran) if ran.ok() => {}
                    _ => return Outcome::Failed("install_failed"),
                }
                // `install` puts the program in its place and brings the hub of the app back: that hub stays.
                let install = Step::new(build, &["install".as_ref()], dir).keeping();
                self.step(id, "installing", install).await
            }
            // A setup shows its own window (what is installed is seen), and the app it may start at its end
            // stays.
            Setup::Nsis => {
                self.step(id, "installing", Step::new(build, &[], dir).keeping())
                    .await
            }
            Setup::Inno => {
                let args = ["/CURRENTUSER".as_ref(), "/NORESTART".as_ref()];
                self.step(id, "installing", Step::new(build, &args, dir).keeping())
                    .await
            }
        };
        match ran {
            Ok(ran) if ran.ok() => Outcome::Installed,
            _ => Outcome::Failed("install_failed"),
        }
    }
}

impl Updater {
    /// Ask `gh` whether `build` was built by the rolling workflow of `repo`, and say what came of it in the job.
    /// `Err`: the code of why nothing is installed.
    async fn attest(
        &self,
        id: &'static str,
        gh: &Path,
        repo: &str,
        build: &Path,
        dir: &Path,
    ) -> Result<(), &'static str> {
        let workflow = format!("{repo}/.github/workflows/{ROLLING_WORKFLOW}");
        let verify = Step::new(
            gh,
            &[
                "attestation".as_ref(),
                "verify".as_ref(),
                build.as_os_str(),
                "-R".as_ref(),
                repo.as_ref(),
                "--signer-workflow".as_ref(),
                workflow.as_ref(),
            ],
            dir,
        );
        let name = build
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let found = match self
            .step_in(id, "attesting", verify, self.cfg.attest_timeout)
            .await
        {
            Ok(ran) => verdict(&ran),
            Err(()) => Verdict::NotChecked(None),
        };
        let (word, result) = match found {
            Verdict::Attested => {
                self.say(id, format!("{name}: attested, built by {workflow}."));
                ("attested", Ok(()))
            }
            Verdict::Wrong => {
                self.say(
                    id,
                    format!("{name}: its attestation does not verify: nothing is run."),
                );
                ("failed", Err("attestation_failed"))
            }
            Verdict::NotChecked(_) if self.cfg.require_attestation => {
                self.say(
                    id,
                    format!(
                        "{name}: its attestation could not be checked, and update.require_attestation is set: nothing is run."
                    ),
                );
                ("not_checked", Err("attestation_required"))
            }
            Verdict::NotChecked(_) => {
                self.say(
                    id,
                    format!("{name}: its attestation could not be checked (see above); installing on its sums."),
                );
                ("not_checked", Ok(()))
            }
        };
        self.change(id, |job| job.attestation = Some(word));
        result
    }
}

/// `SHA256SUMS` of a folder, if it is there and of the size of one.
fn read_sums(dir: &Path) -> Option<String> {
    let file = dir.join(SUMS_FILE);
    let meta = std::fs::metadata(&file).ok()?;
    (meta.is_file() && meta.len() <= MAX_SUMS)
        .then(|| std::fs::read_to_string(&file).ok())
        .flatten()
}

/// A name for a download folder that no other run has.
fn unique() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_of_output_is_plain_text_of_a_bounded_length() {
        assert_eq!(clean_line(b"plain"), "plain");
        assert_eq!(clean_line(b"a\tb\r"), "a b");
        assert_eq!(clean_line(b"\x1b[31mred\x1b[0m and \x1b=x"), "red and x");
        assert_eq!(clean_line(b"bell\x07 nul\x00 del\x7f"), "bell nul del");
        assert_eq!(clean_line("ausführbar".as_bytes()), "ausführbar");
    }

    #[test]
    fn only_what_gh_calls_a_failed_verification_is_a_wrong_attestation() {
        let ran = |code: Option<i32>, lines: &[&str]| Ran {
            code,
            lines: lines.iter().map(|l| (*l).to_owned()).collect(),
        };
        assert_eq!(
            verdict(&ran(Some(0), &["Verification succeeded!"])),
            Verdict::Attested
        );
        for said in [
            "Error: verification failed: no matching predicate",
            "x signer workflow MISMATCH",
            "the digest does not match",
            "the subject doesn't match",
        ] {
            assert_eq!(
                verdict(&ran(Some(1), &["Loaded digest", said])),
                Verdict::Wrong,
                "{said}"
            );
        }
        // Not a "no": no attestation yet, no network, not signed in.
        for said in [
            "HTTP 404: Not Found (no attestations found)",
            "error connecting to api.github.com",
            "To get started with GitHub CLI, please run: gh auth login",
        ] {
            assert_eq!(
                verdict(&ran(Some(1), &["", said])),
                Verdict::NotChecked(Some(said.to_owned())),
                "{said}"
            );
        }
        assert_eq!(verdict(&ran(Some(1), &[])), Verdict::NotChecked(None));
        // Out of time is not a verdict of gh, whatever it had printed by then.
        assert_eq!(
            verdict(&ran(None, &["verification failed"])),
            Verdict::NotChecked(Some("verification failed".to_owned()))
        );
    }

    #[test]
    fn the_tail_keeps_the_last_lines() {
        let mut tail = Tail::default();
        for i in 0..(MAX_LINES + 5) {
            tail.push(i.to_string());
        }
        assert_eq!(tail.0.len(), MAX_LINES);
        assert_eq!(tail.0.front().map(String::as_str), Some("5"));
    }
}

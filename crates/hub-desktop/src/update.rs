//! Updating an app from the hub: the native build from its release, the web variant from its clone.
//!
//! What runs is decided by the catalog and the detection, never by a request: a request names an app
//! (or all of them) and nothing else. The programs are three, found once when the hub starts: `git` and
//! `gh` on the `PATH`, Windows PowerShell below `%SystemRoot%`. Each run is an argv with a working folder,
//! no shell, no console input, and an end in time ([`UpdateConfig::step_timeout`]).
//!
//! - **Web variant** (a clone of `tasks-web`, ...): only a clone on `main` without local changes is touched,
//!   and only by `git pull --ff-only` (no stash, no reset, no rebase). The page is then checked against the
//!   `SHA256SUMS` of the clone, as `start-tasks.cmd /update` does.
//! - **Native build** (an app with a release): only on the private machine ([`Role::installs`]). `gh`
//!   downloads `install.ps1`, `SHA256SUMS` and the file of the catalog from the rolling build `latest` (the
//!   channel the update badge compares with; `gh` brings its own sign-in for a private repository). The hub
//!   checks the script and the file against `SHA256SUMS` and runs nothing that does not match; the script
//!   checks the file again and installs it. An app of the kit stops, replaces and restarts itself in its
//!   `install` (a copy beside, then a move: a failure leaves the old program); a program that serves a page
//!   (ai-desktop) and runs is refused, the hub cannot stop it.
//!
//! One update at a time, in a queue: "Update all" puts every app that has something to update into it, and
//! a failure of one goes on with the next.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::catalog::{self, Entry, Hint, Native};
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

/// The script of a release (appkit `release-kit`), and the list of its sums.
pub const INSTALL_SCRIPT: &str = "install.ps1";
pub const SUMS_FILE: &str = "SHA256SUMS";

/// One program run of an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
}

impl Step {
    fn new(program: &Path, args: &[&std::ffi::OsStr], cwd: &Path) -> Self {
        Self {
            program: program.to_path_buf(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            cwd: cwd.to_path_buf(),
        }
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

/// The programs of the machine, by `std::process::Command` with an argv: nothing of this process's console
/// reaches them (stdin is the null device, the output goes into pipes), each in a process group of its own.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, step: &Step, timeout: Duration) -> Result<Ran, String> {
        use std::io::{BufRead, BufReader, Read};
        use std::process::{Command, Stdio};

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
        let mut command = Command::new(&step.program);
        command
            .args(&step.args)
            .current_dir(&step.cwd)
            // Nobody answers a question: git and gh must fail instead of asking.
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            // A hub started from PowerShell 7 has its module path: Windows PowerShell would load the modules of 7
            // and find no `Get-FileHash` (the install script fails). Without it, 5.1 makes its own (pwsh does the
            // same when it starts powershell.exe); git and gh do not read it.
            .env_remove("PSModulePath")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NEW_PROCESS_GROUP: a Ctrl-C of the console of the hub does not reach the step. Never
            // CREATE_NO_WINDOW (an invisible process is what an endpoint protection looks for).
            command.creation_flags(0x0000_0200);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", step.program.display()))?;
        let (send, lines) = std::sync::mpsc::channel::<String>();
        let pipes: [Option<Box<dyn Read + Send>>; 2] = [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ];
        for pipe in pipes.into_iter().flatten() {
            let send = send.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).split(b'\n') {
                    let Ok(line) = line else { break };
                    if send.send(clean_line(&line)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(send);
        let mut tail = Tail::default();
        let end = Instant::now() + timeout;
        let code = loop {
            while let Ok(line) = lines.try_recv() {
                tail.push(line);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) if Instant::now() >= end => {
                    let _ = child.kill();
                    let _ = child.wait();
                    tail.push("(ended: the step took too long)".to_owned());
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(e.to_string()),
            }
        };
        // What is still in the pipes. A program the step left running (a daemon of git) may hold them
        // open: it is not waited for.
        let grace = Instant::now() + Duration::from_millis(500);
        while let Ok(line) = lines.recv_timeout(grace.saturating_duration_since(Instant::now())) {
            tail.push(line);
        }
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
    pub powershell: Option<PathBuf>,
}

impl Tools {
    /// `git` and `gh` in a folder of the `PATH` that is an absolute folder of this machine; Windows
    /// PowerShell at its fixed place below `%SystemRoot%`.
    #[must_use]
    pub fn find(env: &Env) -> Self {
        let dirs: Vec<PathBuf> = env
            .get("PATH")
            .map(|p| std::env::split_paths(p).collect())
            .unwrap_or_default();
        let on_path = |name: &str| {
            let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
            dirs.iter()
                .filter_map(|d| crate::paths::usable_root(d.clone()))
                .map(|d| d.join(&file))
                .find(|f| f.is_file())
        };
        let powershell = if cfg!(windows) {
            env.path("SystemRoot")
                .and_then(crate::paths::usable_root)
                .map(|root| root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
                .filter(|f| f.is_file())
        } else {
            None
        };
        Self {
            git: on_path("git"),
            gh: on_path("gh"),
            powershell,
        }
    }
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
}

impl UpdateConfig {
    #[must_use]
    pub fn new(role: Role, tools: Tools, work: Option<PathBuf>) -> Self {
        Self {
            role,
            tools,
            work,
            step_timeout: Duration::from_secs(600),
        }
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
            if cfg.tools.gh.is_none() {
                return Err("no_gh");
            }
            if cfg.tools.powershell.is_none() || cfg.work.is_none() {
                return Err("no_powershell");
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
}

impl JobView {
    fn queued() -> Self {
        Self {
            state: JobState::Queued,
            step: None,
            parts: Vec::new(),
            lines: Vec::new(),
            reload: false,
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
        self.change(id, |job| job.step = Some(word));
        self.say(id, step.shown());
        let runner = Arc::clone(&self.runner);
        let timeout = self.cfg.step_timeout;
        let ran = tokio::task::spawn_blocking(move || runner.run(&step, timeout))
            .await
            .unwrap_or_else(|_| Err("the step stopped unexpectedly".to_owned()));
        match ran {
            Ok(ran) => {
                for line in &ran.lines {
                    self.say(id, line.clone());
                }
                Ok(ran)
            }
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
        let (Some(gh), Some(powershell), Some(work), Some(file)) = (
            self.cfg.tools.gh.clone(),
            self.cfg.tools.powershell.clone(),
            self.cfg.work.clone(),
            entry.install.file,
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
                INSTALL_SCRIPT.as_ref(),
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
        for name in [INSTALL_SCRIPT, file] {
            if sums::check(&dir, &text, name) != Ok(Check::Matches) {
                self.say(
                    id,
                    format!("{name} does not match {SUMS_FILE}: nothing is run."),
                );
                return Outcome::Failed("checksum_failed");
            }
            self.say(id, format!("{name}: matches {SUMS_FILE}."));
        }
        let script = dir.join(INSTALL_SCRIPT);
        let build = dir.join(file);
        let sums = dir.join(SUMS_FILE);
        let install = Step::new(
            &powershell,
            &[
                "-NoProfile".as_ref(),
                "-NonInteractive".as_ref(),
                "-ExecutionPolicy".as_ref(),
                "Bypass".as_ref(),
                "-File".as_ref(),
                script.as_os_str(),
                "-File".as_ref(),
                build.as_os_str(),
                "-Sums".as_ref(),
                sums.as_os_str(),
            ],
            &dir,
        );
        match self.step(id, "installing", install).await {
            Ok(ran) if ran.ok() => Outcome::Installed,
            _ => Outcome::Failed("install_failed"),
        }
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
        assert_eq!(clean_line(&[0xff, b'a']), "\u{fffd}a");
        assert_eq!(
            clean_line("é".repeat(400).as_bytes()).chars().count(),
            MAX_LINE
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

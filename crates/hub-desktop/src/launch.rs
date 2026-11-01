//! Starting an app (concept 6, 8).
//!
//! A start is decided by three things only: the id, which must be in the catalog, the mode, which is
//! one of a few words, and what the detection found on this machine. The program that runs is the one the
//! detection found (a file below its root, resolved), its argument list is written here per way of
//! starting and nowhere else, and it goes through `appkit_hub::launch::spawn`: an argv, nothing
//! inherited, a working folder and a window that are named. Nothing of a request, of `config.json` or of
//! a page reaches a command line.
//!
//! One start at a time per app: while one is on its way, a click, a second click and a second tab get
//! `starting` and no second program.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_hub::launch::{spawn, LaunchOpts, Window};

use crate::catalog::{self, Entry, Mode, Native};
use crate::detect::{AppState, Detector, Run};

/// How the hub opens a page in the browser.
#[derive(Debug, Clone)]
pub enum Browser {
    /// The default browser, by the kit (`appkit_hub::browser::open`).
    System,
    /// Write the link to a file instead of opening it (`HUB_DESKTOP_BROWSER_LOG`: the browser tests).
    Log(PathBuf),
}

impl Browser {
    /// Open `url`, which must be an address of this machine on one of `ports` (the kit checks it
    /// again; the check is ours too, so that a test that logs sees the same refusals).
    ///
    /// # Errors
    ///
    /// The link is not allowed, or the browser could not be started.
    pub fn open(&self, url: &str, ports: &[u16]) -> Result<(), String> {
        if !appkit_hub::browser::is_allowed(url, ports) {
            return Err("the link is not one the hub opens".to_owned());
        }
        match self {
            Self::System => appkit_hub::browser::open(url, ports),
            Self::Log(file) => {
                use std::io::Write;
                let mut out = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file)
                    .map_err(|e| e.to_string())?;
                writeln!(out, "{url}").map_err(|e| e.to_string())
            }
        }
    }
}

/// What the hub does around a start.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// The port of the listener of the web variants; none when it could not start.
    pub web_port: Option<u16>,
    pub browser: Browser,
    /// How long the hub waits for an app to come up (a hub to answer, ai-desktop to say `/health`).
    pub ready_timeout: Duration,
    /// How long a start counts as on its way when there is nothing to wait for (a window, a page).
    pub settle: Duration,
}

impl LaunchConfig {
    #[must_use]
    pub fn new(web_port: Option<u16>, browser: Browser) -> Self {
        Self {
            web_port,
            browser,
            ready_timeout: Duration::from_secs(15),
            settle: Duration::from_secs(3),
        }
    }
}

/// What a click asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// The mode of the app (its default, or the one that is there).
    Default,
    Native,
    Web,
    /// The first-start questions of an app: its program with no argument, in a console of its own.
    Setup,
}

/// What came of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A program was started.
    Started,
    /// A start of this app is on its way: nothing was done.
    Starting,
    /// A page was opened.
    Opened,
    /// The app cannot be used yet: nothing was done, the page says what to do.
    SetupRequired,
    /// The app was asked to stop (`<program> stop`); it is not waited for.
    Stopping,
    /// The app does not run: nothing was done.
    NotRunning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    UnknownApp,
    NotInstalled,
    /// The app has no such mode (`web`, `setup`).
    NotAvailable(&'static str),
    /// Something else answers on the port of the app.
    PortBusy,
    /// The listener of the web variants is not running.
    WebUnavailable,
    Failed(String),
}

/// The apps with a start on its way.
#[derive(Default)]
struct Inflight(Mutex<HashSet<&'static str>>);

/// Held while a start is on its way; letting go lets the next one in.
struct Flight {
    set: Arc<Inflight>,
    id: &'static str,
}

impl Drop for Flight {
    fn drop(&mut self) {
        if let Ok(mut set) = self.set.0.lock() {
            set.remove(self.id);
        }
    }
}

pub struct Launcher {
    detector: Arc<Detector>,
    cfg: LaunchConfig,
    inflight: Arc<Inflight>,
}

impl Launcher {
    #[must_use]
    pub fn new(detector: Arc<Detector>, cfg: LaunchConfig) -> Self {
        Self {
            detector,
            cfg,
            inflight: Arc::new(Inflight::default()),
        }
    }

    /// The entry of an app that is in the catalog and that the person did not hide.
    fn shown(&self, id: &str) -> Result<&'static Entry, LaunchError> {
        catalog::find(id)
            .filter(|e| !self.detector.hides(e.id))
            .ok_or(LaunchError::UnknownApp)
    }

    fn begin(&self, id: &'static str) -> Option<Flight> {
        let mut set = self.inflight.0.lock().ok()?;
        set.insert(id).then(|| Flight {
            set: Arc::clone(&self.inflight),
            id,
        })
    }

    /// Start `id` as asked.
    ///
    /// # Errors
    ///
    /// [`LaunchError`]: nothing was started.
    pub async fn launch(self: &Arc<Self>, id: &str, ask: Request) -> Result<Outcome, LaunchError> {
        let entry = self.shown(id)?;
        let Some(flight) = self.begin(entry.id) else {
            return Ok(Outcome::Starting);
        };
        let state = self.detector.look_one(entry).await;
        let mode = match ask {
            Request::Default => state.mode(),
            Request::Native | Request::Setup => Mode::Native,
            Request::Web => Mode::Web,
        };
        match mode {
            Mode::Web => self.open_web(&state, flight),
            Mode::Native => self.start_native(entry, &state, ask, flight).await,
        }
    }

    /// Ask an app that runs to stop. Only an app built on `appkit-hub` can be stopped, and only by its own
    /// program with the one argument `stop` (the app ends its own hub; the hub never reads its secret and
    /// never kills a process of its own accord). The program is the one the detection found.
    ///
    /// # Errors
    ///
    /// [`LaunchError`]: nothing was done.
    pub async fn stop(self: &Arc<Self>, id: &str) -> Result<Outcome, LaunchError> {
        let entry = self.shown(id)?;
        if !matches!(entry.native, Some(Native::AppKitHub { .. })) {
            return Err(LaunchError::NotAvailable("stop"));
        }
        let Some(flight) = self.begin(entry.id) else {
            return Ok(Outcome::Starting);
        };
        let state = self.detector.look_one(entry).await;
        let program = state.native.clone().ok_or(LaunchError::NotInstalled)?;
        if state.run != Some(Run::Running) {
            return Ok(Outcome::NotRunning);
        }
        let cwd = program
            .parent()
            .ok_or_else(|| LaunchError::Failed("the program has no folder".to_owned()))?
            .to_path_buf();
        spawn(
            &program,
            &["stop"],
            &LaunchOpts {
                cwd: &cwd,
                window: Window::Default,
            },
        )
        .map_err(LaunchError::Failed)?;
        self.wait_until_stopped(entry, flight);
        Ok(Outcome::Stopping)
    }

    /// Until the app no longer answers (or the time is up); the next look then sees it.
    fn wait_until_stopped(self: &Arc<Self>, entry: &'static Entry, flight: Flight) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let end = tokio::time::Instant::now() + this.cfg.ready_timeout;
            while tokio::time::Instant::now() < end {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if this.detector.look_one(entry).await.run != Some(Run::Running) {
                    break;
                }
            }
            drop(flight);
        });
    }

    /// The web variant, served by the listener of the hub.
    fn open_web(
        self: &Arc<Self>,
        state: &AppState,
        flight: Flight,
    ) -> Result<Outcome, LaunchError> {
        let web = state.entry.web.ok_or(LaunchError::NotAvailable("web"))?;
        if state.web.is_none() {
            return Err(LaunchError::NotInstalled);
        }
        let port = self.cfg.web_port.ok_or(LaunchError::WebUnavailable)?;
        let url = format!("http://127.0.0.1:{port}/{}/{}", web.repo_dir, web.entry);
        self.cfg
            .browser
            .open(&url, &[port])
            .map_err(LaunchError::Failed)?;
        self.hold(flight, self.cfg.settle);
        Ok(Outcome::Opened)
    }

    async fn start_native(
        self: &Arc<Self>,
        entry: &'static Entry,
        state: &AppState,
        ask: Request,
        flight: Flight,
    ) -> Result<Outcome, LaunchError> {
        let native = entry
            .native
            .as_ref()
            .ok_or(LaunchError::NotAvailable("native"))?;
        let program = state.native.clone().ok_or(LaunchError::NotInstalled)?;
        let cwd = program
            .parent()
            .ok_or_else(|| LaunchError::Failed("the program has no folder".to_owned()))?
            .to_path_buf();
        let run = |args: &[&str], window: Window| {
            spawn(&program, args, &LaunchOpts { cwd: &cwd, window }).map_err(LaunchError::Failed)
        };
        match native {
            Native::AppKitHub { .. } => {
                if ask == Request::Setup {
                    // The questions of a first start: the program as a person starts it.
                    run(&[], Window::NewConsole)?;
                    self.hold(flight, self.cfg.settle);
                    return Ok(Outcome::Started);
                }
                if state.setup.is_some() {
                    return Ok(Outcome::SetupRequired);
                }
                // One command for both cases: it starts the hub when it does not run and opens a paired page
                // either way, so there is nothing to look at first (and nothing to race with).
                run(&["start", "--open"], Window::Default)?;
                self.wait_until_running(entry, flight);
                Ok(Outcome::Started)
            }
            _ if ask == Request::Setup => Err(LaunchError::NotAvailable("setup")),
            Native::GuiExe { .. } => {
                run(&[], Window::Default)?;
                self.hold(flight, self.cfg.settle);
                Ok(Outcome::Started)
            }
            Native::LoopbackServer { .. } => {
                let port = state
                    .port
                    .ok_or_else(|| LaunchError::Failed("no port".to_owned()))?;
                match state.run {
                    Some(Run::Running) => {
                        self.open_page(port)?;
                        self.hold(flight, self.cfg.settle);
                        Ok(Outcome::Opened)
                    }
                    Some(Run::PortBusy) => Err(LaunchError::PortBusy),
                    _ => {
                        run(&[], Window::NewConsole)?;
                        self.open_when_serving(entry, port, flight);
                        Ok(Outcome::Started)
                    }
                }
            }
        }
    }

    fn open_page(&self, port: u16) -> Result<(), LaunchError> {
        self.cfg
            .browser
            .open(&format!("http://127.0.0.1:{port}/"), &[port])
            .map_err(LaunchError::Failed)
    }

    /// Keep the start on its way for `time`, then let the next one in.
    fn hold(&self, flight: Flight, time: Duration) {
        tokio::spawn(async move {
            tokio::time::sleep(time).await;
            drop(flight);
        });
    }

    /// Until the app answers (or the time is up); the next look then sees it.
    fn wait_until_running(self: &Arc<Self>, entry: &'static Entry, flight: Flight) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let end = tokio::time::Instant::now() + this.cfg.ready_timeout;
            while tokio::time::Instant::now() < end {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if this.detector.look_one(entry).await.run == Some(Run::Running) {
                    break;
                }
            }
            drop(flight);
        });
    }

    /// A program that serves a page: when it says `/health`, the page is opened in the browser.
    fn open_when_serving(self: &Arc<Self>, entry: &'static Entry, port: u16, flight: Flight) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let end = tokio::time::Instant::now() + this.cfg.ready_timeout;
            while tokio::time::Instant::now() < end {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if this.detector.look_one(entry).await.run == Some(Run::Running) {
                    let _ = this.open_page(port);
                    break;
                }
            }
            drop(flight);
        });
    }
}

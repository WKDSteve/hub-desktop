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
//!
//! The one secret the hub reads is the control secret of a server whose page wants a ticket (ai-desktop,
//! `catalog::TicketLogin`), and it sends it only to that server, after `/health` named the instance of its
//! `hub.json`. The secrets of the apps built on `appkit-hub` stay theirs.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_core::registry;
use appkit_hub::launch::{spawn, spawn_with, LaunchOpts, Window};
use serde_json::Value;

use crate::catalog::{self, Entry, Mode, Native, TicketLogin};
use crate::client;
use crate::config::Env;
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
    /// The first start of an app of the kit: its page (`start --open`), where it asks for what it misses.
    Setup,
    /// The hub of an app of the kit in the background, without a page (`<program> start`): what
    /// hub-desktop does for the apps of `start_with_hub` when it starts (the login). Nothing else
    /// has this mode.
    Background,
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
    /// The hub of the app runs already: nothing was done (a start in the background).
    Running,
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
pub struct Flight {
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

    /// Hold the app for something that is not a start (an update): while it is held, a click gets
    /// `starting` and no program. `None`: a start or stop of it is on its way.
    #[must_use]
    pub fn reserve(&self, id: &'static str) -> Option<Flight> {
        self.begin(id)
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
            Request::Native | Request::Setup | Request::Background => Mode::Native,
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
            Native::AppKitHub { identity, .. } => {
                if ask == Request::Background {
                    // An app that is not set up waits for a person: its first start is in its page, which nobody
                    // asked for at the login.
                    if state.setup.is_some() {
                        drop(flight);
                        return Ok(Outcome::SetupRequired);
                    }
                    // A hub that runs is left alone: no program is started for nothing.
                    if state.run == Some(Run::Running) {
                        drop(flight);
                        return Ok(Outcome::Running);
                    }
                    // Its hub only: a page nobody asked for would open at every login. It is told to stay
                    // without a page (`<APP>_STAY`, which its `start` passes on to the hub), as a hub that
                    // its own login entry started.
                    let stay = appkit_hub::background::stay_var(identity);
                    spawn_with(
                        &program,
                        &["start"],
                        &LaunchOpts {
                            cwd: &cwd,
                            window: Window::Default,
                        },
                        &[(stay.as_str(), "1")],
                    )
                    .map_err(LaunchError::Failed)?;
                    self.wait_until_running(entry, flight);
                    return Ok(Outcome::Started);
                }
                // One command for all cases: it starts the hub when it does not run and opens a paired page
                // either way, so there is nothing to look at first (and nothing to race with). An app that is not
                // set up (and `setup`, asked for) gets the same: its first start is in its page, never in a console.
                run(&["start", "--open"], Window::Default)?;
                self.wait_until_running(entry, flight);
                Ok(Outcome::Started)
            }
            _ if ask == Request::Setup => Err(LaunchError::NotAvailable("setup")),
            _ if ask == Request::Background => Err(LaunchError::NotAvailable("background")),
            Native::GuiExe { .. } => {
                run(&[], Window::Default)?;
                self.hold(flight, self.cfg.settle);
                Ok(Outcome::Started)
            }
            Native::LoopbackServer { login, .. } => {
                let port = state
                    .port
                    .ok_or_else(|| LaunchError::Failed("no port".to_owned()))?;
                match state.run {
                    Some(Run::Running) => {
                        self.open_page(*login, port).await?;
                        self.hold(flight, self.cfg.settle);
                        Ok(Outcome::Opened)
                    }
                    Some(Run::PortBusy) => Err(LaunchError::PortBusy),
                    _ => {
                        run(&[], Window::NewConsole)?;
                        self.open_when_serving(entry, port, *login, flight);
                        Ok(Outcome::Started)
                    }
                }
            }
        }
    }

    /// Open the page of a server, signed in when it wants a ticket and one can be had. Without one it opens
    /// as it is and says "not signed in" itself: a missing ticket is no reason to open nothing.
    async fn open_page(&self, login: Option<TicketLogin>, port: u16) -> Result<(), LaunchError> {
        let mut url = format!("http://127.0.0.1:{port}/");
        if let Some(login) = login
            && let Ok(ticket) = ticket(&login, self.detector.env(), port).await
        {
            url.push_str("#ticket=");
            url.push_str(&ticket);
        }
        self.cfg
            .browser
            .open(&url, &[port])
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
    fn open_when_serving(
        self: &Arc<Self>,
        entry: &'static Entry,
        port: u16,
        login: Option<TicketLogin>,
        flight: Flight,
    ) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let end = tokio::time::Instant::now() + this.cfg.ready_timeout;
            while tokio::time::Instant::now() < end {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if this.detector.look_one(entry).await.run == Some(Run::Running) {
                    let _ = this.open_page(login, port).await;
                    break;
                }
            }
            drop(flight);
        });
    }
}

/// The folder of the server's `hub.json` and `control.secret`, decided as the server decides it
/// (ai-desktop `family::config_dir`): the variable, else its folder below `%APPDATA%`.
fn login_dir(login: &TicketLogin, env: &Env) -> Option<PathBuf> {
    if let Some(dir) = env
        .path(login.dir_var)
        .filter(|d| !d.as_os_str().is_empty())
    {
        return Some(dir);
    }
    env.path("APPDATA")
        .filter(|d| !d.as_os_str().is_empty())
        .map(|base| base.join(login.folder))
}

/// A one-time ticket for the page of the server on `port`. The control secret goes only to the process that
/// says it is the server of `hub.json`: the same app and the same instance (a file left by a server that was
/// killed, or another program on the port, gets nothing).
async fn ticket(login: &TicketLogin, env: &Env, port: u16) -> Result<String, String> {
    let dir = login_dir(login, env).ok_or("no config folder")?;
    let record = registry::read_record(&dir)
        .map_err(|e| e.to_string())?
        .ok_or("no hub.json")?;
    if record.app != login.app || record.port != port {
        return Err("hub.json names another app or port".to_owned());
    }
    let health = client::request(port, "GET", "/health", &[], "")
        .await
        .map_err(|e| e.to_string())?;
    let doc: Value = serde_json::from_str(&health.body).map_err(|_| "no JSON from /health")?;
    if health.status != 200
        || doc["app"].as_str() != Some(login.app)
        || doc["instance"].as_str() != Some(record.instance.as_str())
    {
        return Err("the server on the port is not the one of hub.json".to_owned());
    }
    let secret = registry::read_secret(&dir)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.is_empty())
        .ok_or("no control secret")?;
    let auth = format!("Bearer {secret}");
    let reply = client::request(
        port,
        "POST",
        login.route,
        &[
            ("Authorization", &auth),
            ("Content-Type", "application/json"),
        ],
        "{}",
    )
    .await
    .map_err(|e| e.to_string())?;
    if reply.status != 200 {
        return Err(format!("no ticket (HTTP {})", reply.status));
    }
    let doc: Value = serde_json::from_str(&reply.body).map_err(|_| "no JSON for the ticket")?;
    let ticket = doc["ticket"].as_str().ok_or("no ticket in the answer")?;
    // It goes into an address the browser opens: hex and nothing else.
    if ticket.is_empty() || ticket.len() > 128 || !ticket.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the ticket is not hex".to_owned());
    }
    Ok(ticket.to_owned())
}

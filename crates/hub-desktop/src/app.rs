//! The commands of the program: `serve`, `open` and what a double click does.
//!
//! They live in the library so that their parts can be tested; `main.rs` only reads the arguments.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use appkit_core::authn::{Secret, SystemClock, CODE_BYTES, TOKEN_BYTES};
use appkit_core::registry::{self, HubRecord};
use serde_json::Value;

use crate::client;
use crate::config::{self, Env};
use crate::detect::Detector;
use crate::launch::{Browser, LaunchConfig, Launcher};
use crate::machine::Role;
use crate::monitor::Monitor;
use crate::overlay::Overlay;
use crate::prefs::Prefs;
use crate::server::{self, ServerConfig};
use crate::ui::UiFiles;
use crate::update::{SystemRunner, Tools, UpdateConfig, Updater};
use crate::webserve::{self, WebServer};
use appkit_hub::Limits;

/// The default port of the listener of the web variants (`appkit-testkit`'s list of ports).
pub const WEB_PORT: u16 = 17611;

/// What to do when the hub is started.
#[derive(Debug, Clone, Default)]
pub struct ServeOptions {
    /// Overrides the settings file and the environment.
    pub port: Option<u16>,
    /// Open the browser with a one-time pairing link once the hub runs (a double click does).
    pub open: bool,
    /// A hub that `start` put in the background without an icon (`serve` where there is no tray) ends itself once
    /// no page has been connected for a while and nothing asks it to stay (`background::left_alone`). The tray
    /// passes `false`: its icon shows it, and it ends from its menu.
    pub alone: bool,
}

/// The folder for `hub.json` and `control.secret`.
fn app_dir(env: &Env) -> Result<PathBuf, String> {
    appkit_hub::instance::config_folder(env, &crate::IDENTITY)
}

/// The port to try first: the option, the environment, the settings file, then the default.
fn wanted_port(opts: &ServeOptions, file: &config::FileConfig, env: &Env) -> Result<u16, String> {
    if let Some(p) = opts.port {
        return Ok(p);
    }
    let port_var = crate::IDENTITY.var("PORT");
    if let Some(v) = env.get(&port_var) {
        let text = v.to_string_lossy();
        return text
            .parse::<u16>()
            .ok()
            .filter(|p| *p >= 1024)
            .ok_or_else(|| format!("{port_var} is not a port from 1024 to 65535: {text:?}"));
    }
    Ok(file.port.unwrap_or(crate::IDENTITY.default_port))
}

/// Is a hub of this app running according to `hub.json`? It is only believed when the process on
/// that port says the same instance.
pub(crate) async fn running_hub(dir: &std::path::Path) -> Option<HubRecord> {
    appkit_hub::instance::running_hub(dir, &crate::IDENTITY).await
}

/// Wait for Ctrl-C, or for SIGTERM where there is one.
async fn wait_for_stop() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

/// What the hub hands to the icon in the notification area once it serves.
#[derive(Clone)]
pub struct Ready {
    pub port: u16,
    pub detector: Arc<Detector>,
    pub launcher: Arc<Launcher>,
    /// The apps whose hub starts with hub-desktop (`start_with_hub` of `config.json`).
    pub start_with_hub: Vec<String>,
    /// What the CI watcher is at (the icon counts its hints).
    pub ci: Arc<crate::ciwatch::Watcher>,
    /// The runners of this machine (none: not here, see `runners.rs`).
    pub runners: Option<Arc<crate::runners::RunnerWatch>>,
}

/// Run the server until Ctrl-C, or until `<program> stop` asks it to end.
///
/// # Errors
///
/// A text for the person at the terminal: what is missing or in the way.
pub async fn serve(opts: ServeOptions) -> Result<(), String> {
    serve_until(opts, wait_for_stop(), |_| {}).await
}

/// Run the server until `stop` resolves or `<program> stop` asks it to end (the control route); `ready`
/// is called once, when it serves and `hub.json` names it.
///
/// # Errors
///
/// A text for the person at the terminal: what is missing or in the way.
pub async fn serve_until(
    opts: ServeOptions,
    stop: impl std::future::Future<Output = ()>,
    ready: impl FnOnce(Ready),
) -> Result<(), String> {
    let env = Env::from_process();
    let dir = app_dir(&env)?;
    let file = config::load_file(&dir).map_err(|e| e.to_string())?;
    let want = wanted_port(&opts, &file, &env)?;

    if let Some(other) = running_hub(&dir).await {
        return Err(format!(
            "a hub is already running (port {}, process {})",
            other.port, other.pid
        ));
    }

    let control = Secret::generate(TOKEN_BYTES).map_err(|e| e.to_string())?;
    let instance = Secret::generate(CODE_BYTES).map_err(|e| e.to_string())?;
    let chosen = file
        .app
        .tiles
        .iter()
        .filter_map(|(id, tile)| Some((id.clone(), config::mode_of(tile.mode.as_deref()?)?)))
        .collect();
    // The overlay is the person's file; one that cannot be used never stops the hub.
    let overlay = Overlay::load(&dir).unwrap_or_else(|e| {
        eprintln!("note: {e}; the hub runs without it");
        Overlay::default()
    });
    // `config.json` names the folder first; the overlay only fills in when it does not.
    let web_root = file
        .app
        .web_root
        .clone()
        .or_else(|| overlay.web_root.clone());
    // The start with the login of an app: the one state of the kit, which the app reads and switches the same
    // way (its own login entry, or `start_with_hub` of this hub while it starts with the login).
    let reading = env.clone();
    let writing = env.clone();
    let detector = Arc::new(
        Detector::new(env.clone(), web_root)
            .with_modes(chosen)
            .with_overlay(overlay)
            .with_autostart(move |identity| {
                appkit_hub::loginstate::is_on(
                    identity,
                    &reading,
                    &appkit_hub::loginstate::SystemLogin,
                )
            }),
    );
    let prefs = Arc::new(
        Prefs::new(Some(dir.clone()), Arc::clone(&detector)).with_login(move |identity, on| {
            appkit_hub::loginstate::set(
                identity,
                on,
                || appkit_hub::loginstate::installed_program(identity, &writing),
                &writing,
                &appkit_hub::loginstate::SystemLogin,
            )
            .map(|_| ())
        }),
    );
    // The pages of the web variants have a listener (and an origin) of their own. When its port is taken
    // the hub runs without it: the apps start, the web mode says why it cannot.
    let web = match webserve::start(wanted_web_port(&env)?, Arc::clone(&detector)).await {
        Ok(web) => Some(web),
        Err(e) => {
            eprintln!("note: the web variants are not served: {e}");
            None
        }
    };
    let browser = match env.path(&crate::IDENTITY.var("BROWSER_LOG")) {
        Some(file) => Browser::Log(file),
        None => Browser::System,
    };
    let launcher = Arc::new(Launcher::new(
        Arc::clone(&detector),
        LaunchConfig::new(web.as_ref().map(|w| w.addr().port()), browser),
    ));
    let monitor = Arc::new(Monitor::new(file.app.testing_export.clone()));
    let (ci, ci_programs) = ci_watcher(&env);
    let runner_watch = runner_watch(&env, ci_programs.clone());
    // What an update may do here: the role of the machine and the programs are read once, now.
    let updater = Arc::new(Updater::new(
        Arc::clone(&detector),
        Arc::clone(&launcher),
        UpdateConfig::new(Role::of(&env), Tools::find(&env), Some(dir.join("updates")))
            .requiring_attestation(file.app.update.require_attestation),
        Arc::new(SystemRunner),
    ));
    let make_cfg = |port: u16| ServerConfig {
        addr: std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        limits: Limits::default(),
        clock: Arc::new(SystemClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: control.clone(),
        instance: instance.clone(),
        ui: Arc::new(UiFiles::embedded()),
        detector: Arc::clone(&detector),
        launcher: Arc::clone(&launcher),
        prefs: Arc::clone(&prefs),
        monitor: Arc::clone(&monitor),
        updater: Arc::clone(&updater),
        ci: Arc::clone(&ci),
    };
    // The port is part of the origin, so the wanted one is tried first. When it is taken the server
    // still starts, on another port, and says so: the page then has another origin.
    let running = match server::start(make_cfg(want)).await {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Two hubs that start at the same moment both passed the check above; the bind is what
            // decides. Another hub of this app on the port is "already running", not a reason for a
            // second one.
            if appkit_hub::instance::hello_of(want, &crate::IDENTITY)
                .await
                .is_some()
            {
                stop_web(web).await;
                return Err(format!("a hub is already running on port {want}"));
            }
            eprintln!("note: port {want} is in use, using a free port; the origin changes");
            match server::start(make_cfg(0)).await {
                Ok(r) => r,
                Err(e) => {
                    stop_web(web).await;
                    return Err(e.to_string());
                }
            }
        }
        Err(e) => {
            stop_web(web).await;
            return Err(e.to_string());
        }
    };
    let port = running.addr().port();

    let record = HubRecord {
        app: crate::IDENTITY.app.to_owned(),
        port,
        pid: std::process::id(),
        version: crate::VERSION.to_owned(),
        instance: running.instance().to_owned(),
    };
    let written = registry::write_secret(&dir, &control.to_hex())
        .and_then(|()| registry::write_record(&dir, &record));
    if let Err(e) = written {
        // Leave nothing half written behind, and do not serve unannounced.
        registry::remove_secret(&dir);
        registry::remove_record(&dir);
        running.shutdown().await;
        stop_web(web).await;
        return Err(e.to_string());
    }

    println!(
        "{} {} on http://127.0.0.1:{port}/",
        crate::IDENTITY.app,
        crate::VERSION
    );
    println!(
        "run `{} open` for a pairing link; Ctrl-C stops the hub",
        crate::IDENTITY.binary
    );
    if opts.open {
        // The hub runs already: a browser that cannot be opened is a note, not a reason to stop it.
        if let Err(e) = open().await.and_then(|url| deliver(&url)) {
            eprintln!("note: the browser was not opened: {e}");
        }
    }

    ready(Ready {
        port,
        detector: Arc::clone(&detector),
        launcher: Arc::clone(&launcher),
        start_with_hub: file.app.start_with_hub.clone(),
        ci: Arc::clone(&ci),
        runners: runner_watch.clone(),
    });
    let watching_runners = runner_watch.map(|watch| {
        let prefs = Arc::clone(&prefs);
        tokio::spawn(crate::runners::watch(
            watch,
            Arc::new(move || (prefs.ci_schedule().0.interval_min, prefs.runner_auto())),
            crate::ciwatch::FIRST_ROUND,
            crate::ciwatch::TICK,
            crate::ciwatch::MINUTE,
        ))
    });
    let watching = ci_programs.map(|programs| {
        tokio::spawn(crate::ciwatch::watch(
            Arc::clone(&ci),
            Arc::new(crate::ciwatch::SystemAsk),
            programs,
            ci_settings(Arc::clone(&prefs)),
            crate::ciwatch::FIRST_ROUND,
            crate::ciwatch::TICK,
            crate::ciwatch::MINUTE,
        ))
    });
    let presence = running.presence();
    tokio::select! {
        () = stop => {}
        () = until_over(&running, &presence, &dir, &env, opts.alone) => {}
    }
    for watching in [watching, watching_runners].into_iter().flatten() {
        watching.abort();
    }
    registry::remove_record(&dir);
    registry::remove_secret(&dir);
    running.shutdown().await;
    stop_web(web).await;
    Ok(())
}

/// The CI watcher (`ciwatch.rs`) and, when it may run, its programs: never for a test (`ciwatch::allowed`). Whether
/// it is switched on and how often it runs, it reads from `config.json` before every round ([`ci_settings`]). A
/// missing program is a note, not a reason to stop the hub; the page says why it does not run.
fn ci_watcher(
    env: &Env,
) -> (
    Arc<crate::ciwatch::Watcher>,
    Option<crate::ciwatch::Programs>,
) {
    let found = crate::ciwatch::allowed(env).and_then(|()| crate::ciwatch::Programs::find(env));
    match found {
        Ok(programs) => (Arc::new(crate::ciwatch::Watcher::default()), Some(programs)),
        Err(why) => {
            eprintln!("note: the CI watcher does not run: {why}");
            (Arc::new(crate::ciwatch::Watcher::off(why)), None)
        }
    }
}

/// The runners of this machine (`runners.rs`): on the private machine only, never for a test. Without the
/// programs of the CI watcher it shows their state but sees no waiting push.
fn runner_watch(
    env: &Env,
    programs: Option<crate::ciwatch::Programs>,
) -> Option<Arc<crate::runners::RunnerWatch>> {
    let found = crate::ciwatch::allowed(env)
        .and_then(|()| crate::runners::Runners::find(env, Role::of(env)));
    match found {
        Ok(runners) => Some(Arc::new(crate::runners::RunnerWatch::new(
            Arc::new(crate::ciwatch::SystemAsk),
            runners,
            programs,
        ))),
        Err(why) => {
            eprintln!("note: the runners are not shown: {why}");
            None
        }
    }
}

/// The settings of the CI watcher, read from `config.json` each time; a note (an interval outside the bounds, a file
/// that cannot be read) is said once, when it changes.
fn ci_settings(prefs: Arc<Prefs>) -> crate::ciwatch::Settings {
    let said = std::sync::Mutex::new(None::<String>);
    Arc::new(move || {
        let (schedule, note) = prefs.ci_schedule();
        if let Ok(mut said) = said.lock()
            && *said != note
        {
            if let Some(note) = &note {
                eprintln!("note: {note}");
            }
            *said = note;
        }
        schedule
    })
}

/// Resolves when the hub is to end besides `stop`: `<program> stop` asked over the control route (without this it
/// ends the hub the hard way), or, for a hub in the background without an icon (`alone`), being left alone. The
/// settings are read then, not at the start: an order to stay (`keep_running`) may be new.
async fn until_over(
    running: &appkit_hub::RunningServer,
    presence: &appkit_hub::presence::Presence,
    dir: &std::path::Path,
    env: &Env,
    alone: bool,
) {
    if !alone || !appkit_hub::background::runs_detached(&crate::background::APP, env) {
        return presence.asked_to_end().await;
    }
    let stay = || {
        let keep_running = match config::load_file(dir) {
            Ok(file) => file.keep_running.unwrap_or(false),
            // Settings that cannot be read are no reason to go: the order may stand in them.
            Err(e) => {
                println!("the settings cannot be read, so the hub stays: {e}");
                true
            }
        };
        appkit_hub::background::has_order(
            &crate::background::APP,
            env,
            keep_running,
            running.paired_devices(),
        )
    };
    tokio::select! {
        () = presence.asked_to_end() => {}
        () = appkit_hub::background::left_alone(&crate::background::APP, env, presence, stay) => {
            println!("no page is connected and nothing asks the hub to stay: it ends");
        }
    }
}

/// The port of the listener of the web variants: `HUB_DESKTOP_WEB_PORT`, else the default.
fn wanted_web_port(env: &Env) -> Result<u16, String> {
    let var = crate::IDENTITY.var("WEB_PORT");
    match env.get(&var) {
        None => Ok(WEB_PORT),
        Some(v) => v
            .to_string_lossy()
            .parse::<u16>()
            .ok()
            .filter(|p| *p >= 1024)
            .ok_or_else(|| format!("{var} is not a port from 1024 to 65535")),
    }
}

async fn stop_web(web: Option<WebServer>) {
    if let Some(web) = web {
        web.shutdown().await;
    }
}

/// What a double click on the window program does (the release build, and the link `install` makes): it
/// has no window to run the hub in, so the hub goes into the background (log in the config folder) unless
/// one runs, and the browser opens with a one-time link, as `start --open` does.
///
/// # Errors
///
/// A text for the person: the console of the terminal, or a message box.
pub async fn click_in_background() -> Result<(), String> {
    start_and_open().await
}

/// `start --open`, and the double click of the window program: the hub in the background, then the page. A hub this
/// call started for the page does not stay behind when the page cannot be opened: nobody would see it (on Windows its
/// icon would, but a start that failed to show anything is not what the person asked for). One that ran already is not
/// this call's to stop.
///
/// # Errors
///
/// A text for the person at the terminal.
pub async fn start_and_open() -> Result<(), String> {
    let started = crate::background::start().await?;
    print!("{started}");
    open_or_stop(
        &started,
        async { open().await.and_then(|url| deliver(&url)) },
        crate::background::stop,
    )
    .await
}

/// Open the page (`page`) of the hub that was just started or found (`started` is what the start printed); when it
/// cannot be opened and the start started the hub, `stop` it again.
///
/// # Errors
///
/// Why the page was not opened, and what came of the stop.
pub async fn open_or_stop<P, S, F>(started: &str, page: P, stop: S) -> Result<(), String>
where
    P: std::future::Future<Output = Result<(), String>>,
    S: FnOnce() -> F,
    F: std::future::Future<Output = Result<String, String>>,
{
    let Err(why) = page.await else {
        return Ok(());
    };
    if !appkit_hub::background::started_now(started) {
        return Err(why);
    }
    Err(match stop().await {
        Ok(_) => format!("{why}; the hub that was started for the page is stopped again"),
        Err(left) => format!("{why}; the hub that was started for the page still runs: {left}"),
    })
}

/// What a double click on a console program does (a debug build): the hub runs in this window and the
/// browser opens with a one-time link. A hub that runs already is not started twice; the browser opens
/// on it, so a second click finds the first.
///
/// # Errors
///
/// A text for the person at the terminal.
pub async fn click() -> Result<(), String> {
    let env = Env::from_process();
    let dir = app_dir(&env)?;
    if running_hub(&dir).await.is_some() {
        let url = open().await?;
        return deliver(&url);
    }
    serve(ServeOptions {
        port: None,
        open: true,
        alone: true,
    })
    .await
}

/// Print the link (`HUB_DESKTOP_NO_LAUNCH` is set: the tests) or open it in the default browser.
///
/// # Errors
///
/// The link is not the hub's own, or the program that opens links could not be started.
pub fn deliver(url: &str) -> Result<(), String> {
    if std::env::var_os(crate::IDENTITY.var("NO_LAUNCH")).is_some() {
        println!("{url}");
        Ok(())
    } else {
        launch(url)
    }
}

/// Ask the running hub for a pairing code and return the link for it.
///
/// # Errors
///
/// A text for the person at the terminal.
pub async fn open() -> Result<String, String> {
    let env = Env::from_process();
    let dir = app_dir(&env)?;
    let record = registry::read_record(&dir)
        .map_err(|e| e.to_string())?
        .ok_or("no hub is running (no hub.json)")?;
    if record.app != crate::IDENTITY.app {
        return Err("hub.json is not from this app".to_owned());
    }
    let secret = registry::read_secret(&dir)
        .map_err(|e| e.to_string())?
        .ok_or("no control secret: the hub is not running")?;

    // The secret goes only to the process that says it is this hub.
    let hello = client::request(record.port, "GET", &crate::IDENTITY.route("hello"), &[], "")
        .await
        .map_err(|e| format!("the hub does not answer on port {}: {e}", record.port))?;
    let doc: Value = serde_json::from_str(&hello.body).map_err(|_| "not a hub on that port")?;
    if doc["app"] != crate::IDENTITY.app
        || doc["instance"].as_str() != Some(record.instance.as_str())
    {
        return Err(format!(
            "port {} is not the hub of hub.json (a stale file, or another program)",
            record.port
        ));
    }

    let auth = format!("Bearer {secret}");
    let reply = client::request(
        record.port,
        "POST",
        "/api/control/pair",
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &auth),
        ],
        "{}",
    )
    .await
    .map_err(|e| e.to_string())?;
    if reply.status != 200 {
        return Err(format!(
            "the hub refused the request (HTTP {})",
            reply.status
        ));
    }
    let doc: Value = serde_json::from_str(&reply.body).map_err(|e| e.to_string())?;
    let code = doc["code"].as_str().ok_or("the hub sent no code")?;
    Ok(format!("http://127.0.0.1:{}/#pair={code}", record.port))
}

/// Is this the link `open` prints: the loopback address, a port, `/#pair=` and hex, and nothing else a
/// program could take as an option or a path?
#[must_use]
pub fn is_own_link(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let Some((port, code)) = rest.split_once("/#pair=") else {
        return false;
    };
    !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|b| b.is_ascii_digit())
        && !code.is_empty()
        && code.len() <= 128
        && code.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Open a link of `open` in the default browser. The link is the hub's own
/// (`http://127.0.0.1:<port>/#pair=<hex>`), and nothing else is passed.
///
/// # Errors
///
/// The link is not the hub's own, or the program that opens links could not be started.
pub fn launch(url: &str) -> Result<(), String> {
    if !is_own_link(url) {
        return Err("the link is not the hub's own: not opened".to_owned());
    }
    // The port of the link is the only one the browser helper is allowed to open.
    let port = url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.split_once('/'))
        .and_then(|(port, _)| port.parse::<u16>().ok())
        .ok_or("the link has no port: not opened")?;
    appkit_hub::browser::open(url, &[port])
}

#[cfg(test)]
mod tests {
    /// The hub a start started for the page is stopped again when the page cannot be opened; one that ran already, or a
    /// page that opened, leaves the hub as it is.
    #[tokio::test]
    async fn a_hub_started_for_a_page_that_cannot_be_opened_is_stopped_again() {
        use std::cell::Cell;
        let stops = Cell::new(0);
        let stop = || {
            stops.set(stops.get() + 1);
            async { Ok::<_, String>("stopped".to_owned()) }
        };
        let started = "the hub runs in the background (port 1, process 2)";
        let failed = open_or_stop(started, async { Err("no browser".to_owned()) }, stop)
            .await
            .unwrap_err();
        assert!(failed.contains("stopped again"), "{failed}");
        assert_eq!(stops.get(), 1);
        let running = "a hub is already running (port 1, process 2)";
        let kept = open_or_stop(running, async { Err("no browser".to_owned()) }, stop)
            .await
            .unwrap_err();
        assert_eq!(kept, "no browser");
        open_or_stop(started, async { Ok(()) }, stop).await.unwrap();
        assert_eq!(stops.get(), 1);
    }

    use super::*;

    #[test]
    fn only_the_pairing_link_of_the_loopback_is_the_hubs_own() {
        assert!(is_own_link("http://127.0.0.1:17610/#pair=00ff"));
        for bad in [
            "",
            "http://127.0.0.1:17610/",
            "http://127.0.0.1:17610/#pair=",
            "http://127.0.0.1:17610/#pair=xyz",
            "http://127.0.0.1:/#pair=00",
            "http://127.0.0.1:123456/#pair=00",
            "http://127.0.0.1:17610/#pair=00 --new-window",
            "http://localhost:17610/#pair=00",
            "https://127.0.0.1:17610/#pair=00",
            "file:///C:/Windows/System32/calc.exe",
            "http://127.0.0.1:17610@evil.example/#pair=00",
        ] {
            assert!(!is_own_link(bad), "{bad:?}");
            assert!(launch(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_port_comes_from_the_option_then_the_environment_then_the_file_then_the_default() {
        let none = Env::from_pairs([("APPDATA", "C:/a")]);
        let file = config::FileConfig::default();
        let opts = ServeOptions::default();
        assert_eq!(wanted_port(&opts, &file, &none), Ok(17610));

        let with_port = config::FileConfig {
            port: Some(20000),
            ..config::FileConfig::default()
        };
        assert_eq!(wanted_port(&opts, &with_port, &none), Ok(20000));

        let env = Env::from_pairs([("HUB_DESKTOP_PORT", "21000")]);
        assert_eq!(wanted_port(&opts, &with_port, &env), Ok(21000));

        let given = ServeOptions {
            port: Some(22000),
            open: false,
            alone: false,
        };
        assert_eq!(wanted_port(&given, &with_port, &env), Ok(22000));
    }

    #[test]
    fn a_port_in_the_environment_must_be_a_usable_one() {
        for bad in ["", "80", "99999", "x"] {
            let env = Env::from_pairs([("HUB_DESKTOP_PORT", bad)]);
            let got = wanted_port(
                &ServeOptions::default(),
                &config::FileConfig::default(),
                &env,
            );
            // An empty variable counts as not set.
            if bad.is_empty() {
                assert_eq!(got, Ok(17610));
            } else {
                assert!(got.is_err(), "{bad:?}");
            }
        }
    }
}

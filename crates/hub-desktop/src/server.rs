//! The server of the hub: the `appkit-hub` core, a runtime that holds the look at the machine, and the
//! routes of the hub below `/api/hub/`.
//!
//! The hub has no vault and no engine, so it does not call `setup::run` and hands the core a runtime
//! without vaults (`vaults: None`). Its routes are closed to anything without a session; `hello` is the
//! only one that is open.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use appkit_core::authn::{Clock, Secret};
use appkit_hub::{
    serve, Core, GuardRules, HubStatus, Limits, Pages, RunningServer, Runtime, ServeParts,
};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::detect::Detector;
use crate::launch::{LaunchError, Launcher, Outcome, Request};
use crate::monitor::Monitor;
use crate::prefs::{Prefs, PrefsError};
use crate::IDENTITY;

/// What the hub keeps: the status `hello` reports (it has no engine) and the way to look at the machine.
/// It starts and stops nothing.
pub struct HubRuntime {
    status: HubStatus,
    detector: Arc<Detector>,
    launcher: Arc<Launcher>,
    prefs: Arc<Prefs>,
    monitor: Arc<Monitor>,
}

impl HubRuntime {
    /// The runtime of a hub that has no engine: `hello` says it is not ready for one, and that is right.
    #[must_use]
    pub fn new(
        detector: Arc<Detector>,
        launcher: Arc<Launcher>,
        prefs: Arc<Prefs>,
        monitor: Arc<Monitor>,
    ) -> Self {
        Self {
            status: HubStatus {
                engine_ready: false,
                problem: None,
                capabilities: Vec::new(),
            },
            detector,
            launcher,
            prefs,
            monitor,
        }
    }

    #[must_use]
    pub fn prefs(&self) -> &Arc<Prefs> {
        &self.prefs
    }

    #[must_use]
    pub fn launcher(&self) -> &Arc<Launcher> {
        &self.launcher
    }

    #[must_use]
    pub fn monitor(&self) -> &Arc<Monitor> {
        &self.monitor
    }

    #[must_use]
    pub fn detector(&self) -> &Arc<Detector> {
        &self.detector
    }
}

impl Runtime for HubRuntime {
    type Target = ();

    fn status(&self) -> &HubStatus {
        &self.status
    }

    fn stop(&self) {}
}

/// How the server is started. Everything that is not the page is a decision of the caller, so that a
/// test can hold the clock, the secrets and the machine it looks at.
pub struct ServerConfig {
    /// Must be loopback; port 0 picks a free port.
    pub addr: SocketAddr,
    pub limits: Limits,
    pub clock: Arc<dyn Clock>,
    pub pairing_ttl: Duration,
    pub control_secret: Secret,
    pub instance: Secret,
    pub ui: Arc<dyn Pages>,
    pub detector: Arc<Detector>,
    pub launcher: Arc<Launcher>,
    pub prefs: Arc<Prefs>,
    /// The export folder of testing.nvim the monitor reads (none: the page shows no monitor).
    pub monitor: Arc<Monitor>,
}

/// The names (below `/api/hub/`) of the routes that need a session. The security matrix reads them
/// from here: a route added to [`routes`] and left out of this list would be a route the matrix
/// never tries (`tests/hub.rs` asks every name with a session).
pub const DATA_ROUTES: [&str; 2] = ["apps", "monitor"];

/// `GET /api/hub/apps`: the apps of the catalog and what the machine says about each. No path of the
/// machine is in it.
async fn apps(State(core): State<Arc<Core<HubRuntime>>>) -> Json<Value> {
    let found = core.rt().detector().apps().await;
    let views: Vec<_> = found.iter().map(|a| a.view()).collect();
    Json(json!({ "apps": views }))
}

/// `GET /api/hub/monitor`: the light of each repository of the export folder of testing.nvim. It reads
/// the folder on every call (a few small files); it holds no history and starts nothing.
async fn monitor(State(core): State<Arc<Core<HubRuntime>>>) -> Json<Value> {
    let monitor = Arc::clone(core.rt().monitor());
    // The files are read on a thread of their own: a slow disk does not hold up the server.
    let reading = tokio::task::spawn_blocking(move || monitor.read())
        .await
        .unwrap_or(crate::monitor::Reading::Problem {
            problem: "the export cannot be read",
        });
    Json(serde_json::to_value(reading).unwrap_or(Value::Null))
}

/// The routes that change something (a `POST`, a session and a JSON body): `{id}` stands for an app. The
/// tests ask each of them without a session, with a wrong content type and with a body that is not one.
pub const MUTATION_ROUTES: [&str; 4] = [
    "apps/{id}/launch",
    "apps/{id}/mode",
    "rescan",
    "apps/{id}/stop",
];

/// What a click sends: a mode, or nothing (the default of the app). Nothing else is read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchBody {
    mode: Option<String>,
}

fn request_of(body: &[u8]) -> Result<Request, &'static str> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Request::Default);
    }
    let parsed: LaunchBody =
        serde_json::from_slice(body).map_err(|_| "the body is not {\"mode\": ...}")?;
    match parsed.mode.as_deref() {
        None => Ok(Request::Default),
        Some("native") => Ok(Request::Native),
        Some("web") => Ok(Request::Web),
        Some("setup") => Ok(Request::Setup),
        Some(_) => Err("mode is native, web or setup"),
    }
}

/// `POST /api/hub/apps/{id}/launch`: start an app. The answer comes at once and does not wait for the app.
async fn launch(
    State(core): State<Arc<Core<HubRuntime>>>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let ask = match request_of(&body) {
        Ok(ask) => ask,
        Err(why) => return appkit_hub::error_body(StatusCode::BAD_REQUEST, "bad_request", why),
    };
    let launcher = Arc::clone(core.rt().launcher());
    match launcher.launch(&id, ask).await {
        Ok(outcome) => Json(json!({
            "result": match outcome {
                Outcome::Started => "started",
                Outcome::Starting => "starting",
                Outcome::Opened => "opened",
                Outcome::SetupRequired => "setup_required",
                Outcome::Stopping => "stopping",
                Outcome::NotRunning => "not_running",
            }
        }))
        .into_response(),
        Err(LaunchError::UnknownApp) => {
            appkit_hub::error_body(StatusCode::NOT_FOUND, "not_found", "no such app")
        }
        Err(LaunchError::NotInstalled) => appkit_hub::error_body(
            StatusCode::CONFLICT,
            "not_installed",
            "the app is not installed",
        ),
        Err(LaunchError::NotAvailable(what)) => appkit_hub::error_body(
            StatusCode::CONFLICT,
            "not_available",
            &format!("the app has no {what} mode"),
        ),
        Err(LaunchError::PortBusy) => appkit_hub::error_body(
            StatusCode::CONFLICT,
            "port_busy",
            "something else answers on the port of the app",
        ),
        Err(LaunchError::WebUnavailable) => appkit_hub::error_body(
            StatusCode::SERVICE_UNAVAILABLE,
            "web_unavailable",
            "the web variants are not served (the port is taken)",
        ),
        Err(LaunchError::Failed(why)) => {
            // What went wrong may hold a path: it goes to the console of the hub, not to the page.
            eprintln!("launch {id}: {why}");
            appkit_hub::error_body(
                StatusCode::BAD_GATEWAY,
                "start_failed",
                "the app could not be started",
            )
        }
    }
}

/// `POST /api/hub/apps/{id}/stop`: ask an app that runs to stop. The body is `{}`; nothing in it is read.
async fn stop(
    State(core): State<Arc<Core<HubRuntime>>>,
    Path(id): Path<String>,
    _body: Bytes,
) -> Response {
    let launcher = Arc::clone(core.rt().launcher());
    match launcher.stop(&id).await {
        Ok(Outcome::Stopping) => Json(json!({ "result": "stopping" })).into_response(),
        Ok(Outcome::NotRunning) => Json(json!({ "result": "not_running" })).into_response(),
        Ok(_) => Json(json!({ "result": "starting" })).into_response(),
        Err(LaunchError::UnknownApp) => {
            appkit_hub::error_body(StatusCode::NOT_FOUND, "not_found", "no such app")
        }
        Err(LaunchError::NotInstalled) => appkit_hub::error_body(
            StatusCode::CONFLICT,
            "not_installed",
            "the app is not installed",
        ),
        Err(LaunchError::NotAvailable(_)) => appkit_hub::error_body(
            StatusCode::CONFLICT,
            "not_available",
            "the hub cannot stop this app",
        ),
        Err(other) => {
            eprintln!("stop {id}: {other:?}");
            appkit_hub::error_body(
                StatusCode::BAD_GATEWAY,
                "stop_failed",
                "the app could not be asked to stop",
            )
        }
    }
}

/// What a choice of the mode sends: `native`, `web`, or `null` to go back to the default of the app.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeBody {
    mode: Option<String>,
}

/// `POST /api/hub/apps/{id}/mode`: the mode a click opens for the app, kept in the settings.
async fn choose_mode(
    State(core): State<Arc<Core<HubRuntime>>>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Ok(parsed) = serde_json::from_slice::<ModeBody>(&body) else {
        return appkit_hub::error_body(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "the body is not {\"mode\": ...}",
        );
    };
    let mode = match parsed.mode.as_deref().map(crate::config::mode_of) {
        None => None,
        Some(Some(mode)) => Some(mode),
        Some(None) => {
            return appkit_hub::error_body(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "mode is native, web or null",
            );
        }
    };
    match core.rt().prefs().set_mode(&id, mode) {
        Ok(()) => Json(json!({ "result": "saved" })).into_response(),
        Err(PrefsError::UnknownApp) => {
            appkit_hub::error_body(StatusCode::NOT_FOUND, "not_found", "no such app")
        }
        Err(PrefsError::NotAvailable(what)) => appkit_hub::error_body(
            StatusCode::CONFLICT,
            "not_available",
            &format!("the app has no {what} mode"),
        ),
        Err(PrefsError::Failed(why)) => {
            eprintln!("settings: {why}");
            appkit_hub::error_body(
                StatusCode::INTERNAL_SERVER_ERROR,
                "not_saved",
                "the settings could not be written",
            )
        }
    }
}

/// `POST /api/hub/rescan`: forget what was found; the next `GET /api/hub/apps` looks at the machine again.
async fn rescan(State(core): State<Arc<Core<HubRuntime>>>, _body: Bytes) -> Json<Value> {
    core.rt().detector().invalidate();
    Json(json!({ "result": "ok" }))
}

fn routes(core: &Arc<Core<HubRuntime>>) -> Router<Arc<Core<HubRuntime>>> {
    let small = core.limits.small_body_bytes;
    Router::new()
        .route(&IDENTITY.route("apps"), get(apps))
        .route(&IDENTITY.route("monitor"), get(monitor))
        .route(
            &IDENTITY.route(MUTATION_ROUTES[0]),
            post(launch).layer(DefaultBodyLimit::max(small)),
        )
        .route(
            &IDENTITY.route(MUTATION_ROUTES[1]),
            post(choose_mode).layer(DefaultBodyLimit::max(small)),
        )
        .route(
            &IDENTITY.route(MUTATION_ROUTES[2]),
            post(rescan).layer(DefaultBodyLimit::max(small)),
        )
        .route(
            &IDENTITY.route(MUTATION_ROUTES[3]),
            post(stop).layer(DefaultBodyLimit::max(small)),
        )
}

/// Start the server.
///
/// # Errors
///
/// An I/O error of binding, or `InvalidInput` for an address that is not loopback.
pub async fn start(cfg: ServerConfig) -> std::io::Result<RunningServer> {
    let detector = cfg.detector;
    let launcher = cfg.launcher;
    let prefs = cfg.prefs;
    let monitor = cfg.monitor;
    let for_build = (
        Arc::clone(&detector),
        Arc::clone(&launcher),
        Arc::clone(&prefs),
        Arc::clone(&monitor),
    );
    serve(
        ServeParts {
            addr: cfg.addr,
            limits: cfg.limits,
            clock: cfg.clock,
            pairing_ttl: cfg.pairing_ttl,
            control_secret: cfg.control_secret,
            instance: cfg.instance,
            app: IDENTITY.app,
            version: crate::VERSION,
            build_id: crate::BUILD,
            // `hello` needs no session; no route of the hub is coarse (rate limited on its own) yet.
            guard_rules: GuardRules::new(IDENTITY.route("hello"), Vec::<String>::new()),
            ui: cfg.ui,
            runtime: HubRuntime::new(detector, launcher, prefs, monitor),
            // There is no vault to choose, so no runtime is ever made again.
            build: Arc::new(move |()| {
                HubRuntime::new(
                    Arc::clone(&for_build.0),
                    Arc::clone(&for_build.1),
                    Arc::clone(&for_build.2),
                    Arc::clone(&for_build.3),
                )
            }),
            vaults: None,
        },
        routes,
    )
    .await
}

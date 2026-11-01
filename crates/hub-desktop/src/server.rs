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
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::detect::Detector;
use crate::IDENTITY;

/// What the hub keeps: the status `hello` reports (it has no engine) and the way to look at the machine.
/// It starts and stops nothing.
pub struct HubRuntime {
    status: HubStatus,
    detector: Arc<Detector>,
}

impl HubRuntime {
    /// The runtime of a hub that has no engine: `hello` says it is not ready for one, and that is right.
    #[must_use]
    pub fn new(detector: Arc<Detector>) -> Self {
        Self {
            status: HubStatus {
                engine_ready: false,
                problem: None,
                capabilities: Vec::new(),
            },
            detector,
        }
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
}

/// The names (below `/api/hub/`) of the routes that need a session. The security matrix reads them
/// from here: a route added to [`routes`] and left out of this list would be a route the matrix
/// never tries (`tests/hub.rs` asks every name with a session).
pub const DATA_ROUTES: [&str; 1] = ["apps"];

/// `GET /api/hub/apps`: the apps of the catalog and what the machine says about each. No path of the
/// machine is in it.
async fn apps(State(core): State<Arc<Core<HubRuntime>>>) -> Json<Value> {
    let found = core.rt().detector().apps().await;
    let views: Vec<_> = found.iter().map(|a| a.view()).collect();
    Json(json!({ "apps": views }))
}

fn routes(_core: &Arc<Core<HubRuntime>>) -> Router<Arc<Core<HubRuntime>>> {
    Router::new().route(&IDENTITY.route("apps"), get(apps))
}

/// Start the server.
///
/// # Errors
///
/// An I/O error of binding, or `InvalidInput` for an address that is not loopback.
pub async fn start(cfg: ServerConfig) -> std::io::Result<RunningServer> {
    let detector = cfg.detector;
    let for_build = Arc::clone(&detector);
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
            // `hello` needs no session; no route of the hub is coarse (rate limited on its own) yet.
            guard_rules: GuardRules::new(IDENTITY.route("hello"), Vec::<String>::new()),
            ui: cfg.ui,
            runtime: HubRuntime::new(detector),
            // There is no vault to choose, so no runtime is ever made again.
            build: Arc::new(move |()| HubRuntime::new(Arc::clone(&for_build))),
            vaults: None,
        },
        routes,
    )
    .await
}

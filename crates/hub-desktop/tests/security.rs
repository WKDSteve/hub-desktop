//! The security matrix of the guard (concept 5.5), run for the hub. The tests are written once, in
//! `appkit_testkit::security`; every app that has a hub runs them for itself.

use std::sync::Arc;

use appkit_core::identity::AppIdentity;
use appkit_hub::{BoxFuture, RunningServer};
use appkit_testkit::subject::{StartConfig, Subject};
use hub_desktop::server::{self, ServerConfig, DATA_ROUTES};
use hub_desktop::ui::UiFiles;

/// The hub of this repository as a subject of the matrix: no page (the placeholder of the kit is served),
/// the clock and the secrets of the test.
struct Hub;

impl Subject for Hub {
    fn identity(&self) -> &'static AppIdentity {
        &hub_desktop::IDENTITY
    }

    fn start(&self, cfg: StartConfig) -> BoxFuture<'_, std::io::Result<RunningServer>> {
        Box::pin(async move {
            server::start(ServerConfig {
                addr: cfg.addr,
                limits: cfg.limits,
                clock: cfg.clock,
                pairing_ttl: cfg.pairing_ttl,
                control_secret: cfg.control_secret,
                instance: cfg.instance,
                ui: Arc::new(UiFiles::none()),
            })
            .await
        })
    }

    fn data_routes(&self) -> Vec<&'static str> {
        DATA_ROUTES.to_vec()
    }
}

mod hub {
    appkit_testkit::security_matrix!(super::Hub);
}

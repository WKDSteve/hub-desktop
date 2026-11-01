//! The hub as a program of its own: it has no vault and no engine, and says so; its route needs a
//! session; the hub that is started is the one `hello` names.

use std::sync::Arc;
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::Limits;
use appkit_testkit::wire::{request, send};
use hub_desktop::server::{self, ServerConfig, DATA_ROUTES};
use hub_desktop::ui::UiFiles;

async fn start() -> appkit_hub::RunningServer {
    server::start(ServerConfig {
        addr: "127.0.0.1:0".parse().unwrap(),
        limits: Limits::default(),
        clock: Arc::new(ManualClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: Secret::generate(TOKEN_BYTES).unwrap(),
        instance: Secret::generate(CODE_BYTES).unwrap(),
        ui: Arc::new(UiFiles::none()),
    })
    .await
    .expect("the hub starts")
}

#[tokio::test]
async fn hello_says_the_hub_has_no_engine_and_needs_no_setup() {
    let server = start().await;
    let host = format!("127.0.0.1:{}", server.addr().port());
    let headers = [("Host", host.as_str())];
    let hello = send(
        server.addr(),
        request("GET", "/api/hub/hello", &headers, b""),
    )
    .await;
    assert_eq!(hello.status, 200, "{hello:?}");
    let doc = hello.json();
    assert_eq!(doc["app"], "hub-desktop");
    assert_eq!(doc["engine"]["ready"], false);
    assert_eq!(doc["setup_required"], false);
    assert_eq!(doc["setup_reason"], serde_json::Value::Null);
    server.shutdown().await;
}

#[tokio::test]
async fn every_route_of_the_hub_needs_a_session() {
    let server = start().await;
    let host = format!("127.0.0.1:{}", server.addr().port());
    let headers = [("Host", host.as_str())];
    for name in DATA_ROUTES {
        let path = hub_desktop::IDENTITY.route(name);
        let reply = send(server.addr(), request("GET", &path, &headers, b"")).await;
        assert_eq!(reply.status, 401, "{path}: {reply:?}");
    }
    server.shutdown().await;
}

/// A route that is added to the server and not named in `DATA_ROUTES` is a route the matrix never
/// tries: the names must be the ones the server answers.
#[tokio::test]
async fn the_routes_the_matrix_tries_are_the_routes_the_server_has() {
    let server = start().await;
    let host = format!("127.0.0.1:{}", server.addr().port());
    let headers = [("Host", host.as_str())];
    let open = send(
        server.addr(),
        request("GET", "/api/hub/nope", &headers, b""),
    )
    .await;
    // Unknown routes below /api/ are refused for lack of a session, like the known ones.
    assert_eq!(open.status, 401, "{open:?}");
    assert_eq!(DATA_ROUTES, ["apps"]);
    server.shutdown().await;
}

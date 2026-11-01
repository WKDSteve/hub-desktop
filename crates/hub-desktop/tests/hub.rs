//! The hub as a program of its own: it has no vault and no engine, and says so; its route needs a
//! session; the hub that is started is the one `hello` names.

use std::sync::Arc;
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::Limits;
use appkit_testkit::wire::{request, send};
use hub_desktop::server::{self, ServerConfig, DATA_ROUTES};
use hub_desktop::ui::UiFiles;

/// The control secret of the hub that `start_with_secret` makes, as the bearer credential of a program.
async fn start_with_secret() -> (appkit_hub::RunningServer, String) {
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let server = server::start(ServerConfig {
        addr: "127.0.0.1:0".parse().unwrap(),
        limits: Limits::default(),
        clock: Arc::new(ManualClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: control.clone(),
        instance: Secret::generate(CODE_BYTES).unwrap(),
        ui: Arc::new(UiFiles::none()),
    })
    .await
    .expect("the hub starts");
    (server, control.to_hex())
}

async fn start() -> appkit_hub::RunningServer {
    start_with_secret().await.0
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

/// A route that is renamed or taken out of the server and left in `DATA_ROUTES` is a route the matrix
/// tries in vain (it is refused without a session whether it exists or not): with a session, every
/// name of the list must be a route that answers.
#[tokio::test]
async fn every_route_the_matrix_tries_exists_and_answers_with_a_session() {
    let (server, control) = start_with_secret().await;
    let host = format!("127.0.0.1:{}", server.addr().port());
    let origin = format!("http://{host}");
    let json = ("Content-Type", "application/json");

    // A program asks for a code with the control secret, the page trades it for a session.
    let auth = format!("Bearer {control}");
    let pair = send(
        server.addr(),
        request(
            "POST",
            "/api/control/pair",
            &[
                ("Host", host.as_str()),
                json,
                ("Authorization", auth.as_str()),
                ("Content-Length", "2"),
            ],
            b"{}",
        ),
    )
    .await;
    assert_eq!(pair.status, 200, "{pair:?}");
    let code = pair.json()["code"].as_str().expect("a code").to_owned();
    let body = format!(r#"{{"code":"{code}"}}"#);
    let length = body.len().to_string();
    let exchange = send(
        server.addr(),
        request(
            "POST",
            "/auth/exchange",
            &[
                ("Host", host.as_str()),
                ("Origin", origin.as_str()),
                json,
                ("Content-Length", length.as_str()),
            ],
            body.as_bytes(),
        ),
    )
    .await;
    assert_eq!(exchange.status, 200, "{exchange:?}");
    let session = format!(
        "Bearer {}",
        exchange.json()["token"].as_str().expect("a token")
    );

    for name in DATA_ROUTES {
        let path = hub_desktop::IDENTITY.route(name);
        let reply = send(
            server.addr(),
            request(
                "GET",
                &path,
                &[("Host", host.as_str()), ("Authorization", session.as_str())],
                b"",
            ),
        )
        .await;
        assert_eq!(reply.status, 200, "{path}: {reply:?}");
    }
    // And a route that is not in the list is none the server has.
    let nope = send(
        server.addr(),
        request(
            "GET",
            "/api/hub/nope",
            &[("Host", host.as_str()), ("Authorization", session.as_str())],
            b"",
        ),
    )
    .await;
    assert_eq!(nope.status, 404, "{nope:?}");
    server.shutdown().await;
}

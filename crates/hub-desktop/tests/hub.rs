//! The hub as a program of its own: it has no vault and no engine, and says so; its route needs a
//! session; the hub that is started is the one `hello` names.

use std::sync::Arc;
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::Limits;
use appkit_testkit::wire::{request, send};
use hub_desktop::config::Env;
use hub_desktop::detect::Detector;
use hub_desktop::launch::{Browser, LaunchConfig, Launcher};
use hub_desktop::monitor::Monitor;
use hub_desktop::prefs::Prefs;
use hub_desktop::server::{self, ServerConfig, DATA_ROUTES};
use hub_desktop::ui::UiFiles;

/// The control secret of the hub that `start_with_secret` makes, as the bearer credential of a program.
async fn start_with_monitor(monitor: Monitor) -> (appkit_hub::RunningServer, String) {
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let detector = Arc::new(Detector::new(
        Env::from_pairs([("APPDATA", "C:/none")]),
        None,
    ));
    let server = server::start(ServerConfig {
        addr: "127.0.0.1:0".parse().unwrap(),
        limits: Limits::default(),
        clock: Arc::new(ManualClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: control.clone(),
        instance: Secret::generate(CODE_BYTES).unwrap(),
        ui: Arc::new(UiFiles::none()),
        detector: Arc::clone(&detector),
        launcher: Arc::new(Launcher::new(
            Arc::clone(&detector),
            LaunchConfig::new(None, Browser::System),
        )),
        prefs: Arc::new(Prefs::new(None, Arc::clone(&detector))),
        monitor: Arc::new(monitor),
    })
    .await
    .expect("the hub starts");
    (server, control.to_hex())
}

async fn start_with_secret() -> (appkit_hub::RunningServer, String) {
    start_with_monitor(Monitor::new(None)).await
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

/// A program asks for a code with the control secret, the page trades it for a session.
async fn session_for(server: &appkit_hub::RunningServer, control: &str) -> String {
    let host = format!("127.0.0.1:{}", server.addr().port());
    let origin = format!("http://{host}");
    let json = ("Content-Type", "application/json");
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
    format!(
        "Bearer {}",
        exchange.json()["token"].as_str().expect("a token")
    )
}

/// A route that is renamed or taken out of the server and left in `DATA_ROUTES` is a route the matrix
/// tries in vain (it is refused without a session whether it exists or not): with a session, every
/// name of the list must be a route that answers.
#[tokio::test]
async fn every_route_the_matrix_tries_exists_and_answers_with_a_session() {
    let (server, control) = start_with_secret().await;
    let host = format!("127.0.0.1:{}", server.addr().port());

    let session = session_for(&server, &control).await;

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

/// The route reads the folder the hub was given, and a folder that cannot be read answers with a sentence,
/// not with a path.
#[tokio::test]
async fn the_monitor_route_reads_the_export_folder_of_the_hub() {
    let machine = appkit_scratch::Scratch::new("hub-monitor-route");
    let fleet = r#"{"schema":"testing-export/1","kind":"fleet","form":"export","classes":{"checked":true},
        "repos":[{"key":"a-1","name":"alpha.nvim","local":{"verdict":"red"},"counts":{"fail":2}}]}"#;
    std::fs::write(machine.path().join("fleet.json"), fleet).unwrap();
    let manifest = format!(
        r#"{{"schema":"testing-export/1","kind":"manifest","files":[{{"path":"fleet.json","bytes":{}}}]}}"#,
        fleet.len()
    );
    std::fs::write(machine.path().join("testing-export.json"), manifest).unwrap();

    for (dir, state) in [
        (machine.path().join(""), "ready"),
        (machine.path().join("absent"), "problem"),
    ] {
        let (server, control) = start_with_monitor(Monitor::new(Some(dir))).await;
        let session = session_for(&server, &control).await;
        let host = format!("127.0.0.1:{}", server.addr().port());
        let reply = send(
            server.addr(),
            request(
                "GET",
                "/api/hub/monitor",
                &[("Host", host.as_str()), ("Authorization", session.as_str())],
                b"",
            ),
        )
        .await;
        assert_eq!(reply.status, 200, "{reply:?}");
        let doc = reply.json();
        assert_eq!(doc["state"], state, "{doc}");
        if state == "ready" {
            assert_eq!(doc["repos"][0]["name"], "alpha.nvim");
            assert_eq!(doc["repos"][0]["light"], "red");
            assert_eq!(doc["repos"][0]["fail"], 2);
        } else {
            let text = doc.to_string();
            assert!(
                !text.contains("absent") && !text.contains("hub-monitor-route"),
                "{text}"
            );
        }
        server.shutdown().await;
    }
}

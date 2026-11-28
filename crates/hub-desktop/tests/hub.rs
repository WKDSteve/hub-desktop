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
use hub_desktop::update::Updater;

/// The control secret of the hub that `start_with_secret` makes, as the bearer credential of a program.
async fn start_with_monitor(monitor: Monitor) -> (appkit_hub::RunningServer, String) {
    start_with(monitor, hub_desktop::ciwatch::Watcher::off("a test"), None).await
}

async fn start_with(
    monitor: Monitor,
    ci: hub_desktop::ciwatch::Watcher,
    settings: Option<std::path::PathBuf>,
) -> (appkit_hub::RunningServer, String) {
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let detector = Arc::new(Detector::new(
        Env::from_pairs([("APPDATA", "C:/none")]),
        None,
    ));
    let launcher = Arc::new(Launcher::new(
        Arc::clone(&detector),
        LaunchConfig::new(None, Browser::System),
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
        launcher: Arc::clone(&launcher),
        prefs: Arc::new(Prefs::new(settings, Arc::clone(&detector))),
        monitor: Arc::new(monitor),
        updater: Arc::new(Updater::disabled(Arc::clone(&detector), launcher)),
        ci: Arc::new(ci),
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

/// `ci` shows what the watcher of the hub is at: off with its reason, or its last round with the hints.
#[tokio::test]
async fn the_ci_route_shows_what_the_watcher_is_at() {
    use hub_desktop::ciwatch::{Hint, Round, Seen, Watcher, OFF_IN_TESTS};
    let defaults =
        serde_json::json!({"on": true, "interval_min": 5, "min": 1, "max": 60, "default": 5});
    let mut round = Round::default();
    round.hints.insert(Hint::Queued {
        repo: "tasks.nvim".into(),
        minutes: 31,
    });
    for (watcher, want) in [
        (
            Watcher::off(OFF_IN_TESTS),
            serde_json::json!({"state": "off", "why": OFF_IN_TESTS, "settings": defaults.clone()}),
        ),
        (
            Watcher::new(Seen::Ran { at: 9, round }),
            serde_json::json!({"state": "ran", "at": 9, "written": [], "not_written": [],
                "hints": [{"kind": "queued", "repo": "tasks.nvim", "minutes": 31}], "problem": null,
                "settings": defaults}),
        ),
    ] {
        let (server, control) = start_with(Monitor::new(None), watcher, None).await;
        let session = session_for(&server, &control).await;
        let host = format!("127.0.0.1:{}", server.addr().port());
        let reply = send(
            server.addr(),
            request(
                "GET",
                &hub_desktop::IDENTITY.route("ci"),
                &[("Host", host.as_str()), ("Authorization", session.as_str())],
                b"",
            ),
        )
        .await;
        assert_eq!(reply.status, 200, "{reply:?}");
        assert_eq!(reply.json(), want);
        server.shutdown().await;
    }
}

/// `ci/settings` writes the switch and the interval of the CI watcher into `config.json` (keeping what else is in
/// it), and `ci` shows them; an interval outside 1 to 60, or a body of another shape, changes nothing.
#[tokio::test]
async fn the_options_of_the_ci_watcher_are_saved_into_the_settings_file() {
    let machine = appkit_scratch::Scratch::new("hub-ci-settings");
    let dir = machine.path().to_path_buf();
    std::fs::write(dir.join("config.json"), r#"{"keep_running":true}"#).unwrap();
    let (server, control) = start_with(
        Monitor::new(None),
        hub_desktop::ciwatch::Watcher::off("a test"),
        Some(dir.clone()),
    )
    .await;
    let session = session_for(&server, &control).await;
    let host = format!("127.0.0.1:{}", server.addr().port());
    let origin = format!("http://{host}");
    let save = |body: &'static str| {
        let (host, origin, session) = (host.clone(), origin.clone(), session.clone());
        let addr = server.addr();
        async move {
            let length = body.len().to_string();
            send(
                addr,
                request(
                    "POST",
                    &hub_desktop::IDENTITY.route("ci/settings"),
                    &[
                        ("Host", host.as_str()),
                        ("Origin", origin.as_str()),
                        ("Authorization", session.as_str()),
                        ("Content-Type", "application/json"),
                        ("Content-Length", length.as_str()),
                    ],
                    body.as_bytes(),
                ),
            )
            .await
        }
    };
    let file = || -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).unwrap()).unwrap()
    };
    let shown = || {
        let (host, session) = (host.clone(), session.clone());
        let addr = server.addr();
        async move {
            send(
                addr,
                request(
                    "GET",
                    &hub_desktop::IDENTITY.route("ci"),
                    &[("Host", host.as_str()), ("Authorization", session.as_str())],
                    b"",
                ),
            )
            .await
            .json()["settings"]
                .clone()
        }
    };

    let reply = save(r#"{"on":false,"interval_min":12}"#).await;
    assert_eq!(reply.status, 200, "{reply:?}");
    assert_eq!(reply.json(), serde_json::json!({"result": "saved"}));
    assert_eq!(file()["ci_watch"], false);
    assert_eq!(file()["ci_watch_interval_min"], 12);
    assert_eq!(file()["keep_running"], true, "the rest of the file stays");
    let settings = shown().await;
    assert_eq!(
        (settings["on"].clone(), settings["interval_min"].clone()),
        (false.into(), 12.into())
    );

    for bad in [
        r#"{"on":true,"interval_min":0}"#,
        r#"{"on":true,"interval_min":61}"#,
        r#"{"on":true,"interval_min":-5}"#,
        r#"{"on":true,"interval_min":"5"}"#,
        r#"{"on":true}"#,
        r#"{"on":"yes","interval_min":5}"#,
        r#"{"on":true,"interval_min":5,"vault":"C:/v"}"#,
    ] {
        let reply = save(bad).await;
        assert_eq!(reply.status, 400, "{bad}: {reply:?}");
        assert_eq!(file()["ci_watch_interval_min"], 12, "{bad}");
    }

    // Back to the defaults: no key is left for them.
    let reply = save(r#"{"on":true,"interval_min":5}"#).await;
    assert_eq!(reply.status, 200, "{reply:?}");
    assert_eq!(file(), serde_json::json!({"keep_running": true}));
    assert_eq!(shown().await["interval_min"], 5);

    // A number outside the bounds written by hand is shown as the default.
    std::fs::write(dir.join("config.json"), r#"{"ci_watch_interval_min":90}"#).unwrap();
    assert_eq!(shown().await["interval_min"], 5);
    server.shutdown().await;
}

/// After a save the state of `ci` follows the switch at once: the loop of the watcher reads the settings only on its
/// next tick (30 s), and the page reads `ci` right after the save. Off: "off" by the settings, not the last round;
/// on again: waiting for the next round. A watcher that does not run for another reason keeps its reason.
#[tokio::test]
async fn the_state_of_the_ci_watcher_follows_a_saved_switch_at_once() {
    use hub_desktop::ciwatch::{Round, Seen, Watcher, OFF_BY_SETTINGS};
    let machine = appkit_scratch::Scratch::new("hub-ci-switch");
    let dir = machine.path().to_path_buf();
    let (server, control) = start_with(
        Monitor::new(None),
        Watcher::new(Seen::Ran {
            at: 7,
            round: Round::default(),
        }),
        Some(dir.clone()),
    )
    .await;
    let session = session_for(&server, &control).await;
    let host = format!("127.0.0.1:{}", server.addr().port());
    let origin = format!("http://{host}");
    let save = |body: &'static str| {
        let (host, origin, session) = (host.clone(), origin.clone(), session.clone());
        let addr = server.addr();
        async move {
            let length = body.len().to_string();
            send(
                addr,
                request(
                    "POST",
                    &hub_desktop::IDENTITY.route("ci/settings"),
                    &[
                        ("Host", host.as_str()),
                        ("Origin", origin.as_str()),
                        ("Authorization", session.as_str()),
                        ("Content-Type", "application/json"),
                        ("Content-Length", length.as_str()),
                    ],
                    body.as_bytes(),
                ),
            )
            .await
            .status
        }
    };
    let state = || {
        let (host, session) = (host.clone(), session.clone());
        let addr = server.addr();
        async move {
            let doc = send(
                addr,
                request(
                    "GET",
                    &hub_desktop::IDENTITY.route("ci"),
                    &[("Host", host.as_str()), ("Authorization", session.as_str())],
                    b"",
                ),
            )
            .await
            .json();
            (doc["state"].clone(), doc["why"].clone())
        }
    };

    assert_eq!(state().await.0, "ran");
    assert_eq!(save(r#"{"on":false,"interval_min":5}"#).await, 200);
    assert_eq!(state().await, ("off".into(), OFF_BY_SETTINGS.into()));
    assert_eq!(save(r#"{"on":true,"interval_min":5}"#).await, 200);
    assert_eq!(state().await.0, "ran", "the last round until the next");
    server.shutdown().await;

    // A watcher that does not run (here: a test) keeps its reason, whatever the switch says.
    let (server, control) = start_with(
        Monitor::new(None),
        Watcher::off("a test"),
        Some(dir.clone()),
    )
    .await;
    let session = session_for(&server, &control).await;
    let host = format!("127.0.0.1:{}", server.addr().port());
    let doc = send(
        server.addr(),
        request(
            "GET",
            &hub_desktop::IDENTITY.route("ci"),
            &[("Host", host.as_str()), ("Authorization", session.as_str())],
            b"",
        ),
    )
    .await
    .json();
    assert_eq!(doc["why"], "a test");
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

/// The page may fetch from this origin and from GitHub (the update badge), and from nothing else.
#[tokio::test]
async fn the_page_may_reach_github_and_nobody_else() {
    let server = start().await;
    let host = format!("127.0.0.1:{}", server.addr().port());
    let reply = send(
        server.addr(),
        request("GET", "/", &[("Host", host.as_str())], b""),
    )
    .await;
    let policy = reply.header("content-security-policy").unwrap().to_owned();
    assert!(
        policy.contains("connect-src 'self' https://api.github.com;"),
        "{policy}"
    );
    assert_eq!(policy.matches("https://").count(), 1, "{policy}");
    server.shutdown().await;
}

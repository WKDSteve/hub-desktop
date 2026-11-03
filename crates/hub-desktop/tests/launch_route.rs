//! `POST /api/hub/apps/{id}/launch` over HTTP, with a session: what the guard refuses, what the route
//! refuses, and that one click starts one program.

use std::sync::Arc;
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::{Limits, RunningServer};
use appkit_testkit::wire::{request, send, Reply};
use hub_desktop::detect::Detector;
use hub_desktop::launch::{Browser, LaunchConfig, Launcher};
use hub_desktop::monitor::Monitor;
use hub_desktop::prefs::Prefs;
use hub_desktop::server::{self, ServerConfig, MUTATION_ROUTES};
use hub_desktop::ui::UiFiles;
use hub_desktop::update::Updater;

mod common;
use common::*;

struct Hub {
    server: RunningServer,
    token: String,
    machine: Machine,
}

impl Hub {
    async fn new(name: &str, extra_env: &[(&str, String)]) -> Self {
        let machine = Machine::new(name);
        let vault = machine.path("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let mut env = vec![("TASKS_VAULT", vault.display().to_string())];
        env.extend(extra_env.iter().map(|(k, v)| (*k, v.clone())));
        let detector = Arc::new(Detector::with_ttl(machine.env(&env), None, Duration::ZERO));
        let mut cfg = LaunchConfig::new(None, Browser::Log(machine.path("browser.log")));
        cfg.ready_timeout = Duration::from_millis(600);
        cfg.settle = Duration::from_millis(600);
        let control = Secret::generate(TOKEN_BYTES).unwrap();
        let launcher = Arc::new(Launcher::new(Arc::clone(&detector), cfg));
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
            prefs: Arc::new(Prefs::new(
                Some(machine.path("cfg-hub")),
                Arc::clone(&detector),
            )),
            monitor: Arc::new(Monitor::new(None)),
            updater: Arc::new(Updater::disabled(detector, launcher)),
        })
        .await
        .unwrap();
        let token = session(&server, &control.to_hex()).await;
        Self {
            server,
            token,
            machine,
        }
    }

    /// A `POST` of a page of the hub: its own origin, the session, JSON.
    async fn post(&self, path: &str, body: &str) -> Reply {
        let host = format!("127.0.0.1:{}", self.server.addr().port());
        let origin = format!("http://{host}");
        let auth = format!("Bearer {}", self.token);
        let length = body.len().to_string();
        send(
            self.server.addr(),
            request(
                "POST",
                path,
                &[
                    ("Host", host.as_str()),
                    ("Origin", origin.as_str()),
                    ("Authorization", auth.as_str()),
                    ("Content-Type", "application/json"),
                    ("Content-Length", length.as_str()),
                ],
                body.as_bytes(),
            ),
        )
        .await
    }

    async fn launch(&self, id: &str, body: &str) -> Reply {
        self.post(&format!("/api/hub/apps/{id}/launch"), body).await
    }
}

#[tokio::test]
async fn a_click_starts_the_app_once_and_a_second_click_is_told_it_is_starting() {
    let hub = Hub::new("hub-route-click", &[]).await;
    let program = hub.machine.tasks_program();
    let first = hub.launch("tasks", r#"{"mode":"native"}"#).await;
    assert_eq!(first.status, 200, "{first:?}");
    assert_eq!(first.json()["result"], "started");
    let second = hub.launch("tasks", r#"{"mode":"native"}"#).await;
    assert_eq!(second.json()["result"], "starting");
    let third = hub.launch("tasks", "").await;
    assert_eq!(third.json()["result"], "starting");
    let end = std::time::Instant::now() + Duration::from_secs(15);
    while starts_of(&program).is_empty() && std::time::Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(starts_of(&program).len(), 1);
    hub.server.shutdown().await;
}

#[tokio::test]
async fn a_request_that_is_not_a_mode_is_refused_before_anything_is_started() {
    let hub = Hub::new("hub-route-bad", &[]).await;
    let program = hub.machine.tasks_program();
    for body in [
        r#"{"mode":"fly"}"#,
        r#"{"mode":"Native"}"#,
        r#"{"mode":["native"]}"#,
        r#"{"mode":null,"program":"C:/Windows/System32/cmd.exe"}"#,
        r#"{"mode":"native","args":["--evil"]}"#,
        r#"{"mode":"native","path":"C:/x"}"#,
        r#"{"id":"tasks"}"#,
        r#"[]"#,
        r#""native""#,
        r#"{"mode":"native""#,
        "not json",
    ] {
        let reply = hub.launch("tasks", body).await;
        assert_eq!(reply.status, 400, "{body}: {reply:?}");
        assert_eq!(reply.json()["error"]["code"], "bad_request");
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(starts_of(&program).is_empty());
    hub.server.shutdown().await;
}

#[tokio::test]
async fn an_unknown_app_a_missing_one_and_a_foreign_program_on_a_port_are_answered_in_words() {
    let busy = fake_server(200, r#"{"hello":"world"}"#.to_owned()).await;
    let hub = Hub::new("hub-route-errors", &[("AI_DESKTOP_PORT", busy.to_string())]).await;
    hub.machine
        .program("Local/Programs/ai-desktop/app/ai-desktop.exe");
    for (id, status, code) in [
        ("nope", 404, "not_found"),
        ("..%2Ftasks", 404, "not_found"),
        ("hub", 404, "not_found"),
        ("tasks", 409, "not_installed"),
        ("streamtap", 409, "not_installed"),
        ("ai", 409, "port_busy"),
    ] {
        let reply = hub.launch(id, "{}").await;
        assert_eq!(reply.status, status, "{id}: {reply:?}");
        assert_eq!(reply.json()["error"]["code"], code, "{id}");
        // What the page reads holds no path of the machine.
        let folder = hub.machine.path("").display().to_string();
        let escaped = folder.replace('\\', "\\\\");
        assert!(
            !reply.body.contains(&folder) && !reply.body.contains(&escaped),
            "{id}: {}",
            reply.body
        );
    }
    // The web mode of an app with no page, and of a hub with no listener.
    let web = hub.launch("tasks", r#"{"mode":"web"}"#).await;
    assert_eq!(web.status, 409, "{web:?}");
    hub.server.shutdown().await;
}

#[tokio::test]
async fn setup_is_asked_as_a_mode_and_an_app_that_needs_it_says_so() {
    let hub = Hub::new("hub-route-setup", &[("TASKS_VAULT", String::new())]).await;
    let program = hub.machine.tasks_program();
    let reply = hub.launch("tasks", "{}").await;
    assert_eq!(reply.status, 200, "{reply:?}");
    assert_eq!(reply.json()["result"], "setup_required");
    let setup = hub.launch("tasks", r#"{"mode":"setup"}"#).await;
    assert_eq!(setup.json()["result"], "started");
    let end = std::time::Instant::now() + Duration::from_secs(15);
    while starts_of(&program).is_empty() && std::time::Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert_eq!(starts_of(&program)[0]["argv"], serde_json::json!([]));
    hub.server.shutdown().await;
}

/// What the guard says before the route is asked: no session, a session of nobody, a body that is not
/// JSON, a body that is too large.
#[tokio::test]
async fn the_guard_stands_in_front_of_every_route_that_changes_something() {
    let hub = Hub::new("hub-route-guard", &[]).await;
    let program = hub.machine.tasks_program();
    let host = format!("127.0.0.1:{}", hub.server.addr().port());
    let origin = format!("http://{host}");
    for name in MUTATION_ROUTES {
        let path = format!("/api/hub/{}", name.replace("{id}", "tasks"));
        let post = |headers: Vec<(&str, String)>, body: &str| {
            let headers: Vec<(String, String)> = headers
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect();
            let body = body.to_owned();
            let path = path.clone();
            let addr = hub.server.addr();
            async move {
                let refs: Vec<(&str, &str)> = headers
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                send(addr, request("POST", &path, &refs, body.as_bytes())).await
            }
        };
        let base = |auth: Option<String>, content_type: &str, length: usize| {
            let mut h = vec![
                ("Host", host.clone()),
                ("Origin", origin.clone()),
                ("Content-Type", content_type.to_owned()),
                ("Content-Length", length.to_string()),
            ];
            if let Some(a) = auth {
                h.push(("Authorization", a));
            }
            h
        };
        // No session, the control secret's shape, a session of nobody.
        for auth in [
            None,
            Some("Bearer ".to_owned()),
            Some(format!("Bearer {}", "0".repeat(64))),
            Some(format!("Basic {}", hub.token)),
        ] {
            let reply = post(base(auth.clone(), "application/json", 2), "{}").await;
            assert_eq!(reply.status, 401, "{path} with {auth:?}: {reply:?}");
        }
        // A cross-site page.
        let mut cross = base(Some(format!("Bearer {}", hub.token)), "application/json", 2);
        cross.retain(|(k, _)| *k != "Origin");
        cross.push(("Origin", "http://evil.example".to_owned()));
        assert_eq!(post(cross, "{}").await.status, 403);
        // Not JSON.
        let wrong = post(
            base(Some(format!("Bearer {}", hub.token)), "text/plain", 2),
            "{}",
        )
        .await;
        assert_eq!(wrong.status, 415, "{wrong:?}");
        // Too large.
        let big = "a".repeat(8192);
        let large = post(
            base(
                Some(format!("Bearer {}", hub.token)),
                "application/json",
                big.len(),
            ),
            &big,
        )
        .await;
        assert_eq!(large.status, 413, "{large:?}");
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        starts_of(&program).is_empty(),
        "nothing the guard refused started anything"
    );
    hub.server.shutdown().await;
}

/// A `GET` of the launch route is no start.
#[tokio::test]
async fn a_get_does_not_start_anything() {
    let hub = Hub::new("hub-route-get", &[]).await;
    let program = hub.machine.tasks_program();
    let host = format!("127.0.0.1:{}", hub.server.addr().port());
    let auth = format!("Bearer {}", hub.token);
    let reply = send(
        hub.server.addr(),
        request(
            "GET",
            "/api/hub/apps/tasks/launch",
            &[("Host", host.as_str()), ("Authorization", auth.as_str())],
            b"",
        ),
    )
    .await;
    assert_eq!(reply.status, 405, "{reply:?}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(starts_of(&program).is_empty());
    hub.server.shutdown().await;
}

fn settings(hub: &Hub) -> serde_json::Value {
    let text = std::fs::read_to_string(hub.machine.path("cfg-hub/config.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Acceptance 4: the mode a click opens is kept in `config.json`, as a word for an app of the catalog that has
/// that mode, and what a person wrote there by hand stays.
#[tokio::test]
async fn the_mode_of_a_tile_is_kept_in_the_settings_and_the_rest_of_the_file_stays() {
    let hub = Hub::new("hub-route-mode", &[]).await;
    std::fs::create_dir_all(hub.machine.path("cfg-hub")).unwrap();
    std::fs::write(
        hub.machine.path("cfg-hub/config.json"),
        r#"{"port":20000,"web_root":"C:/web"}"#,
    )
    .unwrap();

    let saved = hub
        .post("/api/hub/apps/tasks/mode", r#"{"mode":"web"}"#)
        .await;
    assert_eq!(saved.status, 200, "{saved:?}");
    let file = settings(&hub);
    assert_eq!(file["tiles"]["tasks"]["mode"], "web");
    assert_eq!(file["port"], 20000, "what was there stays");
    assert_eq!(file["web_root"], "C:/web");

    // Back to the default of the app: the tile entry goes.
    let reset = hub
        .post("/api/hub/apps/tasks/mode", r#"{"mode":null}"#)
        .await;
    assert_eq!(reset.status, 200, "{reset:?}");
    assert!(settings(&hub).get("tiles").is_none());
    assert_eq!(settings(&hub)["port"], 20000);
    hub.server.shutdown().await;
}

#[tokio::test]
async fn a_mode_the_app_has_not_or_a_word_that_is_none_is_refused_and_nothing_is_written() {
    let hub = Hub::new("hub-route-mode-bad", &[]).await;
    std::fs::create_dir_all(hub.machine.path("cfg-hub")).unwrap();
    for (path, body, status) in [
        ("/api/hub/apps/rules/mode", r#"{"mode":"web"}"#, 409),
        ("/api/hub/apps/testing/mode", r#"{"mode":"native"}"#, 409),
        ("/api/hub/apps/nope/mode", r#"{"mode":"web"}"#, 404),
        ("/api/hub/apps/tasks/mode", r#"{"mode":"fly"}"#, 400),
        (
            "/api/hub/apps/tasks/mode",
            r#"{"mode":"native","program":"x"}"#,
            400,
        ),
        ("/api/hub/apps/tasks/mode", r#"{}"#, 200),
        ("/api/hub/apps/tasks/mode", "not json", 400),
    ] {
        let reply = hub.post(path, body).await;
        assert_eq!(reply.status, status, "{path} {body}: {reply:?}");
    }
    // `{}` is "no mode" (the default): it wrote a file without tiles, never one with a bad word.
    let text = std::fs::read_to_string(hub.machine.path("cfg-hub/config.json")).unwrap_or_default();
    assert!(!text.contains("fly") && !text.contains("program"), "{text}");
    hub.server.shutdown().await;
}

#[tokio::test]
async fn a_rescan_is_a_post_with_a_session_and_answers_ok() {
    let hub = Hub::new("hub-route-rescan", &[]).await;
    let reply = hub.post("/api/hub/rescan", "{}").await;
    assert_eq!(reply.status, 200, "{reply:?}");
    assert_eq!(reply.json()["result"], "ok");
    hub.server.shutdown().await;
}

#[tokio::test]
async fn the_stop_route_asks_a_running_app_once_and_says_so_when_it_does_not_run() {
    let hub = Hub::new("hub-route-stop", &[]).await;
    let program = hub.machine.tasks_program();
    let idle = hub.post("/api/hub/apps/tasks/stop", "{}").await;
    assert_eq!(idle.status, 200, "{idle:?}");
    assert_eq!(idle.json()["result"], "not_running");
    let port = fake_server(200, hello(None)).await;
    write_hub_json(&hub.machine.path("cfg-tasks"), port, "abc123");
    let first = hub.post("/api/hub/apps/tasks/stop", "{}").await;
    assert_eq!(first.json()["result"], "stopping");
    let second = hub.post("/api/hub/apps/tasks/stop", "{}").await;
    assert_eq!(second.json()["result"], "starting");
    let end = std::time::Instant::now() + Duration::from_secs(15);
    while starts_of(&program).is_empty() && std::time::Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let starts = starts_of(&program);
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0]["argv"], serde_json::json!(["stop"]));
    for (id, status, code) in [
        ("nope", 404, "not_found"),
        ("docmap", 409, "not_available"),
        ("rules", 409, "not_installed"),
    ] {
        let reply = hub.post(&format!("/api/hub/apps/{id}/stop"), "{}").await;
        assert_eq!(reply.status, status, "{id}: {reply:?}");
        assert_eq!(reply.json()["error"]["code"], code);
    }
    hub.server.shutdown().await;
}

//! The switch of a tile for the start with the login (`POST /api/hub/apps/{id}/autostart`) and what the tiles
//! show (`autostart` of `GET /api/hub/apps`), with the kit's one state (`appkit_hub::loginstate`) on a login of
//! the test and a settings folder of the test: a change made by the app is what the tile shows, and a change
//! made by the tile is what the app reads. Never the login of the machine.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_core::identity::AppIdentity;
use appkit_hub::env::Env;
use appkit_hub::loginstate::{self, LoginEntries, HUB_DESKTOP};
use appkit_hub::{Limits, RunningServer};
use appkit_testkit::wire::{request, send, Reply};
use hub_desktop::catalog;
use hub_desktop::detect::Detector;
use hub_desktop::launch::{Browser, LaunchConfig, Launcher};
use hub_desktop::monitor::Monitor;
use hub_desktop::prefs::Prefs;
use hub_desktop::server::{self, ServerConfig};
use hub_desktop::ui::UiFiles;
use hub_desktop::update::Updater;

mod common;
use common::*;

/// The login entries of a machine made for the test: the apps that have one.
#[derive(Clone, Default)]
struct FakeLogin(Arc<Mutex<BTreeSet<String>>>);

impl FakeLogin {
    fn has(&self, app: &str) -> bool {
        self.0.lock().unwrap().contains(app)
    }
}

impl LoginEntries for FakeLogin {
    fn is_on(&self, identity: &AppIdentity) -> Option<bool> {
        Some(self.has(identity.app))
    }
    fn set(&self, identity: &AppIdentity, program: Option<&Path>) -> Result<(), String> {
        let mut held = self.0.lock().unwrap();
        if program.is_some() {
            held.insert(identity.app.to_owned());
        } else {
            held.remove(identity.app);
        }
        Ok(())
    }
}

struct Hub {
    server: RunningServer,
    token: String,
    login: FakeLogin,
    /// The settings folder of hub-desktop, which the kit finds through `HUB_DESKTOP_CONFIG_DIR`.
    settings: PathBuf,
    env: Env,
    _machine: Machine,
}

impl Hub {
    async fn new(name: &str) -> Self {
        let machine = Machine::new(name);
        machine.tasks_program();
        let settings = machine.path("cfg-hub");
        std::fs::create_dir_all(&settings).unwrap();
        let env = Env::from_pairs([("HUB_DESKTOP_CONFIG_DIR", settings.as_os_str().to_owned())]);
        let login = FakeLogin::default();
        let (reading, read_env) = (login.clone(), env.clone());
        let (writing, write_env) = (login.clone(), env.clone());
        let detector = Arc::new(
            Detector::with_ttl(machine.env(&[]), None, Duration::ZERO)
                .with_autostart(move |identity| loginstate::is_on(identity, &read_env, &reading)),
        );
        let prefs = Prefs::new(Some(settings.clone()), Arc::clone(&detector)).with_login(
            move |identity, on| {
                loginstate::set(
                    identity,
                    on,
                    || Ok(PathBuf::from("C:/apps/tasks-hub.exe")),
                    &write_env,
                    &writing,
                )
                .map(|_| ())
            },
        );
        let control = Secret::generate(TOKEN_BYTES).unwrap();
        let launcher = Arc::new(Launcher::new(
            Arc::clone(&detector),
            LaunchConfig::new(None, Browser::Log(machine.path("browser.log"))),
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
            prefs: Arc::new(prefs),
            monitor: Arc::new(Monitor::new(None)),
            updater: Arc::new(Updater::disabled(detector, launcher)),
        })
        .await
        .unwrap();
        let token = session(&server, &control.to_hex()).await;
        Self {
            server,
            token,
            login,
            settings,
            env,
            _machine: machine,
        }
    }

    async fn call(&self, method: &str, path: &str, body: &str) -> Reply {
        let host = format!("127.0.0.1:{}", self.server.addr().port());
        let origin = format!("http://{host}");
        let auth = format!("Bearer {}", self.token);
        let length = body.len().to_string();
        let mut headers = vec![
            ("Host", host.as_str()),
            ("Origin", origin.as_str()),
            ("Authorization", auth.as_str()),
        ];
        if method == "POST" {
            headers.push(("Content-Type", "application/json"));
            headers.push(("Content-Length", length.as_str()));
        }
        send(
            self.server.addr(),
            request(method, path, &headers, body.as_bytes()),
        )
        .await
    }

    async fn switch(&self, id: &str, body: &str) -> Reply {
        self.call("POST", &format!("/api/hub/apps/{id}/autostart"), body)
            .await
    }

    /// What the tile of `id` shows as its start with the login.
    async fn shown(&self, id: &str) -> serde_json::Value {
        let reply = self.call("GET", "/api/hub/apps", "").await;
        assert_eq!(reply.status, 200, "{reply:?}");
        reply.json()["apps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
            .unwrap()["autostart"]
            .clone()
    }

    fn list(&self) -> Vec<String> {
        loginstate::read_list(&self.settings).unwrap()
    }
}

fn tasks() -> &'static AppIdentity {
    catalog::find("tasks")
        .and_then(catalog::Entry::kit_identity)
        .unwrap()
}

#[tokio::test]
async fn the_tile_and_the_app_switch_one_state_and_show_what_the_other_did() {
    let hub = Hub::new("hub-autostart-both").await;
    // hub-desktop starts with the login: the switch of the tile goes into its list.
    hub.login
        .set(&HUB_DESKTOP, Some(Path::new("C:/apps/hub-desktop.exe")))
        .unwrap();
    assert_eq!(hub.shown("tasks").await, false);

    let on = hub.switch("tasks", r#"{"on":true}"#).await;
    assert_eq!(on.status, 200, "{on:?}");
    assert_eq!(on.json()["result"], "saved");
    assert_eq!(hub.list(), ["tasks"]);
    assert_eq!(hub.shown("tasks").await, true);
    // The app reads the same: its `autostart status` says hub-desktop starts it.
    assert_eq!(loginstate::is_on(tasks(), &hub.env, &hub.login), Some(true));

    // Switched off in the app (`tasks-hub autostart off`, the switch of its page): the tile shows it.
    loginstate::set(
        tasks(),
        false,
        || Err("unused".into()),
        &hub.env,
        &hub.login,
    )
    .unwrap();
    assert!(hub.list().is_empty());
    assert_eq!(hub.shown("tasks").await, false);

    // Switched on in the app while hub-desktop does not start with the login: its own entry, shown all the same.
    hub.login.set(&HUB_DESKTOP, None).unwrap();
    loginstate::set(
        tasks(),
        true,
        || Ok(PathBuf::from("C:/apps/tasks-hub.exe")),
        &hub.env,
        &hub.login,
    )
    .unwrap();
    assert!(hub.login.has("tasks-desktop"));
    assert_eq!(hub.shown("tasks").await, true);

    // And off from the tile: the own entry goes.
    let off = hub.switch("tasks", r#"{"on":false}"#).await;
    assert_eq!(off.status, 200, "{off:?}");
    assert!(!hub.login.has("tasks-desktop"));
    assert_eq!(hub.shown("tasks").await, false);
}

#[tokio::test]
async fn the_switch_refuses_what_is_not_an_app_of_the_kit_or_not_a_switch() {
    let hub = Hub::new("hub-autostart-refuse").await;
    assert_eq!(hub.switch("nope", r#"{"on":true}"#).await.status, 404);
    // docmap is no app of the kit: its start with the login is not the hub's to switch.
    let other = hub.switch("docmap", r#"{"on":true}"#).await;
    assert_eq!(other.status, 409, "{other:?}");
    for body in [
        "{}",
        r#"{"on":"yes"}"#,
        r#"{"on":true,"program":"C:/x.exe"}"#,
    ] {
        assert_eq!(hub.switch("tasks", body).await.status, 400, "{body}");
    }
    assert!(hub.list().is_empty());
    assert!(!hub.login.has("tasks-desktop"));
}

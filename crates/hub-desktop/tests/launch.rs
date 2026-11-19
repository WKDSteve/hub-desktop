//! Starting apps against a machine that is made for the test: the programs are the fake program (it logs its
//! argument list and working folder), the browser is a file, the servers are fakes on loopback ports.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hub_desktop::config::Env;
use hub_desktop::detect::Detector;
use hub_desktop::launch::{Browser, LaunchConfig, LaunchError, Launcher, Outcome, Request};
use hub_desktop::overlay::Overlay;

mod common;
use common::*;

/// A launcher on `machine`, with the browser written to `browser.log` in the machine's folder. The waits
/// are short: a start is on its way for 600 ms.
fn launcher(
    machine: &Machine,
    env: Env,
    web_root: Option<PathBuf>,
    web_port: Option<u16>,
) -> (Arc<Launcher>, PathBuf) {
    let log = machine.path("browser.log");
    let mut cfg = LaunchConfig::new(web_port, Browser::Log(log.clone()));
    cfg.ready_timeout = Duration::from_millis(600);
    cfg.settle = Duration::from_millis(600);
    let detector = Arc::new(Detector::with_ttl(env, web_root, Duration::ZERO));
    (Arc::new(Launcher::new(detector, cfg)), log)
}

fn opened(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .map(|t| t.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Wait until the fake program has logged `count` starts.
fn wait_for_starts(program: &Path, count: usize) {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        if starts_of(program).len() >= count {
            return;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    panic!(
        "the program did not start {count} time(s): {:?}",
        starts_of(program)
    );
}

fn argv(start: &serde_json::Value) -> Vec<String> {
    start["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_owned())
        .collect()
}

fn vault_env(machine: &Machine) -> Env {
    let vault = machine.path("vault");
    std::fs::create_dir_all(&vault).unwrap();
    machine.env(&[("TASKS_VAULT", vault.display().to_string())])
}

#[tokio::test]
async fn tasks_starts_as_its_own_hub_asks_with_start_open_in_its_own_folder() {
    let machine = Machine::new("hub-launch-tasks");
    let program = machine.tasks_program();
    let (launcher, _) = launcher(&machine, vault_env(&machine), None, None);
    assert_eq!(
        launcher.launch("tasks", Request::Default).await,
        Ok(Outcome::Started)
    );
    wait_for_starts(&program, 1);
    let starts = starts_of(&program);
    assert_eq!(starts.len(), 1);
    assert_eq!(argv(&starts[0]), ["start", "--open"]);
    // A click is no order to stay: the hub ends itself once its page is gone.
    assert_eq!(starts[0]["stay"], serde_json::json!([]));
    // The working folder is the folder of the program, not the one of the hub.
    let cwd = std::fs::canonicalize(starts[0]["cwd"].as_str().unwrap()).unwrap();
    assert_eq!(
        cwd,
        std::fs::canonicalize(program.parent().unwrap()).unwrap()
    );
}

/// The start with hub-desktop (the login): the hub of the app and no page.
#[tokio::test]
async fn a_hub_started_with_hub_desktop_gets_start_and_no_page() {
    let machine = Machine::new("hub-launch-background");
    let program = machine.tasks_program();
    let (launcher, log) = launcher(&machine, vault_env(&machine), None, None);
    assert_eq!(
        launcher.launch("tasks", Request::Background).await,
        Ok(Outcome::Started)
    );
    wait_for_starts(&program, 1);
    assert_eq!(argv(&starts_of(&program)[0]), ["start"]);
    // It is told to stay without a page, as a hub its own login entry started.
    assert_eq!(
        starts_of(&program)[0]["stay"],
        serde_json::json!(["TASKS_DESKTOP_STAY"])
    );
    assert!(opened(&log).is_empty());
}

/// Only an app of the kit has a hub to start in the background; an app that cannot be used yet is
/// not started (its questions would open a console at the login).
#[tokio::test]
async fn only_a_hub_of_the_kit_that_can_be_used_is_started_in_the_background() {
    let machine = Machine::new("hub-launch-background-other");
    let tasks = machine.tasks_program();
    let docmap = machine.program("Local/docmap-desktop/docmap-desktop.exe");
    let (launcher, _) = launcher(&machine, machine.env(&[]), None, None);
    assert_eq!(
        launcher.launch("docmap", Request::Background).await,
        Err(LaunchError::NotAvailable("background"))
    );
    assert_eq!(
        launcher.launch("tasks", Request::Background).await,
        Ok(Outcome::SetupRequired)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(starts_of(&docmap).is_empty() && starts_of(&tasks).is_empty());
}

/// pult keeps no vault: a click starts it right away, with the arguments of every hub of the kit.
#[tokio::test]
async fn pult_starts_as_a_hub_without_any_setup() {
    let machine = Machine::new("hub-launch-pult");
    let program = machine.program(&format!(
        "Local/pult/bin/{}",
        hub_desktop::catalog::PULT.program_file()
    ));
    let (launcher, _) = launcher(&machine, machine.env(&[]), None, None);
    assert_eq!(
        launcher.launch("pult", Request::Default).await,
        Ok(Outcome::Started)
    );
    wait_for_starts(&program, 1);
    assert_eq!(argv(&starts_of(&program)[0]), ["start", "--open"]);
}

/// Acceptance 6: a double click, or two tabs, start the app once.
#[tokio::test]
async fn many_clicks_at_once_start_one_program_and_the_rest_are_told_it_is_starting() {
    let machine = Machine::new("hub-launch-single");
    let program = machine.tasks_program();
    let (launcher, _) = launcher(&machine, vault_env(&machine), None, None);
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let launcher = Arc::clone(&launcher);
        tasks.push(tokio::spawn(async move {
            launcher.launch("tasks", Request::Default).await
        }));
    }
    let mut outcomes = Vec::new();
    for task in tasks {
        outcomes.push(task.await.unwrap());
    }
    let started = outcomes
        .iter()
        .filter(|o| **o == Ok(Outcome::Started))
        .count();
    let starting = outcomes
        .iter()
        .filter(|o| **o == Ok(Outcome::Starting))
        .count();
    assert_eq!((started, starting), (1, 11), "{outcomes:?}");
    wait_for_starts(&program, 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        starts_of(&program).len(),
        1,
        "one program, not one per click"
    );

    // Once the start is over (the app never came up: the time ran out), the next click starts again.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(
        launcher.launch("tasks", Request::Default).await,
        Ok(Outcome::Started)
    );
    wait_for_starts(&program, 2);
}

/// Apps do not hold each other up.
#[tokio::test]
async fn a_start_of_one_app_does_not_hold_up_another() {
    let machine = Machine::new("hub-launch-apps");
    let tasks = machine.tasks_program();
    let docmap = machine.program("Local/docmap-desktop/docmap-desktop.exe");
    let (launcher, _) = launcher(&machine, vault_env(&machine), None, None);
    assert_eq!(
        launcher.launch("tasks", Request::Default).await,
        Ok(Outcome::Started)
    );
    assert_eq!(
        launcher.launch("docmap", Request::Default).await,
        Ok(Outcome::Started)
    );
    wait_for_starts(&tasks, 1);
    wait_for_starts(&docmap, 1);
}

#[test]
fn a_log_line_that_is_still_being_written_is_not_a_start_yet() {
    let machine = Machine::new("hub-launch-halfline");
    let program = machine.program("Local/fake/fake.exe");
    std::fs::write(
        program.with_file_name("fake-app.log"),
        "{\"argv\":[]}
{\"argv\":[\"sta",
    )
    .unwrap();
    assert_eq!(starts_of(&program).len(), 1);
}

/// An app that is not set up opens its page, where its first start is (the vault, Neovim, the installation): the tile
/// and `setup` start `start --open`, never the program in a console of its own.
#[tokio::test]
async fn an_app_that_needs_setup_opens_its_page_for_the_first_start() {
    for (n, ask) in [Request::Default, Request::Native, Request::Setup]
        .into_iter()
        .enumerate()
    {
        let machine = Machine::new(&format!("hub-launch-setup-{n}"));
        let program = machine.tasks_program();
        // No vault: no settings, no variable.
        let (launcher, _) = launcher(&machine, machine.env(&[]), None, None);
        assert_eq!(
            launcher.launch("tasks", ask).await,
            Ok(Outcome::Started),
            "{ask:?}"
        );
        wait_for_starts(&program, 1);
        assert_eq!(
            argv(&starts_of(&program)[0]),
            ["start", "--open"],
            "{ask:?}"
        );
    }
}

#[tokio::test]
async fn docmap_starts_with_no_argument_and_has_no_setup() {
    let machine = Machine::new("hub-launch-docmap");
    let program = machine.program("Program Files/docmap-desktop/docmap-desktop.exe");
    let (launcher, _) = launcher(&machine, machine.env(&[]), None, None);
    assert_eq!(
        launcher.launch("docmap", Request::Setup).await,
        Err(LaunchError::NotAvailable("setup"))
    );
    assert_eq!(
        launcher.launch("docmap", Request::Default).await,
        Ok(Outcome::Started)
    );
    wait_for_starts(&program, 1);
    assert_eq!(argv(&starts_of(&program)[0]), Vec::<String>::new());
}

#[tokio::test]
async fn ai_that_runs_is_opened_in_the_browser_and_not_started_again() {
    let machine = Machine::new("hub-launch-ai-running");
    let program = machine.program("Local/Programs/ai-desktop/app/ai-desktop.exe");
    let port = fake_server(
        200,
        r#"{"status":"ok","model_router_ready":true,"backends":[]}"#.to_owned(),
    )
    .await;
    let env = machine.env(&[("AI_DESKTOP_PORT", port.to_string())]);
    let (launcher, browser) = launcher(&machine, env, None, None);
    assert_eq!(
        launcher.launch("ai", Request::Default).await,
        Ok(Outcome::Opened)
    );
    assert_eq!(opened(&browser), [format!("http://127.0.0.1:{port}/")]);
    assert!(starts_of(&program).is_empty());
}

/// A fake ai-desktop: `/health` says its app and instance, `/session/ticket` gives `ticket` only for
/// `Bearer <secret>`. Every request line is kept, with whether it carried the secret.
async fn fake_ai(
    instance: &str,
    secret: &str,
    ticket: &str,
) -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let health = format!(
        r#"{{"status":"ready","model_router_ready":true,"app":"ai-desktop","instance":"{instance}","backends":{{}}}}"#
    );
    let auth = format!("authorization: bearer {secret}");
    let ticket = format!(r#"{{"ticket":"{ticket}","expires_in":120}}"#);
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let (health, auth, ticket, log) = (
                health.clone(),
                auth.clone(),
                ticket.clone(),
                Arc::clone(&log),
            );
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = text.lines().next().unwrap_or("").to_owned();
                let with_secret = text.to_ascii_lowercase().contains(&auth);
                log.lock()
                    .unwrap()
                    .push(format!("{line} secret={with_secret}"));
                let (status, body) = if line.starts_with("GET /health ") {
                    (200, health)
                } else if line.starts_with("POST /session/ticket ") && with_secret {
                    (200, ticket)
                } else {
                    (401, r#"{"error":{"message":"no"}}"#.to_owned())
                };
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            });
        }
    });
    (port, seen)
}

/// hub.json and control.secret of ai-desktop, in the folder the variable names.
fn ai_files(dir: &Path, port: u16, instance: &str, secret: &str) {
    appkit_core::registry::write_record(
        dir,
        &appkit_core::registry::HubRecord {
            app: "ai-desktop".to_owned(),
            port,
            pid: 4242,
            version: "0.0.87".to_owned(),
            instance: instance.to_owned(),
        },
    )
    .unwrap();
    appkit_core::registry::write_secret(dir, secret).unwrap();
}

/// The tile opened the dashboard without a ticket, and it stayed "not signed in" (2026-11-18): it asks
/// ai-desktop for a one-time ticket with its control secret, as a program of the family does.
#[tokio::test]
async fn ai_that_runs_is_opened_signed_in_with_a_ticket() {
    let machine = Machine::new("hub-launch-ai-ticket");
    machine.program("Local/Programs/ai-desktop/app/ai-desktop.exe");
    let (port, seen) = fake_ai("inst1", "c0ffee", "0a1b2c").await;
    let cfg = machine.path("cfg-ai");
    ai_files(&cfg, port, "inst1", "c0ffee");
    let env = machine.env(&[
        ("AI_DESKTOP_PORT", port.to_string()),
        ("AI_DESKTOP_CONFIG_DIR", cfg.display().to_string()),
    ]);
    let (launcher, browser) = launcher(&machine, env, None, None);
    assert_eq!(
        launcher.launch("ai", Request::Default).await,
        Ok(Outcome::Opened)
    );
    assert_eq!(
        opened(&browser),
        [format!("http://127.0.0.1:{port}/#ticket=0a1b2c")]
    );
    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.contains(&"POST /session/ticket HTTP/1.1 secret=true".to_owned()),
        "{seen:?}"
    );
    // The secret goes only to the ticket route.
    assert!(
        seen.iter()
            .filter(|l| l.ends_with("secret=true"))
            .all(|l| l.starts_with("POST /session/ticket ")),
        "{seen:?}"
    );
}

/// Without the variable the files are in `%APPDATA%\ai-desktop`, as ai-desktop writes them; never in the folder
/// of its earlier name (loomai), which only its config.json may still use.
#[tokio::test]
async fn ai_finds_its_files_below_appdata_and_never_in_the_folder_of_its_earlier_name() {
    let machine = Machine::new("hub-launch-ai-ticket-appdata");
    machine.program("Local/Programs/ai-desktop/app/ai-desktop.exe");
    let (port, seen) = fake_ai("inst2", "beef", "ff00").await;
    ai_files(&machine.path("Roaming/loomai"), port, "inst2", "beef");
    let env = machine.env(&[("AI_DESKTOP_PORT", port.to_string())]);
    let (first, browser) = launcher(&machine, env.clone(), None, None);
    assert_eq!(
        first.launch("ai", Request::Default).await,
        Ok(Outcome::Opened)
    );
    assert_eq!(opened(&browser), [format!("http://127.0.0.1:{port}/")]);
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|l| l.ends_with("secret=false")),
        "the loomai folder is not read"
    );

    ai_files(&machine.path("Roaming/ai-desktop"), port, "inst2", "beef");
    // A launcher of its own (no start of the first on its way); the same browser log, read afresh.
    let (second, browser) = launcher(&machine, env, None, None);
    std::fs::remove_file(&browser).ok();
    assert_eq!(
        second.launch("ai", Request::Default).await,
        Ok(Outcome::Opened)
    );
    assert_eq!(
        opened(&browser),
        [format!("http://127.0.0.1:{port}/#ticket=ff00")]
    );
}

/// A hub.json of another start (a server that was killed, or another program on the port) gets no secret:
/// the page opens as it is and says "not signed in" itself.
#[tokio::test]
async fn ai_of_another_instance_gets_no_secret_and_the_page_opens_without_a_ticket() {
    let machine = Machine::new("hub-launch-ai-ticket-stale");
    machine.program("Local/Programs/ai-desktop/app/ai-desktop.exe");
    let (port, seen) = fake_ai("now", "c0ffee", "0a1b2c").await;
    let cfg = machine.path("cfg-ai");
    ai_files(&cfg, port, "before", "c0ffee");
    let env = machine.env(&[
        ("AI_DESKTOP_PORT", port.to_string()),
        ("AI_DESKTOP_CONFIG_DIR", cfg.display().to_string()),
    ]);
    let (launcher, browser) = launcher(&machine, env, None, None);
    assert_eq!(
        launcher.launch("ai", Request::Default).await,
        Ok(Outcome::Opened)
    );
    assert_eq!(opened(&browser), [format!("http://127.0.0.1:{port}/")]);
    let seen = seen.lock().unwrap().clone();
    assert!(seen.iter().all(|l| l.ends_with("secret=false")), "{seen:?}");
}

/// Acceptance 7: something else on the port of ai-desktop is no reason to start over it.
#[tokio::test]
async fn a_foreign_program_on_the_port_of_ai_is_a_port_busy_and_nothing_starts() {
    let machine = Machine::new("hub-launch-ai-busy");
    let program = machine.program("Local/Programs/ai-desktop/app/ai-desktop.exe");
    let port = fake_server(200, r#"{"hello":"world"}"#.to_owned()).await;
    let env = machine.env(&[("AI_DESKTOP_PORT", port.to_string())]);
    let (launcher, browser) = launcher(&machine, env, None, None);
    assert_eq!(
        launcher.launch("ai", Request::Default).await,
        Err(LaunchError::PortBusy)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(starts_of(&program).is_empty());
    assert!(opened(&browser).is_empty());
}

#[tokio::test]
async fn ai_that_is_stopped_is_started_and_its_page_is_opened_once_it_answers() {
    let machine = Machine::new("hub-launch-ai-start");
    let program = machine.program("Local/Programs/ai-desktop/app/ai-desktop.exe");
    let port = free_port();
    let env = machine.env(&[("AI_DESKTOP_PORT", port.to_string())]);
    let (launcher, browser) = launcher(&machine, env, None, None);
    assert_eq!(
        launcher.launch("ai", Request::Default).await,
        Ok(Outcome::Started)
    );
    wait_for_starts(&program, 1);
    assert_eq!(argv(&starts_of(&program)[0]), Vec::<String>::new());
    assert!(
        opened(&browser).is_empty(),
        "nothing to open before it answers"
    );
    // It comes up.
    fake_server_at(
        port,
        200,
        r#"{"status":"ok","model_router_ready":false,"backends":[]}"#.to_owned(),
    )
    .await;
    let end = Instant::now() + Duration::from_secs(5);
    while opened(&browser).is_empty() && Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(opened(&browser), [format!("http://127.0.0.1:{port}/")]);
}

#[tokio::test]
async fn a_web_variant_is_opened_from_the_listener_of_the_hub_and_never_as_a_file() {
    let machine = Machine::new("hub-launch-web");
    machine.touch("Web/tasks-web/tasks.html");
    let root = machine.path("Web");
    let (launcher, browser) = launcher(&machine, machine.env(&[]), Some(root.clone()), Some(17611));
    // Not installed natively: the web variant is the default.
    assert_eq!(
        launcher.launch("tasks", Request::Default).await,
        Ok(Outcome::Opened)
    );
    assert_eq!(
        opened(&browser),
        ["http://127.0.0.1:17611/tasks-web/tasks.html"]
    );

    // An app with no web variant has no web mode; one whose page is not there is not installed.
    let (launcher, _) = self::launcher(&machine, machine.env(&[]), Some(root.clone()), Some(17611));
    assert_eq!(
        launcher.launch("rules", Request::Web).await,
        Err(LaunchError::NotAvailable("web"))
    );
    let empty = machine.path("Empty");
    let (launcher, _) = self::launcher(&machine, machine.env(&[]), Some(empty), Some(17611));
    assert_eq!(
        launcher.launch("tasks", Request::Web).await,
        Err(LaunchError::NotInstalled)
    );

    // Without the listener there is no web mode.
    let (launcher, browser) = self::launcher(&machine, machine.env(&[]), Some(root), None);
    assert_eq!(
        launcher.launch("tasks", Request::Web).await,
        Err(LaunchError::WebUnavailable)
    );
    // The page of the first click is the only one in the log: nothing was opened for this one.
    assert_eq!(opened(&browser).len(), 1);
}

#[tokio::test]
async fn the_other_mode_is_one_click_away_and_both_modes_start_the_right_thing() {
    let machine = Machine::new("hub-launch-both");
    let program = machine.tasks_program();
    machine.touch("Web/tasks-web/tasks.html");
    let (launcher, browser) = launcher(
        &machine,
        vault_env(&machine),
        Some(machine.path("Web")),
        Some(17611),
    );
    // The default is native.
    assert_eq!(
        launcher.launch("tasks", Request::Default).await,
        Ok(Outcome::Started)
    );
    wait_for_starts(&program, 1);
    tokio::time::sleep(Duration::from_millis(1000)).await;
    // The other one, as asked.
    assert_eq!(
        launcher.launch("tasks", Request::Web).await,
        Ok(Outcome::Opened)
    );
    assert_eq!(
        opened(&browser),
        ["http://127.0.0.1:17611/tasks-web/tasks.html"]
    );
    assert_eq!(starts_of(&program).len(), 1);
}

#[tokio::test]
async fn what_is_not_installed_or_has_no_program_or_is_unknown_is_refused() {
    let machine = Machine::new("hub-launch-refused");
    let (launcher, _) = launcher(&machine, machine.env(&[]), None, Some(17611));
    for (id, want) in [
        ("tasks", LaunchError::NotInstalled),
        ("rules", LaunchError::NotInstalled),
        ("docmap", LaunchError::NotInstalled),
        ("ai", LaunchError::NotInstalled),
        ("streamtap", LaunchError::NotInstalled),
        ("pult", LaunchError::NotInstalled),
        // Planned, with no program and no web variant: there is nothing to install yet.
        ("takt", LaunchError::NotAvailable("native")),
        ("testing", LaunchError::NotInstalled),
        ("nope", LaunchError::UnknownApp),
        ("hub", LaunchError::UnknownApp),
        ("", LaunchError::UnknownApp),
        ("../tasks", LaunchError::UnknownApp),
        ("tasks ", LaunchError::UnknownApp),
        ("TASKS", LaunchError::UnknownApp),
    ] {
        assert_eq!(
            launcher.launch(id, Request::Default).await,
            Err(want),
            "{id:?}"
        );
    }
}

/// Nothing of a request reaches a command line: whatever is asked, a program gets one of the three argument
/// lists that are written in the code.
#[tokio::test]
async fn a_program_only_ever_gets_one_of_the_argument_lists_of_the_code() {
    let machine = Machine::new("hub-launch-argv");
    let tasks = machine.tasks_program();
    let docmap = machine.program("Local/docmap-desktop/docmap-desktop.exe");
    let ai = machine.program("Local/Programs/ai-desktop/app/ai-desktop.exe");
    let env = {
        let vault = machine.path("vault");
        std::fs::create_dir_all(&vault).unwrap();
        machine.env(&[("TASKS_VAULT", vault.display().to_string())])
    };
    let (launcher, _) = launcher(&machine, env, None, Some(17611));
    for id in ["tasks", "docmap", "ai"] {
        // The start in the background first: after any other the hub of the app counts as running or starting.
        for ask in [
            Request::Background,
            Request::Default,
            Request::Native,
            Request::Web,
            Request::Setup,
        ] {
            let _ = launcher.launch(id, ask).await;
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for program in [&tasks, &docmap, &ai] {
        for start in starts_of(program) {
            seen.insert(argv(&start));
        }
    }
    let allowed: std::collections::BTreeSet<Vec<String>> = [
        vec![],
        vec!["start".to_owned(), "--open".to_owned()],
        vec!["start".to_owned()],
    ]
    .into_iter()
    .collect();
    assert!(seen.is_subset(&allowed), "{seen:?}");
    assert_eq!(
        seen, allowed,
        "every list of the code is used once at least"
    );
}

#[test]
fn the_browser_opens_only_loopback_pages_of_the_ports_it_was_given() {
    let machine = Machine::new("hub-launch-browser");
    let log = machine.path("browser.log");
    let browser = Browser::Log(log.clone());
    for bad in [
        "file:///C:/Windows/System32/calc.exe",
        "http://127.0.0.1:9/x",
        "http://127.0.0.1:17611@evil.example/x",
        "http://localhost:17611/x",
        "https://127.0.0.1:17611/x",
        "http://127.0.0.1:17611/x y",
        "http://127.0.0.1:17611",
        "http://127.0.0.1:017611/x",
        "http://127.0.0.1:17611/a\\b",
        "javascript:alert(1)",
    ] {
        assert!(browser.open(bad, &[17611]).is_err(), "{bad}");
    }
    assert!(!log.exists(), "a refused link leaves no trace");
    assert!(browser
        .open("http://127.0.0.1:17611/tasks-web/tasks.html", &[17611])
        .is_ok());
    assert_eq!(
        opened(&log),
        ["http://127.0.0.1:17611/tasks-web/tasks.html"]
    );
}

/// A stop is the program of the app asked with the one word `stop`, and only for an app that runs.
#[tokio::test]
async fn a_running_hub_is_asked_to_stop_with_stop_and_nothing_else() {
    let machine = Machine::new("hub-launch-stop");
    let program = machine.tasks_program();
    let port = fake_server(200, hello(None)).await;
    write_hub_json(&machine.path("cfg-tasks"), port, "abc123");
    let (launcher, _) = launcher(&machine, vault_env(&machine), None, None);
    assert_eq!(launcher.stop("tasks").await, Ok(Outcome::Stopping));
    wait_for_starts(&program, 1);
    assert_eq!(argv(&starts_of(&program)[0]), ["stop"]);
}

/// The start with hub-desktop leaves a hub that runs alone: no program is started for it.
#[tokio::test]
async fn a_hub_that_runs_is_not_started_again_with_hub_desktop() {
    let machine = Machine::new("hub-launch-background-running");
    let program = machine.tasks_program();
    let port = fake_server(200, hello(None)).await;
    write_hub_json(&machine.path("cfg-tasks"), port, "abc123");
    let (launcher, _) = launcher(&machine, vault_env(&machine), None, None);
    assert_eq!(
        launcher.launch("tasks", Request::Background).await,
        Ok(Outcome::Running)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(starts_of(&program).is_empty());
    // And the next start is not held up by it.
    assert_eq!(launcher.stop("tasks").await, Ok(Outcome::Stopping));
}

#[tokio::test]
async fn an_app_that_does_not_run_is_not_asked_to_stop() {
    let machine = Machine::new("hub-launch-stop-idle");
    let program = machine.tasks_program();
    let (launcher, _) = launcher(&machine, vault_env(&machine), None, None);
    assert_eq!(launcher.stop("tasks").await, Ok(Outcome::NotRunning));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(starts_of(&program).is_empty());
}

#[tokio::test]
async fn only_an_appkit_app_that_is_there_can_be_stopped() {
    let machine = Machine::new("hub-launch-stop-kinds");
    let docmap = machine.program("Local/docmap-desktop/docmap-desktop.exe");
    let (launcher, _) = launcher(&machine, vault_env(&machine), None, None);
    assert_eq!(launcher.stop("nope").await, Err(LaunchError::UnknownApp));
    assert_eq!(launcher.stop("rules").await, Err(LaunchError::NotInstalled));
    assert_eq!(
        launcher.stop("docmap").await,
        Err(LaunchError::NotAvailable("stop"))
    );
    assert_eq!(
        launcher.stop("streamtap").await,
        Err(LaunchError::NotAvailable("stop"))
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(starts_of(&docmap).is_empty());
}

#[tokio::test]
async fn a_second_stop_while_one_is_on_its_way_does_nothing_more() {
    let machine = Machine::new("hub-launch-stop-twice");
    let program = machine.tasks_program();
    let port = fake_server(200, hello(None)).await;
    write_hub_json(&machine.path("cfg-tasks"), port, "abc123");
    let (launcher, _) = launcher(&machine, vault_env(&machine), None, None);
    assert_eq!(launcher.stop("tasks").await, Ok(Outcome::Stopping));
    assert_eq!(launcher.stop("tasks").await, Ok(Outcome::Starting));
    wait_for_starts(&program, 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(starts_of(&program).len(), 1);
}

/// A hidden app is not there for the hub: not started, not stopped.
#[tokio::test]
async fn an_app_the_overlay_hides_cannot_be_started_or_stopped() {
    let machine = Machine::new("hub-launch-hidden");
    let program = machine.tasks_program();
    let port = fake_server(200, hello(None)).await;
    write_hub_json(&machine.path("cfg-tasks"), port, "abc123");
    let overlay = Overlay::parse(r#"{"schema":"hub-catalog/1","hidden":["tasks"]}"#).unwrap();
    let detector = Arc::new(
        Detector::with_ttl(vault_env(&machine), None, Duration::ZERO).with_overlay(overlay),
    );
    let launcher = Arc::new(Launcher::new(
        detector,
        LaunchConfig::new(None, Browser::Log(machine.path("browser.log"))),
    ));
    assert_eq!(
        launcher.launch("tasks", Request::Default).await,
        Err(LaunchError::UnknownApp)
    );
    assert_eq!(launcher.stop("tasks").await, Err(LaunchError::UnknownApp));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(starts_of(&program).is_empty());
}

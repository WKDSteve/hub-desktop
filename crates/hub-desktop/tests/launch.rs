//! Starting apps against a machine that is made for the test: the programs are the fake program (it logs its
//! argument list and working folder), the browser is a file, the servers are fakes on loopback ports.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hub_desktop::config::Env;
use hub_desktop::detect::Detector;
use hub_desktop::launch::{Browser, LaunchConfig, LaunchError, Launcher, Outcome, Request};

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
    // The working folder is the folder of the program, not the one of the hub.
    let cwd = std::fs::canonicalize(starts[0]["cwd"].as_str().unwrap()).unwrap();
    assert_eq!(
        cwd,
        std::fs::canonicalize(program.parent().unwrap()).unwrap()
    );
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

#[tokio::test]
async fn an_app_that_needs_setup_is_not_started_but_its_setup_is() {
    let machine = Machine::new("hub-launch-setup");
    let program = machine.tasks_program();
    // No vault: no settings, no variable.
    let (launcher, _) = launcher(&machine, machine.env(&[]), None, None);
    assert_eq!(
        launcher.launch("tasks", Request::Default).await,
        Ok(Outcome::SetupRequired)
    );
    assert_eq!(
        launcher.launch("tasks", Request::Native).await,
        Ok(Outcome::SetupRequired)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        starts_of(&program).is_empty(),
        "nothing is started for an app that cannot work"
    );

    // The setup is the program as a person starts it: no argument.
    assert_eq!(
        launcher.launch("tasks", Request::Setup).await,
        Ok(Outcome::Started)
    );
    wait_for_starts(&program, 1);
    assert_eq!(argv(&starts_of(&program)[0]), Vec::<String>::new());
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
    assert_eq!(
        launcher.launch("testing", Request::Web).await,
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
        ("streamtap", LaunchError::NotAvailable("native")),
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
        for ask in [
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
    let allowed: std::collections::BTreeSet<Vec<String>> =
        [vec![], vec!["start".to_owned(), "--open".to_owned()]]
            .into_iter()
            .collect();
    assert!(seen.is_subset(&allowed), "{seen:?}");
    assert!(
        seen.contains(&Vec::<String>::new()) && seen.len() == 2,
        "{seen:?}"
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

//! The detection against a machine that is made for the test: fake roots (`LOCALAPPDATA`,
//! `PROGRAMFILES`, the home folder, the config folders of the apps), fake programs that are files with
//! a name, and fake servers on loopback ports. The real machine is never looked at.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use hub_desktop::catalog::{self, AI_PORT};
use hub_desktop::config::Env;
use hub_desktop::detect::{AppState, Detector, Run};

mod common;
use common::*;

/// Every look at the machine, fresh.
async fn look(env: Env, web_root: Option<PathBuf>) -> Vec<AppState> {
    let detector = Detector::with_ttl(env, web_root, Duration::ZERO);
    detector.apps().await.as_ref().clone()
}

fn app<'a>(all: &'a [AppState], id: &str) -> &'a AppState {
    all.iter().find(|a| a.entry.id == id).expect(id)
}

#[tokio::test]
async fn on_an_empty_machine_nothing_is_installed_and_nothing_has_a_state() {
    let machine = Machine::new("hub-detect-empty");
    let all = look(machine.env(&[]), None).await;
    assert_eq!(all.len(), catalog::CATALOG.len());
    for state in &all {
        assert!(!state.installed(), "{}", state.entry.id);
        assert!(
            state.run.is_none() && state.setup.is_none(),
            "{}",
            state.entry.id
        );
    }
}

#[tokio::test]
async fn the_order_of_the_catalog_is_the_order_of_the_answer() {
    let machine = Machine::new("hub-detect-order");
    let all = look(machine.env(&[]), None).await;
    let ids: Vec<_> = all.iter().map(|a| a.entry.id).collect();
    let want: Vec<_> = catalog::CATALOG.iter().map(|e| e.id).collect();
    assert_eq!(ids, want);
}

#[tokio::test]
async fn a_program_in_the_install_folder_is_installed_and_one_at_the_override_too() {
    let machine = Machine::new("hub-detect-installed");
    let program = machine.tasks_program();
    let all = look(machine.env(&[]), None).await;
    let tasks = app(&all, "tasks");
    assert!(tasks.installed());
    assert_eq!(
        tasks.native.as_deref(),
        Some(std::fs::canonicalize(&program).unwrap().as_path())
    );
    assert!(!app(&all, "rules").installed());

    // The override names another folder: the program has to be there, not in the default one.
    let elsewhere = machine.path("Elsewhere");
    let env = machine.env(&[("TASKS_DESKTOP_INSTALL_DIR", elsewhere.display().to_string())]);
    assert!(!app(&look(env.clone(), None).await, "tasks").installed());
    machine.touch(&format!("Elsewhere/{}", catalog::TASKS.program_file()));
    assert!(app(&look(env, None).await, "tasks").installed());
}

#[tokio::test]
async fn docmap_is_found_in_either_place_and_has_no_state() {
    let machine = Machine::new("hub-detect-docmap");
    machine.touch("Program Files/docmap-desktop/docmap-desktop.exe");
    let all = look(machine.env(&[]), None).await;
    let docmap = app(&all, "docmap");
    assert!(docmap.installed() && docmap.run.is_none());
    let machine = Machine::new("hub-detect-docmap-local");
    machine.touch("Local/docmap-desktop/docmap-desktop.exe");
    assert!(app(&look(machine.env(&[]), None).await, "docmap").installed());
}

#[tokio::test]
async fn a_running_hub_is_believed_only_when_it_says_the_instance_of_its_file() {
    let machine = Machine::new("hub-detect-running");
    machine.tasks_program();
    let port = fake_server(200, hello(None)).await;

    write_hub_json(&machine.path("cfg-tasks"), port, "abc123");
    let all = look(machine.env(&[]), None).await;
    assert_eq!(app(&all, "tasks").run, Some(Run::Running));
    assert_eq!(app(&all, "tasks").setup, None);

    // The file of an earlier hub, and another program on its port.
    write_hub_json(&machine.path("cfg-tasks"), port, "stale");
    let all = look(machine.env(&[]), None).await;
    assert_eq!(app(&all, "tasks").run, Some(Run::Stopped));
}

#[tokio::test]
async fn a_hub_file_that_names_a_system_port_or_a_foreign_app_is_not_asked() {
    let machine = Machine::new("hub-detect-record");
    machine.tasks_program();
    // A port below 1024 that is not the catalog's.
    write_hub_json(&machine.path("cfg-tasks"), 80, "abc123");
    let all = look(machine.env(&[]), None).await;
    assert_eq!(app(&all, "tasks").run, Some(Run::Stopped));
    // A file of another app is no record of this one (it is refused when it is read).
    std::fs::write(
        machine.path("cfg-tasks/hub.json"),
        r#"{"app":"someone-else","port":20000,"pid":1,"version":"1","instance":"abc123"}"#,
    )
    .unwrap();
    let all = look(machine.env(&[]), None).await;
    assert_eq!(app(&all, "tasks").run, Some(Run::Stopped));
}

#[tokio::test]
async fn a_hub_that_says_it_needs_setup_is_running_and_needs_setup() {
    let machine = Machine::new("hub-detect-setup-running");
    machine.tasks_program();
    let port = fake_server(200, hello(Some("no_vault"))).await;
    write_hub_json(&machine.path("cfg-tasks"), port, "abc123");
    let all = look(machine.env(&[]), None).await;
    assert_eq!(app(&all, "tasks").run, Some(Run::Running));
    assert_eq!(app(&all, "tasks").setup.as_deref(), Some("no_vault"));

    // A reason that is a sentence or a path is not shown as it is.
    let port = fake_server(
        200,
        serde_json::json!({"app":"tasks-desktop","instance":"abc123","setup_required":true,
                           "setup_reason":"C:\\Users\\me\\secret vault"})
        .to_string(),
    )
    .await;
    write_hub_json(&machine.path("cfg-tasks"), port, "abc123");
    let all = look(machine.env(&[]), None).await;
    assert_eq!(app(&all, "tasks").setup.as_deref(), Some("setup_required"));
}

#[tokio::test]
async fn a_hub_that_does_not_run_needs_setup_when_its_settings_name_no_usable_vault() {
    let machine = Machine::new("hub-detect-setup-stopped");
    machine.tasks_program();
    let cfg = machine.path("cfg-tasks");
    std::fs::create_dir_all(&cfg).unwrap();
    let vault = machine.path("vault");
    std::fs::create_dir_all(&vault).unwrap();

    // No settings, no variable.
    let all = look(machine.env(&[]), None).await;
    assert_eq!(app(&all, "tasks").setup.as_deref(), Some("no_vault"));
    assert_eq!(app(&all, "tasks").run, Some(Run::Stopped));

    // The variable names a folder.
    let env = machine.env(&[("TASKS_VAULT", vault.display().to_string())]);
    assert_eq!(app(&look(env, None).await, "tasks").setup, None);

    // The variable names something that is not a folder.
    let file = machine.touch("not-a-folder.txt");
    let env = machine.env(&[("TASKS_VAULT", file.display().to_string())]);
    assert_eq!(
        app(&look(env, None).await, "tasks").setup.as_deref(),
        Some("vault_not_a_directory")
    );

    // The settings name a folder (and win over the variable).
    std::fs::write(
        cfg.join("config.json"),
        serde_json::json!({ "vault": vault }).to_string(),
    )
    .unwrap();
    let env = machine.env(&[("TASKS_VAULT", file.display().to_string())]);
    assert_eq!(app(&look(env, None).await, "tasks").setup, None);

    // Several vaults, none of them the main one.
    std::fs::write(
        cfg.join("config.json"),
        r#"{"vaults":[{"name":"a","path":"C:/x"}]}"#,
    )
    .unwrap();
    assert_eq!(
        app(&look(machine.env(&[]), None).await, "tasks").setup,
        None
    );

    // A settings file that is no JSON, or is far too large, says nothing.
    std::fs::write(cfg.join("config.json"), "{ nope").unwrap();
    assert_eq!(
        app(&look(machine.env(&[]), None).await, "tasks")
            .setup
            .as_deref(),
        Some("no_vault")
    );
    std::fs::write(
        cfg.join("config.json"),
        format!(
            r#"{{"vault":"{}","x":"{}"}}"#,
            vault.display().to_string().replace('\\', "/"),
            "a".repeat(70_000)
        ),
    )
    .unwrap();
    assert_eq!(
        app(&look(machine.env(&[]), None).await, "tasks")
            .setup
            .as_deref(),
        Some("no_vault")
    );
}

#[tokio::test]
async fn ai_runs_when_health_has_its_keys_and_blocks_a_port_that_answers_otherwise() {
    let machine = Machine::new("hub-detect-ai");
    machine.touch("Local/Programs/ai-desktop/app/ai-desktop.exe");
    let health = r#"{"status":"ok","model_router_ready":true,"backends":[]}"#;

    let port = fake_server(200, health.to_owned()).await;
    let env = machine.env(&[("AI_DESKTOP_PORT", port.to_string())]);
    assert_eq!(app(&look(env, None).await, "ai").run, Some(Run::Running));

    let port = fake_server(200, r#"{"hello":"world"}"#.to_owned()).await;
    let env = machine.env(&[("AI_DESKTOP_PORT", port.to_string())]);
    assert_eq!(app(&look(env, None).await, "ai").run, Some(Run::PortBusy));

    let port = fake_server(500, health.to_owned()).await;
    let env = machine.env(&[("AI_DESKTOP_PORT", port.to_string())]);
    assert_eq!(app(&look(env, None).await, "ai").run, Some(Run::PortBusy));

    let port = hanging_server().await;
    let env = machine.env(&[("AI_DESKTOP_PORT", port.to_string())]);
    assert_eq!(app(&look(env, None).await, "ai").run, Some(Run::PortBusy));

    // Nobody on the port: stopped (the port of the environment is a free one).
    assert_eq!(
        app(&look(machine.env(&[]), None).await, "ai").run,
        Some(Run::Stopped)
    );
    let _ = AI_PORT;
}

/// Acceptance 1: the page is there in about a second although a port takes the connection and says
/// nothing. The ports are asked at the same time, so two of them that hang cost one budget.
#[tokio::test]
async fn hanging_ports_cost_one_budget_together_not_one_each() {
    let machine = Machine::new("hub-detect-hang");
    machine.tasks_program();
    machine.touch("Local/Programs/ai-desktop/app/ai-desktop.exe");
    let hung_hub = hanging_server().await;
    write_hub_json(&machine.path("cfg-tasks"), hung_hub, "abc123");
    let hung_ai = hanging_server().await;
    let env = machine.env(&[("AI_DESKTOP_PORT", hung_ai.to_string())]);

    let start = Instant::now();
    let all = look(env, None).await;
    let took = start.elapsed();
    assert!(took < Duration::from_millis(900), "{took:?}");
    assert_eq!(app(&all, "tasks").run, Some(Run::Stopped));
    assert_eq!(app(&all, "ai").run, Some(Run::PortBusy));
}

#[tokio::test]
async fn a_web_variant_below_the_web_root_counts_as_installed_and_opens_as_web() {
    let machine = Machine::new("hub-detect-web");
    machine.touch("Web/tasks-web/tasks.html");
    let root = machine.path("Web");
    let all = look(machine.env(&[]), Some(root.clone())).await;
    let tasks = app(&all, "tasks");
    assert!(tasks.installed() && tasks.native.is_none() && tasks.web.is_some());
    assert_eq!(tasks.mode(), catalog::Mode::Web);
    assert!(tasks.run.is_none());

    // With a native install too, the default (native) wins.
    machine.tasks_program();
    let all = look(machine.env(&[]), Some(root)).await;
    assert_eq!(app(&all, "tasks").mode(), catalog::Mode::Native);

    // Testing is planned, and counts as installed as soon as its page is there.
    assert!(!app(&all, "testing").installed());
    machine.touch("Web/testing-web/testing.html");
    let all = look(machine.env(&[]), Some(machine.path("Web"))).await;
    let testing = app(&all, "testing");
    assert!(testing.installed());
    assert_eq!(testing.view().status, "planned");
}

#[tokio::test]
async fn without_a_web_root_the_checkouts_are_looked_for_in_the_repos_folder_and_the_home_folder() {
    let machine = Machine::new("hub-detect-webdirs");
    machine.touch("Repos/tasks-web/tasks.html");
    let env = machine.env(&[("REPOS_DIR", machine.path("Repos").display().to_string())]);
    assert!(app(&look(env, None).await, "tasks").installed());

    let machine = Machine::new("hub-detect-webdirs-home");
    machine.touch("Home/hub-web-apps/tasks-web/tasks.html");
    assert!(app(&look(machine.env(&[]), None).await, "tasks").installed());

    // A web root of the settings replaces the places, it does not add to them.
    let env = machine.env(&[]);
    let elsewhere = machine.path("Nothing");
    std::fs::create_dir_all(&elsewhere).unwrap();
    assert!(!app(&look(env, Some(elsewhere)).await, "tasks").installed());
}

#[tokio::test]
async fn a_web_page_that_is_a_link_out_of_the_web_root_is_not_found() {
    let machine = Machine::new("hub-detect-weblink");
    let outside = machine.touch("Outside/tasks-web/tasks.html");
    let root = machine.path("Web");
    std::fs::create_dir_all(&root).unwrap();
    link(&machine.path("Outside/tasks-web"), &root.join("tasks-web"));
    assert!(outside.exists());
    assert!(!app(&look(machine.env(&[]), Some(root)).await, "tasks").installed());
}

#[tokio::test]
async fn the_view_for_the_page_holds_no_path_of_the_machine() {
    let machine = Machine::new("hub-detect-view");
    machine.tasks_program();
    machine.touch("Local/docmap-desktop/docmap-desktop.exe");
    machine.touch("Web/tasks-web/tasks.html");
    let all = look(machine.env(&[]), Some(machine.path("Web"))).await;
    let views: Vec<_> = all.iter().map(AppState::view).collect();
    let text = serde_json::to_string(&views).unwrap();
    for needle in [
        machine.path("").display().to_string(),
        machine.path("").display().to_string().replace('\\', "/"),
        "Local".to_owned(),
        // A path of Windows in JSON has doubled backslashes; a drive has a colon and a slash.
        "\\\\".to_owned(),
        "C:".to_owned(),
        "E:".to_owned(),
        "docmap-desktop.exe".to_owned(),
    ] {
        assert!(!text.contains(&needle), "{needle} is in {text}");
    }
    // What the page does get.
    let tasks = views.iter().find(|v| v.id == "tasks").unwrap();
    assert!(tasks.installed && tasks.native && tasks.web);
    assert_eq!(tasks.default_mode, "native");
    assert!(!tasks.repo.private);
    assert!(
        views
            .iter()
            .find(|v| v.id == "docmap")
            .unwrap()
            .repo
            .private
    );
}

#[tokio::test]
async fn an_answer_is_held_for_its_time_and_forgotten_on_request() {
    let machine = Machine::new("hub-detect-cache");
    let detector = Detector::with_ttl(machine.env(&[]), None, Duration::from_secs(60));
    let first = detector.apps().await;
    machine.tasks_program();
    let second = detector.apps().await;
    assert!(std::sync::Arc::ptr_eq(&first, &second), "held");
    assert!(!app(&second, "tasks").installed());
    detector.invalidate();
    let third = detector.apps().await;
    assert!(app(&third, "tasks").installed(), "looked at again");
    assert!(detector.app("tasks").await.is_some());
    assert!(detector.app("nope").await.is_none());
}

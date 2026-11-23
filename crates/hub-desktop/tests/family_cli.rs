//! The commands of the family seen from hub-desktop: `install` makes a command for every app it finds, `cli status` and
//! `cli repair` look at and mend them, `apps` is the table of the machine, `start --all` and `stop --all` start and
//! stop the hubs of the kit. The machine is a folder (`Machine`), the apps are the fake program, the folder of the
//! starters is the family folder of the test, and no login entry or `PATH` of the machine is touched.

use std::path::PathBuf;
use std::process::{Command, Output};

use appkit_hub::cli_entry;
use hub_desktop::catalog;
use hub_desktop::config::Env;
use hub_desktop::family_cli::{self, Starter};

mod common;
use common::{fake_server, hello, starts_of, write_hub_json, Machine};

const PROGRAM: &str = env!("CARGO_BIN_EXE_hub-desktop");

/// The first place the catalog looks for a program that sits below `LOCALAPPDATA`, in the machine of the test.
fn candidate(id: &str) -> String {
    let Some(
        catalog::Native::GuiExe { candidates } | catalog::Native::LoopbackServer { candidates, .. },
    ) = catalog::find(id).and_then(|e| e.native)
    else {
        panic!("{id} has no candidates");
    };
    let rest = candidates[0]
        .strip_prefix("{LOCALAPPDATA}")
        .unwrap_or_else(|| panic!("{}", candidates[0]));
    let below: Vec<&str> = rest.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    format!("Local/{}", below.join("/"))
}

/// The apps that are there: the fake program for each id.
fn install_apps(machine: &Machine, ids: &[&str]) -> Vec<(String, PathBuf)> {
    ids.iter()
        .map(|id| {
            let below = match *id {
                "tasks" => format!("Local/tasks-desktop/bin/{}", catalog::TASKS.program_file()),
                "rules" => format!("Local/rules-desktop/bin/{}", catalog::RULES.program_file()),
                "testing" => format!(
                    "Local/testing-desktop/bin/{}",
                    catalog::TESTING.program_file()
                ),
                other => candidate(other),
            };
            ((*id).to_owned(), machine.program(&below))
        })
        .collect()
}

/// The environment of the machine, with the places of the test: the program of hub-desktop, the folder of the
/// starters (so `PATH` finds it), no login entries of the machine, a private machine.
fn env_of(machine: &Machine, extra: &[(&str, String)]) -> Env {
    let family = machine.path("family");
    let dir = family.join("bin");
    let path = std::env::join_paths(std::iter::once(dir).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let mut pairs: Vec<(&str, String)> = vec![
        ("APPKIT_FAMILY_DIR", family.display().to_string()),
        ("HUB_DESKTOP_NO_LOGIN_SYNC", "1".to_owned()),
        (
            "HUB_DESKTOP_INSTALL_DIR",
            machine.path("Local/hub-desktop/bin").display().to_string(),
        ),
        (
            "HUB_DESKTOP_CONFIG_DIR",
            machine.path("cfg-hub").display().to_string(),
        ),
        ("NVIM_MACHINE_ROLE", "default".to_owned()),
        ("PATH", path.to_string_lossy().into_owned()),
    ];
    pairs.extend(extra.iter().map(|(k, v)| (*k, v.clone())));
    machine.env(&pairs)
}

fn starter_dir(env: &Env) -> PathBuf {
    cli_entry::starter_dir(env).expect("a folder for the starters")
}

fn has_starter(env: &Env, command: &str) -> bool {
    let dir = starter_dir(env);
    cli_entry::starter_files(&dir, command)
        .iter()
        .all(|f| f.is_file())
}

fn no_starter(env: &Env, command: &str) -> bool {
    let dir = starter_dir(env);
    cli_entry::starter_files(&dir, command)
        .iter()
        .all(|f| !f.exists())
}

fn state_of(report: &family_cli::Report, command: &str) -> Option<Starter> {
    report
        .rows
        .iter()
        .find(|r| r.command == command)
        .map(|r| r.starter)
}

#[tokio::test]
async fn the_apps_that_are_there_get_a_command_and_ai_never_on_the_workstation() {
    let machine = Machine::new("family-cli-wanted");
    install_apps(&machine, &["tasks", "rules", "docmap", "ai"]);
    let env = env_of(&machine, &[]);
    let found = family_cli::wanted(&env).await;
    let commands: Vec<&str> = found.iter().map(|w| w.command).collect();
    assert_eq!(
        commands,
        [
            "tasks-desktop",
            "rules-desktop",
            "docmap-desktop",
            "ai-desktop"
        ],
        "{found:?}"
    );
    assert!(found.iter().all(|w| w.program.is_file()));

    // hub-desktop itself is the first one once its installed program is there.
    machine.program("Local/hub-desktop/bin/hub-desktop.exe");
    machine.program("Local/hub-desktop/bin/hub-desktop");
    let with_own = family_cli::wanted(&env).await;
    assert_eq!(with_own[0].command, "hub-desktop");

    // The workstation never has the ai family, not even as a command (MASCHINE-03); the rest is as before.
    let workstation = env_of(&machine, &[("NVIM_MACHINE_ROLE", "workstation".to_owned())]);
    let there: Vec<&str> = family_cli::wanted(&workstation)
        .await
        .iter()
        .map(|w| w.command)
        .collect();
    assert!(!there.contains(&"ai-desktop"), "{there:?}");
    assert!(there.contains(&"docmap-desktop"), "{there:?}");
}

#[tokio::test]
async fn repair_makes_the_commands_and_takes_away_those_of_apps_that_are_gone() {
    let machine = Machine::new("family-cli-repair");
    let apps = install_apps(&machine, &["tasks", "rules", "testing"]);
    let env = env_of(&machine, &[]);

    let found = family_cli::wanted(&env).await;
    let before = family_cli::status(&env, &found);
    assert!(!before.healthy());
    for command in ["tasks-desktop", "rules-desktop", "testing-desktop"] {
        assert_eq!(
            state_of(&before, command),
            Some(Starter::Missing),
            "{command}"
        );
    }

    let said = family_cli::repair(&env, &found, false).unwrap();
    assert!(said.contains("`rules-desktop`"), "{said}");
    for command in ["tasks-desktop", "rules-desktop", "testing-desktop"] {
        assert!(has_starter(&env, command), "{command}");
    }
    let after = family_cli::status(&env, &found);
    assert!(after.healthy(), "{}", after.text());
    // A second repair changes nothing it says.
    assert_eq!(
        family_cli::repair(&env, &found, false)
            .unwrap()
            .matches("is now on the PATH")
            .count(),
        0
    );

    // The terminal does not find a folder that is not on its PATH: not healthy, said in words.
    let lost = env_of(&machine, &[("PATH", String::new())]);
    let report = family_cli::status(&lost, &found);
    assert!(
        !report.healthy() && report.text().contains("not on the PATH"),
        "{}",
        report.text()
    );

    // The program of rules is gone: its starter is an orphan, the others are as they were.
    std::fs::remove_file(&apps[1].1).unwrap();
    let found = family_cli::wanted(&env).await;
    let report = family_cli::status(&env, &found);
    assert_eq!(state_of(&report, "rules-desktop"), Some(Starter::Orphan));
    assert_eq!(state_of(&report, "tasks-desktop"), Some(Starter::Ok));
    assert!(!report.healthy());
    family_cli::repair(&env, &found, false).unwrap();
    assert!(no_starter(&env, "rules-desktop"));
    assert!(has_starter(&env, "tasks-desktop") && has_starter(&env, "testing-desktop"));
    assert!(family_cli::status(&env, &found).healthy());

    // Another program of the same app: the starter names the old one, a repair writes it again.
    let moved = machine.program("Local/elsewhere/tasks-hub-new.exe");
    let mut moved_found = found.clone();
    moved_found[0].program = moved;
    assert_eq!(
        state_of(&family_cli::status(&env, &moved_found), "tasks-desktop"),
        Some(Starter::Stale)
    );
}

#[tokio::test]
async fn a_file_that_is_not_a_starter_is_left_as_it_is() {
    let machine = Machine::new("family-cli-foreign");
    install_apps(&machine, &["tasks", "rules"]);
    let env = env_of(&machine, &[]);
    let dir = starter_dir(&env);
    std::fs::create_dir_all(&dir).unwrap();
    let mine = cli_entry::starter_files(&dir, "tasks-desktop")[0].clone();
    std::fs::write(&mine, "the person's own file").unwrap();

    let found = family_cli::wanted(&env).await;
    assert_eq!(
        state_of(&family_cli::status(&env, &found), "tasks-desktop"),
        Some(Starter::Foreign)
    );
    let said = family_cli::repair(&env, &found, false).unwrap_err();
    assert!(
        said.contains("tasks-desktop") && said.contains("not a starter"),
        "{said}"
    );
    assert_eq!(
        std::fs::read_to_string(&mine).unwrap(),
        "the person's own file"
    );
    // The others were made all the same.
    assert!(has_starter(&env, "rules-desktop"));
}

#[tokio::test]
async fn the_table_names_every_app_what_it_is_and_what_runs() {
    let machine = Machine::new("family-cli-apps");
    install_apps(&machine, &["tasks", "docmap"]);
    let port = fake_server(200, hello(None)).await;
    write_hub_json(&machine.path("cfg-tasks"), port, "abc123");
    let env = env_of(&machine, &[]);
    family_cli::repair(&env, &family_cli::wanted(&env).await, false).unwrap();

    let rows = family_cli::apps(&env).await;
    let ids: Vec<&str> = rows.iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        catalog::CATALOG.iter().map(|e| e.id).collect::<Vec<_>>(),
        "every app of the catalog, in its order"
    );
    let row = |id: &str| rows.iter().find(|r| r.id == id).unwrap();
    let tasks = row("tasks");
    assert!(tasks.installed && tasks.native && !tasks.web);
    assert_eq!(tasks.run, Some("running"));
    assert_eq!(tasks.port, Some(port));
    assert_eq!(tasks.version.as_deref(), Some("0.0.0"));
    assert_eq!(tasks.command, Some(Starter::Ok));
    assert_eq!(row("docmap").command, Some(Starter::Ok));
    let rules = row("rules");
    assert!(!rules.installed && rules.run != Some("running"));
    assert_eq!(rules.command, None);

    let text = family_cli::apps_text(&rows);
    assert!(text.lines().next().unwrap().starts_with("app"), "{text}");
    assert_eq!(text.lines().count(), rows.len() + 1, "{text}");
}

#[tokio::test]
async fn start_all_starts_the_hubs_that_can_be_used_and_says_which_wait_for_a_person() {
    let machine = Machine::new("family-cli-start-all");
    let apps = install_apps(&machine, &["tasks", "rules", "docmap"]);
    let vault = machine.path("vault");
    std::fs::create_dir_all(&vault).unwrap();
    let env = env_of(&machine, &[("TASKS_VAULT", vault.display().to_string())]);

    let said = family_cli::start_all(&env).await.unwrap();
    assert!(said.contains("tasks: started in the background"), "{said}");
    assert!(said.contains("rules: not set up yet"), "{said}");
    assert!(
        !said.contains("docmap"),
        "an app without a hub is not asked: {said}"
    );

    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while starts_of(&apps[0].1).is_empty() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let starts = starts_of(&apps[0].1);
    assert_eq!(
        starts[0]["argv"],
        serde_json::json!(["start"]),
        "{starts:?}"
    );
    // No page: it is told to stay, as a hub that its own login entry started.
    assert_eq!(starts[0]["stay"], serde_json::json!(["TASKS_DESKTOP_STAY"]));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(starts_of(&apps[1].1).is_empty() && starts_of(&apps[2].1).is_empty());
}

#[tokio::test]
async fn stop_all_asks_the_hubs_that_run_and_nothing_else() {
    let machine = Machine::new("family-cli-stop-all");
    let apps = install_apps(&machine, &["tasks", "rules"]);
    let port = fake_server(200, hello(None)).await;
    write_hub_json(&machine.path("cfg-tasks"), port, "abc123");
    let env = env_of(&machine, &[]);

    let said = family_cli::stop_all(&env).await.unwrap();
    assert!(said.contains("tasks: asked to stop"), "{said}");
    assert!(said.contains("rules: does not run"), "{said}");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while starts_of(&apps[0].1).is_empty() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert_eq!(
        starts_of(&apps[0].1)[0]["argv"],
        serde_json::json!(["stop"])
    );
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        starts_of(&apps[1].1).is_empty(),
        "a hub that does not run is not asked"
    );
}

// ── the program itself ───────────────────────────────────────────────────────────────────────────────────────────

/// The program with the environment of the machine of the test (nothing of the person's own).
fn run(env: &Env, args: &[&str]) -> Output {
    let mut c = Command::new(PROGRAM);
    c.args(args).stdin(std::process::Stdio::null());
    for name in [
        "LOCALAPPDATA",
        "PROGRAMFILES",
        "USERPROFILE",
        "APPDATA",
        "TASKS_DESKTOP_CONFIG_DIR",
        "RULES_DESKTOP_CONFIG_DIR",
        "TESTING_DESKTOP_CONFIG_DIR",
        "PULT_CONFIG_DIR",
        "AI_DESKTOP_PORT",
        "APPKIT_FAMILY_DIR",
        "HUB_DESKTOP_NO_LOGIN_SYNC",
        "HUB_DESKTOP_INSTALL_DIR",
        "HUB_DESKTOP_CONFIG_DIR",
        "NVIM_MACHINE_ROLE",
        "PATH",
    ] {
        if let Some(value) = env.get(name) {
            c.env(name, value);
        }
    }
    c.env("HUB_DESKTOP_WEB_PORT", common::free_port().to_string());
    c.output().expect("the program starts")
}

fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Run the starter of `command` with `args`, as a shell of the person does.
fn run_starter(env: &Env, command: &str, args: &[&str]) -> Output {
    let files = cli_entry::starter_files(&starter_dir(env), command);
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd.exe");
        c.args(["/d", "/c"]).arg(&files[0]);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg(&files[0]);
        c
    };
    c.args(args);
    for name in ["LOCALAPPDATA", "PATH", "SystemRoot"] {
        if let Some(value) = env.get(name).cloned().or_else(|| std::env::var_os(name)) {
            c.env(name, value);
        }
    }
    c.env("MSYS2_ARG_CONV_EXCL", "*");
    c.output().expect("the starter starts")
}

#[test]
fn install_makes_the_commands_of_all_apps_it_finds_and_cli_commands_look_after_them() {
    let machine = Machine::new("family-cli-program");
    let apps = install_apps(&machine, &["tasks", "rules", "docmap", "ai"]);
    let env = env_of(&machine, &[]);

    let installed = run(&env, &["install"]);
    assert!(installed.status.success(), "{installed:?}");
    let said = text(&installed);
    assert!(
        said.contains("commands of the other apps found here"),
        "{said}"
    );
    for command in [
        "hub-desktop",
        "tasks-desktop",
        "rules-desktop",
        "docmap-desktop",
        "ai-desktop",
    ] {
        assert!(has_starter(&env, command), "{command}: {said}");
    }
    assert!(no_starter(&env, "testing-desktop"));

    // The starter reaches the program of its app with the arguments, and gives the output back.
    let out = run_starter(&env, "tasks-desktop", &["--say", "hello from tasks"]);
    let starter = cli_entry::starter_files(&starter_dir(&env), "tasks-desktop")[0].clone();
    assert!(
        out.status.success(),
        "{out:?}
starter {}:
{}
LOCALAPPDATA={:?}",
        starter.display(),
        std::fs::read_to_string(&starter).unwrap_or_default(),
        env.get("LOCALAPPDATA")
    );
    assert_eq!(text(&out).trim(), "hello from tasks");
    assert_eq!(
        starts_of(&apps[0].1).last().unwrap()["argv"],
        serde_json::json!(["--say", "hello from tasks"])
    );

    // `cli status`: every command as it should be, the folder on the PATH: exit code 0, in words and in JSON.
    let status = run(&env, &["cli", "status"]);
    assert_eq!(status.status.code(), Some(0), "{status:?}");
    let said = text(&status);
    assert!(said.contains("on the PATH of this terminal: yes"), "{said}");
    for command in ["tasks-desktop", "rules-desktop", "docmap-desktop"] {
        assert!(said.contains(command), "{command}: {said}");
    }
    let json = run(&env, &["cli", "status", "--json"]);
    let doc: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(doc["ok"], true, "{doc}");
    assert_eq!(doc["on_path"], true);
    assert!(doc["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["starter"] == "ok"));

    // An app is gone: exit code 1 and the orphan named; `cli repair` takes it away, and a new app gets its command.
    std::fs::remove_file(&apps[1].1).unwrap();
    machine.program(&format!(
        "Local/testing-desktop/bin/{}",
        catalog::TESTING.program_file()
    ));
    let bad = run(&env, &["cli", "status"]);
    assert_eq!(bad.status.code(), Some(1), "{bad:?}");
    let said = text(&bad);
    assert!(
        said.contains("rules-desktop") && said.contains("orphan"),
        "{said}"
    );
    assert!(
        said.contains("testing-desktop") && said.contains("missing"),
        "{said}"
    );
    let repaired = run(&env, &["cli", "repair"]);
    assert!(repaired.status.success(), "{repaired:?}");
    assert!(no_starter(&env, "rules-desktop") && has_starter(&env, "testing-desktop"));
    assert_eq!(run(&env, &["cli", "status"]).status.code(), Some(0));

    // The terminal that cannot find the folder is told so, with exit code 1.
    let lost = env_of(&machine, &[("PATH", String::new())]);
    let lost_out = run(&lost, &["cli", "status"]);
    assert_eq!(lost_out.status.code(), Some(1), "{lost_out:?}");
    assert!(text(&lost_out).contains("on the PATH of this terminal: no"));

    // `apps --json`: the table as a document.
    let table = run(&env, &["apps", "--json"]);
    assert!(table.status.success(), "{table:?}");
    let doc: serde_json::Value = serde_json::from_slice(&table.stdout).unwrap();
    let tasks = doc["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "tasks")
        .unwrap();
    assert_eq!(tasks["installed"], true);
    assert_eq!(tasks["command"], "ok");
    let plain = text(&run(&env, &["apps"]));
    assert!(plain.starts_with("app"), "{plain}");

    // Wrong words of the new commands are wrong arguments: exit code 2 with the usage.
    for args in [
        &["cli"][..],
        &["cli", "frobnicate"],
        &["cli", "status", "--bogus"],
        &["apps", "--bogus"],
        &["start", "--bogus"],
        &["stop", "--bogus"],
    ] {
        let out = run(&env, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("usage:"),
            "{args:?}"
        );
    }
    // And their help names them.
    for (word, expect) in [
        ("cli", "cli repair"),
        ("apps", "apps [--json]"),
        ("start", "start --all"),
        ("stop", "stop --all"),
    ] {
        let help = run(&env, &["help", word]);
        assert!(help.status.success(), "{word}: {help:?}");
        assert!(text(&help).contains(expect), "{word}: {}", text(&help));
    }
}

#[test]
fn install_on_the_workstation_makes_no_command_for_ai_desktop() {
    let machine = Machine::new("family-cli-workstation");
    install_apps(&machine, &["tasks", "docmap", "ai"]);
    let env = env_of(&machine, &[("NVIM_MACHINE_ROLE", "workstation".to_owned())]);
    let installed = run(&env, &["install"]);
    assert!(installed.status.success(), "{installed:?}");
    assert!(has_starter(&env, "tasks-desktop") && has_starter(&env, "docmap-desktop"));
    assert!(no_starter(&env, "ai-desktop"), "MASCHINE-03");
    let json = run(&env, &["cli", "status", "--json"]);
    let doc: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert!(
        doc["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["command"] != "ai-desktop"),
        "{doc}"
    );
}

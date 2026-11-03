//! Updates over HTTP, with a session and a runner that stands in for git, gh and PowerShell: what is run for a
//! web variant and for a native build, what is refused (a dirty clone, another branch, the workstation, a file
//! that does not match its sums), that "update all" goes on after a failure, and that the runner of the program
//! collects the output, the exit code and ends a step whose time is up.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::{Limits, RunningServer};
use appkit_testkit::wire::{request, send, Reply};
use hub_desktop::detect::Detector;
use hub_desktop::launch::{Browser, LaunchConfig, Launcher};
use hub_desktop::machine::Role;
use hub_desktop::monitor::Monitor;
use hub_desktop::prefs::Prefs;
use hub_desktop::server::{self, ServerConfig};
use hub_desktop::sums;
use hub_desktop::ui::UiFiles;
use hub_desktop::update::{Ran, Runner, Step, SystemRunner, Tools, UpdateConfig, Updater};
use serde_json::Value;

mod common;
use common::*;

/// What the fake programs answer: by the program (`git`, `gh`, `powershell`) and the first argument.
type Answer = dyn Fn(&str, &Step) -> Ran + Send + Sync;

struct FakeRunner {
    tools: Tools,
    answer: Box<Answer>,
    /// Every step, as `<program> <args...>`.
    seen: Mutex<Vec<String>>,
}

impl FakeRunner {
    fn name(&self, program: &Path) -> &'static str {
        if Some(program) == self.tools.git.as_deref() {
            "git"
        } else if Some(program) == self.tools.gh.as_deref() {
            "gh"
        } else if Some(program) == self.tools.powershell.as_deref() {
            "powershell"
        } else {
            "other"
        }
    }
}

impl Runner for FakeRunner {
    fn run(&self, step: &Step, _timeout: Duration) -> Result<Ran, String> {
        let name = self.name(&step.program);
        let args: Vec<String> = step
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        self.seen
            .lock()
            .unwrap()
            .push(format!("{name} {}", args.join(" ")));
        Ok((self.answer)(name, step))
    }
}

fn ok(lines: &[&str]) -> Ran {
    Ran {
        code: Some(0),
        lines: lines.iter().map(|l| (*l).to_owned()).collect(),
    }
}

fn fail(lines: &[&str]) -> Ran {
    Ran {
        code: Some(1),
        lines: lines.iter().map(|l| (*l).to_owned()).collect(),
    }
}

fn arg(step: &Step, i: usize) -> String {
    step.args
        .get(i)
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The answers of a clean clone on `main` whose pull brings a new commit (`moved`) or none.
fn clean_git(name: &str, step: &Step, moved: bool, heads: &Mutex<u32>) -> Option<Ran> {
    if name != "git" {
        return None;
    }
    Some(match arg(step, 0).as_str() {
        "symbolic-ref" => ok(&["main"]),
        "status" => ok(&[]),
        "rev-parse" => {
            let mut n = heads.lock().unwrap();
            *n += 1;
            if moved && *n > 1 {
                ok(&["bbbbbbb"])
            } else {
                ok(&["aaaaaaa"])
            }
        }
        "pull" => ok(&["Fast-forward"]),
        _ => fail(&["unexpected"]),
    })
}

struct Hub {
    server: RunningServer,
    token: String,
    machine: Machine,
    runner: Arc<FakeRunner>,
}

struct Setup {
    role: Role,
    /// The program of tasks is installed (the fake program).
    tasks_native: bool,
    /// The clone of tasks-web with its page and sums.
    tasks_web: bool,
    extra_env: Vec<(&'static str, String)>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            role: Role::Private,
            tasks_native: false,
            tasks_web: true,
            extra_env: Vec::new(),
        }
    }
}

const PAGE: &[u8] = b"<!doctype html><title>tasks</title>";

impl Hub {
    async fn new(name: &str, setup: Setup, answer: Box<Answer>) -> Self {
        let machine = Machine::new(name);
        let vault = machine.path("vault");
        std::fs::create_dir_all(&vault).unwrap();
        if setup.tasks_native {
            machine.tasks_program();
        }
        if setup.tasks_web {
            let dir = machine.path("Repos/tasks-web");
            std::fs::create_dir_all(dir.join(".git")).unwrap();
            std::fs::write(dir.join("tasks.html"), PAGE).unwrap();
            std::fs::write(
                dir.join("SHA256SUMS"),
                format!("{}  tasks.html\n", sums::hex_of(PAGE)),
            )
            .unwrap();
        }
        let tools = Tools {
            git: Some(machine.touch("tools/git.exe")),
            gh: Some(machine.touch("tools/gh.exe")),
            powershell: Some(machine.touch("tools/powershell.exe")),
        };
        let mut env = vec![("TASKS_VAULT", vault.display().to_string())];
        env.extend(setup.extra_env.iter().map(|(k, v)| (*k, v.clone())));
        let detector = Arc::new(Detector::with_ttl(
            machine.env(&env),
            Some(machine.path("Repos")),
            Duration::ZERO,
        ));
        let launcher = Arc::new(Launcher::new(
            Arc::clone(&detector),
            LaunchConfig::new(None, Browser::Log(machine.path("browser.log"))),
        ));
        let runner = Arc::new(FakeRunner {
            tools: tools.clone(),
            answer,
            seen: Mutex::new(Vec::new()),
        });
        let updater = Arc::new(Updater::new(
            Arc::clone(&detector),
            Arc::clone(&launcher),
            UpdateConfig::new(setup.role, tools, Some(machine.path("cfg-hub/updates"))),
            Arc::clone(&runner) as Arc<dyn Runner>,
        ));
        let control = Secret::generate(TOKEN_BYTES).unwrap();
        let server = server::start(ServerConfig {
            addr: "127.0.0.1:0".parse().unwrap(),
            limits: Limits::default(),
            clock: Arc::new(ManualClock::new()),
            pairing_ttl: Duration::from_secs(60),
            control_secret: control.clone(),
            instance: Secret::generate(CODE_BYTES).unwrap(),
            ui: Arc::new(UiFiles::none()),
            detector: Arc::clone(&detector),
            launcher,
            prefs: Arc::new(Prefs::new(Some(machine.path("cfg-hub")), detector)),
            monitor: Arc::new(Monitor::new(None)),
            updater,
        })
        .await
        .unwrap();
        let token = session(&server, &control.to_hex()).await;
        Self {
            server,
            token,
            machine,
            runner,
        }
    }

    fn headers(&self) -> (String, String, String) {
        let host = format!("127.0.0.1:{}", self.server.addr().port());
        let origin = format!("http://{host}");
        let auth = format!("Bearer {}", self.token);
        (host, origin, auth)
    }

    async fn post(&self, path: &str, body: &str) -> Reply {
        let (host, origin, auth) = self.headers();
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

    async fn updates(&self) -> Value {
        let (host, _, auth) = self.headers();
        let reply = send(
            self.server.addr(),
            request(
                "GET",
                "/api/hub/updates",
                &[("Host", host.as_str()), ("Authorization", auth.as_str())],
                b"",
            ),
        )
        .await;
        assert_eq!(reply.status, 200, "{reply:?}");
        reply.json()
    }

    async fn of(&self, id: &str) -> Value {
        let doc = self.updates().await;
        doc["apps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// The job of `id` once it is no longer queued or running.
    async fn finished(&self, id: &str) -> Value {
        let end = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let app = self.of(id).await;
            let state = app["job"]["state"].as_str().unwrap_or("").to_owned();
            if !state.is_empty() && state != "queued" && state != "running" {
                return app["job"].clone();
            }
            assert!(std::time::Instant::now() < end, "{id}: {app}");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    fn seen(&self) -> Vec<String> {
        self.runner.seen.lock().unwrap().clone()
    }
}

fn part<'a>(job: &'a Value, which: &str) -> &'a Value {
    job["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["part"] == which)
        .unwrap_or(&Value::Null)
}

#[tokio::test]
async fn a_clean_clone_on_main_is_pulled_fast_forward_and_its_page_checked() {
    let heads = Mutex::new(0);
    let hub = Hub::new(
        "hub-update-web",
        Setup::default(),
        Box::new(move |name, step| {
            clean_git(name, step, true, &heads).unwrap_or_else(|| fail(&[]))
        }),
    )
    .await;
    assert_eq!(hub.of("tasks").await["web"], "ready");
    // The body is not read: what runs is the catalog's.
    let reply = hub
        .post(
            "/api/hub/apps/tasks/update",
            r#"{"program":"C:/Windows/System32/cmd.exe"}"#,
        )
        .await;
    assert_eq!(reply.status, 200, "{reply:?}");
    assert_eq!(reply.json()["result"], "queued");
    let job = hub.finished("tasks").await;
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(part(&job, "web")["result"], "updated");
    assert_eq!(job["reload"], true);
    assert_eq!(
        hub.seen(),
        [
            "git symbolic-ref --quiet --short HEAD",
            "git status --porcelain --untracked-files=no",
            "git rev-parse HEAD",
            "git pull --ff-only",
            "git rev-parse HEAD",
        ]
    );
    let lines = job["lines"].as_array().unwrap();
    assert!(
        lines.iter().any(|l| l == "tasks.html: matches SHA256SUMS."),
        "{job}"
    );
    hub.server.shutdown().await;
}

#[tokio::test]
async fn a_clone_without_a_new_commit_is_up_to_date_and_the_page_needs_no_reload() {
    let heads = Mutex::new(0);
    let hub = Hub::new(
        "hub-update-web-same",
        Setup::default(),
        Box::new(move |name, step| {
            clean_git(name, step, false, &heads).unwrap_or_else(|| fail(&[]))
        }),
    )
    .await;
    hub.post("/api/hub/apps/tasks/update", "{}").await;
    let job = hub.finished("tasks").await;
    assert_eq!(part(&job, "web")["result"], "up_to_date", "{job}");
    assert_eq!(job["reload"], false);
    hub.server.shutdown().await;
}

#[tokio::test]
async fn a_dirty_clone_or_one_on_another_branch_is_not_touched() {
    for (name, branch, status, code) in [
        ("hub-update-dirty", "main", " M tasks.html", "dirty"),
        ("hub-update-branch", "feature", "", "not_on_main"),
    ] {
        let hub = Hub::new(
            name,
            Setup::default(),
            Box::new(move |_, step| match arg(step, 0).as_str() {
                "symbolic-ref" => ok(&[branch]),
                "status" => ok(&[status]),
                _ => ok(&[]),
            }),
        )
        .await;
        hub.post("/api/hub/apps/tasks/update", "{}").await;
        let job = hub.finished("tasks").await;
        assert_eq!(job["state"], "failed", "{name}: {job}");
        assert_eq!(part(&job, "web")["result"], code, "{name}");
        assert!(
            hub.seen().iter().all(|s| !s.starts_with("git pull")),
            "{name}: {:?}",
            hub.seen()
        );
        hub.server.shutdown().await;
    }
}

#[tokio::test]
async fn a_pull_that_brings_a_page_that_does_not_match_its_sums_fails() {
    let heads = Mutex::new(0);
    let page = Arc::new(Mutex::new(PathBuf::new()));
    let at = Arc::clone(&page);
    let hub = Hub::new(
        "hub-update-mismatch",
        Setup::default(),
        Box::new(move |name, step| {
            if arg(step, 0) == "pull" {
                std::fs::write(step.cwd.join("tasks.html"), b"changed").unwrap();
                *at.lock().unwrap() = step.cwd.clone();
            }
            clean_git(name, step, true, &heads).unwrap_or_else(|| fail(&[]))
        }),
    )
    .await;
    hub.post("/api/hub/apps/tasks/update", "{}").await;
    let job = hub.finished("tasks").await;
    assert_eq!(part(&job, "web")["result"], "sums_mismatch", "{job}");
    assert_eq!(job["state"], "failed");
    assert!(page.lock().unwrap().ends_with("tasks-web"));
    hub.server.shutdown().await;
}

#[tokio::test]
async fn on_the_workstation_the_native_build_is_left_and_the_clone_is_still_pulled() {
    for (role, code) in [
        (Role::Workstation, "workstation"),
        (Role::Unclear, "role_unclear"),
    ] {
        let heads = Mutex::new(0);
        let hub = Hub::new(
            &format!("hub-update-{code}"),
            Setup {
                role,
                tasks_native: true,
                ..Setup::default()
            },
            Box::new(move |name, step| {
                clean_git(name, step, true, &heads).unwrap_or_else(|| fail(&[]))
            }),
        )
        .await;
        assert_eq!(hub.of("tasks").await["native"], code);
        hub.post("/api/hub/apps/tasks/update", "{}").await;
        let job = hub.finished("tasks").await;
        assert_eq!(job["state"], "done", "{job}");
        assert_eq!(part(&job, "native")["outcome"], "skipped");
        assert_eq!(part(&job, "native")["result"], code);
        assert_eq!(part(&job, "web")["result"], "updated");
        assert!(
            hub.seen().iter().all(|s| s.starts_with("git ")),
            "{:?}",
            hub.seen()
        );
        hub.server.shutdown().await;
    }
}

#[tokio::test]
async fn an_app_with_nothing_to_update_here_is_refused_in_words_and_an_unknown_one_is_not_found() {
    let hub = Hub::new(
        "hub-update-refused",
        Setup {
            role: Role::Workstation,
            tasks_native: true,
            tasks_web: false,
            ..Setup::default()
        },
        Box::new(|_, _| ok(&[])),
    )
    .await;
    for (id, status, code) in [
        ("tasks", 409, "not_updatable"),
        ("rules", 409, "not_updatable"),
        ("nope", 404, "not_found"),
        ("hub", 404, "not_found"),
    ] {
        let reply = hub.post(&format!("/api/hub/apps/{id}/update"), "{}").await;
        assert_eq!(reply.status, status, "{id}: {reply:?}");
        assert_eq!(reply.json()["error"]["code"], code, "{id}");
    }
    let all = hub.post("/api/hub/update-all", "{}").await;
    assert_eq!(all.json()["queued"], 0, "{all:?}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(hub.seen().is_empty(), "{:?}", hub.seen());
    hub.server.shutdown().await;
}

/// The answers of a release whose files match their sums (or, with `tamper`, a file that does not).
#[cfg(windows)]
fn release(name: &str, step: &Step, tamper: bool) -> Option<Ran> {
    if name == "gh" {
        let dir = PathBuf::from(arg(step, 12));
        let script = b"param()".to_vec();
        let build = b"a build".to_vec();
        let sums = format!(
            "{}  install.ps1\n{}  tasks-hub-windows-x86_64.exe\n",
            sums::hex_of(&script),
            sums::hex_of(&build)
        );
        std::fs::write(dir.join("install.ps1"), &script).unwrap();
        let body: &[u8] = if tamper { b"another build" } else { &build };
        std::fs::write(dir.join("tasks-hub-windows-x86_64.exe"), body).unwrap();
        std::fs::write(dir.join("SHA256SUMS"), sums).unwrap();
        return Some(ok(&["downloaded"]));
    }
    (name == "powershell").then(|| ok(&["installed tasks-hub"]))
}

#[cfg(windows)]
#[tokio::test]
async fn the_native_build_is_downloaded_checked_and_installed_by_the_script_of_its_release() {
    let hub = Hub::new(
        "hub-update-native",
        Setup {
            tasks_native: true,
            tasks_web: false,
            ..Setup::default()
        },
        Box::new(|name, step| release(name, step, false).unwrap_or_else(|| fail(&[]))),
    )
    .await;
    assert_eq!(hub.of("tasks").await["native"], "ready");
    hub.post("/api/hub/apps/tasks/update", "{}").await;
    let job = hub.finished("tasks").await;
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(part(&job, "native")["result"], "installed");
    let seen = hub.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(
        seen[0].starts_with(
            "gh release download latest -R WKDSteve/tasks-desktop -p install.ps1 -p SHA256SUMS -p tasks-hub-windows-x86_64.exe -D "
        ),
        "{seen:?}"
    );
    assert!(seen[0].ends_with(" --clobber"), "{seen:?}");
    assert!(
        seen[1].starts_with("powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "),
        "{seen:?}"
    );
    assert!(seen[1].contains(r"\install.ps1 -File "), "{seen:?}");
    assert!(
        seen[1].contains(r"\tasks-hub-windows-x86_64.exe -Sums "),
        "{seen:?}"
    );
    // The page is shown the steps by the names of their files, not by the folders of the machine.
    let shown = job["lines"].to_string();
    let folder = hub.machine.path("").display().to_string();
    assert!(
        !shown.contains(&folder) && !shown.contains(&folder.replace('\\', "\\\\")),
        "{shown}"
    );
    assert!(
        shown.contains("-File install.ps1 -File tasks-hub-windows-x86_64.exe -Sums SHA256SUMS"),
        "{shown}"
    );
    // The download folder is gone with the run.
    let work = hub.machine.path("cfg-hub/updates");
    assert_eq!(std::fs::read_dir(&work).map(|d| d.count()).unwrap_or(0), 0);
    hub.server.shutdown().await;
}

#[cfg(windows)]
#[tokio::test]
async fn a_build_that_does_not_match_its_sums_is_never_installed() {
    let hub = Hub::new(
        "hub-update-tampered",
        Setup {
            tasks_native: true,
            tasks_web: false,
            ..Setup::default()
        },
        Box::new(|name, step| release(name, step, true).unwrap_or_else(|| fail(&[]))),
    )
    .await;
    hub.post("/api/hub/apps/tasks/update", "{}").await;
    let job = hub.finished("tasks").await;
    assert_eq!(part(&job, "native")["result"], "checksum_failed", "{job}");
    assert!(
        hub.seen().iter().all(|s| !s.starts_with("powershell")),
        "{:?}",
        hub.seen()
    );
    hub.server.shutdown().await;
}

#[cfg(windows)]
#[tokio::test]
async fn a_failed_download_or_install_is_a_failure_of_the_native_part() {
    for (name, failing, code) in [
        ("hub-update-no-download", "gh", "download_failed"),
        ("hub-update-no-install", "powershell", "install_failed"),
    ] {
        let hub = Hub::new(
            name,
            Setup {
                tasks_native: true,
                tasks_web: false,
                ..Setup::default()
            },
            Box::new(move |program, step| {
                if program == failing {
                    return fail(&["no"]);
                }
                release(program, step, false).unwrap_or_else(|| fail(&[]))
            }),
        )
        .await;
        hub.post("/api/hub/apps/tasks/update", "{}").await;
        let job = hub.finished("tasks").await;
        assert_eq!(part(&job, "native")["result"], code, "{name}: {job}");
        assert_eq!(job["state"], "failed");
        hub.server.shutdown().await;
    }
}

#[cfg(windows)]
#[tokio::test]
async fn a_program_that_serves_a_page_and_runs_must_be_closed_first() {
    let health = fake_server(
        200,
        r#"{"status":"ok","model_router_ready":true,"backends":[]}"#.to_owned(),
    )
    .await;
    let hub = Hub::new(
        "hub-update-ai",
        Setup {
            tasks_web: false,
            extra_env: vec![("AI_DESKTOP_PORT", health.to_string())],
            ..Setup::default()
        },
        Box::new(|_, _| ok(&[])),
    )
    .await;
    hub.machine
        .program("Local/Programs/ai-desktop/app/ai-desktop.exe");
    hub.post("/api/hub/apps/ai/update", "{}").await;
    let job = hub.finished("ai").await;
    assert_eq!(part(&job, "native")["result"], "close_first", "{job}");
    assert!(hub.seen().is_empty(), "{:?}", hub.seen());
    hub.server.shutdown().await;
}

#[cfg(windows)]
#[tokio::test]
async fn update_all_runs_one_app_after_the_other_and_goes_on_after_a_failure() {
    let hub = Hub::new(
        "hub-update-all",
        Setup::default(),
        Box::new(|name, step| {
            // tasks-web is dirty; rules downloads and installs.
            if name == "git" {
                return match arg(step, 0).as_str() {
                    "symbolic-ref" => {
                        // Long enough for the second press below to find the jobs still on their way.
                        std::thread::sleep(Duration::from_millis(500));
                        ok(&["main"])
                    }
                    "status" => ok(&[" M tasks.html"]),
                    _ => ok(&[]),
                };
            }
            if name == "gh" {
                let dir = PathBuf::from(arg(step, 12));
                let script = b"param()".to_vec();
                let build = b"rules".to_vec();
                std::fs::write(dir.join("install.ps1"), &script).unwrap();
                std::fs::write(dir.join("rules-hub-windows-x86_64.exe"), &build).unwrap();
                std::fs::write(
                    dir.join("SHA256SUMS"),
                    format!(
                        "{}  install.ps1\n{}  rules-hub-windows-x86_64.exe\n",
                        sums::hex_of(&script),
                        sums::hex_of(&build)
                    ),
                )
                .unwrap();
            }
            ok(&[])
        }),
    )
    .await;
    hub.machine.program(&format!(
        "Local/rules-desktop/bin/{}",
        hub_desktop::catalog::RULES.program_file()
    ));
    let all = hub.post("/api/hub/update-all", "{}").await;
    assert_eq!(all.status, 200, "{all:?}");
    assert_eq!(all.json()["queued"], 2, "{all:?}");
    // A second press while they run queues nothing twice.
    let again = hub.post("/api/hub/update-all", "{}").await;
    assert_eq!(again.json()["queued"], 0, "{again:?}");
    let tasks = hub.finished("tasks").await;
    let rules = hub.finished("rules").await;
    assert_eq!(tasks["state"], "failed", "{tasks}");
    assert_eq!(part(&tasks, "web")["result"], "dirty");
    assert_eq!(rules["state"], "done", "{rules}");
    assert_eq!(part(&rules, "native")["result"], "installed");
    // One after the other: the steps of tasks all come before the ones of rules.
    let seen = hub.seen();
    let last_git = seen.iter().rposition(|s| s.starts_with("git ")).unwrap();
    let first_gh = seen.iter().position(|s| s.starts_with("gh ")).unwrap();
    assert!(last_git < first_gh, "{seen:?}");
    hub.server.shutdown().await;
}

// The runner of the program, with the fake program: what it prints, how it ends, and a step that takes too long.

#[test]
fn the_system_runner_collects_the_output_and_the_exit_code() {
    let machine = Machine::new("hub-runner-output");
    let program = machine.program(&format!("tools/fake{}", std::env::consts::EXE_SUFFIX));
    let step = Step {
        program: program.clone(),
        args: [
            "--say",
            "hello",
            "--err",
            "\u{1b}[31mwarn\u{1b}[0m",
            "--exit",
            "3",
        ]
        .iter()
        .map(Into::into)
        .collect(),
        cwd: machine.path(""),
    };
    let ran = SystemRunner.run(&step, Duration::from_secs(20)).unwrap();
    assert_eq!(ran.code, Some(3), "{ran:?}");
    let mut lines = ran.lines.clone();
    lines.sort();
    assert_eq!(lines, ["hello", "warn"], "{ran:?}");
}

#[test]
fn the_system_runner_ends_a_step_whose_time_is_up() {
    let machine = Machine::new("hub-runner-timeout");
    let program = machine.program(&format!("tools/fake{}", std::env::consts::EXE_SUFFIX));
    let step = Step {
        program,
        args: ["--sleep", "20000"].iter().map(Into::into).collect(),
        cwd: machine.path(""),
    };
    let began = std::time::Instant::now();
    let ran = SystemRunner.run(&step, Duration::from_millis(300)).unwrap();
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(ran.code, None, "{ran:?}");
    assert!(
        ran.lines.iter().any(|l| l.contains("took too long")),
        "{ran:?}"
    );
}

#[test]
fn the_system_runner_starts_only_an_absolute_program_in_an_absolute_folder() {
    let machine = Machine::new("hub-runner-paths");
    let program = machine.program(&format!("tools/fake{}", std::env::consts::EXE_SUFFIX));
    for (program, cwd) in [
        (PathBuf::from("git"), machine.path("")),
        (machine.path("tools/none.exe"), machine.path("")),
        (program.clone(), PathBuf::from("relative")),
        (program.clone(), machine.path("no-such-folder")),
    ] {
        let step = Step {
            program: program.clone(),
            args: Vec::new(),
            cwd: cwd.clone(),
        };
        assert!(
            SystemRunner.run(&step, Duration::from_secs(5)).is_err(),
            "{program:?} in {cwd:?}"
        );
    }
    assert!(starts_of(&program).is_empty());
}

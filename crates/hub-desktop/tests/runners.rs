//! The runners of the machine against a fake of the programs (`Ask`): `pwsh` with `runners.ps1` and `testing ci
//! remote` answer from a table, nothing is started, no real runner is looked at or started; every call is recorded
//! and checked by its argv.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_scratch::Scratch;
use hub_desktop::ciwatch::{Answer, Ask, Call, Programs};
use hub_desktop::config::Env;
use hub_desktop::machine::Role;
use hub_desktop::runners::{self, Board, RunnerWatch, Runners, Started, NOT_HERE};

const PWSH: &str = "C:/fake/pwsh.exe";
const SCRIPT: &str = "C:/fake/nvim/scripts/runners.ps1";
const HEAD: &str = "cccccccccccccccccccccccccccccccccccccccc";

/// What the fake programs say: the lines of `status all` (before and after a start), the overall word of the CI
/// per repository, the exit code of `ensure`.
struct Fake {
    status: Mutex<Vec<String>>,
    after_start: String,
    overall: BTreeMap<&'static str, &'static str>,
    ensure_code: i32,
    calls: Mutex<Vec<Vec<String>>>,
}

impl Fake {
    fn new(status: &str, overall: &[(&'static str, &'static str)]) -> Self {
        Self {
            status: Mutex::new(vec![status.to_owned()]),
            after_start: status.to_owned(),
            overall: overall.iter().copied().collect(),
            ensure_code: 0,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }

    /// The calls by their last words (`status all`, `ensure x`, `remote <dir>`).
    fn said(&self) -> Vec<String> {
        self.calls()
            .iter()
            .map(|argv| {
                if argv[0] == PWSH {
                    argv[5..].join(" ")
                } else {
                    let dir = PathBuf::from(argv.last().unwrap());
                    format!("remote {}", dir.file_name().unwrap().to_string_lossy())
                }
            })
            .collect()
    }
}

impl Ask for Fake {
    fn ask(&self, call: &Call, _timeout: Duration) -> Result<Answer, String> {
        let mut argv = vec![call.program.display().to_string()];
        argv.extend(call.args.iter().map(|a| a.to_string_lossy().into_owned()));
        self.calls.lock().unwrap().push(argv.clone());
        let answer = |code: i32, text: &str| Answer {
            code: Some(code),
            stdout: text.as_bytes().to_vec(),
        };
        if call.program.as_os_str() == PWSH {
            return Ok(match argv[5].as_str() {
                "status" => {
                    let text = self.status.lock().unwrap()[0].clone();
                    answer(1, &text)
                }
                "ensure" => {
                    if self.ensure_code == 0 {
                        *self.status.lock().unwrap() = vec![self.after_start.clone()];
                    }
                    answer(self.ensure_code, "")
                }
                other => panic!("runners.ps1 {other}"),
            });
        }
        let dir = PathBuf::from(argv.last().unwrap());
        let repo = dir.file_name().unwrap().to_string_lossy().into_owned();
        let overall = self
            .overall
            .get(repo.as_str())
            .copied()
            .unwrap_or("success");
        let doc = serde_json::json!({"schema": "testing-ci-remote/1",
            "repos": [{"sha": HEAD, "overall": overall, "run": {"status": overall}}]});
        Ok(answer(i32::from(overall != "success"), &doc.to_string()))
    }
}

fn runners() -> Runners {
    Runners {
        pwsh: PathBuf::from(PWSH),
        script: PathBuf::from(SCRIPT),
    }
}

struct Place {
    _scratch: Scratch,
    programs: Programs,
}

/// `$REPOS_DIR` with checkouts of the named repositories (a `.git` folder is all that is looked for).
fn place(checkouts: &[&str]) -> Place {
    let scratch = Scratch::new("hub-runners");
    let repos = scratch.path().join("repos");
    for repo in checkouts {
        std::fs::create_dir_all(repos.join(repo).join(".git")).unwrap();
    }
    let programs = Programs {
        nvim: PathBuf::from("C:/fake/nvim.exe"),
        git: PathBuf::from("C:/fake/git.exe"),
        tasks_lua: repos.join("tasks.nvim/scripts/tasks.lua"),
        testing_lua: repos.join("testing.nvim/scripts/testing.lua"),
        vault: scratch.path().join("vault"),
        repos,
    };
    Place {
        _scratch: scratch,
        programs,
    }
}

const STATUS: &str = "hub-desktop    local=running  github=online busy\n\
                      tasks.nvim     local=stopped  github=offline\n\
                      rules.nvim     local=stopped  github=offline\n\
                      my.nvim        local=stopped  github=offline\n";

#[test]
fn the_runners_are_looked_at_on_the_private_machine_only() {
    let scratch = Scratch::new("hub-runners-find");
    let bin = scratch.path().join("bin");
    let config = scratch.path().join("nvim");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(config.join("scripts")).unwrap();
    let pwsh = bin.join(format!("pwsh{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&pwsh, "").unwrap();
    let env = || {
        Env::from_pairs([
            ("PATH", bin.as_os_str().to_owned()),
            ("NVIM_CONFIG_DIR", config.as_os_str().to_owned()),
        ])
    };
    assert_eq!(
        Runners::find(&env(), Role::Private).unwrap_err(),
        "no scripts/runners.ps1 in NVIM_CONFIG_DIR"
    );
    std::fs::write(config.join("scripts/runners.ps1"), "").unwrap();
    assert_eq!(
        Runners::find(&env(), Role::Private).unwrap(),
        Runners {
            pwsh,
            script: config.join("scripts").join("runners.ps1"),
        }
    );
    for role in [Role::Workstation, Role::Unclear] {
        assert_eq!(
            Runners::find(&env(), role).unwrap_err(),
            NOT_HERE,
            "{role:?}"
        );
    }
    let no_pwsh = Env::from_pairs([("NVIM_CONFIG_DIR", config.as_os_str().to_owned())]);
    assert_eq!(
        Runners::find(&no_pwsh, Role::Private).unwrap_err(),
        "pwsh is not on the PATH"
    );
}

/// The exact argv of `status all` and `ensure`: pwsh without a profile, the script as a file, the words.
#[test]
fn runners_ps1_is_run_by_argv() {
    let words = |call: Call| -> Vec<OsString> { call.args };
    assert_eq!(
        words(runners().status_call()),
        [
            "-NoProfile",
            "-NonInteractive",
            "-File",
            SCRIPT,
            "status",
            "all"
        ]
        .map(OsString::from)
    );
    assert_eq!(
        words(runners().ensure_call("tasks.nvim")),
        [
            "-NoProfile",
            "-NonInteractive",
            "-File",
            SCRIPT,
            "ensure",
            "tasks.nvim"
        ]
        .map(OsString::from)
    );
    assert_eq!(runners().status_call().program, PathBuf::from(PWSH));
    assert_eq!(runners().status_call().cwd, PathBuf::from("C:/fake/nvim"));
}

/// A stopped runner whose push waits (`waiting_runner`) is seen; one whose run is queued or green is not; a runner
/// that runs, or that has no checkout, is not asked about. Without the setting nothing is started.
#[test]
fn a_push_that_waits_for_a_stopped_runner_is_seen_and_not_started_without_the_setting() {
    let place = place(&["hub-desktop", "tasks.nvim", "rules.nvim"]);
    let fake = Fake::new(
        STATUS,
        &[
            ("tasks.nvim", "waiting_runner"),
            ("rules.nvim", "queued"),
            ("hub-desktop", "waiting_runner"),
        ],
    );
    let board = runners::round(&fake, &runners(), Some(&place.programs), false);
    assert_eq!(
        board.waiting.into_iter().collect::<Vec<_>>(),
        ["tasks.nvim"]
    );
    assert!(board.started.is_empty());
    assert_eq!(board.problem, None);
    assert_eq!(board.runners.len(), 4);
    assert_eq!(
        fake.said(),
        ["status all", "remote tasks.nvim", "remote rules.nvim"]
    );
    // The CI is asked the way the watcher asks it.
    let remote = &fake.calls()[1];
    assert_eq!(
        remote[1..9],
        ["-n", "-i", "NONE", "--headless", "-u", "NONE", "-l"]
            .iter()
            .map(|s| (*s).to_owned())
            .chain([place.programs.testing_lua.display().to_string()])
            .collect::<Vec<_>>()[..]
    );
    assert_eq!(remote[9..11], ["ci".to_owned(), "remote".to_owned()]);
}

#[test]
fn with_the_setting_a_waiting_runner_is_started_and_looked_at_again() {
    let place = place(&["tasks.nvim", "rules.nvim"]);
    let mut fake = Fake::new(STATUS, &[("tasks.nvim", "waiting_runner")]);
    fake.after_start = STATUS.replace(
        "tasks.nvim     local=stopped",
        "tasks.nvim     local=running",
    );
    let board = runners::round(&fake, &runners(), Some(&place.programs), true);
    assert_eq!(
        fake.said(),
        [
            "status all",
            "remote tasks.nvim",
            "remote rules.nvim",
            "ensure tasks.nvim",
            "status all"
        ]
    );
    assert_eq!(
        board.started,
        [("tasks.nvim".to_owned(), Started::Online)].into()
    );
    assert!(board.waiting.is_empty(), "it runs now");
    assert!(board
        .runners
        .iter()
        .any(|r| r.repo == "tasks.nvim" && r.running));

    // A start that fails stays a waiting runner, said as failed.
    let mut failing = Fake::new(STATUS, &[("tasks.nvim", "waiting_runner")]);
    failing.ensure_code = 1;
    let board = runners::round(&failing, &runners(), Some(&place.programs), true);
    assert_eq!(
        board.started,
        [("tasks.nvim".to_owned(), Started::Failed)].into()
    );
    assert_eq!(
        board.waiting.into_iter().collect::<Vec<_>>(),
        ["tasks.nvim"]
    );
}

#[test]
fn nothing_but_the_status_without_the_programs_of_the_ci_and_a_broken_status_is_a_problem() {
    let fake = Fake::new(STATUS, &[("tasks.nvim", "waiting_runner")]);
    let board = runners::round(&fake, &runners(), None, true);
    assert_eq!(fake.said(), ["status all"]);
    assert!(board.waiting.is_empty() && board.started.is_empty());

    let broken = Fake::new("pwsh: the term is not recognized\n", &[]);
    *broken.status.lock().unwrap() = vec!["unknown runner: x\n".to_owned()];
    // Exit 1 with no line: no runner is in the wanted state, and none is named; that is an empty board.
    let board = runners::round(&broken, &runners(), None, true);
    assert_eq!(board, Board::default());
}

/// A click starts only a runner the last look saw stopped; a second click while it starts does nothing.
#[test]
fn a_click_starts_a_stopped_runner_of_the_last_look_only() {
    let place = place(&["tasks.nvim"]);
    let mut fake = Fake::new(STATUS, &[]);
    fake.after_start = STATUS.replace(
        "tasks.nvim     local=stopped",
        "tasks.nvim     local=running",
    );
    let fake = Arc::new(fake);
    let watch = RunnerWatch::new(
        Arc::clone(&fake) as Arc<dyn Ask>,
        runners(),
        Some(place.programs.clone()),
    );
    assert_eq!(
        watch.start("tasks.nvim"),
        Err("no runner of that name is stopped here"),
        "no look yet"
    );
    watch.refresh(false);
    for refused in ["hub-desktop", "nope", "../x"] {
        assert_eq!(
            watch.start(refused),
            Err("no runner of that name is stopped here"),
            "{refused}"
        );
    }
    assert!(!fake.said().iter().any(|c| c.starts_with("ensure")));
    assert_eq!(watch.start("tasks.nvim"), Ok(Started::Online));
    assert!(fake.said().contains(&"ensure tasks.nvim".to_owned()));
    assert!(watch.starting().is_empty());
    let board = watch.board().unwrap();
    assert!(board
        .runners
        .iter()
        .any(|r| r.repo == "tasks.nvim" && r.running));
    assert_eq!(board.started.get("tasks.nvim"), Some(&Started::Online));
    assert_eq!(
        watch.start("tasks.nvim"),
        Err("no runner of that name is stopped here"),
        "it runs now"
    );
}

/// The loop runs a round and reads the setting every time: switched to "start without asking", the next round
/// starts the waiting runner.
#[tokio::test(flavor = "multi_thread")]
async fn the_loop_reads_the_setting_before_every_round() {
    let place = place(&["tasks.nvim"]);
    let fake = Arc::new(Fake::new(STATUS, &[("tasks.nvim", "waiting_runner")]));
    let watch = Arc::new(RunnerWatch::new(
        Arc::clone(&fake) as Arc<dyn Ask>,
        runners(),
        Some(place.programs.clone()),
    ));
    let auto = Arc::new(Mutex::new(false));
    let read = Arc::clone(&auto);
    let task = tokio::spawn(runners::watch(
        Arc::clone(&watch),
        Arc::new(move || (1, *read.lock().unwrap())),
        Duration::ZERO,
        Duration::from_millis(5),
        Duration::from_millis(10),
    ));
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(watch
        .board()
        .is_some_and(|b| b.waiting.contains("tasks.nvim")));
    assert!(!fake.said().iter().any(|c| c.starts_with("ensure")));
    *auto.lock().unwrap() = true;
    tokio::time::sleep(Duration::from_millis(150)).await;
    task.abort();
    assert!(fake.said().iter().any(|c| c == "ensure tasks.nvim"));
}

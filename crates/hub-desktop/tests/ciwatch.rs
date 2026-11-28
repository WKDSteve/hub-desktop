//! The CI watcher against a fake of the programs (`Ask`): the tasks command, testing and git answer from a table,
//! nothing is started, no file of a vault is read or written; every call is recorded and checked by its argv.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use appkit_scratch::Scratch;
use hub_desktop::ciwatch::{self, Answer, Ask, Call, Hint, Programs, Verdict};
use hub_desktop::config::Env;

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HEAD: &str = "cccccccccccccccccccccccccccccccccccccccc";
/// 2026-11-21T01:00:00Z
const NOW: u64 = 1_795_222_800;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW)
}

/// What the fake programs say.
#[derive(Clone)]
struct World {
    finished: String,
    remote: String,
    /// The commits git knows in the checkout, and which of them the head contains.
    known: Vec<&'static str>,
    contained: Vec<&'static str>,
    set_code: i32,
    /// What `list --done --ci=pending,red` says, when not the ids of `finished`.
    listed: Option<&'static str>,
    /// The documents `ci remote --sha=<sha>` answers by sha; any other sha gets what an older testing says to an
    /// unknown option (exit 2, nothing on stdout).
    own: Vec<(&'static str, String)>,
}

struct Fake {
    world: World,
    calls: Mutex<Vec<Vec<String>>>,
}

impl Fake {
    fn new(world: World) -> Self {
        Self {
            world,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }

    fn sets(&self) -> Vec<Vec<String>> {
        self.calls()
            .into_iter()
            .filter(|c| c.iter().any(|a| a == "set"))
            .collect()
    }
}

fn ok(text: &str) -> Answer {
    Answer {
        code: Some(0),
        stdout: text.as_bytes().to_vec(),
    }
}

impl Ask for Fake {
    fn ask(&self, call: &Call, _timeout: Duration) -> Result<Answer, String> {
        let args: Vec<String> = call
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let mut line = vec![call.program.to_string_lossy().into_owned()];
        line.extend(args.iter().cloned());
        self.calls.lock().unwrap().push(line);
        let has = |word: &str| args.iter().any(|a| a == word);
        if has("list") {
            if let Some(listed) = self.world.listed {
                return Ok(ok(listed));
            }
            // As tasks.nvim: the ids of the finished tasks whose ci is pending or red.
            let doc: serde_json::Value =
                serde_json::from_str(&self.world.finished).unwrap_or_default();
            let ids: Vec<String> = doc["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|i| matches!(i["ci"].as_str(), Some("pending" | "red")))
                .filter_map(|i| i["id"].as_str().map(str::to_owned))
                .collect();
            return Ok(ok(&ids.join("\n")));
        }
        if has("finished") {
            return Ok(ok(&self.world.finished));
        }
        if has("remote")
            && let Some(sha) = args.iter().find_map(|a| a.strip_prefix("--sha="))
        {
            return Ok(
                match self.world.own.iter().find(|(s, _)| s.starts_with(sha)) {
                    Some((_, doc)) => Answer {
                        code: Some(i32::from(!doc.contains("\"success\""))),
                        stdout: doc.as_bytes().to_vec(),
                    },
                    None => Answer {
                        code: Some(2),
                        stdout: Vec::new(),
                    },
                },
            );
        }
        if has("remote") {
            // A red CI: exit 1 and the document all the same.
            return Ok(Answer {
                code: Some(1),
                stdout: self.world.remote.as_bytes().to_vec(),
            });
        }
        if has("cat-file") {
            let commit = args
                .iter()
                .find(|a| a.ends_with("^{commit}"))
                .cloned()
                .unwrap_or_default();
            let known = self.world.known.iter().any(|k| commit.starts_with(k));
            return Ok(Answer {
                code: Some(if known { 0 } else { 128 }),
                stdout: Vec::new(),
            });
        }
        if has("merge-base") {
            let sha = &args[args.len() - 2];
            let inside = self
                .world
                .contained
                .iter()
                .any(|c| c.starts_with(sha.as_str()));
            return Ok(Answer {
                code: Some(if inside { 0 } else { 1 }),
                stdout: Vec::new(),
            });
        }
        if has("set") {
            return Ok(Answer {
                code: Some(self.world.set_code),
                stdout: Vec::new(),
            });
        }
        Err(format!("unexpected call {args:?}"))
    }
}

struct Place {
    _scratch: Scratch,
    programs: Programs,
}

/// A vault and `$REPOS_DIR` with the checkout `demo` (a `.git` folder is all the watcher looks for itself).
fn place() -> Place {
    let scratch = Scratch::new("hub-ciwatch");
    let repos = scratch.path().join("repos");
    std::fs::create_dir_all(repos.join("demo").join(".git")).unwrap();
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

fn finished(items: &[(&str, &str, &[&str])]) -> String {
    let items: Vec<_> = items
        .iter()
        .map(|(id, ci, commits)| serde_json::json!({ "id": id, "ci": ci, "commits": commits, "title": "x" }))
        .collect();
    serde_json::json!({ "kind": "tasks.finished", "items": items, "total": items.len() })
        .to_string()
}

fn remote(overall: &str, created: &str) -> String {
    remote_at(HEAD, overall, created)
}

fn remote_at(head: &str, overall: &str, created: &str) -> String {
    serde_json::json!({
        "schema": "testing-ci-remote/1",
        "repos": [{ "repo": "demo", "branch": "main", "sha": head, "overall": overall,
                    "run": { "created_at": created, "status": "completed" } }]
    })
    .to_string()
}

fn world(items: &[(&str, &str, &[&str])], overall: &str) -> World {
    World {
        finished: finished(items),
        remote: remote(overall, "2026-11-21T00:50:00Z"),
        known: vec![HEAD],
        contained: vec![SHA_A, SHA_B],
        set_code: 0,
        listed: None,
        own: Vec::new(),
    }
}

fn a() -> String {
    format!("demo@{}", &SHA_A[..12])
}

#[test]
fn a_pending_task_whose_commit_is_in_a_green_head_becomes_green_through_the_tasks_command() {
    let place = place();
    let a = a();
    let fake = Fake::new(world(
        &[("demo/done-task", "pending", &[a.as_str()])],
        "success",
    ));
    let round = ciwatch::round(&fake, &place.programs, now());
    assert_eq!(round.problem, None);
    assert_eq!(round.written.get("demo/done-task"), Some(&Verdict::Green));
    let p = &place.programs;
    let nvim_head = |script: &Path| -> Vec<String> {
        [
            "C:/fake/nvim.exe",
            "-n",
            "-i",
            "NONE",
            "--headless",
            "-u",
            "NONE",
            "-l",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .chain([script.to_string_lossy().into_owned()])
        .collect()
    };
    let vault = format!("--vault={}", p.vault.display());
    let mut calls = fake.calls();
    // 1. which finished tasks wait (one quick call), then their commits through the contract
    let mut listing = nvim_head(&p.tasks_lua);
    listing.extend([
        vault.clone(),
        "list".into(),
        "--done".into(),
        "--ci=pending,red".into(),
        "--format=ids".into(),
    ]);
    assert_eq!(calls[0], listing);
    let mut first = nvim_head(&p.tasks_lua);
    first.extend([
        vault.clone(),
        "call".into(),
        "finished".into(),
        r#"--params={"limit":100,"offset":0}"#.into(),
    ]);
    assert_eq!(calls.remove(1), first);
    // 2. the hosted CI of the repository, read-only
    let dir = p.repos.join("demo");
    let mut second = nvim_head(&p.testing_lua);
    second.extend([
        "ci".into(),
        "remote".into(),
        "--json".into(),
        dir.to_string_lossy().into_owned(),
    ]);
    assert_eq!(calls[1], second);
    // 3. git in the checkout: the head is known, the commit is part of it
    assert_eq!(
        calls[2][1..],
        [
            "-C".to_owned(),
            dir.to_string_lossy().into_owned(),
            "cat-file".into(),
            "-e".into(),
            format!("{HEAD}^{{commit}}")
        ]
    );
    assert_eq!(
        calls[3][1..],
        [
            "-C".to_owned(),
            dir.to_string_lossy().into_owned(),
            "merge-base".into(),
            "--is-ancestor".into(),
            SHA_A[..12].to_owned(),
            HEAD.to_owned()
        ]
    );
    // 4. the verdict through `tasks set`, under the hub's name; nothing else
    let mut set = nvim_head(&p.tasks_lua);
    set.extend([
        vault,
        "set".into(),
        "demo/done-task".into(),
        "ci=green".into(),
        "--by=hub-desktop".into(),
    ]);
    assert_eq!(calls[4], set);
    assert_eq!(calls.len(), 5);
}

#[test]
fn a_finished_red_run_of_the_commit_itself_makes_the_task_red() {
    let place = place();
    let a = a();
    for word in ["failure", "rejected"] {
        // The head of origin/main is the task's commit: its own run is red.
        let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], word);
        w.remote = remote_at(SHA_A, word, "2026-11-21T00:50:00Z");
        w.known = vec![SHA_A];
        let fake = Fake::new(w);
        let round = ciwatch::round(&fake, &place.programs, now());
        assert_eq!(
            round.written.get("demo/done-task"),
            Some(&Verdict::Red),
            "{word}"
        );
        assert!(fake.sets()[0].contains(&"ci=red".to_owned()));
    }
}

#[test]
fn a_later_red_head_says_nothing_against_an_older_commit() {
    let place = place();
    let a = a();
    // HEAD is a later push that contains the task's commit and broke the CI: the task waits, it is not red.
    let fake = Fake::new(world(
        &[("demo/done-task", "pending", &[a.as_str()])],
        "failure",
    ));
    let round = ciwatch::round(&fake, &place.programs, now());
    assert!(round.written.is_empty() && fake.sets().is_empty());
}

/// A document of `testing ci remote` for one commit; `guard` is `"refused"` for a run the runner guard turned away.
fn remote_run(sha: &str, overall: &str, guard: Option<&str>) -> String {
    let mut repo = serde_json::json!({
        "repo": "demo", "branch": "main", "sha": sha, "overall": overall,
        "run": { "created_at": "2026-11-21T00:50:00Z", "status": "completed" }
    });
    if let Some(guard) = guard {
        repo["guard"] = guard.into();
    }
    serde_json::json!({ "schema": "testing-ci-remote/1", "repos": [repo] }).to_string()
}

#[test]
fn a_red_head_does_not_keep_an_older_commit_pending_when_its_own_run_was_green() {
    let place = place();
    let a = a();
    // HEAD (a later push) is red; the task's commit is an ancestor whose own run was green on every system.
    let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], "failure");
    w.own = vec![(SHA_A, remote_run(SHA_A, "success", None))];
    let fake = Fake::new(w);
    let round = ciwatch::round(&fake, &place.programs, now());
    assert_eq!(round.written.get("demo/done-task"), Some(&Verdict::Green));
    assert!(fake.sets()[0].contains(&"ci=green".to_owned()));
    let own: Vec<_> = fake
        .calls()
        .into_iter()
        .filter(|c| {
            c.iter()
                .any(|x| x == &format!("--sha={a_sha}", a_sha = &SHA_A[..12]))
        })
        .collect();
    assert_eq!(own.len(), 1, "one question about the commit itself");
}

#[test]
fn the_own_run_of_an_older_commit_counts_red_only_when_it_was_red_and_not_refused() {
    let place = place();
    let a = a();
    let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], "failure");
    w.own = vec![(SHA_A, remote_run(SHA_A, "failure", None))];
    let round = ciwatch::round(&Fake::new(w), &place.programs, now());
    assert_eq!(round.written.get("demo/done-task"), Some(&Verdict::Red));
    // The document of another commit (a testing that ignores the option) is never taken for the commit's own run.
    let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], "failure");
    w.own = vec![(SHA_A, remote_run(HEAD, "success", None))];
    let fake = Fake::new(w);
    let round = ciwatch::round(&fake, &place.programs, now());
    assert!(round.written.is_empty() && fake.sets().is_empty());
}

#[test]
fn a_run_the_runner_guard_turned_away_is_a_hint_awaiting_release_never_red() {
    let place = place();
    let a = a();
    // The head is the task's commit and its run was refused: pending with a hint, nothing written.
    let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], "failure");
    w.remote = serde_json::to_string(&{
        let mut doc: serde_json::Value =
            serde_json::from_str(&remote_at(SHA_A, "failure", "2026-11-21T00:50:00Z")).unwrap();
        doc["repos"][0]["guard"] = "refused".into();
        doc
    })
    .unwrap();
    w.known = vec![SHA_A];
    let fake = Fake::new(w);
    let round = ciwatch::round(&fake, &place.programs, now());
    assert!(round.written.is_empty() && fake.sets().is_empty());
    assert!(round.hints.contains(&Hint::AwaitsTrust {
        repo: "demo".to_owned()
    }));
    // Same for an older commit whose own run was refused while the head is red.
    let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], "failure");
    w.own = vec![(SHA_A, remote_run(SHA_A, "failure", Some("refused")))];
    let round = ciwatch::round(&Fake::new(w), &place.programs, now());
    assert!(round.written.is_empty());
    assert!(round.hints.contains(&Hint::AwaitsTrust {
        repo: "demo".to_owned()
    }));
}

#[test]
fn a_run_queued_for_more_than_30_minutes_is_a_hint_never_red() {
    let place = place();
    let a = a();
    for (word, created, hint) in [
        ("queued", "2026-11-21T00:20:00Z", true),
        ("waiting_runner", "2026-11-21T00:29:00Z", true),
        ("queued", "2026-11-21T00:40:00Z", false), // 20 minutes: no hint yet
    ] {
        let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], word);
        w.remote = remote(word, created);
        let fake = Fake::new(w);
        let round = ciwatch::round(&fake, &place.programs, now());
        assert!(
            round.written.is_empty() && fake.sets().is_empty(),
            "{word}: no verdict"
        );
        let queued = round
            .hints
            .iter()
            .any(|h| matches!(h, Hint::Queued { repo, .. } if repo == "demo"));
        assert_eq!(queued, hint, "{word} since {created}");
    }
}

#[test]
fn only_a_green_head_that_contains_every_commit_makes_a_task_green() {
    let place = place();
    let a = a();
    let b = format!("demo@{}", &SHA_B[..12]);
    // b is not on origin/main yet: the task waits.
    let mut w = world(
        &[("demo/two", "pending", &[a.as_str(), b.as_str()])],
        "success",
    );
    w.contained = vec![SHA_A];
    let fake = Fake::new(w);
    let round = ciwatch::round(&fake, &place.programs, now());
    assert!(round.written.is_empty() && fake.sets().is_empty());
    // A run still going, a run cancelled (a newer one took over): pending.
    for word in ["running", "cancelled", "no_run", "unknown"] {
        let fake = Fake::new(world(&[("demo/done-task", "pending", &[a.as_str()])], word));
        assert!(
            ciwatch::round(&fake, &place.programs, now())
                .written
                .is_empty(),
            "{word}"
        );
    }
}

#[test]
fn a_head_the_checkout_does_not_know_is_a_hint_and_no_verdict() {
    let place = place();
    let a = a();
    let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], "success");
    w.known = vec![];
    let fake = Fake::new(w);
    let round = ciwatch::round(&fake, &place.programs, now());
    assert!(round.written.is_empty());
    assert!(round.hints.contains(&Hint::Behind {
        repo: "demo".into()
    }));
}

#[test]
fn a_red_task_turns_green_with_a_later_green_head_and_is_not_written_red_again() {
    let place = place();
    let a = a();
    let fake = Fake::new(world(
        &[("demo/done-task", "red", &[a.as_str()])],
        "success",
    ));
    assert_eq!(
        ciwatch::round(&fake, &place.programs, now())
            .written
            .get("demo/done-task"),
        Some(&Verdict::Green)
    );
    let fake = Fake::new(world(
        &[("demo/done-task", "red", &[a.as_str()])],
        "failure",
    ));
    let round = ciwatch::round(&fake, &place.programs, now());
    assert!(
        round.written.is_empty() && fake.sets().is_empty(),
        "already red: nothing to write"
    );
}

#[test]
fn what_the_watcher_does_not_look_at() {
    let place = place();
    let a = a();
    let other = format!("absent@{}", &SHA_A[..12]);
    let w = World {
        finished: finished(&[
            ("demo/green-already", "green", &[a.as_str()]),
            ("demo/no-commits", "pending", &[]),
            ("../evil/x", "pending", &[a.as_str()]),
            (
                "demo/odd-commit",
                "pending",
                &["demo@xyz", "../demo@aaaaaaa"],
            ),
            ("demo/elsewhere", "pending", &[other.as_str()]),
        ]),
        ..world(&[], "success")
    };
    let fake = Fake::new(w);
    let round = ciwatch::round(&fake, &place.programs, now());
    assert!(round.written.is_empty() && fake.sets().is_empty());
    assert!(round.hints.contains(&Hint::NoCheckout {
        repo: "absent".into()
    }));
    assert!(!fake
        .calls()
        .iter()
        .any(|c| c.iter().any(|a| a.contains("evil"))));
}

#[test]
fn a_verdict_the_tasks_command_refuses_is_said_not_written() {
    let place = place();
    let a = a();
    let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], "success");
    w.set_code = 1;
    let round = ciwatch::round(&Fake::new(w), &place.programs, now());
    assert!(round.written.is_empty());
    assert_eq!(
        round.not_written.get("demo/done-task"),
        Some(&Verdict::Green)
    );
}

#[test]
fn no_hosted_ci_and_a_broken_answer_are_hints() {
    let place = place();
    let a = a();
    let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], "success");
    w.remote = serde_json::json!({ "schema": "testing-ci-remote/1", "repos": [{ "overall": "no_github" }] }).to_string();
    let round = ciwatch::round(&Fake::new(w), &place.programs, now());
    assert!(
        round.hints.contains(&Hint::NoGithub {
            repo: "demo".into()
        }) && round.written.is_empty()
    );
    let mut w = world(&[("demo/done-task", "pending", &[a.as_str()])], "success");
    w.remote = "not json".into();
    let round = ciwatch::round(&Fake::new(w), &place.programs, now());
    assert!(
        round
            .hints
            .iter()
            .any(|h| matches!(h, Hint::NoAnswer { .. }))
            && round.written.is_empty()
    );
    // A task is listed, but the contract answers with an error: the round says so and writes nothing.
    let mut w = world(&[], "success");
    w.finished = "{\"kind\":\"tasks.error\"}".into();
    w.listed = Some("demo/done-task\n");
    let fake = Fake::new(w);
    let round = ciwatch::round(&fake, &place.programs, now());
    assert_eq!(
        round.problem,
        Some("the tasks command gave no tasks.finished document")
    );
    assert!(fake.sets().is_empty());
    // Nothing listed: one call and the round is over.
    let fake = Fake::new(world(&[], "success"));
    let round = ciwatch::round(&fake, &place.programs, now());
    assert_eq!(round, ciwatch::Round::default());
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn the_programs_come_from_the_path_and_repos_dir_only() {
    let scratch = Scratch::new("hub-ciwatch-find");
    let root = scratch.path();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = std::env::consts::EXE_SUFFIX;
    std::fs::write(bin.join(format!("nvim{exe}")), "").unwrap();
    std::fs::write(bin.join(format!("git{exe}")), "").unwrap();
    let repos = root.join("repos");
    for (dir, file) in [("tasks.nvim", "tasks.lua"), ("testing.nvim", "testing.lua")] {
        std::fs::create_dir_all(repos.join(dir).join("scripts")).unwrap();
        std::fs::write(repos.join(dir).join("scripts").join(file), "").unwrap();
    }
    std::fs::create_dir_all(root.join("vault")).unwrap();
    let env = |vault: Option<&Path>| {
        let mut pairs: Vec<(&str, OsString)> = vec![
            ("PATH", bin.as_os_str().to_owned()),
            ("REPOS_DIR", repos.as_os_str().to_owned()),
        ];
        if let Some(v) = vault {
            pairs.push(("TASKS_VAULT", v.as_os_str().to_owned()));
        }
        Env::from_pairs(pairs)
    };
    let found = Programs::find(&env(Some(&root.join("vault")))).unwrap();
    assert_eq!(found.nvim, bin.join(format!("nvim{exe}")));
    assert_eq!(found.tasks_lua, repos.join("tasks.nvim/scripts/tasks.lua"));
    assert_eq!(
        Programs::find(&env(None)).unwrap_err(),
        "TASKS_VAULT names no folder"
    );
    assert_eq!(
        Programs::find(&env(Some(Path::new("relative/vault")))).unwrap_err(),
        "TASKS_VAULT names no folder"
    );
    std::fs::remove_file(repos.join("testing.nvim/scripts/testing.lua")).unwrap();
    assert_eq!(
        Programs::find(&env(Some(&root.join("vault")))).unwrap_err(),
        "no checkout of testing.nvim in REPOS_DIR"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_watcher_runs_a_round_after_the_first_wait_and_again_every_time_until_it_is_ended() {
    let place = place();
    let fake = std::sync::Arc::new(Fake::new(world(&[], "success")));
    let watcher = std::sync::Arc::new(ciwatch::Watcher::default());
    assert!(watcher.last().is_none(), "no round before the first wait");
    let task = tokio::spawn(ciwatch::watch(
        std::sync::Arc::clone(&watcher),
        std::sync::Arc::clone(&fake) as std::sync::Arc<dyn Ask>,
        place.programs.clone(),
        std::sync::Arc::new(ciwatch::Schedule::default),
        Duration::from_millis(30),
        Duration::from_millis(5),
        // Five minutes of this kind are 30 ms.
        Duration::from_millis(6),
    ));
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(fake.calls().is_empty(), "the first round waits");
    tokio::time::sleep(Duration::from_millis(200)).await;
    task.abort();
    // A round on the blocking pool at that moment runs to its end (the hub's end ends its programs); after it, none.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let rounds = fake.calls().len();
    assert!(rounds >= 2, "{rounds} rounds");
    assert_eq!(watcher.last(), Some(ciwatch::Round::default()));
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        fake.calls().len(),
        rounds,
        "ended: no round after the abort"
    );
}

/// The settings are read before every round: a new interval, the switch off and on again count without a new start.
#[tokio::test(flavor = "multi_thread")]
async fn the_watcher_follows_its_settings_without_a_new_start() {
    use ciwatch::{Schedule, Seen, OFF_BY_SETTINGS};
    let place = place();
    let fake = std::sync::Arc::new(Fake::new(world(&[], "success")));
    let watcher = std::sync::Arc::new(ciwatch::Watcher::default());
    // A minute of this test is 10 ms: 60 of them are 600 ms, 1 is 10 ms.
    let held = std::sync::Arc::new(std::sync::Mutex::new(Schedule {
        on: true,
        interval_min: 60,
    }));
    let read = std::sync::Arc::clone(&held);
    let task = tokio::spawn(ciwatch::watch(
        std::sync::Arc::clone(&watcher),
        std::sync::Arc::clone(&fake) as std::sync::Arc<dyn Ask>,
        place.programs.clone(),
        std::sync::Arc::new(move || *read.lock().unwrap()),
        Duration::ZERO,
        Duration::from_millis(5),
        Duration::from_millis(10),
    ));
    let rounds = || fake.calls().len();
    let wait_for = |what: &str, check: &dyn Fn() -> bool| {
        let end = std::time::Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(
                std::time::Instant::now() < end,
                "timed out waiting for {what}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    wait_for("the first round", &|| rounds() == 1);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(rounds(), 1, "60 minutes have not passed");

    held.lock().unwrap().interval_min = 1;
    // A round every 10 ms (and a look every 5): many in 300 ms, where the old interval would give none.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        rounds() >= 4,
        "{} rounds with the shorter interval",
        rounds()
    );

    held.lock().unwrap().on = false;
    wait_for("the switch off", &|| {
        watcher.seen() == Seen::Off(OFF_BY_SETTINGS)
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    let off = rounds();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(rounds(), off, "switched off: no round");

    held.lock().unwrap().on = true;
    wait_for("a round at once after the switch on", &|| rounds() > off);
    wait_for("the round to be shown", &|| {
        matches!(watcher.seen(), Seen::Ran { .. })
    });
    task.abort();
}

/// Switched off already when the hub starts: the page says so at once, not "no round yet" for the first wait.
#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_switched_off_says_so_before_its_first_wait_ends() {
    let place = place();
    let fake = std::sync::Arc::new(Fake::new(world(&[], "success")));
    let watcher = std::sync::Arc::new(ciwatch::Watcher::default());
    let task = tokio::spawn(ciwatch::watch(
        std::sync::Arc::clone(&watcher),
        std::sync::Arc::clone(&fake) as std::sync::Arc<dyn Ask>,
        place.programs.clone(),
        std::sync::Arc::new(|| ciwatch::Schedule {
            on: false,
            interval_min: 5,
        }),
        Duration::from_secs(60),
        Duration::from_millis(5),
        Duration::from_millis(10),
    ));
    let end = std::time::Instant::now() + Duration::from_secs(5);
    while watcher.seen() != ciwatch::Seen::Off(ciwatch::OFF_BY_SETTINGS) {
        assert!(
            std::time::Instant::now() < end,
            "still {:?}",
            watcher.seen()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    task.abort();
    assert!(fake.calls().is_empty());
}

/// Ten finished tasks wait, each for a commit of its own that the red head contains; two more tasks share one of
/// those commits. The commits have no run of their own yet.
fn many_waiting() -> (World, Vec<&'static str>) {
    let shas: Vec<&'static str> = (1..=10u32)
        .map(|n| &*Box::leak(format!("{n:02x}{}", "d".repeat(38)).into_boxed_str()))
        .collect();
    let commits: Vec<String> = shas.iter().map(|s| format!("demo@{}", &s[..12])).collect();
    let ids: Vec<String> = (1..=10).map(|n| format!("demo/task-{n}")).collect();
    let mut items: Vec<(&str, &str, &[&str])> = Vec::new();
    let lists: Vec<[&str; 1]> = commits.iter().map(|c| [c.as_str()]).collect();
    for (id, list) in ids.iter().zip(&lists) {
        items.push((id.as_str(), "pending", list.as_slice()));
    }
    items.push(("demo/shares-1", "pending", lists[0].as_slice()));
    items.push(("demo/shares-2", "pending", lists[1].as_slice()));
    let mut w = world(&items, "failure");
    w.contained = shas.clone();
    (w, shas)
}

/// The `--sha=` of each call for the run of a single commit.
fn own_asks(fake: &Fake) -> Vec<String> {
    fake.calls()
        .into_iter()
        .filter_map(|c| {
            c.iter()
                .find_map(|a| a.strip_prefix("--sha=").map(str::to_owned))
        })
        .collect()
}

#[test]
fn a_red_head_asks_for_each_commit_once_and_at_most_the_bound_in_a_round() {
    let place = place();
    let (w, _) = many_waiting();
    let fake = Fake::new(w);
    let round = ciwatch::round(&fake, &place.programs, now());
    assert!(round.written.is_empty());
    let asked = own_asks(&fake);
    assert_eq!(asked.len(), ciwatch::OWN_ASKS_PER_ROUND, "{asked:?}");
    let mut once = asked.clone();
    once.sort();
    once.dedup();
    assert_eq!(once.len(), asked.len(), "a commit asked twice: {asked:?}");
}

#[test]
fn an_answer_that_is_no_verdict_is_kept_for_some_rounds_and_a_verdict_while_it_is_watched() {
    let place = place();
    let (mut w, shas) = many_waiting();
    // The first commit's own run was red: that task is red, and the commit is not asked about again.
    w.own = vec![(shas[0], remote_run(shas[0], "failure", None))];
    let mut memory = ciwatch::OwnRuns::default();

    let fake = Fake::new(w.clone());
    let first = ciwatch::round_with(&fake, &place.programs, now(), &mut memory);
    assert_eq!(first.written.get("demo/task-1"), Some(&Verdict::Red));
    let first_asked = own_asks(&fake);
    assert_eq!(first_asked.len(), ciwatch::OWN_ASKS_PER_ROUND);

    // The next round asks only for the commits the first did not get to.
    let mut w2 = w.clone();
    w2.finished = w2.finished.replace(
        r#""ci":"pending","commits":["demo@01dddddddddd"#,
        r#""ci":"red","commits":["demo@01dddddddddd"#,
    );
    let fake = Fake::new(w2.clone());
    ciwatch::round_with(&fake, &place.programs, now(), &mut memory);
    let second_asked = own_asks(&fake);
    assert_eq!(
        second_asked.len(),
        10 - ciwatch::OWN_ASKS_PER_ROUND,
        "{second_asked:?}"
    );
    assert!(second_asked.iter().all(|s| !first_asked.contains(s)));

    // After the rounds an answer without a verdict is kept, it is asked again; the red one never.
    let mut later = Vec::new();
    for _ in 0..ciwatch::OWN_KEEP_ROUNDS {
        let fake = Fake::new(w2.clone());
        ciwatch::round_with(&fake, &place.programs, now(), &mut memory);
        later.extend(own_asks(&fake));
    }
    assert!(!later.is_empty(), "asked again after the rounds");
    assert!(!later.contains(&shas[0][..12].to_owned()), "{later:?}");
}

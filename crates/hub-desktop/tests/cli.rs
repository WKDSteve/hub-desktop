//! The program itself, run as a person runs it: a double click, a second double click, `serve`
//! beside a hub that runs, `stop`. The config folder is a folder of the test (`HUB_DESKTOP_CONFIG_DIR`),
//! the port a free one, and `HUB_DESKTOP_NO_LAUNCH` prints the pairing link instead of opening a
//! browser: nothing of the machine's own is touched.

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use appkit_scratch::Scratch;

mod common;

const PROGRAM: &str = env!("CARGO_BIN_EXE_hub-desktop");

/// A port nobody listens on right now.
fn free_port() -> u16 {
    // The listener is dropped at once, so two calls can see the same number; a number is handed out once per process.
    static GIVEN: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut given = GIVEN.lock().unwrap();
        if !given.contains(&port) {
            given.push(port);
            return port;
        }
    }
}

fn command(config: &Path, port: u16) -> Command {
    let mut cmd = Command::new(PROGRAM);
    cmd.env("HUB_DESKTOP_CONFIG_DIR", config)
        .env("HUB_DESKTOP_PORT", port.to_string())
        .env("HUB_DESKTOP_NO_LAUNCH", "1")
        // Never the port of a hub that may run on this machine.
        .env("HUB_DESKTOP_WEB_PORT", free_port().to_string())
        .stdin(Stdio::null());
    cmd
}

fn run(config: &Path, port: u16, args: &[&str]) -> Output {
    command(config, port).args(args).output().unwrap()
}

/// Wait until `check` holds, or fail with `what`.
fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

/// A hub that runs; it is stopped when the value drops, so a failed test leaves no process behind.
struct Running {
    child: Child,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_by_click(config: &Path, port: u16) -> Running {
    start_by_click_with(config, port, &[])
}

fn start_by_click_with(config: &Path, port: u16, env: &[(&str, &Path)]) -> Running {
    let out = std::fs::File::create(config.join("click-out.txt")).unwrap();
    let mut cmd = command(config, port);
    for (name, value) in env {
        cmd.env(name, value);
    }
    let child = cmd
        .stdout(Stdio::from(out))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let running = Running { child };
    wait_for("the hub of the click to announce itself", || {
        config.join("hub.json").is_file()
            && std::fs::read_to_string(config.join("click-out.txt"))
                .is_ok_and(|text| text.contains("#pair="))
    });
    running
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_click_runs_the_hub_and_a_second_click_opens_on_it_instead_of_starting_another() {
    let scratch = Scratch::new("hub-cli-click");
    let config = scratch.path();
    let port = free_port();
    let mut first = start_by_click(config, port);

    // The first click printed the link of the hub it started.
    let printed = std::fs::read_to_string(config.join("click-out.txt")).unwrap();
    let link = printed
        .lines()
        .find(|l| l.starts_with("http://"))
        .expect("the click printed a pairing link");
    assert!(
        link.starts_with(&format!("http://127.0.0.1:{port}/#pair=")),
        "{link}"
    );
    let record_before = std::fs::read_to_string(config.join("hub.json")).unwrap();

    // The second click finds the hub: success, a link of its own, and the same hub.
    let second = run(config, port, &[]);
    assert!(second.status.success(), "{second:?}");
    let second_link = text(&second);
    assert!(
        second_link.starts_with(&format!("http://127.0.0.1:{port}/#pair=")),
        "{second_link}"
    );
    assert_ne!(second_link.trim(), link, "a link is good once");
    assert_eq!(
        std::fs::read_to_string(config.join("hub.json")).unwrap(),
        record_before,
        "the second click must not replace the hub"
    );
    assert!(
        first.child.try_wait().unwrap().is_none(),
        "the hub still runs"
    );

    // `serve` beside a hub that runs is refused in words.
    let again = run(config, port, &["serve"]);
    assert!(!again.status.success());
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("already running"),
        "{again:?}"
    );

    // `stop` ends it and takes its files away. It asks over the control route and the hub ends itself:
    // the kit would end a hub that does not listen the hard way, after 10 s, and leave its files.
    let asked = Instant::now();
    let stopped = run(config, port, &["stop"]);
    assert!(stopped.status.success(), "{stopped:?}");
    wait_for("the hub to end", || {
        first.child.try_wait().unwrap().is_some()
    });
    assert!(
        asked.elapsed() < Duration::from_secs(8),
        "the hub ended only after {:?}: it did not listen to stop",
        asked.elapsed()
    );
    assert!(
        first.child.try_wait().unwrap().is_some_and(|s| s.success()),
        "the hub ended itself"
    );
    assert!(!config.join("hub.json").exists());
}

#[test]
fn the_page_and_hello_come_from_the_program_and_link_prints_a_link_that_pairs() {
    let scratch = Scratch::new("hub-cli-page");
    let config = scratch.path();
    let port = free_port();
    let _hub = start_by_click(config, port);

    let link = text(&run(config, port, &["link"]));
    let code = link
        .trim()
        .split_once("#pair=")
        .expect("a pairing link")
        .1
        .to_owned();
    assert_eq!(code.len(), 32, "{link}");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let hello = hub_desktop::client::request(port, "GET", "/api/hub/hello", &[], "")
            .await
            .unwrap();
        assert_eq!(hello.status, 200);
        assert!(
            hello.body.contains(r#""app":"hub-desktop""#),
            "{}",
            hello.body
        );
        let page = hub_desktop::client::request(port, "GET", "/", &[], "")
            .await
            .unwrap();
        assert_eq!(page.status, 200);
        // Without `ui/dist` at build time the binary carries the placeholder page of the kit: say so, not only that
        // a string is missing (a fresh checkout; the CI builds the page first).
        assert!(
            hub_desktop::ui::UiFiles::embedded().has_index(),
            "{}",
            hub_desktop::ui::NOT_BUILT
        );
        assert!(page.body.contains("<title>Hub</title>"), "{}", page.body);
    });
}

#[test]
fn the_version_line_is_the_one_a_release_is_compared_with() {
    let scratch = Scratch::new("hub-cli-version");
    let out = run(scratch.path(), free_port(), &["--version"]);
    assert!(out.status.success());
    assert_eq!(text(&out).trim(), hub_desktop::version_line());
}

#[test]
fn an_unknown_verb_and_a_stray_argument_print_the_usage_and_fail() {
    let scratch = Scratch::new("hub-cli-usage");
    for args in [
        vec!["bogus"],
        vec!["install", "--autostart"],
        vec!["install", "--shortcut", "--shortcut"],
        vec!["serve", "--port", "80"],
        vec!["open", "extra"],
        vec!["--version", "x"],
        vec!["logs", "-n", "0"],
        vec!["help", "bogus"],
    ] {
        let out = run(scratch.path(), free_port(), &args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("usage:") || err.contains("--port must be") || err.contains("logs:"),
            "{args:?}: {err}"
        );
    }
    // `close` would read as "close the window, the hub runs on": it points to `stop` instead.
    let close = run(scratch.path(), free_port(), &["close"]);
    assert_eq!(close.status.code(), Some(2), "{close:?}");
    assert!(
        String::from_utf8_lossy(&close.stderr).contains("`stop`"),
        "{close:?}"
    );
}

/// The commands every hub app shares, on the program: `status` (text, JSON, exit code 0 runs / 1 does not), `quit` as
/// `stop`, `paths`, `logs` without a log, `version` and `help` for one command, common or own.
#[test]
fn the_common_commands_say_whether_the_hub_runs_and_quit_ends_it() {
    let scratch = Scratch::new("hub-cli-common");
    let config = scratch.path();
    let port = free_port();

    let off = run(config, port, &["status"]);
    assert_eq!(off.status.code(), Some(1), "{off:?}");
    assert!(
        text(&off).starts_with("hub-desktop: not running\n"),
        "{off:?}"
    );

    let mut hub = start_by_click(config, port);
    let on = run(config, port, &["status"]);
    assert_eq!(on.status.code(), Some(0), "{on:?}");
    let said = text(&on);
    assert!(
        said.starts_with("hub-desktop: running (process ")
            && said.contains(&format!("port {port}, version {}", hub_desktop::VERSION)),
        "{said}"
    );
    assert!(said.contains(&config.display().to_string()), "{said}");
    let json = run(config, port, &["status", "--json"]);
    assert_eq!(json.status.code(), Some(0), "{json:?}");
    let doc: serde_json::Value = serde_json::from_str(text(&json).trim()).unwrap();
    assert_eq!(doc["running"], true);
    assert_eq!(doc["port"], port);
    assert_eq!(doc["app"], "hub-desktop");

    let paths = run(config, port, &["paths"]);
    assert!(paths.status.success(), "{paths:?}");
    assert!(
        text(&paths).contains(&format!("settings:  {}", config.display())),
        "{paths:?}"
    );

    // A hub that runs in a terminal writes no hub.log: `logs` says why there is none.
    let logs = run(config, port, &["logs"]);
    assert_eq!(logs.status.code(), Some(1), "{logs:?}");
    assert!(
        String::from_utf8_lossy(&logs.stderr).contains("no log at"),
        "{logs:?}"
    );

    let quit = run(config, port, &["quit"]);
    assert!(quit.status.success(), "{quit:?}");
    wait_for("the hub to end on quit", || {
        hub.child.try_wait().unwrap().is_some()
    });
    assert_eq!(run(config, port, &["status"]).status.code(), Some(1));

    let version = run(config, port, &["version"]);
    assert_eq!(text(&version).trim(), hub_desktop::version_line());
    for (command, expect) in [
        ("status", "hub-desktop status [--json]"),
        ("serve", "hub-desktop serve [--port"),
        ("install", "make the\n"),
    ] {
        let help = run(config, port, &["help", command]);
        assert!(help.status.success(), "{command}: {help:?}");
        assert!(text(&help).contains(expect), "{command}: {help:?}");
    }
    let asked = run(config, port, &["link", "--help"]);
    assert!(text(&asked).starts_with("hub-desktop link"), "{asked:?}");
    let all = text(&run(config, port, &["--help"]));
    for word in [
        "hub-desktop status [--json]",
        "hub-desktop tray",
        "Exit codes",
    ] {
        assert!(all.contains(word), "{word}: {all}");
    }
}

/// The listener of the web variants is part of the program: its own port, its own origin, the page of an
/// app of the catalog from the checkout, and nothing of the hub.
#[test]
fn the_program_serves_a_web_variant_on_a_port_of_its_own() {
    let scratch = Scratch::new("hub-cli-web");
    let config = scratch.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    let repos = scratch.path().join("repos");
    std::fs::create_dir_all(repos.join("tasks-web")).unwrap();
    std::fs::write(repos.join("tasks-web/tasks.html"), "<title>a page</title>").unwrap();
    let port = free_port();
    let web_port = free_port();
    let web = web_port.to_string();
    let _hub = start_by_click_with(
        &config,
        port,
        &[
            ("REPOS_DIR", repos.as_path()),
            ("HUB_DESKTOP_WEB_PORT", Path::new(&web)),
            (
                "HUB_DESKTOP_BROWSER_LOG",
                scratch.path().join("browser.log").as_path(),
            ),
        ],
    );
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let page = hub_desktop::client::request(web_port, "GET", "/tasks-web/tasks.html", &[], "")
            .await
            .unwrap();
        assert_eq!(page.status, 200, "{page:?}");
        assert_eq!(page.body, "<title>a page</title>");
        // The hub is not on this port, and the page is not on the port of the hub.
        let api = hub_desktop::client::request(web_port, "GET", "/api/hub/hello", &[], "")
            .await
            .unwrap();
        assert_eq!(api.status, 404);
        let wrong = hub_desktop::client::request(port, "GET", "/tasks-web/tasks.html", &[], "")
            .await
            .unwrap();
        assert!(!wrong.body.contains("a page"), "{wrong:?}");
    });
}

/// `serve` as `start` runs it where there is no icon (`HUB_DESKTOP_DETACHED`), with a short time alone and without the
/// login entries of the machine (`HUB_DESKTOP_NO_LOGIN_SYNC`). Its output goes to `out`.
fn serve_in_background(config: &Path, port: u16, detached: bool, out: &Path) -> Running {
    let mut cmd = command(config, port);
    cmd.arg("serve")
        .env("HUB_DESKTOP_ALONE_SECS", "1")
        .env("HUB_DESKTOP_NO_LOGIN_SYNC", "1")
        .stdout(Stdio::from(std::fs::File::create(out).unwrap()))
        .stderr(Stdio::null());
    if detached {
        cmd.env("HUB_DESKTOP_DETACHED", "1");
    }
    Running {
        child: cmd.spawn().unwrap(),
    }
}

/// A hub in the background without an icon that nobody opens ends itself: nobody would see or stop it.
#[test]
fn a_hub_in_the_background_without_an_icon_that_nobody_opens_ends_itself() {
    let scratch = Scratch::new("hub-cli-alone");
    let config = scratch.path();
    let port = free_port();
    let out = config.join("serve-out.txt");
    let mut hub = serve_in_background(config, port, true, &out);
    wait_for("the hub to write hub.json", || {
        config.join("hub.json").is_file()
    });
    let end = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = hub.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < end, "the hub left alone did not end");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(status.success(), "{status:?}");
    assert!(!config.join("hub.json").exists());
    let said = std::fs::read_to_string(&out).unwrap();
    assert!(said.contains("no page is connected"), "{said}");
}

/// An order to stay is the person's: `keep_running` keeps a hub in the background; a hub in a terminal (or the tray,
/// whose icon shows it) is never left alone.
#[test]
fn a_hub_with_an_order_to_stay_and_a_hub_in_a_terminal_do_not_end_themselves() {
    let kept = Scratch::new("hub-cli-alone-kept");
    std::fs::write(kept.path().join("config.json"), r#"{"keep_running":true}"#).unwrap();
    let terminal = Scratch::new("hub-cli-alone-terminal");
    let mut first =
        serve_in_background(kept.path(), free_port(), true, &kept.path().join("out.txt"));
    let mut second = serve_in_background(
        terminal.path(),
        free_port(),
        false,
        &terminal.path().join("out.txt"),
    );
    wait_for("both hubs to write hub.json", || {
        kept.path().join("hub.json").is_file() && terminal.path().join("hub.json").is_file()
    });
    // Several times the time alone.
    std::thread::sleep(Duration::from_secs(5));
    assert!(
        first.child.try_wait().unwrap().is_none(),
        "keep_running did not keep it"
    );
    assert!(
        second.child.try_wait().unwrap().is_none(),
        "the hub in a terminal ended"
    );
}

/// On Linux and macOS the program is also the watcher of the process group of each child it starts (appkit's
/// `child::watcher_main`, the first call of `main`): started with `--appkit-child-watcher` it only waits on descriptor
/// 3, here a pipe with one byte in it (the group released), and ends without a word. Without that call the argument
/// would be a usage error. In a process group of its own, so nothing of the test run is ever its group.
#[cfg(unix)]
#[test]
fn the_program_is_the_watcher_of_its_childrens_groups() {
    use std::io::Write;
    use std::os::unix::process::CommandExt;

    let mut watcher = Command::new("/bin/sh")
        .args(["-c", "exec \"$0\" --appkit-child-watcher 3<&0"])
        .arg(env!("CARGO_BIN_EXE_hub-desktop"))
        .env_clear()
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the program starts");
    let mut pipe = watcher.stdin.take().unwrap();
    pipe.write_all(&[1]).unwrap();
    drop(pipe);
    let out = watcher.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
}

/// A hub of a test never runs the CI watcher, whatever `config.json` says (nothing: on; `true`): a test starts the
/// real program, and with the environment of the person it would write into the person's vault. Everything the
/// watcher needs is there and a fake that logs every start (nvim and git on the `PATH`, the scripts of tasks.nvim
/// and testing.nvim, a vault): the hub says why the watcher does not run, nothing is started, the vault stays as
/// it was. `ciwatch::allowed` has each variable of a test on its own.
#[test]
fn a_hub_of_a_test_never_runs_the_ci_watcher() {
    const TASK: &str = "ci: pending";
    for setting in [None, Some(r#"{"ci_watch":true}"#)] {
        let scratch = Scratch::new("hub-cli-ciwatch");
        let config = scratch.path().join("cfg");
        let bin = scratch.path().join("bin");
        let repos = scratch.path().join("repos");
        let vault = scratch.path().join("vault");
        for dir in [&config, &bin, &vault] {
            std::fs::create_dir_all(dir).unwrap();
        }
        for name in ["nvim", "git", "pwsh"] {
            let file = bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            std::fs::copy(common::fake_app_exe(), file).unwrap();
        }
        for script in [
            "tasks.nvim/scripts/tasks.lua",
            "testing.nvim/scripts/testing.lua",
        ] {
            let file = repos.join(script);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "os.exit(3)").unwrap();
        }
        std::fs::write(vault.join("task.md"), TASK).unwrap();
        // The script of the runners, so that only the variables of the test keep them from being looked at.
        let nvim_config = scratch.path().join("nvim");
        std::fs::create_dir_all(nvim_config.join("scripts")).unwrap();
        std::fs::write(nvim_config.join("scripts/runners.ps1"), "exit 3").unwrap();
        if let Some(text) = setting {
            std::fs::write(config.join("config.json"), text).unwrap();
        }
        let err = config.join("serve-err.txt");
        let mut cmd = command(&config, free_port());
        cmd.arg("serve")
            .env("HUB_DESKTOP_NO_LOGIN_SYNC", "1")
            .env("PATH", &bin)
            .env("REPOS_DIR", &repos)
            .env("TASKS_VAULT", &vault)
            .env("NVIM_CONFIG_DIR", &nvim_config)
            .env("NVIM_MACHINE_ROLE", "default")
            .env_remove("USERDOMAIN")
            .env_remove("COMPUTERNAME")
            .stdout(Stdio::null())
            .stderr(Stdio::from(std::fs::File::create(&err).unwrap()));
        let hub = Running {
            child: cmd.spawn().unwrap(),
        };
        wait_for("the hub to write hub.json", || {
            config.join("hub.json").is_file()
        });
        for note in [
            "note: the CI watcher does not run: the hub runs for a test",
            "note: the runners are not shown: the hub runs for a test",
        ] {
            wait_for(note, || {
                std::fs::read_to_string(&err).is_ok_and(|t| t.contains(note))
            });
        }
        std::thread::sleep(Duration::from_millis(500));
        drop(hub);
        assert!(
            common::starts_of(&bin.join("nvim")).is_empty(),
            "{setting:?}: a program of the watcher was started"
        );
        let names: Vec<_> = std::fs::read_dir(&vault)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["task.md"], "{setting:?}");
        assert_eq!(
            std::fs::read_to_string(vault.join("task.md")).unwrap(),
            TASK
        );
    }
}

/// `install` and `uninstall` of the program itself, into folders of the test (`HUB_DESKTOP_INSTALL_DIR`, a family folder
/// of its own, `HUB_DESKTOP_NO_LOGIN_SYNC`: no login entry, no link, no PATH of the machine). The install copies the
/// program and makes the command `hub-desktop`, whose starter runs the installed program; the uninstall takes the
/// command away again. Run in a debug build, it is also the check that the commands fit the stack of the main thread.
#[test]
fn install_makes_the_command_and_uninstall_takes_it_away() {
    let scratch = Scratch::new("hub-cli-install");
    let config = scratch.path().join("config");
    let bin = scratch.path().join("bin");
    let family = scratch.path().join("family");
    std::fs::create_dir_all(&config).unwrap();
    let port = free_port();
    let with_folders = |args: &[&str]| {
        command(&config, port)
            .env("HUB_DESKTOP_INSTALL_DIR", &bin)
            .env("HUB_DESKTOP_NO_LOGIN_SYNC", "1")
            .env("APPKIT_FAMILY_DIR", &family)
            // No app of the machine of the person is found: `install` makes the commands of the other apps too
            // (tests/family_cli.rs), and here there are none.
            .env("LOCALAPPDATA", scratch.path().join("no-local"))
            .env("PROGRAMFILES", scratch.path().join("no-program-files"))
            .env("APPDATA", scratch.path().join("no-roaming"))
            .env("USERPROFILE", scratch.path().join("no-home"))
            .args(args)
            .output()
            .unwrap()
    };

    let installed = with_folders(&["install"]);
    assert!(installed.status.success(), "{installed:?}");
    let said = text(&installed);
    assert!(said.contains("command: `hub-desktop`"), "{said}");
    let program = bin.join(if cfg!(windows) {
        "hub-desktop.exe"
    } else {
        "hub-desktop"
    });
    assert!(program.is_file(), "{said}");

    // The starter runs the installed program with the arguments, and hands its output back.
    let starter = family.join("bin").join(if cfg!(windows) {
        "hub-desktop.cmd"
    } else {
        "hub-desktop"
    });
    let version = if cfg!(windows) {
        Command::new("cmd.exe")
            .args(["/d", "/c"])
            .arg(&starter)
            .arg("--version")
            .output()
    } else {
        Command::new("sh").arg(&starter).arg("--version").output()
    }
    .unwrap();
    assert!(version.status.success(), "{version:?}");
    assert_eq!(text(&version).trim(), hub_desktop::version_line());

    let removed = with_folders(&["uninstall"]);
    assert!(removed.status.success(), "{removed:?}");
    assert!(!starter.exists(), "{removed:?}");
}

#[test]
fn completions_print_a_script_for_each_shell_and_refuse_another() {
    let scratch = Scratch::new("hub-cli-completions");
    for (shell, needle) in [
        ("bash", "complete -F"),
        ("powershell", "Register-ArgumentCompleter"),
    ] {
        let out = run(scratch.path(), free_port(), &["completions", shell]);
        assert!(out.status.success(), "{shell}: {out:?}");
        let script = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(script.contains(needle), "{shell}: {script}");
        for word in ["status", "stop", "serve", "install", "hub-desktop"] {
            assert!(script.contains(word), "{shell} lacks {word}: {script}");
        }
    }
    let out = run(scratch.path(), free_port(), &["completions", "zsh"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
}

//! The program itself, run as a person runs it: a double click, a second double click, `serve`
//! beside a hub that runs, `stop`. The config folder is a folder of the test (`HUB_DESKTOP_CONFIG_DIR`),
//! the port a free one, and `HUB_DESKTOP_NO_LAUNCH` prints the pairing link instead of opening a
//! browser: nothing of the machine's own is touched.

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use appkit_scratch::Scratch;

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
fn the_page_and_hello_come_from_the_program_and_open_prints_a_link_that_pairs() {
    let scratch = Scratch::new("hub-cli-page");
    let config = scratch.path();
    let port = free_port();
    let _hub = start_by_click(config, port);

    let link = text(&run(config, port, &["open"]));
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
    ] {
        let out = run(scratch.path(), free_port(), &args);
        assert!(!out.status.success(), "{args:?}: {out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("usage:") || err.contains("--port must be"),
            "{args:?}: {err}"
        );
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

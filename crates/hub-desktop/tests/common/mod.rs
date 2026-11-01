//! What the tests of the hub share: a machine in a folder, fake servers on loopback ports, and the fake
//! program that stands in for the apps the hub starts.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use appkit_core::registry::{self, HubRecord};
use appkit_scratch::Scratch;
use hub_desktop::catalog;
use hub_desktop::config::Env;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The fake program of `crates/fake-app`. Cargo builds the binaries of a package for its own tests only,
/// so the first test that needs it builds it (once, in the target folder that is in use): the tests then
/// do not depend on how they were started.
pub fn fake_app_exe() -> PathBuf {
    static BUILT: std::sync::Once = std::sync::Once::new();
    let exe = std::env::current_exe().unwrap();
    // target/debug/deps/<test>.exe -> target/debug/fake-app.exe
    let profile_dir = exe.parent().unwrap().parent().unwrap().to_path_buf();
    let file = profile_dir.join(format!("fake-app{}", std::env::consts::EXE_SUFFIX));
    BUILT.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut build = std::process::Command::new(cargo);
        build.args(["build", "--locked", "--quiet", "-p", "fake-app"]);
        build.current_dir(env!("CARGO_MANIFEST_DIR"));
        // The same folder and profile as the tests that run: `debug` or `release` by the folder's name.
        if profile_dir.file_name().is_some_and(|n| n == "release") {
            build.arg("--release");
        }
        let status = build.status().expect("cargo runs");
        assert!(status.success(), "cargo build -p fake-app failed");
    });
    assert!(file.is_file(), "{} is missing", file.display());
    file
}

/// The lines the fake program wrote (one JSON document per start), if it ever ran. A line that is
/// still being written is not a start yet: the next look sees it whole.
pub fn starts_of(program: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(program.with_file_name("fake-app.log"))
        .map(|text| {
            text.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// A machine in a folder.
pub struct Machine {
    pub dir: Scratch,
}

impl Machine {
    pub fn new(name: &str) -> Self {
        Self {
            dir: Scratch::new(name),
        }
    }

    pub fn path(&self, below: &str) -> PathBuf {
        self.dir.path().join(below)
    }

    pub fn touch(&self, below: &str) -> PathBuf {
        let file = self.path(below);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"a program").unwrap();
        file
    }

    pub fn env(&self, extra: &[(&str, String)]) -> Env {
        let mut pairs: Vec<(String, String)> = vec![
            (
                "LOCALAPPDATA".into(),
                self.path("Local").display().to_string(),
            ),
            (
                "PROGRAMFILES".into(),
                self.path("Program Files").display().to_string(),
            ),
            (
                "USERPROFILE".into(),
                self.path("Home").display().to_string(),
            ),
            ("APPDATA".into(), self.path("Roaming").display().to_string()),
            (
                "TASKS_DESKTOP_CONFIG_DIR".into(),
                self.path("cfg-tasks").display().to_string(),
            ),
            (
                "RULES_DESKTOP_CONFIG_DIR".into(),
                self.path("cfg-rules").display().to_string(),
            ),
            // Never the port of a real ai-desktop on the machine that runs the test.
            ("AI_DESKTOP_PORT".into(), free_port().to_string()),
        ];
        for (k, v) in extra {
            pairs.retain(|(name, _)| name != k);
            pairs.push(((*k).to_owned(), v.clone()));
        }
        Env::from_pairs(pairs)
    }

    /// The program of tasks: the fake program, so that a start of it leaves a line in its log.
    pub fn tasks_program(&self) -> PathBuf {
        self.program(&format!(
            "Local/tasks-desktop/bin/{}",
            catalog::TASKS.program_file()
        ))
    }

    /// The fake program at `below`.
    pub fn program(&self, below: &str) -> PathBuf {
        let file = self.path(below);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::copy(fake_app_exe(), &file).unwrap();
        file
    }
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A server that answers every request with `status` and `body` and closes.
pub async fn fake_server(status: u16, body: String) -> u16 {
    fake_server_at(0, status, body).await
}

/// The same on `port` (0: a free one).
pub async fn fake_server_at(port: u16, status: u16, body: String) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).await;
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            });
        }
    });
    port
}

/// A server that takes the connection and never says a word.
pub async fn hanging_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    port
}

pub fn write_hub_json(dir: &Path, port: u16, instance: &str) {
    std::fs::create_dir_all(dir).unwrap();
    registry::write_record(
        dir,
        &HubRecord {
            app: "tasks-desktop".to_owned(),
            port,
            pid: 4242,
            version: "0.0.0".to_owned(),
            instance: instance.to_owned(),
        },
    )
    .unwrap();
}

pub fn hello(setup: Option<&str>) -> String {
    serde_json::json!({
        "app": "tasks-desktop",
        "instance": "abc123",
        "setup_required": setup.is_some(),
        "setup_reason": setup,
    })
    .to_string()
}

#[cfg(windows)]
pub fn link(target: &Path, at: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(at.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(unix)]
pub fn link(target: &Path, at: &Path) {
    std::os::unix::fs::symlink(target, at).unwrap();
}

/// The pairing of a page: a code from the control secret, traded for a session token.
pub async fn session(server: &appkit_hub::RunningServer, control: &str) -> String {
    use appkit_testkit::wire::{request, send};
    let host = format!("127.0.0.1:{}", server.addr().port());
    let origin = format!("http://{host}");
    let auth = format!("Bearer {control}");
    let pair = send(
        server.addr(),
        request(
            "POST",
            "/api/control/pair",
            &[
                ("Host", host.as_str()),
                ("Content-Type", "application/json"),
                ("Authorization", auth.as_str()),
                ("Content-Length", "2"),
            ],
            b"{}",
        ),
    )
    .await;
    assert_eq!(pair.status, 200, "{pair:?}");
    let code = pair.json()["code"].as_str().expect("a code").to_owned();
    let body = format!(r#"{{"code":"{code}"}}"#);
    let length = body.len().to_string();
    let exchange = send(
        server.addr(),
        request(
            "POST",
            "/auth/exchange",
            &[
                ("Host", host.as_str()),
                ("Origin", origin.as_str()),
                ("Content-Type", "application/json"),
                ("Content-Length", length.as_str()),
            ],
            body.as_bytes(),
        ),
    )
    .await;
    assert_eq!(exchange.status, 200, "{exchange:?}");
    exchange.json()["token"]
        .as_str()
        .expect("a token")
        .to_owned()
}

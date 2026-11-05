//! The listener of the web variants, asked in raw bytes (a client would repair what is under test: the
//! path, the `Host`, the method).

use std::sync::Arc;
use std::time::Duration;

use appkit_testkit::wire::{request, send, Reply};
use hub_desktop::detect::Detector;
use hub_desktop::overlay::Overlay;
use hub_desktop::webserve::{self, WebServer, CSP_STRICT, MAX_PAGE};

mod common;
use common::*;

const PAGE_POLICY: &str = "default-src 'none'; script-src 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'unsafe-inline'; img-src data: blob:; worker-src blob:; connect-src 'none'; base-uri 'none'; form-action 'none'";

fn page_with(policy: Option<&str>) -> String {
    let meta = policy
        .map(|p| format!(r#"<meta http-equiv="Content-Security-Policy" content="{p}">"#))
        .unwrap_or_default();
    format!("<!doctype html><html><head><meta charset=\"utf-8\">{meta}<title>tasks</title></head><body>the page</body></html>")
}

struct Setup {
    machine: Machine,
    server: WebServer,
}

impl Setup {
    async fn new(name: &str, page: &str) -> Self {
        Self::with_overlay(name, page, Overlay::default()).await
    }

    async fn with_overlay(name: &str, page: &str, overlay: Overlay) -> Self {
        let machine = Machine::new(name);
        let file = machine.path("Web/tasks-web/tasks.html");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, page).unwrap();
        let detector = Arc::new(
            Detector::with_ttl(
                machine.env(&[]),
                Some(machine.path("Web")),
                Duration::from_secs(60),
            )
            .with_overlay(overlay),
        );
        let server = webserve::start(0, detector).await.unwrap();
        Self { machine, server }
    }

    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.server.addr().port())
    }

    async fn ask(&self, method: &str, path: &str) -> Reply {
        let host = self.host();
        send(
            self.server.addr(),
            request(method, path, &[("Host", host.as_str())], b""),
        )
        .await
    }

    async fn get(&self, path: &str) -> Reply {
        self.ask("GET", path).await
    }
}

#[tokio::test]
async fn the_page_of_an_app_of_the_catalog_is_served_with_the_headers_of_its_own_origin() {
    let page = page_with(Some(PAGE_POLICY));
    let setup = Setup::new("hub-web-page", &page).await;
    let reply = setup.get("/tasks-web/tasks.html").await;
    assert_eq!(reply.status, 200, "{reply:?}");
    assert_eq!(reply.body, page);
    assert_eq!(
        reply.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    // The policy of the page, as a header too.
    assert_eq!(reply.header("content-security-policy"), Some(PAGE_POLICY));
    assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(reply.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    assert_eq!(
        reply.header("cross-origin-resource-policy"),
        Some("same-origin")
    );
    assert_eq!(
        reply.header("cross-origin-opener-policy"),
        Some("same-origin")
    );
    // No session, no cookie: this origin has none.
    assert!(reply.header("set-cookie").is_none());
    assert!(reply.header("access-control-allow-origin").is_none());
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_page_with_no_policy_or_a_policy_that_is_not_understood_gets_the_strict_one() {
    for (name, policy) in [
        ("none", None),
        ("wild", Some("default-src *")),
        ("open", Some("default-src 'none'; connect-src *")),
        (
            "host",
            Some("default-src 'none'; connect-src 'none'; script-src https://evil.example"),
        ),
    ] {
        let setup = Setup::new(&format!("hub-web-csp-{name}"), &page_with(policy)).await;
        let reply = setup.get("/tasks-web/tasks.html").await;
        assert_eq!(reply.status, 200);
        assert_eq!(
            reply.header("content-security-policy"),
            Some(CSP_STRICT),
            "{name}"
        );
        setup.server.shutdown().await;
    }
}

#[tokio::test]
async fn head_has_the_headers_and_no_body() {
    let page = page_with(Some(PAGE_POLICY));
    let setup = Setup::new("hub-web-head", &page).await;
    let reply = setup.ask("HEAD", "/tasks-web/tasks.html").await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "");
    assert_eq!(
        reply.header("content-length").map(str::to_owned),
        Some(page.len().to_string())
    );
    setup.server.shutdown().await;
}

/// Anything but the one path of an app of the catalog finds nothing: not a listing, not a file beside the
/// page, not a file of the checkout, and not the same path written another way.
#[tokio::test]
async fn nothing_but_the_exact_path_of_the_page_is_served() {
    let setup = Setup::new("hub-web-paths", &page_with(None)).await;
    // Files that are there, beside the page and elsewhere.
    setup.machine.touch("Web/tasks-web/other.html");
    setup.machine.touch("Web/tasks-web/.git/config");
    setup.machine.touch("Web/secret.txt");
    for path in [
        "/",
        "/index.html",
        "/tasks-web",
        "/tasks-web/",
        "/tasks-web/other.html",
        "/tasks-web/.git/config",
        "/tasks-web/tasks.html/",
        "/tasks-web/tasks.html/x",
        "/secret.txt",
        "/../secret.txt",
        "/tasks-web/../tasks-web/tasks.html",
        "/tasks-web/./tasks.html",
        "//tasks-web/tasks.html",
        "/tasks-web//tasks.html",
        "/tasks-web/%74asks.html",
        "/tasks-web/tasks%2ehtml",
        "/tasks-web%2ftasks.html",
        "/tasks-web/tasks.html%00",
        "/tasks-web/tasks.html%20",
        "/tasks-web/tasks.html.",
        "/TASKS-WEB/tasks.html",
        "/tasks-web/Tasks.html",
        "/tasks-web\\tasks.html",
        "/tasks-web/tasks.html::$DATA",
        "/rules-web/rules.html",
        "/api/hub/hello",
        "/api/hub/apps",
        "/auth/exchange",
        "/api/control/pair",
    ] {
        let reply = setup.get(path).await;
        assert_ne!(reply.status, 200, "{path} was served: {reply:?}");
        assert!(
            !reply.body.contains("the page") && !reply.body.contains("config"),
            "{path}"
        );
    }
    // A query does not make another path of it.
    assert_eq!(setup.get("/tasks-web/tasks.html?x=1").await.status, 200);
    setup.server.shutdown().await;
}

#[tokio::test]
async fn only_get_and_head_and_only_for_our_own_host() {
    let setup = Setup::new("hub-web-methods", &page_with(None)).await;
    for method in [
        "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "TRACE", "CONNECT",
    ] {
        let reply = setup.ask(method, "/tasks-web/tasks.html").await;
        assert_eq!(reply.status, 405, "{method}: {reply:?}");
        assert_eq!(reply.header("allow"), Some("GET, HEAD"));
    }
    let port = setup.server.addr().port();
    let own = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    for host in &own {
        let reply = send(
            setup.server.addr(),
            request(
                "GET",
                "/tasks-web/tasks.html",
                &[("Host", host.as_str())],
                b"",
            ),
        )
        .await;
        assert_eq!(reply.status, 200, "{host}");
    }
    for host in [
        "evil.example",
        "evil.example:80",
        &format!("127.0.0.1:{}", port + 1),
        &format!("127.0.0.1:{port}.evil.example"),
        &format!("[::1]:{port}"),
        "",
    ] {
        let reply = send(
            setup.server.addr(),
            request("GET", "/tasks-web/tasks.html", &[("Host", host)], b""),
        )
        .await;
        assert_eq!(reply.status, 403, "{host:?}");
        assert!(!reply.body.contains("the page"));
    }
    // No Host at all (HTTP/1.1 requires one) and two of them.
    let none = send(
        setup.server.addr(),
        request("GET", "/tasks-web/tasks.html", &[], b""),
    )
    .await;
    assert_ne!(none.status, 200, "{none:?}");
    let host = format!("127.0.0.1:{port}");
    let two = send(
        setup.server.addr(),
        request(
            "GET",
            "/tasks-web/tasks.html",
            &[("Host", host.as_str()), ("Host", host.as_str())],
            b"",
        ),
    )
    .await;
    assert_ne!(two.status, 200, "{two:?}");
    setup.server.shutdown().await;
}

/// A checkout whose folder is a link to somewhere else: what is behind it is not below the web root.
#[tokio::test]
async fn a_page_behind_a_link_out_of_the_web_root_is_not_served() {
    let machine = Machine::new("hub-web-link");
    machine.touch("Outside/tasks-web/tasks.html");
    std::fs::create_dir_all(machine.path("Web")).unwrap();
    link(
        &machine.path("Outside/tasks-web"),
        &machine.path("Web/tasks-web"),
    );
    let detector = Arc::new(Detector::with_ttl(
        machine.env(&[]),
        Some(machine.path("Web")),
        Duration::from_secs(60),
    ));
    let server = webserve::start(0, detector).await.unwrap();
    let host = format!("127.0.0.1:{}", server.addr().port());
    let reply = send(
        server.addr(),
        request(
            "GET",
            "/tasks-web/tasks.html",
            &[("Host", host.as_str())],
            b"",
        ),
    )
    .await;
    assert_eq!(reply.status, 404, "{reply:?}");
    server.shutdown().await;
}

#[tokio::test]
async fn the_file_is_looked_at_for_every_request_and_a_page_over_the_limit_is_not_served() {
    let setup = Setup::new("hub-web-fresh", &page_with(None)).await;
    assert_eq!(setup.get("/tasks-web/tasks.html").await.status, 200);

    // Larger than a page may be: refused without being read (a file that is only a length).
    let file = setup.machine.path("Web/tasks-web/tasks.html");
    let big = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
    big.set_len(MAX_PAGE + 1).unwrap();
    drop(big);
    assert_eq!(setup.get("/tasks-web/tasks.html").await.status, 404);

    // Gone: not served from a look that is older than the request, although the cache of the detector is
    // set to a minute.
    std::fs::remove_file(&file).unwrap();
    assert_eq!(setup.get("/tasks-web/tasks.html").await.status, 404);
    setup.server.shutdown().await;
}

/// The listener has no part of the hub: nothing under `/api/` answers, nothing sets a cookie, and the
/// port is a port of its own.
#[tokio::test]
async fn this_origin_has_no_api_no_cookie_and_no_pairing() {
    let setup = Setup::new("hub-web-noapi", &page_with(None)).await;
    for (method, path) in [
        ("GET", "/api/hub/hello"),
        ("GET", "/api/hub/apps"),
        ("POST", "/api/hub/apps/tasks/launch"),
        ("POST", "/auth/exchange"),
        ("POST", "/api/control/pair"),
        ("GET", "/api/vaults"),
    ] {
        let reply = setup.ask(method, path).await;
        assert!(
            reply.status == 404 || reply.status == 405,
            "{path}: {reply:?}"
        );
        assert!(reply.header("set-cookie").is_none());
    }
    setup.server.shutdown().await;
}

#[tokio::test]
async fn it_listens_on_loopback_only_and_stops_when_it_is_told() {
    let setup = Setup::new("hub-web-bind", &page_with(None)).await;
    assert!(setup.server.addr().ip().is_loopback());
    let addr = setup.server.addr();
    setup.server.shutdown().await;
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
}

/// The cache of the detector may hold a page for a minute; what is served is looked at again for each request.
/// A checkout folder that is turned into a link to somewhere else after the first request must not go on
/// serving what is behind the link.
#[tokio::test]
async fn a_folder_that_becomes_a_link_out_of_the_root_after_the_first_request_is_not_served() {
    let setup = Setup::new("hub-web-swap", &page_with(None)).await;
    assert_eq!(setup.get("/tasks-web/tasks.html").await.status, 200);

    setup.machine.touch("Outside/tasks-web/tasks.html");
    std::fs::remove_dir_all(setup.machine.path("Web/tasks-web")).unwrap();
    link(
        &setup.machine.path("Outside/tasks-web"),
        &setup.machine.path("Web/tasks-web"),
    );
    let reply = setup.get("/tasks-web/tasks.html").await;
    assert_eq!(reply.status, 404, "{reply:?}");
    setup.server.shutdown().await;
}

#[tokio::test]
async fn the_page_of_an_app_the_overlay_hides_is_not_served() {
    let page = page_with(Some(PAGE_POLICY));
    let overlay = Overlay::parse(r#"{"schema":"hub-catalog/1","hidden":["tasks"]}"#).unwrap();
    let setup = Setup::with_overlay("hub-web-hidden", &page, overlay).await;
    assert_eq!(setup.get("/tasks-web/tasks.html").await.status, 404);
    setup.server.shutdown().await;
}

//! The listener of the web variants (concept 6.3): a second port of this machine that serves the page of
//! an app of the catalog from its checkout, and nothing else.
//!
//! It is not part of the hub. It has its own origin, so that a flaw in a page that renders text of a
//! vault cannot reach the session of the hub: there is no API here, no cookie, no pairing, and no
//! route that takes anything but a plain `GET` or `HEAD` for the page of an app of the catalog.
//!
//! What is served is chosen by the catalog and the detection, never by the request: the request path has to
//! be exactly `/<repo_dir>/<entry>` of an app whose page the detection found below its root, and the
//! file is the one the detection resolved (links followed, root checked). A path that is encoded, has a
//! dot part, a second slash or a query is a path of nobody and gets a 404.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::Response;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::catalog::CATALOG;
use crate::detect::Detector;

/// The policy of a page without WASM and without a worker (`CSP_STRICT` of `appkit/web-kit`): the one a
/// page gets when it carries none that this listener accepts.
pub const CSP_STRICT: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; connect-src 'none'; base-uri 'none'; form-action 'none'";

/// The most of a page that is served.
pub const MAX_PAGE: u64 = 32 * 1024 * 1024;

/// How far into the file the policy of its meta tag is looked for.
const HEAD_BYTES: usize = 4096;

/// The listener, started.
pub struct WebServer {
    addr: SocketAddr,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl WebServer {
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

/// Listen on `127.0.0.1:<port>` (0: a free one).
///
/// # Errors
///
/// An I/O error of binding (the port is taken).
pub async fn start(port: u16, detector: Arc<Detector>) -> std::io::Result<WebServer> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let addr = listener.local_addr()?;
    let app = Router::new().fallback(page).with_state(Shared {
        detector,
        port: addr.port(),
    });
    let (stop, mut stopped) = watch::channel(false);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.wait_for(|s| *s).await;
            })
            .await;
    });
    Ok(WebServer { addr, stop, task })
}

#[derive(Clone)]
struct Shared {
    detector: Arc<Detector>,
    port: u16,
}

/// The `Host` that is ours: the loopback names with the port of this listener (a page that was asked for
/// under another name is a DNS rebinding).
fn host_is_ours(headers: &HeaderMap, port: u16) -> bool {
    let mut hosts = headers.get_all(header::HOST).iter();
    let (Some(host), None) = (hosts.next(), hosts.next()) else {
        return false;
    };
    let Ok(host) = host.to_str() else {
        return false;
    };
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

/// The policy of the file, if its meta tag holds one of the shape the kit writes: directives of plain
/// words with `default-src 'none'` and `connect-src 'none'`. Anything else is not echoed into a header.
fn policy_of(head: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(head);
    let lower = text.to_ascii_lowercase();
    let at = lower.find("http-equiv=\"content-security-policy\"")?;
    let tag_end = lower[at..].find('>')? + at;
    let tag = &text[at..tag_end];
    let value_at = tag.to_ascii_lowercase().find("content=\"")? + "content=\"".len();
    let value = &tag[value_at..];
    let policy = &value[..value.find('"')?];
    let plain = policy.bytes().all(|b| {
        b.is_ascii_lowercase()
            || b.is_ascii_digit()
            || matches!(b, b' ' | b';' | b'\'' | b'-' | b':')
    });
    let directives: Vec<&str> = policy.split("; ").collect();
    (plain
        && policy.len() <= 512
        && directives.contains(&"default-src 'none'")
        && directives.contains(&"connect-src 'none'"))
    .then(|| policy.to_owned())
}

fn refuse(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(
            status.canonical_reason().unwrap_or("").to_owned(),
        ))
        .unwrap_or_default()
}

/// The headers every answer has.
fn secure(mut response: Response) -> Response {
    let headers = response.headers_mut();
    for (name, value) in [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::CACHE_CONTROL, "no-store"),
        (
            HeaderName::from_static("cross-origin-resource-policy"),
            "same-origin",
        ),
        (
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        ),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

async fn page(
    State(shared): State<Shared>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    secure(answer(&shared, &method, &headers, &uri).await)
}

async fn answer(shared: &Shared, method: &Method, headers: &HeaderMap, uri: &Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        let mut response = refuse(StatusCode::METHOD_NOT_ALLOWED);
        response
            .headers_mut()
            .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }
    if !host_is_ours(headers, shared.port) {
        return refuse(StatusCode::FORBIDDEN);
    }
    let path = uri.path();
    // The page of an app of the catalog, by exactly this path.
    let Some(entry) = CATALOG.iter().find(|e| {
        !shared.detector.hides(e.id)
            && e.web
                .is_some_and(|w| path == format!("/{}/{}", w.repo_dir, w.entry))
    }) else {
        return refuse(StatusCode::NOT_FOUND);
    };
    // The file is looked for now, not taken from a look of ten seconds ago.
    let state = shared.detector.look_one(entry).await;
    let Some(file) = state.web else {
        return refuse(StatusCode::NOT_FOUND);
    };
    let head_only = *method == Method::HEAD;
    let read = tokio::task::spawn_blocking(move || read_page(&file)).await;
    let Ok(Some(bytes)) = read else {
        return refuse(StatusCode::NOT_FOUND);
    };
    let policy =
        policy_of(&bytes[..bytes.len().min(HEAD_BYTES)]).unwrap_or_else(|| CSP_STRICT.to_owned());
    let mut response = Response::new(if head_only {
        Body::empty()
    } else {
        Body::from(bytes.clone())
    });
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));
    if let Ok(value) = HeaderValue::from_str(&policy) {
        headers.insert(header::CONTENT_SECURITY_POLICY, value);
    }
    response
}

/// The file, if it is a file that is not larger than a page may be.
fn read_page(file: &std::path::Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(file).ok()?;
    if !meta.is_file() || meta.len() > MAX_PAGE {
        return None;
    }
    std::fs::read(file).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(content: &str) -> Option<String> {
        policy_of(
            format!(r#"<meta http-equiv="Content-Security-Policy" content="{content}"><title>x"#)
                .as_bytes(),
        )
    }

    #[test]
    fn the_policy_of_the_kit_is_taken_from_the_page() {
        let kit = "default-src 'none'; script-src 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'unsafe-inline'; img-src data: blob:; worker-src blob:; connect-src 'none'; base-uri 'none'; form-action 'none'";
        assert_eq!(policy(kit).as_deref(), Some(kit));
        assert_eq!(policy(CSP_STRICT).as_deref(), Some(CSP_STRICT));
    }

    /// A page that carries another policy, none, or one that is not plain gets the strict one: the header is
    /// never text of the file that the listener has not understood.
    #[test]
    fn a_policy_that_is_not_the_shape_of_the_kit_is_not_taken() {
        for bad in [
            "default-src *",
            "default-src 'none'; connect-src *",
            "default-src 'none'",
            "connect-src 'none'",
            "default-src 'none'; connect-src 'none'\r\nX-Evil: 1",
            "default-src 'none'; connect-src 'none'; script-src https://evil.example",
        ] {
            assert_eq!(policy(bad), None, "{bad}");
        }
        assert_eq!(policy_of(b"<html>no meta at all"), None);
        assert_eq!(
            policy_of(b"<meta http-equiv=\"Content-Security-Policy\" content=\"unterminated"),
            None
        );
    }

    #[test]
    fn the_host_is_a_loopback_name_with_our_port_and_only_one() {
        let with = |hosts: &[&str]| {
            let mut headers = HeaderMap::new();
            for h in hosts {
                headers.append(header::HOST, HeaderValue::from_str(h).unwrap());
            }
            host_is_ours(&headers, 17611)
        };
        assert!(with(&["127.0.0.1:17611"]));
        assert!(with(&["localhost:17611"]));
        for bad in [
            &[][..],
            &["127.0.0.1"],
            &["127.0.0.1:17610"],
            &["evil.example:17611"],
            &["127.0.0.1:17611.evil.example"],
            &["127.0.0.1:17611", "127.0.0.1:17611"],
            &["LOCALHOST:17611"],
        ] {
            assert!(!with(bad), "{bad:?}");
        }
    }
}

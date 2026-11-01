//! A look at a port of this machine: is something there, and does it answer like the app (concept 5.2).
//!
//! The client of `appkit-hub` waits five seconds and takes 64 KiB, which would make "the page is there in
//! a second" false as soon as one port accepts and says nothing. This one has a budget for the whole
//! exchange, reads a few kilobytes, goes to `127.0.0.1` only, and trusts nothing it reads: an answer is
//! text to compare, never something to act on.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;

/// The time for connecting, asking and reading the answer together.
pub const BUDGET: Duration = Duration::from_millis(300);

/// The most of a body that is read; a larger answer is not the answer of an app of ours.
pub const MAX_BODY: usize = 16 * 1024;

/// The most of the head (status line and headers) that is read.
const MAX_HEAD: usize = 8 * 1024;

/// What is on a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// Nobody listens (refused, or no connection within the budget).
    Nothing,
    /// Somebody listens and sent a complete HTTP answer.
    Answer { status: u16, body: String },
    /// Somebody listens and the answer is none we use: silence until the budget ran out, something
    /// that is not HTTP, a head or a body that is too large, a chunked body.
    Other,
}

/// The ports a probe may go to: the one of the catalog, or one a hub names in its `hub.json` (the
/// range of a program that is not a system service).
#[must_use]
pub fn allowed_port(port: u16, catalog_port: Option<u16>) -> bool {
    Some(port) == catalog_port || port >= 1024
}

/// `GET <path>` on `127.0.0.1:<port>`.
pub async fn get(port: u16, path: &str) -> Probe {
    let end = Instant::now() + BUDGET;
    let stream = match tokio::time::timeout_at(end, TcpStream::connect(("127.0.0.1", port))).await {
        Ok(Ok(stream)) => stream,
        _ => return Probe::Nothing,
    };
    match tokio::time::timeout_at(end, exchange(stream, port, path)).await {
        Ok(Some(answer)) => answer,
        _ => Probe::Other,
    }
}

async fn exchange(mut stream: TcpStream, port: u16, path: &str) -> Option<Probe> {
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.ok()?;
    let mut raw: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
        // Nothing is read beyond what could be a head and a body of the sizes we take.
        if raw.len() > MAX_HEAD + MAX_BODY {
            return None;
        }
    }
    parse(&raw)
}

/// An HTTP/1.x answer that was read to its end. `None` for anything else.
fn parse(raw: &[u8]) -> Option<Probe> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    if split > MAX_HEAD {
        return None;
    }
    let head = std::str::from_utf8(&raw[..split]).ok()?;
    let body = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status_line = lines.next()?;
    let status = status_line
        .strip_prefix("HTTP/1.1 ")
        .or_else(|| status_line.strip_prefix("HTTP/1.0 "))?
        .split(' ')
        .next()?
        .parse::<u16>()
        .ok()
        .filter(|s| (100..600).contains(s))?;
    for line in lines {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("transfer-encoding")
            && !value.trim().eq_ignore_ascii_case("identity")
        {
            return None;
        }
    }
    if body.len() > MAX_BODY {
        return None;
    }
    Some(Probe::Answer {
        status,
        body: String::from_utf8_lossy(body).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;

    /// A server that answers every connection with `reply` (read first, so the client's write lands) and
    /// closes. Returns its port.
    async fn serve_once(reply: &'static [u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let _ = stream.write_all(reply).await;
                });
            }
        });
        port
    }

    #[tokio::test]
    async fn a_port_nobody_listens_on_is_nothing() {
        let free = {
            let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        assert_eq!(get(free, "/x").await, Probe::Nothing);
    }

    #[tokio::test]
    async fn a_complete_answer_is_read_with_its_status_and_body() {
        let port = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").await;
        assert_eq!(
            get(port, "/x").await,
            Probe::Answer {
                status: 200,
                body: "{}".to_owned()
            }
        );
        let port = serve_once(b"HTTP/1.0 404 Not Found\r\n\r\nno").await;
        assert_eq!(
            get(port, "/x").await,
            Probe::Answer {
                status: 404,
                body: "no".to_owned()
            }
        );
    }

    /// The case the budget is for: a port that takes the connection and says nothing. The probe is over
    /// at the budget, not after the five seconds of a client with a patience of its own.
    #[tokio::test]
    async fn a_server_that_accepts_and_hangs_costs_the_budget_and_no_more() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let _held = tokio::spawn(async move {
            let mut streams = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                streams.push(stream);
            }
        });
        let start = Instant::now();
        assert_eq!(get(port, "/x").await, Probe::Other);
        let took = start.elapsed();
        assert!(took >= BUDGET - Duration::from_millis(20), "{took:?}");
        assert!(took < Duration::from_millis(900), "{took:?}");
    }

    #[tokio::test]
    async fn something_that_is_not_http_or_not_ours_to_read_is_other() {
        for reply in [
            &b"SSH-2.0-OpenSSH_9\r\n"[..],
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n",
            b"HTTP/1.1 999 Odd\r\n\r\n",
            b"HTTP/2 200\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nbroken header\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n",
        ] {
            let port = serve_once(reply).await;
            assert_eq!(
                get(port, "/x").await,
                Probe::Other,
                "{}",
                String::from_utf8_lossy(reply)
            );
        }
    }

    #[tokio::test]
    async fn an_answer_over_the_limit_is_other_and_does_not_fill_memory() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
                    // More than the probe takes, written until it hangs up.
                    let block = [b'a'; 4096];
                    while stream.write_all(&block).await.is_ok() {}
                });
            }
        });
        assert_eq!(get(port, "/x").await, Probe::Other);
    }

    #[test]
    fn a_hub_may_name_any_port_from_1024_and_the_catalog_its_own() {
        assert!(allowed_port(17615, Some(17615)));
        assert!(allowed_port(1024, None));
        assert!(allowed_port(65535, None));
        assert!(!allowed_port(1023, None));
        assert!(!allowed_port(80, None));
        // The catalog may name a low port (a program that serves there by design).
        assert!(allowed_port(80, Some(80)));
    }
}

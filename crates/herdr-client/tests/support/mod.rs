//! `MockHerdr` — the offline stand-in for the herdr session daemon.
//!
//! A REAL `tokio::net::UnixListener` bound inside a `tempfile::TempDir`, so the code under test
//! exercises the same syscalls it will use in production. It answers **one request per connection
//! and then closes**, mirroring the real server (verified live: a second write on an answered
//! connection is a `BrokenPipe`).
//!
//! It records the raw request bytes and the connection count, which is what makes the two wire
//! invariants assertable: the trailing newline (herdr hangs forever without it) and the
//! never-reuse-a-connection rule.
//!
//! Nothing here reads `$HOME`, `$HERDR_SOCKET_PATH`, or
//! `~/.config/herdr/herdr.sock`: the whole offline suite is structurally incapable of
//! touching the operator's live herd, which is what makes the crate gateable on thev-box (D6).

// Each test target compiles the whole module and uses a different slice of it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::task::JoinHandle;

/// How long the mock waits for a request line before giving up on a connection.
///
/// Bounded on purpose: if the client ever stopped terminating its request with a newline, the real
/// server would hang forever. The mock must fail the assertion instead of hanging the suite.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// How the mock answers ONE connection.
#[derive(Clone, Debug)]
pub enum Reply {
    /// Write each string as its own newline-terminated line, then close.
    Lines(Vec<String>),
    /// Read the request, then close without writing anything (the `ClosedEarly` path).
    CloseAfterRequest,
    /// Read the request and hold the connection open forever without answering. This is the shape
    /// a request with no trailing newline takes against the real server.
    Silent,
}

impl Reply {
    /// One line, then close — the ordinary RPC answer.
    pub fn line(s: impl Into<String>) -> Reply {
        Reply::Lines(vec![s.into()])
    }
}

/// A one-request-per-connection herdr stand-in on a temporary Unix socket.
pub struct MockHerdr {
    // Dropping the TempDir unlinks the socket; kept alive for the mock's lifetime.
    _dir: TempDir,
    path: PathBuf,
    connections: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    accept_loop: JoinHandle<()>,
}

impl MockHerdr {
    /// Answers every connection with the same `reply`, forever.
    pub async fn always(reply: Reply) -> MockHerdr {
        MockHerdr::spawn(vec![reply], true)
    }

    /// Answers connection *n* with `replies[n]`; once the list is exhausted every further
    /// connection is closed after its request is read.
    pub async fn sequence(replies: Vec<Reply>) -> MockHerdr {
        MockHerdr::spawn(replies, false)
    }

    fn spawn(replies: Vec<Reply>, repeat_last: bool) -> MockHerdr {
        let dir = tempfile::Builder::new()
            .prefix("herdr-mock-")
            .tempdir()
            .expect("tempdir for the mock socket");
        // Kept short: a Unix socket path is capped at ~108 bytes by the kernel.
        let path = dir.path().join("herdr.sock");

        let listener = UnixListener::bind(&path)
            .unwrap_or_else(|e| panic!("bind mock socket at {}: {e}", path.display()));

        let connections = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));

        let accept_loop = tokio::spawn({
            let connections = Arc::clone(&connections);
            let requests = Arc::clone(&requests);
            async move {
                let mut n = 0usize;
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        return;
                    };
                    connections.fetch_add(1, Ordering::SeqCst);

                    let reply = match replies.get(n) {
                        Some(r) => r.clone(),
                        None if repeat_last => {
                            replies.last().cloned().unwrap_or(Reply::CloseAfterRequest)
                        }
                        None => Reply::CloseAfterRequest,
                    };
                    n += 1;

                    // One task per connection so a `Silent` hold does not wedge the accept loop.
                    tokio::spawn(serve_one(stream, reply, Arc::clone(&requests)));
                }
            }
        });

        MockHerdr {
            _dir: dir,
            path,
            connections,
            requests,
            accept_loop,
        }
    }

    /// The socket to point a client at.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many times a client has DIALED. Zero is the assertion that proves a client-side guard
    /// fired before the connection was made.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    /// Every request line the mock has read, raw, INCLUDING the trailing newline if one was sent.
    /// Recorded before the reply is written, so it is always populated by the time a client's call
    /// returns.
    pub fn requests(&self) -> Vec<Vec<u8>> {
        self.requests.lock().expect("mock request log").clone()
    }

    /// The most recent request line, raw. Panics if nothing has been received.
    pub fn last_request(&self) -> Vec<u8> {
        self.requests()
            .pop()
            .expect("no request reached the mock socket")
    }

    /// The most recent request parsed as JSON, newline stripped.
    pub fn last_request_json(&self) -> serde_json::Value {
        let raw = self.last_request();
        let line = std::str::from_utf8(&raw).expect("request is UTF-8");
        serde_json::from_str(line.trim_end_matches(['\r', '\n'])).expect("request is JSON")
    }
}

impl Drop for MockHerdr {
    fn drop(&mut self) {
        self.accept_loop.abort();
    }
}

async fn serve_one(
    mut stream: tokio::net::UnixStream,
    reply: Reply,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
) {
    let mut buf = Vec::new();
    {
        let (read_half, _write_half) = stream.split();
        let mut reader = BufReader::new(read_half);
        // A timeout, not a hang: see REQUEST_READ_TIMEOUT.
        let _ =
            tokio::time::timeout(REQUEST_READ_TIMEOUT, reader.read_until(b'\n', &mut buf)).await;
    }
    requests.lock().expect("mock request log").push(buf);

    match reply {
        // Dropping `stream` at the end of this fn is the close.
        Reply::CloseAfterRequest => {}
        Reply::Silent => {
            // Hold the connection open, answering nothing. The task is dropped with the test's
            // runtime; nothing here outlives the test.
            std::future::pending::<()>().await;
        }
        Reply::Lines(lines) => {
            for line in lines {
                if stream.write_all(line.as_bytes()).await.is_err() {
                    return;
                }
                if stream.write_all(b"\n").await.is_err() {
                    return;
                }
            }
            let _ = stream.flush().await;
        }
    }
}

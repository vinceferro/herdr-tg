//! The wire. Crate-private on purpose: every request the bridge can send goes through this module,
//! and this module appends the newline itself.
//!
//! Two entry points, one framing rule:
//!
//! * [`round_trip`] — the one-shot RPC. Dial, write, read exactly one reply line, drop.
//! * [`open_stream`] — `events.subscribe`. Dial, write, read exactly one ACK line, and hand back
//!   the still-open reader so the frames that follow can be consumed.
//!
//! Both go through [`dial_and_send`], which is the ONLY place in this crate that appends a `\n`.
//!
//! ## Why the newline is a type-level invariant, not a doc comment
//!
//! Omitting the trailing newline makes herdr hang FOREVER with no error and no close — verified
//! live: 5.01 s elapsed, zero bytes read, connection still open. Only a client-side timeout catches
//! it, which for a phone-only operator is the worst failure mode in the product. So there is no
//! public path that can send an unterminated line.
//!
//! ## Why this is a LINE reader and never `read_to_end`
//!
//! herdr writes one reply line and then RESETS the connection. Reading to EOF surfaces ECONNRESET
//! *after* a perfectly good reply has already arrived — observed during fixture capture as
//! `ConnectionResetError [Errno 104]` with zero bytes recovered. Read exactly one line and stop.

use std::io;
use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines, Take};
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::error::HerdrError;
use crate::{MAX_REQUEST_BODY_BYTES, MAX_RESPONSE_BYTES};

/// The reader an open event stream is consumed through.
///
/// `tokio::io::Lines::next_line` is documented **cancel-safe** in tokio 1.53.1 (its partial state
/// lives in the `Lines` struct, not in the future), while `AsyncBufReadExt::read_line` explicitly
/// is NOT — "data may have been partially read, and this data is lost". Slice 3 will
/// `tokio::select!` this stream against teloxide's long-poll, where a lost partial line is a lost
/// ask, so the reader type is a correctness decision and not a style one.
///
/// `.take(MAX_RESPONSE_BYTES)` is what stops one pathological unterminated line from OOM-ing a
/// bridge that has to run unattended. Note that `Take` counts for the LIFETIME of the reader, so
/// the ceiling set here would otherwise bound the whole stream rather than one frame:
/// [`crate::stream::EventStream`] restores the limit after every complete line, which is what makes
/// the ceiling mean what `MAX_RESPONSE_BYTES` says it means. Do not remove that restore — the
/// failure it prevents is a stream that reports a disconnect herdr never performed, and the
/// reasoning plus the two tests that pin it are in the `stream` module docs.
pub(crate) type EventLines = Lines<BufReader<Take<UnixStream>>>;

/// Dial, write `body` + `\n`, and hand back the connected stream.
///
/// **THE ONLY WRITER.** The newline is appended here and nowhere else in the crate.
async fn dial_and_send(
    socket_path: &Path,
    method: &'static str,
    body: &[u8],
    connect_timeout: Duration,
) -> Result<UnixStream, HerdrError> {
    // A raw newline inside the body would split one request into two lines on the wire. Compact
    // `serde_json` output escapes newlines inside strings, so this can only fire on a hand-built
    // body, i.e. a bug in this crate.
    debug_assert!(
        !body.contains(&b'\n'),
        "{method}: request body contains a raw newline; it would frame as two requests"
    );

    // Rejected BEFORE dialing: an oversize body must not cost the operator a connection.
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return Err(HerdrError::RequestTooLarge {
            method,
            len: body.len(),
            max: MAX_REQUEST_BODY_BYTES,
        });
    }

    let mut stream = match timeout(connect_timeout, UnixStream::connect(socket_path)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(source)) => {
            return Err(HerdrError::Connect {
                path: socket_path.to_path_buf(),
                source,
            });
        }
        // A dial that times out is still "herdr unreachable" to the operator, so it maps to the
        // same variant, the same predicate and the same exit code 3.
        Err(_) => {
            return Err(HerdrError::Connect {
                path: socket_path.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("connect timed out after {connect_timeout:?}"),
                ),
            });
        }
    };

    tracing::trace!(method, socket = %socket_path.display(), len = body.len(), "herdr rpc");

    // THE INVARIANT. One buffer, one write, newline appended here and nowhere else.
    let mut framed = Vec::with_capacity(body.len() + 1);
    framed.extend_from_slice(body);
    framed.push(b'\n');

    if let Err(source) = stream.write_all(&framed).await {
        return Err(HerdrError::Io { method, source });
    }
    if let Err(source) = stream.flush().await {
        return Err(HerdrError::Io { method, source });
    }

    Ok(stream)
}

/// Dial, write `body` + `\n`, read exactly one reply line, drop the connection.
///
/// RPC is strictly one-shot — the CONNECTION is the correlation (a second write on an answered
/// connection is a `BrokenPipe`, verified live), so there is no pool and no id-correlation map.
///
/// `body` is the JSON request body with the newline EXCLUDED; [`dial_and_send`] frames it.
pub(crate) async fn round_trip(
    socket_path: &Path,
    method: &'static str,
    body: &[u8],
    connect_timeout: Duration,
    request_timeout: Duration,
) -> Result<String, HerdrError> {
    let mut stream = dial_and_send(socket_path, method, body, connect_timeout).await?;

    // `.take(MAX_RESPONSE_BYTES)` so a pathological reply cannot OOM the bridge; the cap yields EOF
    // rather than an error, which then surfaces downstream as a decode failure naming the line.
    let read = timeout(request_timeout, async {
        let mut line = String::new();
        let n = BufReader::new((&mut stream).take(MAX_RESPONSE_BYTES))
            .read_line(&mut line)
            .await?;
        Ok::<(usize, String), io::Error>((n, line))
    })
    .await;

    match read {
        // A 0-byte read is the server closing before it answered — NOT an empty-string parse error.
        Ok(Ok((0, _))) => Err(HerdrError::ClosedEarly { method }),
        Ok(Ok((_, line))) => Ok(line.trim_end_matches(['\r', '\n']).to_owned()),
        Ok(Err(source)) => Err(HerdrError::Io { method, source }),
        Err(_) => Err(HerdrError::Timeout {
            method,
            elapsed: request_timeout,
        }),
    }
}

/// Dial, write `body` + `\n`, read exactly ONE line — the subscribe ack — and hand back the ack
/// plus the still-open reader.
///
/// This is `events.subscribe`, the one method whose connection outlives its reply. [`round_trip`]
/// cannot serve it: it reads one line and drops the socket.
///
/// **The ack and every later frame share ONE `BufReader`, and that is load-bearing.** herdr can
/// (and, when a filtered subscription replays at t=0.00, does) put the ack and the first frames in
/// the same TCP-ish read, so a second reader built after the ack would silently discard whatever
/// the first had already buffered — losing exactly the replayed ask that proof gate 5 depends on.
///
/// The ack read is bounded by `request_timeout`. Nothing after it is: there is **no heartbeat** on
/// this stream (>9 s of silence observed on a healthy one after the backlog drained), so a read
/// timeout on the frames would be a liveness *misreport*. Liveness is probed out-of-band with
/// `is_alive()` on a fresh connection.
pub(crate) async fn open_stream(
    socket_path: &Path,
    method: &'static str,
    body: &[u8],
    connect_timeout: Duration,
    request_timeout: Duration,
) -> Result<(String, EventLines), HerdrError> {
    let stream = dial_and_send(socket_path, method, body, connect_timeout).await?;
    let mut lines = BufReader::new(stream.take(MAX_RESPONSE_BYTES)).lines();

    match timeout(request_timeout, lines.next_line()).await {
        Ok(Ok(Some(ack))) => Ok((ack, lines)),
        // Accepted the subscribe and closed without acking.
        Ok(Ok(None)) => Err(HerdrError::ClosedEarly { method }),
        Ok(Err(source)) => Err(HerdrError::Io { method, source }),
        Err(_) => Err(HerdrError::Timeout {
            method,
            elapsed: request_timeout,
        }),
    }
}
// ───────────────────────────────────────────────────────────────────────────────────────────────
// The offline suite for the wire layer.
//
// These are rows of the spec's `tests/wire.rs` and `tests/failure_paths.rs` tables. They live HERE
// rather than in `tests/` because the spec's API section makes `transport` crate-private
// (`mod transport;`), and an integration test cannot reach a `pub(crate)` fn. Widening the public
// API to place a test would have created exactly the "public path that can send an unterminated
// line" this module exists to forbid. The mock is still the spec's `tests/support::MockHerdr`,
// wired in from `lib.rs` under `#[cfg(test)]`. See the row table in `tests/wire.rs`.
//
// NOTHING here touches ~/.config/herdr/herdr.sock: every path is a `tempfile::TempDir` or
// a deliberately absent one.
// ───────────────────────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::support::{MockHerdr, Reply};

    const PONG: &str = r#"{"id":"","result":{"type":"pong","version":"0.8.2","protocol":20}}"#;
    const PING_BODY: &[u8] = br#"{"id":"probe","method":"ping","params":{}}"#;
    const FAST: Duration = Duration::from_millis(2000);

    /// `tests/wire.rs` row 1 — *the request is always newline-terminated*.
    #[tokio::test]
    async fn request_is_always_newline_terminated() {
        let mock = MockHerdr::always(Reply::line(PONG)).await;

        let reply = round_trip(mock.path(), "ping", PING_BODY, FAST, FAST)
            .await
            .expect("mock answers");
        assert_eq!(reply, PONG);

        let raw = mock.last_request();
        assert_eq!(
            raw.last().copied(),
            Some(b'\n'),
            "the last byte on the wire MUST be 0x0A; without it herdr hangs forever with no error \
             and no close, and only the request timeout would ever notice"
        );
        assert_eq!(
            &raw[..raw.len() - 1],
            PING_BODY,
            "body must be sent verbatim"
        );
        assert_eq!(
            raw.iter().filter(|b| **b == b'\n').count(),
            1,
            "exactly one newline: a second would frame as a second request"
        );
    }

    /// `tests/wire.rs` row 4 — *the connection is never reused*.
    #[tokio::test]
    async fn connection_is_not_reused() {
        // The mock answers ONE request per connection and then closes, exactly like herdr.
        let mock = MockHerdr::always(Reply::line(PONG)).await;

        for _ in 0..2 {
            assert_eq!(
                round_trip(mock.path(), "ping", PING_BODY, FAST, FAST)
                    .await
                    .expect("each call dials fresh"),
                PONG
            );
        }

        assert_eq!(
            mock.connections(),
            2,
            "two calls must be two dials; a client that pooled would fail here exactly as it fails live"
        );
        assert_eq!(mock.requests().len(), 2);
    }

    /// `tests/wire.rs` row 5 — *an oversize request is rejected client-side, WITHOUT dialing*.
    #[tokio::test]
    async fn oversize_request_is_rejected_client_side() {
        let mock = MockHerdr::always(Reply::line(PONG)).await;

        let too_big = vec![b'x'; MAX_REQUEST_BODY_BYTES + 1];
        let err = round_trip(mock.path(), "pane.send_text", &too_big, FAST, FAST)
            .await
            .expect_err("over the cap");
        match err {
            HerdrError::RequestTooLarge { method, len, max } => {
                assert_eq!(method, "pane.send_text");
                assert_eq!(len, MAX_REQUEST_BODY_BYTES + 1);
                assert_eq!(max, MAX_REQUEST_BODY_BYTES);
            }
            other => panic!("expected RequestTooLarge, got {other:?}"),
        }
        assert_eq!(
            mock.connections(),
            0,
            "the guard must fire BEFORE the dial: zero connections"
        );

        // Exactly at the cap it dials and completes.
        let at_cap = vec![b'x'; MAX_REQUEST_BODY_BYTES];
        assert_eq!(
            round_trip(mock.path(), "pane.send_text", &at_cap, FAST, FAST)
                .await
                .expect("exactly at the cap is allowed"),
            PONG
        );
        assert_eq!(mock.connections(), 1);
        assert_eq!(mock.last_request().len(), MAX_REQUEST_BODY_BYTES + 1);
    }

    /// `tests/failure_paths.rs` row 1 — *a missing socket is a typed error, not a panic*.
    #[tokio::test]
    async fn missing_socket_is_a_typed_error_not_a_panic() {
        let connect_timeout = Duration::from_secs(2);
        let started = Instant::now();
        let err = round_trip(
            Path::new("/nonexistent/herdr.sock"),
            "ping",
            PING_BODY,
            connect_timeout,
            FAST,
        )
        .await
        .expect_err("no such socket");

        assert!(
            matches!(err, HerdrError::Connect { .. }),
            "expected Connect, got {err:?}"
        );
        assert!(err.is_unreachable());
        assert_eq!(err.exit_code(), 3);
        assert!(!err.is_fatal());
        assert!(
            started.elapsed() < connect_timeout,
            "ENOENT must fail immediately, not burn the connect timeout"
        );
        assert!(err.to_string().contains("/nonexistent/herdr.sock"));
    }

    /// `tests/failure_paths.rs` row 2 — *a directory socket maps to the SAME error*.
    /// ENOENT and ECONNREFUSED must produce one operator message, not two.
    #[tokio::test]
    async fn directory_socket_maps_to_the_same_error() {
        let dir = tempfile::tempdir().expect("tempdir");

        let as_dir = round_trip(dir.path(), "ping", PING_BODY, FAST, FAST)
            .await
            .expect_err("a directory is not a socket");
        let as_missing = round_trip(
            &dir.path().join("absent.sock"),
            "ping",
            PING_BODY,
            FAST,
            FAST,
        )
        .await
        .expect_err("no such socket");

        for err in [&as_dir, &as_missing] {
            assert!(
                matches!(err, HerdrError::Connect { .. }),
                "expected Connect, got {err:?}"
            );
            assert!(err.is_unreachable());
            assert_eq!(err.exit_code(), 3);
        }
    }

    /// `tests/failure_paths.rs` row 4 — *a server that never replies hits the request timeout*.
    /// This is also the shape a missing trailing newline takes against the real server.
    #[tokio::test]
    async fn server_that_never_replies_hits_the_request_timeout() {
        let mock = MockHerdr::always(Reply::Silent).await;
        let request_timeout = Duration::from_millis(200);

        let started = Instant::now();
        let err = round_trip(mock.path(), "ping", PING_BODY, FAST, request_timeout)
            .await
            .expect_err("the mock never answers");

        match err {
            HerdrError::Timeout { method, elapsed } => {
                assert_eq!(method, "ping", "the error must name the wedged method");
                assert_eq!(elapsed, request_timeout);
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
        assert!(err.is_unreachable() && err.exit_code() == 3);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the bridge must not wedge on a wedged herdr"
        );
        assert_eq!(mock.connections(), 1, "it did dial and did send");
    }

    /// `tests/failure_paths.rs` row 5 — *a server that closes early yields `ClosedEarly`*.
    #[tokio::test]
    async fn server_that_closes_early_yields_closed_early() {
        let mock = MockHerdr::always(Reply::CloseAfterRequest).await;

        let err = round_trip(mock.path(), "session.snapshot", PING_BODY, FAST, FAST)
            .await
            .expect_err("closed without replying");

        match err {
            HerdrError::ClosedEarly { method } => assert_eq!(method, "session.snapshot"),
            other => panic!("expected ClosedEarly, not an empty-string parse error; got {other:?}"),
        }
        assert!(err.is_unreachable() && err.exit_code() == 3);
    }

    /// Not a spec row, but the contract the reply reader rests on: herdr writes ONE line and resets,
    /// and anything after that first newline is not ours to read.
    #[tokio::test]
    async fn only_the_first_reply_line_is_consumed() {
        let mock = MockHerdr::always(Reply::Lines(vec![
            PONG.to_owned(),
            r#"{"id":"","result":{"type":"never_read"}}"#.to_owned(),
        ]))
        .await;

        assert_eq!(
            round_trip(mock.path(), "ping", PING_BODY, FAST, FAST)
                .await
                .unwrap(),
            PONG,
            "the trailing newline is stripped and the second line is ignored"
        );
    }
}

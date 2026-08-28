//! The wire contract — spec `docs/SLICE-1.md` § Tests → `tests/wire.rs`.
//!
//! # Where each row of the spec's table lives right now (build order step 8)
//!
//! | Row | Status |
//! |---|---|
//! | `request_is_always_newline_terminated` | **LANDED** — `src/transport.rs::tests` |
//! | `params_is_emitted_even_when_empty` | **LANDED, here** (step 6) |
//! | `id_is_a_string_and_is_never_correlated` | **LANDED, here** (step 6) |
//! | `connection_is_not_reused` | **LANDED** — `src/transport.rs::tests`, plus the client-level twin here |
//! | `oversize_request_is_rejected_client_side` | **LANDED** — `src/transport.rs::tests` |
//! | `result_wrapper_is_unwrapped_per_method` | **LANDED, here** (step 6) |
//! | `read_visible_sends_source_visible_and_omits_lines` | **LANDED, here** (step 6) |
//! | `write_ack_is_the_bare_ok_tag` | **LANDED, here** (step 8) — the last deferred row is closed |
//!
//! The socket-level rows are unit tests inside `src/transport.rs` and not integration tests here,
//! because the spec's own API section makes `transport` crate-private (`mod transport;`) and an
//! integration test cannot reach a `pub(crate)` fn. Widening the public API to place a test would
//! have created exactly the "public path that can send an unterminated line" the transport module
//! exists to forbid. They use this same `support::MockHerdr`. Step 8 filled the last deferred row
//! in HERE, against the public client, and did NOT re-home the landed transport-level ones. Every
//! row of the spec's table is now green somewhere, and this note records where.
//!
//! Everything below drives the PUBLIC `HerdrClient` only, and inspects the raw bytes the mock
//! received — which is the only vantage point from which "what actually went on the wire" is a
//! fact rather than a claim about our own serializer.

mod support;

use std::num::NonZeroU32;
use std::time::Duration;

use herdr_client::{HerdrClient, HerdrError, Key, PaneId, WorkspaceId, WriteAccepted};
use support::{MockHerdr, Reply};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const PONG: &str = r#"{"id":"","result":{"type":"pong","version":"0.8.2","protocol":20}}"#;

/// The harness contract, asserted rather than assumed: `MockHerdr` answers ONE request per
/// connection and then CLOSES, which is what makes `connection_is_not_reused` a real test of the
/// client rather than a test of a forgiving mock. Live, a second write on an answered connection is
/// a `BrokenPipe`; here the peer half-close shows up as a 0-byte read.
#[tokio::test]
async fn mock_answers_one_request_per_connection_and_closes() {
    let mock = MockHerdr::always(Reply::line(PONG)).await;

    let mut stream = UnixStream::connect(mock.path())
        .await
        .expect("dial the mock");
    stream
        .write_all(br#"{"id":"probe","method":"ping","params":{}}"#)
        .await
        .expect("write");
    stream.write_all(b"\n").await.expect("newline");

    let mut line = String::new();
    let (read_half, mut write_half) = stream.split();
    let mut reader = BufReader::new(read_half);
    reader.read_line(&mut line).await.expect("read the reply");
    assert_eq!(line, format!("{PONG}\n"));

    // The mock has closed: nothing further arrives, ever.
    let mut tail = Vec::new();
    let n = tokio::time::timeout(Duration::from_secs(2), reader.read_to_end(&mut tail))
        .await
        .expect("the mock must close, not hold the connection open")
        .expect("read to EOF");
    assert_eq!(
        n, 0,
        "a second reply must never arrive on the same connection"
    );

    // And it recorded the request, newline included, before answering.
    let _ = write_half.shutdown().await;
    assert_eq!(mock.connections(), 1);
    assert_eq!(mock.last_request().last().copied(), Some(b'\n'));
    assert_eq!(mock.last_request_json()["method"], "ping");
}

/// The mock must never be reachable at the operator's real socket. Structural, not aspirational:
/// nothing in the crate reads `$HOME` or `$HERDR_SOCKET_PATH` yet, and every path the suite uses is
/// a `tempfile::TempDir`.
#[tokio::test]
async fn mock_socket_lives_in_a_temp_dir_not_in_the_operators_config() {
    let mock = MockHerdr::always(Reply::CloseAfterRequest).await;
    let path = mock.path().to_string_lossy().into_owned();
    assert!(
        !path.contains(".config/herdr"),
        "the offline suite must never bind or dial anything under ~/.config/herdr: {path}"
    );
    assert!(
        path.contains("herdr-mock-"),
        "unexpected mock socket path: {path}"
    );
}

// ────────────────────────────────────────────────────────────────────────────────────────────────
// The spec's rows, driven through the public client (build order step 6).
// ────────────────────────────────────────────────────────────────────────────────────────────────

/// The two captured reply lines, in the live herd's own shape (de-identified by
/// `scripts/scrub-fixtures.py`: structure and key order verbatim, identifying values synthetic).
/// Used as mock answers so the decode path under test is a real payload shape, not a
/// hand-written approximation of one.
const SNAPSHOT_REPLY: &str = include_str!("fixtures/snapshot.json");
const PANE_READ_REPLY: &str = include_str!("fixtures/pane_read.json");

fn line(fixture: &str) -> &str {
    fixture.trim_end_matches(['\n', '\r'])
}

/// Re-wraps rows lifted out of the captured snapshot into another method's reply envelope, so
/// `agent.list` / `pane.list` are exercised against REAL captured objects rather than invented ones.
fn reply_from_snapshot(tag: &str, key: &str) -> String {
    let snapshot: serde_json::Value =
        serde_json::from_str(line(SNAPSHOT_REPLY)).expect("the captured snapshot is JSON");
    let rows = snapshot["result"]["snapshot"][key].clone();
    assert!(
        rows.as_array().is_some_and(|a| !a.is_empty()),
        "the fixture must carry real `{key}` rows"
    );
    serde_json::json!({ "id": "", "result": { "type": tag, key: rows } }).to_string()
}

/// Verified live: omitting `params` is `invalid_request: missing field `params``. So
/// `skip_serializing_if` must NEVER be applied to it, and the check has to be made on the BYTES the
/// server would have received — a `serde_json::Value` view alone could not tell an emitted `{}`
/// from an omitted key.
#[tokio::test]
async fn params_is_emitted_even_when_empty() {
    let mock = MockHerdr::always(Reply::line(PONG)).await;
    let client = HerdrClient::new(mock.path());
    client.ping().await.expect("the mock answers a pong");

    let raw = String::from_utf8(mock.last_request()).expect("the request is UTF-8");
    assert!(
        raw.contains(r#""params":{}"#),
        "a ping with no `params` is `invalid_request: missing field params` live: {raw}"
    );

    let sent = mock.last_request_json();
    assert_eq!(sent["method"], "ping");
    assert!(sent.get("params").is_some(), "{sent}");
    assert_eq!(sent["params"], serde_json::json!({}));

    // The same holds for the other no-argument methods and for an unscoped `pane.list`, whose
    // `workspace_id` IS omitted while `params` itself still is not.
    let mock = MockHerdr::always(Reply::line(reply_from_snapshot("pane_list", "panes"))).await;
    let client = HerdrClient::new(mock.path());
    client.panes(None).await.expect("an unscoped pane.list");
    let raw = String::from_utf8(mock.last_request()).expect("the request is UTF-8");
    assert!(raw.contains(r#""params":{}"#), "{raw}");
}

/// herdr echoes the id on a semantic refusal and BLANKS it to `""` on a parse/routing refusal, so
/// correlating on it would misclassify every `invalid_request` as a framing bug and hide the one
/// message the operator actually needs. RPC is one-shot: the CONNECTION is the correlation.
///
/// This guards against anyone later adding `assert_eq!(reply.id, sent.id)`.
#[tokio::test]
async fn id_is_a_string_and_is_never_correlated() {
    // What we send: a JSON STRING. An integer id is `invalid type: integer` live.
    let mock = MockHerdr::always(Reply::line(PONG)).await;
    let client = HerdrClient::new(mock.path());
    client.ping().await.expect("blank echoed id is fine");
    let sent = mock.last_request_json();
    assert!(
        sent["id"].is_string(),
        "an integer id is `invalid type: integer` live: {sent}"
    );
    assert_ne!(sent["id"], "", "we always send a real id for herdr's logs");

    // What we accept: an id that does not match, and an id that is not there at all. Both are
    // ordinary successes.
    for reply in [
        r#"{"id":"not-the-id-we-sent-9999","result":{"type":"pong","version":"0.8.2","protocol":20}}"#,
        r#"{"result":{"type":"pong","version":"0.8.2","protocol":20}}"#,
    ] {
        let mock = MockHerdr::always(Reply::line(reply)).await;
        let pong = HerdrClient::new(mock.path())
            .ping()
            .await
            .unwrap_or_else(|e| panic!("the id must never be correlated, got {e:?} for {reply}"));
        assert_eq!(pong.protocol, 20);
    }
}

/// The client-level twin of `src/transport.rs::tests::connection_is_not_reused`: two sequential
/// calls on ONE `HerdrClient` both succeed and cost two dials. A client that pooled would fail here
/// exactly as it would live, where a second write on an answered connection is a `BrokenPipe`.
#[tokio::test]
async fn a_second_call_on_the_same_client_dials_again() {
    let mock = MockHerdr::always(Reply::line(PONG)).await;
    let client = HerdrClient::new(mock.path());
    client.ping().await.expect("first");
    client.ping().await.expect("second");
    assert_eq!(mock.connections(), 2, "one connection per RPC, always");
}

/// Every result but `pong` nests its payload under a per-method key, and the typed method must hand
/// back the INNER value. A wrong `type` tag is caught BEFORE the payload is decoded, so the operator
/// gets "expected session_snapshot, got pane_read" rather than a serde error naming a field they
/// have never heard of.
#[tokio::test]
async fn result_wrapper_is_unwrapped_per_method() {
    // session_snapshot -> .snapshot
    let mock = MockHerdr::always(Reply::line(line(SNAPSHOT_REPLY))).await;
    let snapshot = HerdrClient::new(mock.path())
        .snapshot()
        .await
        .expect("the captured snapshot");
    assert_eq!(snapshot.protocol, 20);
    assert!(!snapshot.panes.is_empty());
    assert!(!snapshot.agents.is_empty());

    // pane_read -> .read
    let mock = MockHerdr::always(Reply::line(line(PANE_READ_REPLY))).await;
    let read = HerdrClient::new(mock.path())
        .read_visible(&PaneId::new("w9:p1"))
        .await
        .expect("the captured read");
    assert_eq!(read.pane_id, PaneId::new("w9:p1"));
    assert!(!read.text.is_empty());

    // agent_list -> .agents
    let mock = MockHerdr::always(Reply::line(reply_from_snapshot("agent_list", "agents"))).await;
    let agents = HerdrClient::new(mock.path())
        .agents()
        .await
        .expect("the roster");
    assert_eq!(agents.len(), snapshot.agents.len());
    assert_eq!(agents[0].pane_id, snapshot.agents[0].pane_id);

    // pane_list -> .panes, scoped and unscoped alike
    let mock = MockHerdr::always(Reply::line(reply_from_snapshot("pane_list", "panes"))).await;
    let client = HerdrClient::new(mock.path());
    let panes = client.panes(None).await.expect("every pane");
    assert_eq!(panes.len(), snapshot.panes.len());
    let ws = WorkspaceId::new("w9");
    client.panes(Some(&ws)).await.expect("one workspace");
    assert_eq!(
        mock.last_request_json()["params"]["workspace_id"],
        "w9",
        "D2 scopes the roster SERVER-side, in one RPC"
    );

    // A wrong tag is `UnexpectedResult`, naming both tags — never a confusing serde error.
    let mock = MockHerdr::always(Reply::line(line(PANE_READ_REPLY))).await;
    let err = HerdrClient::new(mock.path())
        .snapshot()
        .await
        .expect_err("pane_read is not a session_snapshot");
    match &err {
        HerdrError::UnexpectedResult {
            method,
            expected,
            got,
        } => {
            assert_eq!(*method, "session.snapshot");
            assert_eq!(*expected, "session_snapshot");
            assert_eq!(got, "pane_read");
        }
        other => panic!("expected UnexpectedResult, got {other:?}"),
    }
    let rendered = err.to_string();
    assert!(rendered.contains("session_snapshot"), "{rendered}");
    assert!(rendered.contains("pane_read"), "{rendered}");

    // A result with no `type` tag at all is the same class of error, not a panic.
    let mock = MockHerdr::always(Reply::line(r#"{"id":"","result":{"snapshot":{}}}"#)).await;
    let err = HerdrClient::new(mock.path())
        .snapshot()
        .await
        .expect_err("an untagged result");
    assert!(
        matches!(&err, HerdrError::UnexpectedResult { got, .. } if got.contains("no type tag")),
        "{err:?}"
    );
}

/// The type-level guarantee, asserted on the bytes: there is no public path to `recent` /
/// `recent_unwrapped` / `detection`, which harvest-scroll the operator's REAL viewport when
/// `lines > viewport_rows`. `read_visible` pins `"source":"visible"` and omits `lines` entirely;
/// `read_visible_tail(n)` carries both — and `visible` is clamped to the viewport however large `n`
/// is, so neither can move the operator's screen.
#[tokio::test]
async fn read_visible_sends_source_visible_and_omits_lines() {
    let mock = MockHerdr::always(Reply::line(line(PANE_READ_REPLY))).await;
    let client = HerdrClient::new(mock.path());
    let pane = PaneId::new("w9:p1");

    client.read_visible(&pane).await.expect("a visible read");
    let sent = mock.last_request_json();
    assert_eq!(sent["method"], "pane.read");
    assert_eq!(sent["params"]["pane_id"], "w9:p1");
    assert_eq!(sent["params"]["source"], "visible");
    assert!(
        sent["params"].get("lines").is_none(),
        "a plain visible read must not carry a line count at all: {sent}"
    );

    client
        .read_visible_tail(&pane, NonZeroU32::new(40).unwrap())
        .await
        .expect("a tail read");
    let sent = mock.last_request_json();
    assert_eq!(sent["params"]["source"], "visible");
    assert_eq!(sent["params"]["lines"], 40);

    // And nothing this client can be made to send names any other source. There is no constructor
    // that could, which is the point — this asserts it on the wire as well as in the type system.
    for request in mock.requests() {
        let raw = String::from_utf8(request).expect("UTF-8");
        for forbidden in ["recent", "recent_unwrapped", "detection"] {
            assert!(
                !raw.contains(forbidden),
                "a read source that harvest-scrolls the operator's screen reached the wire: {raw}"
            );
        }
    }
}

/// **All three write methods map the bare `{"type":"ok"}` to `WriteAccepted`, and the API offers no
/// `bool` anyone could mistake for a delivery flag.**
///
/// This is the one row of the spec's table that could not land before build order step 8, because
/// it needs `keys.rs` and the three send methods.
///
/// The distinction it pins is the whole reason `WriteAccepted` exists rather than `Ok(())` or
/// `Ok(true)`: an ack means *herdr took the bytes*, never *the TUI acted on them*. A focused TUI
/// dialog can swallow both the text and the Enter with both RPCs reporting exactly this success.
///
/// ⚠ These are the ONLY calls to `send_text` / `send_keys` / `send_input` anywhere in the
/// workspace, and they run against a `MockHerdr` on a socket inside a `TempDir`. Nothing outside
/// `#[cfg(test)]` calls them — `tests/no_live_write_call_site.rs` fails the suite if that changes.
#[tokio::test]
async fn write_ack_is_the_bare_ok_tag() {
    const OK: &str = r#"{"id":"","result":{"type":"ok"}}"#;
    let pane = PaneId::new("wZ:p9");

    // ── pane.send_text ──────────────────────────────────────────────────────────────────────────
    let mock = MockHerdr::always(Reply::line(OK)).await;
    let client = HerdrClient::new(mock.path());

    let accepted = client
        .send_text(&pane, "ship it")
        .await
        .expect("a bare ok is a successful write");
    let sent = mock.last_request_json();
    assert_eq!(sent["method"], "pane.send_text");
    assert_eq!(sent["params"]["pane_id"], "wZ:p9");
    assert_eq!(sent["params"]["text"], "ship it");
    assert_eq!(
        accepted.pane_id, pane,
        "the ack carries no pane id; we echo it"
    );
    assert_eq!(accepted.bytes, "ship it".len());

    // ── pane.send_keys ──────────────────────────────────────────────────────────────────────────
    let mock = MockHerdr::always(Reply::line(OK)).await;
    let client = HerdrClient::new(mock.path());

    let keys = [Key::parse("2").expect("a literal key"), Key::enter()];
    let accepted = client.send_keys(&pane, &keys).await.expect("ok");
    let sent = mock.last_request_json();
    assert_eq!(sent["method"], "pane.send_keys");
    assert_eq!(
        sent["params"]["keys"],
        serde_json::json!(["2", "Enter"]),
        "the Key newtype must reach the wire as plain strings: {sent}"
    );
    assert_eq!(accepted.bytes, "2".len() + "Enter".len());

    // ── pane.send_input: protocol 20's atomic text+keys ─────────────────────────────────────────
    let mock = MockHerdr::always(Reply::line(OK)).await;
    let client = HerdrClient::new(mock.path());

    let accepted = client
        .send_input(&pane, Some("ship it"), &keys)
        .await
        .expect("ok");
    let sent = mock.last_request_json();
    assert_eq!(sent["method"], "pane.send_input");
    assert_eq!(sent["params"]["text"], "ship it");
    assert_eq!(sent["params"]["keys"], serde_json::json!(["2", "Enter"]));
    assert_eq!(accepted.bytes, "ship it".len() + "2".len() + "Enter".len());

    // Only `pane_id` is required, and an absent field is OMITTED rather than null.
    let mock = MockHerdr::always(Reply::line(OK)).await;
    let client = HerdrClient::new(mock.path());
    let bare = client.send_input(&pane, None, &[]).await.expect("ok");
    assert_eq!(bare.bytes, 0, "a no-op write accepted zero bytes");
    let sent = mock.last_request_json();
    assert_eq!(
        sent["params"],
        serde_json::json!({"pane_id": "wZ:p9"}),
        "PaneSendInputParams.required = [\"pane_id\"]: {sent}"
    );

    // ── the ack is a TAG CHECK, not a shrug ─────────────────────────────────────────────────────
    // A reply that is not `ok` must be `UnexpectedResult` naming both tags, so a herdr that starts
    // answering writes differently is loud rather than silently "accepted".
    let mock = MockHerdr::always(Reply::line(
        r#"{"id":"","result":{"type":"pong","version":"0.8.2","protocol":20}}"#,
    ))
    .await;
    let client = HerdrClient::new(mock.path());
    let err = client
        .send_text(&pane, "ship it")
        .await
        .expect_err("a pong is not a write ack");
    match err {
        HerdrError::UnexpectedResult {
            method,
            expected,
            got,
        } => {
            assert_eq!(method, "pane.send_text");
            assert_eq!(expected, "ok");
            assert_eq!(got, "pong");
        }
        other => panic!("expected UnexpectedResult, got {other:?}"),
    }

    // ── and a refusal stays a refusal, verbatim ──────────────────────────────────────────────────
    let mock = MockHerdr::always(Reply::line(
        r#"{"id":"p","error":{"code":"pane_not_found","message":"pane not found: wZ:p9"}}"#,
    ))
    .await;
    let client = HerdrClient::new(mock.path());
    let err = client
        .send_keys(&pane, &keys)
        .await
        .expect_err("herdr said no");
    assert!(
        err.is_not_found(),
        "a write to a closed pane must route to the picker, never be retried elsewhere: {err}"
    );
    assert!(err.to_string().contains("pane not found: wZ:p9"), "{err}");
}

/// The type-level half of the same claim: `WriteAccepted` carries no delivery flag, and there is
/// nothing on it a caller could read as one.
///
/// Beyond the spec's table, and cheap. A `bool` return, a `delivered` field or a `deliver()` method
/// is the single most likely "helpful" refactor to land here, and it would make the bridge tell a
/// phone-only operator "sent" when a TUI dialog ate every byte.
#[tokio::test]
async fn write_accepted_carries_no_delivery_claim() {
    let mock = MockHerdr::always(Reply::line(r#"{"id":"","result":{"type":"ok"}}"#)).await;
    let client = HerdrClient::new(mock.path());
    let pane = PaneId::new("wZ:p9");

    let accepted: WriteAccepted = client.send_text(&pane, "hello").await.expect("ok");

    // Everything the type exposes, exhaustively — a struct literal here means a new field is a
    // compile error in this test, which is exactly the review moment a `delivered: bool` needs.
    let WriteAccepted { pane_id, bytes, at } = accepted.clone();
    assert_eq!(pane_id, pane);
    assert_eq!(bytes, 5);
    assert!(
        at.elapsed().is_ok(),
        "`at` is this host's clock at ack-decode time; herdr stamps nothing"
    );

    // The Debug rendering is what ends up in a log line, and it must not read as a receipt.
    let rendered = format!("{accepted:?}").to_lowercase();
    for forbidden in ["deliver", "received", "confirmed", "true"] {
        assert!(
            !rendered.contains(forbidden),
            "`{forbidden}` in a write ack reads as a delivery claim: {rendered}"
        );
    }
}

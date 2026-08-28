//! The failure paths — spec `docs/SLICE-1.md` § Tests → `tests/failure_paths.rs`.
//!
//! # Where each row of the spec's table lives right now (build order step 6)
//!
//! | Row | Status |
//! |---|---|
//! | `missing_socket_is_a_typed_error_not_a_panic` | **LANDED** — `src/transport.rs::tests` |
//! | `directory_socket_maps_to_the_same_error` | **LANDED** — `src/transport.rs::tests` |
//! | `older_protocol_is_fatal_newer_is_a_warning` | **LANDED, here** (step 6) |
//! | `server_that_never_replies_hits_the_request_timeout` | **LANDED** — `src/transport.rs::tests` |
//! | `server_that_closes_early_yields_closed_early` | **LANDED** — `src/transport.rs::tests` |
//! | `blank_id_and_echoed_id_errors_both_map_to_protocol` | **LANDED, here** — both halves, step 6 |
//! | `unknown_error_code_becomes_other_and_keeps_the_message` | **LANDED, here** |
//!
//! The four socket-level rows are unit tests inside `src/transport.rs` because the spec's API
//! section makes `transport` crate-private and an integration test cannot reach a `pub(crate)` fn —
//! see the same note in `tests/wire.rs`. They must not be re-homed here; doing so would mean
//! widening the public API to place a test.

mod support;

use herdr_client::{Compatibility, ErrorCode, HerdrClient, HerdrError, PaneId};
use support::{MockHerdr, Reply};

/// The two REAL captured error frames, straight off the live herd.
const ERRORS: &str = include_str!("fixtures/errors.ndjson");
/// The captured pong, capabilities and all.
const PONG_FIXTURE: &str = include_str!("fixtures/pong.json");

/// `ErrorBody.code` is an OPEN string in the schema — verified: `{"code":{"type":"string"},
/// "message":{"type":"string"}}`, no `enum`. A closed enum would fail to parse a future code and
/// take its message down with it, which is precisely the message the operator needs at 2 a.m.
#[test]
fn unknown_error_code_becomes_other_and_keeps_the_message() {
    let code: ErrorCode = serde_json::from_str("\"future_code_2027\"").expect("open string");
    assert_eq!(code, ErrorCode::Other("future_code_2027".to_owned()));
    assert_eq!(
        code.as_str(),
        "future_code_2027",
        "the wire string survives verbatim"
    );

    let err = HerdrError::Protocol {
        method: "pane.read",
        code,
        message: "herdr grew a new refusal we have never seen".to_owned(),
    };
    let rendered = err.to_string();
    assert!(rendered.contains("future_code_2027"), "{rendered}");
    assert!(
        rendered.contains("herdr grew a new refusal we have never seen"),
        "the message must survive an unknown code: {rendered}"
    );
    assert!(!err.is_not_found());
    assert!(!err.is_unsupported_method());
    assert_eq!(err.exit_code(), 5);
}

/// herdr refuses in two shapes and BOTH must reach the operator as `HerdrError::Protocol` carrying
/// the message verbatim: a semantic refusal ECHOES the request id, a parse/routing refusal BLANKS
/// it to `""`. Correlating on the id would misclassify every `invalid_request` as a framing bug and
/// hide the one line the operator needs.
///
/// Two halves, both live here as of step 6: the decode/predicate half (step 4, kept verbatim) and
/// the `HerdrClient::call` half (step 6), which is the claim the row is actually about — an `error`
/// reply must become a `Protocol` error and never a decode failure.
#[tokio::test]
async fn blank_id_and_echoed_id_errors_both_map_to_protocol() {
    let fixture = ERRORS;
    let frames: Vec<serde_json::Value> = fixture
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("captured frame is JSON"))
        .collect();
    assert_eq!(
        frames.len(),
        2,
        "errors.ndjson holds the two captured shapes"
    );

    // Semantic refusal: herdr ECHOES the request id.
    let echoed = &frames[0];
    assert_eq!(echoed["id"], "probe");
    let echoed_err = protocol_error_from(echoed);
    assert!(
        matches!(&echoed_err, HerdrError::Protocol { code, .. } if *code == ErrorCode::PaneNotFound),
        "{echoed_err:?}"
    );
    assert!(
        echoed_err.is_not_found(),
        "a closed sticky target must reach the picker, never a silent reroute"
    );
    assert_eq!(echoed_err.exit_code(), 5);

    // Parse/routing refusal: herdr BLANKS the id. Never correlate on it.
    let blank = &frames[1];
    assert_eq!(blank["id"], "");
    let blank_err = protocol_error_from(blank);
    assert!(
        matches!(&blank_err, HerdrError::Protocol { code, .. } if *code == ErrorCode::InvalidRequest),
        "{blank_err:?}"
    );
    assert!(!blank_err.is_not_found());
    assert!(
        !blank_err.is_unsupported_method(),
        "`missing field params` is not a missing METHOD; only `unknown variant` is"
    );
    assert_eq!(blank_err.exit_code(), 5);

    // ── the step-6 half: the SAME two frames, now through `HerdrClient::call` ────────────────────
    // A reply carrying `error` must short-circuit BEFORE the result tag is looked at, so herdr's
    // own message survives verbatim instead of becoming "missing field `result`".
    let mock = MockHerdr::always(Reply::line(echoed.to_string())).await;
    let err = HerdrClient::new(mock.path())
        .read_visible(&PaneId::new("zz:p9"))
        .await
        .expect_err("pane_not_found is a refusal, not a success");
    match &err {
        HerdrError::Protocol {
            method,
            code,
            message,
        } => {
            assert_eq!(*method, "pane.read");
            assert_eq!(*code, ErrorCode::PaneNotFound);
            assert_eq!(
                message, "pane zz:p9 not found",
                "herdr's message reaches the operator verbatim"
            );
        }
        other => panic!("an `error` reply must be a Protocol error, got {other:?}"),
    }
    assert!(err.is_not_found());
    assert_eq!(err.exit_code(), 5);

    // The blank-id frame, answered to a request that DID carry a real id. The mismatch must not
    // matter: no `assert_eq!(reply.id, sent.id)` exists, and adding one would break this.
    let mock = MockHerdr::always(Reply::line(blank.to_string())).await;
    let client = HerdrClient::new(mock.path());
    let err = client
        .snapshot()
        .await
        .expect_err("invalid_request is a refusal");
    match &err {
        HerdrError::Protocol {
            method,
            code,
            message,
        } => {
            assert_eq!(*method, "session.snapshot");
            assert_eq!(*code, ErrorCode::InvalidRequest);
            assert!(message.contains("missing field `params`"), "{message}");
        }
        other => panic!("expected Protocol, got {other:?}"),
    }
    assert_eq!(err.exit_code(), 5);
    assert!(!err.is_not_found());
    assert!(
        !err.is_unsupported_method(),
        "`missing field params` is not a missing METHOD; only `unknown variant` is"
    );

    let sent = mock.last_request_json();
    assert_ne!(
        sent["id"], "",
        "we sent a real id and herdr blanked it in the reply; the mismatch must be irrelevant"
    );
}

/// Builds the `Protocol` error `client.call` builds, from a captured frame, so the decode half of
/// the row above can assert on `ErrorCode` and the routing predicates without a socket.
fn protocol_error_from(frame: &serde_json::Value) -> HerdrError {
    HerdrError::Protocol {
        method: "pane.read",
        code: ErrorCode::from_wire(frame["error"]["code"].as_str().expect("error.code")),
        message: frame["error"]["message"]
            .as_str()
            .expect("error.message")
            .to_owned(),
    }
}

/// **Unknown ADDITIONS are survivable; REMOVALS are not.** That asymmetry is the whole version
/// policy, and this is it as a test. `agent.send` vanished between protocol 16 and 20 — a removal
/// surfaces as `invalid_request: unknown variant`, which this client can detect but cannot repair,
/// so running degraded below the minimum would mean silently missing asks. For a phone-only
/// operator that is worse than not starting.
///
/// There is no hello method in protocol 20: the handshake is built on `ping`, whose reply carries
/// version, protocol and a capabilities map.
#[tokio::test]
async fn older_protocol_is_fatal_newer_is_a_warning() {
    fn pong(protocol: u32) -> String {
        // No `capabilities` key at all — `PingResult.capabilities` defaults to null, so a pong
        // without it must still parse. That is part of this row.
        format!(
            r#"{{"id":"h","result":{{"type":"pong","version":"0.8.2","protocol":{protocol}}}}}"#
        )
    }

    // 19: below MIN_SUPPORTED_PROTOCOL. Fatal, exit 4, and NOT an unreachable-herdr error — the
    // operator's message is "your herdr is too old", not "herdr is down".
    let mock = MockHerdr::always(Reply::line(pong(19))).await;
    let err = HerdrClient::new(mock.path())
        .handshake()
        .await
        .expect_err("19 is below the minimum");
    match &err {
        HerdrError::ProtocolTooOld {
            server,
            min,
            client,
            server_version,
        } => {
            assert_eq!((*server, *min, *client), (19, 20, 20));
            assert_eq!(server_version, "0.8.2");
        }
        other => panic!("expected ProtocolTooOld, got {other:?}"),
    }
    assert!(err.is_fatal());
    assert_eq!(err.exit_code(), 4);
    assert!(!err.is_unreachable(), "exit 3 is the socket-is-gone story");

    // 20: exactly what this client was built against.
    let mock = MockHerdr::always(Reply::line(pong(20))).await;
    let hs = HerdrClient::new(mock.path())
        .handshake()
        .await
        .expect("20 is ours");
    assert_eq!(hs.compatibility, Compatibility::Exact);
    assert_eq!(hs.protocol(), 20);
    assert_eq!(hs.version(), "0.8.2");
    assert!(
        hs.capabilities().is_none(),
        "a pong with no capabilities key must still parse"
    );
    assert!(
        !hs.live_handoff(),
        "no advertisement is read conservatively as `no live handoff`"
    );

    // 21: ahead of us. Ok, not Err — the bridge stays alive through a routine `herdr update`.
    let mock = MockHerdr::always(Reply::line(pong(21))).await;
    let hs = HerdrClient::new(mock.path())
        .handshake()
        .await
        .expect("a newer server is survivable, not fatal");
    assert_eq!(hs.compatibility, Compatibility::ServerNewer { by: 1 });
    assert_eq!(hs.compatibility.ahead_by(), 1);
    assert_eq!(hs.compatibility.as_str(), "server_newer");

    // And the REAL captured pong, capabilities and all. `live_handoff` is why the handshake must be
    // re-run on every event-stream reconnect: herdr can swap its own binary under a live socket.
    let mock = MockHerdr::always(Reply::line(PONG_FIXTURE.trim_end_matches(['\n', '\r']))).await;
    let hs = HerdrClient::new(mock.path())
        .handshake()
        .await
        .expect("the captured pong");
    assert_eq!(hs.compatibility, Compatibility::Exact);
    assert!(hs.live_handoff());
    assert!(
        hs.capabilities()
            .expect("advertised")
            .detached_server_daemon
    );
}

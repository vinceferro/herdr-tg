//! The event decoder and the event stream — spec `docs/SLICE-1.md` § Tests → `tests/events.rs`,
//! "the highest-value file".
//!
//! # Every row of the spec's table, and where it is (build order step 7)
//!
//! | Row | Status |
//! |---|---|
//! | `two_envelope_families_decode_from_one_stream` | **LANDED, here** |
//! | `roster_event_discards_pane_info` | **LANDED, here** |
//! | `unknown_event_kind_is_bucketed_not_fatal` | **LANDED, here** |
//! | `malformed_known_kind_still_errors` | **LANDED, here** |
//! | `subscription_serializes_dot_form_with_pane_id` | **LANDED, here** |
//! | `ack_is_consumed_then_events_flow` | **LANDED, here** |
//! | `end_of_stream_is_none_and_no_reconnect` | **LANDED, here** |
//! | `subscriptions_are_retained_for_reissue` | **LANDED, here** |
//!
//! Three tests beyond the spec's table are marked `BEYOND THE SPEC'S TABLE` in their own doc
//! comments (the `subscribe` error paths, which no row covers). Tests that must name crate-private
//! items live next to them instead: two in `src/proto/event.rs::tests` for the decoder's private
//! shapes and `KNOWN_ROSTER`, and four in `src/stream.rs::tests` for the reader itself — the
//! per-frame byte ceiling, the bound on an unterminated line, the EOF latch, and the cancel-safety
//! property slice 3's `tokio::select!` depends on.
//!
//! # Offline, and structurally so
//!
//! Every frame here is either a frame from `fixtures/events-mixed.ndjson` (captured from the live
//! herd on 2026-08-28 in ONE 10 s read-only `events.subscribe`, then de-identified by
//! `scripts/scrub-fixtures.py` — frame shape verbatim, identifying values synthetic) or an inline
//! literal. Every stream is a `support::MockHerdr` on a Unix socket in a `TempDir`. Nothing reads
//! `$HOME`, `$HERDR_SOCKET_PATH` or the operator's real herdr socket.

mod support;

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;
use herdr_client::proto::event::decode_event;
use herdr_client::{
    AgentStatus, Event, EventStream, HerdrClient, HerdrError, PaneId, RosterEvent, Subscription,
    WorkspaceId,
};
use support::{MockHerdr, Reply};

/// The captured stream, verbatim. 29 frames from one connection: 27 snake_case `pane_updated`
/// (the replay backlog), 1 dot-form `pane.agent_status_changed`, and the hand-appended invented
/// `pane_teleported`.
const CAPTURED: &str = include_str!("fixtures/events-mixed.ndjson");

/// The `events.subscribe` ack, exactly as herdr writes it.
const ACK: &str = r#"{"id":"7","result":{"type":"subscription_started"}}"#;

fn frames() -> Vec<&'static str> {
    CAPTURED.lines().filter(|l| !l.trim().is_empty()).collect()
}

fn json(line: &str) -> serde_json::Value {
    serde_json::from_str(line).expect("every captured frame is well-formed JSON")
}

/// The index of the one dot-form frame in the capture.
fn dot_form_index(frames: &[&str]) -> usize {
    frames
        .iter()
        .position(|l| json(l)["event"] == "pane.agent_status_changed")
        .expect(
            "fixtures/events-mixed.ndjson must contain a `pane.agent_status_changed` frame; \
             without both families on one connection this whole file is worthless. Re-run \
             ./scripts/capture-fixtures.sh",
        )
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// THE test.
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// **THE test.** Two incompatible envelope encodings, adjacent, on one connection.
///
/// The captured dot-form `pane.agent_status_changed` frame — `data` with **no `type` key at all** —
/// immediately followed by the captured snake_case `pane_updated` frame that came next on the same
/// socket. Both must decode to the right variant.
///
/// It also builds the trap decoder inline and proves it fails: a `#[serde(tag = "type")]` model
/// over `data` (what herdr's own `HERDR_API.md` leads you to write) parses the lifecycle frame and
/// **errors on the ask** — the product silently delivering nothing, which no test of the lifecycle
/// family alone could ever see.
#[tokio::test]
async fn two_envelope_families_decode_from_one_stream() {
    let frames = frames();
    let i = dot_form_index(&frames);
    let ask = frames[i];
    let roster = *frames.get(i + 1).expect("a frame follows the dot-form one");

    // The capture's own shape, asserted before anything is decoded — this is the trap itself.
    let ask_json = json(ask);
    let roster_json = json(roster);
    assert_eq!(ask_json["event"], "pane.agent_status_changed");
    assert!(
        ask_json["data"].get("type").is_none(),
        "the dot-form family carries NO `type` inside `data`; if this ever becomes present the \
         whole two-step decoder can be simplified: {ask_json}"
    );
    assert_eq!(roster_json["event"], "pane_updated");
    assert_eq!(
        roster_json["data"]["type"], "pane_updated",
        "the snake_case family repeats its own name inside `data`"
    );

    // ── the trap, built here so it can be proven to fail ─────────────────────────────────────────
    #[derive(Debug, serde::Deserialize)]
    #[serde(tag = "type")]
    enum TaggedOnData {
        #[serde(rename = "pane_updated")]
        PaneUpdated {
            #[allow(dead_code)]
            pane: serde_json::Value,
        },
        // The spelling a reader of `HERDR_API.md`'s "the event field is snake_case" sentence
        // would reach for. It never matches, because the dot-form `data` has no tag to match on.
        #[serde(rename = "pane_agent_status_changed")]
        AgentStatus {
            #[allow(dead_code)]
            agent_status: String,
        },
    }

    assert!(
        serde_json::from_value::<TaggedOnData>(roster_json["data"].clone()).is_ok(),
        "the trap decoder DOES parse the lifecycle family — that is exactly what makes it look \
         correct in every test that only feeds it lifecycle frames"
    );
    let trapped = serde_json::from_value::<TaggedOnData>(ask_json["data"].clone())
        .expect_err("a data-tagged model cannot parse a frame whose data has no tag");
    assert!(
        trapped.to_string().contains("missing field `type`"),
        "the trap fails for the documented reason: {trapped}"
    );

    // ── the real decoder, on the same two lines ─────────────────────────────────────────────────
    match decode_event(ask).expect("the ask must decode") {
        Event::AgentStatus(a) => {
            assert_eq!(a.pane_id, PaneId::new("wA:p1"));
            assert_eq!(a.workspace_id, WorkspaceId::new("wA"));
            assert_eq!(a.agent_status, AgentStatus::Blocked);
            assert!(!a.agent_status.is_indeterminate(), "blocked is pushable");
            assert_eq!(a.agent.as_deref(), Some("opencode"));
            // Absent from every captured frame — slice 3's ask summary must come from
            // read_visible(), not from here.
            assert!(a.title.is_none() && a.display_agent.is_none() && a.state_labels.is_none());
        }
        other => panic!("the product's one load-bearing event decoded as {other:?}"),
    }

    match decode_event(roster).expect("the lifecycle frame must decode") {
        Event::Roster(RosterEvent::PaneUpdated {
            pane_id,
            workspace_id,
        }) => {
            assert_eq!(pane_id, PaneId::new("wD:p1"));
            assert_eq!(workspace_id, WorkspaceId::new("wD"));
        }
        other => panic!("expected a roster PaneUpdated, got {other:?}"),
    }

    // ── and now through a REAL stream, because "from one stream" is the claim ───────────────────
    let mock = MockHerdr::always(Reply::Lines(vec![
        ACK.to_owned(),
        ask.to_owned(),
        roster.to_owned(),
    ]))
    .await;
    let pane = PaneId::new("wA:p1");
    let mut stream = HerdrClient::new(mock.path())
        .subscribe(&[Subscription::agent_status_any(&pane)])
        .await
        .expect("the mock acks");

    let first = stream.next().await.expect("frame 1").expect("decodes");
    let second = stream.next().await.expect("frame 2").expect("decodes");
    assert!(
        matches!(first, Event::AgentStatus(_)),
        "frame 1 came back as {first:?}"
    );
    assert!(
        matches!(second, Event::Roster(RosterEvent::PaneUpdated { .. })),
        "frame 2 came back as {second:?}"
    );
    assert!(stream.next().await.is_none(), "then EOF");
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The roster family must not be able to speak about status.
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// The decoded roster event exposes **no status at all**.
///
/// `pane.updated` replays an ageing backlog on every connect, and the captured fixture is that
/// backlog: `wB:p1` revisions 6→19 all carrying `agent_status:"blocked"`, `wD:p1` revisions 16→26
/// all carrying `"working"`. A bridge that read status from this family would fire a phantom-push
/// burst every time it reconnected — and because the backlog *ages*, dedupe-by-frame-identity
/// cannot save it either.
///
/// So this asserts the structural property, not the behavioural one: the status is not merely
/// ignored, it is absent from the decoded value. The `Debug` check is the strong half — `Debug`
/// prints every field a type has, so if a status were ever "helpfully" restored in a refactor, it
/// would show up here.
#[test]
fn roster_event_discards_pane_info() {
    let frames = frames();

    // The single frame the spec names: a `pane_updated` whose embedded PaneInfo says "blocked".
    let blocked = frames
        .iter()
        .copied()
        .find(|l| {
            let v = json(l);
            v["event"] == "pane_updated" && v["data"]["pane"]["agent_status"] == "blocked"
        })
        .expect("the captured backlog contains a `blocked` pane_updated frame");

    let raw = json(blocked);
    assert_eq!(raw["data"]["pane"]["agent_status"], "blocked");
    assert_eq!(raw["data"]["pane"]["pane_id"], "wB:p1");

    let decoded = decode_event(blocked).expect("a well-formed roster frame decodes");
    match &decoded {
        Event::Roster(r) => assert_eq!(
            *r,
            RosterEvent::PaneUpdated {
                pane_id: PaneId::new("wB:p1"),
                workspace_id: WorkspaceId::new("wB"),
            },
            "the decoded value is ids and nothing else"
        ),
        other => panic!("expected a roster event, got {other:?}"),
    }

    // The whole backlog, not just one frame: every status the wire carried, gone.
    let mut raw_statuses: Vec<String> = Vec::new();
    let mut decoded_debug: Vec<String> = Vec::new();
    for line in frames.iter().filter(|l| json(l)["event"] == "pane_updated") {
        let status = json(line)["data"]["pane"]["agent_status"]
            .as_str()
            .expect("every replayed frame carries a historical agent_status")
            .to_owned();
        if !raw_statuses.contains(&status) {
            raw_statuses.push(status);
        }
        decoded_debug.push(format!("{:?}", decode_event(line).expect("decodes")));
    }

    assert!(
        raw_statuses.len() >= 2 && raw_statuses.iter().any(|s| s == "blocked"),
        "the fixture must actually carry the hazard it is proving unreachable; saw {raw_statuses:?}"
    );
    for rendered in &decoded_debug {
        assert!(
            !rendered.contains("agent_status"),
            "a status field reached the decoded roster event: {rendered}"
        );
        for status in &raw_statuses {
            assert!(
                !rendered.contains(status.as_str()),
                "the historical status {status:?} survived decode: {rendered}"
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// Forward compatibility, and its limit.
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// An unmodelled KIND buckets to [`Event::Unrecognized`] and returns `Ok`, never `Err`.
///
/// This is the forward-compat contract that replaces `deny_unknown_fields`: a routine `herdr
/// update` that adds an event kind must not kill a bridge running unattended under
/// `Restart=always`.
///
/// Two inputs, deliberately different in kind:
///
/// * the invented `pane_teleported`, appended to the fixture by hand;
/// * `workspace_focused`, a REAL unmodelled kind — written inline as a string literal because it
///   cannot be captured passively (`workspace.focused` only fires when the operator switches
///   workspace, which a read-only capture must not induce). `tests/schema_drift.rs` proves it is a
///   genuine member of the schema's 26-name `EventKind` enum, so this literal is grounded rather
///   than guessed.
#[test]
fn unknown_event_kind_is_bucketed_not_fatal() {
    let invented = frames()
        .into_iter()
        .find(|l| json(l)["event"] == "pane_teleported")
        .expect("the fixture's hand-appended invented kind");

    match decode_event(invented).expect("an unknown kind is Ok, not Err") {
        Event::Unrecognized { event, data } => {
            assert_eq!(event, "pane_teleported");
            assert_eq!(data, serde_json::json!({}));
        }
        other => panic!("expected Unrecognized, got {other:?}"),
    }

    // A real unmodelled kind, snake_case per the two-family rule (the SUBSCRIPTION is
    // `workspace.focused`; the EVENT arrives as `workspace_focused`).
    let real =
        r#"{"event":"workspace_focused","data":{"type":"workspace_focused","workspace_id":"wD"}}"#;
    match decode_event(real).expect("a protocol-21-shaped addition must never be fatal") {
        Event::Unrecognized { event, data } => {
            assert_eq!(event, "workspace_focused");
            assert_eq!(
                data["workspace_id"], "wD",
                "the payload is carried through verbatim so a later slice can model it without a \
                 re-capture"
            );
        }
        other => panic!("expected Unrecognized, got {other:?}"),
    }

    // The snake_case twin of the product's own event is NOT subscribable (verified live:
    // `{"type":"pane_agent_status_changed"}` -> `invalid_request: unknown variant`), so it too
    // buckets. Pinned here so nobody "fixes" it into the AgentStatus arm and quietly creates a
    // second, differently-shaped path to the push trigger.
    let twin = r#"{"event":"pane_agent_status_changed","data":{"type":"pane_agent_status_changed","pane_id":"wA:p1","workspace_id":"wA","agent_status":"blocked"}}"#;
    assert!(
        matches!(decode_event(twin).expect("Ok"), Event::Unrecognized { .. }),
        "the snake_case twin must bucket, not become a second AgentStatus path"
    );
}

/// A MALFORMED frame of a kind we DO claim to handle still errors — loudly, carrying the raw line.
///
/// The catch-all is gated on the KIND, so it cannot swallow real corruption. If this ever regresses
/// to `Ok(Unrecognized)`, the bridge would go quiet on a herdr that had started emitting a broken
/// `pane.agent_status_changed`, which is indistinguishable from "no agents ever ask anything".
#[test]
fn malformed_known_kind_still_errors() {
    let cases: &[(&str, &str)] = &[
        (
            "the product's event, missing the one field it exists to carry",
            r#"{"event":"pane.agent_status_changed","data":{"pane_id":"wA:p1","workspace_id":"wA"}}"#,
        ),
        (
            "a roster frame whose embedded pane is missing",
            r#"{"event":"pane_updated","data":{"type":"pane_updated"}}"#,
        ),
        (
            "a roster frame whose embedded pane lost its workspace_id",
            r#"{"event":"pane_updated","data":{"type":"pane_updated","pane":{"pane_id":"wD:p1"}}}"#,
        ),
        (
            "a top-level-id roster frame missing an id",
            r#"{"event":"pane_closed","data":{"type":"pane_closed","pane_id":"wD:p1"}}"#,
        ),
        (
            "an envelope with no data at all",
            r#"{"event":"pane_updated"}"#,
        ),
        ("not JSON", "}{"),
    ];

    for (why, line) in cases {
        let err = decode_event(line).expect_err(&format!("must error: {why}"));
        match &err {
            HerdrError::Decode {
                method, line: raw, ..
            } => {
                assert_eq!(*method, "events.subscribe");
                assert_eq!(
                    raw, line,
                    "the raw frame must be carried: a decode error the operator cannot see the \
                     bytes of is not actionable from a phone"
                );
            }
            other => panic!("{why}: expected Decode, got {other:?}"),
        }
        // Not a transport problem, so it must not be reported as "herdr unreachable".
        assert!(!err.is_unreachable(), "{why}");
    }

    // And the boundary itself: the SAME kind, well-formed, is Ok. Without this the test above
    // would still pass if the decoder simply errored on everything.
    assert!(
        decode_event(
            r#"{"event":"pane.agent_status_changed","data":{"pane_id":"wA:p1","workspace_id":"wA","agent_status":"idle"}}"#
        )
        .is_ok(),
        "the minimal well-formed frame must decode"
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The outbound half.
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// Subscriptions serialize **dot-form, with `pane_id`**, and the unfiltered form **omits**
/// `agent_status` rather than sending null.
///
/// Live, both mistakes are refusals: the snake_case spelling is `invalid_request: unknown variant`,
/// and the `pane_id`-less form is `invalid_request: missing field 'pane_id'`.
#[test]
fn subscription_serializes_dot_form_with_pane_id() {
    let pane = PaneId::new("wA:p1");

    let filtered = serde_json::to_string(&Subscription::agent_status(&pane, AgentStatus::Blocked))
        .expect("plain data serializes");
    assert_eq!(
        filtered,
        r#"{"type":"pane.agent_status_changed","pane_id":"wA:p1","agent_status":"blocked"}"#
    );

    let unfiltered =
        serde_json::to_string(&Subscription::agent_status_any(&pane)).expect("serializes");
    assert_eq!(
        unfiltered,
        r#"{"type":"pane.agent_status_changed","pane_id":"wA:p1"}"#
    );
    let v: serde_json::Value = serde_json::from_str(&unfiltered).unwrap();
    assert!(
        v.get("agent_status").is_none(),
        "omitted, NOT null: herdr must see no `agent_status` key at all"
    );

    // Every other variant, so the wire names are pinned against the schema's 27 rather than
    // re-derived by the next slice. Only these three take a pane_id.
    let cases: &[(Subscription, &str)] = &[
        (
            Subscription::PaneScrollChanged {
                pane_id: pane.clone(),
            },
            r#"{"type":"pane.scroll_changed","pane_id":"wA:p1"}"#,
        ),
        (Subscription::PaneCreated, r#"{"type":"pane.created"}"#),
        (Subscription::PaneClosed, r#"{"type":"pane.closed"}"#),
        (Subscription::PaneExited, r#"{"type":"pane.exited"}"#),
        (Subscription::PaneMoved, r#"{"type":"pane.moved"}"#),
        (
            Subscription::PaneAgentDetected,
            r#"{"type":"pane.agent_detected"}"#,
        ),
        (
            Subscription::WorkspaceClosed,
            r#"{"type":"workspace.closed"}"#,
        ),
        (Subscription::PaneFocused, r#"{"type":"pane.focused"}"#),
    ];
    for (sub, expected) in cases {
        assert_eq!(&serde_json::to_string(sub).expect("serializes"), expected);
    }
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The stream.
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// `subscribe()` returns only after the ack, and the ack never leaks out as an event.
///
/// The mock emits `subscription_started` + 3 frames + EOF; the stream must yield exactly 3.
#[tokio::test]
async fn ack_is_consumed_then_events_flow() {
    let frames = frames();
    let i = dot_form_index(&frames);
    let three: Vec<String> = [frames[i], frames[i + 1], frames[i + 2]]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();

    let mut lines = vec![ACK.to_owned()];
    lines.extend(three);
    let mock = MockHerdr::always(Reply::Lines(lines)).await;

    let pane = PaneId::new("wA:p1");
    let subs = [Subscription::agent_status(&pane, AgentStatus::Blocked)];
    let mut stream = HerdrClient::new(mock.path())
        .subscribe(&subs)
        .await
        .expect("subscribe returns after the ack");

    // What actually went on the wire. `EventsSubscribeParams.required = ["subscriptions"]`, so the
    // params are an OBJECT with that key — a bare array is `invalid_request`.
    let sent = mock.last_request_json();
    assert_eq!(sent["method"], "events.subscribe");
    assert!(sent["id"].is_string());
    assert!(sent["params"].is_object(), "not a bare array: {sent}");
    assert_eq!(
        sent["params"]["subscriptions"][0],
        serde_json::json!({
            "type": "pane.agent_status_changed",
            "pane_id": "wA:p1",
            "agent_status": "blocked",
        })
    );
    assert_eq!(
        mock.last_request().last().copied(),
        Some(b'\n'),
        "subscribe goes through the same framing invariant as every other request"
    );

    let mut got = Vec::new();
    while let Some(event) = stream.next().await {
        got.push(event.expect("every captured frame decodes"));
    }

    assert_eq!(
        got.len(),
        3,
        "exactly the 3 frames — `subscription_started` must never appear here: {got:?}"
    );
    for event in &got {
        if let Event::Unrecognized { event, .. } = event {
            panic!("the ack leaked into the stream as {event:?}");
        }
    }
    assert!(matches!(got[0], Event::AgentStatus(_)));
    assert_eq!(mock.connections(), 1);
}

/// EOF yields `None`, and the client does **not** reconnect.
///
/// The mock is willing to serve another connection — `MockHerdr::always` answers forever — so a
/// self-healing client would show up as a second dial and a fourth event. `connections() == 1` is
/// therefore a real assertion about the client, not about a stingy harness.
///
/// This is the contract PLAN.md's "a single recovery notice when the stream re-establishes (not one
/// per retry)" rests on: the binary cannot report a recovery it never saw fail.
#[tokio::test]
async fn end_of_stream_is_none_and_no_reconnect() {
    let frames = frames();
    let one = frames[dot_form_index(&frames)].to_owned();
    let mock = MockHerdr::always(Reply::Lines(vec![ACK.to_owned(), one])).await;

    let pane = PaneId::new("wA:p1");
    let mut stream = HerdrClient::new(mock.path())
        .subscribe(&[Subscription::agent_status_any(&pane)])
        .await
        .expect("ack");

    // Driven through the `futures_core::Stream` impl itself, not the convenience wrapper, so the
    // trait the spec names is genuinely exercised.
    assert!(matches!(
        poll_via_stream_trait(&mut stream).await,
        Some(Ok(Event::AgentStatus(_)))
    ));
    assert!(
        poll_via_stream_trait(&mut stream).await.is_none(),
        "EOF is None"
    );
    assert!(
        poll_via_stream_trait(&mut stream).await.is_none(),
        "and stays None when polled past the end, rather than poking a dead socket"
    );

    assert_eq!(
        mock.connections(),
        1,
        "the client must NOT re-dial: a silent internal reconnect would replay the roster backlog \
         as if it were new edges, and would hide the drop the binary has to report"
    );
}

/// `subscriptions()` returns what was passed, verbatim, so the reconnect loop can re-issue it.
///
/// `events.subscribe` freezes the set at connect and there is no `events.update`, so a pane created
/// later means tearing this stream down and opening a new one with these entries plus the new pane.
#[tokio::test]
async fn subscriptions_are_retained_for_reissue() {
    let a = PaneId::new("wA:p1");
    let b = PaneId::new("wD:p1");
    let subs = vec![
        Subscription::agent_status(&a, AgentStatus::Blocked),
        Subscription::agent_status_any(&b),
        Subscription::PaneCreated,
    ];

    let mock = MockHerdr::always(Reply::Lines(vec![ACK.to_owned()])).await;
    let stream = HerdrClient::new(mock.path())
        .subscribe(&subs)
        .await
        .expect("ack");

    assert_eq!(stream.subscriptions(), subs.as_slice());
    assert_eq!(
        stream.subscriptions().len(),
        3,
        "there is no global agent-status subscription, so slice 3 fans out one entry per agent pane"
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// BEYOND THE SPEC'S TABLE — the subscribe error paths, which no row covers.
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// **BEYOND THE SPEC'S TABLE.** A refused or wrong-shaped ack must never hand back a stream.
///
/// `subscribe` takes the same error / tag / unwrap path as every other method, so herdr's own
/// refusal message survives and a wrong shape names both tags instead of producing a serde error
/// about a field the operator has never heard of.
#[tokio::test]
async fn a_refused_or_wrong_shaped_ack_never_yields_a_stream() {
    let pane = PaneId::new("wA:p1");
    let subs = [Subscription::agent_status_any(&pane)];

    // herdr said no.
    let mock = MockHerdr::always(Reply::line(
        r#"{"id":"7","error":{"code":"pane_not_found","message":"pane wA:p1 not found"}}"#,
    ))
    .await;
    let err = HerdrClient::new(mock.path())
        .subscribe(&subs)
        .await
        .expect_err("a refusal is not a stream");
    match &err {
        HerdrError::Protocol { method, code, .. } => {
            assert_eq!(*method, "events.subscribe");
            assert_eq!(code.as_str(), "pane_not_found");
        }
        other => panic!("expected Protocol, got {other:?}"),
    }
    assert!(err.is_not_found(), "the picker path, not a crash");

    // Right envelope, wrong tag.
    let mock = MockHerdr::always(Reply::line(
        r#"{"id":"7","result":{"type":"pong","version":"0.8.2","protocol":20}}"#,
    ))
    .await;
    let err = HerdrClient::new(mock.path())
        .subscribe(&subs)
        .await
        .expect_err("wrong tag");
    match err {
        HerdrError::UnexpectedResult {
            method,
            expected,
            got,
        } => {
            assert_eq!(method, "events.subscribe");
            assert_eq!(expected, "subscription_started");
            assert_eq!(got, "pong");
        }
        other => panic!("expected UnexpectedResult, got {other:?}"),
    }

    // Accepted the subscribe and closed without acking.
    let mock = MockHerdr::always(Reply::CloseAfterRequest).await;
    let err = HerdrClient::new(mock.path())
        .subscribe(&subs)
        .await
        .expect_err("no ack");
    assert!(
        matches!(err, HerdrError::ClosedEarly { method } if method == "events.subscribe"),
        "expected ClosedEarly, got {err:?}"
    );
    assert_eq!(err.exit_code(), 3);
}

/// **BEYOND THE SPEC'S TABLE.** A server that accepts the subscribe and never acks must not wedge
/// the bridge.
///
/// The ack read is the ONLY bounded read on this connection — there is no heartbeat on the event
/// stream (>9 s of silence observed on a healthy one), so a timeout on the frames themselves would
/// be a liveness misreport rather than a safety net.
#[tokio::test]
async fn a_server_that_never_acks_hits_the_request_timeout() {
    let mock = MockHerdr::always(Reply::Silent).await;
    let pane = PaneId::new("wA:p1");

    let err = HerdrClient::new(mock.path())
        .with_timeouts(Duration::from_secs(2), Duration::from_millis(200))
        .subscribe(&[Subscription::agent_status_any(&pane)])
        .await
        .expect_err("the mock never acks");

    match err {
        HerdrError::Timeout { method, elapsed } => {
            assert_eq!(method, "events.subscribe");
            assert_eq!(elapsed, Duration::from_millis(200));
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
}

/// **BEYOND THE SPEC'S TABLE.** One corrupt frame does not end the stream.
///
/// A bridge that has to run unattended for weeks must not be killed by a single bad line, and the
/// only alternative — swallowing it — is the failure this whole file exists to prevent. So a
/// decode error is reported AND the next frame still arrives.
#[tokio::test]
async fn a_decode_error_is_reported_and_the_stream_continues() {
    let frames = frames();
    let good = frames[dot_form_index(&frames)].to_owned();
    let mock = MockHerdr::always(Reply::Lines(vec![
        ACK.to_owned(),
        r#"{"event":"pane.agent_status_changed","data":{"pane_id":"wA:p1"}}"#.to_owned(),
        good,
    ]))
    .await;

    let pane = PaneId::new("wA:p1");
    let mut stream = HerdrClient::new(mock.path())
        .subscribe(&[Subscription::agent_status_any(&pane)])
        .await
        .expect("ack");

    let first = stream
        .next()
        .await
        .expect("a frame")
        .expect_err("malformed");
    assert!(matches!(first, HerdrError::Decode { .. }), "{first:?}");
    let second = stream.next().await.expect("a frame").expect("well-formed");
    assert!(matches!(second, Event::AgentStatus(_)), "{second:?}");
    assert!(stream.next().await.is_none());
}

/// Poll the stream through `futures_core::Stream` rather than through the inherent convenience
/// method, so the trait impl the spec names is exercised by at least one test.
async fn poll_via_stream_trait(stream: &mut EventStream) -> Option<Result<Event, HerdrError>> {
    std::future::poll_fn(
        |cx: &mut Context<'_>| -> Poll<Option<Result<Event, HerdrError>>> {
            Pin::new(&mut *stream).poll_next(cx)
        },
    )
    .await
}

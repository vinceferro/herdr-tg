//! The event decoder — the two-step one, and the highest-risk file in this crate.
//!
//! # THE TRAP: one connection, two incompatible envelope encodings
//!
//! Both wire schemas require exactly `{event, data}`, and they are *the same envelope* — but the
//! two `data` shapes are incompatible, and the ONLY thing that discriminates them is the outer
//! `event` string. Verified on one 12 s connection against herdr 0.8.2 / protocol 20:
//!
//! ```text
//! 30 x {"data":{"pane":{…,"agent_status":"blocked"},"type":"pane_updated"},"event":"pane_updated"}
//!  1 x {"data":{"agent":"opencode","agent_status":"working","pane_id":"wD:p1","workspace_id":"wD"},
//!       "event":"pane.agent_status_changed"}
//! ```
//!
//! (That is the replan's probe. The checked-in fixture is a second capture of the same two families
//! — its dot-form frame is `wA:p1` / `blocked` — which is what `tests/events.rs` asserts against.)
//!
//! The lifecycle family is snake_case and repeats its own name inside `data` as a redundant `type`.
//! The product's ONE load-bearing event is dot-form and carries **no `type` key inside `data` at
//! all** — the schema agrees: `subscription_event/$defs/PaneAgentStatusChangedEvent.properties` has
//! no `type`, and its `required` is `[pane_id, workspace_id, agent_status]`.
//!
//! So a single `#[serde(tag = "type")]` enum over `data` — which is exactly what herdr's own
//! `HERDR_API.md` ("the event field is snake_case") leads you to write — parses every lifecycle
//! frame and **silently errors on every ask**. That is the product failing at its only job,
//! quietly, in a way no test of the lifecycle frames can see. `tests/events.rs::`
//! `two_envelope_families_decode_from_one_stream` builds that broken decoder inline and proves it
//! drops the ask, so the trap cannot be re-introduced by a well-meaning refactor.
//!
//! Decode is therefore **two-step and tagged on the OUTER field**: read `event`, then dispatch.
//! This file is written decoder-first on purpose — [`decode_event`] and its helpers come before the
//! [`Event`] enum, because the real wire shape drove these types rather than the reverse.
//!
//! # THE OTHER TRAP: the roster family carries a stale status, and must not expose it
//!
//! `pane.updated` replays a rolling, ageing backlog on EVERY connect at ~100 ms/frame, each frame
//! carrying a *historical* `agent_status`. Two independent captures, so the two sets of numbers
//! below are not a contradiction: the replan's probe saw 30 frames in 2.93 s, and
//! `tests/fixtures/events-mixed.ndjson` — the one checked in, and the one the tests read — holds 27
//! of them, `wB:p1` replaying revisions 6→19 all reading `"blocked"` and `wD:p1` revisions 16→26
//! all reading `"working"`. It is the only globally-subscribable status-bearing event, so a bridge
//! that read status from it would fire a phantom-push burst on every reconnect. Because the backlog
//! *ages*, dedupe-by-frame-identity cannot save you either.
//!
//! [`RosterEvent`] therefore keeps **ids only**. The status is not merely ignored — it is
//! unrepresentable in the decoded type, which is what makes the phantom burst impossible to write.
//! Truth about status comes from `session.snapshot`, never from this family.

use std::collections::BTreeMap;
use std::sync::Once;

use serde::{Deserialize, Serialize};

use crate::error::HerdrError;
use crate::ids::{PaneId, WorkspaceId};
use crate::proto::model::AgentStatus;

/// The one wire name for the method that opens an event stream.
///
/// Lives here rather than in `client.rs` so the decoder, the stream's I/O errors and
/// `<EventsSubscribeRequest as Request>::METHOD` are all the SAME literal — a rename cannot leave
/// one of them behind.
pub(crate) const EVENTS_SUBSCRIBE: &str = "events.subscribe";

// ════════════════════════════════════════════════════════════════════════════════════════════════
// THE DECODER — deliberately above the types it produces.
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// Step one: the envelope, and NOTHING about `data`'s shape.
///
/// `data` stays a raw [`serde_json::Value`] because at this point we genuinely do not know which of
/// the two families it belongs to. Both schemas declare `required = ["event", "data"]`.
#[derive(Deserialize)]
struct RawEvent {
    event: String,
    data: serde_json::Value,
}

/// The seven lifecycle kinds this client models, as they appear in the OUTER `event` field.
///
/// This list is the gate that makes [`Event::Unrecognized`] safe: a kind that is not here and is
/// not one of the three dot-form kinds is bucketed, while a MALFORMED frame of a kind that IS here
/// still errors loudly. `tests::known_roster_and_the_decode_match_are_the_same_list` proves this
/// const and the match in [`decode_roster`] never drift apart.
///
/// The other 19 kinds in the schema's `EventKind` enum (tabs, worktrees, workspace renames, the
/// `pane_focused` firehose, `layout_updated`) are deliberately absent: nothing in the product reads
/// them, and bucketing costs one allocation.
pub(crate) const KNOWN_ROSTER: &[&str] = &[
    "pane_created",
    "pane_updated",
    "pane_closed",
    "pane_exited",
    "pane_agent_detected",
    "pane_moved",
    "workspace_closed",
];

static UNRECOGNIZED_EVENT_WARNED: Once = Once::new();

/// Decode one NDJSON line from an open event stream.
///
/// Two steps, and the order is the whole point:
///
/// 1. parse the envelope and read the **outer** `event` string;
/// 2. dispatch on it — dot-form kinds carry an untagged `data`, snake_case kinds carry a `data`
///    with a redundant `type`.
///
/// # What errors and what does not
///
/// * an unmodelled KIND → `Ok(`[`Event::Unrecognized`]`)`, warned once per process. This is the
///   forward-compat contract that keeps a bridge alive through a routine `herdr update`; it
///   replaces the `deny_unknown_fields` loudness, which is bought back offline in
///   `tests/schema_drift.rs`.
/// * a MALFORMED frame of a kind we DO claim to handle → [`HerdrError::Decode`], carrying the raw
///   line. The catch-all is gated on the kind and must never swallow real corruption.
pub fn decode_event(line: &str) -> Result<Event, HerdrError> {
    // STEP ONE — the envelope, knowing nothing about `data`.
    let RawEvent { event, data } =
        serde_json::from_str(line).map_err(|source| decode_error(source, line))?;

    // STEP TWO — classify on the OUTER `event` string, and on nothing else. Deliberately its own
    // step: the result borrows nothing from `event`, so step three can own both halves of the
    // frame and the two families never have to be told apart from inside `data`.
    enum Family {
        /// Dot-form. `data` carries NO `type`.
        AgentStatus,
        ScrollChanged,
        OutputMatched,
        /// snake_case lifecycle, `data` carries a redundant `type`. The payload is the matched
        /// `KNOWN_ROSTER` entry, which is `'static` and therefore free of `event`.
        Roster(&'static str),
        Unmodelled,
    }

    let family = match event.as_str() {
        // THE ONE THE PRODUCT EXISTS FOR. Reachable only per-pane: `pane.agent_status_changed`
        // requires a `pane_id` (3 of the schema's 27 subscription variants do), and
        // `events.subscribe` freezes the set at connect.
        "pane.agent_status_changed" => Family::AgentStatus,
        "pane.scroll_changed" => Family::ScrollChanged,
        "pane.output_matched" => Family::OutputMatched,
        // Note that the schema's `EventKind` also declares a snake_case
        // `pane_agent_status_changed` twin, which is NOT in `KNOWN_ROSTER` and so lands in
        // `Unmodelled`. That is right: it is not subscribable — verified live,
        // `{"type":"pane_agent_status_changed"}` → `invalid_request: unknown variant` — so it
        // cannot reach us. If herdr ever renamed the dot form to it, our own subscription would be
        // refused at subscribe time with that same loud error rather than silently starved.
        other => match KNOWN_ROSTER.iter().copied().find(|known| *known == other) {
            Some(kind) => Family::Roster(kind),
            None => Family::Unmodelled,
        },
    };

    // STEP THREE — decode `data` the way this family, and only this family, encodes it.
    match family {
        Family::AgentStatus => Ok(Event::AgentStatus(
            serde_json::from_value(data).map_err(|source| decode_error(source, line))?,
        )),
        // Opaque on purpose: nothing in slice 1 reads either payload, and modelling a shape we
        // never inspect would only be another thing to get wrong on a `herdr update`.
        Family::ScrollChanged => Ok(Event::ScrollChanged(data)),
        Family::OutputMatched => Ok(Event::OutputMatched(data)),
        Family::Roster(kind) => decode_roster(kind, event, data, line),
        // Never fatal. This is the forward-compat contract.
        Family::Unmodelled => Ok(unrecognized(event, data)),
    }
}

/// The lifecycle half of the dispatch. Ids only — see the module docs and [`RosterEvent`].
///
/// Takes the already-owned `event`/`data` so the fall-through arm can hand them to
/// [`Event::Unrecognized`] without cloning.
fn decode_roster(
    kind: &str,
    event: String,
    data: serde_json::Value,
    line: &str,
) -> Result<Event, HerdrError> {
    let err = |source: serde_json::Error| decode_error(source, line);

    let roster = match kind {
        // `pane_created` / `pane_updated` carry only `{type, pane}` — the ids live INSIDE the
        // embedded PaneInfo, which is also where the stale `agent_status` lives. `PaneIds` reads
        // the two ids and is structurally incapable of seeing anything else.
        "pane_created" => {
            let d: PaneCarrier = serde_json::from_value(data).map_err(err)?;
            RosterEvent::PaneCreated {
                pane_id: d.pane.pane_id,
                workspace_id: d.pane.workspace_id,
            }
        }
        "pane_updated" => {
            let d: PaneCarrier = serde_json::from_value(data).map_err(err)?;
            RosterEvent::PaneUpdated {
                pane_id: d.pane.pane_id,
                workspace_id: d.pane.workspace_id,
            }
        }
        // These two carry the ids directly (`required = [type, pane_id, workspace_id]`).
        "pane_closed" => {
            let d: PaneIds = serde_json::from_value(data).map_err(err)?;
            RosterEvent::PaneClosed {
                pane_id: d.pane_id,
                workspace_id: d.workspace_id,
            }
        }
        "pane_exited" => {
            let d: PaneIds = serde_json::from_value(data).map_err(err)?;
            RosterEvent::PaneExited {
                pane_id: d.pane_id,
                workspace_id: d.workspace_id,
            }
        }
        // `required = [type, previous_pane_id, previous_workspace_id, previous_tab_id, pane]`:
        // the NEW ids come from the embedded pane, the old pane_id is a top-level field.
        "pane_moved" => {
            let d: MovedCarrier = serde_json::from_value(data).map_err(err)?;
            RosterEvent::PaneMoved {
                previous_pane_id: d.previous_pane_id,
                pane_id: d.pane.pane_id,
                workspace_id: d.pane.workspace_id,
            }
        }
        "pane_agent_detected" => {
            let d: AgentDetected = serde_json::from_value(data).map_err(err)?;
            RosterEvent::PaneAgentDetected {
                pane_id: d.pane_id,
                workspace_id: d.workspace_id,
                agent: d.agent,
                released: d.released,
            }
        }
        "workspace_closed" => {
            let d: WorkspaceClosed = serde_json::from_value(data).map_err(err)?;
            RosterEvent::WorkspaceClosed {
                workspace_id: d.workspace_id,
            }
        }
        // Unreachable while `KNOWN_ROSTER` and this match agree, which
        // `tests::known_roster_and_the_decode_match_are_the_same_list` proves at `cargo test`.
        // Bucketed rather than `unreachable!()`: this is a data path in a bridge that has to
        // survive unattended, and a panic here would be a crash loop under `Restart=always`.
        _ => return Ok(unrecognized(event, data)),
    };
    Ok(Event::Roster(roster))
}

/// Bucket an unmodelled kind, warning **once** per process.
///
/// Once, not per frame: a `herdr update` that adds a chatty kind must not turn the bridge's own log
/// into a denial of service on a machine whose operator has only a phone.
fn unrecognized(event: String, data: serde_json::Value) -> Event {
    UNRECOGNIZED_EVENT_WARNED.call_once(|| {
        tracing::warn!(
            event = %event,
            "herdr sent an event kind this client does not model; bucketing it and carrying on \
             (this is the forward-compat path that keeps the bridge alive through a herdr update)"
        );
    });
    tracing::debug!(event = %event, "unmodelled herdr event kind");
    Event::Unrecognized { event, data }
}

/// Every decode failure on this path carries the raw frame: a decode error the operator cannot see
/// the bytes of is not actionable from a phone.
fn decode_error(source: serde_json::Error, line: &str) -> HerdrError {
    HerdrError::Decode {
        method: EVENTS_SUBSCRIBE,
        source,
        line: line.to_owned(),
    }
}

// ── the private shapes the roster decoder reads, and nothing more ───────────────────────────────
//
// THESE ARE THE STRUCTURAL GUARANTEE. `PaneIds` is the only view this crate ever takes of an
// embedded `PaneInfo` on the event path: it can see two ids and it cannot see `agent_status`, so
// the phantom-push burst is not something a later refactor can "helpfully" re-enable without
// deleting these types first.

/// Two ids lifted out of an embedded `PaneInfo`. Every other property — `agent_status` included —
/// is dropped by construction.
#[derive(Deserialize)]
struct PaneIds {
    pane_id: PaneId,
    workspace_id: WorkspaceId,
    // NO `agent_status`, and no other field, ever. serde skips unknown keys here by design: the
    // embedded `PaneInfo` on the wire carries a stale status and ~20 other properties, and this
    // struct's whole job is to be incapable of seeing any of them.
}

/// `{type, pane}` — `pane_created` and `pane_updated`.
#[derive(Deserialize)]
struct PaneCarrier {
    pane: PaneIds,
}

/// `{type, previous_pane_id, previous_workspace_id, previous_tab_id, pane}` — `pane_moved`.
///
/// `previous_workspace_id` / `previous_tab_id` / `closed_*` / `created_*` are on the wire and
/// deliberately unread: a moved pane's problem is that its `pane_id` changed, and that is what
/// sticky state has to migrate on.
#[derive(Deserialize)]
struct MovedCarrier {
    previous_pane_id: PaneId,
    pane: PaneIds,
}

/// `pane_agent_detected`.
///
/// The wire also carries `final_status` (`AgentStatus | null`) — **deliberately not read**. It is
/// the only other status-bearing field in the lifecycle family, and modelling it would reopen
/// exactly the hole [`RosterEvent`] exists to close, on the one kind whose replay a reader would
/// least expect to be stale.
#[derive(Deserialize)]
struct AgentDetected {
    pane_id: PaneId,
    workspace_id: WorkspaceId,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    released: Option<bool>,
}

/// `workspace_closed`. `workspace` (a full `WorkspaceInfo | null`) is present on the wire and
/// unread, for the same reason: a `WorkspaceInfo` carries an `agent_status` too.
#[derive(Deserialize)]
struct WorkspaceClosed {
    workspace_id: WorkspaceId,
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// THE TYPES THE DECODER PRODUCES
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// One decoded frame from an open event stream.
///
/// `#[non_exhaustive]`: a later slice modelling one more kind must not be a breaking change.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Event {
    /// **THE ONLY EVENT SLICE 3 MAY PUSH ON.** Dot-form outer `event`; `data` carries no `type`.
    ///
    /// Not replayed at subscribe *unless* the subscription pinned a matching status filter — see
    /// [`Subscription::agent_status`] — so unfiltered, these are the real edges.
    AgentStatus(AgentStatusChanged),
    /// `pane.scroll_changed`, opaque. Nothing in slice 1 reads it.
    ScrollChanged(serde_json::Value),
    /// `pane.output_matched`, opaque. Nothing in slice 1 reads it.
    OutputMatched(serde_json::Value),
    /// **CACHE-INVALIDATION POKE ONLY** — see [`RosterEvent`]. Never a push trigger.
    Roster(RosterEvent),
    /// A kind this client was not built for: a protocol-21 addition, or one of the 19 lifecycle
    /// kinds we do not model. Bucketed and logged once, never fatal — this is the property that
    /// keeps the bridge alive through a routine `herdr update`.
    Unrecognized {
        /// The outer `event` string, verbatim.
        event: String,
        /// The frame's `data`, carried through untouched so a later slice can model it without a
        /// re-capture.
        data: serde_json::Value,
    },
}

/// `pane.agent_status_changed` — the product's entire payload.
///
/// Verified `required = [pane_id, workspace_id, agent_status]`. `title`, `display_agent` and
/// `state_labels` are optional AND were absent from **every** live frame captured, so an ask
/// summary is NOT free here: slice 3's ask extraction has to come from `read_visible()`.
#[derive(Clone, Debug, Deserialize)]
pub struct AgentStatusChanged {
    /// The pane whose agent changed state.
    pub pane_id: PaneId,
    /// Carried by the frame, so a bridge need not consult the snapshot to route it.
    pub workspace_id: WorkspaceId,
    /// The new status. [`AgentStatus::is_indeterminate`] is the guard before any push.
    pub agent_status: AgentStatus,
    /// e.g. `"opencode"`. Present on every captured frame, still optional in the schema.
    #[serde(default)]
    pub agent: Option<String>,
    /// Absent from every captured frame.
    #[serde(default)]
    pub display_agent: Option<String>,
    /// Absent from every captured frame — do not plan an ask summary around it.
    #[serde(default)]
    pub title: Option<String>,
    /// Absent from every captured frame.
    #[serde(default)]
    pub state_labels: Option<BTreeMap<String, String>>,
}

/// A roster change: **ids only, never a status.**
///
/// The deserializer reads the embedded `PaneInfo` and throws it away, keeping the ids. That is not
/// tidiness — `pane.updated` replays an ageing backlog on every connect and each frame carries a
/// historical `agent_status`, so a bridge that read status from this family would fire a phantom-
/// push burst every time it reconnected. Making the status structurally unreadable here is what
/// makes that burst impossible to express. Truth comes from `session.snapshot`.
///
/// `tests/events.rs::roster_event_discards_pane_info` asserts this against the real captured
/// backlog, because it is easy to "helpfully" restore in a later refactor.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RosterEvent {
    /// A new pane appeared. `events.subscribe` freezes its set at connect and there is no
    /// `events.update`, so this is the signal to tear the stream down and re-open it with one more
    /// `pane.agent_status_changed` subscription.
    PaneCreated {
        pane_id: PaneId,
        workspace_id: WorkspaceId,
    },
    /// The backlog replayer, and the single most dangerous frame on this stream. A
    /// cache-invalidation poke and nothing else: it carries a *historical* `agent_status` on the
    /// wire, and this variant deliberately has nowhere to put it.
    PaneUpdated {
        pane_id: PaneId,
        workspace_id: WorkspaceId,
    },
    PaneClosed {
        pane_id: PaneId,
        workspace_id: WorkspaceId,
    },
    PaneExited {
        pane_id: PaneId,
        workspace_id: WorkspaceId,
    },
    /// A moved pane gets a **new** `pane_id` and the old one stops resolving, even though the agent
    /// is alive and the pane is not closed. `previous_pane_id` is what lets sticky state migrate
    /// silently instead of falling back to the picker.
    PaneMoved {
        previous_pane_id: PaneId,
        pane_id: PaneId,
        workspace_id: WorkspaceId,
    },
    /// `final_status` is on the wire here and is deliberately not modelled — see `AgentDetected`.
    PaneAgentDetected {
        pane_id: PaneId,
        workspace_id: WorkspaceId,
        agent: Option<String>,
        released: Option<bool>,
    },
    WorkspaceClosed {
        workspace_id: WorkspaceId,
    },
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// THE OUTBOUND HALF
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// One entry in `events.subscribe`'s `subscriptions` array.
///
/// Serializes **dot-form**, internally tagged on `type`. Verified live, both ways round:
/// the snake_case spelling is `invalid_request: unknown variant`, and
/// `{"type":"pane.agent_status_changed"}` without a `pane_id` is
/// `invalid_request: missing field 'pane_id'`.
///
/// Exactly **3 of the schema's 27** subscription variants take a `pane_id`
/// (`pane.output_matched`, `pane.agent_status_changed`, `pane.scroll_changed`) — so there is **no
/// global agent-status subscription**, and slice 3 must fan out one entry per agent pane.
///
/// `#[non_exhaustive]`: adding one of the other 18 wire variants must not be a breaking change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Subscription {
    /// The product's trigger. `pane_id` is REQUIRED and unrepresentable-if-missing.
    ///
    /// With `agent_status: Some(s)` the server **replays the pane's current status at subscribe
    /// time** if it already equals `s` — verified firing at t=0.00 for `idle` and for `working`,
    /// while both the unfiltered form and a non-matching filter fired nothing. That replay is
    /// proof gate 5's mechanism and slice 3's "laptop was asleep, recover the missed ask" path.
    #[serde(rename = "pane.agent_status_changed")]
    PaneAgentStatusChanged {
        pane_id: PaneId,
        /// Omitted entirely when `None` — herdr must see no `agent_status` key at all, not a null.
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_status: Option<AgentStatus>,
    },
    #[serde(rename = "pane.scroll_changed")]
    PaneScrollChanged { pane_id: PaneId },
    #[serde(rename = "pane.created")]
    PaneCreated,
    #[serde(rename = "pane.closed")]
    PaneClosed,
    #[serde(rename = "pane.exited")]
    PaneExited,
    #[serde(rename = "pane.moved")]
    PaneMoved,
    #[serde(rename = "pane.agent_detected")]
    PaneAgentDetected,
    #[serde(rename = "workspace.closed")]
    WorkspaceClosed,
    /// ⚠ **FIREHOSE** — ~10 frames/s observed with no user interaction. Never in a default set,
    /// never a push trigger. Present only so a later slice cannot "discover" it by re-deriving the
    /// wire name and wiring it up without reading this line.
    #[serde(rename = "pane.focused")]
    PaneFocused,
}

impl Subscription {
    /// A **filtered** subscription pinned to one status — what `watch --once` and proof gate 5 use.
    ///
    /// The filter is what makes the decoder provable read-only and herd-state-independent: if the
    /// pane is already in `status`, herdr replays it immediately at subscribe time, so no
    /// transition and no live agent activity is needed to see a frame.
    pub fn agent_status(pane: &PaneId, status: AgentStatus) -> Self {
        Subscription::PaneAgentStatusChanged {
            pane_id: pane.clone(),
            agent_status: Some(status),
        }
    }

    /// **Unfiltered** — fires only on real transitions, and never replays.
    pub fn agent_status_any(pane: &PaneId) -> Self {
        Subscription::PaneAgentStatusChanged {
            pane_id: pane.clone(),
            agent_status: None,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Unit tests that need to name the PRIVATE decode shapes or `KNOWN_ROSTER`. The spec's
// `tests/events.rs` table lives in `tests/events.rs` and drives the public API only.
// ───────────────────────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// The consistency proof that lets `decode_roster`'s fall-through arm be a bucket rather than
    /// an `unreachable!()`: every name in `KNOWN_ROSTER` really does have a decode arm, so the
    /// gate and the match cannot drift apart silently.
    #[test]
    fn known_roster_and_the_decode_match_are_the_same_list() {
        // A minimal but VALID body for each kind, so anything that reached the fall-through arm
        // would surface as `Unrecognized` rather than as a decode error.
        let body = |kind: &str| -> String {
            let pane = r#"{"pane_id":"w9:p1","workspace_id":"w9"}"#;
            let data = match kind {
                "pane_created" | "pane_updated" => format!(r#"{{"type":"{kind}","pane":{pane}}}"#),
                "pane_moved" => {
                    format!(r#"{{"type":"{kind}","previous_pane_id":"w8:p1","pane":{pane}}}"#)
                }
                "workspace_closed" => format!(r#"{{"type":"{kind}","workspace_id":"w9"}}"#),
                _ => format!(r#"{{"type":"{kind}","pane_id":"w9:p1","workspace_id":"w9"}}"#),
            };
            format!(r#"{{"event":"{kind}","data":{data}}}"#)
        };

        for kind in KNOWN_ROSTER {
            let decoded = decode_event(&body(kind))
                .unwrap_or_else(|e| panic!("`{kind}` is in KNOWN_ROSTER but does not decode: {e}"));
            assert!(
                matches!(decoded, Event::Roster(_)),
                "`{kind}` is in KNOWN_ROSTER but fell through to the bucket: {decoded:?}"
            );
        }

        assert_eq!(
            KNOWN_ROSTER.len(),
            7,
            "the lifecycle family this client models is seven kinds"
        );
    }

    /// The ids come out of the EMBEDDED pane for the two kinds that carry one, and off the top
    /// level for the rest. Getting this backwards would produce a decode error, not a wrong id —
    /// but only because `PaneIds` has no `#[serde(default)]`, which this pins.
    #[test]
    fn pane_ids_are_read_from_the_right_place_per_kind() {
        let embedded = r#"{"event":"pane_created","data":{"type":"pane_created","pane":{"pane_id":"wD:p1","workspace_id":"wD","agent_status":"working"}}}"#;
        assert_eq!(
            decode_roster_of(embedded),
            RosterEvent::PaneCreated {
                pane_id: PaneId::new("wD:p1"),
                workspace_id: WorkspaceId::new("wD"),
            }
        );

        let top_level = r#"{"event":"pane_closed","data":{"type":"pane_closed","pane_id":"wD:p1","workspace_id":"wD"}}"#;
        assert_eq!(
            decode_roster_of(top_level),
            RosterEvent::PaneClosed {
                pane_id: PaneId::new("wD:p1"),
                workspace_id: WorkspaceId::new("wD"),
            }
        );

        // `pane_moved`: NEW ids from the embedded pane, old id from the top level.
        let moved = r#"{"event":"pane_moved","data":{"type":"pane_moved","previous_pane_id":"wC:p1","previous_workspace_id":"wC","previous_tab_id":"wC:t1","pane":{"pane_id":"wD:p2","workspace_id":"wD","agent_status":"blocked"}}}"#;
        assert_eq!(
            decode_roster_of(moved),
            RosterEvent::PaneMoved {
                previous_pane_id: PaneId::new("wC:p1"),
                pane_id: PaneId::new("wD:p2"),
                workspace_id: WorkspaceId::new("wD"),
            }
        );

        // `pane_agent_detected` keeps `agent` and `released` — and NOT `final_status`, which is on
        // the wire right here and must not appear in the decoded value.
        let detected = r#"{"event":"pane_agent_detected","data":{"type":"pane_agent_detected","pane_id":"wD:p1","workspace_id":"wD","agent":"opencode","released":true,"final_status":"blocked"}}"#;
        let ev = decode_roster_of(detected);
        assert_eq!(
            ev,
            RosterEvent::PaneAgentDetected {
                pane_id: PaneId::new("wD:p1"),
                workspace_id: WorkspaceId::new("wD"),
                agent: Some("opencode".to_owned()),
                released: Some(true),
            }
        );
        assert!(
            !format!("{ev:?}").contains("blocked"),
            "`final_status` reached the decoded roster event: {ev:?}"
        );
    }

    fn decode_roster_of(line: &str) -> RosterEvent {
        match decode_event(line).expect("well-formed roster frame") {
            Event::Roster(r) => r,
            other => panic!("expected a roster event, got {other:?}"),
        }
    }
}

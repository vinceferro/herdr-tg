//! The reply envelope and the per-method result **wrappers**.
//!
//! # Result payloads are NESTED under a per-method key — never flat
//!
//! Verified live on all six methods this client calls:
//!
//! ```text
//! session.snapshot -> {"type":"session_snapshot","snapshot":{…}}
//! pane.read        -> {"type":"pane_read","read":{…}}
//! agent.list       -> {"type":"agent_list","agents":[…]}
//! pane.list        -> {"type":"pane_list","panes":[…]}
//! ping             -> {"type":"pong","version":"0.8.2","protocol":20,"capabilities":{…}}
//! the three writes -> {"type":"ok"}
//! ```
//!
//! A flat model — deserializing `result` straight into `SessionSnapshot` — breaks on **every**
//! call, so each method models its own wrapper and `client::call` unwraps exactly one level.
//!
//! Crate-private: these are the transport's shape, not the caller's. Nothing outside this crate
//! should ever have to know that `agents` sits under `result.agents`.

use serde::Deserialize;

use crate::error::ErrorCode;
use crate::proto::model::{AgentInfo, PaneInfo, PaneRead, SessionSnapshot};

/// One reply line. Exactly one of `result` / `error` is present.
#[derive(Debug, Deserialize)]
pub(crate) struct Reply {
    /// **NEVER COMPARED.** Verified: semantic errors echo the request id (`{"id":"probe","error":
    /// {"code":"pane_not_found"…}}`) while parse and routing errors blank it to `""`
    /// (`{"id":"","error":{"code":"invalid_request"…}}`). Because RPC is strictly one-shot, the
    /// CONNECTION is the correlation — an `assert_eq!(reply.id, sent.id)` would misclassify every
    /// `invalid_request` as a framing bug and hide the real message, which is the one thing an
    /// operator holding only a phone needs to read.
    ///
    /// Kept on the struct anyway so it survives into a `Decode` error's raw line, and so nobody
    /// "discovers" it is missing and adds correlation back.
    ///
    /// `allow(dead_code)` IS the invariant here, not an oversight: nothing in the production build
    /// reads this field, and the day something does, this attribute is the thing that has to be
    /// deleted first.
    #[allow(dead_code)]
    #[serde(default)]
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<ErrorBody>,
}

impl Reply {
    /// The `result.type` tag, if the reply carried a result object with a string `type`.
    ///
    /// Used to check the tag against `Request::RESULT_TAG` so a method that answers with the wrong
    /// shape yields `UnexpectedResult` naming both tags, rather than a serde error pointing at a
    /// field the operator has never heard of.
    pub fn result_tag(&self) -> Option<&str> {
        self.result.as_ref()?.get("type")?.as_str()
    }
}

/// herdr's semantic refusal. `code` is an OPEN string in the schema — verified
/// `{"code":{"type":"string"},"message":{"type":"string"}}` with no enum — so it decodes through
/// [`ErrorCode::from_wire`], which never fails.
#[derive(Debug, Deserialize)]
pub(crate) struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

// ── the per-method result wrappers ──────────────────────────────────────────────────────────────
//
// `type` is present on the wire and deliberately absent here: it is read off the raw `Value` by
// `Reply::result_tag` before the payload is unwrapped, so modelling it twice would only create a
// second place for the tag check to disagree with itself.

/// `session.snapshot` -> `{"type":"session_snapshot","snapshot":{…}}`
#[derive(Debug, Deserialize)]
pub(crate) struct SnapshotResult {
    pub snapshot: SessionSnapshot,
}

/// `pane.read` -> `{"type":"pane_read","read":{…}}`
#[derive(Debug, Deserialize)]
pub(crate) struct ReadResult {
    pub read: PaneRead,
}

/// `agent.list` -> `{"type":"agent_list","agents":[…]}`
#[derive(Debug, Deserialize)]
pub(crate) struct AgentListResult {
    pub agents: Vec<AgentInfo>,
}

/// `pane.list` -> `{"type":"pane_list","panes":[…]}`
#[derive(Debug, Deserialize)]
pub(crate) struct PaneListResult {
    pub panes: Vec<PaneInfo>,
}

/// The three write methods and `events.subscribe`'s ack -> a bare `{"type":"ok"}` /
/// `{"type":"subscription_started"}`, with no payload at all.
///
/// **`ok` carries NO delivery semantics.** It means herdr took the bytes — not that the agent
/// received, rendered, parsed or acted on them. Slice 3's Telegram confirmation must say
/// "accepted", never "delivered", which is why the client's write methods return `WriteAccepted`
/// and not `bool`.
///
/// Constructed by `client::subscribe` (the `subscription_started` ack) and by all three write
/// methods (`ok`). `ok` is the only VOID tag among the schema's 58 `ResponseResult` variants, which
/// is where the write tag comes from — it is INFERRED, never observed, because observing it means
/// typing into a real pane (`scripts/verify-send-p20.sh` P1).
#[derive(Debug, Deserialize)]
pub(crate) struct OkResult {}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT: &str = include_str!("../../tests/fixtures/snapshot.json");
    const PANE_READ: &str = include_str!("../../tests/fixtures/pane_read.json");
    const ERRORS: &str = include_str!("../../tests/fixtures/errors.ndjson");

    fn reply(line: &str) -> Reply {
        serde_json::from_str(line).expect("fixture is a well-formed reply")
    }

    #[test]
    fn result_payloads_are_nested_under_a_per_method_key() {
        // The whole point of the wrapper types: `result` is NOT a SessionSnapshot.
        let r = reply(SNAPSHOT);
        assert_eq!(r.result_tag(), Some("session_snapshot"));
        assert!(r.error.is_none());
        let raw = r.result.clone().unwrap();
        assert!(
            serde_json::from_value::<SessionSnapshot>(raw.clone()).is_err(),
            "a FLAT model must not parse — if it ever does, the wire changed shape"
        );
        let wrapped: SnapshotResult = serde_json::from_value(raw).unwrap();
        assert_eq!(wrapped.snapshot.protocol, 20);
        assert_eq!(wrapped.snapshot.panes.len(), 6);

        let r = reply(PANE_READ);
        assert_eq!(r.result_tag(), Some("pane_read"));
        let wrapped: ReadResult = serde_json::from_value(r.result.unwrap()).unwrap();
        assert_eq!(wrapped.read.pane_id.as_str(), "w9:p1");
    }

    #[test]
    fn list_results_are_nested_too() {
        // `agent.list` / `pane.list` were not captured as standalone fixtures (the capture script
        // uses `agent.list` only to re-pin an event filter), so the payloads are lifted verbatim
        // from the snapshot fixture — the same shape herdr puts in both places.
        let snap: SnapshotResult = serde_json::from_value(reply(SNAPSHOT).result.unwrap()).unwrap();
        let agents = serde_json::to_value(&snap.snapshot.agents).unwrap();
        let panes = serde_json::to_value(&snap.snapshot.panes).unwrap();

        let wrapped: AgentListResult =
            serde_json::from_value(serde_json::json!({"type":"agent_list","agents":agents}))
                .unwrap();
        assert_eq!(wrapped.agents.len(), 6);

        let wrapped: PaneListResult =
            serde_json::from_value(serde_json::json!({"type":"pane_list","panes":panes})).unwrap();
        assert_eq!(wrapped.panes.len(), 6);
    }

    #[test]
    fn ok_and_subscription_started_are_bare_tags() {
        let r = reply(r#"{"id":"x","result":{"type":"ok"}}"#);
        assert_eq!(r.result_tag(), Some("ok"));
        // Deserializing the payload must succeed even though there is nothing under the tag.
        let _: OkResult = serde_json::from_value(r.result.unwrap()).unwrap();

        let r = reply(r#"{"id":"x","result":{"type":"subscription_started"}}"#);
        assert_eq!(r.result_tag(), Some("subscription_started"));
        let _: OkResult = serde_json::from_value(r.result.unwrap()).unwrap();
    }

    #[test]
    fn an_error_reply_has_no_result_and_keeps_its_code_and_message() {
        let mut lines = ERRORS.lines();

        // Semantic error: id is ECHOED.
        let r = reply(lines.next().unwrap());
        assert_eq!(r.id, "probe");
        assert!(r.result.is_none());
        let e = r.error.expect("error body");
        assert_eq!(e.code, ErrorCode::PaneNotFound);
        assert_eq!(e.message, "pane zz:p9 not found");

        // Parse/routing error: id is BLANKED. Both must decode identically well.
        let r = reply(lines.next().unwrap());
        assert_eq!(r.id, "");
        assert!(r.result.is_none());
        let e = r.error.expect("error body");
        assert_eq!(e.code, ErrorCode::InvalidRequest);
        assert!(
            e.message
                .starts_with("invalid request: missing field `params`")
        );
    }

    #[test]
    fn an_unknown_error_code_becomes_other_and_keeps_the_message() {
        let r = reply(
            r#"{"id":"","error":{"code":"future_code_2027","message":"herdr grew a new refusal"}}"#,
        );
        let e = r.error.unwrap();
        assert_eq!(e.code, ErrorCode::Other("future_code_2027".to_owned()));
        assert_eq!(e.message, "herdr grew a new refusal");
    }

    #[test]
    fn a_missing_id_still_decodes() {
        // Nothing observed omits `id`, but the field is `#[serde(default)]` precisely so a reply
        // that did would not become a decode failure on a path that must never wedge.
        let r = reply(r#"{"result":{"type":"ok"}}"#);
        assert_eq!(r.id, "");
        assert_eq!(r.result_tag(), Some("ok"));
    }

    #[test]
    fn result_tag_is_none_when_there_is_no_result_object() {
        let r = reply(r#"{"id":"","error":{"code":"ui_busy","message":"busy"}}"#);
        assert_eq!(r.result_tag(), None);
    }
}

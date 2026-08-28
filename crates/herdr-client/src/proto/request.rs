//! The request envelope, the params types, and the request id.
//!
//! # `id` and `params` are BOTH mandatory, and `params` is emitted even when empty
//!
//! Verified live against herdr 0.8.2 / protocol 20:
//!
//! ```text
//! {"id":"b","method":"ping"}            -> invalid_request: missing field `params`
//! {"method":"ping","params":{}}         -> invalid_request: missing field `id`
//! {"id":7,"method":"ping","params":{}}  -> invalid_request: invalid type: integer
//! ```
//!
//! So `#[serde(skip_serializing_if = …)]` must NEVER be applied to [`Envelope::params`], and `id`
//! must serialize as a JSON **string**. The schema agrees: `request` requires `id`, and every one
//! of its 91 method variants requires both `method` and `params`.
//!
//! # The id is never correlated
//!
//! herdr echoes the id on a semantic refusal and blanks it to `""` on a parse/routing refusal, so
//! comparing it would misclassify every `invalid_request` as a framing bug and hide the message the
//! operator actually needs. RPC is one-shot: the CONNECTION is the correlation. The id exists only
//! so herdr's own logs can be lined up with ours.
//!
//! Crate-private: these are the transport's shape. The public surface is the typed methods on
//! [`crate::client::HerdrClient`].

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::ids::{PaneId, WorkspaceId};
use crate::keys::Key;
use crate::proto::event::Subscription;
use crate::proto::model::ReadSource;

/// One request line: `{"id":…,"method":…,"params":{…}}`, in that field order.
///
/// `params` carries no `skip_serializing_if` and never will — see the module docs.
#[derive(Debug, Serialize)]
pub(crate) struct Envelope<'a, P: Serialize> {
    pub id: &'a str,
    pub method: &'static str,
    pub params: &'a P,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A fresh request id. A plain decimal counter rendered as a **string** — herdr rejects an integer
/// id (`invalid type: integer`), and nothing in this crate ever reads the id back.
pub(crate) fn next_request_id() -> String {
    NEXT_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

/// `{}` — the params of a method that takes none. Serializes as an empty **object**, never as
/// `null` and never omitted.
///
/// A Rust unit struct would serialize as `null`, which herdr rejects; the braces are what make this
/// an object.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub(crate) struct EmptyParams {}

/// `ping` — the real capability handshake. Not documented in `HERDR_API.md` at all.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(transparent)]
pub(crate) struct PingRequest(EmptyParams);

/// `session.snapshot` — the whole herd in one RPC.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(transparent)]
pub(crate) struct SnapshotRequest(EmptyParams);

/// `agent.list` — the agent roster only.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(transparent)]
pub(crate) struct AgentListRequest(EmptyParams);

/// `pane.list`, optionally scoped to one workspace.
///
/// D2 (one bot per workspace) scopes the roster **server-side**: an unknown id returns a distinct
/// `workspace_not_found`, so a bot whose workspace closed says so rather than reporting an empty
/// herd. `workspace_id` is omitted when absent — the schema types it `["string","null"]` and
/// declares no `required`.
#[derive(Debug, Serialize)]
pub(crate) struct PaneListRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<&'a WorkspaceId>,
}

/// `pane.read`.
///
/// **`source` is always [`ReadSource::Visible`] and there is no constructor that can say otherwise.**
/// `PaneReadParams.required` is `["pane_id","source"]`, so the field cannot be omitted to inherit
/// the CLI's `recent` default — which is exactly the point: `recent` harvest-scrolls the operator's
/// real viewport when `lines > viewport_rows`, and the two constructors below are the only ways to
/// build this struct.
///
/// `format` and `strip_ansi` are omitted so the server's own defaults (`text` / `true`) apply.
#[derive(Debug, Serialize)]
pub(crate) struct PaneReadRequest<'a> {
    pub pane_id: &'a PaneId,
    pub source: ReadSource,
    /// Omitted entirely for a plain visible read, so the request cannot even name a line count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
}

impl<'a> PaneReadRequest<'a> {
    /// The safe background read: `{"pane_id":…,"source":"visible"}` with **no** `lines` key.
    pub(crate) fn visible(pane_id: &'a PaneId) -> Self {
        PaneReadRequest {
            pane_id,
            source: ReadSource::Visible,
            lines: None,
        }
    }

    /// Also safe: `visible` is clamped to the viewport however large `lines` is (verified:
    /// `lines=200` on a 63-row viewport returned the full text with `truncated:false`).
    ///
    /// `NonZeroU32` because `lines=0` returns an empty string with `truncated:true`, which is a
    /// silently useless read rather than an error.
    pub(crate) fn visible_tail(pane_id: &'a PaneId, lines: NonZeroU32) -> Self {
        PaneReadRequest {
            pane_id,
            source: ReadSource::Visible,
            lines: Some(lines.get()),
        }
    }
}

// ── writes ──────────────────────────────────────────────────────────────────────────────────────
// ⚠ NO LIVE CALL SITE. These three types exist, are serialized correctly and are mock-tested; no
// code path outside `#[cfg(test)]` constructs one, and the binary exposes no subcommand that
// reaches `HerdrClient::send_text` / `send_keys` / `send_input`. That is D3's catastrophic-failure
// guard — the failure mode of a remote-control surface is words landing in the wrong terminal —
// and it is structural, not a convention. See `crates/herdr-client/src/keys.rs` for the
// UNVERIFIED-ON-P20 banner on the key grammar, and `scripts/verify-send-p20.sh` for the live
// verification that is deliberately deferred to a throwaway probe session.

/// `pane.send_text` — text only, no submit.
///
/// Schema: `PaneSendTextParams.required = ["pane_id","text"]`, so BOTH fields are always emitted.
///
/// `HERDR_API.md`'s 0.7.4 finding is that this writes **raw** bytes: a `\n` inside `text` is a real
/// Enter at the PTY, not pasted content. Never retested on 0.8.2, and multi-line replies are this
/// product's default case — `scripts/verify-send-p20.sh` P3 settles it before slice 3 sends
/// anything. The type does not sanitize: silently rewriting an operator's text would be a worse
/// surprise than the one it prevents, and the caller (slice 3) is the layer that knows whether a
/// newline was meant.
#[derive(Debug, Serialize)]
pub(crate) struct PaneSendTextRequest<'a> {
    pub pane_id: &'a PaneId,
    pub text: &'a str,
}

/// `pane.send_keys` — keys only.
///
/// Schema: `PaneSendKeysParams.required = ["pane_id","keys"]`, so `keys` is emitted even when
/// empty (an empty array is a well-formed no-op; omitting the field is `invalid_request`).
///
/// The key grammar is UNVERIFIED-ON-P20 — see [`crate::keys`]. [`Key`] guarantees only that no
/// element is empty, whitespace-only, or carries a raw newline; the server's validator is the
/// authority and answers `invalid_key: unsupported key <X>`.
#[derive(Debug, Serialize)]
pub(crate) struct PaneSendKeysRequest<'a> {
    pub pane_id: &'a PaneId,
    pub keys: &'a [Key],
}

/// `pane.send_input` — protocol 20's atomic text+keys in ONE RPC.
///
/// Schema: `PaneSendInputParams.required = ["pane_id"]` only, with `text` and `keys` both optional.
/// Both are therefore OMITTED when absent rather than sent as `null` / `[]`, which is what makes
/// "text only", "keys only" and "text then keys" three shapes of one request.
///
/// This is slice 3's intended product path: it collapses the `send_text` → `send_keys` pair and
/// removes the ordering question entirely. It replaces `agent.send`, which was REMOVED between
/// protocol 16 and 20.
#[derive(Debug, Serialize)]
pub(crate) struct PaneSendInputRequest<'a> {
    pub pane_id: &'a PaneId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>,
    #[serde(skip_serializing_if = "<[Key]>::is_empty")]
    pub keys: &'a [Key],
}

/// `events.subscribe`.
///
/// **The params are an OBJECT with a `subscriptions` key, never a bare array** — verified against
/// the schema (`EventsSubscribeParams.required = ["subscriptions"]`) and on the wire during fixture
/// capture. Getting this wrong is an `invalid_request`, which is at least loud.
///
/// The set is FROZEN at connect: there is no `events.update`, so a pane created later needs the
/// stream torn down and re-opened. `EventStream::subscriptions()` exists so the reconnect loop can
/// re-issue this list verbatim.
#[derive(Debug, Serialize)]
pub(crate) struct EventsSubscribeRequest<'a> {
    pub subscriptions: &'a [Subscription],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json<P: Serialize>(method: &'static str, params: &P) -> serde_json::Value {
        let id = next_request_id();
        let envelope = Envelope {
            id: &id,
            method,
            params,
        };
        serde_json::to_value(&envelope).expect("an envelope of plain data always serializes")
    }

    #[test]
    fn empty_params_is_an_object_not_null_and_not_omitted() {
        assert_eq!(
            serde_json::to_string(&EmptyParams {}).unwrap(),
            "{}",
            "a unit struct would serialize as `null`, which herdr rejects"
        );

        let v = json("ping", &PingRequest::default());
        assert_eq!(v["params"], serde_json::json!({}));
        assert!(
            v.get("params").is_some(),
            "omitting params is `invalid_request: missing field params` live"
        );

        // The raw line, because that is what actually goes on the wire.
        let id = next_request_id();
        let raw = serde_json::to_string(&Envelope {
            id: &id,
            method: "ping",
            params: &PingRequest::default(),
        })
        .unwrap();
        assert!(raw.contains(r#""params":{}"#), "{raw}");
        assert!(raw.starts_with(r#"{"id":"#), "id comes first: {raw}");
    }

    #[test]
    fn the_id_is_always_a_json_string_and_is_unique_per_call() {
        let a = json("ping", &PingRequest::default());
        let b = json("ping", &PingRequest::default());
        assert!(
            a["id"].is_string(),
            "an integer id is `invalid type: integer`"
        );
        assert!(b["id"].is_string());
        assert_ne!(a["id"], b["id"]);
    }

    #[test]
    fn pane_read_visible_omits_lines_and_pins_the_source() {
        let pane = PaneId::new("w9:p1");

        let v = json("pane.read", &PaneReadRequest::visible(&pane));
        assert_eq!(v["params"]["pane_id"], "w9:p1");
        assert_eq!(v["params"]["source"], "visible");
        assert!(
            v["params"].get("lines").is_none(),
            "a plain visible read must not carry a line count at all: {v}"
        );

        let v = json(
            "pane.read",
            &PaneReadRequest::visible_tail(&pane, NonZeroU32::new(40).unwrap()),
        );
        assert_eq!(v["params"]["source"], "visible");
        assert_eq!(v["params"]["lines"], 40);
    }

    #[test]
    fn pane_list_omits_the_workspace_when_unscoped() {
        let v = json("pane.list", &PaneListRequest { workspace_id: None });
        assert_eq!(v["params"], serde_json::json!({}));

        let ws = WorkspaceId::new("wD");
        let v = json(
            "pane.list",
            &PaneListRequest {
                workspace_id: Some(&ws),
            },
        );
        assert_eq!(v["params"]["workspace_id"], "wD");
    }

    #[test]
    fn events_subscribe_params_are_an_object_not_a_bare_array() {
        let pane = PaneId::new("wA:p1");
        let subs = [
            Subscription::agent_status(&pane, crate::proto::model::AgentStatus::Blocked),
            Subscription::PaneCreated,
        ];
        let v = json(
            "events.subscribe",
            &EventsSubscribeRequest {
                subscriptions: &subs,
            },
        );
        assert!(
            v["params"].is_object(),
            "EventsSubscribeParams.required = [\"subscriptions\"]; a bare array is \
             invalid_request: {v}"
        );
        assert_eq!(v["params"]["subscriptions"].as_array().unwrap().len(), 2);
        assert_eq!(
            v["params"]["subscriptions"][0]["type"],
            "pane.agent_status_changed"
        );
        assert_eq!(v["params"]["subscriptions"][1]["type"], "pane.created");
    }

    /// `PaneSendTextParams.required = ["pane_id","text"]` and
    /// `PaneSendKeysParams.required = ["pane_id","keys"]` — neither optional field exists, so both
    /// are always emitted. `keys` is emitted even when EMPTY: omitting a required field is
    /// `invalid_request`, while an empty array is a well-formed no-op.
    #[test]
    fn send_text_and_send_keys_always_emit_their_required_fields() {
        let pane = PaneId::new("wZ:p9");

        let v = json(
            "pane.send_text",
            &PaneSendTextRequest {
                pane_id: &pane,
                text: "",
            },
        );
        assert_eq!(v["params"]["pane_id"], "wZ:p9");
        assert_eq!(v["params"]["text"], "");
        assert_eq!(
            v["params"].as_object().unwrap().len(),
            2,
            "send_text has exactly two params: {v}"
        );

        let v = json(
            "pane.send_keys",
            &PaneSendKeysRequest {
                pane_id: &pane,
                keys: &[],
            },
        );
        assert_eq!(
            v["params"]["keys"],
            serde_json::json!([]),
            "`keys` is REQUIRED; an empty array is a no-op, an absent key is invalid_request: {v}"
        );

        let keys = [Key::parse("2").unwrap(), Key::enter()];
        let v = json(
            "pane.send_keys",
            &PaneSendKeysRequest {
                pane_id: &pane,
                keys: &keys,
            },
        );
        assert_eq!(
            v["params"]["keys"],
            serde_json::json!(["2", "Enter"]),
            "the Key newtype must serialize transparently: {v}"
        );
    }

    /// `PaneSendInputParams.required = ["pane_id"]` — `text` and `keys` are BOTH optional, and an
    /// absent one is omitted rather than sent as `null` / `[]`. That is what makes text-only,
    /// keys-only and text-then-keys three shapes of one RPC (protocol 20's replacement for the
    /// removed `agent.send`).
    #[test]
    fn send_input_omits_whatever_it_was_not_given() {
        let pane = PaneId::new("wZ:p9");
        let keys = [Key::enter()];

        let v = json(
            "pane.send_input",
            &PaneSendInputRequest {
                pane_id: &pane,
                text: None,
                keys: &[],
            },
        );
        assert_eq!(
            v["params"],
            serde_json::json!({"pane_id": "wZ:p9"}),
            "only pane_id is required, so a bare send_input is exactly one key: {v}"
        );

        let v = json(
            "pane.send_input",
            &PaneSendInputRequest {
                pane_id: &pane,
                text: Some("ship it"),
                keys: &[],
            },
        );
        assert_eq!(v["params"]["text"], "ship it");
        assert!(
            v["params"].get("keys").is_none(),
            "an empty key list must be OMITTED, not sent as []: {v}"
        );

        let v = json(
            "pane.send_input",
            &PaneSendInputRequest {
                pane_id: &pane,
                text: None,
                keys: &keys,
            },
        );
        assert!(
            v["params"].get("text").is_none(),
            "absent text must be omitted, never null: {v}"
        );
        assert_eq!(v["params"]["keys"], serde_json::json!(["Enter"]));

        let v = json(
            "pane.send_input",
            &PaneSendInputRequest {
                pane_id: &pane,
                text: Some("ship it"),
                keys: &keys,
            },
        );
        assert_eq!(v["params"]["text"], "ship it");
        assert_eq!(v["params"]["keys"], serde_json::json!(["Enter"]));
    }

    /// A `Key` cannot carry a raw newline, so the framing invariant holds on the write path too —
    /// and a newline inside `text` survives as a JSON ESCAPE, never as a second request line.
    /// (Whether that escaped `\n` then executes at the PTY is `verify-send-p20.sh` P3's question,
    /// and a different hazard entirely.)
    #[test]
    fn a_write_body_never_contains_a_raw_newline_either() {
        let pane = PaneId::new("wZ:p9");
        let id = next_request_id();
        let raw = serde_json::to_string(&Envelope {
            id: &id,
            method: "pane.send_text",
            params: &PaneSendTextRequest {
                pane_id: &pane,
                text: "line one\nline two",
            },
        })
        .unwrap();
        assert!(!raw.contains('\n'), "{raw}");
        assert!(
            raw.contains(r"\n"),
            "the newline must survive as an ESCAPE: {raw}"
        );

        assert!(
            Key::parse("Enter\n").is_err(),
            "the Key newtype is what keeps a raw newline out of the keys array"
        );
    }

    #[test]
    fn a_request_body_never_contains_a_raw_newline() {
        // `transport::round_trip` debug_asserts this; it is the framing invariant. Compact
        // serde_json escapes newlines inside strings, so the only way to break it is a hand-built
        // body.
        let pane = PaneId::new("w9:p1\nnot-a-second-request");
        let id = next_request_id();
        let raw = serde_json::to_string(&Envelope {
            id: &id,
            method: "pane.read",
            params: &PaneReadRequest::visible(&pane),
        })
        .unwrap();
        assert!(!raw.contains('\n'), "{raw}");
        assert!(
            raw.contains(r"\n"),
            "the newline must survive as an ESCAPE: {raw}"
        );
    }
}

//! The frames, and the envelope every one of them travels in.
//!
//! # Authority flows one way
//!
//! The hub decides where a bridge's words go. A bridge decides nothing about the hub. That is why
//! no frame here carries addressing: there is no `project` field on `say`, no `chat_id` on `ask`,
//! no topic anywhere. The hub knows which connection is which project because it resolved a token
//! at `hello`, and a bridge that tries to name a project is refused.
//!
//! `hello` is the proof: it carries `repo` and `instance` for the audit log and for a human
//! reading it, and it deliberately does NOT carry a display name. The name comes from the
//! registry. A bridge that could name itself could impersonate another project's topic.
//!
//! # Version skew is first-class
//!
//! The hub and the bridge ship from different repositories and are updated on different days, so
//! skew is the normal case rather than the exceptional one. Three rules, and all three are
//! deliberately different:
//!
//! * A major `v` mismatch on `hello` is refused, naming the command that fixes it.
//! * An unknown frame kind is logged and ignored — [`BridgeFrame::Unknown`] exists so that
//!   receiving one is a value this code can hold, not a parse error that kills the connection.
//! * An unknown field inside a known kind is ignored, which is serde's default and is left alone
//!   on purpose: `deny_unknown_fields` here would turn every additive change on the other side
//!   into a dead worker.

use serde::{Deserialize, Serialize};

use crate::ids::{AskId, FrameId, MsgId, OptionId, ProjectId};

/// The protocol version carried in every envelope's `v`.
///
/// A single integer, not a semver triple: the only distinction that changes behaviour is
/// "can these two speak at all", and a second number invites a compatibility matrix nobody
/// maintains.
pub const VERSION: u16 = 1;

/// Every frame, in both directions, is one of these objects on one line.
///
/// `#[serde(flatten)]` on the payload puts `v`, `id` and the payload's own `t` in one flat object,
/// which is what the wire spec says. It also means unknown fields are ignored rather than
/// rejected, which is the version-skew rule above.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope<P> {
    /// Protocol version. See [`VERSION`].
    pub v: u16,
    /// Opaque, per-connection, monotonic. What an `ack` refers back to.
    pub id: FrameId,
    #[serde(flatten)]
    pub payload: P,
}

impl<P> Envelope<P> {
    /// Wraps a payload at the current version.
    pub fn new(id: FrameId, payload: P) -> Self {
        Self {
            v: VERSION,
            id,
            payload,
        }
    }
}

/// How a send ended, in the only three values that can be told apart.
///
/// Two values would be a lie. Telegram has no idempotency key, so a send that times out may or may
/// not have landed, and there is no way to ask. `Unseen` is that state named. It is the surviving
/// principle of a four-rung delivery ladder that used to be two thousand lines: never claim a rung
/// you did not observe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Delivered {
    /// Observed to have landed.
    Yes,
    /// Observed not to have landed.
    No,
    /// Went out and could not be checked. Never retried when the message carried buttons: two live
    /// menus for one question, both tappable forever, is a misfire this system would have built.
    Unseen,
}

/// Why the hub did not do what a frame asked. A closed set, because the bridge branches on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AckWhy {
    /// The project's own rate budget refused it.
    TooFast,
    /// It was sent, but shortened to fit.
    Clamped,
    /// There is no topic to put it in.
    NoTopic,
    /// Telegram itself refused it.
    TelegramRefused,
}

/// Why the hub refused a connection outright. The frame is followed by a close.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusedReason {
    /// The token resolved to nothing. Enrolment is terminal-only.
    UnknownProject,
    /// The token did not match the registry's hash.
    BadToken,
    /// Another live connection already holds this project.
    ///
    /// A refusal, never a takeover. A takeover is what bridge-murder felt like from the inside:
    /// the incumbent kept running and stopped being heard.
    AlreadyClaimed,
    /// The major version does not match.
    VersionSkew,
    /// Enrolled, but not switched on.
    NotEnabled,
    /// The frame was larger than the ceiling. Never truncated — a half message is worse than none.
    FrameTooLarge,
    /// The `lane` on the `hello` is not a name the hub will address a conversation by.
    ///
    /// Safe to add to a closed set the bridge branches on, because only a bridge that SENT a lane
    /// can ever receive it — and a bridge old enough not to know the word cannot send one. It is
    /// permanent, not temporary: the same lane name will be refused every time, so a bridge that
    /// treats an unknown reason as worth retrying must be taught this one or it spins for ever.
    BadLane,
}

/// One answer button, as the bridge minted it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AskOption {
    /// Opaque. What comes back in a [`HubFrame::Choice`].
    pub option_id: OptionId,
    /// What the operator reads on the button.
    pub label: String,
}

/// What a `say` is, so the hub can render it without guessing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SayHint {
    /// Sentences meant for a person.
    Prose,
    /// A command's output. Monospace, and clipped rather than reflowed.
    Output,
}

/// What an agent is doing, as the bridge sees it from inside its own turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BeatState {
    Working,
    Idle,
    Blocked,
    Done,
}

/// How an ask stopped being open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AskEnd {
    /// Someone answered it, possibly at the terminal rather than on the phone.
    Answered,
    /// The agent stopped asking.
    Withdrawn,
    /// It aged out.
    Timeout,
}

/// Bridge → hub.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum BridgeFrame {
    /// First frame on a connection, exactly once.
    ///
    /// No display name, by design — see the module docs.
    Hello {
        project_id: ProjectId,
        /// The enrolment secret. Compared against a stored hash, in constant time.
        token: String,
        /// This run of the worker. A new instance invalidates every outstanding ask, so a tap on a
        /// menu drawn for a dead session is refused with a reason the operator can read.
        instance: String,
        /// The repo path, for the audit record and for a human reading it. Never for routing.
        repo: String,
        pid: u32,
        /// Absent means the project's own voice — which is every bridge shipped before this field
        /// existed, and is why it is optional rather than required.
        ///
        /// Present means one worktree of that project, speaking for itself. The hub reads nothing
        /// OUT of the string; it is an address, exactly as an [`crate::ids::AskId`] is. It cannot
        /// widen a bridge's reach, because the token beside it still resolves to the PROJECT and
        /// the hub builds the address from that resolved project plus this name — so a lane named
        /// here is always a lane of the project the secret already proved.
        ///
        /// `skip_serializing_if`, so a bridge that names no lane puts BYTE FOR BYTE what it always
        /// put on the wire. A `"lane":null` would be a field an older hub has to tolerate for no
        /// reason at all, on the one frame whose failure is a project that can never connect.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        lane: Option<crate::ids::LaneId>,
    },
    /// Something the agent said. Does not buzz.
    Say {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        hint: Option<SayHint>,
    },
    /// A question the agent is waiting on. Buzzes.
    Ask {
        ask_id: AskId,
        text: String,
        /// Absent means a free-text answer. Present means buttons.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        options: Option<Vec<AskOption>>,
    },
    /// The question stopped being open.
    ///
    /// This is the frame no pane-reading design could ever produce: a screen cannot tell you that
    /// a question is no longer being asked. The hub edits the original message and strips its
    /// buttons, so a stale keyboard cannot be tapped an hour later.
    AskResolved {
        ask_id: AskId,
        how: AskEnd,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        outcome: Option<String>,
    },
    /// The turn finished. Buzzes.
    Done { text: String },
    /// Liveness and state. Does not buzz.
    Beat {
        state: BeatState,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        note: Option<String>,
    },
    /// The bridge's answer to something the hub sent it.
    Ack {
        #[serde(rename = "ref")]
        r#ref: FrameId,
        status: AckStatus,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        reason: Option<String>,
    },
    /// Going away. Inside the grace window after a clean `bye` the hub says nothing at all: a
    /// phone that buzzes on every context refresh is worse than useless.
    Bye { reason: String },
    /// Answer to a [`HubFrame::Ping`], naming the ping's own frame id.
    ///
    /// The design sketch gave ping and pong a payload field called `id`. It cannot be called that:
    /// every frame already carries an envelope `id`, and flattening the two together produces a
    /// duplicate key that serde refuses. The correlation is `ref`, exactly as it is for an `ack`,
    /// and the ping's nonce is simply its envelope id — one fewer id to mint and one fewer to
    /// confuse with another.
    Pong {
        #[serde(rename = "ref")]
        r#ref: FrameId,
    },
    /// A kind this build does not know.
    ///
    /// Logged and ignored. It is a variant rather than a parse error so that a bridge shipped
    /// after this hub cannot kill the connection just by being newer.
    #[serde(other)]
    Unknown,
}

/// Whether the far side took a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AckStatus {
    Accepted,
    Refused,
}

/// Hub → bridge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum HubFrame {
    /// The connection is admitted. Carries the name the REGISTRY holds, not anything the bridge sent.
    ///
    /// `topic_id` is `None` at this point and usually stays that way for a few seconds: a topic is
    /// created when the connection becomes LIVE — after a settling window and one answered ping —
    /// not when it is admitted. A channel plugin that is not allowlisted boots and exits in about a
    /// tenth of a second, and a topic created for one of those is an empty topic bound forever to a
    /// project whose bridge was never there.
    ///
    /// A bridge does not need it and must not use it to address anything. It is here because the
    /// number is useful in a log when someone is working out where a message went.
    Welcome {
        project: String,
        /// The lane the hub actually admitted, echoed back.
        ///
        /// It exists so a bridge that NAMED a lane can tell it was heard. An unknown field inside a
        /// known kind is ignored on purpose, which is what lets a new bridge talk to an old hub —
        /// but it also means an old hub admits a lane silently as the project itself, giving the
        /// worktree the project's one claim and its topic while the project's own session is turned
        /// away. Nothing else in this frame can distinguish that from being given a place of one's
        /// own: `project` is a registry-owned title the bridge cannot predict.
        ///
        /// Absent means "no lane", which is both what an ordinary session is and what every hub
        /// built before lanes says about everything. A bridge that named one and gets no echo has
        /// learned the hub is older than it is, and must refuse rather than impersonate.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        lane: Option<crate::ids::LaneId>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        topic_id: Option<i32>,
        limits: Limits,
    },
    /// Not admitted. The connection closes immediately after.
    Refused { reason: RefusedReason },
    /// The operator's own words, relayed verbatim.
    ///
    /// **Opaque.** The hub does not parse it, does not act on it, and does not let it name
    /// anything. Inbound content selects; it never names.
    Message {
        msg_id: MsgId,
        text: String,
        from: From,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        in_reply_to_ask: Option<AskId>,
    },
    /// A tap, resolved against the record written down beside the message.
    Choice {
        msg_id: MsgId,
        ask_id: AskId,
        option_id: OptionId,
    },
    /// What became of one of the bridge's frames. Every frame gets exactly one.
    ///
    /// Backpressure reaches the only party that can act on it. A rejected send used to be one
    /// error log and a drop, which already lost 5,164 characters of a real agent's longest message.
    Ack {
        #[serde(rename = "ref")]
        r#ref: FrameId,
        delivered: Delivered,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        why: Option<AckWhy>,
    },
    /// Liveness probe. Its envelope `id` is the nonce; the answer names it in `ref`.
    ///
    /// A project counts as live only after `hello`, a settling window, and one answered ping — a
    /// channel that is not allowlisted boots and exits in about a tenth of a second, and would
    /// otherwise look exactly like a healthy worker for as long as anyone cared to watch.
    Ping,
    /// A kind this build does not know. Logged and ignored.
    #[serde(other)]
    Unknown,
}

/// Who sent an inbound message. For the audit record and for the allowlist decision that has
/// already been made by the time this frame exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct From {
    pub chat_id: i64,
    pub user_id: i64,
}

/// What the hub will accept from this connection, told to the bridge rather than discovered by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub max_frame: usize,
    pub max_text: usize,
    pub frames_per_min: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<P>(p: P) -> Envelope<P> {
        Envelope::new(FrameId::new("f1"), p)
    }

    #[test]
    fn an_envelope_is_one_flat_object_carrying_v_and_id_and_the_kind() {
        let json = serde_json::to_string(&env(BridgeFrame::Done {
            text: "built it".into(),
        }))
        .expect("serialises");
        assert_eq!(json, r#"{"v":1,"id":"f1","t":"done","text":"built it"}"#);
    }

    #[test]
    fn an_unknown_frame_kind_parses_rather_than_killing_the_connection() {
        // The whole point: a bridge shipped after this hub sends something newer, and the
        // connection survives it. A parse error here would be a deaf worker on upgrade day.
        let f: Envelope<BridgeFrame> =
            serde_json::from_str(r#"{"v":1,"id":"f9","t":"telepathy","mood":"blue"}"#)
                .expect("an unknown kind is a value, not an error");
        assert_eq!(f.payload, BridgeFrame::Unknown);
        assert_eq!(f.v, VERSION);
    }

    #[test]
    fn an_unknown_field_inside_a_known_kind_is_ignored() {
        let f: Envelope<BridgeFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f2","t":"say","text":"hi","hint":"prose","colour":"red"}"#,
        )
        .expect("additive fields do not break a known kind");
        assert_eq!(
            f.payload,
            BridgeFrame::Say {
                text: "hi".into(),
                hint: Some(SayHint::Prose),
            }
        );
    }

    #[test]
    fn a_hello_carries_no_display_name_for_the_topic() {
        // Pinned as a property of the TYPE, not of a code path: a bridge that could name itself
        // could claim another project's topic, and no amount of escaping downstream would help.
        // A lane is not an exception to this. It names WHICH CONVERSATION a connection is, and the
        // hub still takes every title it prints from its own registry.
        let json = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("p-herdr-tg"),
            token: "s3cret".into(),
            instance: "i1".into(),
            repo: "/home/u/Projects/herdr-tg".into(),
            pid: 42,
            lane: Some(crate::ids::LaneId::new("lane-0902-201212-2783563")),
        }))
        .expect("serialises");
        for forbidden in ["\"name\"", "\"project\":", "\"title\"", "\"topic\""] {
            assert!(
                !json.contains(forbidden),
                "hello must not carry {forbidden}: {json}"
            );
        }
    }

    #[test]
    fn a_hello_without_a_lane_is_byte_for_byte_the_hello_this_protocol_has_always_sent() {
        // The upgrade that matters most is the one nobody performs: a bridge already installed in a
        // running session keeps sending exactly this, and it must go on being the project's own
        // voice. Pinned as BYTES rather than as a round trip, because a round trip is green even
        // when a `"lane":null` has appeared on the wire — and a null here is a field an older hub
        // would have to be tolerant of for no reason at all.
        let json = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("unknown-until-the-hub-says"),
            token: "s3cret".into(),
            instance: "i1".into(),
            repo: "/home/u/Projects/herdr-tg".into(),
            pid: 42,
            lane: None,
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"hello","project_id":"unknown-until-the-hub-says","token":"s3cret","instance":"i1","repo":"/home/u/Projects/herdr-tg","pid":42}"#
        );
    }

    #[test]
    fn a_hello_from_a_bridge_that_has_never_heard_of_lanes_is_still_a_hello() {
        // The upgrade day that actually happens: the hub is replaced and the bridge is not, because
        // a channel plugin restarts only when its session does. The hello already on the wire has no
        // `lane` in it at all, and if this build required one, every project on the box would be
        // refused at the first gate with nothing to say why.
        let f: Envelope<BridgeFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f2","t":"hello","project_id":"p","token":"s","instance":"i","repo":"/r","pid":1}"#,
        )
        .expect("a hello with no lane must not be a parse error");
        assert_eq!(
            f.payload,
            BridgeFrame::Hello {
                project_id: ProjectId::new("p"),
                token: "s".into(),
                instance: "i".into(),
                repo: "/r".into(),
                pid: 1,
                lane: None,
            },
            "a bridge that named no lane stopped being the project's own voice"
        );
    }

    #[test]
    fn a_hello_that_names_a_lane_still_parses_on_a_build_that_has_never_heard_of_lanes() {
        // The other direction of the same skew, and the only one that cannot be run end to end,
        // because the hub that would have to be old is the one being replaced. `hello` is where a
        // wrong answer costs most: it is refused before anything else can happen and the bridge is
        // left with a closed socket and no reason.
        //
        // What makes it safe is structural rather than a policy anyone could flip. The escape hatch
        // — `deny_unknown_fields` on this variant — DOES NOT COMPILE here: serde refuses it on an
        // internally tagged enum reached through the envelope's `flatten`, with 49 errors. So the
        // tolerance is a property of the shape of this type, and the assertion below is what a
        // reader gets to see rather than the whole of the guarantee.
        let f: Envelope<BridgeFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f2","t":"hello","project_id":"p","token":"s","instance":"i","repo":"/r","pid":1,"lane":"lane-0902-201212-2783563"}"#,
        )
        .expect("a hello naming a lane must not be a parse error");
        let BridgeFrame::Hello { instance, .. } = f.payload else {
            panic!("a hello that names a lane stopped being a hello");
        };
        assert_eq!(instance, "i");
    }

    #[test]
    fn an_ack_says_which_of_the_three_delivery_states_it_observed() {
        let json = serde_json::to_string(&env(HubFrame::Ack {
            r#ref: FrameId::new("f7"),
            delivered: Delivered::Unseen,
            why: Some(AckWhy::TooFast),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"ack","ref":"f7","delivered":"unseen","why":"too-fast"}"#
        );
    }

    #[test]
    fn every_bridge_frame_round_trips() {
        let frames = vec![
            BridgeFrame::Say {
                text: "x".into(),
                hint: None,
            },
            BridgeFrame::Ask {
                ask_id: AskId::new("a1"),
                text: "ok?".into(),
                options: Some(vec![AskOption {
                    option_id: OptionId::new("y"),
                    label: "Yes".into(),
                }]),
            },
            BridgeFrame::AskResolved {
                ask_id: AskId::new("a1"),
                how: AskEnd::Answered,
                outcome: Some("No".into()),
            },
            BridgeFrame::Beat {
                state: BeatState::Blocked,
                note: None,
            },
            BridgeFrame::Ack {
                r#ref: FrameId::new("f3"),
                status: AckStatus::Refused,
                reason: Some("busy".into()),
            },
            BridgeFrame::Bye {
                reason: "refresh".into(),
            },
            BridgeFrame::Pong {
                r#ref: FrameId::new("f4"),
            },
        ];
        for f in frames {
            let json = serde_json::to_string(&env(f.clone())).expect("serialises");
            let back: Envelope<BridgeFrame> = serde_json::from_str(&json).expect("deserialises");
            assert_eq!(back.payload, f, "round trip changed the frame: {json}");
        }
    }

    #[test]
    fn no_payload_field_collides_with_the_envelope_id() {
        // The collision this pins is not hypothetical: the first cut of ping/pong named its nonce
        // `id`, which flattens on top of the envelope's own `id`, and serde refused every such
        // frame with "duplicate field `id`". Cheap to reintroduce, invisible until a round trip.
        let ping = serde_json::to_string(&env(HubFrame::Ping)).expect("serialises");
        assert_eq!(ping, r#"{"v":1,"id":"f1","t":"ping"}"#);
        let back: Envelope<HubFrame> = serde_json::from_str(&ping).expect("round trips");
        assert_eq!(back.payload, HubFrame::Ping);

        let pong = serde_json::to_string(&env(BridgeFrame::Pong {
            r#ref: FrameId::new("f1"),
        }))
        .expect("serialises");
        assert_eq!(pong, r#"{"v":1,"id":"f1","t":"pong","ref":"f1"}"#);
        serde_json::from_str::<Envelope<BridgeFrame>>(&pong).expect("round trips");
    }

    #[test]
    fn a_refusal_names_a_reason_the_bridge_can_branch_on() {
        let json = serde_json::to_string(&env(HubFrame::Refused {
            reason: RefusedReason::AlreadyClaimed,
        }))
        .expect("serialises");
        assert!(json.contains(r#""reason":"already_claimed""#), "{json}");
    }
}

//! Opaque, non-interchangeable id newtypes.
//!
//! Every id on this wire is a plain string, and the ones that matter are trivially transposable:
//! an `AskId` and an `OptionId` arrive in the same frame, and swapping them answers the wrong
//! question. Making them separate types means the compiler refuses the transposition that a
//! reviewer would have to catch by eye.
//!
//! There is deliberately no parsing and no validation. A bridge mints these; the hub stores and
//! echoes them. Anything the hub tried to read out of an id would be a fact it invented.

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! opaque_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        ///
        /// `#[serde(transparent)]`: a bare JSON string on the wire, exactly as it was minted.
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wraps a raw wire id. No validation: the far side is entitled to any string, and a
            /// shape this crate refused would be a shape the operator could never be told about.
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            /// The raw wire id.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
    };
}

opaque_id!(
    ProjectId,
    "A conversation the hub resolves a secret to: a project, minted once at enrolment from its
canonical repo path, or a room, minted at random by a terminal verb. Opaque here either way.

Never a counter. A recycled counter silently inheriting a dead agent's topic is a confirmed
defect in this repo's history, not a hypothetical."
);
opaque_id!(
    FrameId,
    "One frame on one connection. Opaque and monotonic per connection; the correlation for an `ack`."
);
opaque_id!(
    AskId,
    "One question an agent asked, minted by the bridge that asked it.

This is what makes \"is that still the same question?\" a fact instead of a guess. The old screen
path had to infer it from a status sequence and refused whenever the sequence was absent."
);
opaque_id!(
    OptionId,
    "One answer button, minted by the bridge alongside the question it belongs to.

Resolved against the record written down beside the message, never against a button's position.
Position is how a button reading \"Reject\" once confirmed \"Allow always\"."
);
opaque_id!(MsgId, "One Telegram message, as the hub knows it.");
opaque_id!(
    LaneId,
    "One conversation of a project beside its own voice — a worktree, or whatever a dispatcher
named — speaking for itself rather than as the project.

Minted by whatever dispatched the lane and read off the machine by an adapter — never parsed here,
and never resolved to anything. It is an ADDRESS, not a credential: the secret still proves only the
project, so a lane named on the wire can only ever be a lane of the project that secret resolved to.

Its SHAPE is checked in the hub, in the same place the 64-byte button check lives and for the same
reason: a lane carrying a newline or a tab would forge a line in the audit, and the audit is the one
record that has to stay unforgeable."
);
opaque_id!(
    SpecId,
    "One thing a controller has already approved and can be asked to run, named.

Opaque here in the strongest sense this crate has: the hub holds no table to resolve it against and
no way to get one. That is what stops an image, a command or a mount ever travelling on this wire —
what travels is a HANDLE, and only the party that already holds the approved thing can turn a handle
back into the thing. A hub that could dereference one would be a hub that had to be told what the
thing is.

Its SHAPE is checked in the hub, exactly as a lane's is and for the same reason: it is written into
the audit, and the audit is the one record that has to stay unforgeable."
);
opaque_id!(
    IntentId,
    "One intention the hub carried to a controller, minted by the hub before the frame is on the wire.

The correlation an outcome names, and the ONLY one. A second id to look the same thing up by is a
second place for one of them to win, which is the reason there is one list of what the hub is
waiting to hear about rather than two."
);
opaque_id!(
    IdempotencyKey,
    "What makes a repeated intention safe to receive twice.

Minted by the HUB, from what the operator was looking at when he tapped — so two taps on one button
are one key, and the same button drawn again after the address moved is a different one. Delivery
here is at-least-once and nothing pretends otherwise; this is what makes that survivable.

A separate type from `IntentId` because the two ride in one frame and are both id-shaped strings.
Transposed, every intention becomes its own duplicate and nothing is ever carried out — a defect a
reviewer would have to catch by eye, which the compiler now refuses instead."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_round_trips_as_a_bare_string() {
        let a = AskId::new("a1");
        let json = serde_json::to_string(&a).expect("an id serialises");
        assert_eq!(json, r#""a1""#);
        assert_eq!(
            serde_json::from_str::<AskId>(&json).expect("an id deserialises"),
            a
        );
    }

    #[test]
    fn an_id_keeps_whatever_shape_it_was_given() {
        // The hub must never normalise an id. Round-tripping it verbatim is what lets a bridge
        // recognise its own answer coming back.
        for raw in ["", " ", "a/b", "🙂", "a\"b"] {
            let id = OptionId::new(raw);
            let json = serde_json::to_string(&id).expect("serialises");
            let back: OptionId = serde_json::from_str(&json).expect("deserialises");
            assert_eq!(back.as_str(), raw);
        }
    }
}

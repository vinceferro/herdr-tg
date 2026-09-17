//! The frames and the framing spoken between the Kickoff Channel hub and a project's bridge.
//!
//! One bot holds the only Telegram connection and listens on one Unix socket. Each project runs a
//! small bridge that talks to that socket instead of talking to Telegram. This crate is the
//! contract between them, and it is deliberately the *only* thing the two share: the hub is Rust,
//! a bridge may be anything, and neither imports the other.
//!
//! # What is not here, and why
//!
//! No transport, no listener, no authentication. Those belong to the hub, which owns the
//! decisions — who may connect, which project a connection is, where its words go. A protocol
//! crate that also opened sockets would invite a bridge to reuse the hub's half of the
//! conversation, and the one property that has to hold is that **authority flows one way**.
//!
//! # The ceiling
//!
//! [`MAX_FRAME_BYTES`] bounds one frame, not one conversation. Over it is a protocol error:
//! refused, then closed. It is never truncated — see [`codec`] for why that distinction is
//! load-bearing rather than pedantic.

#![forbid(unsafe_code)]

pub mod codec;
pub mod error;
pub mod frame;
pub mod ids;

pub use codec::{FrameReader, write_frame};
pub use error::ProtoError;
pub use frame::{
    AckStatus, AckWhy, AskEnd, AskOption, BeatState, BridgeFrame, Control, Delivered, Envelope,
    FileAs, FileKind, FileWhy, From, HubFrame, IntentStatus, Limits, MAX_GENERATION, MessageFile,
    Op, RefusedReason, SayFile, SayHint, VERSION, control_for, promises_to_confirm,
};
pub use ids::{
    AskId, FrameId, IdempotencyKey, IntentId, LaneId, MsgId, OptionId, ProjectId, SpecId,
};

/// Hard ceiling on one frame, terminator included.
///
/// 64 KiB. Large enough for the longest real agent message this system has seen — a 5,164-character
/// one that the old path dropped with a single log line — and small enough that a peer cannot make
/// the hub hold an unbounded buffer per connection.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

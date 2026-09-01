//! What can go wrong on the wire, in variants the caller acts on differently.
//!
//! These are deliberately not collapsed into one "bad frame" error. The hub answers a peer
//! differently for each: an oversize frame gets `refused` and a close, a decode failure is logged
//! and the connection survives, and an unterminated frame means the peer is already gone.

use thiserror::Error;

/// A framing or decoding failure.
#[derive(Debug, Error)]
pub enum ProtoError {
    /// The socket itself failed.
    #[error("socket error: {0}")]
    Io(#[source] std::io::Error),

    /// A frame exceeded the ceiling. Refused and closed — never truncated, because half a message
    /// on a phone is worse than none and looks the same as a whole one.
    #[error("frame is larger than the {max}-byte ceiling; refused rather than truncated")]
    Oversize { max: usize },

    /// The peer stopped mid-frame. The bytes that arrived are not a message.
    #[error("the peer closed inside a frame after {bytes} bytes")]
    Unterminated { bytes: usize },

    /// A complete line that is not a frame this build can read.
    #[error("could not decode a {len}-byte frame: {source}")]
    Decode {
        #[source]
        source: serde_json::Error,
        len: usize,
    },

    /// A frame could not be built. A bug on this side, not the peer's.
    #[error("could not encode a frame: {0}")]
    Encode(#[source] serde_json::Error),
}

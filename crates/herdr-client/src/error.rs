//! The one error type the whole crate returns, plus the routing predicates the binary branches on.
//!
//! Each predicate maps 1:1 to a named row of `PLAN.md`'s failure table, so the bridge branches on a
//! type rather than string-matching an error message.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Deserializer};

/// Everything that can go wrong talking to herdr.
///
/// `#[non_exhaustive]`: adding a variant must not be a breaking change for a downstream slice.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HerdrError {
    /// Could not reach the socket at all (missing, a directory, wrong permissions, or the dial
    /// timed out). `path` is carried because "which socket" is the operator's first question.
    #[error("herdr unreachable: {path} ({source})")]
    Connect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// The dial succeeded and then the I/O failed.
    #[error("herdr I/O error during {method}: {source}")]
    Io {
        method: &'static str,
        #[source]
        source: io::Error,
    },

    /// The server accepted the connection and never answered. This is the shape a MISSING TRAILING
    /// NEWLINE takes: herdr hangs forever with no error and no close, so only this timeout catches
    /// it (verified live: 5.01 s, zero bytes, connection still open).
    #[error("herdr timed out after {elapsed:?} during {method}")]
    Timeout {
        method: &'static str,
        elapsed: Duration,
    },

    /// A 0-byte read: the server closed before writing a reply. Distinct from `Decode` so an empty
    /// line never surfaces as a confusing "EOF while parsing a value".
    #[error("herdr closed the connection during {method} before replying")]
    ClosedEarly { method: &'static str },

    /// Rejected client-side, BEFORE dialing. Belt-and-braces: the server is loud (ECONNRESET)
    /// rather than silent here, but a 1 MiB+ body is a bug worth naming precisely.
    #[error("{method} request body is {len} B, over the server's {max} B cap")]
    RequestTooLarge {
        method: &'static str,
        len: usize,
        max: usize,
    },

    /// The reply arrived but did not parse. Carries the raw line: a decode failure the operator
    /// cannot see the bytes of is not actionable from a phone.
    #[error("could not decode herdr reply to {method}: {source}\n  line: {line}")]
    Decode {
        method: &'static str,
        #[source]
        source: serde_json::Error,
        line: String,
    },

    /// herdr answered with `{"error":{code,message}}` — a semantic refusal, not a transport fault.
    #[error("herdr returned {code}: {message}")]
    Protocol {
        method: &'static str,
        code: ErrorCode,
        message: String,
    },

    /// The reply's `result.type` tag was not the one this method's `Request` impl declares.
    #[error("{method} returned result type {got:?}, expected {expected:?}")]
    UnexpectedResult {
        method: &'static str,
        expected: &'static str,
        got: String,
    },

    /// The server speaks a protocol below [`crate::MIN_SUPPORTED_PROTOCOL`]. Fatal by design:
    /// a REMOVED method surfaces as `invalid_request: unknown variant`, which the client can detect
    /// but cannot repair, and running degraded means silently missing asks.
    #[error(
        "herdr {server_version} speaks protocol {server}; this client requires >= {min} (built for {client})"
    )]
    ProtocolTooOld {
        server: u32,
        min: u32,
        client: u32,
        server_version: String,
    },
}

/// The `code` string herdr puts in an error reply.
///
/// `ErrorBody.code` is an OPEN string in the schema — verified: `{"code":{"type":"string"},
/// "message":{"type":"string"}}` with no `enum`, and grepping the whole 255 KB schema dump for
/// `pane_not_found` / `invalid_key` returns nothing. A closed enum would fail to parse a future code
/// and take its message down with it, so the catch-all carries the wire string verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    InvalidRequest,
    PaneNotFound,
    TabNotFound,
    WorkspaceNotFound,
    AgentNotFound,
    AgentBlocked,
    AgentNotReady,
    AgentPromptFailed,
    AgentPromptStalled,
    AgentSendKeysFailed,
    InvalidKey,
    InvalidTarget,
    UiBusy,
    UnsupportedEventWaitMatch,
    /// A code this client was not built for. Keeps the wire string so the message stays readable
    /// and `--json` stays honest.
    Other(String),
}

impl ErrorCode {
    /// The wire string. Round-trips: `from_wire(c.as_str()) == c` for every variant.
    pub fn as_str(&self) -> &str {
        match self {
            ErrorCode::InvalidRequest => "invalid_request",
            ErrorCode::PaneNotFound => "pane_not_found",
            ErrorCode::TabNotFound => "tab_not_found",
            ErrorCode::WorkspaceNotFound => "workspace_not_found",
            ErrorCode::AgentNotFound => "agent_not_found",
            ErrorCode::AgentBlocked => "agent_blocked",
            ErrorCode::AgentNotReady => "agent_not_ready",
            ErrorCode::AgentPromptFailed => "agent_prompt_failed",
            ErrorCode::AgentPromptStalled => "agent_prompt_stalled",
            ErrorCode::AgentSendKeysFailed => "agent_send_keys_failed",
            ErrorCode::InvalidKey => "invalid_key",
            ErrorCode::InvalidTarget => "invalid_target",
            ErrorCode::UiBusy => "ui_busy",
            ErrorCode::UnsupportedEventWaitMatch => "unsupported_event_wait_match",
            ErrorCode::Other(s) => s,
        }
    }

    /// Never fails: an unknown code becomes [`ErrorCode::Other`] rather than an error.
    pub fn from_wire(s: &str) -> Self {
        match s {
            "invalid_request" => ErrorCode::InvalidRequest,
            "pane_not_found" => ErrorCode::PaneNotFound,
            "tab_not_found" => ErrorCode::TabNotFound,
            "workspace_not_found" => ErrorCode::WorkspaceNotFound,
            "agent_not_found" => ErrorCode::AgentNotFound,
            "agent_blocked" => ErrorCode::AgentBlocked,
            "agent_not_ready" => ErrorCode::AgentNotReady,
            "agent_prompt_failed" => ErrorCode::AgentPromptFailed,
            "agent_prompt_stalled" => ErrorCode::AgentPromptStalled,
            "agent_send_keys_failed" => ErrorCode::AgentSendKeysFailed,
            "invalid_key" => ErrorCode::InvalidKey,
            "invalid_target" => ErrorCode::InvalidTarget,
            "ui_busy" => ErrorCode::UiBusy,
            "unsupported_event_wait_match" => ErrorCode::UnsupportedEventWaitMatch,
            other => ErrorCode::Other(other.to_owned()),
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Manual, because `derive` cannot express a data-carrying catch-all.
impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(ErrorCode::from_wire(&String::deserialize(d)?))
    }
}

/// The prefix herdr puts on every parse/routing refusal, verified live:
/// `invalid request: missing field \`params\` at line 1 column 37`.
const UNKNOWN_METHOD_PREFIX: &str = "invalid request: unknown variant";

impl HerdrError {
    /// "herdr dies / socket gone" → `/status` says "herdr unreachable"; the loop backs off.
    pub fn is_unreachable(&self) -> bool {
        matches!(
            self,
            HerdrError::Connect { .. }
                | HerdrError::Io { .. }
                | HerdrError::Timeout { .. }
                | HerdrError::ClosedEarly { .. }
        )
    }

    /// "sticky target pane closed" → offer the picker, NEVER silently reroute.
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            HerdrError::Protocol {
                code: ErrorCode::PaneNotFound
                    | ErrorCode::TabNotFound
                    | ErrorCode::WorkspaceNotFound,
                ..
            }
        )
    }

    /// `invalid_request` whose message begins "invalid request: unknown variant" — this herdr lacks
    /// the method. Verified: the message enumerates every method it DOES have, so this doubles as a
    /// capability probe (`agent.send` was removed between protocol 16 and 20).
    pub fn is_unsupported_method(&self) -> bool {
        matches!(
            self,
            HerdrError::Protocol {
                code: ErrorCode::InvalidRequest,
                message,
                ..
            } if message.starts_with(UNKNOWN_METHOD_PREFIX)
        )
    }

    /// Exit non-zero with a distinct log signature; do not retry.
    pub fn is_fatal(&self) -> bool {
        matches!(self, HerdrError::ProtocolTooOld { .. })
    }

    /// 3 = unreachable · 4 = protocol skew · 5 = herdr protocol error · 1 = otherwise.
    /// Proof gate 6 asserts 3 and 4 exactly.
    pub fn exit_code(&self) -> i32 {
        if self.is_unreachable() {
            3
        } else if matches!(self, HerdrError::ProtocolTooOld { .. }) {
            4
        } else if matches!(self, HerdrError::Protocol { .. }) {
            5
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protocol(code: &str, message: &str) -> HerdrError {
        HerdrError::Protocol {
            method: "pane.read",
            code: ErrorCode::from_wire(code),
            message: message.to_owned(),
        }
    }

    #[test]
    fn every_known_code_round_trips_through_the_wire_string() {
        for code in [
            ErrorCode::InvalidRequest,
            ErrorCode::PaneNotFound,
            ErrorCode::TabNotFound,
            ErrorCode::WorkspaceNotFound,
            ErrorCode::AgentNotFound,
            ErrorCode::AgentBlocked,
            ErrorCode::AgentNotReady,
            ErrorCode::AgentPromptFailed,
            ErrorCode::AgentPromptStalled,
            ErrorCode::AgentSendKeysFailed,
            ErrorCode::InvalidKey,
            ErrorCode::InvalidTarget,
            ErrorCode::UiBusy,
            ErrorCode::UnsupportedEventWaitMatch,
        ] {
            assert_eq!(ErrorCode::from_wire(code.as_str()), code);
            assert_eq!(
                serde_json::from_str::<ErrorCode>(&format!("\"{}\"", code.as_str())).unwrap(),
                code
            );
        }
    }

    #[test]
    fn unsupported_method_is_gated_on_the_message_not_just_the_code() {
        let removed = protocol(
            "invalid_request",
            "invalid request: unknown variant `agent.send`, expected one of `ping`, `pane.read`",
        );
        assert!(removed.is_unsupported_method());
        assert!(!removed.is_not_found());

        // The other invalid_request shape captured live must NOT read as a missing method.
        let malformed = protocol(
            "invalid_request",
            "invalid request: missing field `params` at line 1 column 37",
        );
        assert!(!malformed.is_unsupported_method());
    }

    #[test]
    fn exit_codes_are_the_ones_proof_gate_6_asserts() {
        let unreachable = HerdrError::Connect {
            path: PathBuf::from("/nonexistent/herdr.sock"),
            source: io::Error::from(io::ErrorKind::NotFound),
        };
        assert_eq!(unreachable.exit_code(), 3);
        assert!(unreachable.is_unreachable());
        assert!(!unreachable.is_fatal());

        let skew = HerdrError::ProtocolTooOld {
            server: 19,
            min: 20,
            client: 20,
            server_version: "0.8.1".to_owned(),
        };
        assert_eq!(skew.exit_code(), 4);
        assert!(skew.is_fatal());
        assert!(!skew.is_unreachable());

        assert_eq!(
            protocol("pane_not_found", "pane zz:p9 not found").exit_code(),
            5
        );

        let other = HerdrError::RequestTooLarge {
            method: "pane.send_text",
            len: 2,
            max: 1,
        };
        assert_eq!(other.exit_code(), 1);
        assert!(!other.is_unreachable());
    }

    #[test]
    fn timeout_and_closed_early_both_read_as_unreachable() {
        let t = HerdrError::Timeout {
            method: "ping",
            elapsed: Duration::from_millis(200),
        };
        let c = HerdrError::ClosedEarly { method: "ping" };
        assert!(t.is_unreachable() && c.is_unreachable());
        assert_eq!((t.exit_code(), c.exit_code()), (3, 3));
    }
}

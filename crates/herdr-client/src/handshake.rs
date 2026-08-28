//! The version handshake — built on `ping`, because **protocol 20 has no hello method**.
//!
//! `ping` is a real capability handshake and is not in `HERDR_API.md` at all. Its reply carries the
//! server version, the protocol number and a capabilities map:
//!
//! ```text
//! {"id":"p","method":"ping","params":{}}
//!   -> {"type":"pong","version":"0.8.2","protocol":20,
//!       "capabilities":{"live_handoff":true,"detached_server_daemon":true}}
//! ```
//!
//! # Unknown ADDITIONS are survivable; REMOVALS are not — but "survivable" has a range
//!
//! That asymmetry is the whole policy. A protocol above [`KNOWN_PROTOCOL`] may have grown methods,
//! result variants and event kinds this client does not model — every one of those is bucketed and
//! logged rather than fatal, so the bridge stays alive through a routine `herdr update`. A protocol
//! *below* [`MIN_SUPPORTED_PROTOCOL`] may have **lost** a method: `agent.send` vanished between
//! protocol 16 and 20, and a missing method surfaces as `invalid_request: unknown variant`, which
//! this client can detect but cannot repair. Running degraded there means silently missing asks,
//! which for a phone-only operator is worse than not starting.
//!
//! **The claim is bounded.** [`FAR_AHEAD_PROTOCOLS`] is where "unknown additions are survivable"
//! stops being an earned statement and becomes a guess. A server one or two revisions ahead is a
//! routine `herdr update` that this client's bucketing really does absorb; a server 79 revisions
//! ahead is a herdr nobody has ever run this client against, and reporting that as "survivable"
//! is a confidence the evidence does not support. Beyond the threshold the handshake still
//! SUCCEEDS — whether a too-new server should be fatal the way a too-old one is is the operator's
//! values call, the exact mirror of the `MIN_SUPPORTED_PROTOCOL` question docs/SLICE-1.md leaves
//! open — but it stops claiming survivability, in the `WARN` and in `doctor`'s output alike. See
//! [`Compatibility::is_far_ahead`].
//!
//! # Re-run this on every reconnect, not only at boot
//!
//! `capabilities.live_handoff` is `true` on this server, so herdr can swap its own binary
//! underneath a running bridge **without the socket path changing**. A handshake taken once at
//! startup is a snapshot of a server that may no longer exist.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Once;

use serde::{Deserialize, Serialize};

use crate::error::HerdrError;
use crate::{KNOWN_PROTOCOL, MIN_SUPPORTED_PROTOCOL};

/// `ping`'s result payload.
///
/// This is the ONE result whose payload is **flat**: every other method nests its data under a
/// per-method key (`{"type":"session_snapshot","snapshot":{…}}`), but pong's fields sit directly
/// beside its `type` tag. The tag itself is checked by `client::call` before this is decoded, so it
/// is deliberately not modelled here — same convention as `proto::response`'s wrappers.
// No `Eq`: `ServerCapabilities::extra` holds `serde_json::Value`, which is `PartialEq`
// but not `Eq` (floats).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pong {
    /// herdr's own semver, e.g. `"0.8.2"`. Reported verbatim; never parsed for feature detection —
    /// `protocol` is the number that decides anything.
    pub version: String,
    /// The wire protocol. `PingResult.required` is `["type","version","protocol"]`.
    pub protocol: u32,
    /// Absent on a server that does not advertise any. `PingResult.capabilities` has
    /// `"default": null`, so a pong without the key must still parse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ServerCapabilities>,
}

/// What the server says it can do.
///
/// The schema declares exactly two properties and requires only `live_handoff`. Anything else the
/// server advertises is kept verbatim in [`ServerCapabilities::extra`] rather than dropped — a
/// capability this client does not understand is still something `doctor` should be able to show
/// the operator.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// `true` on this server: herdr can replace its own binary under a live socket. This is why
    /// [`crate::client::HerdrClient::handshake`] must be re-run on every event-stream reconnect.
    pub live_handoff: bool,
    /// Schema default `false`, so a pong that omits it still parses.
    #[serde(default)]
    pub detached_server_daemon: bool,
    /// Capabilities this client was not built for. Forward compatibility, not a feature: a
    /// protocol-21 capability must not vanish between the wire and `doctor --json`.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Past this many protocol revisions ahead, this client stops calling a newer server
/// "survivable".
///
/// **Not a refusal.** `Handshake::evaluate` (plain code, not a link: it is `pub(crate)`, and an
/// intra-doc link from public docs to a private item is a hard error under `-D warnings`) returns
/// `Ok` at any distance ahead; this only
/// changes what the client CLAIMS. Making a far-ahead server fatal the way an old one is
/// (`ProtocolTooOld` / exit 4) is a deliberate values call about how the bridge should behave at
/// its worst moment — the same call docs/SLICE-1.md's open question 2 leaves to the operator for
/// `MIN_SUPPORTED_PROTOCOL` — and it is not one this client makes for them: a bridge that refuses
/// to start is a bridge that is not there when the operator only has a phone.
///
/// 3 is chosen as "a couple of routine `herdr update`s", not measured — nothing has been run
/// against a herdr above protocol 20.
pub const FAR_AHEAD_PROTOCOLS: u32 = 3;

/// The verdict on the server's protocol number.
///
/// `#[non_exhaustive]`: a future slice that lowers [`MIN_SUPPORTED_PROTOCOL`] below
/// [`KNOWN_PROTOCOL`] will want a third arm, and adding it must not be a breaking change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Compatibility {
    /// The server speaks exactly the protocol this client was built and tested against.
    Exact,
    /// The server is ahead by `by` protocol revisions. **Not an error** — it is logged once and the
    /// bridge proceeds, because unknown additions are survivable.
    ServerNewer { by: u32 },
}

impl Compatibility {
    /// A short, stable label for logs and `doctor`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Compatibility::Exact => "exact",
            Compatibility::ServerNewer { .. } => "server_newer",
        }
    }

    /// How far ahead the server is; `0` for [`Compatibility::Exact`].
    pub fn ahead_by(&self) -> u32 {
        match self {
            Compatibility::Exact => 0,
            Compatibility::ServerNewer { by } => *by,
        }
    }

    /// More than [`FAR_AHEAD_PROTOCOLS`] revisions ahead — far enough that "unknown additions are
    /// survivable" is an unearned claim rather than a tested property. Callers that print a
    /// compatibility line to the operator should say so; nothing refuses to run on it.
    pub fn is_far_ahead(&self) -> bool {
        self.ahead_by() > FAR_AHEAD_PROTOCOLS
    }
}

/// A completed handshake: what the server said, plus the verdict.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Handshake {
    pub pong: Pong,
    pub compatibility: Compatibility,
}

/// Warned at most once per process, not once per reconnect — a bridge that reconnects every few
/// seconds against a newer herdr must not fill the operator's journal with the same line.
static NEWER_SERVER_WARNED: Once = Once::new();

impl Handshake {
    /// Apply the version policy to a pong.
    ///
    /// - `protocol < MIN_SUPPORTED_PROTOCOL` → [`HerdrError::ProtocolTooOld`], which
    ///   [`HerdrError::is_fatal`] reports `true` for and which exits **4**.
    /// - `protocol == KNOWN_PROTOCOL` → [`Compatibility::Exact`].
    /// - `protocol > KNOWN_PROTOCOL` → [`Compatibility::ServerNewer`], warned once, `Ok`.
    pub(crate) fn evaluate(pong: Pong) -> Result<Handshake, HerdrError> {
        if pong.protocol < MIN_SUPPORTED_PROTOCOL {
            return Err(HerdrError::ProtocolTooOld {
                server: pong.protocol,
                min: MIN_SUPPORTED_PROTOCOL,
                client: KNOWN_PROTOCOL,
                server_version: pong.version,
            });
        }

        let compatibility = match pong.protocol.cmp(&KNOWN_PROTOCOL) {
            Ordering::Equal => Compatibility::Exact,
            Ordering::Greater => {
                let by = pong.protocol - KNOWN_PROTOCOL;
                let version = pong.version.clone();
                let far = by > FAR_AHEAD_PROTOCOLS;
                NEWER_SERVER_WARNED.call_once(|| {
                    if far {
                        tracing::warn!(
                            server_version = %version,
                            server_protocol = pong.protocol,
                            client_protocol = KNOWN_PROTOCOL,
                            ahead_by = by,
                            "herdr is FAR ahead of the protocol this client was built for; \
                             proceeding, but this client has never been run against it and its \
                             handling of protocol changes this large is UNVERIFIED — it may be \
                             bucketing frames that carry real asks. Rebuild herdr-tg against \
                             this herdr."
                        );
                    } else {
                        tracing::warn!(
                            server_version = %version,
                            server_protocol = pong.protocol,
                            client_protocol = KNOWN_PROTOCOL,
                            "herdr speaks a newer protocol than this client was built for; \
                             unknown additions are bucketed, not fatal"
                        );
                    }
                });
                Compatibility::ServerNewer { by }
            }
            // Unreachable while MIN_SUPPORTED_PROTOCOL == KNOWN_PROTOCOL, which is the case today:
            // a protocol at or above the minimum cannot also be below the one we were built for.
            // If a later slice lowers the minimum, this arm becomes reachable and should grow a
            // `ServerOlder` verdict rather than keep reporting `Exact` — the enum is
            // `#[non_exhaustive]` precisely so that is an additive change.
            Ordering::Less => Compatibility::Exact,
        };

        Ok(Handshake {
            pong,
            compatibility,
        })
    }

    /// herdr's semver, e.g. `"0.8.2"`.
    pub fn version(&self) -> &str {
        &self.pong.version
    }

    /// The protocol the server speaks.
    pub fn protocol(&self) -> u32 {
        self.pong.protocol
    }

    /// What the server advertised, if anything.
    pub fn capabilities(&self) -> Option<&ServerCapabilities> {
        self.pong.capabilities.as_ref()
    }

    /// Whether herdr may swap its own binary under this socket. `false` when the server advertised
    /// no capabilities at all — the conservative reading, since the field is required whenever the
    /// map is present.
    pub fn live_handoff(&self) -> bool {
        self.pong
            .capabilities
            .as_ref()
            .is_some_and(|c| c.live_handoff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PONG_FIXTURE: &str = include_str!("../tests/fixtures/pong.json");

    fn pong(protocol: u32) -> Pong {
        Pong {
            version: "0.8.2".to_owned(),
            protocol,
            capabilities: None,
        }
    }

    #[test]
    fn the_captured_pong_decodes_with_its_capabilities() {
        let reply: serde_json::Value = serde_json::from_str(PONG_FIXTURE.trim()).unwrap();
        let pong: Pong = serde_json::from_value(reply["result"].clone()).expect("captured pong");
        assert_eq!(pong.version, "0.8.2");
        assert_eq!(pong.protocol, 20);
        let caps = pong.capabilities.as_ref().expect("this server advertises");
        assert!(caps.live_handoff);
        assert!(caps.detached_server_daemon);
        assert!(caps.extra.is_empty(), "no unknown capabilities on 0.8.2");

        let hs = Handshake::evaluate(pong).expect("protocol 20 is exactly ours");
        assert_eq!(hs.compatibility, Compatibility::Exact);
        assert!(hs.live_handoff());
        assert_eq!(hs.version(), "0.8.2");
        assert_eq!(hs.protocol(), 20);
    }

    #[test]
    fn a_pong_without_capabilities_still_parses() {
        let p: Pong =
            serde_json::from_str(r#"{"type":"pong","version":"0.8.2","protocol":20}"#).unwrap();
        assert!(p.capabilities.is_none());
        let hs = Handshake::evaluate(p).unwrap();
        assert!(
            !hs.live_handoff(),
            "no advertisement is read conservatively as `no live handoff`"
        );
    }

    #[test]
    fn an_unknown_capability_is_kept_rather_than_dropped() {
        let p: Pong = serde_json::from_str(
            r#"{"type":"pong","version":"0.9.0","protocol":21,
                "capabilities":{"live_handoff":true,"teleportation":{"beta":true}}}"#,
        )
        .unwrap();
        let caps = p.capabilities.as_ref().unwrap();
        assert!(caps.live_handoff);
        assert!(!caps.detached_server_daemon, "schema default is false");
        assert_eq!(
            caps.extra["teleportation"],
            serde_json::json!({"beta": true})
        );

        // And it survives a round trip, so `doctor --json` can show it.
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["capabilities"]["teleportation"]["beta"], true);
        assert!(
            back["capabilities"].get("detached_server_daemon").is_some(),
            "a required-with-default bool is not an Option and always re-serializes"
        );
    }

    #[test]
    fn the_version_policy_is_asymmetric() {
        // Older is fatal.
        let err = Handshake::evaluate(pong(19)).expect_err("19 is below the minimum");
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
        assert!(!err.is_unreachable());

        // Exactly ours.
        assert_eq!(
            Handshake::evaluate(pong(20)).unwrap().compatibility,
            Compatibility::Exact
        );

        // Newer is a warning, not an error.
        let hs = Handshake::evaluate(pong(21)).expect("newer is survivable");
        assert_eq!(hs.compatibility, Compatibility::ServerNewer { by: 1 });
        assert_eq!(hs.compatibility.ahead_by(), 1);
        assert_eq!(hs.compatibility.as_str(), "server_newer");
        assert_eq!(
            Handshake::evaluate(pong(99)).unwrap().compatibility,
            Compatibility::ServerNewer { by: 79 }
        );
    }

    /// **Review minor, closed 2026-08-28.** A protocol-99 server (79 revisions ahead) handshook
    /// with a `WARN` and `doctor` reported "unknown additions are survivable". At +79 that is an
    /// unearned claim: nothing has ever run this client against a herdr above protocol 20.
    ///
    /// What did NOT change, deliberately: it is still `Ok`, not `ProtocolTooOld`. Refusing to
    /// start against a too-new server is the mirror of docs/SLICE-1.md's open question 2 (should
    /// the bridge refuse a too-OLD herdr?), which that document leaves to the operator because it
    /// is a values call about the product's worst moment — and a bridge that refuses to start is a
    /// bridge that is not there when the operator only has a phone. This client stops claiming
    /// survivability; it does not decide the refusal on the operator's behalf.
    #[test]
    fn far_ahead_is_still_ok_but_stops_claiming_survivability() {
        assert_eq!(FAR_AHEAD_PROTOCOLS, 3);

        // At and below the threshold: a routine `herdr update`, claim unchanged.
        for by in 1..=FAR_AHEAD_PROTOCOLS {
            let hs = Handshake::evaluate(pong(KNOWN_PROTOCOL + by)).expect("newer is survivable");
            assert_eq!(hs.compatibility, Compatibility::ServerNewer { by });
            assert!(
                !hs.compatibility.is_far_ahead(),
                "+{by} is a routine update, not far ahead"
            );
        }

        // Past it: still Ok, still `server_newer`, but flagged.
        for by in [FAR_AHEAD_PROTOCOLS + 1, 79] {
            let hs = Handshake::evaluate(pong(KNOWN_PROTOCOL + by))
                .expect("a far-ahead server still handshakes — the refusal is the operator's call");
            assert_eq!(hs.compatibility.ahead_by(), by);
            assert_eq!(hs.compatibility.as_str(), "server_newer");
            assert!(
                hs.compatibility.is_far_ahead(),
                "+{by} must not be reported as survivable"
            );
        }

        // Exact and old are untouched by any of this.
        assert!(!Compatibility::Exact.is_far_ahead());
        assert_eq!(Compatibility::Exact.ahead_by(), 0);
        assert_eq!(Handshake::evaluate(pong(19)).unwrap_err().exit_code(), 4);
    }

    #[test]
    fn the_ancient_protocol_that_removed_agent_send_is_refused() {
        // Protocol 16 is the one HERDR_API.md documents. `agent.send` existed there and is gone at
        // 20, which is the concrete removal MIN_SUPPORTED_PROTOCOL exists to refuse.
        let err = Handshake::evaluate(Pong {
            version: "0.7.4".to_owned(),
            protocol: 16,
            capabilities: None,
        })
        .expect_err("16 is below the minimum");
        assert_eq!(err.exit_code(), 4);
        assert!(err.to_string().contains("0.7.4"), "{err}");
    }
}

//! A typed, offline-testable client for the herdr session daemon (herdr 0.8.2 / **protocol 20**).
//!
//! # COMPLETE — build order step 8; the whole client crate has landed
//!
//! `error`, `ids`, `keys`, `transport` (crate-private), `proto::model`, `proto::response`
//! (crate-private), `proto::request` (crate-private), `client` (reads, writes, `subscribe`),
//! `handshake`, `proto::event` and `stream`. What remains in slice 1 is the BINARY
//! (`crates/herdr-tg`, step 9) and the proof harness (step 10). Do not add a module here without
//! its file — the clippy gate is `-D warnings`, so an item nothing calls yet fails the commit.
//!
//! # The write methods have NO live call site, and that is a load-bearing property
//!
//! [`client::HerdrClient::send_text`], [`client::HerdrClient::send_keys`] and
//! [`client::HerdrClient::send_input`] type real keystrokes into the operator's real terminals.
//! They are built, typed and mock-tested; nothing outside `#[cfg(test)]` calls one, and the binary
//! exposes no subcommand that reaches them. `tests/no_live_write_call_site.rs` fails the suite if
//! that ever stops being true. Their live verification is `scripts/verify-send-p20.sh`, which
//! refuses to run against anything but a throwaway probe socket.
//!
//! # Two absences that are load-bearing, not omissions
//!
//! There is **no `focus`** method and there never will be: focusing a pane marks it *seen*, and
//! `done` means "idle after work the operator has not seen", so a focus call from a bridge would
//! destroy the signal its own push trigger depends on. And there is **no public
//! `ReadSource`** — `recent` / `recent_unwrapped` harvest-scroll the operator's real viewport when
//! `lines > viewport_rows`, so [`client::HerdrClient`] offers only `read_visible` /
//! `read_visible_tail`, both pinned to `visible` at construction. Neither hazard is reachable from
//! a timer, by construction rather than by convention.
//!
//! # The two invisible traps
//!
//! **1. The event stream multiplexes two incompatible envelope encodings on one connection.**
//! Lifecycle frames are snake_case and repeat their name inside `data` (`{"event":"pane_updated",
//! "data":{"type":"pane_updated","pane":{…}}}`). The product's one load-bearing event arrives
//! **dot-form with no `type` in `data` at all** (`{"event":"pane.agent_status_changed","data":
//! {"pane_id":…,"agent_status":…}}`). A `#[serde(tag = "type")]` model over `data` — which is what
//! herdr's own `HERDR_API.md` leads you to — parses every lifecycle frame and **silently errors on
//! every ask**. Decode in two steps: read `event`, then dispatch on it.
//!
//! **2. `pane.updated` replays a stale backlog on every connect**, ~100 ms/frame, each frame
//! carrying a *historical* `agent_status`. It is the only globally-subscribable status-bearing
//! event, so a bridge that reads status from it fires a phantom-push burst on every reconnect.
//! There is no global agent-status subscription: `pane.agent_status_changed` requires a `pane_id`,
//! and `events.subscribe` freezes the set at connect (there is no `events.update`).

/// The protocol this client was built and tested against (herdr 0.8.2, verified 2026-08-28).
pub const KNOWN_PROTOCOL: u32 = 20;

/// Below this we refuse to run. Unknown ADDITIONS are survivable; REMOVALS are not —
/// `agent.send` vanished between protocol 16 and 20, and a missing method surfaces as
/// `invalid_request: unknown variant`, which the client can detect but cannot repair.
pub const MIN_SUPPORTED_PROTOCOL: u32 = 20;

/// Max JSON body, newline EXCLUDED. Belt-and-braces: the server is loud (ECONNRESET), not silent.
pub const MAX_REQUEST_BODY_BYTES: usize = 1_048_576;

/// Hard ceiling on a single reply line. `transport` reads through `.take(MAX_RESPONSE_BYTES)` so a
/// pathological reply cannot OOM a bridge that is supposed to survive unattended.
pub(crate) const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;

/// Socket path relative to `$HOME`, used when `$HERDR_SOCKET_PATH` is absent — which is the normal
/// case for the production `systemd --user` unit, since every `HERDR_*` var is pane-injected only.
pub const DEFAULT_SOCKET_RELPATH: &str = ".config/herdr/herdr.sock";

// ── modules ─────────────────────────────────────────────────────────────────────────────────────
pub mod client;
pub mod error;
pub mod handshake;
pub mod ids;
/// The key grammar for `pane.send_keys` is UNVERIFIED on protocol 20 — see the module banner.
/// [`Key`] is a validating newtype rather than a closed enum for exactly that reason.
pub mod keys;
pub mod proto;
pub mod stream;
/// Crate-private on purpose: this module is the ONLY writer, and it appends the request's trailing
/// newline itself. A public path here would be a public path to an unterminated line, which hangs
/// herdr forever with no error and no close.
mod transport;

pub use client::{HerdrClient, Request, WriteAccepted};
pub use error::{ErrorCode, HerdrError};
pub use handshake::{Compatibility, FAR_AHEAD_PROTOCOLS, Handshake, Pong, ServerCapabilities};
pub use ids::{PaneId, TabId, WorkspaceId};
pub use keys::{Key, KeyParseError};
pub use proto::event::{AgentStatusChanged, Event, RosterEvent, Subscription};
pub use proto::model::*;
pub use stream::EventStream;

// The offline suite's herdr stand-in. It lives at `tests/support/mod.rs` (the spec's location) and
// is wired in here as well so the crate-private `transport` module can be tested without widening
// the public API. Integration tests under `tests/` declare their own `mod support;`.
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod support;

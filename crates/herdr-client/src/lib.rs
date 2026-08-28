//! A typed, offline-testable client for the herdr session daemon (herdr 0.8.2 / **protocol 20**).
//!
//! # SCAFFOLD STUB
//!
//! Build order step 1 only: constants and the crate-level traps. The modules named in the slice-1
//! spec (`client`, `error`, `handshake`, `ids`, `keys`, `proto`, `stream`, `transport`) and their
//! re-exports land in steps 4–8. Do not add a module here without its file.
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

// `pub(crate) const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;` lands with `transport.rs`
// (build order step 4). It is omitted here because an unused crate-private const is a `dead_code`
// warning, and the clippy gate is `-D warnings`.

/// Socket path relative to `$HOME`, used when `$HERDR_SOCKET_PATH` is absent — which is the normal
/// case for the production `systemd --user` unit, since every `HERDR_*` var is pane-injected only.
pub const DEFAULT_SOCKET_RELPATH: &str = ".config/herdr/herdr.sock";

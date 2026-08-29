//! [`HerdrClient`] — the typed RPC surface, and the [`Request`] trait that keeps method, params,
//! result tag and result type welded together.
//!
//! # What this module deliberately does NOT expose, permanently
//!
//! - **No `focus`.** `pane.focus` / `agent.focus` exist on the wire and are not reachable from
//!   here. Focusing a pane marks it **seen**, and `done` is defined as "idle after work the
//!   operator has not seen". A focus call from the bridge would destroy the very signal PLAN.md's
//!   second push trigger depends on. Absent from the API, so it cannot be called by accident —
//!   including by a timer.
//! - **No caller-chosen read source.** `ReadSource` is `pub(crate)` and the
//!   only two reads offered are [`HerdrClient::read_visible`] and
//!   [`HerdrClient::read_visible_tail`], both pinned to `visible`. `recent` and `recent_unwrapped`
//!   harvest-scroll the operator's **real** viewport when `lines > viewport_rows` (63 here) — the
//!   operator would watch their screen move. There is no public path to them, so no timer can
//!   reach one. (`ReadSource` is written as plain code here, never as an intra-doc link: a link
//!   from public docs to a private item is a hard error under `RUSTDOCFLAGS='-D warnings'`, and
//!   the fix for that error must never be to widen `ReadSource` to `pub` — that would undo the
//!   compiler-enforced no-`recent` property this whole section is about.)
//! - **No `agent.prompt`.** Slice 3. It refuses an already-blocked agent with `agent_blocked`
//!   *before* sending anything, which is precisely the case this product exists to serve.
//!
//! # One-shot RPC
//!
//! The client is stateless: a path and two timeouts. A second write on an answered connection is a
//! `BrokenPipe` (verified live), so there is no pool, no id-correlation map and no background
//! reader task. Cloning is cheap and gives an independent caller.

use std::io;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::HerdrError;
use crate::handshake::{Handshake, Pong};
use crate::ids::{PaneId, WorkspaceId};
use crate::keys::Key;
use crate::proto::event::{EVENTS_SUBSCRIBE, Subscription};
use crate::proto::model::{AgentInfo, PaneInfo, PaneRead, ReadSource, SessionSnapshot};
use crate::proto::request::{
    AgentListRequest, Envelope, EventsSubscribeRequest, PaneListRequest, PaneReadRequest,
    PaneSendInputRequest, PaneSendKeysRequest, PaneSendTextRequest, PingRequest, SnapshotRequest,
    next_request_id,
};
use crate::proto::response::{
    AgentListResult, OkResult, PaneListResult, ReadResult, Reply, SnapshotResult,
};
use crate::stream::EventStream;
use crate::{DEFAULT_SOCKET_RELPATH, transport};

/// The default dial timeout. A Unix socket either answers immediately or is not there.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// The default reply timeout. Generous: `session.snapshot` on a large herd is the slow one, and
/// this is also the only thing that catches a request that lost its trailing newline.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The seal. Private module, so `Sealed` is unnameable outside this crate and `Request` is
/// therefore unimplementable outside it — see the [`Request`] docs for why that is load-bearing.
///
/// `unreachable_pub` is allowed because the `pub` here is precisely the point: it must be `pub`
/// for `Request` to name it as a supertrait, and it must be unreachable for the seal to hold.
mod sealed {
    /// Implemented only for this crate's own request types, immediately below each `Request` impl.
    #[allow(unreachable_pub)]
    pub trait Sealed {}
}

/// Ties a method to its params, its result **tag** and its result **type**, so a mismatch between
/// them is unrepresentable rather than a runtime surprise.
///
/// `RESULT_TAG` is checked against the reply's `result.type` **before** the payload is unwrapped:
/// a method that answers with the wrong shape then yields [`HerdrError::UnexpectedResult`] naming
/// both tags, instead of a serde error about a field the operator has never heard of.
///
/// The tags are verified against the schema's 58 `ResponseResult` variants:
/// `ping`→`pong`, `session.snapshot`→`session_snapshot`, `pane.read`→`pane_read`,
/// `agent.list`→`agent_list`, `pane.list`→`pane_list`, `events.subscribe`→`subscription_started`,
/// and the three writes→`ok`.
///
/// # SEALED — and that is the whole D3 guarantee, not a style choice
///
/// The supertrait `sealed::Sealed` lives in a private module, so **no code outside this crate can
/// implement `Request`**, and therefore no code outside this crate can hand [`HerdrClient::call`] a
/// method name of its own choosing. Without the seal, `call` is a public generic escape hatch onto
/// the ENTIRE herdr method surface: ~20 lines in any downstream crate — a new workspace member,
/// slice 2's Telegram bridge, any library consumer — reach both catastrophic failures this product
/// exists to prevent, `pane.read {source:"recent", lines:5000}` (harvest-scrolls the operator's
/// real viewport) and `pane.send_text` (types into their real terminal), and every structural
/// guard is bypassed at once: `ReadSource` being `pub(crate)`, the two pinned
/// `PaneReadRequest::visible`/`PaneReadRequest::visible_tail` constructors, and
/// `tests/no_live_write_call_site.rs` (a `const METHOD` never has to spell `send_text` —
/// `concat!("pane.send", "_text")` is invisible to a grep).
///
/// The seal costs nothing: every impl already lives in this file. It turns
/// "no reachable write call site" from a property a grep hopes for into one the compiler enforces,
/// the same by-construction argument the crate already makes for `ReadSource`. If a downstream
/// escape hatch is ever genuinely wanted, unsealing it is a deliberate operator decision made at
/// that point — not a hole nobody chose.
pub trait Request: sealed::Sealed + Serialize + Send + Sync {
    /// The wire method name, e.g. `"session.snapshot"`.
    const METHOD: &'static str;
    /// The `result.type` tag this method answers with.
    const RESULT_TAG: &'static str;
    /// The payload shape under `result` — for every method but `ping`, a wrapper that models the
    /// per-method nesting key.
    type Response: DeserializeOwned + Send;
}

impl sealed::Sealed for PingRequest {}
impl Request for PingRequest {
    const METHOD: &'static str = "ping";
    const RESULT_TAG: &'static str = "pong";
    /// The ONE flat payload: pong's fields sit beside its tag rather than under a nesting key.
    type Response = Pong;
}

impl sealed::Sealed for SnapshotRequest {}
impl Request for SnapshotRequest {
    const METHOD: &'static str = "session.snapshot";
    const RESULT_TAG: &'static str = "session_snapshot";
    type Response = SnapshotResult;
}

impl sealed::Sealed for AgentListRequest {}
impl Request for AgentListRequest {
    const METHOD: &'static str = "agent.list";
    const RESULT_TAG: &'static str = "agent_list";
    type Response = AgentListResult;
}

impl sealed::Sealed for PaneListRequest<'_> {}
impl Request for PaneListRequest<'_> {
    const METHOD: &'static str = "pane.list";
    const RESULT_TAG: &'static str = "pane_list";
    type Response = PaneListResult;
}

impl sealed::Sealed for PaneReadRequest<'_> {}
impl Request for PaneReadRequest<'_> {
    const METHOD: &'static str = "pane.read";
    const RESULT_TAG: &'static str = "pane_read";
    type Response = ReadResult;
}

/// The three writes all answer with the same bare `{"type":"ok"}` tag.
///
/// `ok` is the only VOID result tag among the schema's 58 `ResponseResult` variants, which is where
/// the tag comes from — it is INFERRED, not observed, because observing it means typing into a real
/// pane (`scripts/verify-send-p20.sh` P1 settles it). And it carries **no delivery semantics**: see
/// [`WriteAccepted`].
impl sealed::Sealed for PaneSendTextRequest<'_> {}
impl Request for PaneSendTextRequest<'_> {
    const METHOD: &'static str = "pane.send_text";
    const RESULT_TAG: &'static str = "ok";
    type Response = OkResult;
}

impl sealed::Sealed for PaneSendKeysRequest<'_> {}
impl Request for PaneSendKeysRequest<'_> {
    const METHOD: &'static str = "pane.send_keys";
    const RESULT_TAG: &'static str = "ok";
    type Response = OkResult;
}

impl sealed::Sealed for PaneSendInputRequest<'_> {}
impl Request for PaneSendInputRequest<'_> {
    const METHOD: &'static str = "pane.send_input";
    const RESULT_TAG: &'static str = "ok";
    type Response = OkResult;
}

/// The one method whose connection OUTLIVES its reply, so it never goes through
/// [`HerdrClient::call`] — but it still declares its method and tag here, because that is the one
/// place a rename can be caught.
impl sealed::Sealed for EventsSubscribeRequest<'_> {}
impl Request for EventsSubscribeRequest<'_> {
    const METHOD: &'static str = EVENTS_SUBSCRIBE;
    const RESULT_TAG: &'static str = "subscription_started";
    /// The ack is a bare tag with no payload, exactly like a write's `ok`.
    type Response = OkResult;
}

/// A handle to one herdr socket.
#[derive(Clone, Debug)]
pub struct HerdrClient {
    socket_path: Arc<Path>,
    connect_timeout: Duration,
    request_timeout: Duration,
}

impl HerdrClient {
    /// A client for an explicit socket path, with the default 2 s connect / 10 s request timeouts.
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        HerdrClient {
            socket_path: Arc::from(socket_path.into()),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// `$HERDR_SOCKET_PATH`, else `$HOME/.config/herdr/herdr.sock`.
    ///
    /// **The env var is PANE-INJECTED ONLY** — verified: a stripped child sees no `HERDR_*` at all.
    /// So the production `systemd --user` unit WILL take the `$HOME` fallback, and that fallback is
    /// the path proof gate 2 exercises. The socket is `srw------- thev:thev`, so filesystem
    /// permissions ARE the auth layer; there is no token to configure.
    pub fn from_env() -> Result<Self, HerdrError> {
        let path = resolve_socket_path(
            std::env::var_os("HERDR_SOCKET_PATH"),
            std::env::var_os("HOME").map(PathBuf::from),
        )?;
        Ok(HerdrClient::new(path))
    }

    /// Override both timeouts. Consuming, so a configured client is a value rather than a mutable
    /// thing two tasks can disagree about.
    pub fn with_timeouts(mut self, connect: Duration, request: Duration) -> Self {
        self.connect_timeout = connect;
        self.request_timeout = request;
        self
    }

    /// The socket this client dials. `doctor` prints it, because "which socket" is the operator's
    /// first question when nothing answers.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// The dial timeout in force.
    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// The reply timeout in force.
    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// One request, one connection, one reply line.
    ///
    /// The order of checks is deliberate: `error` first (herdr's semantic refusal is the message
    /// the operator needs, verbatim), then the `result.type` tag, then the payload. The reply's
    /// `id` is never compared — see `proto::request`.
    pub async fn call<R: Request>(&self, req: &R) -> Result<R::Response, HerdrError> {
        let body = encode_request(req)?;

        let line = transport::round_trip(
            &self.socket_path,
            R::METHOD,
            &body,
            self.connect_timeout,
            self.request_timeout,
        )
        .await?;

        decode_reply::<R>(line)
    }

    /// The capability handshake's raw reply: version, protocol, capabilities.
    ///
    /// Applies no policy — [`HerdrClient::handshake`] is the one that decides whether to run.
    pub async fn ping(&self) -> Result<Pong, HerdrError> {
        self.call(&PingRequest::default()).await
    }

    /// `ping` plus the version policy.
    ///
    /// Errors [`HerdrError::ProtocolTooOld`] below [`crate::MIN_SUPPORTED_PROTOCOL`] (fatal, exit
    /// 4); warns ONCE above [`crate::KNOWN_PROTOCOL`] and proceeds.
    ///
    /// **MUST be re-run on every event-stream reconnect, not only at boot:**
    /// `capabilities.live_handoff` is `true` on this server, so herdr can swap its own binary
    /// underneath a running bridge without the socket path changing.
    pub async fn handshake(&self) -> Result<Handshake, HerdrError> {
        Handshake::evaluate(self.ping().await?)
    }

    /// The whole herd in one RPC: workspaces, tabs, panes, agents, and what is focused.
    ///
    /// **This is the only trustworthy source of agent status.** The event stream's globally
    /// subscribable `pane.updated` replays a stale, ageing backlog on every connect; truth comes
    /// from here.
    pub async fn snapshot(&self) -> Result<SessionSnapshot, HerdrError> {
        Ok(self.call(&SnapshotRequest::default()).await?.snapshot)
    }

    /// The agent roster only. Includes indeterminate statuses — filter with
    /// [`crate::proto::model::AgentStatus::is_indeterminate`] before deciding to push.
    pub async fn agents(&self) -> Result<Vec<AgentInfo>, HerdrError> {
        Ok(self.call(&AgentListRequest::default()).await?.agents)
    }

    /// Every pane, or just one workspace's.
    ///
    /// D2 (one bot per workspace): scope the roster **server-side** in one RPC. An unknown id
    /// returns a distinct `workspace_not_found` — [`HerdrError::is_not_found`] — so a bot whose
    /// workspace closed says so rather than reporting an empty herd.
    pub async fn panes(
        &self,
        workspace: Option<&WorkspaceId>,
    ) -> Result<Vec<PaneInfo>, HerdrError> {
        Ok(self
            .call(&PaneListRequest {
                workspace_id: workspace,
            })
            .await?
            .panes)
    }

    // ── reads ───────────────────────────────────────────────────────────────────────────────────
    // `recent` / `recent_unwrapped` / `detection` are UNREACHABLE from outside this crate. Both
    // methods below pin `source: "visible"` at construction; there is no parameter to override it.

    /// The safe background read: `{"pane_id":…,"source":"visible"}` with **no** `lines` key.
    ///
    /// It cannot trip the `lines > viewport_rows` scroll harvest even in principle, which is what
    /// makes it safe to call from a timer against the operator's live terminals.
    pub async fn read_visible(&self, pane: &PaneId) -> Result<PaneRead, HerdrError> {
        self.finish_read(self.call(&PaneReadRequest::visible(pane)).await?.read)
    }

    /// The visible screen with its colour escapes intact.
    ///
    /// Same `source: "visible"`, so the same safety: the scroll harvest is a property of `recent`,
    /// never of the format. Slice 3 needs it because a TUI renders its selected option as colour,
    /// and the text read strips exactly that.
    pub async fn read_visible_ansi(&self, pane: &PaneId) -> Result<PaneRead, HerdrError> {
        self.finish_read(self.call(&PaneReadRequest::visible_ansi(pane)).await?.read)
    }

    /// Also safe: `visible` is clamped to the viewport however large `lines` is (verified:
    /// `lines=200` against a 63-row viewport returned the full text with `truncated:false`).
    ///
    /// `NonZeroU32` because `lines=0` returns an empty string with `truncated:true` — a silently
    /// useless read rather than an error.
    pub async fn read_visible_tail(
        &self,
        pane: &PaneId,
        lines: NonZeroU32,
    ) -> Result<PaneRead, HerdrError> {
        self.finish_read(
            self.call(&PaneReadRequest::visible_tail(pane, lines))
                .await?
                .read,
        )
    }

    /// Belt-and-braces on the one thing that would move the operator's screen: we asked for
    /// `visible`, so herdr must have answered `visible`. A `debug_assert` rather than an error —
    /// this cannot happen without a server bug, and turning a working read into a failure would be
    /// a worse outcome than the log line.
    fn finish_read(&self, read: PaneRead) -> Result<PaneRead, HerdrError> {
        debug_assert_eq!(
            read.source,
            ReadSource::Visible,
            "pane.read answered with a source we never asked for"
        );
        if read.source != ReadSource::Visible {
            tracing::warn!(
                pane = %read.pane_id,
                source = read.source_name(),
                "pane.read answered with a source this client never requests"
            );
        }
        Ok(read)
    }

    // ── writes ──────────────────────────────────────────────────────────────────────────────────
    //
    // ⚠ NO LIVE CALL SITE IN SLICE 1, AND THIS IS THE POINT.
    //
    // These three methods type real keystrokes into the operator's REAL terminals, where REAL
    // agents are working. The catastrophic failure of a remote-control surface is words landing in
    // the wrong terminal (D3), so slice 1 ships them built, typed and mock-tested, with:
    //
    //   * no call site anywhere outside `#[cfg(test)]` — checked by
    //     `tests/no_live_write_call_site.rs`, which fails the suite if one appears;
    //   * no subcommand in `crates/herdr-tg` that reaches them, and no mention of their names in
    //     that crate's source at all;
    //   * the key grammar behind a validating newtype under an UNVERIFIED-ON-P20 banner
    //     ([`crate::keys`]), never a closed enum that would encode protocol-16 evidence as truth.
    //
    // Their live verification is `scripts/verify-send-p20.sh`, which refuses to run unless it is
    // pointed at a throwaway `herdr --session probe` socket that is provably not the operator's.

    /// Send literal text to a pane. **Does not submit it.**
    ///
    /// Returns [`WriteAccepted`], never `bool` — and there is no method named `deliver`. The wire
    /// ack is a bare `{"type":"ok"}` meaning *herdr took the bytes*. It does NOT mean the agent
    /// received, rendered, parsed or acted on them: a focused TUI dialog can swallow both the text
    /// and a following Enter with both RPCs reporting success. Slice 3's Telegram confirmation must
    /// say "accepted", never "delivered".
    ///
    /// ⚠ `HERDR_API.md`'s 0.7.4 finding is that this writes **raw** bytes, so a `\n` inside `text`
    /// is a real Enter — a multi-line reply relayed verbatim would execute line-by-line in the
    /// operator's terminal. Never retested on 0.8.2; `scripts/verify-send-p20.sh` P3 settles it
    /// before slice 3 sends anything. Nothing is sanitized here on purpose: only the caller knows
    /// whether a newline was meant, and silently rewriting an operator's words is its own failure.
    #[must_use = "an ack is not a delivery receipt; the WriteAccepted must be inspected, not dropped"]
    pub async fn send_text(&self, pane: &PaneId, text: &str) -> Result<WriteAccepted, HerdrError> {
        let OkResult {} = self
            .call(&PaneSendTextRequest {
                pane_id: pane,
                text,
            })
            .await?;
        Ok(WriteAccepted::new(pane.clone(), text.len()))
    }

    /// Send key presses to a pane, applied in order.
    ///
    /// Same ack semantics as [`HerdrClient::send_text`]: `WriteAccepted` means herdr took the keys,
    /// not that anything acted on them.
    ///
    /// ⚠ The key grammar is UNVERIFIED-ON-P20 — see [`crate::keys`]. [`Key`] guarantees only that
    /// no key is empty, whitespace-only, or carries a raw newline into the PTY; herdr's own
    /// validator is the authority and refuses the rest with `invalid_key`.
    #[must_use = "an ack is not a delivery receipt; the WriteAccepted must be inspected, not dropped"]
    pub async fn send_keys(
        &self,
        pane: &PaneId,
        keys: &[Key],
    ) -> Result<WriteAccepted, HerdrError> {
        if keys.is_empty() {
            // Not an error — herdr accepts it — but a write that cannot do anything is exactly the
            // "silently delivered nothing" shape this whole path exists to make impossible to
            // mistake for success.
            tracing::warn!(pane = %pane, "pane.send_keys called with NO keys; this is a no-op");
        }
        let OkResult {} = self
            .call(&PaneSendKeysRequest {
                pane_id: pane,
                keys,
            })
            .await?;
        Ok(WriteAccepted::new(pane.clone(), keys_len(keys)))
    }

    /// Protocol 20's atomic text + keys in ONE RPC.
    ///
    /// **Slice 3's intended product path.** It collapses the `send_text` → `send_keys` pair and
    /// removes the ordering question entirely. `PaneSendInputParams.required = ["pane_id"]`, so
    /// text-only, keys-only and text-then-keys are three shapes of one request; an absent field is
    /// omitted, never sent as `null`. This method replaces `agent.send`, which was REMOVED between
    /// protocol 16 and 20.
    ///
    /// Same ack semantics as the other two — [`WriteAccepted`] is not a delivery receipt.
    ///
    /// ⚠ UNVERIFIED: whether `pane.send_input` frames its text in **bracketed paste**. That matters
    /// because multi-line Telegram replies are this product's default case and the 0.7.4 finding on
    /// `send_text` was raw bytes. `scripts/verify-send-p20.sh` P3.
    #[must_use = "an ack is not a delivery receipt; the WriteAccepted must be inspected, not dropped"]
    pub async fn send_input(
        &self,
        pane: &PaneId,
        text: Option<&str>,
        keys: &[Key],
    ) -> Result<WriteAccepted, HerdrError> {
        if text.is_none() && keys.is_empty() {
            tracing::warn!(
                pane = %pane,
                "pane.send_input called with neither text nor keys; this is a no-op"
            );
        }
        let OkResult {} = self
            .call(&PaneSendInputRequest {
                pane_id: pane,
                text,
                keys,
            })
            .await?;
        Ok(WriteAccepted::new(
            pane.clone(),
            text.map_or(0, str::len) + keys_len(keys),
        ))
    }

    // ── events ──────────────────────────────────────────────────────────────────────────────────

    /// Open an event stream. Returns only AFTER the `{"result":{"type":"subscription_started"}}`
    /// ack has been consumed, so "subscribed" is a distinct awaitable moment and the ack can never
    /// leak out of [`EventStream`] as an event.
    ///
    /// # The two invisible traps, restated here because this is where they bite
    ///
    /// **1. One connection, two incompatible envelope encodings.** Lifecycle frames are snake_case
    /// with a redundant `data.type`; the product's one load-bearing event,
    /// `pane.agent_status_changed`, arrives dot-form with **no `type` inside `data` at all**. A
    /// `#[serde(tag = "type")]` model over `data` parses the lifecycle family and silently errors
    /// on every ask. [`crate::proto::event::decode_event`] is two-step and tagged on the OUTER
    /// `event` field for exactly this reason.
    ///
    /// **2. `pane.updated` replays a stale, ageing backlog on EVERY connect** at ~100 ms/frame,
    /// each frame carrying a historical `agent_status` (verified: `wB:p1` replaying revisions 6→19,
    /// all reading `"blocked"`). It is the only globally-subscribable status-bearing event, so a
    /// bridge that read status from it would fire a phantom-push burst on every reconnect.
    /// [`crate::proto::event::RosterEvent`] exposes no status at all, which is what makes that
    /// unwritable. Status truth comes from [`HerdrClient::snapshot`].
    ///
    /// # There is no global agent-status subscription
    ///
    /// Exactly 3 of the schema's 27 subscription variants take a `pane_id`, and
    /// `pane.agent_status_changed` is one of them — so slice 3 must fan out **one entry per agent
    /// pane**. `events.subscribe` then FREEZES the set at connect (there is no `events.update`), so
    /// a pane created later requires dropping this stream and opening a new one;
    /// [`EventStream::subscriptions`] returns the set verbatim so the loop can re-issue it.
    ///
    /// The stream does not self-heal. `None` means the server closed it, and reconnecting is the
    /// binary's job — see the [`crate::stream`] module docs.
    pub async fn subscribe(&self, subs: &[Subscription]) -> Result<EventStream, HerdrError> {
        // Not an error — herdr accepts it — but a stream that can never yield anything is exactly
        // the "silently delivers nothing" failure this whole path exists to prevent, so it is said
        // out loud rather than debugged at 2 a.m.
        if subs.is_empty() {
            tracing::warn!(
                "events.subscribe called with an EMPTY subscription set; this stream will never \
                 yield an event"
            );
        }

        let req = EventsSubscribeRequest {
            subscriptions: subs,
        };
        let body = encode_request(&req)?;

        let (ack, lines) = transport::open_stream(
            &self.socket_path,
            EventsSubscribeRequest::METHOD,
            &body,
            self.connect_timeout,
            self.request_timeout,
        )
        .await?;

        // The SAME error / tag / unwrap path every other method takes: a refusal keeps herdr's own
        // message, and a wrong shape is an `UnexpectedResult` naming both tags.
        let OkResult {} = decode_reply::<EventsSubscribeRequest<'_>>(ack)?;

        Ok(EventStream::new(lines, subs.to_vec()))
    }

    /// A liveness probe on a FRESH connection.
    ///
    /// There is no heartbeat on the event stream (>9 s of silence observed on a healthy one after
    /// the backlog drained), so liveness must be probed out-of-band. Verified that fresh RPCs work
    /// fine while a stream is held open — the stream does not serialize herdr.
    pub async fn is_alive(&self) -> bool {
        self.ping().await.is_ok()
    }
}

/// herdr **accepted** a write. NOT a delivery receipt, and deliberately not a `bool`.
///
/// # What an ack means, and the four things it does not
///
/// The wire ack for all three write methods is a bare `{"type":"ok"}` with no payload. It means
/// herdr took the bytes and handed them to the pane's PTY. It does **not** mean the agent
///
/// 1. received them (a focused TUI dialog can swallow the text *and* the Enter, with both RPCs
///    reporting success),
/// 2. rendered them,
/// 3. parsed them, or
/// 4. acted on them.
///
/// This type exists so that distinction cannot be lost at a call site. There is no `bool` on this
/// API that anyone could mistake for a delivery flag, no field named `delivered`, and no method
/// named `deliver` — because the one thing a phone-only operator must never be told is "sent" when
/// nothing arrived. Slice 3's Telegram confirmation says **"accepted"**.
///
/// `#[must_use]`, so an ignored write is a compile-time warning rather than a silent one.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct WriteAccepted {
    /// The pane the write was addressed to. Echoed from the request, because herdr's `ok` carries
    /// nothing at all — including no pane id to confirm the target.
    pub pane_id: PaneId,
    /// Payload size in UTF-8 bytes, computed **client-side** from what was sent: the text plus the
    /// key names. herdr reports no count, so this is an audit figure — how much this client handed
    /// over — and emphatically not how much anything received.
    pub bytes: usize,
    /// When the ack was decoded, on THIS host's clock. herdr stamps nothing.
    pub at: SystemTime,
}

impl WriteAccepted {
    fn new(pane_id: PaneId, bytes: usize) -> Self {
        WriteAccepted {
            pane_id,
            bytes,
            at: SystemTime::now(),
        }
    }
}

/// Total UTF-8 bytes of a key list, for [`WriteAccepted::bytes`].
fn keys_len(keys: &[Key]) -> usize {
    keys.iter().map(|k| k.as_str().len()).sum()
}

/// Serialize one request into the line body, newline EXCLUDED (`transport` frames it).
///
/// Shared by [`HerdrClient::call`] and [`HerdrClient::subscribe`] so both spell the envelope the
/// same way — `id` first, `params` always emitted, both mandatory on the wire.
fn encode_request<R: Request>(req: &R) -> Result<Vec<u8>, HerdrError> {
    let id = next_request_id();
    let envelope = Envelope {
        id: &id,
        method: R::METHOD,
        params: req,
    };

    // Structurally unreachable: every params type in this crate is plain data with derived
    // `Serialize`, and `serde_json` only fails on a custom impl that errors, a non-string map key,
    // or a non-finite float. Mapped rather than unwrapped anyway, because a bridge that has to
    // survive unattended never panics on a data path.
    serde_json::to_vec(&envelope).map_err(|source| HerdrError::Decode {
        method: R::METHOD,
        source,
        line: "<outbound request — this is a serialization failure, not a reply>".to_owned(),
    })
}

/// Turn one reply line into `R::Response`.
///
/// The order of checks is deliberate: `error` first (herdr's semantic refusal is the message the
/// operator needs, verbatim), then the `result.type` tag, then the payload. The reply's `id` is
/// never compared — see `proto::request`.
///
/// Shared with [`HerdrClient::subscribe`], whose ack takes exactly this path even though its
/// connection stays open afterwards.
fn decode_reply<R: Request>(line: String) -> Result<R::Response, HerdrError> {
    let mut reply: Reply = serde_json::from_str(&line).map_err(|source| HerdrError::Decode {
        method: R::METHOD,
        source,
        line: line.clone(),
    })?;

    // herdr said no. This is a semantic refusal, not a transport fault, and its message is the
    // whole payload as far as the operator is concerned.
    if let Some(body) = reply.error.take() {
        return Err(HerdrError::Protocol {
            method: R::METHOD,
            code: body.code,
            message: body.message,
        });
    }

    match reply.result_tag() {
        Some(tag) if tag == R::RESULT_TAG => {}
        got => {
            let got = match got {
                Some(other) => other.to_owned(),
                None if reply.result.is_none() => "<neither result nor error>".to_owned(),
                None => "<result carries no type tag>".to_owned(),
            };
            return Err(HerdrError::UnexpectedResult {
                method: R::METHOD,
                expected: R::RESULT_TAG,
                got,
            });
        }
    }

    // The tag matched, which required `result` to be an object; `Value::Null` is a panic-free
    // fallback that would surface as an ordinary decode error.
    let result = reply.result.unwrap_or(serde_json::Value::Null);
    serde_json::from_value(result).map_err(|source| HerdrError::Decode {
        method: R::METHOD,
        source,
        line,
    })
}

/// The socket-path policy, split out from [`HerdrClient::from_env`] so it is testable without
/// mutating the process environment (which is `unsafe` and racy across test threads).
///
/// An EMPTY `$HERDR_SOCKET_PATH` falls through to the `$HOME` default rather than dialing `""`.
fn resolve_socket_path(
    env_socket: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> Result<PathBuf, HerdrError> {
    if let Some(raw) = env_socket {
        if !raw.is_empty() {
            return Ok(PathBuf::from(raw));
        }
    }
    match home {
        Some(home) if !home.as_os_str().is_empty() => Ok(home.join(DEFAULT_SOCKET_RELPATH)),
        _ => Err(HerdrError::Connect {
            path: PathBuf::from(DEFAULT_SOCKET_RELPATH),
            source: io::Error::new(
                io::ErrorKind::NotFound,
                "neither $HERDR_SOCKET_PATH nor $HOME is set, so the herdr socket cannot be located",
            ),
        }),
    }
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Offline unit tests. These are the ones that need to NAME the crate-private request types — the
// public-API rows of the spec's `tests/wire.rs` and `tests/failure_paths.rs` tables live in
// `tests/`, against `HerdrClient` only.
//
// Nothing here touches ~/.config/herdr/herdr.sock.
// ───────────────────────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    /// Closes the drift hole `tests/schema_drift.rs` could not: that test checks nine method-name
    /// LITERALS against the schema, so a renamed `Request::METHOD` const would drift past it. These
    /// assertions anchor the consts to the same literals, in a file that can name the private
    /// request types. All nine methods are covered, the three writes included — a renamed write
    /// const would otherwise sail past `schema_drift` AND past every mock test, since a mock
    /// answers whatever it is asked.
    #[test]
    fn the_method_and_tag_consts_are_the_wire_names_schema_drift_checks() {
        assert_eq!(<PingRequest as Request>::METHOD, "ping");
        assert_eq!(<PingRequest as Request>::RESULT_TAG, "pong");

        assert_eq!(<SnapshotRequest as Request>::METHOD, "session.snapshot");
        assert_eq!(<SnapshotRequest as Request>::RESULT_TAG, "session_snapshot");

        assert_eq!(<AgentListRequest as Request>::METHOD, "agent.list");
        assert_eq!(<AgentListRequest as Request>::RESULT_TAG, "agent_list");

        assert_eq!(<PaneListRequest<'_> as Request>::METHOD, "pane.list");
        assert_eq!(<PaneListRequest<'_> as Request>::RESULT_TAG, "pane_list");

        assert_eq!(<PaneReadRequest<'_> as Request>::METHOD, "pane.read");
        assert_eq!(<PaneReadRequest<'_> as Request>::RESULT_TAG, "pane_read");

        assert_eq!(
            <EventsSubscribeRequest<'_> as Request>::METHOD,
            "events.subscribe"
        );
        assert_eq!(
            <EventsSubscribeRequest<'_> as Request>::RESULT_TAG,
            "subscription_started"
        );
        // The decoder, the stream's I/O errors and this const are one literal, not three.
        assert_eq!(EVENTS_SUBSCRIBE, "events.subscribe");

        // The three writes. `ok` is the only VOID result tag among the schema's 58, which is the
        // whole basis for asserting it — it has never been observed, because observing it means
        // typing into a real pane (scripts/verify-send-p20.sh P1).
        assert_eq!(
            <PaneSendTextRequest<'_> as Request>::METHOD,
            "pane.send_text"
        );
        assert_eq!(<PaneSendTextRequest<'_> as Request>::RESULT_TAG, "ok");

        assert_eq!(
            <PaneSendKeysRequest<'_> as Request>::METHOD,
            "pane.send_keys"
        );
        assert_eq!(<PaneSendKeysRequest<'_> as Request>::RESULT_TAG, "ok");

        assert_eq!(
            <PaneSendInputRequest<'_> as Request>::METHOD,
            "pane.send_input"
        );
        assert_eq!(<PaneSendInputRequest<'_> as Request>::RESULT_TAG, "ok");

        // The method that is GONE at protocol 20 and must never come back into this crate.
        for source in [
            include_str!("client.rs"),
            include_str!("proto/request.rs"),
            include_str!("keys.rs"),
        ] {
            assert!(
                !source.contains("\"agent.send\""),
                "`agent.send` was REMOVED between protocol 16 and 20; a call to it is an \
                 `invalid_request: unknown variant` this client cannot repair"
            );
        }
    }

    #[test]
    fn the_env_var_wins_and_an_empty_one_falls_back_to_home() {
        let home = PathBuf::from("/home/testuser");

        let explicit =
            resolve_socket_path(Some(OsString::from("/run/herdr.sock")), Some(home.clone()))
                .unwrap();
        assert_eq!(explicit, PathBuf::from("/run/herdr.sock"));

        let fallback = resolve_socket_path(None, Some(home.clone())).unwrap();
        assert_eq!(
            fallback,
            PathBuf::from("/home/testuser/.config/herdr/herdr.sock"),
            "the systemd --user unit sees no HERDR_* at all and MUST land here"
        );

        let empty = resolve_socket_path(Some(OsString::new()), Some(home)).unwrap();
        assert_eq!(
            empty,
            PathBuf::from("/home/testuser/.config/herdr/herdr.sock"),
            "an empty env var must not become a dial of \"\""
        );
    }

    #[test]
    fn no_home_and_no_env_is_a_typed_unreachable_not_a_panic() {
        let err = resolve_socket_path(None, None).expect_err("nothing to resolve");
        assert!(err.is_unreachable());
        assert_eq!(err.exit_code(), 3);
        assert!(err.to_string().contains(DEFAULT_SOCKET_RELPATH), "{err}");
    }

    #[test]
    fn timeouts_default_to_two_and_ten_seconds_and_are_overridable() {
        let c = HerdrClient::new("/tmp/nope.sock");
        assert_eq!(c.connect_timeout(), Duration::from_secs(2));
        assert_eq!(c.request_timeout(), Duration::from_secs(10));
        assert_eq!(c.socket_path(), Path::new("/tmp/nope.sock"));

        let c = c.with_timeouts(Duration::from_millis(50), Duration::from_millis(80));
        assert_eq!(c.connect_timeout(), Duration::from_millis(50));
        assert_eq!(c.request_timeout(), Duration::from_millis(80));
    }
}

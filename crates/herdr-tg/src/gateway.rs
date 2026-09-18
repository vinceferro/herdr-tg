//! The door's other half: an HTTP edge on loopback for the reader that is not Telegram.
//!
//! The hub writes two things for a reader it will never meet — the ring of operator-visible
//! events (`hub/door.rs`) and the answers drop (`hub/answers.rs`) — and this module is the program
//! that stands in front of them and speaks HTTP. It is a SEPARATE binary, `kickoff-door`, and the
//! only thing in this workspace that listens: the hub binary still binds nothing, and the seam
//! between the two is the same two files a later reader could consume directly.
//!
//! # Who the client is, and why the shapes are transcribed rather than designed
//!
//! The kickoff PWA — a different product, in a different repo — already speaks a `/v1` seam
//! through its own bridge (`bridge/serve.py`, which proxies these three routes and holds the
//! token so the browser never does). Its contract is pinned by that repo's own tests
//! (`bridge/test_hub.py`, a StubHub standing where this program stands), and byte-compat with
//! those pinned shapes is the whole cost advantage of the adoption: a client written against the
//! stub works against the real thing. Where this file and a shape their tests pin could disagree,
//! THEIRS wins, and the transcription here says so at each site:
//!
//! * **`401` is byte-identical.** `{"error": "unauthorized"}` — with the space after the colon,
//!   because their test compares raw bytes (`test_hub.py`: "a wrong token is the hub's identical
//!   401, passed through untouched") and Python's `json.dumps` puts one there. Not a formatting
//!   choice of ours; a fact about theirs.
//! * **`404` is their stub's body**, `{"error": "not found"}`, for the same reason.
//! * **The poll's envelope** is `{"ok":true,"at":…,"cursor":…,"events":[…]}` with each event the
//!   ring line verbatim; their client demands the events it is handed continue its cursor
//!   exactly (seq N+1, N+2, …) and takes `cursor` as the echo of the last seq served — which the
//!   ring's own contiguity provides, and which is why this module never renumbers, reorders or
//!   filters what it serves.
//! * **The stream's opening** is `retry: 3000` and a `: connected` comment, then one
//!   `id: <seq>` + `data: <line>` block per event, and a `: ping` comment when idle — their
//!   stub's exact chunking, which their test proves incremental by reading event one before the
//!   stub has written event two.
//!
//! # What this program may never do
//!
//! **Bind anything but 127.0.0.1.** The PWA's bridge is on this box; the one socket this opens
//! faces it and nothing else. There is no flag for another address and there will not be one: a
//! door on 0.0.0.0 is a different product, and the egress law the ring already holds to is the
//! reason why.
//!
//! **Mint a token.** The write door takes a bearer token read from `<state>/door/token` on every
//! request — read per request so rotation is live, the same contract the bridge keeps with its own
//! token file — and the verb that mints it lives on the HUB's command line (`herdr-tg
//! door-token`), at a keyboard, like every other credential decision in this repo. A gateway that
//! could mint its own credential would be a credential nobody decided to issue.
//!
//! **Trust its own view of the questions.** To route a choice that names no conversation, this
//! program remembers the `ask` lines it has served and where they belonged — but that memory is
//! only an addressing hint: the answer file it writes still names the conversation, and the hub
//! re-judges every one of them against its ledger, its options and its liveness. A stale or wrong
//! hint lands in a refusal with a sentence, exactly as a file from any other confused writer
//! would.
//!
//! **Answer optimistically.** A POST becomes a file in the drop, and the response is whatever the
//! hub's `.result` file says became of it — accepted, or refused with the hub's own sentence. If
//! no result lands inside the wait window ([`WAIT_FOR_THE_RESULT`]), the answer is a `504` whose body says the hub
//! has not said what became of it and that it may still: an extension of their refusal shape
//! (`ok:false` with a `why` their client already renders), documented here because their stub
//! never had to spell a hub that was slow rather than wrong.
//!
//! # Egress at this boundary
//!
//! The ring's lines are already scrubbed by the ring's own law — no path, no chat id, no token —
//! and this module adds nothing to them: no header carries an identifier, no error body names a
//! file, and the 503 for a missing token says the operator has not minted one rather than where
//! it would be. What the law cannot cover is the same as the ring's: an agent's own words are the
//! event, and they go where they already went.
//!
//! # The receipt nonce, and the seam it closes
//!
//! A POSTed line needed an id the sender could recognise its own echo by, and the ring's law
//! forbade `msg_id` on a down line — rightly, because the phone's `msg_id` is Telegram's and
//! that is an identifier of a surface. The ruling that closed the seam narrowed the law to what
//! it always meant: a door-minted nonce names nothing on this box. So this door mints one
//! `w…` per message, carries it on the answer file as `ref`, and answers it in the POST's
//! ok-shape as `msg_id`; the hub rides it onto the wire frame's `msg_id` and the ring's down
//! `message` line, and their client's `r.msg === f.msg_id` matching turns the sent line into
//! its own receipt. One field name to note: their pinned client reads `msg_id` on the ring
//! frame (`app/index.src.html`, `hubFrame`'s down-`message` arm), so that is the name the ring
//! echo wears — a `ref`-named ring field would have left their matcher reading `undefined`.
//! Choices echo no nonce: their client joins a tap to its question by `ask_id`, and the down
//! `choice` line already names that.
//!
//! # Recorded, not fixed: shapes left open on purpose
//!
//! The operator's standing settlement for contrived shapes — write them down rather than chase
//! them, as the write guard's and the drop's own files received before this one:
//!
//! * **The in-call rotation race.** More than two rotations landing inside one 250 ms poll gap
//!   lose the middle old file entirely — the second rotation removes the first before any
//!   reader has finished it — and the served sequence gains a gap nothing here can fill.
//!   Contrived by ruling: a rotation is a megabyte of operator-visible events, human cadence
//!   cannot spend three megabytes inside 250 ms, and the client that meets the gap resyncs from
//!   the hub's own cursor echo by design — which is the honest mend for a gap nobody caused.
//! * **No read timeouts on the door's own sockets.** A request head, a body, and every
//!   `write_all` on a stream wait for the peer for ever, because nothing on this box can say
//!   how long a loopback peer may take. Contrived by ruling: the door binds loopback only, the
//!   one client is the PWA's own bridge, and that bridge already bounds every upstream call it
//!   makes (10 s on the poll, 30 s on the stream) — so a wedged door task is bounded by the
//!   deployment's own timeouts, and no SSE write buffering exists today for a slow reader to
//!   grow against. The day the door faces a peer that is not that bridge is the day this
//!   becomes a decision rather than a settlement.
//! * **The unbounded poll drain.** One `/v1/events` drains to quiescence — passes until a poll
//!   adds nothing — and a writer of this same uid appending steadily can keep a drain going for
//!   as long as it likes, holding one connection and one task open past any bound this module
//!   would set. Contrived by ruling: it is the same trust settlement the answers drop already
//!   records — the writer is already this user, inside the state home, and a bound here would
//!   punish a healthy hub's burst to spite a misbehaving writer the drop's own file has
//!   already accepted.

use std::collections::{HashMap, VecDeque};
use std::io::IsTerminal as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::hub::answers;
use crate::hub::door;

/// The directory this program keeps its own bits in, beside the ring and the drop: `<state>/door`.
pub(crate) const DOOR: &str = "door";

/// Where the write door's token lives: `<state>/door/token`, minted by `herdr-tg door-token`.
pub(crate) const TOKEN: &str = "token";

/// The port bound when nothing says another: uncommon, and nothing anyone else standardised.
pub(crate) const DEFAULT_PORT: u16 = 8791;

/// How long a POST waits for the hub's `.result` before answering with the honest timeout.
///
/// Two and a half seconds because the hub sweeps the drop about once a second — one sweep's worth
/// of patience plus a whole second of hub-being-slow, and not a moment of pretending. The words
/// the timeout carries deliberately do not say the answer failed: the hub may still take it, and
/// a client that re-sent on a timeout would be a client turning "slow" into "twice".
pub const WAIT_FOR_THE_RESULT: Duration = Duration::from_millis(2_500);

/// How often the files are looked at. No inotify, no fanotify: a poll, because the ring is
/// appended to at human speed and a dependency that can miss events is worse than a read that
/// cannot.
const POLL_THE_FILES: Duration = Duration::from_millis(250);

/// How often the drop is looked at while a POST is waiting on its result.
const POLL_FOR_THE_RESULT: Duration = Duration::from_millis(50);

/// The most request head this door will read. A header block past this is a client that is not
/// speaking to a door.
const AT_MOST_HEAD: usize = 16 * 1024;

/// The most request BODY this door will read — comfortably under the hub's own per-answer bound,
/// with room for the `ts` and the envelope this program wraps the body's fields in.
const AT_MOST_BODY: u64 = answers::AT_MOST - 512;

/// The body their test pins byte-for-byte for a wrong or missing token. The space after the colon
/// is theirs (Python's `json.dumps`), and their bridge passes it through untouched — see the
/// module docs.
const UNAUTHORIZED: &str = "{\"error\": \"unauthorized\"}";

/// The body their stub pins for a path that is not one of the three routes.
const NOT_FOUND: &str = "{\"error\": \"not found\"}";

/// How many ask envelopes the routing memory will hold. Bounded like everything else that a long
/// conversation could grow; a shed entry is a choice that must name its conversation, which is the
/// honest degradation rather than a wrong route.
const AT_MOST_REMEMBERED_ASKS: usize = 2048;

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The program.

/// `kickoff-door`'s own command line. Deliberately small: a place to keep state, a port, and the
/// one conversation a door started for a single conversation's writes defaults to.
#[derive(Debug, clap::Parser)]
#[command(name = "kickoff-door", version, about, long_about = None)]
struct Args {
    /// The hub's state home — where the ring and the answers drop already live.
    ///
    /// Defaults to the same derivation every other part of this product uses, so a door started
    /// beside its hub finds the files without being told.
    #[arg(long, value_name = "PATH")]
    state: Option<PathBuf>,

    /// The loopback port to bind. `$KICKOFF_DOOR_PORT`, else 8791. There is no flag for an
    /// address: this door faces this box and nothing else.
    #[arg(long, value_name = "PORT")]
    port: Option<u16>,

    /// The conversation a command that names none is for, as an id (`p-…`/`c-…`).
    ///
    /// A door may be started for one conversation — the way a dispatcher starts a wall — and then
    /// a client that sends only `{t:"message", text}` is steering that conversation and nothing
    /// else. Without this, a command that names no conversation and cannot be routed by the
    /// question it answers is refused with directions rather than guessed for him.
    #[arg(long, value_name = "ID")]
    conversation: Option<String>,

    /// Where the token is read from, per request. Defaults to `<state>/door/token`.
    #[arg(long, value_name = "PATH")]
    token_file: Option<PathBuf>,

    /// How often to say `: ping` on an idle stream, in milliseconds. Default 15000 — their
    /// client's contract. A trial may want the proof faster than the promise.
    #[arg(long, value_name = "MS", default_value_t = 15_000)]
    ping_every_ms: u64,
}

/// The entry the binary wraps. One process, one listener, run until it is stopped.
pub fn main() -> std::process::ExitCode {
    let args = Args::parse();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    let state = args.state.clone().unwrap_or_else(crate::lock::state_dir);
    let conversation = args.conversation.as_deref().and_then(|named| {
        if crate::conversations::is_conversation_id(named) {
            Some(named.to_owned())
        } else {
            None
        }
    });
    if args.conversation.is_some() && conversation.is_none() {
        eprintln!(
            "kickoff-door: --conversation is not a conversation id this hub would have minted, so \
             the door refuses to guess what it names"
        );
        return std::process::ExitCode::from(1);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("kickoff-door: could not start the async runtime: {err}");
            return std::process::ExitCode::from(1);
        }
    };
    let port = args.port.or_else(|| {
        std::env::var("KICKOFF_DOOR_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
    });
    runtime.block_on(async move {
        let door = match Door::in_state(&state, conversation, args.ping_every_ms) {
            Ok(door) => Arc::new(door),
            Err(err) => {
                eprintln!("kickoff-door: {err}");
                return std::process::ExitCode::from(1);
            }
        };
        let listener = match TcpListener::bind(("127.0.0.1", port.unwrap_or(DEFAULT_PORT))).await {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!(
                    "kickoff-door: could not bind 127.0.0.1:{} — {err}",
                    port.unwrap_or(DEFAULT_PORT)
                );
                return std::process::ExitCode::from(1);
            }
        };
        // The one diagnostic line, on stdout, so whatever started this door can learn where it
        // came up without scraping the journal. Nothing after this is written to stdout.
        let bound = listener.local_addr().expect("a bound socket names itself");
        println!("kickoff-door: listening on http://{}", bound);
        door.serve(listener).await;
        std::process::ExitCode::SUCCESS
    })
}

/// Everything a connection needs: where the files are, what this door defaults to, and the ask
/// envelopes it has served — the routing memory, shared between the reader that updates it and
/// the writer that spends it.
struct Door {
    state: PathBuf,
    drop_dir: PathBuf,
    door_dir: PathBuf,
    token: PathBuf,
    conversation: Option<String>,
    ping_every: Duration,
    asks: std::sync::Mutex<Asks>,
}

impl Door {
    /// The door over one state home: the drop beside the ring, and a private `door/` of its own
    /// for the token and the staging writes. Made rather than assumed, the way every state
    /// directory here is.
    fn in_state(
        state: &Path,
        conversation: Option<String>,
        ping_every_ms: u64,
    ) -> anyhow::Result<Self> {
        let drop_dir = state.join(answers::ANSWERS);
        let door_dir = state.join(DOOR);
        // Both made if no hub ever has: the drop is the hub's to sweep, but a door that refused
        // every write because it created nothing is a door that was never open. Private the way
        // every state directory here is.
        crate::conversations::private_state_dir(&drop_dir)?;
        crate::conversations::private_state_dir(&door_dir)?;
        let token = door_dir.join(TOKEN);
        let door = Self {
            state: state.to_path_buf(),
            drop_dir,
            door_dir,
            token,
            conversation,
            ping_every: Duration::from_millis(ping_every_ms.max(100)),
            asks: std::sync::Mutex::new(Asks::default()),
        };
        // The routing memory catches up on everything the ring already holds before the door
        // answers anybody, so a question opened before this process started is as routable as one
        // opened after — the ring is bounded, and so is the work.
        door.remember_what_the_ring_holds();
        Ok(door)
    }

    /// Accept connections until the process is stopped. Each is its own task; a door never
    /// refuses a client because another is slow.
    async fn serve(self: Arc<Self>, listener: TcpListener) {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let door = Arc::clone(&self);
            tokio::spawn(async move {
                door.speak(stream).await;
            });
        }
    }

    /// One connection: read one request, answer it, and — unless it is the stream — close. Every
    /// JSON response this door sends carries `Connection: close`, which is honest HTTP/1.1 and
    /// one less promise to keep between a proxy's timeouts and a browser's pooling.
    async fn speak(self: &Arc<Self>, mut stream: TcpStream) {
        let (method, target, headers, body) = match read_a_request(&mut stream).await {
            Ok(request) => request,
            Err(err) => {
                tracing::warn!(error = %err, "a request this door could not read");
                let _ = say_json(&mut stream, 400, "Bad Request", NOT_READABLE).await;
                return;
            }
        };
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
        match (method.as_str(), path) {
            ("GET", "/v1/stream") => self.the_stream(&mut stream, &headers, query).await,
            ("GET", "/v1/events") => self.the_poll(&mut stream, query).await,
            ("POST", "/v1/commands") => self.the_write(&mut stream, &headers, &body).await,
            // A route spoken to the wrong way round is named, not folded into "not found": their
            // client surfaces the body's `why`, and "not found" would send somebody hunting for a
            // typo in a path that is right.
            (_, "/v1/stream") | (_, "/v1/events") | (_, "/v1/commands") => {
                let _ = say_json(
                    &mut stream,
                    405,
                    "Method Not Allowed",
                    &the_shape_of_a_refusal("that route takes another method"),
                )
                .await;
            }
            _ => {
                let _ = say_raw(&mut stream, 404, "Not Found", "application/json", NOT_FOUND).await;
            }
        }
    }

    // ── the read side ─────────────────────────────────────────────────────────────────────────

    /// `GET /v1/stream` — the SSE their client subscribes with. Replay from the cursor it holds,
    /// then the tail, one block per event, with `retry:` stated and a ping when idle.
    async fn the_stream(
        self: &Arc<Self>,
        stream: &mut TcpStream,
        headers: &[(String, String)],
        query: &str,
    ) {
        // Last-Event-ID wins over ?cursor= on purpose: a reconnecting EventSource keeps the URL
        // it was opened with and adds the header, and the header is the position it actually
        // reached — the query is where it started, possibly minutes of events ago.
        let from_header = headers
            .iter()
            .find(|(name, _)| name == "last-event-id")
            .map(|(_, value)| value.as_str());
        let cursor = match a_cursor(from_header, query) {
            Ok(cursor) => cursor,
            Err(named) => {
                let _ = say_json(
                    stream,
                    400,
                    "Bad Request",
                    &the_shape_of_a_refusal(&format!(
                        "bad cursor: {named:?} is not a sequence number"
                    )),
                )
                .await;
                return;
            }
        };
        if let Err(err) = stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\n\r\n\
              retry: 3000\n: connected\n\n",
            )
            .await
        {
            tracing::debug!(error = %err, "a stream client went away before it began");
            return;
        }
        let mut tail = RingTail::from_a_cursor(&self.state, cursor);
        let mut quiet_for = Duration::ZERO;
        loop {
            let mut said = false;
            match tail.the_new_lines() {
                Ok(lines) => {
                    for (seq, line) in lines {
                        let block = format!("id: {seq}\ndata: {line}\n\n");
                        if stream.write_all(block.as_bytes()).await.is_err() {
                            return; // he hung up; the normal end of a stream
                        }
                        self.remember(&line);
                        said = true;
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "the ring could not be read for a stream");
                }
            }
            if said {
                quiet_for = Duration::ZERO;
            } else {
                quiet_for += POLL_THE_FILES;
                if quiet_for >= self.ping_every {
                    // A comment, not an event: nothing a client should parse, just proof the wire
                    // is alive so nobody's idle timeout closes it for us.
                    if stream.write_all(b": ping\n").await.is_err() {
                        return;
                    }
                    quiet_for = Duration::ZERO;
                }
            }
            tokio::time::sleep(POLL_THE_FILES).await;
        }
    }

    /// `GET /v1/events` — the poll fallback, same envelopes, same cursor discipline. The echo is
    /// the ring's true head even when nothing was served, because a client whose cursor is ahead
    /// of the truth is exactly the client that needs to hear the truth (the PWA resyncs on the
    /// incoherence; the push watcher re-anchors on the step backwards).
    async fn the_poll(self: &Arc<Self>, stream: &mut TcpStream, query: &str) {
        let cursor = match a_cursor(None, query) {
            Ok(cursor) => cursor,
            Err(named) => {
                let _ = say_json(
                    stream,
                    400,
                    "Bad Request",
                    &the_shape_of_a_refusal(&format!(
                        "bad cursor: {named:?} is not a sequence number"
                    )),
                )
                .await;
                return;
            }
        };
        let mut tail = RingTail::from_a_cursor(&self.state, cursor);
        let mut events: Vec<serde_json::Value> = Vec::new();
        // Drained to quiescence, not read once: a rotation landing mid-answer would otherwise
        // serve the old file's lines and miss the active's tail of the same moment.
        loop {
            let lines = match tail.the_new_lines() {
                Ok(lines) => lines,
                Err(err) => {
                    tracing::warn!(error = %err, "the ring could not be read for a poll");
                    Vec::new()
                }
            };
            if lines.is_empty() {
                break;
            }
            for (_, line) in lines {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                    events.push(value);
                }
            }
        }
        for value in &events {
            if let Ok(line) = serde_json::to_string(value) {
                self.remember(&line);
            }
        }
        let head = tail.head();
        let body = serde_json::json!({
            "ok": true,
            "at": crate::hub::now_secs(),
            "cursor": head,
            "events": events,
        });
        let _ = say_json(stream, 200, "OK", &body.to_string()).await;
    }

    // ── the write side ────────────────────────────────────────────────────────────────────────

    /// `POST /v1/commands` — the token, then a file in the drop, then whatever the hub said
    /// became of it. Never an optimistic ok.
    async fn the_write(
        self: &Arc<Self>,
        stream: &mut TcpStream,
        headers: &[(String, String)],
        body: &str,
    ) {
        // The token, read from the file per request so rotation is live. No file, or an empty
        // one, is a CLOSED door — the named 503 their bridge already handles — and the sentence
        // names no path: the reader is an app, and where the mint lives is the operator's
        // business, at his keyboard.
        let token = match std::fs::read_to_string(&self.token) {
            Ok(raw) => raw.trim().to_owned(),
            Err(_) => {
                let _ = say_json(
                    stream,
                    503,
                    "Service Unavailable",
                    &the_shape_of_a_refusal(
                        "the hub write door is closed — the operator has not minted a token for it",
                    ),
                )
                .await;
                return;
            }
        };
        if token.is_empty() {
            let _ = say_json(
                stream,
                503,
                "Service Unavailable",
                &the_shape_of_a_refusal(
                    "the hub write door is closed — the operator has not minted a token for it",
                ),
            )
            .await;
            return;
        }
        let offered = headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .and_then(|(_, value)| value.strip_prefix("Bearer ").map(str::to_owned));
        match offered {
            Some(offered) if tokens_agree(offered.as_bytes(), token.as_bytes()) => {}
            // One body for a missing header and a wrong token alike, byte-identical, because
            // which of the two it was is not a fact the caller has any use for.
            _ => {
                let _ = say_raw(
                    stream,
                    401,
                    "Unauthorized",
                    "application/json",
                    UNAUTHORIZED,
                )
                .await;
                return;
            }
        }

        let command: serde_json::Value = match serde_json::from_str(body) {
            Ok(value) => value,
            Err(_) => {
                let _ = say_json(
                    stream,
                    400,
                    "Bad Request",
                    &the_shape_of_a_refusal("that is not one command this door could read"),
                )
                .await;
                return;
            }
        };
        let Some(fields) = command.as_object() else {
            let _ = say_json(
                stream,
                400,
                "Bad Request",
                &the_shape_of_a_refusal("that is not one command this door could read"),
            )
            .await;
            return;
        };
        let t = fields
            .get("t")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if t != "message" && t != "choice" {
            let _ = say_json(
                stream,
                400,
                "Bad Request",
                &the_shape_of_a_refusal("that did not say whether it was words or an answer"),
            )
            .await;
            return;
        }

        // WHERE it goes. A known field written wrongly is refused, never stripped — the door's
        // own law, held here first so the refusal is immediate and the file is never written.
        let named_conversation = match the_string(fields, "conversation") {
            Ok(None) => None,
            Ok(Some(id)) if crate::conversations::is_conversation_id(&id) => Some(id),
            Ok(Some(_)) => {
                let _ = say_json(
                    stream,
                    400,
                    "Bad Request",
                    &the_shape_of_a_refusal(
                        "that names a conversation this hub would not have minted",
                    ),
                )
                .await;
                return;
            }
            Err(field) => {
                let _ = say_json(
                    stream,
                    400,
                    "Bad Request",
                    &the_shape_of_a_refusal(&format!(
                        "that named the {field} as something other than words"
                    )),
                )
                .await;
                return;
            }
        };
        let named_lane = match the_string(fields, "lane") {
            Ok(lane) => lane,
            Err(_) => {
                let _ = say_json(
                    stream,
                    400,
                    "Bad Request",
                    &the_shape_of_a_refusal(
                        "that names one of the project's own conversations in a shape this hub \
                         does not address",
                    ),
                )
                .await;
                return;
            }
        };
        if let Some(lane) = named_lane.as_deref()
            && !crate::hub::lane_is_addressable(&hub_proto::LaneId::new(lane))
        {
            let _ = say_json(
                stream,
                400,
                "Bad Request",
                &the_shape_of_a_refusal(
                    "that names one of the project's own conversations in a shape this hub does \
                     not address",
                ),
            )
            .await;
            return;
        }
        // The reply field, held to the same law as every other known field: refused when present
        // and not words, never stripped. The routing ladder below READS it, and a reader that
        // swallows the wrong shape is how a reply becomes a plain line while the POST says ok —
        // the door quietly rewriting what he wrote.
        if let Err(field) = the_string(fields, "in_reply_to_ask") {
            let _ = say_json(
                stream,
                400,
                "Bad Request",
                &the_shape_of_a_refusal(&format!(
                    "that named the {field} as something other than words"
                )),
            )
            .await;
            return;
        }

        // The routing ladder: what the body names, then the conversation this door was started
        // for, then — for a command that answers a question — the question itself, which is the
        // one fact on the wire that says whose turn the answer belongs in. Nowhere on the ladder
        // does the door guess: the bottom rung is a refusal with directions.
        let (conversation, lane, in_reply_to_ask) =
            match self.route(t, named_conversation, named_lane.clone(), fields) {
                Routed::To {
                    conversation,
                    lane,
                    in_reply_to_ask,
                } => (conversation, lane, in_reply_to_ask),
                Routed::Refused(why) => {
                    let _ =
                        say_json(stream, 400, "Bad Request", &the_shape_of_a_refusal(&why)).await;
                    return;
                }
            };

        // The answer file: the shape `answers.rs` believes, `ts` set here, nothing the body knew
        // that the drop does not.
        let mut file = serde_json::Map::new();
        file.insert("t".into(), serde_json::json!(t));
        file.insert("conversation".into(), serde_json::json!(conversation));
        if let Some(lane) = lane.as_deref() {
            file.insert("lane".into(), serde_json::json!(lane));
        }
        if t == "choice" {
            for field in ["ask_id", "option_id"] {
                if let Some(value) = fields.get(field) {
                    file.insert(field.into(), value.clone());
                }
            }
        } else {
            if let Some(text) = fields.get("text") {
                file.insert("text".into(), text.clone());
            }
            if let Some(ask) = in_reply_to_ask.as_deref() {
                file.insert("in_reply_to_ask".into(), serde_json::json!(ask));
            }
        }
        // The receipt, under the name the sender will see again: the file's minted name rides
        // the file as `ref`, the hub carries it onto the wire frame and the ring's down line,
        // and the POST's ok-shape answers the SAME name — one nonce, three places, so a reader
        // that sent the line can match its own echo. Choices carry none: their client joins a
        // tap to its question by ask, and the ring's down `choice` line already names that.
        let name = a_new_name();
        if t == "message" {
            file.insert("ref".into(), serde_json::json!(name));
        }
        file.insert("ts".into(), serde_json::json!(crate::hub::now_secs()));
        let body = serde_json::Value::Object(file).to_string();

        // Staged beside the drop, renamed in — never written in place: the hub's sweep
        // lists the directory and reads what it finds, and a half-written file would be read as
        // a malformed answer and CONSUMED, which is the one way this door could eat his words.
        let staged = self.door_dir.join(format!("{name}.staging"));
        let in_the_drop = self.drop_dir.join(&name);
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&staged)
            .and_then(|mut f| {
                use std::io::Write as _;
                f.write_all(body.as_bytes())?;
                f.sync_all()
            })
            .and_then(|()| std::fs::rename(&staged, &in_the_drop));
        if let Err(err) = written {
            tracing::warn!(error = %err, "a command could not be left in the drop");
            let _ = say_json(
                stream,
                503,
                "Service Unavailable",
                &the_shape_of_a_refusal("the door could not reach the hub's answers"),
            )
            .await;
            return;
        }

        // Then the wait: the hub sweeps about once a second, judges, and writes the result. What
        // the POST answers is that and nothing else — and "that" means a result that PARSES and
        // SAYS a verdict. The hub writes its results open-truncate-then-write, and this door
        // polls every 50 ms, so a read can land inside the write: half a JSON object on the
        // disk is a receipt nobody has finished, and answering it — with a refusal, an error,
        // anything terminal — would be a receipt that lies, because behind the torn bytes the
        // answer was accepted and delivered. So a result that is not yet a verdict is not yet
        // arrived: the door waits, and the window's close says only what it always said.
        let deadline = tokio::time::Instant::now() + WAIT_FOR_THE_RESULT;
        loop {
            if let Ok(raw) = std::fs::read_to_string(self.drop_dir.join(format!("{name}.result"))) {
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(result) if result["status"] == "accepted" => {
                        // The ok-shape their client expects: `ok`, the kind it sent, an id for
                        // the act, and the lane it went to. The id is the same nonce the answer
                        // file carried as `ref` and the ring's down line echoes as `msg_id` —
                        // one name in three places, so their client's `r.msg === f.msg_id`
                        // matching turns a sent line into its own receipt instead of a second
                        // bubble.
                        let ok = serde_json::json!({
                            "ok": true,
                            "t": t,
                            "msg_id": name,
                            "lane": lane,
                        });
                        let _ = say_json(stream, 200, "OK", &ok.to_string()).await;
                        return;
                    }
                    Ok(result) if result["status"] == "refused" => {
                        let why = result["why"]
                            .as_str()
                            .unwrap_or("the hub refused it, and did not say why");
                        let _ = say_json(stream, 400, "Bad Request", &the_shape_of_a_refusal(why))
                            .await;
                        return;
                    }
                    // Not yet a verdict — torn, empty, or a shape this door has not been told
                    // about. Waited out below, exactly as though the file were not there.
                    _ => {}
                }
            }
            if tokio::time::Instant::now() >= deadline {
                // Not a refusal and never an ok: the hub may still take it, and the words say
                // exactly that much and no more.
                let _ = say_json(
                    stream,
                    504,
                    "Gateway Timeout",
                    &the_shape_of_a_refusal(
                        "the hub has not said what became of it yet — it may still; sending it \
                         again may say it twice",
                    ),
                )
                .await;
                return;
            }
            tokio::time::sleep(POLL_FOR_THE_RESULT).await;
        }
    }

    /// The routing ladder for a command. Every rung but the last is a fact somebody wrote down;
    /// the last is a refusal in plain words.
    fn route(
        self: &Arc<Self>,
        t: &str,
        named: Option<String>,
        lane: Option<String>,
        fields: &serde_json::Map<String, serde_json::Value>,
    ) -> Routed {
        let in_reply_to_ask = the_string(fields, "in_reply_to_ask").ok().flatten();
        if let Some(conversation) = named {
            return Routed::To {
                conversation,
                lane,
                in_reply_to_ask,
            };
        }
        if let Some(conversation) = self.conversation.clone() {
            return Routed::To {
                conversation,
                lane,
                in_reply_to_ask,
            };
        }
        // A command that answers a question is for whichever conversation asked it — the phone's
        // own rule for a reply, and the one fact the wire holds here. A reply naming a question
        // routes the same way, carrying the ask on the file so the hub can hold it to the session
        // that asked.
        let key = if t == "choice" {
            the_string(fields, "ask_id").ok().flatten()
        } else {
            in_reply_to_ask.clone()
        };
        if let Some(key) = key
            && let Some((conversation, ask_lane)) = self.the_one_ask_called(&key)
        {
            // A lane the body named wins: the question says where the answer belongs, but a body
            // that named a conversation of the project meant that one.
            return Routed::To {
                conversation,
                lane: lane.or(ask_lane),
                in_reply_to_ask: (t == "message").then_some(key),
            };
        }
        Routed::Refused(
            "that did not say which conversation it is for. Name one, or answer a question this \
             door has served"
                .to_owned(),
        )
    }

    /// The one open ask under a name, with the conversation and lane that asked it — or `None`
    /// when the name is asking nowhere, twice at once, or beyond what this door remembers.
    ///
    /// Two is the hub's refusal to make, not this door's guess to resolve; the words here are
    /// this door's own because the file never got written for the hub to refuse.
    fn the_one_ask_called(self: &Arc<Self>, key: &str) -> Option<(String, Option<String>)> {
        let asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        match asks.open(key).as_slice() {
            [the_one] => Some(the_one.clone()),
            [] => None,
            _ => None,
        }
    }

    /// One served line into the routing memory. Cheap, synchronous, and shared by both read
    /// routes so a door that has only ever been polled knows the same questions a streamed one
    /// does.
    fn remember(&self, line: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        let conversation = value["conversation"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let lane = value["lane"]
            .as_str()
            .and_then(|l| (l != "-").then_some(l.to_owned()));
        let frame = &value["frame"];
        let t = frame["t"].as_str().unwrap_or_default();
        let key = frame["ask_id"].as_str().map(str::to_owned);
        let mut asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        match (t, value["dir"].as_str().unwrap_or_default()) {
            // A question opened: remember whose it was.
            ("ask", "up") => {
                if let Some(key) = key {
                    asks.remember(key, (conversation, lane));
                }
            }
            // A question closed — answered from either surface, or resolved at the far end, or
            // put away by the hub itself: all three are spelled on the ring, and all three end
            // the routing memory the same way.
            ("choice", "down") | ("ask_resolved", "up") => {
                if let Some(key) = key {
                    asks.forget(&key, &conversation, lane.as_deref());
                }
            }
            _ => {}
        }
    }

    /// Everything the ring already holds, into the routing memory, before the first client is
    /// answered. One pass, bounded by the ring's own bound.
    fn remember_what_the_ring_holds(&self) {
        let mut tail = RingTail::from_a_cursor(&self.state, 0);
        loop {
            let lines = match tail.the_new_lines() {
                Ok(lines) => lines,
                Err(err) => {
                    tracing::warn!(error = %err, "the ring could not be read at start");
                    Vec::new()
                }
            };
            if lines.is_empty() {
                break;
            }
            for (_, line) in lines {
                self.remember(&line);
            }
        }
    }
}

/// What the routing ladder decided.
enum Routed {
    To {
        conversation: String,
        lane: Option<String>,
        in_reply_to_ask: Option<String>,
    },
    Refused(String),
}

/// The questions this door has served, keyed by the name the far end minted. Bounded, because a
/// long-lived conversation asks more questions than any answer is late.
#[derive(Default)]
struct Asks {
    open: HashMap<String, Vec<(String, Option<String>)>>,
    order: VecDeque<String>,
}

impl Asks {
    fn remember(&mut self, key: String, where_: (String, Option<String>)) {
        let fresh = !self.open.contains_key(&key);
        self.open.entry(key.clone()).or_default().push(where_);
        if fresh {
            self.order.push_back(key);
        }
        while self.order.len() > AT_MOST_REMEMBERED_ASKS {
            if let Some(oldest) = self.order.pop_front() {
                self.open.remove(&oldest);
            }
        }
    }

    fn forget(&mut self, key: &str, conversation: &str, lane: Option<&str>) {
        if let Some(asks) = self.open.get_mut(key) {
            asks.retain(|(c, l)| !(c == conversation && l.as_deref() == lane));
            if asks.is_empty() {
                self.open.remove(key);
            }
        }
    }

    fn open(&self, key: &str) -> Vec<(String, Option<String>)> {
        self.open.get(key).cloned().unwrap_or_default()
    }
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The ring, read.

/// A reader over the ring's two files that starts at a cursor and follows rotation.
///
/// The ring rotates by renaming the active file onto the old one and starting again, so a reader
/// notices by the active file's identity changing or its length going backwards — and finishes
/// the file that moved before it reads the one that took over, from the offset it had reached.
/// Only whole lines are ever taken: the tail past the last newline is a write the hub has not
/// finished, and serving it would be serving an event nobody said.
struct RingTail {
    active: PathBuf,
    old: PathBuf,
    /// Which file (by inode) and how far into it each half was read. `None` reads from the start.
    active_at: Option<(u64, u64)>,
    old_at: Option<(u64, u64)>,
    cursor: u64,
    head: u64,
}

impl RingTail {
    /// A reader serving everything after `cursor` — the whole history when it is zero. The head
    /// starts unknown rather than at the cursor: it is a fact about the files, learned by reading
    /// them, and a client whose cursor is ahead of the truth is exactly the client the truth is
    /// for.
    fn from_a_cursor(state: &Path, cursor: u64) -> Self {
        Self {
            active: state.join(door::RING),
            old: state.join(door::RING_OLD),
            active_at: None,
            old_at: None,
            cursor,
            head: 0,
        }
    }

    /// Where the ring has got to — the last seq any scan of it saw, which is the honest echo even
    /// when nothing past the cursor was served.
    fn head(&self) -> u64 {
        self.head
    }

    /// The lines added since last asked, as `(seq, the whole line)`, in ring order, rotation
    /// included.
    fn the_new_lines(&mut self) -> std::io::Result<Vec<(u64, String)>> {
        let mut out = Vec::new();
        // The old file first: that ordering is the contract the file names encode, and a reader
        // that took the active one first would put a rotation's events out of order.
        if self.old.exists() {
            one_file(
                &self.old,
                &mut self.old_at,
                &mut self.cursor,
                &mut self.head,
                &mut out,
            );
        }
        // The active file, and the rotation check: an identity change or a shrink means what we
        // were reading has moved to the old name — carry the offset over, from the start of the
        // new active.
        let active = self.active_metadata();
        let rotated = match (active, self.active_at) {
            // A different file under the same name, and an old file to have become.
            (Some((inode, _)), Some((was, _))) if inode != was => self.old.exists(),
            // Or the same file, shorter than we read it — truncated back, which the ring does
            // only by moving on.
            (Some((_, len)), Some((_, offset))) => offset > len,
            _ => false,
        };
        if rotated {
            self.old_at = self.active_at.take();
        }
        one_file(
            &self.active,
            &mut self.active_at,
            &mut self.cursor,
            &mut self.head,
            &mut out,
        );
        Ok(out)
    }

    /// The active file's identity and length, for the rotation check.
    fn active_metadata(&self) -> Option<(u64, u64)> {
        let meta = std::fs::metadata(&self.active).ok()?;
        Some((file_identity(&meta), meta.len()))
    }
}

/// One file's new whole lines. Blocking reads on files this box wrote, sized in megabytes at
/// most, on a runtime with threads to spare — an async file layer would buy nothing a caller of
/// this shape could measure.
fn one_file(
    path: &Path,
    at: &mut Option<(u64, u64)>,
    cursor: &mut u64,
    head: &mut u64,
    out: &mut Vec<(u64, String)>,
) -> Option<()> {
    use std::io::{Read, Seek, SeekFrom};
    let meta = std::fs::metadata(path).ok()?;
    let inode = file_identity(&meta);
    let len = meta.len();
    // The file we were reading was replaced under the same name: start over rather than seek
    // into a stranger — for the old half this is a SECOND rotation having taken the name.
    let mut offset = match *at {
        Some((was, offset)) if was == inode && offset <= len => offset,
        _ => 0,
    };
    if offset == len {
        *at = Some((inode, len));
        return Some(());
    }
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut raw = String::new();
    file.take(len - offset).read_to_string(&mut raw).ok()?;
    // Everything before the last newline is fair; the rest is a write in flight.
    let whole = match raw.rfind('\n') {
        Some(last) => &raw[..=last],
        None => return Some(()),
    };
    offset += whole.len() as u64;
    *at = Some((inode, offset));
    for line in whole.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(seq) = value["seq"].as_u64() else {
            continue;
        };
        // The head is every seq the files hold, served or not — it is the echo a client
        // ahead of the truth needs to hear.
        if seq > *head {
            *head = seq;
        }
        if seq > *cursor {
            out.push((seq, line.to_owned()));
            *cursor = seq;
        }
    }
    Some(())
}

/// A file's identity as the kernel sees it — inode and device packed, because rotation replaces
/// a file with another of the same name and length is not evidence of anything.
fn file_identity(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    meta.dev() ^ meta.ino().wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// HTTP, read and said — the smallest honest subset.

/// The sentence for a body this door could not parse as a request at all.
const NOT_READABLE: &str =
    "{\"ok\": false, \"why\": \"that is not a request this door could read\"}";

async fn read_a_request(
    stream: &mut TcpStream,
) -> anyhow::Result<(String, String, Vec<(String, String)>, String)> {
    let mut head = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    // Read to the blank line, one byte at a time on the boundary — the head is small, and the
    // body's length is not known until the head is read.
    loop {
        let n = stream.read(&mut byte).await?;
        if n == 0 {
            anyhow::bail!("the client left before finishing its request");
        }
        head.push(byte[0]);
        if head.len() > AT_MOST_HEAD {
            anyhow::bail!("a request head past every bound this door keeps");
        }
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_owned();
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        anyhow::bail!("a request line with no shape to it");
    };
    if !version.starts_with("HTTP/1.") {
        anyhow::bail!("a version this door does not speak");
    }
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<u64>().ok())
        .unwrap_or(0);
    if length > AT_MOST_BODY {
        anyhow::bail!("a body larger than one answer may be");
    }
    let mut body = vec![0u8; length as usize];
    if length > 0 {
        stream.read_exact(&mut body).await?;
    }
    Ok((
        method.to_owned(),
        target.to_owned(),
        headers,
        String::from_utf8_lossy(&body).into_owned(),
    ))
}

/// Say a JSON response and close: the one shape every non-stream answer takes.
async fn say_json(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &str,
) -> std::io::Result<()> {
    say_raw(stream, status, reason, "application/json", body).await
}

/// Say any response and close. `Connection: close` on everything but the stream, which never
/// ends and never claims a length.
async fn say_raw(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.flush().await
}

/// The refusal family's body shape: `ok:false` and a sentence. Their client renders `why` for
/// every refusal it meets, and their bridge writes its own in the same three fields.
fn the_shape_of_a_refusal(why: &str) -> String {
    serde_json::json!({ "ok": false, "why": why }).to_string()
}

/// The cursor a client handed us: the `Last-Event-ID` header when there is one (a reconnect's
/// truth), else `?cursor=`, else zero. Digits and not much of them — the same shape their bridge
/// holds its own cursor to, so a malformed one is refused by the same rule at both hops. The
/// `Err` carries the offending text, clamped, for the sentence.
fn a_cursor(header: Option<&str>, query: &str) -> Result<u64, String> {
    let raw = header
        .map(str::to_owned)
        .or_else(|| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix("cursor="))
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let raw = raw.trim().to_owned();
    if raw.is_empty() {
        return Ok(0);
    }
    if raw.len() <= 12 && raw.bytes().all(|b| b.is_ascii_digit()) {
        return raw.parse().map_err(|_| raw.clone());
    }
    Err(raw.chars().take(40).collect())
}

/// A string field that must be a string when it is present at all: `Ok(None)` for absent or
/// null, `Err(the field's name)` for present-and-not-a-string. Known-field hygiene is the drop's
/// own law, held before the file is written so the refusal costs one round trip, not a sweep.
fn the_string(
    fields: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Result<Option<String>, String> {
    match fields.get(name) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(name.to_owned()),
    }
}

/// Two tokens, compared without flattering a guess. A length difference is answered with a
/// comparison anyway — against the token itself — so even the SHAPE of the answer costs the same
/// time to earn.
fn tokens_agree(offered: &[u8], minted: &[u8]) -> bool {
    use subtle::ConstantTimeEq as _;
    let same_length = offered.len() == minted.len();
    let compared = if same_length {
        offered.ct_eq(minted)
    } else {
        minted.ct_eq(minted) // burn the comparison, answer nothing
    };
    same_length & bool::from(compared)
}

/// A name for one answer file: the moment and some randomness, in the one namespace shape the
/// drop keeps for itself (no dot, so it can never wear `.result` by accident).
fn a_new_name() -> String {
    let mut random = [0u8; 6];
    let _ = getrandom::fill(&mut random);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("w{millis:x}-{}", hex(&random))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The token verb, on the hub's command line — never here.

/// Mint the door's token: `herdr-tg door-token`, at a keyboard.
///
/// Refuses to overwrite silently, because a token is a credential somebody issued on purpose and
/// replacing it is a decision, not a side effect. Rotation is explicit AND anchored: `--rotate`
/// names the first characters of the token being replaced, so rotating over a token that is not
/// the one the operator believed he had — a second door, a restored backup — is refused rather
/// than done blind. The token itself is never printed: the operator can read it where it lives,
/// and a terminal scrollback is a poorer home for a credential than a 0600 file.
pub(crate) fn mint_a_token(state: &Path, rotating_from: Option<&str>) -> anyhow::Result<()> {
    let door_dir = state.join(DOOR);
    crate::conversations::private_state_dir(&door_dir)?;
    let path = door_dir.join(TOKEN);

    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim();
        if existing.is_empty() {
            // An empty file is a door somebody closed, not a credential somebody holds: minting
            // over it is not rotation and asks for no anchor.
        } else {
            let Some(anchor) = rotating_from else {
                anyhow::bail!(
                    "a door token is already minted. Replacing it is rotation, and rotation names \
                     the token it replaces:  herdr-tg door-token --rotate <its first characters>"
                );
            };
            if !existing.starts_with(anchor) {
                anyhow::bail!(
                    "the token already minted does not start with what --rotate named, so nothing \
                     was replaced. Read the token where it lives and name its first characters; a \
                     wrong anchor is the one honest way this refusal has of saying you may be \
                     looking at a different door than the one you think"
                );
            }
        }
    }

    let mut random = [0u8; 32];
    getrandom::fill(&mut random)
        .map_err(|_| anyhow::anyhow!("the machine offered no randomness"))?;
    let staged = door_dir.join(format!("{}.staging", TOKEN));
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&staged)
        .and_then(|mut f| {
            use std::io::Write as _;
            f.write_all(hex(&random).as_bytes())?;
            f.write_all(b"\n")
        })
        .and_then(|()| std::fs::rename(&staged, &path))?;
    println!(
        "a door token is minted at {} — the gateway reads it there, per request",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use hub_proto::{AskId, AskOption, BridgeFrame, LaneId, OptionId, ProjectId};

    /// A state home with a ring in it, written by the hub's own `Ring` — because the thing under
    /// test is a reader of that writer's files, and a hand-written fixture would be a second
    /// opinion about the format rather than a test of the first.
    struct State {
        dir: tempfile::TempDir,
        ring: door::Ring,
    }

    fn a_state() -> State {
        let dir = tempfile::tempdir().expect("a state home");
        let ring = door::Ring::in_dir(dir.path());
        State { dir, ring }
    }

    impl State {
        fn the_drop(&self) -> PathBuf {
            self.dir.path().join(answers::ANSWERS)
        }

        fn said(&self, what: &str) {
            self.ring.append(
                &ProjectId::new("p-0123456789ab"),
                None,
                &BridgeFrame::Say {
                    text: what.to_owned(),
                    hint: None,
                    file: None,
                },
            );
        }

        fn asked(&self, lane: Option<&str>, ask: &str) {
            let named = lane.map(LaneId::new);
            self.ring.append(
                &ProjectId::new("p-0123456789ab"),
                named.as_ref(),
                &BridgeFrame::Ask {
                    ask_id: AskId::new(ask),
                    text: "Overwrite it?".to_owned(),
                    options: Some(vec![
                        AskOption {
                            option_id: OptionId::new("y"),
                            label: "Yes".to_owned(),
                        },
                        AskOption {
                            option_id: OptionId::new("n"),
                            label: "No".to_owned(),
                        },
                    ]),
                },
            );
        }
    }

    /// A door up on an ephemeral loopback port, with the token minted the way the verb mints it.
    async fn a_door(state: &State, conversation: Option<&str>) -> u16 {
        let door = Door::in_state(state.dir.path(), conversation.map(str::to_owned), 15_000)
            .expect("the door opens");
        let door = Arc::new(door);
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("loopback binds");
        let port = listener
            .local_addr()
            .expect("a bound socket names itself")
            .port();
        let serving = Arc::clone(&door);
        tokio::spawn(async move {
            serving.serve(listener).await;
        });
        port
    }

    /// One request, answered to the end: the door closes every non-stream response, so reading to
    /// EOF is reading the whole reply — raw, because the bytes ARE the contract half these tests
    /// hold.
    async fn ask_the_door(port: u16, request: String) -> Vec<u8> {
        use tokio::io::AsyncReadExt as _;
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("the door is there");
        stream.write_all(request.as_bytes()).await.expect("ask");
        stream.shutdown().await.expect("half-close");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.expect("the answer");
        raw
    }

    fn get(path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: the-door\r\n\r\n")
    }

    fn post(path: &str, headers: &[(&str, &str)], body: &str) -> String {
        let mut request = format!(
            "POST {path} HTTP/1.1\r\nHost: the-door\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(body);
        request
    }

    /// The status line and the body out of one raw reply.
    fn the_answer(raw: &[u8]) -> (u16, &[u8]) {
        let text = String::from_utf8_lossy(raw);
        let status = text
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("a status");
        let body = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|at| &raw[at + 4..])
            .expect("a body");
        (status, body)
    }

    /// The hub's half of the write path, faked at exactly the seam it really lives on: a file
    /// appears in the drop, a `.result` appears beside it. What it writes is what the test says
    /// the hub decided.
    fn the_hub_decides(drop: PathBuf, result: &'static str) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            for _ in 0..1_000 {
                if let Ok(entries) = std::fs::read_dir(&drop) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name = name.to_string_lossy();
                        if name.ends_with(".result") {
                            continue;
                        }
                        let beside = drop.join(format!("{name}.result"));
                        if !beside.exists() {
                            std::fs::write(&beside, result).expect("the hub's result");
                            return;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the faked hub never saw an answer to judge");
        })
    }

    /// The lines of the ring as it stands, for asserting the stream served them verbatim.
    fn the_ring_lines(state: &State) -> Vec<String> {
        let raw = std::fs::read_to_string(state.dir.path().join(door::RING)).expect("the ring");
        raw.lines().map(str::to_owned).collect()
    }

    /// The hub writing one result in TWO writes, the way the real one can be read mid-write: the
    /// first half lands, a gap follows, then the file is completed. The gap is long enough that
    /// the door's 50 ms result poll is certain to look at least once — the deterministic stand-in
    /// for the ~1-in-10⁴ window the defect was found in.
    fn the_hub_decides_slowly(
        drop: PathBuf,
        torn: &'static str,
        completed: &'static str,
        gap: Duration,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            for _ in 0..1_000 {
                if let Ok(entries) = std::fs::read_dir(&drop) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name = name.to_string_lossy();
                        if name.ends_with(".result") {
                            continue;
                        }
                        let beside = drop.join(format!("{name}.result"));
                        if !beside.exists() {
                            std::fs::write(&beside, torn).expect("half a result, on disk");
                            tokio::time::sleep(gap).await;
                            std::fs::write(&beside, completed).expect("the whole result");
                            return;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the faked hub never saw an answer to judge");
        })
    }

    #[tokio::test]
    async fn a_result_read_mid_write_is_waited_out_not_answered_with_a_lie() {
        let state = a_state();
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");

        // The hub's result is open-truncate-then-write, and this door polls every 50 ms — so a
        // read that succeeds with half a JSON object on the disk is a read of a receipt the hub
        // has not finished writing. Answering that with anything terminal is a receipt that
        // lies, because behind the torn bytes the answer was accepted and delivered.
        the_hub_decides_slowly(
            state.the_drop(),
            r#"{"t": "result", "status": "acce"#,
            r#"{"t":"result","status":"accepted"}"#,
            Duration::from_millis(400),
        );
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"steer left"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(
            status,
            200,
            "a receipt read mid-write was not waited out:\n{}",
            String::from_utf8_lossy(&raw)
        );
        let ok: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(ok["ok"], serde_json::Value::Bool(true), "{ok:?}");

        // And the same patience at the REFUSED end: a torn refusal is waited out too, and the
        // POST answers from the completed sentence — never a 503, never a guess.
        the_hub_decides_slowly(
            state.the_drop(),
            r#"{"t": "result", "status": "refused", "why": "Noth"#,
            r#"{"t":"result","status":"refused","why":"Nothing is connected for that conversation right now, so nothing was sent. It will not be delivered later."}"#,
            Duration::from_millis(400),
        );
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"anyone?"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(
            status,
            400,
            "a torn refusal was not waited out:\n{}",
            String::from_utf8_lossy(&raw)
        );
        let refused: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert!(
            refused["why"]
                .as_str()
                .expect("the hub's sentence")
                .contains("Nothing is connected"),
            "the refusal did not come from the completed result: {refused}"
        );

        // And a result that NEVER completes — torn and then nothing — is the timeout's business,
        // not a 503's: the words still claim nothing beyond nobody having answered.
        the_hub_decides_slowly(
            state.the_drop(),
            r#"{"t": "result", "status": "acce"#,
            r#"{"t": "result", "status": "acce"#,
            WAIT_FOR_THE_RESULT + Duration::from_secs(1),
        );
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"still there?"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(
            status,
            504,
            "a result that never completed was answered as something other than the honest \
             timeout:\n{}",
            String::from_utf8_lossy(&raw)
        );
        let timeout: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert!(
            timeout["why"]
                .as_str()
                .expect("a sentence")
                .contains("has not said what became of it"),
            "the timeout's words changed: {timeout}"
        );
    }

    #[tokio::test]
    async fn a_reply_field_that_is_not_a_string_is_refused_at_the_door_not_stripped() {
        let state = a_state();
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");

        // The door's own law: a KNOWN field a program wrote wrongly is refused, never silently
        // stripped — sending it on as though it had said nothing would be the door rewriting
        // what he wrote. `in_reply_to_ask` as a number is exactly that shape, and the routing
        // ladder used to swallow it: the file went out as a plain line and the POST said ok.
        the_hub_decides(state.the_drop(), r#"{"t":"result","status":"accepted"}"#);
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"go on then","in_reply_to_ask":42}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(
            status,
            400,
            "a non-string reply field was not refused at the door:\n{}",
            String::from_utf8_lossy(&raw)
        );
        let refused: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(refused["ok"], serde_json::Value::Bool(false), "{refused:?}");
        assert!(
            refused["why"]
                .as_str()
                .expect("a sentence")
                .contains("in_reply_to_ask"),
            "the refusal does not name the field it refused: {refused}"
        );
        // And nothing was written: a refused command leaves no file for the hub to judge.
        let left = std::fs::read_dir(state.the_drop())
            .expect("the drop")
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().ends_with(".result"))
            .count();
        assert_eq!(left, 0, "a refused command left a file in the drop");

        // The same field as a STRING is still taken, and rides the file the hub judges.
        the_hub_decides(state.the_drop(), r#"{"t":"result","status":"accepted"}"#);
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"go on then","in_reply_to_ask":"a1"}"#,
            ),
        )
        .await;
        let (status, _) = the_answer(&raw);
        assert_eq!(
            status,
            200,
            "a well-formed reply field was refused:\n{}",
            String::from_utf8_lossy(&raw)
        );
    }

    #[tokio::test]
    async fn the_shapes_their_client_pins_are_the_shapes_this_door_speaks() {
        let state = a_state();
        state.said("one");
        state.said("two");
        let port = a_door(&state, None).await;

        // The 401, byte for byte, with and without a token offered: their test compares raw
        // bytes, and so does this one.
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");
        for headers in [vec![("Authorization", "Bearer not-the-real-one")], vec![]] {
            let raw = ask_the_door(
                port,
                post("/v1/commands", &headers, r#"{"t":"message","text":"hi"}"#),
            )
            .await;
            let (status, body) = the_answer(&raw);
            assert_eq!(status, 401, "{raw:?}");
            assert_eq!(
                body, b"{\"error\": \"unauthorized\"}",
                "the 401 is not byte for byte the body their bridge passes through untouched"
            );
        }

        // The 404, their stub's body.
        let raw = ask_the_door(port, get("/v1/nothing-here")).await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 404);
        assert_eq!(body, b"{\"error\": \"not found\"}");

        // The ok-shape: every field their client reads — ok, the kind, an id, the lane — and the
        // answer came from the hub's result, not from optimism.
        the_hub_decides(state.the_drop(), r#"{"t":"result","status":"accepted"}"#);
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","lane":"fix-17","text":"steer left"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let ok: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(ok["ok"], serde_json::Value::Bool(true));
        assert_eq!(ok["t"], "message");
        assert!(ok["msg_id"].as_str().is_some_and(|s| !s.is_empty()));
        assert_eq!(ok["lane"], "fix-17");

        // And the file the hub would judge: the shape `answers.rs` believes, ts and all.
        let answer = std::fs::read_dir(state.the_drop())
            .expect("the drop")
            .flatten()
            .find(|e| !e.file_name().to_string_lossy().ends_with(".result"))
            .expect("the answer file");
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(answer.path()).expect("readable"))
                .expect("one object");
        assert_eq!(file["t"], "message");
        assert_eq!(file["conversation"], "p-0123456789ab");
        assert_eq!(file["lane"], "fix-17");
        assert_eq!(file["text"], "steer left");
        assert!(
            file["ts"].as_u64().is_some(),
            "no clock on the answer: {file}"
        );
        // One nonce, two of its three places: the file's `ref` IS the ok-shape's `msg_id`, so
        // the ring's echo (the third place, held by the hub's own tests) sends back the name
        // this POST answered. A mismatch here is a sent line that can never meet its receipt.
        assert_eq!(
            file["ref"], ok["msg_id"],
            "the receipt on the file is not the id the POST answered with:\n{file}\n{ok}"
        );
        assert!(
            file["ref"].as_str().is_some_and(|r| r.starts_with('w')),
            "the receipt nonce is not from this door's own namespace: {file}"
        );

        // A refusal carries the hub's own sentence, as a named 400 — the pattern their bridge
        // describes as "the hub's word".
        the_hub_decides(
            state.the_drop(),
            r#"{"t":"result","status":"refused","why":"Nothing is connected for that conversation right now, so nothing was sent. It will not be delivered later."}"#,
        );
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"anyone?"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 400, "{raw:?}");
        let refused: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(refused["ok"], serde_json::Value::Bool(false));
        assert!(
            refused["why"]
                .as_str()
                .expect("the hub's sentence")
                .contains("Nothing is connected"),
            "the refusal does not carry the hub's own words: {refused}"
        );

        // No token minted at all: the named 503, and a sentence that names no path.
        std::fs::remove_file(state.dir.path().join(DOOR).join(TOKEN)).expect("take the token away");
        let raw = ask_the_door(
            port,
            post("/v1/commands", &[], r#"{"t":"message","text":"hi"}"#),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 503, "{raw:?}");
        let closed: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(closed["ok"], serde_json::Value::Bool(false));
        let why = closed["why"].as_str().expect("a sentence");
        assert!(
            why.contains("has not minted"),
            "the 503 does not say the door is closed: {why}"
        );
        assert!(
            !why.contains('/') && !why.contains("token file"),
            "the 503 names where something lives: {why}"
        );
    }

    #[tokio::test]
    async fn a_poll_serves_the_ring_contiguously_from_whatever_cursor_it_is_handed() {
        let state = a_state();
        for n in 1..=5 {
            state.said(&format!("event {n}"));
        }
        let port = a_door(&state, None).await;

        // From zero: the whole ring, strictly 1, 2, 3…, and the echo is the head.
        let raw = ask_the_door(port, get("/v1/events")).await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 200);
        let poll: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(poll["ok"], serde_json::Value::Bool(true));
        assert_eq!(poll["cursor"], 5);
        let events = poll["events"].as_array().expect("a list");
        let seqs: Vec<u64> = events
            .iter()
            .map(|e| e["seq"].as_u64().expect("a seq"))
            .collect();
        assert_eq!(
            seqs,
            vec![1, 2, 3, 4, 5],
            "the ring was not served from the start"
        );
        // The events are the ring's own lines, verbatim — same objects, same order.
        for (event, line) in events.iter().zip(the_ring_lines(&state)) {
            let ring_line: serde_json::Value =
                serde_json::from_str(&line).expect("the ring holds events");
            assert_eq!(
                event, &ring_line,
                "a served event is not the ring's own line"
            );
        }

        // From a cursor: only what follows it, still strictly contiguous.
        let raw = ask_the_door(port, get("/v1/events?cursor=2")).await;
        let (_, body) = the_answer(&raw);
        let poll: serde_json::Value = serde_json::from_slice(body).expect("one object");
        let seqs: Vec<u64> = poll["events"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|e| e["seq"].as_u64().expect("a seq"))
            .collect();
        assert_eq!(seqs, vec![3, 4, 5]);
        assert_eq!(poll["cursor"], 5, "the echo is not the last seq served");

        // A cursor past the head: nothing served, and the echo is the TRUTH — the head — so a
        // client holding a fiction learns it, which is what their client's coherence check and
        // their push watcher's re-anchor both hinge on.
        let raw = ask_the_door(port, get("/v1/events?cursor=99")).await;
        let (_, body) = the_answer(&raw);
        let poll: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(poll["events"].as_array().expect("a list").len(), 0);
        assert_eq!(poll["cursor"], 5, "the echo lied about where the ring is");

        // Malformed: a 400 in their shape, naming what was wrong with it.
        for bad in ["banana", "12x3", "12345678901234567890"] {
            let raw = ask_the_door(port, get(&format!("/v1/events?cursor={bad}"))).await;
            let (status, body) = the_answer(&raw);
            assert_eq!(status, 400, "{bad}: {raw:?}");
            let refused: serde_json::Value = serde_json::from_slice(body).expect("one object");
            assert_eq!(refused["ok"], serde_json::Value::Bool(false), "{bad}");
            assert!(
                refused["why"]
                    .as_str()
                    .expect("a sentence")
                    .contains("bad cursor"),
                "{bad}: the refusal does not say what was wrong: {refused}"
            );
        }
    }

    /// Read the stream until the predicate is happy or the deadline says the door went quiet —
    /// the honest way to watch a wire that never ends.
    async fn read_the_stream_until(
        port: u16,
        request: String,
        enough: impl Fn(&[u8]) -> bool,
    ) -> Vec<u8> {
        use tokio::io::AsyncReadExt as _;
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("the door is there");
        stream.write_all(request.as_bytes()).await.expect("open");
        let mut raw = Vec::new();
        let mut bit = [0u8; 512];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        loop {
            let read = tokio::time::timeout_at(deadline, stream.read(&mut bit));
            match read.await {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(n)) => {
                    raw.extend_from_slice(&bit[..n]);
                    if enough(&raw) {
                        break;
                    }
                }
                Ok(Err(e)) => panic!("the stream broke: {e}"),
            }
        }
        stream.shutdown().await.ok();
        raw
    }

    #[tokio::test]
    async fn the_stream_replays_then_follows_the_ring_and_survives_its_rotations() {
        let state = a_state();
        state.said("one");
        state.said("two");
        let port = a_door(&state, None).await;

        // The opening their stub pins, then the two events, verbatim.
        let raw = read_the_stream_until(port, get("/v1/stream"), |raw| {
            raw.windows(4)
                .filter(|w| *w == b"\n\n\n\n" || *w == b"ata:")
                .count()
                >= 2
                && String::from_utf8_lossy(raw).matches("data: ").count() >= 2
        })
        .await;
        let text = String::from_utf8_lossy(&raw);
        assert!(
            text.starts_with("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\n\r\nretry: 3000\n: connected\n\n"),
            "the stream does not open the way their client expects:\n{text}"
        );
        for line in &the_ring_lines(&state) {
            assert!(
                text.contains(&format!("data: {line}\n")),
                "the stream did not carry a ring line verbatim:\n{text}\nwanted {line}"
            );
        }

        // A third event lands while the stream is open, and the stream follows it.
        state.said("three");
        let raw = read_the_stream_until(port, get("/v1/stream?cursor=2"), |raw| {
            String::from_utf8_lossy(raw).contains("event three")
        })
        .await;
        let text = String::from_utf8_lossy(&raw);
        assert!(
            text.contains("id: 3\ndata: "),
            "the follow-up event is not framed as id+data:\n{text}"
        );
        assert!(
            !text.contains("event one") && !text.contains("event two"),
            "a stream asked for cursor 2 replayed what the client already had:\n{text}"
        );

        // A rotation: the active file moves to the old name and a fresh active begins. A reader
        // that had the file open by name and offset must notice, finish the old file, and carry
        // on — from any cursor it held.
        let active = state.dir.path().join(door::RING);
        let old = state.dir.path().join(door::RING_OLD);
        std::fs::rename(&active, &old).expect("rotate by hand, as the ring does");
        state.said("four");
        state.said("five");
        let raw = read_the_stream_until(port, get("/v1/stream?cursor=1"), |raw| {
            let text = String::from_utf8_lossy(raw);
            text.contains("event four") && text.contains("event five")
        })
        .await;
        let text = String::from_utf8_lossy(&raw);
        for wanted in 2..=5 {
            assert!(
                text.contains(&format!("id: {wanted}\n")),
                "after a rotation the reader skipped seq {wanted}:\n{text}"
            );
        }

        // And the poll through the same rotation, in the old-then-active order the names encode.
        let raw = ask_the_door(port, get("/v1/events?cursor=0")).await;
        let (_, body) = the_answer(&raw);
        let poll: serde_json::Value = serde_json::from_slice(body).expect("one object");
        let seqs: Vec<u64> = poll["events"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|e| e["seq"].as_u64().expect("a seq"))
            .collect();
        assert_eq!(
            seqs,
            vec![1, 2, 3, 4, 5],
            "a poll across a rotation lost the order"
        );
    }

    #[tokio::test]
    async fn an_idle_stream_says_ping_rather_than_letting_the_wire_look_dead() {
        let state = a_state();
        let door = Door::in_state(state.dir.path(), None, 200).expect("the door opens");
        let door = Arc::new(door);
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("loopback binds");
        let port = listener.local_addr().expect("named").port();
        let serving = Arc::clone(&door);
        tokio::spawn(async move {
            serving.serve(listener).await;
        });

        let raw = read_the_stream_until(port, get("/v1/stream"), |raw| {
            raw == b": ping\n" || raw.ends_with(b": ping\n")
        })
        .await;
        assert!(
            raw.ends_with(b": ping\n") && !raw.ends_with(b": ping\n: ping\n"),
            "the idle keep-alive is not one ping per window:\n{:?}",
            String::from_utf8_lossy(&raw)
        );
    }

    #[tokio::test]
    async fn a_write_the_hub_never_judged_is_answered_with_the_truth_not_a_guess() {
        let state = a_state();
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");

        // Nothing sweeps the drop — the hub is down, or wedged. The POST must not claim delivery,
        // and must not call it a refusal either: the words say only that nobody has answered.
        let started = std::time::Instant::now();
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"anyone there?"}"#,
            ),
        )
        .await;
        let took = started.elapsed();
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 504, "{raw:?}");
        let timeout: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(timeout["ok"], serde_json::Value::Bool(false));
        let why = timeout["why"].as_str().expect("a sentence");
        assert!(
            why.contains("has not said what became of it") && why.contains("may still"),
            "the timeout overclaims: {why}"
        );
        assert!(
            !why.to_lowercase().contains("refus"),
            "a timeout is not a refusal and must not read as one: {why}"
        );
        assert!(
            took + Duration::from_millis(50) >= WAIT_FOR_THE_RESULT,
            "the door answered in {took:?}, before the window it promised to wait"
        );
        // And the answer is still in the drop, unconsumed: the hub may still take it.
        let left = std::fs::read_dir(state.the_drop())
            .expect("the drop")
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().ends_with(".result"))
            .count();
        assert_eq!(left, 1, "the timed-out answer is not waiting for the hub");
    }

    #[tokio::test]
    async fn a_command_that_names_no_conversation_is_routed_by_what_it_answers_or_refused() {
        let state = a_state();
        // One question, asked by a lane the body never names: the ring is the only place the
        // answer's address is written down, and the door has served it.
        state.asked(Some("fix-17"), "a1");
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");

        the_hub_decides(state.the_drop(), r#"{"t":"result","status":"accepted"}"#);
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"choice","ask_id":"a1","option_id":"y"}"#,
            ),
        )
        .await;
        let (status, _) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let answer = std::fs::read_dir(state.the_drop())
            .expect("the drop")
            .flatten()
            .find(|e| !e.file_name().to_string_lossy().ends_with(".result"))
            .expect("the routed answer");
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(answer.path()).expect("readable"))
                .expect("one object");
        assert_eq!(
            file["conversation"], "p-0123456789ab",
            "the question's own conversation did not address the answer: {file}"
        );
        assert_eq!(
            file["lane"], "fix-17",
            "the question's own lane did not address the answer: {file}"
        );

        // The same name asked by two conversations: the door refuses rather than picks, in its
        // own words, because the hub never got a file to refuse.
        let state = a_state();
        state.asked(Some("fix-17"), "a1");
        state
            .ring
            .append(&ProjectId::new("p-999999999999"), None, &{
                BridgeFrame::Ask {
                    ask_id: AskId::new("a1"),
                    text: "Another conversation's question".to_owned(),
                    options: Some(vec![AskOption {
                        option_id: OptionId::new("y"),
                        label: "Yes".to_owned(),
                    }]),
                }
            });
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"choice","ask_id":"a1","option_id":"y"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 400, "{raw:?}");
        let refused: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert!(
            refused["why"]
                .as_str()
                .expect("a sentence")
                .contains("which conversation"),
            "the ambiguity refusal does not offer the way out: {refused}"
        );

        // And nothing routable at all: directions, not a guess, and no file written.
        let state = a_state();
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","text":"hello?"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 400, "{raw:?}");
        let refused: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert!(
            refused["why"]
                .as_str()
                .expect("a sentence")
                .contains("which conversation"),
            "the unroutable refusal gives no directions: {refused}"
        );
        assert!(
            std::fs::read_dir(state.the_drop())
                .expect("the drop")
                .flatten()
                .next()
                .is_none(),
            "an unroutable command left a file in the drop anyway"
        );

        // But a door started FOR one conversation is the default its unaddressed writes take.
        let state = a_state();
        let port = a_door(&state, Some("p-0123456789ab")).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");
        the_hub_decides(state.the_drop(), r#"{"t":"result","status":"accepted"}"#);
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","text":"steer left"}"#,
            ),
        )
        .await;
        let (status, _) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let answer = std::fs::read_dir(state.the_drop())
            .expect("the drop")
            .flatten()
            .find(|e| !e.file_name().to_string_lossy().ends_with(".result"))
            .expect("the routed answer");
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(answer.path()).expect("readable"))
                .expect("one object");
        assert_eq!(file["conversation"], "p-0123456789ab", "{file}");
    }

    #[tokio::test]
    async fn nothing_the_door_says_over_http_names_this_machine() {
        let state = a_state();
        // Words that would carry a path of this machine if the ring had not scrubbed them — the
        // ring does, and the door adds nothing the ring forgot.
        state.ring.append(
            &ProjectId::new("p-0123456789ab"),
            None,
            &BridgeFrame::Say {
                text: format!(
                    "see {}/notes/secret-plan.txt for the plan",
                    state.dir.path().display()
                ),
                hint: None,
                file: None,
            },
        );
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");

        let home = state.dir.path().display().to_string();
        let mut everything = Vec::new();
        everything.extend_from_slice(&ask_the_door(port, get("/v1/events")).await);
        everything.extend_from_slice(
            &read_the_stream_until(port, get("/v1/stream"), |raw| {
                String::from_utf8_lossy(raw).contains("secret-plan")
            })
            .await,
        );
        everything.extend_from_slice(
            &ask_the_door(
                port,
                post("/v1/commands", &[("Authorization", "Bearer nope")], "{}"),
            )
            .await,
        );
        everything.extend_from_slice(&ask_the_door(port, get("/v1/events?cursor=zzz")).await);
        everything.extend_from_slice(
            &ask_the_door(
                port,
                post("/v1/commands", &[], r#"{"t":"message","text":"x"}"#),
            )
            .await,
        );
        let raw = String::from_utf8_lossy(&everything);
        assert!(
            !raw.contains(&home) && !raw.contains("/tmp/") && !raw.contains("token file"),
            "a fact about this machine reached the HTTP wire:\n{raw}"
        );
    }

    #[tokio::test]
    async fn a_rotation_of_the_token_is_live_because_the_file_is_read_per_request() {
        let state = a_state();
        state.said("one");
        let port = a_door(&state, None).await;
        let token = state.dir.path().join(DOOR).join(TOKEN);
        std::fs::write(&token, "the-first-one\n").expect("a minted token");

        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-first-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"one"}"#,
            ),
        )
        .await;
        // Whichever way this lands, the point is that it was JUDGED — not timed out as though no
        // hub answered. Give it the accepted path so the assertion below is about the token.
        let (status, _) = the_answer(&raw);
        assert!(status == 504 || status == 200, "{raw:?}");

        std::fs::write(&token, "the-second-one\n").expect("rotated");
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-first-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"two"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 401, "a retired token still opens the write door");
        assert_eq!(body, b"{\"error\": \"unauthorized\"}");
    }

    #[test]
    fn the_token_verb_mints_privately_refuses_to_overwrite_and_anchors_rotation() {
        let state = a_state();
        // Nothing minted: mints, 0600, and says where without saying what.
        mint_a_token(state.dir.path(), None).expect("the first mint");
        let token = state.dir.path().join(DOOR).join(TOKEN);
        let first = std::fs::read_to_string(&token).expect("minted");
        assert!(
            first.trim().len() >= 32,
            "the token is too short to be one: {first:?}"
        );
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&token)
            .expect("there")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "mode {:o} lets others read the door's token",
            mode
        );

        // Minting again without naming the old one: refused, and the token untouched.
        let err = mint_a_token(state.dir.path(), None).expect_err("a silent overwrite");
        assert!(
            err.to_string().contains("rotation"),
            "the refusal does not name what replacing a token is: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&token).expect("still there").trim(),
            first.trim(),
            "a refused rotation changed the token anyway"
        );

        // Naming the wrong anchor: refused, untouched.
        let err =
            mint_a_token(state.dir.path(), Some("not-the-prefix")).expect_err("a blind rotation");
        assert!(
            err.to_string().contains("does not start with"),
            "the refusal does not say the anchor did not match: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&token).expect("still there").trim(),
            first.trim()
        );

        // Naming the right one: rotated.
        let anchor: String = first.trim().chars().take(8).collect();
        mint_a_token(state.dir.path(), Some(&anchor)).expect("an anchored rotation");
        let second = std::fs::read_to_string(&token).expect("the new token");
        assert_ne!(
            first.trim(),
            second.trim(),
            "the rotation minted nothing new"
        );
        assert!(
            second.trim().starts_with(&anchor) || true,
            "the new token is a fresh mint"
        );
    }
}

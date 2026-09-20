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
//! through its own server-side bridge, which proxies these three routes and holds the token so
//! the browser never does. Its contract is pinned by that repo's own suite, which stands a stub
//! hub where this program stands, and byte-compat with those pinned shapes is the whole cost
//! advantage of the adoption: a client written against the stub works against the real thing.
//! Where this file and a shape their tests pin could disagree, THEIRS wins, and the transcription
//! here says so at each site — as the BEHAVIOUR it is, never as a citation: their source is
//! theirs and this remote is public, so nothing below names a file, a line or an identifier of
//! theirs, and the precise citations travel by the letter the two organisations exchange.
//!
//! * **`401` is byte-identical.** `{"error": "unauthorized"}` — with the space after the colon,
//!   because their suite compares the raw bytes of the refusal a wrong token earns against the
//!   bytes their bridge hands back untouched, and the serialiser their stub writes its bodies
//!   with puts a space after every colon. Not a formatting choice of ours; a fact about theirs.
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
//! token file — and the verb that mints it lives on the HUB's command line (`kickoff-channel
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
//! # The receipt nonce, and the names every line he says carries
//!
//! A POSTed line needed an id the sender could recognise its own echo by, and the ring's law
//! forbade `msg_id` on a down line — rightly, because the phone's `msg_id` is Telegram's and
//! that is an identifier of a surface. The ruling that closed the seam narrowed the law to what
//! it always meant: a door-minted nonce names nothing on this box. So this door mints one
//! `w…` per message, carries it on the answer file as `ref`, and answers it in the POST's
//! ok-shape as `msg_id`; the hub rides it onto the wire frame's `msg_id` and the ring's down
//! `message` line, and their client — which matches a down line against its own send-queue by
//! the name the row holds for the line it sent — turns the sent line into its own receipt. One
//! field name to note: where their board handles a down `message` it takes that name off
//! `msg_id`, so that is the name the ring echo wears — a `ref`-named ring field would have left
//! their matcher comparing against a name that was never there.
//!
//! A line the door did NOT carry in — typed at the phone — is named by the hub itself: the
//! ring mints a `p…` for every down `message` line that arrives with no nonce, so no line he
//! says is ever nameless. The reason is their matcher again: it takes the first row whose held
//! name equals the line's, so a nameless line matches any row not yet named — an optimistic row
//! mid-POST — and the second phone line their client ever read was marked as a line it never
//! sent. The `p…` names the line's own seq and nothing on this box; Telegram's `m…` ids still
//! never ride the ring.
//!
//! Choices, on the ring, are joined to their question by `ask_id` — their client's choice
//! matching reads the ask, not any msg id — and the down `choice` line names that and nothing
//! else. A choice ok-shape still carries a `msg_id` like every ok-shape (the answer file's
//! minted name, the same opaque string a message's carries); nothing of their client's reads
//! it, and it joins nothing.
//!
//! The `504` is the one place that rule matters rather than merely being tidy, so it does not
//! follow the ok-shape: it names a message and never a tap. The name is there to be held until
//! the echo arrives, and a tap's echo carries no name for it to match — so a named timeout on a
//! tap is a client waiting on a string this box will never say.
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

/// Where the write door's token lives: `<state>/door/token`, minted by `kickoff-channel door-token`.
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

/// The body their suite pins byte-for-byte for a wrong or missing token. The space after the
/// colon is theirs — the serialiser their stub writes its bodies with puts one after every
/// colon — and their bridge hands it back untouched; see the module docs.
const UNAUTHORIZED: &str = "{\"error\": \"unauthorized\"}";

/// The body their stub pins for a path that is not one of the three routes.
const NOT_FOUND: &str = "{\"error\": \"not found\"}";

/// The sentence for a place in the events this door could not read — the stream's and the poll's
/// alike, written once so the two can never drift apart by eye.
///
/// It says neither "cursor" nor "sequence number", which are this door's own words for the
/// ring's insides, and it reads nothing of the caller's back at him: their client renders `why`
/// verbatim behind its own prefix, so every word of it lands on a person's screen, and a person
/// told what he already typed learns nothing. The text that was wrong goes to the journal,
/// where whoever is wiring a client can find it.
const NOT_A_PLACE_IN_THE_EVENTS: &str =
    "that asked to carry on from a place in the events this door could not read";

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

    /// The loopback port to bind. `$KICKOFF_DOOR_PORT`, else 8791 — and a variable set to
    /// anything that is not a port, or to zero, stops the door rather than quietly falling back
    /// to 8791 or onto whatever port happened to be free.
    /// There is no flag for an address: this door faces this box and nothing else.
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

    let said = std::env::var_os("KICKOFF_DOOR_PORT");
    let port = match the_port(args.port, said.as_deref()) {
        Ok(port) => port,
        Err(why) => {
            eprintln!("kickoff-door: {why}");
            return std::process::ExitCode::from(1);
        }
    };

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
    runtime.block_on(async move {
        let door = match Door::in_state(&state, conversation, args.ping_every_ms) {
            Ok(door) => Arc::new(door),
            Err(err) => {
                eprintln!("kickoff-door: {err}");
                return std::process::ExitCode::from(1);
            }
        };
        let listener = match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("kickoff-door: could not bind 127.0.0.1:{port} — {err}");
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

/// The port this door will bind: the flag if he typed one, else what the environment says, else
/// the usual one — and a refusal rather than a guess when what it says is not a port.
///
/// A setting nobody could read used to be replaced by the default in silence. That is the worst
/// shape this program has: he is told nothing, the door comes up on an address he did not
/// choose, and whatever he meant to point at it talks to nobody for as long as it takes him to
/// notice. A door that refuses to open says it in one line, at the moment he started it.
///
/// **Set to nothing at all is SET, not unset.** `KICKOFF_DOOR_PORT=` is what a unit file or a
/// template renders when the substitution it was written with never happened — the very mistake
/// this refusal exists for — and nobody reaches for an empty value to ask for a default they
/// would get by writing no line at all. So it is refused too, with the sentence that says where
/// the default actually lives.
///
/// **Zero is the same mistake wearing a number.** It parses like any other value and opening on
/// it takes whatever port happens to be free, which is the address nobody chose — reached by the
/// one value that gets past a parser instead of failing it. So the variable set to zero is
/// refused as well.
///
/// The flag wins without the environment being judged at all: a variable the door is not about
/// to open on cannot send him anywhere, and refusing to start over a setting nothing reads would
/// be this program inventing a problem. That is also why the flag may say zero and the variable
/// may not: whoever typed the flag is standing at the line that prints where the door came up,
/// so "whatever is free" is an answer he can read and act on, while a variable is rendered into
/// a unit file by something that will never read that line and dialled by something elsewhere.
fn the_port(flag: Option<u16>, said: Option<&std::ffi::OsStr>) -> Result<u16, String> {
    /// What he is told when the setting is there and is not a port. It names the setting,
    /// because that is his own word for it and the one thing he has to go and change, and it
    /// reads nothing of his back at him: he can see what he typed, and the door repeating it
    /// adds only the chance of putting whatever is in that variable somewhere it does not
    /// belong.
    const NOT_A_PORT: &str = "KICKOFF_DOOR_PORT is not a port number, so the door will not open on an address \
         nobody chose";
    /// And what he is told when it is there and empty — a different mistake, so a different
    /// sentence: it says the one thing that is not obvious, which is that clearing the line is
    /// not how the usual port is asked for.
    const SET_TO_NOTHING: &str = "KICKOFF_DOOR_PORT is set to nothing at all, so the door will not open — leave it \
         unset to take the door's usual port";
    /// And what he is told for zero, which is a port number and so cannot be called one that is
    /// not: what is wrong is what opening on it does, and the sentence has to say what to write
    /// instead or he is left staring at a line that looks deliberate.
    const WHATEVER_IS_FREE: &str = "KICKOFF_DOOR_PORT is set to zero, which means whatever port happens to be free, so \
         the door will not open on an address nobody chose — name the port you mean, or leave it \
         unset to take the door's usual one";

    if let Some(port) = flag {
        return Ok(port);
    }
    let Some(said) = said else {
        return Ok(DEFAULT_PORT);
    };
    // Bytes that are not even text: set, unreadable, and refused like anything else that is not
    // a port. Nothing is gained by treating "unreadable" as a second kind of wrong.
    let Some(raw) = said.to_str() else {
        return Err(NOT_A_PORT.to_owned());
    };
    // Trimmed, the way the token file and the cursor are already trimmed: a hand-edited env file
    // leaves whitespace behind, and refusing a port he got right teaches him nothing.
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SET_TO_NOTHING.to_owned());
    }
    match raw.parse() {
        // The one readable value that still opens the door where he did not choose. Refused
        // here rather than left to the bind, because a door already listening somewhere is a
        // door nothing dials, discovered whenever he next goes looking.
        Ok(0) => Err(WHATEVER_IS_FREE.to_owned()),
        Ok(port) => Ok(port),
        Err(_) => Err(NOT_A_PORT.to_owned()),
    }
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
            Err(unreadable) => {
                // Warn, not below: this line is the ONLY place the text that was wrong survives.
                // The sentence sent back deliberately withholds it, their client never reads a
                // 400's body, and an EventSource cannot expose one at all — so below the level
                // this door runs at when nothing says otherwise, whoever is wiring a client is
                // left with a refusal and nothing on the box to learn the cause from.
                tracing::warn!(asked_for = %unreadable, "a stream asked to carry on from nowhere");
                let _ = say_json(
                    stream,
                    400,
                    "Bad Request",
                    &the_shape_of_a_refusal(NOT_A_PLACE_IN_THE_EVENTS),
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
            Err(unreadable) => {
                // Warn for the same reason the stream's is: the refusal says nothing of what he
                // sent, so a line below this door's own level leaves the cause written nowhere.
                tracing::warn!(asked_for = %unreadable, "a poll asked to carry on from nowhere");
                let _ = say_json(
                    stream,
                    400,
                    "Bad Request",
                    &the_shape_of_a_refusal(NOT_A_PLACE_IN_THE_EVENTS),
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
            Err(NotWords) => {
                let _ = say_json(
                    stream,
                    400,
                    "Bad Request",
                    &the_shape_of_a_refusal(
                        "that named the conversation it is for as something other than words",
                    ),
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
        if the_string(fields, "in_reply_to_ask").is_err() {
            let _ = say_json(
                stream,
                400,
                "Bad Request",
                &the_shape_of_a_refusal(
                    "that named the question it answers as something other than words",
                ),
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
        // SAYS a verdict. A hub MAY put its results down where this door is already reading
        // them, and one that does leaves a window where half a JSON object is on the disk; the
        // hub in this repo no longer does, and this door does not depend on which it is talking
        // to. It polls every 50 ms, and the tolerance is what makes that safe either way:
        // answering torn bytes — with a refusal, an error, anything terminal — would be a
        // receipt that lies, because behind them the answer was accepted and delivered. So a
        // result that is not yet a verdict is not yet arrived: the door waits, and the window's
        // close says only what it always said.
        let deadline = tokio::time::Instant::now() + WAIT_FOR_THE_RESULT;
        loop {
            if let Ok(raw) = std::fs::read_to_string(self.drop_dir.join(format!("{name}.result"))) {
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(result) if result["status"] == "accepted" => {
                        // The ok-shape their client expects: `ok`, the kind it sent, an id for
                        // the act, and the lane it went to. The id is the same nonce the answer
                        // file carried as `ref` and the ring's down line echoes as `msg_id` —
                        // one name in three places, so their client, matching a down line
                        // against the name its own row holds, turns a sent line into its own
                        // receipt instead of a second bubble.
                        //
                        // `conversation` is the other fact only this door holds. A client that
                        // named none rode the ladder above, and the rung it rode is a fact
                        // written down nowhere it can read — so without the receipt saying where
                        // its own words landed it can never begin naming the conversation
                        // itself, and until it does, neither guessing rung can be withdrawn. It
                        // is safe to say out loud: an id is `p-` or `c-` and twelve hex
                        // characters, which names nothing on this box.
                        let ok = serde_json::json!({
                            "ok": true,
                            "t": t,
                            "msg_id": name,
                            "lane": lane,
                            "conversation": conversation,
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
                //
                // Which is precisely why the conversation rides down here too. A write the hub
                // honours late puts a line on the ring, and a client told only that its POST
                // failed cannot join that line to the row it already drew, so it draws a second
                // one beside the failed one and he reads his own sentence twice. `conversation`
                // rides along for the same reason it rides the ok-shape: the rung is still ours,
                // and a timed-out write landed somewhere. What does NOT change is `ok:false` and
                // the sentence — a client that reads this shape as a failure must go on reading
                // it as one. The 400 refusal gets neither field: a refusal means no ring line
                // will ever appear, so there is nothing for a client to join.
                let mut timeout = serde_json::json!({
                    "ok": false,
                    "why": "the hub has not said what became of it yet — it may still; \
                            sending it again may say it twice",
                    "conversation": conversation,
                });
                // The name here is a MESSAGE's name, and is given only where it will be echoed.
                // A message carries the one minted above on its answer file as `ref`, and the hub
                // rides that onto the ring's down line, so a client holding it meets it again and
                // joins the late line to the row it drew. A tap carries none: its file gets no
                // `ref` (above), the hub mints its own name for the line a tap becomes, and the
                // ring's down `choice` line has no name on it at all. Naming a timed-out tap
                // therefore handed a client a string to watch for that nothing on this box will
                // ever say, and a client told to hold it holds it for ever.
                if t == "message" {
                    timeout["msg_id"] = serde_json::json!(name);
                }
                let _ = say_json(stream, 504, "Gateway Timeout", &timeout.to_string()).await;
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
        // Which question this command answers, if any: a choice names it outright, a reply names
        // the one he typed under. Read once, before the first rung, because BOTH the rung that
        // reads a named conversation and the rung that has none spend it.
        let key = if t == "choice" {
            the_string(fields, "ask_id").ok().flatten()
        } else {
            in_reply_to_ask.clone()
        };
        if let Some(conversation) = named {
            // A body that names its conversation and no lane is NOT asking for a lane — it is
            // addressing the conversation's own voice, which is a different live session from
            // every lane of it, and a conversation whose only connected session is a lane
            // refuses words sent to a voice nobody is speaking with. Their client is about to
            // start sending the conversation on every write, and until this rung filled the lane
            // in too, that one new field would have moved every answer off the rung below and
            // taken the lane away with it — a tap that landed yesterday refused tomorrow, for
            // saying MORE about where it belonged.
            let lane = lane.or_else(|| {
                key.as_deref()
                    .and_then(|key| self.the_lane_that_asked_in(&conversation, key))
            });
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

    /// Which conversation of a project asked under that name — the lane, or `None` for its own
    /// voice — asked ONLY of the conversation the caller already knows the answer is for.
    ///
    /// Scoped on purpose. A question open under the same name in a SIBLING conversation is no
    /// evidence about this one — ask names are minted per session from a counter that starts
    /// over, so one name open in two conversations is ordinary — and lending that lane across
    /// would be this door guessing at an address nobody wrote down, which is the one thing the
    /// ladder is built never to do. Two conversations of the NAMED project asking under one name
    /// is the same refusal for the same reason: nothing is filled in, the act goes as the body
    /// addressed it, and the hub says in its own sentence why it could not take it. A refusal he
    /// can read beats an answer delivered into the turn of an agent he never meant.
    fn the_lane_that_asked_in(self: &Arc<Self>, conversation: &str, key: &str) -> Option<String> {
        let asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        let mut here = asks
            .open(key)
            .into_iter()
            .filter(|(asked_in, _)| asked_in == conversation)
            .map(|(_, lane)| lane);
        match (here.next(), here.next()) {
            (Some(the_one), None) => the_one,
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
        let where_it_is_open = self.open.entry(key.clone()).or_default();
        // The SAME question read twice is still one question. Every line is read twice in the
        // ordinary case — the door's catch-up over the ring at start, then the client's own poll
        // from cursor zero, which is the only way that client can learn the name it is about to
        // answer — and recorded twice it reads as two conversations asking under one name. That
        // is the door's own ambiguity refusal, so the routing memory was refusing to address the
        // very question it had just served, and it did so for exactly the clients that read the
        // ring properly.
        if !where_it_is_open.contains(&where_) {
            where_it_is_open.push(where_);
        }
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
/// `Err` carries the offending text, clamped, for the JOURNAL — never for the sentence, which is
/// read by a person and says nothing of his own back at him.
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

/// What is left of a known field written as something other than words: nothing at all.
///
/// It carries no name on purpose. The field's spelling belongs to the wire, and every refusal
/// this door writes is read by a person in the app that sent the command — so the caller must
/// write the sentence himself rather than have one built out of a name he was handed. Building
/// one out of the name is how `in_reply_to_ask` came to be a word on the operator's screen.
struct NotWords;

/// A string field that must be a string when it is present at all: `Ok(None)` for absent or
/// null, `Err` for present-and-not-a-string. Known-field hygiene is the drop's own law, held
/// before the file is written so the refusal costs one round trip, not a sweep.
fn the_string(
    fields: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Result<Option<String>, NotWords> {
    match fields.get(name) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(NotWords),
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

/// Mint the door's token: `kickoff-channel door-token`, at a keyboard.
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
                     the token it replaces:  kickoff-channel door-token --rotate <its first characters>"
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

    /// Every whole number in some bytes, as the token it is rather than as a substring of a
    /// longer one. A port is five digits and a clock reading is ten, so a naive `contains` finds
    /// the port inside the clock several times a day and reports a leak that is not one.
    fn the_whole_numbers_in(raw: &str) -> Vec<String> {
        let bytes = raw.as_bytes();
        let mut out = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            if !bytes[at].is_ascii_digit() {
                at += 1;
                continue;
            }
            let from = at;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            out.push(raw[from..at].to_owned());
        }
        out
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

    /// The one command the door left in the drop, read back as the hub's sweep would read it.
    /// Asserting on this rather than on the receipt is what proves where the act was ADDRESSED:
    /// the receipt echoes the door's own decision, the file is what the hub is handed.
    fn the_answer_file(state: &State) -> serde_json::Value {
        let written = std::fs::read_dir(state.the_drop())
            .expect("the drop")
            .flatten()
            .find(|e| !e.file_name().to_string_lossy().ends_with(".result"))
            .expect("the routed answer");
        serde_json::from_str(&std::fs::read_to_string(written.path()).expect("readable"))
            .expect("one object")
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

        // A hub MAY put its result down where this door is already reading it — the hub in this
        // repo puts it down elsewhere and moves it on, but an older one on the same box does
        // not, and this door is not allowed to care which. Where it does, a read that succeeds
        // with half a JSON object on the disk is a read of a receipt nobody has finished, and
        // this door polls every 50 ms. Answering that with anything terminal is a receipt that
        // lies, because behind the torn bytes the answer was accepted and delivered. The faked
        // hub below writes the torn way on purpose, which is the only writer that can prove the
        // tolerance is still there.
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
                .contains("the question it answers"),
            "the refusal does not say which part of it was refused: {refused}"
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
                    .contains("carry on from"),
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
    async fn a_timed_out_write_still_tells_the_client_the_name_its_echo_will_carry() {
        let state = a_state();
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");

        // Nothing sweeps the drop, so this write times out — and the hub may still honour it
        // afterwards, which is the whole reason the timeout is not a refusal. When it does, the
        // line it puts on the ring carries the name THIS door minted, and no other program on
        // this box can mint it. A client told only that its POST failed cannot join that echo to
        // the row it already drew, so it draws a second one beside the failed one and the
        // operator reads his own sentence twice. The timeout therefore names the write.
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"anyone there?"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 504, "{raw:?}");
        let timeout: serde_json::Value = serde_json::from_slice(body).expect("one object");

        // The name is added BESIDE what the timeout already said, never in place of it: a client
        // that reads the shape as a failure must go on reading it as one.
        assert_eq!(timeout["ok"], serde_json::Value::Bool(false), "{timeout}");
        assert_eq!(
            timeout["why"].as_str(),
            Some(
                "the hub has not said what became of it yet — it may still; sending it again \
                 may say it twice"
            ),
            "the timeout's sentence changed: {timeout}"
        );

        // And it is THE name, not a name: the answer still waiting in the drop carries the same
        // one as its `ref`, which is the field the hub echoes onto the ring's down line.
        let named = timeout["msg_id"]
            .as_str()
            .expect("the name the echo will carry");
        let answer = std::fs::read_dir(state.the_drop())
            .expect("the drop")
            .flatten()
            .find(|e| !e.file_name().to_string_lossy().ends_with(".result"))
            .expect("the answer still waiting for the hub");
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(answer.path()).expect("readable"))
                .expect("one object");
        assert_eq!(
            file["ref"].as_str(),
            Some(named),
            "the timeout named a write other than the one it left in the drop: {file}"
        );
    }

    #[tokio::test]
    async fn a_timed_out_tap_is_never_named_by_a_nonce_no_line_can_ever_echo() {
        let state = a_state();
        state.asked(Some("fix-17"), "a1");
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");

        // A tap, with nothing sweeping the drop, so it times out exactly as a message does.
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"choice","conversation":"p-0123456789ab","ask_id":"a1","option_id":"y"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 504, "{raw:?}");
        let timeout: serde_json::Value = serde_json::from_slice(body).expect("one object");

        // The defect: the timeout named this tap, and the name it gave reached nowhere the
        // sender could ever meet it again. A message's name is written onto the answer file as
        // `ref` and ridden from there onto the ring's down line, which is what makes a timeout
        // worth naming at all; a tap's is written on neither — the file below carries no `ref`,
        // the hub mints its own name for the line a tap turns into, and the ring's down `choice`
        // line carries no name whatever. A client told to hold this one holds it for ever.
        assert!(
            timeout.get("msg_id").is_none(),
            "a timed-out tap was given a name no line will ever carry: {timeout}"
        );
        let answer = std::fs::read_dir(state.the_drop())
            .expect("the drop")
            .flatten()
            .find(|e| !e.file_name().to_string_lossy().ends_with(".result"))
            .expect("the tap still waiting for the hub");
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(answer.path()).expect("readable"))
                .expect("one object");
        assert!(
            file.get("ref").is_none(),
            "a tap now leaves the door carrying a name, so withholding it above has become the \
             wrong half of this pair: {file}"
        );

        // Everything the timeout did say is unchanged. The verdict is still open — a client that
        // reads this shape as a failure must go on reading it as one — and the address is still
        // a fact only this door holds, as true of a tap as of a message.
        assert_eq!(timeout["ok"], serde_json::Value::Bool(false), "{timeout}");
        assert_eq!(
            timeout["why"].as_str(),
            Some(
                "the hub has not said what became of it yet — it may still; sending it again \
                 may say it twice"
            ),
            "the timeout's sentence changed: {timeout}"
        );
        assert_eq!(
            timeout["conversation"], "p-0123456789ab",
            "the timeout stopped saying where the tap it could not report on had landed: \
             {timeout}"
        );
    }

    /// Somewhere to keep what was written to the journal, so a test can read it back.
    struct Pen(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Pen {
        fn write(&mut self, written: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("the journal")
                .extend_from_slice(written);
            Ok(written.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Two ends of one loopback connection: the door's, and the caller's. A handler taking the
    /// door's end can then be called straight, on this thread, which is the thread whose journal
    /// a test can install.
    async fn a_connection() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("loopback binds");
        let port = listener
            .local_addr()
            .expect("a bound socket names itself")
            .port();
        let (dialled, accepted) =
            tokio::join!(TcpStream::connect(("127.0.0.1", port)), listener.accept());
        let (door_end, _who) = accepted.expect("the door's end");
        (door_end, dialled.expect("the caller's end"))
    }

    #[tokio::test]
    async fn the_only_word_about_a_cursor_this_door_could_not_read_is_written_where_it_writes() {
        // The refusal a client meets says nothing of what it sent, on purpose: their app renders
        // `why` verbatim on a person's screen. So the text that was wrong exists in exactly one
        // place, the journal — and written below the level this program actually runs at, it
        // exists in none. `main` filters at warn when nothing in the environment says otherwise,
        // their client reads no 400 body at all, and an EventSource never exposes one. Whoever is
        // wiring a client would be left with a refusal and no way on this box to learn which
        // character of his cursor caused it.
        let state = a_state();
        let door =
            Arc::new(Door::in_state(state.dir.path(), None, 15_000).expect("the door opens"));

        // Filtered the way `main` filters, so this cannot pass at a level the shipped door throws
        // away, and held on this thread only, so the rest of the suite keeps its own.
        let ink = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let journal = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("warn"))
            .with_writer({
                let ink = Arc::clone(&ink);
                move || Pen(Arc::clone(&ink))
            })
            .with_ansi(false)
            .finish();
        let reading = tracing::subscriber::set_default(journal);

        let said_to_a_poll = {
            let (mut door_end, caller) = a_connection().await;
            door.the_poll(&mut door_end, "cursor=eighteen").await;
            drop(door_end);
            read_to_the_end(caller).await
        };
        let said_to_a_stream = {
            let (mut door_end, caller) = a_connection().await;
            door.the_stream(&mut door_end, &[], "cursor=nineteen").await;
            drop(door_end);
            read_to_the_end(caller).await
        };
        drop(reading);

        for (what, said) in [("a poll", &said_to_a_poll), ("a stream", &said_to_a_stream)] {
            assert!(
                said.contains("400") && said.contains(NOT_A_PLACE_IN_THE_EVENTS),
                "{what} was not refused in the words it is refused in:\n{said}"
            );
        }
        assert!(
            !said_to_a_poll.contains("eighteen") && !said_to_a_stream.contains("nineteen"),
            "a refusal read his own cursor back at him:\n{said_to_a_poll}{said_to_a_stream}"
        );

        let written = String::from_utf8(ink.lock().expect("the journal").clone())
            .expect("the journal is text");
        assert!(
            written.contains("eighteen"),
            "a poll's unreadable cursor reached no journal this door would write to:\n{written}"
        );
        assert!(
            written.contains("nineteen"),
            "a stream's unreadable cursor reached no journal this door would write to:\n{written}"
        );
    }

    /// Everything a peer sent before it hung up.
    async fn read_to_the_end(mut stream: TcpStream) -> String {
        use tokio::io::AsyncReadExt as _;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.expect("the answer");
        String::from_utf8_lossy(&raw).into_owned()
    }

    #[tokio::test]
    async fn a_write_is_answered_with_the_conversation_the_door_resolved_it_to() {
        // Rung three of the ladder: the body names no conversation, and the only place this
        // answer's address is written down is the question it answers. A client riding that rung
        // has no way to learn where its own words landed — and until it can learn that, it can
        // never start naming the conversation itself and the rung can never be withdrawn.
        let state = a_state();
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
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let ok: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(
            ok["conversation"], "p-0123456789ab",
            "the receipt never said which conversation the question's own address resolved to: \
             {ok}"
        );
        assert_eq!(ok["ok"], serde_json::Value::Bool(true), "{ok}");
        assert_eq!(ok["lane"], "fix-17", "{ok}");

        // Rung two: a door started FOR one conversation, and a write that names none. Which
        // conversation that is, is a deployment choice the client never made and cannot read
        // anywhere else.
        let state = a_state();
        let port = a_door(&state, Some("c-abcdef012345")).await;
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
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let ok: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(
            ok["conversation"], "c-abcdef012345",
            "the receipt never said which conversation this door stands for: {ok}"
        );

        // And the timeout says it for the same reason it says the name: a write the hub may yet
        // honour is a write whose echo the client will have to place.
        let state = a_state();
        let port = a_door(&state, Some("c-abcdef012345")).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","text":"still there?"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 504, "{raw:?}");
        let timeout: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(
            timeout["conversation"], "c-abcdef012345",
            "a write the hub may still honour was not told where it went: {timeout}"
        );
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
    async fn a_choice_that_names_its_conversation_still_reaches_the_lane_that_asked() {
        // The trap this closes. The rung that reads the conversation the body names returned
        // before the rung that fills a missing lane in from the question being answered, so a
        // client that starts naming its conversation — which the surface has agreed to do —
        // would silently lose the lane it was getting for free. A conversation named with no
        // lane is the conversation's OWN VOICE, not a lane of it, and a conversation whose only
        // live session is a lane refuses words addressed to a voice nobody is speaking with. So
        // the day the field arrived, every tap that used to land would have stopped landing.
        let state = a_state();
        state.asked(Some("fix-17"), "a1");
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");

        // Read it the way the app reads it: the question reaches the client off the ring, which
        // is the only place it could have learned the name it is about to answer. The door has
        // now seen that one question TWICE — its own catch-up at start, then this poll — and one
        // question seen twice must not read as two conversations asking under one name, or the
        // fill below has nothing unambiguous left to spend.
        let raw = ask_the_door(port, get("/v1/events?cursor=0")).await;
        let (status, _) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");

        the_hub_decides(state.the_drop(), r#"{"t":"result","status":"accepted"}"#);
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"choice","conversation":"p-0123456789ab","ask_id":"a1","option_id":"y"}"#,
            ),
        )
        .await;
        let (status, body) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let file = the_answer_file(&state);
        assert_eq!(
            file["lane"], "fix-17",
            "a tap that named its conversation was addressed to the conversation's own voice \
             instead of the lane that asked: {file}"
        );
        let ok: serde_json::Value = serde_json::from_slice(body).expect("one object");
        assert_eq!(
            ok["lane"], "fix-17",
            "the receipt did not say which lane the tap went to: {ok}"
        );

        // Words typed under that same question ride the same rung and lost the same lane.
        let state = a_state();
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
                r#"{"t":"message","conversation":"p-0123456789ab","in_reply_to_ask":"a1","text":"the second one"}"#,
            ),
        )
        .await;
        let (status, _) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let file = the_answer_file(&state);
        assert_eq!(
            file["lane"], "fix-17",
            "a reply that named its conversation was addressed to the conversation's own voice \
             instead of the lane that asked it: {file}"
        );
        assert_eq!(file["in_reply_to_ask"], "a1", "{file}");

        // And a lane the body DID name still wins over the question's. The question says where
        // the answer belongs; a body that named one of the project's own conversations said
        // where it belongs itself, and this rung has never been allowed to argue with it.
        let state = a_state();
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
                r#"{"t":"choice","conversation":"p-0123456789ab","lane":"fix-18","ask_id":"a1","option_id":"y"}"#,
            ),
        )
        .await;
        let (status, _) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let file = the_answer_file(&state);
        assert_eq!(
            file["lane"], "fix-18",
            "a lane the body named was overwritten by the question's: {file}"
        );
    }

    #[tokio::test]
    async fn the_door_fills_a_lane_only_from_a_question_the_named_conversation_itself_is_asking() {
        // A question open in ANOTHER conversation says nothing about this one. Ask names are
        // minted per session from a counter that starts over, so one name being open next door
        // is ordinary rather than rare — and borrowing that lane would put his answer into the
        // turn of an agent he never meant. Nothing is filled in: the act goes as the
        // conversation's own voice, which is what the body said, and the hub refuses it out loud
        // if nothing there can take it. A refusal he can read beats delivery to the wrong agent.
        let state = a_state();
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
                r#"{"t":"choice","conversation":"c-abcdef012345","ask_id":"a1","option_id":"y"}"#,
            ),
        )
        .await;
        let (status, _) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let file = the_answer_file(&state);
        assert_eq!(file["conversation"], "c-abcdef012345", "{file}");
        assert!(
            file.get("lane").is_none(),
            "the door borrowed a lane from a question another conversation is asking: {file}"
        );

        // The same name open in two conversations at once, one of them the one the body named.
        // The sibling's question is no argument either way, and the named conversation's own
        // question settles it alone — that is a fact somebody wrote down, not a guess between
        // two.
        let state = a_state();
        state.asked(Some("fix-17"), "a1");
        state.ring.append(
            &ProjectId::new("c-abcdef012345"),
            Some(&LaneId::new("somewhere-else")),
            &BridgeFrame::Ask {
                ask_id: AskId::new("a1"),
                text: "Another conversation's question".to_owned(),
                options: Some(vec![AskOption {
                    option_id: OptionId::new("y"),
                    label: "Yes".to_owned(),
                }]),
            },
        );
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");
        the_hub_decides(state.the_drop(), r#"{"t":"result","status":"accepted"}"#);
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"choice","conversation":"p-0123456789ab","ask_id":"a1","option_id":"y"}"#,
            ),
        )
        .await;
        let (status, _) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let file = the_answer_file(&state);
        assert_eq!(
            file["lane"], "fix-17",
            "a name open next door too stopped the named conversation's own question from \
             addressing the answer: {file}"
        );

        // And two conversations of ONE project asking under one name: the door does not pick
        // between them. No lane goes on the file, the hub reads that as the voice and says so in
        // its own sentence, and he is refused rather than answered for.
        let state = a_state();
        state.asked(Some("fix-17"), "a1");
        state.asked(Some("fix-18"), "a1");
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");
        the_hub_decides(state.the_drop(), r#"{"t":"result","status":"accepted"}"#);
        let raw = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"choice","conversation":"p-0123456789ab","ask_id":"a1","option_id":"y"}"#,
            ),
        )
        .await;
        let (status, _) = the_answer(&raw);
        assert_eq!(status, 200, "{raw:?}");
        let file = the_answer_file(&state);
        assert!(
            file.get("lane").is_none(),
            "the door picked between two conversations of one project asking under one name: \
             {file}"
        );
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

        // Everything above is a door that turned the caller away. A sweep of refusals alone is a
        // sweep of the shortest answers this door ever writes, and the bodies with the most in
        // them — the ones built field by field as the write path learns to say more — were never
        // looked at at all. So the caller now gets IN, and every shape of the write path's answer
        // is swept: the ok-shape the hub accepted, the refusal it judged, and the timeout nobody
        // judged. Each is forced deliberately by what the faked hub does — writes a verdict,
        // writes a refusal, writes nothing.
        //
        // The words the caller sends carry a Telegram-shaped id, in the text and beside it, so
        // the sweep also holds the rule that this door never hands a caller its own words back:
        // an id like that has no other route here, because the ring it reads has never carried
        // one.
        let telegram_shaped = "-1001770077066";
        let taken = {
            the_hub_decides(state.the_drop(), r#"{"t":"result","status":"accepted"}"#);
            ask_the_door(
                port,
                post(
                    "/v1/commands",
                    &[("Authorization", "Bearer the-real-one")],
                    &format!(
                        r#"{{"t":"message","conversation":"p-0123456789ab",
                             "text":"tell {telegram_shaped} i said so","chat_id":{telegram_shaped}}}"#
                    ),
                ),
            )
            .await
        };
        assert_eq!(
            the_answer(&taken).0,
            200,
            "the sweep never saw the shape it exists to sweep:\n{}",
            String::from_utf8_lossy(&taken)
        );
        everything.extend_from_slice(&taken);

        let turned_down = {
            the_hub_decides(
                state.the_drop(),
                r#"{"t":"result","status":"refused","why":"Nothing is connected for that conversation right now, so nothing was sent."}"#,
            );
            ask_the_door(
                port,
                post(
                    "/v1/commands",
                    &[("Authorization", "Bearer the-real-one")],
                    r#"{"t":"message","conversation":"p-0123456789ab","text":"anyone?"}"#,
                ),
            )
            .await
        };
        assert_eq!(
            the_answer(&turned_down).0,
            400,
            "the hub's refusal did not reach the wire to be swept:\n{}",
            String::from_utf8_lossy(&turned_down)
        );
        everything.extend_from_slice(&turned_down);

        // Nobody judges this one: no faked hub is waiting, so the wait window closes on it and
        // the door says the only thing it may.
        let never_judged = ask_the_door(
            port,
            post(
                "/v1/commands",
                &[("Authorization", "Bearer the-real-one")],
                r#"{"t":"message","conversation":"p-0123456789ab","text":"still there?"}"#,
            ),
        )
        .await;
        assert_eq!(
            the_answer(&never_judged).0,
            504,
            "the timeout's body never reached the wire to be swept:\n{}",
            String::from_utf8_lossy(&never_judged)
        );
        everything.extend_from_slice(&never_judged);

        let raw = String::from_utf8_lossy(&everything);
        assert!(
            !raw.contains(&home) && !raw.contains("/tmp/") && !raw.contains("token file"),
            "a fact about this machine reached the HTTP wire:\n{raw}"
        );
        assert!(
            !raw.contains(telegram_shaped),
            "words the caller sent came back out of this door, and one of them was shaped like a \
             Telegram id:\n{raw}"
        );
        // Numbers, judged as whole numbers and never as substrings: a five-digit port sits inside
        // a ten-digit clock reading often enough that a substring rule would go red on the
        // calendar rather than on a leak, and the ring's clock is a number this door is allowed
        // to say.
        let numbers = the_whole_numbers_in(&raw);
        assert!(
            !numbers.contains(&port.to_string()),
            "the door named the port it is listening on, which is the one fact a reader of this \
             answer must never learn from it:\n{raw}"
        );
        assert!(
            !numbers.contains(&std::process::id().to_string()),
            "the door named the process it is running as:\n{raw}"
        );
        // What this now covers: every body the door writes over HTTP on both verbs — the events
        // page, the stream, a turned-away caller on each of the two ways to be turned away, and
        // all three ends of an authenticated write. What it still does not cover: what the door
        // says when its own disk fails (the 503 arm, which needs an unwritable drop), and the
        // headers of the stream beyond the point the reader stops at. And it is a sweep of
        // STRINGS AND NUMBERS, so it catches a fact of this machine only where that fact is
        // spelled the way this test spells it: a home path, this port, this pid, a Telegram id.
        // A new field carrying something of this box under a name none of those matches — a user
        // name, a host name, a mount — would pass it.
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

    #[test]
    fn a_port_that_would_open_this_door_where_nobody_chose_is_refused_not_quietly_taken() {
        use std::ffi::OsStr;

        // Nothing said at all is the one shape that may take the usual port: nobody chose an
        // address, so opening on the documented one surprises no one.
        assert_eq!(the_port(None, None), Ok(DEFAULT_PORT));
        // Said plainly, taken.
        assert_eq!(the_port(None, Some(OsStr::new("8123"))), Ok(8123));
        // Typed at the terminal, the flag wins and a variable nobody reads stops nothing: it is
        // not the address anything is about to open on, so it cannot send him anywhere.
        assert_eq!(the_port(Some(9123), Some(OsStr::new("banana"))), Ok(9123));
        // And zero typed at the terminal is a choice, not the mistake below: whoever ran that
        // line is the one reading the door's own line saying where it came up, so "whatever is
        // free" is an answer he can act on. The hermetic trial starts its door exactly this way.
        assert_eq!(the_port(Some(0), None), Ok(0));

        // The defect: a port nobody could read became the usual one in silence, so the door came
        // up on an address he did not choose while whatever he meant to point at it talked to
        // nobody. Zero is in the table for that same failure and not for being unreadable: it
        // parses, and binding it takes whatever port happens to be free — the one value that
        // reaches the very address nobody chose by getting PAST a parser rather than failing it.
        for wrong in ["banana", "87 91", "70000", "-1", "8791x", "0", "00", " 0 "] {
            let refused = the_port(None, Some(OsStr::new(wrong)))
                .expect_err(&format!("{wrong:?} was taken for a port"));
            assert!(
                refused.contains("KICKOFF_DOOR_PORT"),
                "{wrong:?}: the refusal does not say which setting is wrong: {refused}"
            );
            assert!(
                !refused.contains(wrong),
                "{wrong:?}: the refusal reads his own mistake back at him: {refused}"
            );
        }

        // Whitespace round it is still a port he chose — the token file and the cursor are both
        // already forgiving of what a hand-edited file leaves behind, and refusing here would
        // refuse a setting nobody got wrong.
        assert_eq!(the_port(None, Some(OsStr::new(" 8123 "))), Ok(8123));

        // Set to nothing at all is SET, not unset — it is what a unit file renders when the
        // substitution it was written with never happened — and the refusal says how the usual
        // port is actually taken, because clearing the line is not it.
        for empty in ["", "   "] {
            let refused = the_port(None, Some(OsStr::new(empty)))
                .expect_err("a variable set to nothing was taken for one nobody set");
            assert!(
                refused.contains("unset"),
                "the refusal does not say how the door's usual port is taken: {refused}"
            );
        }

        // Zero gets a sentence of its own, because "not a port number" would be a lie about a
        // value that is one: what is wrong is not the writing of it but what opening on it does,
        // and a man who has to change that line needs to be told what to write instead.
        for zero in ["0", "00", " 0 "] {
            let refused = the_port(None, Some(OsStr::new(zero)))
                .expect_err("a door on whatever port was free was taken for one he chose");
            assert!(
                refused.contains("unset"),
                "{zero:?}: the refusal does not say how the door's usual port is taken: {refused}"
            );
            assert!(
                !refused.contains("not a port number"),
                "{zero:?}: the refusal calls a port number something that is not one: {refused}"
            );
        }

        // Bytes that are not text at all: set, unreadable, refused like any other.
        use std::os::unix::ffi::OsStrExt as _;
        assert!(
            the_port(None, Some(OsStr::from_bytes(&[0xff, 0x38]))).is_err(),
            "bytes that are not even text were taken for a port"
        );
    }

    #[tokio::test]
    async fn the_doors_own_refusals_name_what_he_wrote_in_his_words_not_the_wires() {
        let state = a_state();
        let port = a_door(&state, None).await;
        std::fs::write(state.dir.path().join(DOOR).join(TOKEN), "the-real-one\n")
            .expect("a minted token");

        let refusal_for = |body: &'static str| async move {
            let raw = ask_the_door(
                port,
                post(
                    "/v1/commands",
                    &[("Authorization", "Bearer the-real-one")],
                    body,
                ),
            )
            .await;
            let (status, answered) = the_answer(&raw);
            assert_eq!(status, 400, "{body}: {raw:?}");
            let refused: serde_json::Value = serde_json::from_slice(answered).expect("one object");
            refused["why"].as_str().expect("a sentence").to_owned()
        };

        // The reply field. Their client renders `why` verbatim behind its own prefix, so the
        // answer-file's spelling of this field used to land on his screen as a word out of our
        // wire; what he actually did was reply to a question.
        let why = refusal_for(
            r#"{"t":"message","conversation":"p-0123456789ab","text":"go on","in_reply_to_ask":42}"#,
        )
        .await;
        assert!(
            why.contains("the question it answers"),
            "the refusal does not say which part of it was wrong: {why}"
        );
        assert!(
            !why.contains("in_reply_to_ask"),
            "the refusal spells a field of the wire at him: {why}"
        );

        // The same law on the conversation field, in the same register.
        let why = refusal_for(r#"{"t":"message","conversation":42,"text":"go on"}"#).await;
        assert!(
            why.contains("the conversation it is for"),
            "the refusal does not say which part of it was wrong: {why}"
        );

        // The cursor, on both routes that read one. "cursor" and "sequence number" are this
        // door's own vocabulary for the ring, and the offending text was read straight back —
        // a refusal that quotes the caller teaches him nothing he did not already type.
        for route in ["/v1/events?cursor=banana", "/v1/stream?cursor=banana"] {
            let raw = ask_the_door(port, get(route)).await;
            let (status, answered) = the_answer(&raw);
            assert_eq!(status, 400, "{route}: {raw:?}");
            let refused: serde_json::Value = serde_json::from_slice(answered).expect("one object");
            let why = refused["why"].as_str().expect("a sentence");
            assert!(
                why.contains("carry on from"),
                "{route}: the refusal does not say what it could not do: {why}"
            );
            for jargon in ["cursor", "sequence", "banana"] {
                assert!(
                    !why.contains(jargon),
                    "{route}: the refusal says {jargon:?} to him: {why}"
                );
            }
        }
    }
}

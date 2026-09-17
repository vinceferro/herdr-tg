//! The ring: one line per operator-visible event, for a reader that is not on this machine.
//!
//! The PWA is the operator's work surface now, and this hub is the conversation plane. The ring
//! is the seam between them: the hub appends what he would have seen — an agent's `say`, its
//! `ask`, its `done`, an `ask` stopping being open, the hub's own retirement of a question a
//! dead session left open, and HIS OWN words and taps on their way down to an agent — and a
//! later increment serves those lines to the PWA over HTTP. Nothing consumes the ring yet; the
//! hub writes it regardless, because the events it misses before a reader exists are the events
//! no reader will ever see.
//!
//! # The egress law, which is the whole design
//!
//! This file will one day leave this machine — that is its purpose. So it carries NO chat id, NO
//! topic id, NO user id, NO Telegram message id, NO filesystem path and NO token or secret:
//!
//! * The envelope names a conversation by its id (`p-…`/`c-…`, the shapes the registry mints) and
//!   a lane by its address or `-`, which is the wire's own spelling for the project's own voice.
//!   Neither is a fact about this machine the way a path or a chat id is.
//! * The `hello` that carries the token is never recorded, and neither is any other frame that is
//!   not one of the four operator-visible kinds. A `bye`, an `ack`, a `beat` reaches nobody's
//!   eyes and takes nothing of his with it.
//! * An agent's own words ARE the event, so they are recorded — but no absolute path survives
//!   them: scrubbed to `[a path]` before the line is built, because `/home/…/secret.txt` in an
//!   agent's sentence is the one place a path reaches this file from. Clipped, too, by the same
//!   [`crate::queue::fit`] the queue uses at its max text, so one line is bounded.
//! * One rule for every string in the frame, applied in one pass: scrub, then clip. A rule with
//!   exceptions is a rule somebody has to remember, and the field somebody forgets is the one
//!   that leaves.
//!
//! What the law cannot cover: a secret an agent pastes into its own words is indistinguishable
//! from any other word to a scanner that does not know it, and goes where the words go. The same
//! words already reached his phone; the ring is not a new disclosure, it is a new reader.
//!
//! And what the scrubber ACCEPTS, by ruling rather than by proof: relative paths with no leading
//! slash (`src/hub.rs`), backslash forms (`C:\Users\…`, `..\x`), a bare `~` with no slash after
//! it, and any scheme that is not exactly lower-case `http://` or `https://`. Each is contrived —
//! this box's agents quote POSIX paths and lower-case URLs — and the ruling is the same
//! settlement the write guard received: the shapes nobody produces are not worth the mangle that
//! chasing them would put on ordinary words.
//!
//! # As spoken, not as delivered — and the down half's one exception
//!
//! A line is stamped where the hub first handles the frame — before the gist rewrites a question
//! and before the pacer decides a wait — and it carries no delivery field, no seen, no shed.
//! Whether the phone took a message, clipped it, or refused it for the ceiling is the phone
//! ledger's fact (the audit beside this file, and the marks he reads), and a second copy of it
//! here would be two files that can disagree about one thing. The one hub-minted line is the
//! exception that proves the shape: a retirement the hub itself performed
//! ([`Ring::resolved_by_the_hub`]) is stamped where the edit was observed to land, because that
//! edit is the event.
//!
//! The DOWN half ([`Ring::the_operator_said`], [`Ring::the_operator_chose`]) is stamped only
//! where the frame was actually handed to a live connection, for the reason the up half's rule
//! exists mirrored: the PWA's echo of his own words is a receipt, and a receipt for words that
//! reached nobody is the one thing it must never say. A refusal is the door's result file's
//! fact (or the phone's line), not the ring's — the ring records what happened, not what did
//! not.
//!
//! # A ring failure is never an agent's failure
//!
//! The ring is written at the seam the audit is, from the hub's frame handling, and it is the
//! least important thing that happens there. Every IO error on it is logged and swallowed: a
//! question must never fail to reach the operator because a log for another machine could not
//! be written. A ring whose history cannot be read at start is closed for the run rather than
//! restarted at 1 — a reader that joined before would see the same seq twice and could not tell
//! the two events apart, and that is the one failure worth refusing over.
//!
//! # The files
//!
//! `hub.ring.ndjson` is the active file; `hub.ring.1.ndjson` is the one old file kept, and a
//! reader orders them `.1.` first. Around a megabyte the active file is rotated: renamed onto
//! the old one — whose previous contents are spent, which is what "bounded" means — and started
//! again. `seq` never resets, across a rotation or across a hub restart, so a reader holding a
//! cursor can always ask for "after this one" and be answered. Each line is written whole in one
//! write, and a restart heals the tail of a write that never finished rather than building on it
//! — see [`the_floor`].

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hub_proto::{AskId, BridgeFrame, LaneId, OptionId, ProjectId};

use super::now_secs;

/// The active ring file, beside the audit log and every other state file of this hub.
pub const RING: &str = "hub.ring.ndjson";

/// The one old file a rotation keeps. A reader takes it before the active file.
pub const RING_OLD: &str = "hub.ring.1.ndjson";

/// The active file is rotated onto the old one around this size.
///
/// A megabyte, because that is a few hundred events at the clip ceiling — enough that a reader
/// reconnecting after a minute misses nothing, small enough that two files bound the ring's
/// whole life on disk to about the size of one screenshot.
pub const ROTATE_AT: u64 = 1024 * 1024;

/// What an absolute or home-relative path becomes in a recorded line.
const A_PATH: &str = "[a path]";

/// Which way the event was travelling when the hub saw it. Up is an agent's own words; down is
/// the operator's — his typed lines and his taps, from whichever surface he said them on, so a
/// reader holding one file sees both halves of one conversation.
const UP: &str = "up";

/// The operator's half. See the module docs for why a down line exists only where the frame was
/// delivered.
const DOWN: &str = "down";

/// The ring itself: a path pair, the next sequence number, and whether this run may write.
///
/// Interior mutability and a `std::sync::Mutex`, like the hub's own titles cache: the append is
/// a handful of syscalls with no await in it, and a lock never held across an await is a lock
/// with no order to get wrong.
#[derive(Debug)]
pub struct Ring(Mutex<Where>);

/// What the lock guards: where the files are, where the sequence has got to, how big the active
/// file was as of the last line we know about, and whether this run writes at all.
#[derive(Debug)]
struct Where {
    active: PathBuf,
    old: PathBuf,
    next_seq: u64,
    bytes: u64,
    /// Closed when the history could not be read at start: continuing would mint a second event
    /// with a number the ring already used, and a reader cursored on the first could not tell
    /// them apart. See the module docs — this is the one refusal the ring makes.
    closed: bool,
}

impl Ring {
    /// The ring beside the hub's other state, resuming the sequence from whatever is already
    /// there. IO here is not swallowed: a hub that cannot read its own ring still serves, but it
    /// must not serve a ring that lies about which event is which.
    pub fn in_dir(dir: &Path) -> Self {
        let active = dir.join(RING);
        let old = dir.join(RING_OLD);

        // The active file is authoritative — it holds the newest events whenever it holds any.
        // An empty or missing one falls back to the old file, which is exactly the state a crash
        // between a rotation's rename and the next append leaves behind.
        let (floor, bytes) = match the_floor(&active) {
            Floor::At(seq) => (seq, fs::metadata(&active).map_or(0, |m| m.len())),
            Floor::Gone | Floor::Empty => match the_floor(&old) {
                Floor::At(seq) => (seq, 0),
                Floor::Gone | Floor::Empty => (0, 0),
                Floor::Unreadable(why) => {
                    tracing::error!(
                        error = %why, path = %old.display(),
                        "the ring of operator-visible events could not be read, so it stays \
                         closed for this run; nothing is recorded until the hub restarts"
                    );
                    return Self(Mutex::new(Where {
                        active,
                        old,
                        next_seq: 1,
                        bytes: 0,
                        closed: true,
                    }));
                }
            },
            Floor::Unreadable(why) => {
                tracing::error!(
                    error = %why, path = %active.display(),
                    "the ring of operator-visible events could not be read, so it stays closed \
                     for this run; nothing is recorded until the hub restarts"
                );
                return Self(Mutex::new(Where {
                    active,
                    old,
                    next_seq: 1,
                    bytes: 0,
                    closed: true,
                }));
            }
        };

        Self(Mutex::new(Where {
            active,
            old,
            next_seq: floor + 1,
            bytes,
            closed: false,
        }))
    }

    /// Record one frame, if it is one of the four operator-visible kinds.
    ///
    /// Never fails and never awaits. The hub calls this as a frame reaches its handling — before
    /// the gist rewrites a question, before the pacer decides a wait — so what the ring holds is
    /// what the agent said, not what this hub did with it.
    pub fn append(&self, conversation: &ProjectId, lane: Option<&LaneId>, frame: &BridgeFrame) {
        if !the_shape_the_registry_mints(conversation) {
            return;
        }
        let Some(frame) = the_operator_visible(frame) else {
            return;
        };
        self.stamp(conversation, lane, frame, UP);
    }

    /// A question the HUB itself put away, in the wire's own vocabulary.
    ///
    /// The one operator-visible event minted hub-side: a session that dies holding a question has
    /// its keyboard taken off by the sweep, and that edit never passes the hub's frame handling —
    /// so without this line the PWA's only history would show the question open for ever. The
    /// frame reuses `ask_resolved`, the wire's own word for "this question stopped being open",
    /// with the hub's sentence in `how` — the same words the phone reads — rather than a frame
    /// kind nobody on the wire has ever spoken. No new kind, no change to hub-proto.
    ///
    /// Called only where the retirement was OBSERVED to land (`retire_each`'s success arm), so
    /// the ring never says a keyboard came off one Telegram refused to take.
    pub fn resolved_by_the_hub(
        &self,
        conversation: &ProjectId,
        lane: Option<&LaneId>,
        ask_id: &AskId,
        how: &str,
    ) {
        if !the_shape_the_registry_mints(conversation) {
            return;
        }
        // The sentence can carry a button's label — the phone's own "answered from your phone —
        // …", an agent's words — so it goes through the same single rule every string in a frame
        // does, rather than being trusted for being the hub's.
        let mut frame = serde_json::json!({
            "t": "ask_resolved",
            "ask_id": ask_id.as_str(),
            "how": how,
        });
        nothing_of_this_machine(&mut frame);
        self.stamp(conversation, lane, frame, UP);
    }

    /// His own words, on their way down to an agent — the down half of the ring.
    ///
    /// Recorded at the shared delivery seam where the hub hands a `message` frame to a live
    /// connection, from EITHER surface he typed on, because the PWA's one history must show his
    /// half of the conversation or it is a transcript of a monologue. The frame is the wire's
    /// own vocabulary with the phone's plumbing left OUT: `msg_id` is a Telegram message id and
    /// `from` is a chat-and-person pair, and both are exactly the identifiers this file's law
    /// forbids — the event is his words, not the phone's.
    ///
    /// Called only where the frame was DELIVERED, never on a refusal: an echo is a receipt, and
    /// the ring must not tell him his words reached an agent when they did not.
    pub fn the_operator_said(
        &self,
        conversation: &ProjectId,
        lane: Option<&LaneId>,
        text: &str,
        in_reply_to_ask: Option<&AskId>,
    ) {
        if !the_shape_the_registry_mints(conversation) {
            return;
        }
        let mut frame = serde_json::json!({ "t": "message", "text": text });
        if let Some(ask) = in_reply_to_ask {
            frame["in_reply_to_ask"] = serde_json::json!(ask.as_str());
        }
        nothing_of_this_machine(&mut frame);
        self.stamp(conversation, lane, frame, DOWN);
    }

    /// A tap of his, on its way down to the agent that asked — the down half of the ring.
    ///
    /// The twin of [`Ring::the_operator_said`], at the seam a `choice` frame crosses, from
    /// either surface. The frame names the question and the answer he chose and nothing else:
    /// the `msg_id` the wire frame carries is a Telegram message id, which this file does not
    /// take. The button's LABEL is deliberately absent too — it is the bridge's own words, and
    /// the reader that wants it has it on the `ask` line already.
    pub fn the_operator_chose(
        &self,
        conversation: &ProjectId,
        lane: Option<&LaneId>,
        ask_id: &AskId,
        option_id: &OptionId,
    ) {
        if !the_shape_the_registry_mints(conversation) {
            return;
        }
        let mut frame = serde_json::json!({
            "t": "choice",
            "ask_id": ask_id.as_str(),
            "option_id": option_id.as_str(),
        });
        nothing_of_this_machine(&mut frame);
        self.stamp(conversation, lane, frame, DOWN);
    }

    /// The one built frame onto the ring, wearing the next number.
    fn stamp(
        &self,
        conversation: &ProjectId,
        lane: Option<&LaneId>,
        frame: serde_json::Value,
        dir: &'static str,
    ) {
        let mut where_ = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if where_.closed {
            return;
        }
        let line = Line {
            seq: where_.next_seq,
            ts: now_secs(),
            dir,
            conversation: conversation.as_str(),
            lane: lane.map_or("-", LaneId::as_str),
            frame: &frame,
        };
        let body = match serde_json::to_string(&line) {
            Ok(body) => body,
            // Cannot happen — every part is a string or a number this code built — and the
            // honest answer to it is the same as every other failure here: say so, carry on.
            Err(e) => {
                tracing::warn!(error = %e, "a ring line could not be built; the event is not recorded");
                return;
            }
        };
        if let Err(e) = where_.append(&body) {
            tracing::warn!(
                error = %e,
                "the ring of operator-visible events could not be written; nothing was delayed \
                 and nothing was lost from the conversation itself"
            );
            // The number this line would have worn is not spent: a reader holding the last seq
            // that landed must be answered by the NEXT line with seq+1, and a number consumed by
            // a write nobody saw would be a gap nothing can explain.
            return;
        }
        where_.next_seq += 1;
    }
}

impl Where {
    /// One line onto the active file, rotating first if this line would cross the bound.
    fn append(&mut self, line: &str) -> std::io::Result<()> {
        if self.bytes > 0 && self.bytes + line.len() as u64 > ROTATE_AT {
            // A rotation that fails leaves the active file in place and over the bound, which is
            // logged and retried on the next line — not fatal, because the one thing this file
            // may never do is take the conversation down with it.
            if let Err(e) = self.rotate() {
                tracing::warn!(
                    error = %e,
                    "the ring's active file could not be rotated; it grows past its bound until \
                     this succeeds"
                );
            }
        }
        // The same seam the audit writes on: the directory re-asserted private before anything is
        // appended, because a state directory left wider than 0700 is one every file in it
        // inherits.
        if let Some(dir) = self.active.parent() {
            crate::conversations::private_state_dir(dir)?;
        }
        let mut f = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&self.active)?;
        // Re-asserted: `.mode()` applies only at creation, and a file that was once world-readable
        // stays that way through every append.
        let _ = fs::set_permissions(&self.active, fs::Permissions::from_mode(0o600));
        // ONE write, line and newline together. Two writes leave a window between them where
        // death or a full disk has put the line's bytes down without its terminator — and the
        // next event glues onto it, making both unparseable for ever. A partial single write is
        // the torn tail `the_floor` heals at the next start.
        let mut whole = Vec::with_capacity(line.len() + 1);
        whole.extend_from_slice(line.as_bytes());
        whole.push(b'\n');
        f.write_all(&whole)?;
        self.bytes += whole.len() as u64;
        Ok(())
    }

    /// The active file becomes the old one; the old one's contents are spent.
    ///
    /// One old file, not a growing family: "bounded" is the property, and a reader that needs
    /// further back than the last megabyte is a reader for the audit, which is not this file.
    fn rotate(&mut self) -> std::io::Result<()> {
        match fs::remove_file(&self.old) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        fs::rename(&self.active, &self.old)?;
        self.bytes = 0;
        Ok(())
    }
}

/// The conversation becomes half of what a reader addresses an event by, so it must be the shape
/// the registry mints and nothing else: an id of another shape could be any string the wire chose,
/// and the ring's whole promise is that it names nothing of this machine.
fn the_shape_the_registry_mints(conversation: &ProjectId) -> bool {
    let shaped = crate::conversations::is_conversation_id(conversation.as_str());
    if !shaped {
        tracing::warn!(
            conversation = %conversation,
            "an event arrived for a conversation whose id is not a shape this hub minted; the \
             ring records nothing about it"
        );
    }
    shaped
}

/// The wire frame as the ring records it, or nothing — and nothing is every other kind.
///
/// Four kinds are an operator-visible event: something said, something asked, a turn finished, a
/// question stopped being open. The `hello` that carries the token is not among them, nor the
/// bookkeeping a bridge answers with; serialising the frame itself keeps the wire's own field
/// spellings, so a reader learns one vocabulary from both ends.
fn the_operator_visible(frame: &BridgeFrame) -> Option<serde_json::Value> {
    match frame {
        BridgeFrame::Say { .. }
        | BridgeFrame::Ask { .. }
        | BridgeFrame::Done { .. }
        | BridgeFrame::AskResolved { .. } => {}
        _ => return None,
    }
    let mut value = serde_json::to_value(frame).ok()?;
    nothing_of_this_machine(&mut value);
    Some(value)
}

/// One pass, one rule: every string in the frame loses its paths and its length.
///
/// The agent's own words are the event, so they are kept — but no absolute or home-relative path
/// survives them, and nothing survives past the clip the queue uses, so a line is bounded by the
/// same ceiling an operator's screen is. Applied to EVERY string rather than to the fields their
/// names suggest, because the field somebody forgets to add to the list is the one that leaves.
fn nothing_of_this_machine(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => {
            *s = crate::queue::fit(&no_paths(s), crate::queue::MAX_TEXT).0;
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(nothing_of_this_machine),
        serde_json::Value::Object(o) => o.values_mut().for_each(nothing_of_this_machine),
        _ => {}
    }
}

/// A byte that can sit inside a path, a URL, or a word that might hold either.
///
/// The path bytes proper, plus the punctuation a URL carries (`: ? & = # % @ +`) — a token is a
/// run of these, and everything else (a space, a comma, a bracket) is an edge a path or a URL
/// cannot cross. The edges are what keep `/etc/x` inside `see(/etc/x)` scrubbed while the
/// sentence around it stands, and what keep a trailing comma out of a spared URL's span.
fn is_token_byte(b: u8) -> bool {
    is_path_byte(b) || matches!(b, b':' | b'?' | b'&' | b'=' | b'#' | b'%' | b'@' | b'+')
}

/// The bytes a path is built from, on this machine. A `:` or a `?` breaks a path's run even
/// though a token may carry on past it.
fn is_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'~' | b'/')
}

/// Every absolute and home-relative path in the words, replaced by `[a path]` — with `http://`
/// and `https://` URLs spared whole.
///
/// The shapes that name a place on THIS machine — `/`-rooted and `~`-rooted paths, taken out
/// whole — are not the same as a URL, and coding agents cite URLs constantly: the ring is the
/// PWA's only history, and mangling every link an agent pasted would quietly gut it. A token
/// that CONTAINS `http://` or `https://` is a citation — something may be glued to its front
/// (`src=https://…`), and the glued thing is not a path — so the whole token is spared. Every
/// other scheme is a path wearing a costume: `file:///home/…` scrubs exactly where the `/home/…`
/// inside it would. Lower case only, because that is the spelling the law names and a
/// capitalised scheme is nobody's citation.
///
/// Repository-relative paths (`src/hub.rs`) are kept: they name a place in a conversation's own
/// work and carry nothing of the operator's filesystem. Non-token bytes are edges too — this
/// box's paths are ASCII — and a scanner that guessed further would be a scanner guessing.
fn no_paths(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;

    while at < bytes.len() {
        // An edge passes through untouched, byte for byte — the string is only ever rebuilt
        // where a path is taken out of it.
        if !is_token_byte(bytes[at]) {
            out.push(bytes[at]);
            at += 1;
            continue;
        }
        // A token begins here. Find where it ends, and treat the whole token at once: whether it
        // is a URL citation is a fact about the token, not about any one slash in it.
        let token_from = at;
        let mut token_to = at;
        while token_to < bytes.len() && is_token_byte(bytes[token_to]) {
            token_to += 1;
        }
        let token = &text[token_from..token_to];
        if token.contains("http://") || token.contains("https://") {
            out.extend_from_slice(token.as_bytes());
        } else {
            out.extend_from_slice(paths_out_of_a_token(token).as_bytes());
        }
        at = token_to;
    }
    // Nothing was invented here: every byte is either one the input already held or one of the
    // ASCII marker's, and spans only ever end at byte positions a valid string can be cut at.
    String::from_utf8(out).expect("taking paths out of a valid string cannot make an invalid one")
}

/// The paths out of one token that is not a URL citation.
///
/// A path begins at a `/`, or at a `~` with a `/` later in the same run of path bytes — and only
/// where the byte before it is the token's start or a non-path token byte (`file:`, `?q=`),
/// because mid-run a slash is a join: `and/or` and `read/write` are prose, and `src/hub.rs` is
/// relative.
fn paths_out_of_a_token(token: &str) -> String {
    let b = token.as_bytes();
    let mut out = String::with_capacity(token.len());
    let mut at = 0;
    while at < b.len() {
        // A path starts at a `/`, or at a `~` with a `/` later in the same run of path bytes.
        // The forward scan runs only for a `~` at a run's start, so a long token costs one pass.
        let at_a_path_start = at == 0 || !is_path_byte(b[at - 1]);
        let is_a_path = at_a_path_start
            && match b[at] {
                b'/' => true,
                b'~' => {
                    let mut end = at;
                    while end < b.len() && is_path_byte(b[end]) && b[end] != b'/' {
                        end += 1;
                    }
                    b.get(end) == Some(&b'/')
                }
                _ => false,
            };
        if is_a_path {
            let mut end = at;
            while end < b.len() && is_path_byte(b[end]) {
                end += 1;
            }
            out.push_str(A_PATH);
            at = end;
        } else {
            out.push(b[at] as char);
            at += 1;
        }
    }
    out
}

/// One line of the ring. The field order is the envelope a reader parses, and it does not change:
/// `seq` first because it is the cursor, `frame` last because it is the payload.
#[derive(serde::Serialize)]
struct Line<'a> {
    seq: u64,
    ts: u64,
    dir: &'a str,
    conversation: &'a str,
    lane: &'a str,
    frame: &'a serde_json::Value,
}

/// The last sequence number a ring file holds, or why it does not answer.
///
/// Reads only COMPLETE lines — everything before the file's last newline — because a line is
/// written whole or not at all, and what follows the last newline is a write the process did not
/// finish. That torn tail is healed here: truncated away, so the next event lands on a line of
/// its own instead of gluing onto a fragment and making both unreadable for ever. A tail that
/// cannot be cut is a file this run cannot safely append to, and reads as [`Floor::Unreadable`].
enum Floor {
    At(u64),
    /// No such file. Not an error: that is every ring's first run.
    Gone,
    /// There, but holding no event. An empty active file falls back to the old one, which is the
    /// crash-between-rename-and-append shape — and so does a file holding nothing but a torn
    /// first line, which healing has just emptied.
    Empty,
    /// There, and refusing to say: not a ring at all, or one whose torn tail could not be cut.
    Unreadable(std::io::Error),
}

fn the_floor(path: &Path) -> Floor {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Floor::Gone,
        Err(e) => return Floor::Unreadable(e),
    };
    // The tail after the last newline is a write that never finished. It was never an event —
    // nobody read it, nothing answered for it — so it goes, and the file it leaves behind is one
    // a reader can take line by line and an append can build on.
    let whole = match raw.rfind('\n') {
        Some(last_newline) => &raw[..=last_newline],
        None => "",
    };
    if whole.len() < raw.len()
        && let Err(e) = cut_the_file_down_to(path, whole.len() as u64)
    {
        return Floor::Unreadable(e);
    }
    if whole.trim().is_empty() {
        return Floor::Empty;
    }
    // Last line that parses, not the last line: a complete line that is not an event is skipped
    // rather than trusted, and no floor is better than a wrong one.
    let mut floor = None;
    for line in whole.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(seq) = v.get("seq").and_then(serde_json::Value::as_u64)
        {
            floor = Some(seq);
        }
    }
    match floor {
        Some(seq) => Floor::At(seq),
        // Bytes are there and none of them is an event. Not a torn tail — that was healed above —
        // so this file was never a ring, and nothing may be assumed about what its numbers were.
        None => Floor::Unreadable(std::io::Error::other(
            "the file holds no line this ring ever wrote",
        )),
    }
}

/// Take the file down to its last complete line. The caller has already decided the bytes past
/// that point never happened; this is the act, not the decision.
fn cut_the_file_down_to(path: &Path, to: u64) -> std::io::Result<()> {
    let f = fs::OpenOptions::new().write(true).open(path)?;
    f.set_len(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn say(what: &str) -> BridgeFrame {
        BridgeFrame::Say {
            text: what.to_owned(),
            hint: None,
            file: None,
        }
    }

    #[test]
    fn a_torn_last_line_is_healed_not_inherited() {
        let dir = tempfile::tempdir().expect("tmp");
        let conversation = ProjectId::new("p-0123456789ab");
        let ring = Ring::in_dir(dir.path());
        ring.append(&conversation, None, &say("one"));
        ring.append(&conversation, None, &say("two"));

        // Death between the line and its newline: half a line on the disk, ending nowhere. The
        // next event would glue onto it and BOTH would be lost to every reader for ever.
        {
            use std::io::Write as _;
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(dir.path().join(RING))
                .expect("the ring");
            f.write_all(br#"{"seq":3,"ts":1"#)
                .expect("half a line, torn");
        }

        // A restart. The torn tail was never an event — nobody read it, nothing answered for it —
        // so the ring must come back whole: healed, and the next event on a fresh line of its own.
        let ring = Ring::in_dir(dir.path());
        ring.append(&conversation, None, &say("three"));

        let raw = fs::read_to_string(dir.path().join(RING)).expect("the healed ring");
        let lines: Vec<serde_json::Value> = raw
            .lines()
            .map(|l| serde_json::from_str(l).expect("every line of the healed ring is one event"))
            .collect();
        let seqs: Vec<u64> = lines
            .iter()
            .map(|l| l["seq"].as_u64().expect("a seq"))
            .collect();
        assert_eq!(
            seqs,
            vec![1, 2, 3],
            "the torn tail was inherited rather than healed:\n{raw}"
        );
        assert_eq!(
            lines[2]["frame"]["text"], "three",
            "the event after a torn tail was not the one that was asked for:\n{raw}"
        );
        assert_eq!(
            lines.len(),
            3,
            "the fragment of the write that never finished is still in the ring:\n{raw}"
        );
        assert!(raw.ends_with('\n'), "the healed ring ends mid-line:\n{raw}");
    }

    #[test]
    fn a_restarted_hub_resumes_a_rotated_ring_from_the_active_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let conversation = ProjectId::new("p-0123456789ab");
        let ring = Ring::in_dir(dir.path());
        // Big enough to cross the rotation bound, so a restart meets TWO files and must take its
        // floor from the one holding the newest events.
        let words = "w".repeat(crate::queue::MAX_TEXT * 3);
        let events = 400;
        for n in 1..=events {
            ring.append(&conversation, None, &say(&format!("{n} {words}")));
        }
        assert!(
            dir.path().join(RING_OLD).is_file(),
            "the run never crossed a rotation, so this test is not about a rotated ring"
        );
        let old_before = fs::read(dir.path().join(RING_OLD)).expect("the old file");

        let ring = Ring::in_dir(dir.path());
        ring.append(&conversation, None, &say("after the restart"));

        let old_after = fs::read(dir.path().join(RING_OLD)).expect("the old file, still there");
        assert_eq!(
            old_before, old_after,
            "a restart touched the file it was not resuming from"
        );
        let active = fs::read_to_string(dir.path().join(RING)).expect("the active file");
        let lines: Vec<serde_json::Value> = active
            .lines()
            .map(|l| serde_json::from_str(l).expect("one line, one event"))
            .collect();
        assert_eq!(
            lines.last().and_then(|l| l["frame"]["text"].as_str()),
            Some("after the restart"),
            "the post-restart event is not the last thing in the active file:\n{active}"
        );
        let first_seq = lines[0]["seq"].as_u64().expect("a seq");
        let last_seq = lines.last().unwrap()["seq"].as_u64().expect("a seq");
        assert_eq!(
            last_seq,
            events + 1,
            "the restarted ring did not resume at the top"
        );
        assert_eq!(
            lines
                .iter()
                .map(|l| l["seq"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            (first_seq..=events + 1).collect::<Vec<_>>(),
            "the active file's own sequence is not contiguous across the restart"
        );
    }

    #[test]
    fn a_ring_whose_history_cannot_be_read_stays_closed_rather_than_renumbering() {
        let dir = tempfile::tempdir().expect("tmp");
        // Bytes that are not a ring and never were: complete lines, none of them an event.
        let poison = "not a ring\nnot one either\n";
        fs::write(dir.path().join(RING), poison).expect("poison the history");

        let conversation = ProjectId::new("p-0123456789ab");
        let ring = Ring::in_dir(dir.path());
        ring.append(&conversation, None, &say("must not be written"));
        ring.append(&conversation, None, &say("nor this"));

        let after = fs::read_to_string(dir.path().join(RING)).expect("still readable");
        assert_eq!(
            after, poison,
            "a hub that could not read its ring's history wrote to it anyway — and a reader that \
             had joined before would now see two different events under one number, with nothing \
             able to tell them apart"
        );
    }

    #[test]
    fn the_door_speaks_strictly_contiguous_seq_past_any_cursor_a_reader_can_hold() {
        let dir = tempfile::tempdir().expect("tmp");
        let ring = Ring::in_dir(dir.path());
        let conversation = ProjectId::new("p-0123456789ab");

        // Words three times the clip ceiling, so every line is the biggest line the door writes:
        // the run crosses the rotation on the door's real terms, not a test's small ones.
        let words = "w".repeat(crate::queue::MAX_TEXT * 3);
        const EVENTS: u64 = 700;

        for n in 1..=EVENTS {
            ring.append(&conversation, None, &say(&format!("{n} {words}")));
        }

        // A reader takes the old file first, then the active one — that ordering is the contract
        // the file names encode.
        let mut seqs = Vec::new();
        for name in [RING_OLD, RING] {
            if let Ok(raw) = fs::read_to_string(dir.path().join(name)) {
                for line in raw.lines() {
                    let v: serde_json::Value = serde_json::from_str(line)
                        .unwrap_or_else(|e| panic!("{name} holds a line that is not one: {e}"));
                    seqs.push(
                        v["seq"]
                            .as_u64()
                            .unwrap_or_else(|| panic!("{name} holds a line with no seq: {line}")),
                    );
                }
            }
        }

        assert!(
            dir.path().join(RING_OLD).is_file(),
            "the run crossed a megabyte of events and the active file was never rotated"
        );
        let first = *seqs.first().expect("a ring with lines");
        let last = *seqs.last().expect("a ring with lines");
        assert_eq!(last, EVENTS, "events were lost from the tail of the ring");
        assert_eq!(
            seqs,
            (first..=EVENTS).collect::<Vec<_>>(),
            "the ring's sequence has a gap or a repeat in it"
        );

        // From any cursor a reader can still hold, what follows is exactly cursor+1, cursor+2, …
        for cursor in [first, first + 1, first + 137, EVENTS / 2, EVENTS - 1] {
            let after: Vec<u64> = seqs.iter().copied().filter(|s| *s > cursor).collect();
            assert_eq!(
                after,
                (cursor + 1..=EVENTS).collect::<Vec<_>>(),
                "a reader holding cursor {cursor} is not answered with the events after it"
            );
        }

        // And no line is bigger than the clip allows: the door's writes are bounded.
        for name in [RING_OLD, RING] {
            if let Ok(raw) = fs::read_to_string(dir.path().join(name)) {
                for line in raw.lines() {
                    let v: serde_json::Value = serde_json::from_str(line).expect("one line");
                    let text = v["frame"]["text"].as_str().expect("the words");
                    assert!(
                        text.chars().count() <= crate::queue::MAX_TEXT,
                        "a line slipped past the clip at {} characters",
                        text.chars().count()
                    );
                    assert!(
                        text.contains("(clipped)"),
                        "long words were not said to be clipped"
                    );
                }
            }
        }
    }
}

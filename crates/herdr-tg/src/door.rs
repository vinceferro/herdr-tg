//! The ring: one line per operator-visible event, for a reader that is not on this machine.
//!
//! The PWA is the operator's work surface now, and this hub is the conversation plane. The ring
//! is the seam between them: the hub appends what he would have seen — an agent's `say`, its
//! `ask`, its `done`, an `ask` stopping being open — and a later increment serves those lines to
//! the PWA over HTTP. Nothing consumes the ring yet; the hub writes it regardless, because the
//! events it misses before a reader exists are the events no reader will ever see.
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
//! cursor can always ask for "after this one" and be answered.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hub_proto::{BridgeFrame, LaneId, ProjectId};

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

/// Which way the event was travelling when the hub saw it. Up is an agent's own words; the
/// operator's echoes (down) are a later increment, and the field is here from the start so a
/// reader never has to learn a second envelope shape.
const UP: &str = "up";

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
        // The conversation becomes half of what a reader addresses an event by, so it must be the
        // shape the registry mints and nothing else: an id of another shape could be any string
        // the wire chose, and the ring's whole promise is that it names nothing of this machine.
        if !crate::conversations::is_conversation_id(conversation.as_str()) {
            tracing::warn!(
                conversation = %conversation,
                "a frame arrived for a conversation whose id is not a shape this hub minted; the \
                 ring records nothing about it"
            );
            return;
        }
        let Some(frame) = the_operator_visible(frame) else {
            return;
        };

        let mut where_ = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if where_.closed {
            return;
        }
        let line = Line {
            seq: where_.next_seq,
            ts: crate::hub::now_secs(),
            dir: UP,
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
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        self.bytes += line.len() as u64 + 1;
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

/// A byte that can sit inside a path or a word that might hold one. Everything else — a space, a
/// comma, a bracket — is an edge a path cannot cross.
fn is_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'~' | b'/')
}

/// Every absolute and home-relative path in the words, replaced by `[a path]`.
///
/// The shapes are the ones that name a place on THIS machine — `/`-rooted and `~`-rooted words,
/// taken out WHOLE — and not the relative paths of a repository (`src/hub.rs`), which name a
/// place in a conversation's own work and carry nothing of the operator's filesystem. A URL is
/// not a filesystem path either and is spared whole, because an agent citing documentation is not
/// an agent citing a disk. A word is a run of bytes a path can hold; everything else — a space, a
/// comma, a bracket — is an edge a path cannot cross, which is what keeps `/etc/x` inside
/// `see(/etc/x)` scrubbed while the sentence around it stands. Non-ASCII bytes are edges too:
/// this box's paths are ASCII, and a scanner that guessed further would be a scanner guessing.
fn no_paths(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;

    while at < bytes.len() {
        // Whitespace and everything a candidate cannot start at pass through untouched, byte for
        // byte — the string is only ever rebuilt where a path is taken out of it.
        if !is_path_byte(bytes[at]) {
            out.push(bytes[at]);
            at += 1;
            continue;
        }
        // A word begins here. Find where it ends, and treat the whole word at once: the URL
        // sparing is a fact about the word, not about any one slash in it.
        let word_from = at;
        let mut word_to = at;
        while word_to < bytes.len() && is_path_byte(bytes[word_to]) {
            word_to += 1;
        }
        let word = &text[word_from..word_to];
        if word.contains("://") {
            out.extend_from_slice(word.as_bytes());
        } else if is_a_machine_path(word) {
            out.extend_from_slice(A_PATH.as_bytes());
        } else {
            out.extend_from_slice(word.as_bytes());
        }
        at = word_to;
    }
    // Nothing was invented here: every byte is either one the input already held or one of the
    // ASCII marker's, and spans only ever end at byte positions a valid string can be cut at.
    String::from_utf8(out).expect("taking paths out of a valid string cannot make an invalid one")
}

/// Does this word name a place on this machine — and so may not leave it?
///
/// A word that BEGINS with `/` is an absolute path, whatever follows. A word that begins with `~`
/// and holds a slash is a home-relative one. Nothing else is: `src/hub.rs` names a place in a
/// repository rather than on a disk, `and/or` is prose, and a word that merely CONTAINS a slash
/// after other characters — `read/write` — cannot be told from either of those by shape, so it is
/// kept, which fails towards recording more of the agent's own words rather than less.
fn is_a_machine_path(word: &str) -> bool {
    word.starts_with('/') || (word.starts_with('~') && word.contains('/'))
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
enum Floor {
    At(u64),
    /// No such file. Not an error: that is every ring's first run.
    Gone,
    /// There, but holding no event. An empty active file falls back to the old one, which is the
    /// crash-between-rename-and-append shape.
    Empty,
    /// There, and refusing to say. The tail of a file whose process died mid-write is handled —
    /// the last line that PARSES is the floor — so this is a file that is not a ring at all.
    Unreadable(std::io::Error),
}

fn the_floor(path: &Path) -> Floor {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Floor::Gone,
        Err(e) => return Floor::Unreadable(e),
    };
    if raw.trim().is_empty() {
        return Floor::Empty;
    }
    // Last line that parses, not the last line: the one way a ring file legitimately ends
    // half-written is the one way this reader must tolerate.
    let mut floor = None;
    for line in raw.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(seq) = v.get("seq").and_then(serde_json::Value::as_u64)
        {
            floor = Some(seq);
        }
    }
    match floor {
        Some(seq) => Floor::At(seq),
        // Bytes are there and none of them is an event. Not a torn tail — a torn tail leaves the
        // whole lines above it intact — so this file was never a ring, and nothing may be
        // assumed about what its numbers were.
        None => Floor::Unreadable(std::io::Error::other(
            "the file holds no line this ring ever wrote",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ring.append(
                &conversation,
                None,
                &BridgeFrame::Say {
                    text: format!("{n} {words}"),
                    hint: None,
                    file: None,
                },
            );
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

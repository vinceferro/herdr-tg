//! What a bridge says, and everything that decides what it may do.
//!
//! # Authority flows one way
//!
//! A bridge speaks; the hub decides. No frame a bridge sends carries addressing — no chat, no
//! topic, no project name — because the hub already knows which connection is which project. It
//! learned that by resolving a **secret**, not by reading a field, which is why a bridge cannot
//! claim another project's topic no matter what it puts on the wire.
//!
//! # Four gates, all fail-closed, in this order
//!
//! 1. **Who is on the other end.** The transport hands the hub an identity and the hub asks it two
//!    questions: is this peer this user, and what does the single-claim rule fence on. A peer that
//!    is not this user is closed without a reply — an answer, even a refusal, is information. HOW
//!    the transport knows is not decided here (`transport.rs`), and that is what lets every gate
//!    below be tested against a peer this process could not have been.
//! 2. **Which project.** The secret resolves to one, in constant time, over the whole registry.
//!    The `project_id` on the wire is not consulted.
//! 3. **Switched on.** Enrolled and disabled is a real state.
//! 4. **A lane, if the bridge named one, must be a name the hub can address.** Empty, over-long, or
//!    carrying a control character is refused here — before a claim, before a topic, before a line
//!    of audit — because the audit is one tab-separated record per line and a lane with a newline in
//!    it would write records of its own choosing into the one file an incident is read from.
//! 5. **Exactly one live connection per CONVERSATION.** A second is refused, never a takeover. A
//!    takeover is what bridge-murder felt like from the inside: the incumbent kept running and
//!    quietly stopped being heard. If the incumbent's pid is gone from `/proc` it is evicted
//!    instead — a crashed worker must not lock its own project out until someone finds a keyboard.
//!
//! # A conversation is a project, or one worktree of it
//!
//! kickoff runs several worktrees of one repo at once, each its own agent process, and gate 5 keyed
//! on the project admitted exactly one of them. So `hello` carries an optional `lane` and the claim
//! key is [`Addr`] — the project plus that lane.
//!
//! **The secret still proves only the PROJECT.** The address is built from the project the secret
//! resolved to plus the lane the bridge named, never from anything else on the wire, so a bridge
//! naming a lane can only ever reach a lane of the project it has already proved it is. A lane and
//! its project are one repository and one trust domain; a lane is an address, not a credential.
//!
//! Every key that used to say "this project" now has to be asked which it means, and getting one
//! wrong is silent: an answer delivered into an agent that never asked, or a question retired on the
//! phone while the agent behind it waits for ever. The two filters in [`AskLedger`], the claims map,
//! the topic lookup and the tap's route all say **conversation**. The per-chat delivery budget, the
//! registry, the secret and enrolment all still say **project**, because a lane is not enrolled and
//! Telegram's ceiling is per chat rather than per topic.
//!
//! # Live is not the same as connected
//!
//! A channel plugin that is not allowlisted boots and exits in about a tenth of a second. From the
//! process table that is indistinguishable from a healthy worker, and the hub would happily create
//! a topic and greet a bridge that had already gone. So a project becomes **live** only after
//! `hello`, a settling window, and one answered `ping` — and the topic is created at that moment,
//! not before. An invisible topic full of nothing is the same as no topic to the person looking
//! for it.
//!
//! # Nothing is sent without a record of it first
//!
//! [`HubAudit`] writes `sent` before a send and the outcome after, so a dangling `sent` means the
//! process died mid-write and nothing else. It is the hub's own file: the `audit.rs` it was once
//! modelled on had a pane and a keystroke for its subject, and went with the scraper.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use hub_proto::frame::{Control, IntentStatus, Op, control_for};
use hub_proto::ids::{IdempotencyKey, IntentId, SpecId};
use hub_proto::{
    AckStatus, AckWhy, AskId, AskOption, BridgeFrame, Delivered, Envelope, FrameId, HubFrame,
    LaneId, Limits, MsgId, OptionId, ProjectId, RefusedReason, VERSION,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};

pub(crate) use self::intent::{
    AlreadyCarried, Heard, INTENT_VALID_FOR, INTENTS_FILE, Intended, Intent, IntentLedger,
    IntentRefusal, IntentState, Wanted, changes_a_running_thing, mint_idempotency_key,
};
// The two the hub itself no longer names: the bound belongs to the ledger and the phase is set
// where the state is. The tests below read both, and an import a build without them would call
// unused is an import that build refuses.
#[cfg(test)]
pub(crate) use self::intent::{INTENTS_KEPT, Phase};
use crate::registry::Registry;
use crate::transport::{Accepted, ConnectionIdentity, fence_is_alive};

/// How long after `hello` the hub waits for a `pong` before calling a project live.
pub const DEFAULT_SETTLE: Duration = Duration::from_secs(5);

/// How much a bridge may say between `welcome` and its pong before the hub stops holding it.
///
/// Exactly what a conforming bridge can be carrying when it dials, and that is one more than its
/// queue. `hub-link.ts` queues at most sixty-four frames, each at most
/// [`hub_proto::MAX_FRAME_BYTES`] — and the frame the kernel had taken only part of when the last
/// connection ended is put back at the HEAD of that queue on close, outside the bound `send`
/// keeps. The queue is full precisely when a socket has stopped taking bytes, which is when a
/// half-written frame is the ordinary state, so a bridge that outlived a hub that wedged and was
/// restarted redials with sixty-five. A hold of exactly sixty-four refused the whole of that legal
/// backlog on the sixty-fifth frame. So: the queue, plus the one put back, and only a bridge
/// breaking its own rules ever trips this.
///
/// The byte bound used to be 256 KiB — sixteen times less than the bridge is allowed to hold — so
/// a bridge that outlived a hub restart with an ordinary day's backlog hit it on every redial.
/// Measured with the real bridge: sixty-four messages destroyed across three refused connections,
/// not one of them acked, nothing on the phone.
pub const PRE_PONG_FRAMES: usize = 64 + 1;
pub const PRE_PONG_BYTES: usize = PRE_PONG_FRAMES * hub_proto::MAX_FRAME_BYTES;

/// How long a connection that is being refused is given to take its acks before it is closed
/// on it. A bridge reads these in a millisecond; a peer that has stopped reading is the one this
/// bounds, so that a refusal can never hold a task open for ever.
pub const GOODBYE_SHELF_LIFE: Duration = Duration::from_secs(2);

/// How long a line of an agent's prose is worth holding before it is given up on.
///
/// **This used to be ten seconds for everything, and ten seconds is a queue position rather than a
/// judgement.** A turn costs the one-second rhythm, so ten waiters ahead of you is ten seconds:
/// below eleven live connections nothing could ever be given up on, and at eleven it began — and
/// since a worktree became a connection of its own, eleven is an ordinary dispatch morning for one
/// repo. A message was thrown away for its POSITION, with the per-minute ceiling untouched, and the
/// agent was told it had been sent too fast. That was not true and it was not useful.
///
/// Ninety seconds instead, and the number comes from what it has to survive: Telegram's own flood
/// wait is a sixty-second window (`docs/RATE-PROBE.md`), so prose caught behind one goes out on the
/// far side of it rather than being lost to a limit nobody here caused, with room for a queue
/// behind that.
///
/// It is still bounded, and what it is bounded for is no longer LIVENESS. A frame used to be
/// handled inside its connection's own read loop, so a bridge waiting here was a bridge whose next
/// frame — and whose `bye`, and whose end-of-file — went unread until it finished, and the claim it
/// held is what refuses the session when it comes back. That made "how long is this worth holding"
/// and "is this bridge still there" the same question at an order of magnitude's distance, which
/// they never were: `serve_connection` hands frames to a task of its own now, so the socket is read
/// while one is in flight and a bridge that goes away is let go of at once.
///
/// What remains is the honest reason for a bound at all: a message nobody can send is eventually a
/// message not worth sending, and a queue of them is a queue of turns some other project could have
/// had. Ninety seconds is one flood wait plus room to queue behind it. The agent is never blocked
/// on it either way — the bridge reports a frame as away the moment it is written to the socket, and
/// corrects itself later from the ack.
pub const PROSE_SHELF_LIFE: Duration = Duration::from_secs(90);

/// The longest the hub will spend fetching ONE of his files before it gives up.
///
/// The fetch runs inside the update handler, which the client library serialises per conversation,
/// so what it costs is what his next line in that topic waits — and unbounded it was the HTTP
/// client's own default twice over, once for `getFile` and once for the body, against a Telegram
/// that had simply stopped answering. A minute carries the 20 MB ceiling at about 2.7 Mbit/s,
/// which is slower than any link that could have sent the screenshot in the first place; past it
/// the honest reading is not "slow" but "not coming", and he is asked to send it again.
pub const FETCH_DEADLINE: Duration = Duration::from_secs(60);

/// How long the topic opened at connection time will queue for, and it is deliberately the shortest
/// of the three.
///
/// The one send in this file that can afford to be impatient, because it has a designed fallback:
/// if the topic cannot be made now, the bridge's first actual message makes it instead — greeting
/// and all — and nothing is lost but a little visibility in the meantime. What it CANNOT afford is
/// to wait long, because it runs BEFORE that connection's read loop starts, so every second here is
/// a second the bridge's first frame goes unread and its `bye` goes unseen.
///
/// Ten seconds, which is deliberately the number the old flat deadline used. That constant's stated
/// reason was exactly this one — bounding how long a read loop may be blocked — and this is the one
/// place in the file where the reason genuinely applies, because it is the one send that runs
/// outside the loop it would block.
pub const GREETING_SHELF_LIFE: Duration = Duration::from_secs(10);

/// How long a QUESTION is worth holding. Much less, and it is not a smaller version of the same
/// judgement — it is the opposite one.
///
/// A line of prose that arrives late is still worth reading. A question that arrives late is a
/// keyboard for a decision that has moved on: the operator taps it, and the answer lands in an
/// agent that gave up on the question long ago, or in no agent at all. The wire already refuses to
/// retry an ask it could not confirm for exactly this reason — two live menus for one question is
/// worse than none — and holding one for a minute and a half manufactures the same thing more
/// slowly.
///
/// Twenty seconds is about the longest a queue can honestly be worth waiting in for something whose
/// value is that it is CURRENT, and it is comfortably longer than any wait the per-minute bucket can
/// name on its own. So a question is only ever given up on when the chat is genuinely shut — by a
/// flood wait, or by a herd deep enough that its turn is most of a minute away — and never merely
/// for being eleventh.
pub const QUESTION_SHELF_LIFE: Duration = Duration::from_secs(20);

/// How long Telegram lets a bot edit one of its own messages. Not ours to raise either.
///
/// It is the shelf life of an ask record: past it, the edit that takes a keyboard off is refused,
/// so the record can no longer do the only thing it is kept for.
pub const EDIT_WINDOW_SECS: u64 = 48 * 60 * 60;

/// The most of a retirement's note that is worth keeping and worth showing.
///
/// The note can be an adapter's own free text — the `outcome` it sends with `ask_resolved` — which
/// the wire bounds only at a whole frame, 64 KiB. Two costs, and neither is chosen by anyone: the
/// note is stored with the record in a file that is rewritten whole on every ask and every tap of
/// every project on this box, and it is written onto the retired message, where the room it takes
/// is taken off the QUESTION — past roughly this much, the operator watches his question get eaten
/// to make space for a paragraph explaining why it went away.
pub const RETIREMENT_NOTE_ROOM: usize = 500;

/// How long a refused topic creation is remembered before Telegram is asked again.
///
/// Long enough that an agent talking steadily costs one call rather than one per message, short
/// enough that a flood wait or a 5xx mends itself inside a turn or two. Telegram's own flood waits
/// on this call are seconds to half a minute.
pub const TOPIC_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Telegram's own ceiling on a button's `callback_data`. Not ours to raise.
///
/// The `h|` prefix comes out of this budget, which is why the check is `len + 2`.
pub const CALLBACK_DATA_MAX: usize = 64;

/// The longest caption Telegram puts under a picture or a document. Not ours to raise.
///
/// *"0-1024 characters after entities parsing"* — the Bot API page, on `sendPhoto` and
/// `sendDocument`. Words that fit go as the file's caption, one send; longer words are their own
/// message and the file follows under it, two sends and two tokens.
/// Telegram's caption ceiling, in UTF-16 code units — which is how the API counts it, and not
/// how `str::chars` does. Unlike `MAX_TEXT`, which sits well under its limit, this is the limit
/// itself, so the unit of measurement is load-bearing.
pub const CAPTION_MAX: usize = 1024;

/// What the hub will accept from one connection.
pub const LIMITS: Limits = Limits {
    max_frame: hub_proto::MAX_FRAME_BYTES,
    max_text: 3500,
    frames_per_min: 60,
};

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The surface the hub speaks to the operator through.

/// What a frame is, for the one purpose of deciding how long it is worth waiting to send.
///
/// Two, because a queue that only ever holds is its own lie: holding everything for as long as
/// prose deserves would put a keyboard in front of the operator for a decision the agent behind it
/// abandoned a minute ago. See [`PROSE_SHELF_LIFE`] and [`QUESTION_SHELF_LIFE`] for the judgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perishable {
    /// Something an agent said. Late is fine; missing is not.
    Prose,
    /// Something the operator is expected to answer. Late is WORSE than missing.
    Question,
}

impl Perishable {
    /// The kind a message's own shape already tells you.
    ///
    /// Buttons are the evidence, and the one caller that knows better — an `ask` whose agent minted
    /// no options, so the answer comes back as typed words rather than a tap — says so explicitly
    /// rather than being guessed at from here.
    pub fn of(buttons: &[AskOption]) -> Self {
        if buttons.is_empty() {
            Self::Prose
        } else {
            Self::Question
        }
    }

    fn shelf_life(self) -> Duration {
        match self {
            Self::Prose => PROSE_SHELF_LIFE,
            Self::Question => QUESTION_SHELF_LIFE,
        }
    }
}

/// Why a Telegram write did not happen, in the two facts the hub acts on.
///
/// The seconds are a VALUE rather than part of the sentence, because they have somewhere to go: a
/// flood wait belongs to the whole CHAT, not to the call that discovered it, so the budget has to
/// hear it or the very next send walks into the same wall. Before this they survived only as
/// characters inside an audit line's `why=` field, which nothing reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    /// What Telegram said, for the audit and the journal.
    pub why: String,
    /// How long it said to wait before anything else is sent to this chat, when it said so.
    pub flood_wait: Option<Duration>,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.why)
    }
}

/// What became of one attempt to put a message in front of the operator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendOutcome {
    /// It landed, and this is its message id.
    Sent(MsgId),
    /// It landed, but it was shortened first. The bridge is told, because the bridge is the only
    /// party that can decide to say less next time.
    Clamped(MsgId),
    /// Somebody's rate limit refused it, and this is how long to wait.
    ///
    /// **Either budget can say this, and the caller does not have to care which.** Ours refuses
    /// before the send, because sending anyway is how a bot earns a 429 and a 429 on a shared bot
    /// punishes every project rather than the one that caused it. Telegram's refuses after it, and
    /// that one used to fall through to [`Self::Refused`] — a permanent-sounding answer about
    /// something that mends itself inside a minute, with the seconds it came with thrown away.
    ///
    /// What separates them is WHERE it came from, and the one place that needs to know is
    /// `send_into`: a `TooFast` handed back by the surface is Telegram's word and drains the chat's
    /// budget, because ours had already said yes.
    TooFast(Duration),
    /// The topic is gone. Telegram never says so with a service message and offers no way to list
    /// topics, so this is the only way the hub finds out. Handled as a rebinding, exactly once —
    /// never retried as if it were a transient, which would swallow the project's messages.
    TopicGone,
    /// Telegram refused it, and said why.
    Refused(String),
    /// It went out and could not be checked. Never retried when the message carried buttons.
    Unseen,
    /// Its project was switched off at the terminal while it waited for its turn. Nothing was
    /// spent on it and nothing landed; the bridge is told with no reason, the same answer every
    /// frame queued behind it gets, because the closed set of reasons has none for this and the
    /// `refused{not_enabled}` that precedes these acks already says why.
    SwitchedOff,
}

/// Where one of HIS messages has got to, marked on the message itself.
///
/// The hub already knows three stages of a line he typed: it handed the words to the bridge, the
/// bridge said the agent has them, or the bridge said it could not hand them on. A reaction on his
/// own message says which, without spending a send and without adding a line — measured free
/// against the send ceiling on 5 September (`docs/RATE-PROBE.md` §3). ONE reaction per message,
/// replaced as the stage advances, never stacked; a refusal still gets the line in the topic too,
/// because a reaction carries no reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// The hub took his words and handed them to the bridge. Nothing has answered yet.
    HandedOn,
    /// The bridge said the words reached the agent's turn.
    Accepted,
    /// The bridge said it could not hand them on. The line under his message says why.
    Refused,
}

/// Telegram, behind a trait, so the hub's decisions can be tested without one.
pub trait Surface: Send + Sync + 'static {
    /// Create the project's topic and return its id.
    ///
    /// The failure carries [`Refused::flood_wait`] rather than only a sentence, because this is a
    /// metered write to the same chat as every other: a `429` here means the chat is shut for
    /// everybody, and it used to be swallowed into a string that only the audit log ever saw.
    fn create_topic(
        &self,
        title: &str,
        icon_color: u8,
    ) -> impl std::future::Future<Output = Result<i32, Refused>> + Send;

    /// Put a message in a topic, with buttons if there are any.
    ///
    /// `reply_to` threads it under one of the OPERATOR's messages, when it is about one. The line
    /// saying his typed words reached nobody used to be a bare post in the topic, and with two
    /// lines typed a second apart and one of them refused, it named neither. It must still go out
    /// when the message it points at is gone — he may have deleted it — as a bare post rather
    /// than not at all.
    fn send(
        &self,
        topic_id: i32,
        text: &str,
        buttons: &[AskOption],
        reply_to: Option<&MsgId>,
    ) -> impl std::future::Future<Output = SendOutcome> + Send;

    /// Say one line in the forum itself, outside every project's topic.
    ///
    /// For the one thing the hub has to say that is not any project's: that the chat as a whole is
    /// carrying more than it will take. Putting that in whichever topic happened to be shed first
    /// would read as that project's problem, and it would land in a conversation an agent is
    /// reading back.
    fn say_in_general(&self, text: &str) -> impl std::future::Future<Output = SendOutcome> + Send;

    /// Rewrite one of this bot's own messages in place, keeping its message id.
    ///
    /// **Measured free.** Thirty edits straight after five sends, none refused, and a send still
    /// went through afterwards (`docs/RATE-PROBE.md`). That is the whole reason the hub can keep a
    /// running count in front of the operator while it is over the ceiling: the first line is a
    /// send it has to pay for, and every update after it costs nothing.
    fn rewrite(
        &self,
        msg_id: &MsgId,
        text: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;

    /// Strip a stale keyboard AND say what happened to the question.
    ///
    /// No design that read a rendered screen could ever do this: a screen cannot tell you that a
    /// question stopped being asked. The original text comes back in because the message has to
    /// keep saying what was asked — a body replaced by a bare note reads as the bot having lost the
    /// question, which is the opposite of the reassurance this is for.
    fn retire_buttons(
        &self,
        topic_id: i32,
        msg_id: &MsgId,
        original: &str,
        note: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;

    /// Put one reaction on one of HIS messages, replacing whatever was there.
    ///
    /// **Not charged against the send ceiling, and never retried.** Reactions were measured on
    /// 5 September (`docs/RATE-PROBE.md` §3): twenty of them and a send still went through, so
    /// they come out of no send budget. They have a ceiling of their own — twenty a minute — which
    /// the hub keeps to before calling this, and a mark that ceiling or Telegram refuses simply
    /// does not appear: the line in the topic carries the meaning, and nothing an agent is waiting
    /// on is behind a reaction. A refusal is logged and forgotten by the caller — but it carries
    /// [`Refused::flood_wait`], because the caller says a 429 quietly and anything else out loud.
    fn mark(
        &self,
        chat_id: i64,
        msg_id: &MsgId,
        mark: Mark,
    ) -> impl std::future::Future<Output = Result<(), Refused>> + Send;

    /// Ask Telegram where one of HIS files is and how big it is — `getFile`.
    ///
    /// Two calls rather than one, so the hub owns every decision between them: the size check
    /// against the answer, the path it mints, the mode it opens with, and the ceiling on the
    /// stream. A surface that fetched in one go would be making those on the hub's behalf, out of
    /// sight of the tests that pin them.
    fn locate(
        &self,
        file_id: &str,
    ) -> impl std::future::Future<Output = Result<Located, Refused>> + Send;

    /// Stream one file's bytes into `into`. `file_path` is the one [`Self::locate`] answered with.
    ///
    /// The destination is the hub's: opened by the hub, `0600`, at a path the hub minted, and
    /// counting — so the bytes past Telegram's ceiling are refused by the writer, whatever size
    /// was reported. Nothing about where the bytes go is the surface's to decide.
    fn download(
        &self,
        file_path: &str,
        into: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
    ) -> impl std::future::Future<Output = Result<(), Refused>> + Send;

    /// Put one of an AGENT's files in a topic, with the words as its caption — `sendPhoto` when
    /// [`Upload::as_photo`], `sendDocument` otherwise.
    ///
    /// The bytes are the hub's, read off a descriptor it opened and checked; this surface never
    /// takes a path. The upload library's path-taking constructor follows links and names the
    /// upload after the last path segment, and both are exactly what the outbox rules forbid.
    /// A refusal comes back through the same classification as a text send, so a flood wait
    /// drains the chat and a deleted topic rebinds: a file is a send like any other.
    fn send_file(
        &self,
        topic_id: i32,
        file: &Upload,
        caption: &str,
    ) -> impl std::future::Future<Output = SendOutcome> + Send;
}

/// A file on its way to his phone, as the hub read it off the descriptor it checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upload {
    /// The bytes, whole. Read into memory rather than streamed so that a flood wait can send the
    /// same bytes again a moment later; fifty megabytes at the very most, and once at a time.
    pub bytes: Vec<u8>,
    /// What he sees a document called: the adapter's `filename`, or its `name`, with nothing in
    /// it a phone cannot show.
    pub filename: String,
    pub mime: Option<String>,
    /// `sendPhoto` rather than `sendDocument`: a jpeg, png or webp under the picture ceiling,
    /// unless the agent said `document`.
    pub as_photo: bool,
}

/// What `getFile` answers, in the shape the Bot API returns it.
///
/// The field names are the API's own — `file_id`, `file_unique_id`, `file_size`, `file_path` —
/// so the fake surface in the tests is built from a literal in that shape rather than from a
/// struct this crate invented. `file_size` is optional on the wire and stays optional here: the
/// client library reads an absent one as four gigabytes, which is not a size anyone should act on.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Located {
    pub file_id: String,
    pub file_unique_id: String,
    #[serde(default)]
    pub file_size: Option<u64>,
    pub file_path: String,
}

/// One file he sent, as the message described it and before anything was fetched.
///
/// Everything here except `kind` and `file_id` is what a phone said about itself: `size` is an
/// optional claim the hub checks again on the stream, `mime` is a declaration, and `filename` is
/// a string somebody chose that is carried as DATA and is never a segment of any path.
#[derive(Clone, Debug, PartialEq)]
pub struct SentFile {
    pub kind: hub_proto::FileKind,
    pub file_id: String,
    pub size: Option<u64>,
    pub mime: Option<String>,
    pub filename: Option<String>,
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The tap ledger.

/// What was written down beside a message that carried buttons.
///
/// A tap is resolved against this, never against a button's position. Position is how a button
/// reading "Reject" once confirmed "Allow always".
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AskRecord {
    pub project: ProjectId,
    /// Which worktree of that project asked, when it was not the project itself.
    ///
    /// Half the address, and it has to be written down rather than re-derived: two lanes of one
    /// repo are two agents that both mint their first ask id from a counter starting at the same
    /// number, and the opencode adapter gives both of them ONE instance because it is one process.
    /// So neither the ask id nor the instance can tell them apart, and an outcome matched without
    /// this field lands on a question a different lane is still waiting on.
    ///
    /// `#[serde(default)]` because the ledger on the operator's box holds records written before
    /// lanes existed, and a ledger that will not parse turns every keyboard on his phone into a
    /// button that answers "I have no record of that question".
    #[serde(default)]
    pub lane: Option<LaneId>,
    pub ask_id: AskId,
    pub topic_id: i32,
    /// The options exactly as the bridge minted them, labels included, so the hub can say what was
    /// chosen in the words the operator actually read.
    pub options: Vec<AskOption>,
    /// What the question said. Kept so that retiring the keyboard can leave the question visible
    /// with its outcome beside it, rather than replacing it with a bare note.
    #[serde(default)]
    pub text: String,
    /// Which run of the worker asked. A tap on a menu drawn for a session that has since restarted
    /// is refused with a reason, rather than answered into a process that never asked.
    pub instance: String,
    /// The process that asked, so "that agent is gone" can be a fact rather than a guess.
    ///
    /// A lane is never dispatched twice under the same name, so nothing of its own ever comes back
    /// to clear what it left open — and clearing it from a sibling's arrival needs proof, because
    /// the two states it has to tell apart look identical in the ledger: a worktree that ENDED, and
    /// one whose socket dropped for a second and is coming straight back. A bridge keeps its pid
    /// across a reconnect, so the pid separates them and nothing else here does.
    ///
    /// `None` means "written by a build that did not record it", and it is read as unknown rather
    /// than as gone: a sweep that guesses takes a live agent's question off the phone, which is the
    /// failure this whole field exists to avoid.
    #[serde(default)]
    pub pid: Option<u32>,
    /// When it was written, in seconds since the epoch.
    ///
    /// Not for display. Telegram refuses an edit on a message older than about 48 hours, so past
    /// that age a record can no longer do the one job it has — being the handle that takes a
    /// keyboard off — and keeping it only grows a file that is rewritten whole on every ask.
    ///
    /// `0` means a build that did not record it wrote this, and it is read as an UNKNOWN age that
    /// is never dropped — never as "older than anything". Nothing is lost by that: lanes have never
    /// shipped, so every record already on the operator's box belongs to a project's own voice, and
    /// those are collected the ordinary way when that project's next session arrives.
    #[serde(default)]
    pub at: u64,
    /// What was already answered, if anything.
    ///
    /// The record's presence used to BE the authorisation, which was fine only while the record was
    /// deleted the instant a tap landed. Once a failed retirement started keeping the record — so
    /// the keyboard could be retired later — presence stopped meaning "unanswered", and the still
    /// live keyboard on the operator's phone became re-tappable: a second tap delivered a second,
    /// contradicting `Choice` into an agent that had already been answered. Worse the other way
    /// round, an agent that answered at its own terminal could still be sent a phone tap.
    ///
    /// So authorisation is this field, and the record's presence is only retirement bookkeeping.
    #[serde(default)]
    pub answered: Option<OptionId>,
    /// That the question has stopped being asked, and the note its keyboard is retired with.
    ///
    /// The other half of `answered`. A question can stop being asked from two sides — a tap on
    /// the phone, or the agent answering, withdrawing or timing it out at its own terminal and
    /// saying so with `ask_resolved` — and each side has to be written down before anything is
    /// done about it, or the other side lands in the gap. The phone's side always was. The
    /// terminal's side used to be an edit followed by forgetting the record, with nothing marked
    /// in between: a tap during that edit — a Telegram round trip on a menu he is looking at — or
    /// after the edit had failed, resolved and delivered into an agent that had already answered.
    ///
    /// Two fields and not one enum, because they answer different questions: `answered` is WHAT
    /// the phone sent, `closed` is that the question is over and how it must be signed off.
    /// Together they are one predicate, [`Self::refusal_if_closed`], which is the only thing a
    /// tap is judged against.
    ///
    /// It is also written by the phone's own side, and only there — when the retirement that
    /// follows a tap is refused by Telegram. That record still says `answered`, which every sweep
    /// deliberately leaves alone, so without this mark nothing in the hub was left looking at a
    /// menu that is provably still live: it sat on his phone refusing every tap until the ledger
    /// dropped it two days later.
    ///
    /// `#[serde(default)]` because the ledger on the operator's box holds records from before
    /// this existed, and those are open questions or phone answers exactly as they were.
    #[serde(default)]
    pub closed: Option<Closed>,
}

/// That a question is over, and the note its keyboard is retired with.
///
/// The note is kept with the record rather than re-derived, because the retirement that writes
/// it may not be the one that heard `ask_resolved`: an edit Telegram refused is tried again when
/// the next session arrives, and that sweep only knows the question is not open. Without the
/// note it wrote "the session that asked this restarted" over a question the agent had answered.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Closed {
    pub how: hub_proto::AskEnd,
    pub note: String,
}

impl AskRecord {
    /// Why a tap on this question is refused, if it is — the one predicate both checks in
    /// `resolve_tap` use, so the unlocked look and the locked one cannot disagree.
    ///
    /// An answer from either side reads as "already answered": the operator does not care which
    /// side it was, and "at the terminal" is a place he cannot see. A question that was withdrawn
    /// or timed out was never answered, and saying it was would be false; what is true is that
    /// nobody is asking any more.
    pub fn refusal_if_closed(&self) -> Option<TapRefusal> {
        if self.answered.is_some() {
            return Some(TapRefusal::AlreadyAnswered);
        }
        match self.closed.as_ref().map(|c| c.how) {
            None => None,
            Some(hub_proto::AskEnd::Answered) => Some(TapRefusal::AlreadyAnswered),
            Some(hub_proto::AskEnd::Withdrawn | hub_proto::AskEnd::Timeout) => {
                Some(TapRefusal::NoLongerAsked)
            }
        }
    }

    /// Is this a keyboard somebody still has to take off, and does the record know what to say?
    ///
    /// "It was never answered" was the whole test, in every sweep and in the terminal's own
    /// retirement, and it was right while a phone answer meant the keyboard was already gone. It
    /// stopped being right the moment a refused edit started keeping the record: an answered
    /// record whose retirement Telegram refused is exactly the menu that most needs taking off,
    /// and every one of those walked past it. What separates it from a tap whose retirement is
    /// still in flight — which is not anybody else's to touch — is `closed`, which is written
    /// only once something knows the question is over AND knows the words to sign it off with.
    fn needs_retiring(&self) -> bool {
        self.answered.is_none() || self.closed.is_some()
    }

    /// Who asked this: the project speaking for itself, or one lane of it.
    pub fn addr(&self) -> Addr {
        Addr {
            project: self.project.clone(),
            lane: self.lane.clone(),
        }
    }

    /// Was it this exact conversation? Both halves, always — a project and a lane of it are two
    /// different agents, and matching on the project alone is the whole of the defect above.
    fn addr_is(&self, addr: &Addr) -> bool {
        self.project == addr.project && self.lane == addr.lane
    }
}

/// Why a conversation has no topic to send into.
///
/// Two values and not one, because the ack the agent gets is different and the two sentences it
/// renders are not interchangeable: "the chat is over its limit for this minute" is a thing that
/// mends itself, and "Telegram would not make the topic" is not. Collapsed into one, a budget shed
/// reached the agent as a Telegram refusal, which is a small untruth in the one place this system
/// exists to keep honest.
#[derive(Debug)]
pub enum NoTopic {
    /// The chat's budget could not pay for the topic. Try again in the duration named.
    TooFast(Duration),
    /// Telegram, or the registry, would not give this conversation a topic.
    Failed(String),
    /// The project was switched off while the topic waited for its turn.
    SwitchedOff,
}

impl std::fmt::Display for NoTopic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooFast(d) => write!(f, "the chat is at its limit for now; {}s", d.as_secs()),
            Self::Failed(why) => write!(f, "{why}"),
            Self::SwitchedOff => write!(f, "the project was switched off"),
        }
    }
}

/// The off switch, as seen from inside one connection's frames.
///
/// Thrown by the connection when its project is switched off at the terminal, and read in two
/// places: the handler, before it starts on each queued frame, and [`take_a_turn`] inside the one
/// frame the handler was already in. That second reader is why this exists. The switch used to
/// finish the frame in flight, and the frame in flight on the loud project — the project the
/// switch is for — is one holding the send permit while it sleeps out a spent minute: the socket
/// stayed open, the claim was gone but the goodbye had not been said, and one more line landed
/// in the topic up to ninety seconds after the operator had typed `disable`. A frame that is still
/// WAITING has spent nothing, so it is refused like everything queued behind it; one already
/// inside the send is finished, because cancelling a Telegram call mid-flight is how a message
/// lands with an ack saying it did not.
///
/// Carried into the frame as a task-local rather than as a parameter, because the wait is six
/// calls under `handle` and every caller of `say` between here and there — the bot's own sends,
/// the tests — has no switch to pass.
///
/// [`take_a_turn`]: Hub::take_a_turn
#[derive(Default)]
pub(crate) struct ConnectionSwitch {
    off: AtomicBool,
    thrown: tokio::sync::Notify,
}

impl ConnectionSwitch {
    /// Off from now on, and every wait inside this connection's current frame woken to see it.
    fn throw(&self) {
        self.off.store(true, Ordering::Release);
        self.thrown.notify_waiters();
    }

    fn is_off(&self) -> bool {
        self.off.load(Ordering::Acquire)
    }
}

tokio::task_local! {
    /// The switch of the connection whose frame is being handled, if a connection's frame is.
    static SWITCH: Arc<ConnectionSwitch>;
}

/// Wait on `what`, unless the switch is thrown first — `None` when it was.
///
/// The wake-up is registered BEFORE the flag is read: `notify_waiters` wakes only what was already
/// waiting, so reading first and registering second would miss a throw that landed between the
/// two, and the frame would sleep out its minute as if nothing had happened.
async fn unless_switched_off<T>(
    switch: Option<&ConnectionSwitch>,
    what: impl std::future::Future<Output = T>,
) -> Option<T> {
    let Some(switch) = switch else {
        return Some(what.await);
    };
    let thrown = switch.thrown.notified();
    tokio::pin!(thrown);
    thrown.as_mut().enable();
    if switch.is_off() {
        return None;
    }
    tokio::pin!(what);
    tokio::select! {
        t = &mut what => Some(t),
        _ = &mut thrown => {
            if switch.is_off() {
                None
            } else {
                Some(what.await)
            }
        }
    }
}

/// Put one reaction on one of his messages, or not — see [`Hub::mark_his_message`] for the rules.
///
/// A free function rather than a method because the eyes land in a task of their own, after
/// `relay` has returned, and a task cannot borrow the hub.
async fn mark_his_message<S: Surface>(
    surface: &S,
    reactions: &Mutex<crate::queue::ReactionBudget>,
    refusal_said: &AtomicBool,
    chat_id: i64,
    msg_id: &MsgId,
    mark: Mark,
) {
    if !reactions.lock().await.take(std::time::Instant::now()) {
        tracing::debug!(
            message = %msg_id, ?mark,
            "over the reactions' own ceiling this minute; the mark does not appear"
        );
        return;
    }
    let Err(refused) = surface.mark(chat_id, msg_id, mark).await else {
        return;
    };
    // The ceiling is expected and costs nothing; anything else is the receipts silently gone —
    // a forum whose settings allow no reactions, or a bot with no right to react — and it is said
    // once, at a level the unit's journal shows.
    if refused.flood_wait.is_some() {
        tracing::debug!(
            error = %refused, message = %msg_id, ?mark,
            "Telegram refused a reaction for its own ceiling; the topic still says what happened"
        );
    } else if !refusal_said.swap(true, Ordering::AcqRel) {
        tracing::warn!(
            error = %refused, message = %msg_id, ?mark,
            "Telegram refuses this bot's reactions, so his messages will carry no marks; the \
             topic still says what happened. Said once; the cause is the chat's settings or the \
             bot's rights, not the line"
        );
    } else {
        tracing::debug!(
            error = %refused, message = %msg_id, ?mark,
            "Telegram refused a reaction again; said at warn once already"
        );
    }
}

/// Why a tap did not become an answer. Every one of these is said out loud in the topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapRefusal {
    /// Nothing was written down beside that message.
    NoRecord,
    /// The button is not one of the ones written down.
    NotAnOption,
    /// The project that asked is not connected now.
    NotConnected,
    /// It asked, then restarted. The question belongs to a process that no longer exists.
    Restarted,
    /// It has already been answered — from the phone or at the terminal — and the keyboard is only
    /// still there because taking it away has not happened yet: the edit failed, or is in flight.
    AlreadyAnswered,
    /// The agent withdrew it or let it time out, so nobody is waiting for an answer; the keyboard
    /// is only still there because taking it away has not happened yet.
    NoLongerAsked,
    /// The tap came from a chat this bot does not answer, or from a person who may not speak
    /// there. Silence either way: a reply confirms something is listening.
    NotYours,
}

/// Whether a person may speak, and where. What the sender gate answers.
///
/// Three answers and not two, because a command and a line typed at an agent need different ones.
/// A person let into one project's conversations may type at that project's agents; he may not
/// run `/projects` and read every other project's name and state, and he may not speak in General
/// or in a topic the hub cannot place. Only the bot-wide list opens those.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Not on any list that covers where this arrived. Silence and one audit line.
    Stranger,
    /// On the list of the project this conversation belongs to, and nothing wider.
    InThisConversation,
    /// On the bot-wide list: may speak wherever this bot listens, and may give it commands.
    Anywhere,
}

impl Standing {
    /// May this person type at the agent in this conversation?
    pub fn may_speak_here(self) -> bool {
        !matches!(self, Self::Stranger)
    }

    /// May this person give the bot a command — which is answered with facts about every project?
    pub fn may_command(self) -> bool {
        matches!(self, Self::Anywhere)
    }
}

impl TapRefusal {
    /// What the operator reads. No ids, no enum names, no "None".
    pub fn say(&self) -> &'static str {
        match self {
            Self::NoRecord => "I have no record of that question, so I will not answer it for you.",
            Self::NotAnOption => {
                "That button is not one of the answers I wrote down for this question."
            }
            // "That project" was true while a topic could only ever belong to a project. A lane has
            // its own topic now, and the project it belongs to may be connected and busy while the
            // worktree he is looking at is not — so that sentence became one he could read as false
            // with the project's own topic open beside it. The topic is what he is looking at, so
            // the topic is what this talks about.
            Self::NotConnected => {
                "Nothing is connected in this topic right now, so there is nobody to tell."
            }
            Self::Restarted => {
                "That question belonged to a session that has since restarted. Ask again and it will come back."
            }
            Self::AlreadyAnswered => {
                "That one has already been answered. I have not sent anything."
            }
            Self::NoLongerAsked => {
                "That question is no longer being asked, so I have not sent anything."
            }
            Self::NotYours => "",
        }
    }
}

/// What happened when a tap that reached nobody was taken back.
///
/// Three, because the operator reads a different sentence for each and two of them are opposites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Withdrawal {
    /// The keyboard came off and the record went with it.
    Retired,
    /// Telegram refused the edit. The menu is still live — and it can be tapped again, because the
    /// answer was taken back off the question rather than the record being thrown away.
    StillOnHisPhone,
    /// There was nothing written down any more. The project's own side finished with the question
    /// while the tap was in flight, which is not far-fetched: the delivery that failed may well
    /// have failed because that same bridge's outbox was full.
    NothingLeftToTakeBack,
}

/// Every open question, keyed by the message the operator can see.
///
/// Persisted, because a restart must not turn every live keyboard into a button that does nothing.
#[derive(Debug, Default)]
pub struct AskLedger {
    path: PathBuf,
    records: BTreeMap<String, AskRecord>,
}

/// `chat:message` — flat, so it survives a JSON round trip without a tuple-key encoding.
///
/// **Keyed on the chat as well as the message.** Every supergroup numbers its reply threads with
/// plain message ids drawn from one counter, which is how a swipe-to-reply on a direct message
/// once reached a forum pane. The chat is half the key, not decoration.
fn ledger_key(chat_id: i64, msg_id: &MsgId) -> String {
    format!("{chat_id}:{}", msg_id.as_str())
}

impl AskLedger {
    /// Read the ledger, or start empty — and say so when starting empty was not the plan.
    ///
    /// This used to swallow both the read error and the parse error with `.ok()`. The cost of that
    /// silence is specific: every keyboard already on the operator's phone becomes a button that
    /// answers "I have no record of that question", with nothing anywhere explaining why. Starting
    /// empty is still the right behaviour — refusing to boot would take the whole fleet's channel
    /// down over one bad byte — but it must not be quiet.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let records = match fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<BTreeMap<String, AskRecord>>(&raw) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(
                        error = %e, path = %path.display(),
                        "the open-questions ledger could not be read. Every keyboard already on \
                         the operator's phone will refuse; those questions need asking again."
                    );
                    BTreeMap::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "the open-questions ledger is unreadable");
                BTreeMap::new()
            }
        };
        Self { path, records }
    }

    pub fn default_path() -> PathBuf {
        crate::lock::state_dir().join("asks.json")
    }

    /// Write down what a message means, before anyone can tap it.
    pub fn record(
        &mut self,
        chat_id: i64,
        msg_id: &MsgId,
        record: AskRecord,
    ) -> std::io::Result<()> {
        self.records.insert(ledger_key(chat_id, msg_id), record);
        self.save()
    }

    pub fn get(&self, chat_id: i64, msg_id: &MsgId) -> Option<&AskRecord> {
        self.records.get(&ledger_key(chat_id, msg_id))
    }

    /// Write down that a question has been answered, so it cannot be answered again.
    ///
    /// The record stays — the keyboard may still need retiring — but it stops authorising anything.
    pub fn mark_answered(
        &mut self,
        chat_id: i64,
        msg_id: &MsgId,
        option_id: &OptionId,
    ) -> std::io::Result<()> {
        if let Some(r) = self.records.get_mut(&ledger_key(chat_id, msg_id)) {
            r.answered = Some(option_id.clone());
        }
        self.save()
    }

    /// Take the answer back off a question, so its keyboard can be tapped again.
    ///
    /// For the one case where an answer was written down and then reached nobody. The record has to
    /// stay — it is the only handle anything has on a keyboard that is still live — so what is
    /// undone is the authorisation, not the record.
    pub fn mark_unanswered(&mut self, chat_id: i64, msg_id: &MsgId) -> std::io::Result<()> {
        if let Some(r) = self.records.get_mut(&ledger_key(chat_id, msg_id)) {
            // Only the phone's mark comes off. A terminal close that landed in the meantime is a
            // fact about the agent, not about this tap, and taking it back would reopen a question
            // nobody is asking.
            r.answered = None;
        }
        self.save()
    }

    /// Write down that a question ended at the terminal, on every message that carries it, and
    /// hand back the messages whose keyboards now need retiring.
    ///
    /// One lock, one write: the mark is what a tap is judged against, so it has to be there BEFORE
    /// the first keyboard edit begins, and it has to be there for every message of the ask at
    /// once — marking each as its edit came round would leave the later ones tappable while the
    /// earlier ones were coming off.
    ///
    /// The mark goes on EVERY message of the ask, the phone's own answers included. It is not the
    /// same fact as `answered` and cannot be folded into it: a tap that was written down and then
    /// reached nobody has its authorisation taken back off (`mark_unanswered`), and if the
    /// terminal's answer were not written down beside it that record came back fully open — a live
    /// keyboard for a question the agent had already resolved, one tap from a contradicting choice
    /// landing in its turn.
    ///
    /// What is handed back is narrower: a record the phone answered whose keyboard is still being
    /// taken off is NOT a target. That retirement belongs to the tap, is already in flight, and
    /// entering it twice means two edits of one message with the same words — the second of which
    /// Telegram refuses as unchanged, which reads in the journal as a keyboard that would not come
    /// off. Once that edit has actually failed it says so on the record (`closed`), and from then
    /// on this is the retry that takes it off. A record already closed keeps its first note.
    ///
    /// The targets come back even when the write to disk fails: the mark is held in memory and a
    /// tap is refused against that, so refusing to retire would leave a keyboard live for a
    /// question the hub already knows is closed. The failed write is said in the journal.
    pub fn close_all(
        &mut self,
        addr: &Addr,
        instance: &str,
        ask_id: &AskId,
        closed: Closed,
    ) -> Vec<(i64, MsgId)> {
        let mine = |r: &AskRecord| r.addr_is(addr) && r.instance == instance && &r.ask_id == ask_id;
        // Read before anything is written, because the write below is what would make every
        // record look like one whose retirement had already been refused.
        let targets = self.matching(|r| mine(r) && r.needs_retiring());
        let mut changed = false;
        for (chat, msg) in self.matching(mine) {
            if let Some(r) = self.records.get_mut(&ledger_key(chat, &msg))
                && r.closed.is_none()
            {
                r.closed = Some(closed.clone());
                changed = true;
            }
        }
        if changed && let Err(e) = self.save() {
            tracing::error!(
                error = %e, project = %addr.project, lane = addr.lane_field(),
                "could not write down that a question ended at the terminal; it is closed until \
                 the next restart, and open again after it"
            );
        }
        targets
    }

    /// Write down that one message's question is over and how its keyboard must be signed off,
    /// unless something already said so.
    ///
    /// The one-message twin of [`Self::close_all`], for the side that knows a message and not an
    /// ask id: the retirement that follows a tap. Whoever wrote the mark first keeps it — a second
    /// writer here is a later account of a question that already ended, and the words the operator
    /// sees should be the ones from the moment it did.
    pub fn mark_closed(
        &mut self,
        chat_id: i64,
        msg_id: &MsgId,
        closed: Closed,
    ) -> std::io::Result<()> {
        if let Some(r) = self.records.get_mut(&ledger_key(chat_id, msg_id))
            && r.closed.is_none()
        {
            r.closed = Some(closed);
        }
        self.save()
    }

    /// Forget a question that has been answered or withdrawn.
    pub fn forget(&mut self, chat_id: i64, msg_id: &MsgId) -> std::io::Result<()> {
        self.records.remove(&ledger_key(chat_id, msg_id));
        self.save()
    }

    /// Every message still carrying a live keyboard for one ask, so it can be retired.
    ///
    /// **Scoped to the session that asked it.** A bridge mints its ask ids from a counter that
    /// starts over with the process, so the first question of every session carries the same
    /// string — and a session that dies with its opening question still open leaves that record
    /// behind, because nothing prunes. Matching on the ask id alone therefore reached across
    /// sessions: a new session answering its own first question rewrote the dead one's question on
    /// the operator's phone with an outcome that belonged to a different question, and then deleted
    /// the record that proved it had ever been asked.
    pub fn messages_for(&self, addr: &Addr, instance: &str, ask_id: &AskId) -> Vec<(i64, MsgId)> {
        self.matching(|r| r.addr_is(addr) && r.instance == instance && &r.ask_id == ask_id)
    }

    /// Every question left open by some run of THIS CONVERSATION other than the one named, so a
    /// worker that never came back does not leave a keyboard on the operator's phone that nothing
    /// will ever take away.
    ///
    /// "Other than this one" is the whole of it, and it is asked when a bridge arrives rather than
    /// when one leaves. A claim is exclusive, so at the moment one is granted every other run of
    /// this conversation is provably not connected. Asked the other way round — at `release` — the
    /// answer would be wrong: a bridge keeps its instance across a reconnect, so a session that
    /// drops and comes straight back is still waiting for exactly those answers, and taking their
    /// keyboards away would be a live question removed from his phone.
    ///
    /// **Scoped to the lane, and that is not a refinement — it is what keeps the argument above
    /// true.** A claim used to be exclusive per PROJECT, so "nothing else of this project is
    /// connected" and "nothing else of this conversation is connected" were the same sentence. Two
    /// lanes may now hold claims at once, and only the second sentence survives. Matched on the
    /// project, an arriving lane swept every open question of every other LIVE lane off his phone
    /// with "the session that asked this restarted" — false, while each of those agents sat there
    /// still waiting for the answer it had taken away.
    ///
    /// A record the operator answered from his phone IS collected, but only once the retirement
    /// that belonged to that tap has been refused and said so on the record. It used to be left
    /// alone outright, on the reasoning that its keyboard is live only because taking it away
    /// failed and a note about a restart would be misinformation from the other direction — true
    /// about the note, wrong about the keyboard, and nothing else was ever going to take it off.
    /// The note is no longer the sweep's to invent: the record carries what he chose, and that is
    /// what `retire_each` writes.
    pub fn open_for_other_instances(&self, addr: &Addr, instance: &str) -> Vec<(i64, MsgId)> {
        self.matching(|r| r.addr_is(addr) && r.instance != instance && r.needs_retiring())
    }

    /// Every question left open by a worktree of this project whose process is GONE.
    ///
    /// The sweep above only ever reaches the conversation that is arriving, and for a project's own
    /// voice that was enough: the next session of a project always arrives eventually. A lane is
    /// never dispatched twice under the same name, so its address never arrives again and nothing
    /// above will ever look at what it left behind — a keyboard on his phone that no restart, no
    /// sibling and no timeout can take off, and a record in a file that is rewritten whole on every
    /// ask of every project on the box.
    ///
    /// Widening the sweep to the project needed PROOF, not a heuristic, because the widening that
    /// was tried first — "nothing is connected at that address right now" — is the exact failure
    /// `release` was rejected for: a bridge that merely lost its socket is disconnected for a second
    /// and is still waiting for those answers. Two facts together are the proof, and neither alone
    /// is: nothing holds that address now, AND the process that asked is no longer running. A
    /// reconnecting bridge is the same process, so it fails the second.
    ///
    /// A record whose pid was never written down is left alone. Unknown is not gone.
    pub fn open_where_the_asker_is_gone(
        &self,
        project: &ProjectId,
        live: &BTreeSet<Addr>,
    ) -> Vec<(i64, MsgId)> {
        self.matching(|r| {
            &r.project == project
                && r.needs_retiring()
                && !live.contains(&r.addr())
                && r.pid.is_some_and(|pid| !fence_is_alive(pid))
        })
    }

    /// Drop every record too old for Telegram to edit, and say how many went.
    ///
    /// A record's one job is to be the handle that takes a keyboard off a message. Telegram refuses
    /// an edit on a message older than about 48 hours, so past that age it cannot do that job for
    /// anybody — and what is left is a row in a file that is serialised whole and rewritten on every
    /// ask and every tap of every project. Keeping it costs and buys nothing.
    ///
    /// A tap on one of those keyboards is then refused with "I have no record of that question",
    /// which is the fail-closed answer and the true one: the session that asked it two days ago is
    /// not there to hear an answer.
    pub fn drop_what_can_no_longer_be_retired(&mut self, now: u64) -> usize {
        let before = self.records.len();
        self.records
            .retain(|_, r| r.at == 0 || now.saturating_sub(r.at) <= EDIT_WINDOW_SECS);
        let dropped = before - self.records.len();
        if dropped > 0 {
            // Not silent: the ledger shrinking is a thing someone reading an incident afterwards
            // has to be able to account for.
            tracing::info!(
                dropped,
                "dropped questions too old for their keyboards ever to come off again"
            );
            let _ = self.save();
        }
        dropped
    }

    /// The chat and message behind every record the predicate accepts.
    fn matching(&self, mut want: impl FnMut(&AskRecord) -> bool) -> Vec<(i64, MsgId)> {
        self.records
            .iter()
            .filter(|(_, r)| want(r))
            .filter_map(|(k, _)| {
                let (chat, msg) = k.split_once(':')?;
                Some((chat.parse().ok()?, MsgId::new(msg)))
            })
            .collect()
    }

    /// Write the whole ledger, every time.
    ///
    /// **This is the state file that compounds, and it is the expensive one.** It is serialised
    /// whole and renamed into place on every `record`, `mark_answered`, `mark_unanswered`,
    /// `close_all` and `forget` — so every ask and every tap of every project on the box pays for
    /// whatever is in it, on a blocking write with the ledger mutex held. Measured: a record
    /// holding a worst-case-length question costs about 3.9 KB, and one further ask on top of a
    /// 360-record backlog costs about 37 ms under that lock.
    ///
    /// Two things bound it, and both are here because a topic per lane made "the next session of
    /// this project will collect it" stop being true: [`Self::open_where_the_asker_is_gone`] takes
    /// what a worktree left when its process is provably gone, and
    /// [`Self::drop_what_can_no_longer_be_retired`] takes what has aged past the point of being
    /// useful to anybody. `projects.json` grows too, and is the one people notice — but a lane row
    /// there costs 40 bytes and is only read on admission. This is the one to watch.
    fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            crate::conversations::private_state_dir(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(&self.records)?;
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(&body)?;
            f.flush()?;
        }
        fs::rename(&tmp, &self.path)
    }
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The audit.

/// Sent-before, outcome-after, nothing dangles.
#[derive(Debug, Clone)]
pub struct HubAudit {
    path: PathBuf,
}

impl HubAudit {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        crate::lock::state_dir().join("hub.audit.log")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Recorded BEFORE the send. A `sent` with no outcome after it means the process died in the
    /// middle of writing, and means nothing else — which is the only reason the pair is useful.
    pub fn sent(&self, addr: &Addr, topic_id: i32, bytes: usize) -> std::io::Result<()> {
        self.line(&format!(
            "sent\t{}\ttopic={topic_id}\tbytes={bytes}",
            subject(addr)
        ))
    }

    /// The same, for a file an agent attached: recorded BEFORE the upload, with its size and the
    /// name in the outbox — which has passed the address rules by now, so it can carry nothing
    /// that forges a line here.
    pub fn sent_file(
        &self,
        addr: &Addr,
        topic_id: i32,
        bytes: usize,
        name: &str,
    ) -> std::io::Result<()> {
        self.line(&format!(
            "sent-file\t{}\ttopic={topic_id}\tbytes={bytes}\tname={name}",
            subject(addr)
        ))
    }

    pub fn outcome(&self, addr: &Addr, outcome: &SendOutcome) -> std::io::Result<()> {
        let word = match outcome {
            SendOutcome::Sent(id) => format!("delivered\tmessage={id}"),
            SendOutcome::Clamped(id) => format!("delivered\tmessage={id}\tclipped=yes"),
            SendOutcome::TooFast(d) => format!("shed\tretry_after_ms={}", d.as_millis()),
            SendOutcome::TopicGone => "topic-gone".to_owned(),
            SendOutcome::Refused(why) => format!("refused\twhy={why}"),
            SendOutcome::Unseen => "unseen".to_owned(),
            SendOutcome::SwitchedOff => "switched-off".to_owned(),
        };
        self.line(&format!("{word}\t{}", subject(addr)))
    }

    /// A branch that sends nothing still writes a line, so silence in this file always means the
    /// process stopped rather than that the hub decided something quietly.
    pub fn refused(&self, addr: &Addr, why: &str) -> std::io::Result<()> {
        self.line(&format!("refused\t{}\twhy={why}", subject(addr)))
    }

    /// One file he sent, and what became of it: on disk at a path the hub minted, or not and why.
    ///
    /// The reported filename is NOT written here, on purpose. This file is one record per line,
    /// and a name from a phone can carry a newline — which would be a second record of the
    /// sender's choosing. The path is the hub's own and carries nothing anybody else chose.
    pub fn file(&self, addr: &Addr, file: &hub_proto::MessageFile) -> std::io::Result<()> {
        let kind = format!("{:?}", file.kind).to_lowercase();
        let word = match (&file.path, file.why) {
            (Some(path), _) => format!(
                "fetched\t{}\tkind={kind}\tbytes={}\tpath={path}",
                subject(addr),
                file.bytes.unwrap_or(0)
            ),
            (None, why) => format!(
                "not-fetched\t{}\tkind={kind}\twhy={}",
                subject(addr),
                match why {
                    Some(hub_proto::FileWhy::TooBig) => "too-big",
                    Some(hub_proto::FileWhy::NotStored) => "not-stored",
                    Some(hub_proto::FileWhy::DownloadFailed) | None => "download-failed",
                }
            ),
        };
        self.line(&word)
    }

    /// One intention, written down BEFORE the frame is on the wire.
    ///
    /// `HubAudit::sent`'s rule and its reason: a line with no `intent-outcome` after it means the
    /// process died in the middle, and means nothing else.
    ///
    /// Every field is either the hub's own or one whose shape was checked before a claim could hold
    /// it — the spec by `spec_is_addressable`, the domain by `lane_is_addressable`, the op is a
    /// variant, and the key is a fixed-length token over a fixed alphabet. So nothing in this line
    /// can forge a second record. The user id is written for `HubAudit::stranger`'s reason: an id
    /// in this file is one the operator can copy.
    ///
    /// The one absent field is `count` where there is none and `for_lease` where there is none, and
    /// both are written as `-` rather than left out — a field that is sometimes there is a field
    /// every search for it has to guess about.
    // No shipped caller until the keyboard is drawn; see the note at the head of `hub::intent`.
    #[allow(dead_code)]
    fn intent(&self, i: &Intent) -> std::io::Result<()> {
        self.line(&format!(
            "intent\t{}\tdomain={}\top={}\tspec={}\tintent={}\tkey={}\tfor_lease={}\tcount={}\tsender={}",
            subject(&i.to),
            i.about.lane_field(),
            i.op.name(),
            i.spec,
            i.id,
            i.key,
            i.for_lease.map_or("-".to_owned(), |g| g.to_string()),
            i.count.map_or("-".to_owned(), |c| c.to_string()),
            name_the_sender(Some(i.user)),
        ))
    }

    /// An intention that was not carried, and why in the hub's own words.
    ///
    /// Its own line rather than `HubAudit::refused` because it spells the domain the way the two
    /// lines beside it do: a search for one conversation's lifecycle work then finds the refusals
    /// with the intentions, which is the whole reason the field exists rather than being folded
    /// into the subject. `why` is a sentence this binary owns — never a word from anywhere else.
    // No shipped caller until the keyboard is drawn; see the note at the head of `hub::intent`.
    #[allow(dead_code)]
    fn intent_refused(
        &self,
        project: &ProjectId,
        domain: Option<&LaneId>,
        why: &'static str,
    ) -> std::io::Result<()> {
        self.line(&format!(
            "intent-refused\tproject={project}\tdomain={}\twhy={why}",
            domain.map_or("-", LaneId::as_str)
        ))
    }

    /// A button that had already been carried, tapped again.
    ///
    /// A branch that carries nothing still writes a line, so silence in this file always means the
    /// process stopped rather than that the hub decided something quietly — and this is the one
    /// branch where nothing was carried and nothing was refused either.
    // No shipped caller until the keyboard is drawn; see the note at the head of `hub::intent`.
    #[allow(dead_code)]
    fn intent_again(&self, at: &Addr, id: &IntentId, key: &IdempotencyKey) -> std::io::Result<()> {
        self.line(&format!(
            "intent-again\t{}\tintent={id}\tkey={key}",
            subject(at)
        ))
    }

    /// A button whose intention nothing ever answered for, tapped again — and carried again.
    ///
    /// Its own word rather than `intent-again`, because the two are opposite events: that one says
    /// nothing went out, and this one says something did. The line names the record that was
    /// dropped, so a reader can join it to the `intent` line that follows.
    // No shipped caller until the keyboard is drawn; see the note at the head of `hub::intent`.
    #[allow(dead_code)]
    fn intent_again_carried(
        &self,
        at: &Addr,
        id: &IntentId,
        key: &IdempotencyKey,
    ) -> std::io::Result<()> {
        self.line(&format!(
            "intent-again-carried\t{}\tafter={id}\tkey={key}",
            subject(at)
        ))
    }

    /// What the controller said became of it.
    ///
    /// The controller's own sentence is **never written here**, ever. `HubAudit::file` gives the
    /// reason for a filename and it is the same one: this file is one record per line, and a
    /// sentence from another process can carry a newline, which would be a second record of the
    /// sender's choosing. The status is a variant of a closed set and carries nothing anybody chose.
    fn intent_outcome(
        &self,
        at: &Addr,
        id: &IntentId,
        status: IntentStatus,
    ) -> std::io::Result<()> {
        let word = match status {
            IntentStatus::Accepted => "accepted",
            IntentStatus::Refused => "refused",
            IntentStatus::Completed => "completed",
            IntentStatus::Failed => "failed",
        };
        self.line(&format!(
            "intent-outcome\t{}\tintent={id}\tstatus={word}",
            subject(at)
        ))
    }

    /// Something arrived from a person who may not speak where it arrived, and was dropped.
    ///
    /// The one line a stranger leaves. He gets silence on the phone — a reply confirms something
    /// is listening — so this file is the only place the operator can learn that somebody in his
    /// forum is typing at his agents, and WHO: the user id is written down exactly so that
    /// `herdr-tg allow <repo> <that id>` is a copy from this line, and so that a teammate who says
    /// "the bot ignores me" can be found without guessing. `project=-` when the words had no
    /// conversation to belong to (General, a command, a button nothing was written down beside),
    /// so a search for a project's lines still finds every line about it and nothing else.
    pub fn stranger(
        &self,
        user: Option<i64>,
        chat_id: i64,
        at: Option<&Addr>,
        what: &str,
    ) -> std::io::Result<()> {
        let sender = name_the_sender(user);
        let at = at.map_or("project=-".to_owned(), subject);
        self.line(&format!(
            "refused\t{at}\tsender={sender}\tchat={chat_id}\twhat={what}\twhy=not allowed to speak here"
        ))
    }

    fn line(&self, body: &str) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            crate::conversations::private_state_dir(dir)?;
        }
        let mut f = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&self.path)?;
        // Re-asserted: `.mode()` applies only at creation, so a file that was once world-readable
        // would stay that way for the life of the machine.
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600));
        writeln!(f, "{}\t{body}", now_iso())
    }
}

/// How a sender is written down wherever the operator may read an id back: the number itself, or
/// `unknown` when the update carried no person the bot could vouch for.
///
/// One function for the audit line and the journal line, so the two places he can copy an id
/// from say the same thing. The journal printed the `Option` as it was — `Some(555001)`, `None` —
/// which is not a number anyone can paste into `herdr-tg allow`, on the one line whose stated
/// purpose is that he does exactly that.
pub fn name_the_sender(user: Option<i64>) -> String {
    user.map_or("unknown".to_owned(), |u| u.to_string())
}

/// Who a line is about, as the fields an incident is grepped by.
///
/// `project=` stays exactly what it was, so a search for a project still finds every line its lanes
/// wrote. The lane is a SECOND field rather than a decoration on the first, because a compound
/// subject would match neither of the two queries anyone actually types.
///
/// Nothing here escapes anything, and nothing here needs to: a lane that could carry a tab or a
/// newline would write lines of its own choosing into this file, so the shape of a lane is checked
/// once, where a lane first becomes an address, and a bad one never reaches a claim or a topic —
/// let alone this.
fn subject(addr: &Addr) -> String {
    match &addr.lane {
        None => format!("project={}", addr.project),
        Some(lane) => format!("project={}\tlane={lane}", addr.project),
    }
}

fn now_iso() -> String {
    // Seconds since the epoch. Not pretty, and deliberately dependency-free: a log line's job here
    // is ordering and correlation, and a date crate is a supply-chain decision for a timestamp.
    format!("t={}", now_secs())
}

/// Seconds since the epoch, or zero when the clock will not say.
///
/// Zero is the same value a record written before ages existed carries, and both mean the same
/// thing to the only reader there is: an age this code will not act on.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Telling the operator the chat is full, without spending the chat to say it.

/// How long the chat has to stop losing messages before a cooling-off window counts as over.
///
/// A minute, because that is the width of Telegram's own window: anything shorter would declare it
/// over while the flood wait that caused it was still running, and he would read "it is keeping up
/// again" about a chat that is still shut.
pub const THROTTLE_WINDOW: Duration = Duration::from_secs(60);

/// The shortest gap between two updates of the running count.
///
/// An edit costs nothing against the per-minute ceiling — that is the measured fact this whole
/// design rests on — but it is still an HTTPS call, and fourteen agents shedding in the same second
/// should not be fourteen of them. He is reading a number, not a ticker.
///
/// One second, matching the chat's own rhythm.
///
/// **It defers a write; it never drops one.** This used to return without writing and without
/// remembering that it had not, so a burst inside one second updated the number once and threw the
/// rest away. That is not an edge case at fourteen connections — it is the characteristic shape,
/// because `until` is minted per message and frames that arrive together expire together — and what
/// he was left reading was "1 message did not get through" after fifteen had. So the first update
/// the rhythm turns away BOOKS the write instead, and whichever number the burst has reached by
/// then is the one that goes out. See [`Throttle::flush_booked`].
const THROTTLE_EDIT_GAP: Duration = Duration::from_secs(1);

/// The one message in the forum that says the chat is over its ceiling, and what it is counting.
///
/// **The shape is one SEND per cooling-off window and free EDITS inside it**, and that shape is the
/// answer to a recursion: a line saying the chat is full is itself a message, wanting a token from a
/// budget that is empty precisely when it is worth sending. One line per shed would spend eleven of
/// eighteen tokens apologising, and each of those is a message from an agent that did not go out.
///
/// A window is at least a minute wide, so the send can never cost more than one eighteenth of the
/// budget, and it is paid for out of [`crate::queue::RESERVED_FOR_THE_OPERATOR`] — held back from
/// the agents exactly so that this can always go out. Everything after it is an edit of the same
/// message, which the probe on 3 September measured as free.
///
/// **What he sees**: one notification when the window opens, and then that same line quietly
/// updating in place with the count. An edit does not re-notify and does not move the message down
/// the topic, so the send is what he feels and the edits are what he finds when he looks — which is
/// the right way round for something that is by nature one fact repeated.
#[derive(Debug, Default)]
struct Throttle {
    /// The message being kept up to date, once one has gone out.
    notice: Option<MsgId>,
    /// How many messages have not gone out during this window.
    lost: u32,
    /// The count the operator can actually SEE, which is not the same number.
    ///
    /// Kept so that a write the rhythm deferred can tell whether it still has anything to say by
    /// the time it runs — a burst that has already been written out in full does not need a
    /// second edit saying the same thing.
    last_written: u32,
    /// Which conversations lost something, by the names their topics carry.
    ///
    /// A bare count is a number he cannot act on: fourteen topics and no indication of which is
    /// missing a turn leaves him opening each one in turn, which is the thing this whole line
    /// exists to save him from.
    from: BTreeSet<String>,
    /// Set the first time a loss happens in a conversation the registry could not name.
    ///
    /// See [`conversations_that_lost_something`]: a partial list would say "from these two" about
    /// four, so the list is dropped whole rather than shortened. It is a default of `false` on
    /// purpose — a window with nothing in it has nothing it failed to name.
    a_loss_it_could_not_name: bool,
    /// When the last of them was lost, which is what decides the window is over.
    last_loss: Option<std::time::Instant>,
    /// When the count was last written, so a burst of losses is not a burst of edits.
    last_edit: Option<std::time::Instant>,
    /// Whether a task is already waiting out [`THROTTLE_EDIT_GAP`] to write the count.
    ///
    /// One at a time, chat-wide. Fourteen agents shedding in the same second must not be fourteen
    /// HTTPS calls — but they must not be one call and thirteen silences either, which is what
    /// dropping the deferred writes actually produced.
    flush_booked: bool,
    /// A window that ought to be open and could not be, because the chat would not take the line.
    ///
    /// The honest hole in the design, kept as a flag rather than pretended away: while Telegram has
    /// the chat shut, the reserve buys nothing — nothing at all goes out — so the one moment he most
    /// needs telling is the one moment nothing can tell him. It is opened at the next opportunity
    /// instead, which is the next send or the next loss after the chat reopens.
    ///
    /// **A window that is owed is never closed unsaid.** It used to be: the cooling-off check ran
    /// before the debt was paid and reset the whole record, so a herd that went quiet for a minute
    /// after a flood — which is exactly what a herd does once everything it says is being shed —
    /// had the only account of that minute thrown away by the first message that got through.
    owed: bool,
}

/// Which conversations went quiet, in the words his topic titles use.
///
/// Empty when nothing can be said honestly: a window with even one loss it could not name would
/// otherwise read as a complete list of a subset, which is worse than no list at all — he would
/// stop looking after the ones it named.
fn conversations_that_lost_something(from: &BTreeSet<String>, all_named: bool) -> String {
    if from.is_empty() || !all_named {
        return String::new();
    }
    let names: Vec<&str> = from.iter().map(String::as_str).collect();
    match names.as_slice() {
        [one] => format!(" from {one}"),
        [one, two] => format!(" from {one} and {two}"),
        [one, two, ..] => format!(
            " from {one}, {two} and {} other conversations",
            names.len() - 2
        ),
        [] => String::new(),
    }
}

/// What he reads while the chat is over its ceiling.
///
/// No ids, no counts of tokens, no word for the thing that happened inside the code. What it has to
/// carry is: how much he is missing and from where, that this is the chat's limit rather than
/// anything broken, and that nothing is sitting waiting on him — so he does not go looking for an
/// answer nobody is waiting to give.
///
/// **The count comes first, and that is not a style choice.** The send is the only part of this he
/// ever feels: it is one notification per cooling-off window, and what a lock screen and a chat-list
/// row show him is the BEGINNING of it. This began with a hundred and sixteen characters of standing
/// fact about Telegram's rate limit, so the one message engineered to cost a precious send spent it
/// on a banner indistinguishable from an informational blurb, with the news below the fold.
///
/// It is still deliberately NOT in the present tense about the trouble. This message is edited in
/// place and is the last thing standing when a herd goes quiet, so a clause reading "more is being
/// said than this chat will carry" would be a sentence he finds hours later about a chat that has
/// been idle since. A count of what did not get through is past tense and does not go stale.
///
/// The limit is described rather than numbered. The API's ceiling is twenty and this hub sheds at
/// seventeen for an agent, so printing either number invites him to compare it with the other and
/// conclude the bot is misconfigured.
fn throttle_line(lost: u32, whose: &str) -> String {
    let (thing, it) = if lost == 1 {
        ("message", "it")
    } else {
        ("messages", "them")
    };
    format!(
        "{lost} {thing}{whose} did not get through. This chat will only carry a little under twenty \
         messages a minute — Telegram's limit for a group, shared by everything running here — and \
         nothing is waiting on you for {it}."
    )
}

/// And what that same line becomes once the chat is keeping up again. One more free edit.
///
/// The pronoun is taken from the count for the same reason it is in [`throttle_line`], and this is
/// the one of the pair that used to get it wrong: "1 message did not get through. Whatever was
/// saying them was told." That is the commonest value this line ever holds — one shed and a quiet
/// minute is the low-load shape — and it is the version of the message he is most likely to be
/// looking at, because it is the last edit and the one that stands afterwards.
fn throttle_cleared_line(lost: u32, whose: &str) -> String {
    let (thing, it) = if lost == 1 {
        ("message", "it")
    } else {
        ("messages", "them")
    };
    format!(
        "For a while more was being said here at once than this chat would carry, and {lost} \
         {thing}{whose} did not get through. Nothing is waiting on you for {it}. It is keeping up \
         again now."
    )
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Claims.

/// A live connection, and how to reach it.
#[derive(Debug)]
struct Claim {
    pid: u32,
    /// Which run of this address this connection is. Minted here, handed to the bridge on the
    /// welcome's own envelope, and the number every later frame of this connection is judged
    /// against. Never zero: a zero is how a peer says it holds none.
    generation: u64,
    instance: String,
    /// Does this run stamp a generation on what it sends?
    ///
    /// Shared with the connection's own read loop, which sets it the first time a frame arrives
    /// carrying one — the redial's `hello` cannot, because a run that has never been welcomed
    /// holds no number yet. It gates one thing and one thing only: whether this connection may be
    /// told `stale_generation`. A bridge that does not know the word treats an unknown refusal as
    /// temporary, by a deliberate default in its own table, and redials for ever with nothing on
    /// the operator's phone to say why.
    speaks_generations: Arc<AtomicBool>,
    /// Did its `hello` promise to say what became of every choice it is handed?
    ///
    /// Read through `hub_proto::promises_to_confirm` and never by asking whether the field was
    /// present: an adapter that builds the list by filtering sends an empty one when it promises
    /// nothing, and a hub that read presence would tell the operator a tap "has not been
    /// confirmed" by a bridge that never said it would.
    confirms_choices: bool,
    /// What this connection said IN ADVANCE it can be asked to do, as the hub admitted it.
    ///
    /// Kept on the claim for `confirms_choices`' reason: the declaration belongs to the connection
    /// that made it, and a run that has since been replaced cannot have its successor asked to
    /// honour it. Read only through `hub_proto::frame::control_for`, which asks whether it names
    /// THIS operation rather than whether anything was declared at all.
    ///
    /// `None` for every bridge that is not a controller, which is every bridge shipped so far.
    // No shipped caller until the keyboard is drawn; see the note at the head of `hub::intent`.
    #[allow(dead_code)]
    controls: Option<Vec<Control>>,
    tx: mpsc::Sender<Envelope<HubFrame>>,
    /// The one way to end this connection from OUTSIDE its own read loop, and why.
    ///
    /// A read loop is a bare `reader.next()`, so nothing but the socket ending could ever stop it —
    /// and removing the map entry alone does not: the send path never consults this map, so a
    /// connection whose claim was taken away kept posting into its topic and spending the chat's
    /// budget until its bridge happened to hang up. Switching a project off has to end the
    /// connection, not merely forget it.
    kick: mpsc::Sender<Kick>,
}

/// Who was connected at one moment, and where that moment sits in the order the map changed.
///
/// It exists because the two halves of writing the presence file want opposite things. Deciding
/// the list needs the claims lock — a list read off a map something else is changing is a list of
/// no moment at all. Writing it must not hold that lock, because the same lock is how a message
/// finds the bridge it is going to. So the list is decided under the lock, carried out of it, and
/// written by [`Hub::put_who_is_connected_on_disk`] with nothing held that a delivery wants.
#[must_use = "a snapshot decided under the claims lock and never written leaves the file naming \
              connections that have since gone; hand it to `put_who_is_connected_on_disk`"]
struct WhoIsConnected {
    /// Which change to the map this is, minted under the claims lock — so this order IS the order
    /// the map changed in. The order the writes reach the DISK is not, and cannot be made to be:
    /// two writers let go of the lock and then race for one file. Comparing this is what makes a
    /// later snapshot win over an earlier one whatever order they arrive in.
    at: u64,
    connected: Vec<Addr>,
}

/// What an arriving run said about itself at `hello`, and what its claim has to remember of it.
///
/// One value rather than three arguments, because all three are the same fact — what this run
/// claims to be — and a caller that got their order wrong would silently promise a confirmation on
/// behalf of a bridge that made none.
struct Arriving {
    /// The generation it believes it holds, read off its `hello`'s own envelope. `None` for a run
    /// that has never been welcomed, and for every bridge from before the field existed.
    generation: Option<u64>,
    /// Whether its `hello` promised to say what became of every choice it is handed.
    confirms_choices: bool,
    /// What it declared it can be asked to do, already filtered to what this hub will act on —
    /// see [`admit_controls`]. Filtered BEFORE the claim so nothing unadmitted is ever held.
    controls: Option<Vec<Control>>,
    /// Set by its read loop the first time one of its frames carries a generation. See
    /// [`Claim::speaks_generations`].
    speaks_generations: Arc<AtomicBool>,
}

/// Why a connection is being ended from outside its own read loop, and what is said about it.
///
/// Two things and not one, because the two ways a connection is ended from here differ in exactly
/// this: a project switched off at the terminal is told `not_enabled`, and a run evicted because a
/// later one took the address is told `stale_generation` — but only if it stamped a generation, and
/// otherwise told nothing at all, which is what an evicted corpse has always been told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Kick {
    /// What goes on the wire ahead of the close. `None` closes without a word: there is either
    /// nobody behind the socket to tell, or no word in the closed set this peer could read.
    reason: Option<RefusedReason>,
    /// What the journal and the audit say happened. Never operator-facing.
    sentence: &'static str,
    /// What the frames this connection has already said are acked with. `None` where the closed
    /// set has no word for it, which is what a bridge is owed rather than a wrong one.
    why: Option<AckWhy>,
}

/// How often the hub looks at the registry file for a project switched off at the terminal.
///
/// A second, because "off" has to mean the flood stops NOW, and a poll is the only way a separate
/// process's write reaches a connection this process is holding: the CLI does not hold the socket,
/// and a signal would tie two processes together by pid for the one fact a file already carries. A
/// stat a second on a file of a few kilobytes is nothing; the registry is re-read only when the stat
/// says it changed.
pub const REGISTRY_WATCH_EVERY: Duration = Duration::from_secs(1);

/// Where the highest generation this hub has handed out for each address is written down.
///
/// Beside the audit log, like every other state file here, so a test's hub writes into its own
/// temp directory rather than into the operator's.
pub const GENERATIONS_FILE: &str = "hub.generations.json";

/// The highest generation this hub has handed out for each address.
///
/// # Why it is on disk at all
///
/// The mint is `max(highest + 1, the clock in milliseconds)`, and the clock alone would do for an
/// ordinary restart: the next hub starts numbering from a moment later than the last one stopped.
/// It does not do for a clock that steps BACKWARDS — an NTP correction, a laptop that came back
/// from suspend with a bad RTC, a container started with the wrong date — and a hub that re-hands a
/// number it has already given is a hub whose fence points the wrong way: the run it fences off is
/// the live one.
///
/// # What it is NOT
///
/// It is not [`crate::presence`]. That file says who is connected NOW, so a reader must disbelieve
/// it unless the hub that wrote it is still alive; this one says what has already been handed out,
/// which stays true precisely BECAUSE the hub that wrote it is gone. The only reader is the next
/// hub, and believing a number that is too high costs nothing — every live bridge is welcomed with
/// a fresh one — while believing one that is too low is the whole failure. So it is read as a
/// floor and nothing else, and a file that cannot be read leaves the clock to hold the line.
///
/// The writing discipline IS presence's, and for presence's reason: whole, temp-and-rename, 0600,
/// stamped with the pid that wrote it, so a reader never sees half a list and a person reading the
/// file can tell which hub put it there. With one deliberate difference — a failed write leaves the
/// old file where it is rather than unlinking it, because a stale floor is a low answer and not a
/// wrong one. See [`write_handed_out`].
#[derive(Debug)]
struct Generations {
    path: PathBuf,
    highest: BTreeMap<Addr, u64>,
}

/// What the file holds. `hub_pid` and `at` are for a person reading it; nothing decides on them.
#[derive(Debug, Serialize, Deserialize)]
struct HandedOut {
    hub_pid: u32,
    at: u64,
    addresses: Vec<AddressGeneration>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AddressGeneration {
    project: ProjectId,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    lane: Option<LaneId>,
    generation: u64,
}

impl Generations {
    fn load(path: PathBuf) -> Self {
        let highest = match fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => {
                // Not fatal, and not silent. The clock floor still climbs, so the fence still
                // works for every ordinary restart; what is lost is the belt for a clock that
                // went backwards, and a person has to know that before it matters.
                tracing::error!(
                    error = %e, path = %path.display(),
                    "could not read which run numbers this hub has already handed out; the clock \
                     alone will keep them climbing"
                );
                BTreeMap::new()
            }
            Ok(raw) => match serde_json::from_slice::<HandedOut>(&raw) {
                Err(e) => {
                    tracing::error!(
                        error = %e, path = %path.display(),
                        "the run numbers this hub had handed out are not readable; the clock alone \
                         will keep them climbing"
                    );
                    BTreeMap::new()
                }
                Ok(file) => file
                    .addresses
                    .into_iter()
                    .map(|a| {
                        (
                            Addr {
                                project: a.project,
                                lane: a.lane,
                            },
                            // Repaired on the way IN, not only on the way out. The one thing a
                            // hand-edited or corrupted file could do that the clock cannot undo is
                            // put the floor past what a bridge can read back, which fences every
                            // run of that address for ever with no wrong-looking number anywhere.
                            //
                            // And it is put back to the CLOCK rather than clamped to the ceiling,
                            // because a floor sitting exactly on the ceiling is worse than a high
                            // one: the mint cannot climb past it, so two runs are handed the same
                            // number, the reclaim fence stops refusing and the delivery fence
                            // stops firing — the double-admit the fence exists to prevent, again
                            // with nothing anywhere that looks wrong. No clock this hub can read
                            // is anywhere near the ceiling, so a number that is says the file is
                            // corrupt, not that the address has had that many runs.
                            if a.generation >= hub_proto::MAX_GENERATION {
                                now_millis()
                            } else {
                                a.generation
                            },
                        )
                    })
                    .collect(),
            },
        };
        Self { path, highest }
    }

    /// The number for the run that is taking this address now.
    ///
    /// Past both floors: one more than anything this hub has handed out for the address, and never
    /// behind the clock. The clock is what makes the number climb across a restart whose file was
    /// lost; the counter is what makes it climb when the clock does not.
    ///
    /// **Decides, and does not write.** Deciding has to happen under the claim's lock or two runs
    /// can be handed one number; writing needs nothing that lock holds, and doing it here put a
    /// disk between every project and its next message. The caller writes, with the lock let go —
    /// see [`Hub::write_down_the_run_numbers`].
    fn mint(&mut self, addr: &Addr, held_by_the_arriving_run: u64) -> u64 {
        let next = self
            .highest
            .get(addr)
            .copied()
            .unwrap_or(0)
            // Past the number the ARRIVING run already holds, too. The fence admits a run whose
            // number is ahead of this hub's floor on purpose — a state file lost, a clock that
            // came back wrong — and minting it something LOWER then had the wire fence refuse the
            // backlog it carried in as a later generation's, permanently, with nothing on his
            // phone. Admitting it and then fencing its own words is the one outcome neither
            // branch wants.
            .max(held_by_the_arriving_run)
            .saturating_add(1)
            .max(now_millis())
            // Every bridge reads frames with `JSON.parse`, which has no integers. A number past
            // this comes back as the nearest one a double can hold, and the bridge then stamps a
            // generation this hub never minted — fenced for ever, with nothing anywhere that looks
            // wrong. Milliseconds since the epoch are a quarter of a million years short of it.
            .min(hub_proto::MAX_GENERATION);
        // And the floor stays BELOW the ceiling, never on it. Sitting on it, the clamp above hands
        // every later run of the address the same number: the reclaim fence stops refusing and the
        // delivery fence stops firing, with nothing anywhere that looks wrong. Reachable now that
        // the number a bridge sends is believed — an adapter a stranger wrote to the document has
        // only to say its lease is the highest there is — so it gets the repair `load` already
        // makes for a corrupt file: put back to the clock, which no address's run count is within
        // a quarter of a million years of. That run's own backlog is then behind the number it
        // claimed and is refused, which is the right answer for a lease this hub never minted.
        let next = if next >= hub_proto::MAX_GENERATION {
            now_millis()
        } else {
            next
        };
        self.highest.insert(addr.clone(), next);
        next
    }

    /// Give a number back, because the run it was minted for never took the address.
    ///
    /// Only when nothing has been handed out since: if a later run has already been given a
    /// number, this one is history and putting the floor back would let a run the later one
    /// replaced come back. A connection that never became live — `kickoff-hub-attach --check`
    /// says `bye` before the pong on purpose — otherwise moves the address on and fences a
    /// session that was merely redialling, permanently, from a command that makes nothing.
    ///
    /// Decides and does not write, for [`Self::mint`]'s reason; the caller puts the map on disk.
    fn give_back(&mut self, addr: &Addr, minted: u64, was: u64) {
        if self.highest.get(addr).copied() != Some(minted) {
            return;
        }
        self.highest.insert(addr.clone(), was);
    }

    fn highest_for(&self, addr: &Addr) -> u64 {
        self.highest.get(addr).copied().unwrap_or(0)
    }

    /// Everything that should be on disk now, and where it goes.
    ///
    /// The WHOLE map every time, which is what makes a write that failed repairable: the next one
    /// that lands carries the numbers the failed one was carrying, so one bad moment on the disk
    /// costs nothing as soon as any later claim writes.
    fn to_write_down(&self) -> (PathBuf, HandedOut) {
        (
            self.path.clone(),
            HandedOut {
                hub_pid: std::process::id(),
                at: now_secs(),
                addresses: self
                    .highest
                    .iter()
                    .map(|(addr, generation)| AddressGeneration {
                        project: addr.project.clone(),
                        lane: addr.lane.clone(),
                        generation: *generation,
                    })
                    .collect(),
            },
        )
    }
}

/// Put the numbers on disk, whole.
///
/// A free function and not a method, because it must be callable with no lock held and nothing
/// borrowed: [`Generations`] lives behind a `std::sync::Mutex`, and a write that took `&self` would
/// have to be made while that mutex was held — which is where this write used to be, inside the
/// claims lock as well, with every project's delivery behind it.
fn write_handed_out(path: &Path, file: &HandedOut) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        crate::conversations::private_state_dir(dir)?;
    }
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let body = serde_json::to_vec_pretty(file)?;
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&body)?;
        f.flush()?;
    }
    // The file a failed write leaves behind is NEVER unlinked, which is where this parts company
    // with `presence` next door. Presence's file answers "who is connected now", so a stale one is
    // a lie and unlinking it is the honest repair. This one answers "what has already been handed
    // out", and a stale copy is not a lie — it is the same answer, lower. A lower floor is what the
    // clock is there to lift; no floor at all is worse than a low one.
    fs::rename(&tmp, path)
}

/// The wall clock in milliseconds, as the generation's floor reads it.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Something of HIS on its way to a bridge: the envelope id it went down under — the id the
/// bridge's `ack` for it names — and which of his messages, in which conversation, it was about.
///
/// It held only his typed words until taps joined them, and the two are one record on purpose: a
/// bridge answers both with the same `ack{ref}`, one id counter mints both, and the hub has to be
/// able to find whichever the id belongs to from the same place. Two lists keyed the same way is
/// two places for the same id to be looked up and one of them to win.
#[derive(Debug)]
struct Down {
    frame: FrameId,
    addr: Addr,
    /// The chat he typed or tapped in, so the mark on his message can find it. A message id is
    /// only half an address on Telegram: every chat numbers its own.
    chat_id: i64,
    /// His message: the line he typed, or the question whose button he pressed.
    msg_id: MsgId,
    what: His,
}

/// Which of his the record is about, and what has to be known to answer for it.
#[derive(Debug)]
enum His {
    Words {
        /// How many of the files on that message actually reached the hub's disk and went down as
        /// paths. The bridge's `ack` says how many it handed on; fewer than this is a file the
        /// agent never saw, and he is told.
        files_on_disk: u32,
    },
    Tap(HisTap),
}

/// One tap of his, handed down and not yet answered for.
#[derive(Debug)]
struct HisTap {
    /// What the button said, because every line the hub writes about it names what he chose.
    label: String,
    /// Which message his receipt — `Sent: <label>` — is, once the bot has sent it and been told
    /// which message Telegram made of it.
    ///
    /// `None` for the moment in between, and that moment is real: the receipt is a Telegram round
    /// trip that starts AFTER the answer is on the wire, and a tool server acks a choice within a
    /// millisecond of reading it. So an answer can arrive before there is any line to change, which
    /// is what `said` is for.
    receipt: Option<MsgId>,
    /// What the bridge said became of it, when that arrived before the receipt did.
    said: Option<WhatBecameOfTheTap>,
    /// Did the bridge promise, at `hello`, to say what became of every choice? Only a bridge that
    /// promised is ever said to have gone silent.
    promised: bool,
    /// Telegram refused the send that would have been his receipt, so there is no line to change
    /// and none is coming. Different from `receipt: None`, which is the ordinary moment before the
    /// round trip comes back: this one says the wait is over and nothing arrived. Without it a
    /// refusal from the agent waited for ever for a line that would never exist, and he was left
    /// with a keyboard that had gone and no word at all — precisely when the chat is busy, which
    /// is when a receipt is refused and when a refusal matters most.
    no_receipt_is_coming: bool,
    /// Did the window run out before there was any line to change? The window starts when the
    /// answer goes down and his receipt is a Telegram round trip that starts after it, so the two
    /// can cross — and when they do, the window has already run and nothing runs it again. Written
    /// here so the receipt, when it finally arrives, says what the window could not.
    overdue: bool,
}

/// What a bridge said became of one of his taps.
#[derive(Clone, Debug)]
enum WhatBecameOfTheTap {
    Took,
    /// Not taken, and the bridge's own words for why.
    Refused(String),
}

/// How many of his messages and taps the hub keeps waiting for an answer about, before the oldest
/// is forgotten. A bridge that never answers must not turn a record nobody will read into a leak.
const DOWN_KEPT: usize = 256;

/// How long a bridge that promised to confirm a choice has to say what became of it before the
/// operator is told it has not.
///
/// Twenty seconds, which is the question's own shelf life on his phone: past that he is looking at
/// a line that says his answer was sent and nothing has agreed. Shorter would call a slow worker a
/// broken one; longer and the line he is reading is wrong for the whole time he is reading it.
pub const TAP_CONFIRM_WINDOW: Duration = Duration::from_secs(20);

/// The longest lane name the hub will address a conversation by.
///
/// Well past anything kickoff mints — `lane-0902-201212-2783563` is twenty-four characters — and
/// short enough that a lane cannot crowd its project's own name out of a topic title.
pub const MAX_LANE: usize = 64;

/// The longest spec id the hub will write down beside an intention.
///
/// The same bound a lane gets and for the same half of the same reason: it is interpolated into the
/// audit and into the journal, and a handle long enough to crowd a line out is a line nobody reads.
/// The hub reads nothing OUT of a spec id — it holds no table to resolve one against — so this
/// bounds the RECORD and never the meaning.
pub const MAX_SPEC: usize = 64;

/// Which conversation a connection is: a project speaking for itself, or one worktree of it.
///
/// Built ONLY from the project a SECRET resolved to plus the lane the bridge named. That
/// construction is the entire security argument: the project half never comes from the wire, so a
/// bridge naming a lane can only ever reach a lane of the project it has already proved it is. A
/// lane is an address, never a credential.
///
/// `Ord` puts `None` before `Some(_)`, so a project sorts immediately above its own lanes and the
/// list the operator reads comes out grouped without anything sorting it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Addr {
    pub project: ProjectId,
    pub lane: Option<LaneId>,
}

impl Addr {
    /// The project speaking for itself — which is every bridge shipped before lanes existed.
    pub fn project_itself(project: ProjectId) -> Self {
        Self {
            project,
            lane: None,
        }
    }

    /// One worktree of a project, speaking for itself.
    pub fn lane_of(project: ProjectId, lane: LaneId) -> Self {
        Self {
            project,
            lane: Some(lane),
        }
    }

    /// The lane as a log field, and a dash where there is none.
    ///
    /// Always present, so finding one lane's lines is a search rather than a guess about which
    /// lines left the field out on purpose.
    fn lane_field(&self) -> &str {
        self.lane.as_ref().map_or("-", |l| l.as_str())
    }
}

impl std::fmt::Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.lane {
            None => write!(f, "{}", self.project),
            Some(lane) => write!(f, "{}/{lane}", self.project),
        }
    }
}

/// Is this a name the hub can safely address a conversation by?
///
/// Refused rather than sanitised, and refused before a claim is taken, before a topic exists and
/// before a byte is audited. Five things it stops, and only the first is obvious:
///
/// * A tab or a newline FORGES A LINE IN THE AUDIT. That file is one tab-separated record per line
///   and it interpolates its subject exactly as given, so a lane carrying either writes records of
///   its own choosing into the one place an incident is read from.
/// * Any other control character reaches a topic title and the journal, where it is invisible and
///   can reorder what a person reads.
/// * A separator or a `..` is refused because nothing here joins a lane onto a path today, and the
///   cheapest moment to close that door is before anyone is tempted to.
/// * An empty name is not "no lane" — sending none is. A conversation with no name is not one the
///   hub can address, and guessing what was meant is how the wrong agent gets an answer.
/// * A lone `-` IS the project's own voice on disk. Both file trees write a project speaking for
///   itself under that segment (`media.rs`), so a lane admitted under the name would be handed the
///   project's own two directories: it would read every screenshot he sent the project's own
///   session out of a mount given to it for its own, and write into the outbox the hub uploads
///   from under the project's name. `docs/ATTACHING.md` §2 already takes `-` for "as if this
///   variable were not set", so nothing going through the namespace can ask for one — but `hello`
///   carries the lane itself, and §14 invites a stranger to write an adapter from §6 alone, which
///   touches no variable. The shape rule is what makes the collision unreachable from the wire.
fn lane_is_addressable(lane: &LaneId) -> bool {
    let s = lane.as_str();
    !s.is_empty()
        && s.len() <= MAX_LANE
        && s != "."
        && s != ".."
        && s != "-"
        && !s.contains('/')
        && !s.contains('\\')
        && !s.chars().any(char::is_control)
}

/// Is this a handle the hub can safely write down beside an intention?
///
/// [`lane_is_addressable`]'s rule, applied to the other opaque handle this wire carries, and the
/// first reason it gives is the whole of this one: the audit is one tab-separated record per line
/// and it interpolates a spec id exactly as given, so a spec carrying a tab or a newline would
/// write records of its own choosing into the one file an incident is read from. A control whose
/// spec fails here is dropped at admission, before a claim holds it and long before an intention
/// could name it.
///
/// Two differences from a lane's rule, both deliberate. Nothing joins a spec id onto a path today,
/// so the separators and `..` are refused for the cheaper reason: closing the door before anyone is
/// tempted to. And `-` is refused for a narrower reason than a lane's — it is what every log field
/// in this file writes for "there is none", so a spec named `-` would read in the audit as a spec
/// nobody named.
fn spec_is_addressable(spec: &SpecId) -> bool {
    let s = spec.as_str();
    !s.is_empty()
        && s.len() <= MAX_SPEC
        && s != "."
        && s != ".."
        && s != "-"
        && !s.contains('/')
        && !s.contains('\\')
        && !s.chars().any(char::is_control)
}

/// What of a connection's declaration the hub will actually act on.
///
/// **Filtered, never refused** — which is `confirms`' rule and its argument: a controller shipped
/// after this hub names an op this build has never heard of, and a hub that closed the connection
/// over it would be a controller that cannot connect because it was newer. What was dropped is
/// readable in the `welcome`'s echo, so a controller learns rather than guesses.
///
/// Three things go. A control whose spec would forge a line in the audit; a control whose domain is
/// not a name this hub will address a conversation by — the same check a `hello`'s own lane gets,
/// and the same one, so the two can never drift apart; and, from a control that survives both,
/// every op name this build does not know. A control left with no op it knows is dropped whole: it
/// authorises nothing, and echoing it back would say the hub can be asked for something it cannot.
///
/// The ops are re-spelled through [`hub_proto::frame::Op::name`] rather than kept as they arrived,
/// so the one spelling of a verb in this process is the crate's. A duplicate declaration is left
/// alone here on purpose: `control_for` declines to guess which of two bounds a controller meant,
/// and the echo shows both, which is how the controller finds out.
fn admit_controls(declared: &Option<Vec<Control>>, addr: &Addr) -> Option<Vec<Control>> {
    let admitted: Vec<Control> = declared
        .iter()
        .flatten()
        .filter(|c| {
            if !spec_is_addressable(&c.spec_id) {
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(),
                    "a declared control names a spec this hub will not write down; dropped"
                );
                return false;
            }
            match &c.domain {
                Some(domain) if !lane_is_addressable(domain) => {
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(),
                        "a declared control names a domain this hub will not address a \
                         conversation by; dropped"
                    );
                    false
                }
                _ => true,
            }
        })
        .filter_map(|c| {
            let allowed: Vec<String> = c
                .allowed
                .iter()
                .filter_map(|name| Op::named(name).map(|op| op.name().to_owned()))
                .collect();
            // A control this hub could ask for nothing under is not a control.
            (!allowed.is_empty()).then(|| Control {
                allowed,
                ..c.clone()
            })
        })
        .collect();
    // An empty list and silence are one meaning, and the wire has one spelling for it: a
    // controller reads the ABSENCE of an echo as "this hub is older than I am", so an echo of
    // nothing must not be sent to a bridge that declared nothing.
    (!admitted.is_empty()).then_some(admitted)
}

/// What the registry file looks like from outside, for noticing that it changed.
///
/// The inode is the load-bearing part: the registry is written by temp-and-rename, so every save is
/// a new inode whatever the size and however coarse the clock. A file that is not there is a
/// fingerprint of its own rather than an error, so a registry that appears later is noticed too.
fn registry_fingerprint(path: &Path) -> Option<(u64, u64, Option<std::time::SystemTime>)> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path).ok()?;
    Some((m.ino(), m.len(), m.modified().ok()))
}

/// Why a connection was turned away before it became a project.
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Admitted(Addr),
    /// Closed without a reply. A refusal is information, and this one is not owed.
    ClosedSilently,
    Refused(RefusedReason),
}

/// The hub: one socket, one registry, one claim per conversation.
pub struct Hub<S: Surface> {
    pub surface: Arc<S>,
    pub registry: Arc<Mutex<Registry>>,
    /// Where the channel keeps each conversation's secret and its optional title. The hub reads
    /// exactly one thing there — the title, once per run, the first time it composes the
    /// conversation's name — and writes nothing.
    conversations: crate::conversations::ChannelHome,
    /// Each conversation's title as it was the FIRST time this hub composed its name, kept for
    /// the life of the process and never re-read.
    ///
    /// "Read once, at topic creation" is the promise that keeps the title file a small widening:
    /// display only, and whoever holds a room cannot change what the log's subject, the throttle
    /// notice or a lane's topic call it after the topic exists. The first version read the file
    /// afresh on every admission and every throttled message, so the topic and the notice could
    /// disagree. A rename on his phone sticks for ever anyway, because the hub never renames a
    /// topic; a title changed after the fact shows up, at most, after the hub restarts.
    titles: std::sync::Mutex<BTreeMap<ProjectId, Option<String>>>,
    pub ledger: Arc<Mutex<AskLedger>>,
    pub audit: Arc<HubAudit>,
    /// One live connection per ADDRESS, not per project. Two worktrees of one repo are two agents
    /// that can each block on a question of their own, and the second used to be turned away.
    claims: Arc<Mutex<BTreeMap<Addr, Claim>>>,
    /// The claims map, written down for a process that is not this one.
    ///
    /// `herdr-tg projects --json` runs at a terminal and has to say who is connected from the one
    /// place that knows, which is here. Rewritten whole every time the map changes — DECIDED under
    /// the claims lock and written with it let go, which is [`WhoIsConnected`]'s whole reason to
    /// exist. See `presence.rs` for why a file and what a reader must check before believing it.
    presence: crate::presence::Presence,
    /// The order the claims map has actually changed in.
    ///
    /// Minted under the claims lock, so these numbers are handed out in the order the map really
    /// changed. Nothing else about them is ordered and nothing else needs to be — the lock is what
    /// gives them their meaning, and `Relaxed` is honest about the atomic contributing nothing
    /// beyond a distinct number.
    presence_order: AtomicU64,
    /// Whose turn it is to put the claims map on disk, and the highest [`WhoIsConnected::at`] that
    /// has already been taken to the disk.
    ///
    /// The number lives INSIDE the permit because the permit is the only thing allowed to read or
    /// change it: one writer at a time, and each one decides against what the last one did.
    ///
    /// Taken with the claims lock let go, never inside it — that is the fix — and it never reaches
    /// for the claims lock itself. That direction is deliberate: a writer that re-read the map
    /// under this permit would be the neater code and would deadlock the whole hub the first time
    /// a call site kept hold of the claims lock while asking for a write.
    presence_writes: Arc<Mutex<u64>>,
    /// One outbound budget per chat, because Telegram's ceiling is per chat and forum topics do
    /// not get one of their own. Six busy projects share it.
    budgets: Arc<Mutex<crate::queue::Budgets>>,
    /// Whose turn it is to send. Held for the whole of one send's pacing wait, so that projects
    /// queue for the rhythm instead of racing for it — see `send_into` for what racing cost.
    ///
    /// **This is the whole of the queue's fairness, and the guarantee is documented rather than
    /// hoped for**: `tokio::sync::Mutex` hands the lock out strictly first-come-first-served, so a
    /// turn belongs to whoever asked for it first. Nothing here sorts, prioritises or rations by
    /// project, and nothing needs to — arrival order is already what stops one project's backlog
    /// pushing another project's single line behind it. It mattered less while a wait was capped at
    /// ten seconds and the queue stayed shallow; it is the load-bearing property now that a frame
    /// can wait a minute and a half.
    send_permit: Arc<Mutex<()>>,
    /// The one line in the forum that says the chat is carrying more than it will take.
    throttle: Arc<Mutex<Throttle>>,
    /// When Telegram last refused to make a topic for an address.
    ///
    /// `topic_for` runs on EVERY message, so without a memory a conversation whose topic cannot be
    /// made asked Telegram for one per message, with no backoff, outside the budget. An entry is
    /// dropped the moment a topic is made, so this only ever holds addresses that are currently
    /// failing.
    topic_refused: Arc<Mutex<BTreeMap<Addr, std::time::Instant>>>,
    /// The one-line gist put above a question, when one is configured.
    ///
    /// `None` unless the operator has set it up, and that default matters: a gist is the only thing
    /// in this binary that sends an agent's words to a model, so it is off until someone says
    /// otherwise. `summarize.rs` proves the endpoint is on this machine before a single character
    /// of the agent's text is on the wire.
    ///
    /// One call site, agent to operator, never the reverse.
    gist: Option<Arc<crate::summarize::Summarizer>>,
    /// What of HIS went down to a bridge and may still be answered for: his typed words, and his
    /// taps.
    ///
    /// The wire lets a bridge answer a `message` with `ack{status, reason}`, and until this
    /// existed the hub read the status of no ack at all — so an adapter with nothing to hand the
    /// words to could say so, honestly, on the wire, and he was told nothing. His taps joined them
    /// for the same reason from the other side: "Sent" is what the queue knows, and a tap the
    /// bridge could not act on read "Sent" on his phone for ever. Bounded at [`DOWN_KEPT`], oldest
    /// first out.
    down: Arc<Mutex<VecDeque<Down>>>,
    /// The intentions this hub has carried and is still waiting to hear about, newest last.
    ///
    /// The idempotency ledger and the outcome ledger are ONE list, for `Down`'s reason: an outcome
    /// and a repeat are two questions about the same intention, and two lists keyed on one thing is
    /// two places to look it up and one of them to win. Bounded at [`intent::INTENTS_KEPT`], and
    /// written down — [`IntentLedger`] holds what survives a restart and what deliberately does not.
    intents: Arc<Mutex<IntentLedger>>,
    /// The highest run number handed out for each address, and the file it survives a restart in.
    ///
    /// A `std::sync::Mutex` and not an async one, deliberately: it is taken INSIDE the claims lock
    /// at the mint, and inside [`Self::run_number_writes`] at the write, and never held across an
    /// await in either — so there is one lock order and no way to build a cycle out of it.
    generations: Arc<std::sync::Mutex<Generations>>,
    /// Whose turn it is to put those numbers on disk, and it is held across the SNAPSHOT as well as
    /// the write.
    ///
    /// `mark_permit`'s problem, on the one file whose whole job is never to go backwards. Two
    /// claims a millisecond apart each read the map and then reach the disk; without a turn between
    /// them the one that read FIRST can land SECOND, and the file is left holding a floor below
    /// numbers this hub has already handed out — which is the single failure the file exists to
    /// prevent, written by the code that exists to prevent it. Taken outside the claims lock, never
    /// inside it, which is the whole of the fix: a claim waits here for its own number to land, and
    /// nothing else in the process waits with it.
    run_number_writes: Arc<Mutex<()>>,
    /// How long a promising bridge has to confirm a tap. [`TAP_CONFIRM_WINDOW`], except under the
    /// tests that have to watch the window bite — waiting the real twenty seconds out is twenty
    /// seconds of a suite doing nothing, which is the reason `settle` is a field too.
    tap_confirm_window: AtomicU64,
    /// The order the marks on his messages land in.
    ///
    /// A tool server acks a `message` within a millisecond of reading it, while the eyes are an
    /// HTTPS call of a hundred and fifty. Two reactions in flight on one message land in whichever
    /// order Telegram takes them, and the eyes landing last leave him looking at "handed on" for a
    /// line the agent already has — for good, since nothing marks it again. So `relay` takes this
    /// before the frame goes down and hands it to the task that lands the eyes, which drops it
    /// when they have; the ack's mark waits its turn. Marks are one per line he types, so a permit
    /// costs nobody anything.
    mark_permit: Arc<Mutex<()>>,
    /// The order the edits of his receipt for a tap land in.
    ///
    /// `mark_permit`'s problem again, on a line of text instead of a reaction. Two things edit one
    /// receipt and neither can see the other: the silence window, and the session's own answer.
    /// Each read the record, let the lock go, and only then reached Telegram — so an answer
    /// arriving while the window's edit was in flight decided second and landed FIRST, and he was
    /// left reading "the session has not confirmed it took your answer" about a tap the agent had
    /// taken, for good, since nothing edits it again. The window is twenty seconds and a worker
    /// that takes about twenty seconds is the calibration point, not a corner.
    ///
    /// Held across the read AND the edit, so decision order is landing order. A tap is one thumb
    /// on one phone, so a permit costs nobody anything.
    tap_edits: Arc<Mutex<()>>,
    /// What may still be spent on reactions this minute — their own ledger, never the send budget.
    ///
    /// Measured 5 September (`docs/RATE-PROBE.md` §3): twenty reactions in a trailing minute, the
    /// twenty-first refused for the rest of it, and a send still going through beside them. So a
    /// reaction takes nothing from an agent, and past its own ceiling the hub stops asking rather
    /// than walking every later mark into a `429`.
    reactions: Arc<Mutex<crate::queue::ReactionBudget>>,
    /// Whether the journal has been told that Telegram refuses this bot's reactions.
    ///
    /// A mark refused for the ceiling is expected and logged at debug; one refused for anything
    /// else — a forum whose settings allow no reactions, or none of these three, or a bot with no
    /// right to react — is the whole receipt silently absent, and at debug the journal said
    /// nothing. Said once at warn, not once per line he types: the cause does not change between
    /// lines, and a journal that repeats one sentence a hundred times is one nobody reads.
    reaction_refusal_said: Arc<AtomicBool>,
    /// Which chats this bot answers. Checked first, before any state is touched.
    allowed_chats: Arc<Vec<i64>>,
    /// The people who may speak ANYWHERE this bot listens: the operator, and whoever he listed
    /// beside himself. A project's own people live in the registry, not here. Checked second, for
    /// every typed line and every tap, and never learned from a message.
    people: Arc<BTreeSet<i64>>,
    /// The one forum every topic lives in. Routing is a single rule — topic, inside this chat —
    /// and every other rule this bridge used to have is deleted rather than tested against.
    forum_chat: i64,
    /// Where what he sends is written, one directory per conversation. Beside the audit log, so
    /// every state file of this hub lives in one directory — and so a test's hub writes into its
    /// own temp dir rather than into the operator's.
    media: crate::media::MediaStore,
    /// Where an agent's adapter puts a file it wants him to see, one directory per conversation,
    /// named to the adapter at `welcome`. The untrusted side: nothing in it is sent until it has
    /// been opened following no link and found to be a regular file of the hub's own.
    outbox: crate::media::MediaStore,
    settle: Duration,
    /// How long one of his files may take to fetch. [`FETCH_DEADLINE`], except under the test
    /// that has to watch a Telegram which never answers be given up on — held as a number rather
    /// than a constant for the same reason `MediaStore`'s owner is: the only way to see a bound
    /// bite is to move it, and waiting the real one out is a minute of a test suite doing nothing.
    fetch_deadline: AtomicU64,
    /// How many frames, and how many bytes of them, are held for a bridge before its pong. The
    /// constants, except under a test that has to watch a real bridge trip the bound.
    pre_pong: (usize, usize),
}

impl<S: Surface> Hub<S> {
    pub fn new(
        surface: Arc<S>,
        registry: Registry,
        ledger: AskLedger,
        audit: HubAudit,
        allowed_chats: Vec<i64>,
        people: Vec<i64>,
        forum_chat: i64,
    ) -> Self {
        // Beside the audit log, because every state file of this hub lives in one directory and
        // the audit's path is the one this constructor is already handed. Written EMPTY at once:
        // a hub that has just started has nothing connected, and until it says so the file on disk
        // is the last hub's, naming that hub's pid — which a reader rightly refuses to believe, and
        // reports as unknown for as long as it is left there.
        let audit_path = audit.path().to_path_buf();
        let presence =
            crate::presence::Presence::new(audit.path().with_file_name(crate::presence::FILE));
        if let Err(e) = presence.write(std::iter::empty()) {
            tracing::error!(
                error = %e, path = %presence.path().display(),
                "could not write down that nothing is connected yet; `projects --json` will say \
                 unknown until a bridge arrives or leaves"
            );
        }
        // Made at start, because `docs/ATTACHING.md` §14.1 tells a dispatcher they are there from
        // then on — and because a root somebody else owns, or one left wider than 0700, then
        // reaches the journal in the first second rather than on the first screenshot he sends.
        //
        // Swept at start as well as before every write, so a hub that was down for a week does
        // not keep a week-old mailbox for exactly as long as nobody sends anything.
        let media = crate::media::MediaStore::new(audit.path().with_file_name("media"));
        media.make_the_tree();
        media.sweep(std::time::SystemTime::now());
        let outbox = crate::media::MediaStore::new(audit.path().with_file_name("outbox"));
        outbox.make_the_tree();
        outbox.sweep(std::time::SystemTime::now());
        // The same directory every other state file lives in, derived from the one path this
        // constructor is handed — so the hub and the terminal verbs cannot disagree about where a
        // conversation's title is.
        let conversations =
            crate::conversations::ChannelHome::at(audit.path().parent().unwrap_or(Path::new(".")));
        Self {
            surface,
            registry: Arc::new(Mutex::new(registry)),
            conversations,
            titles: std::sync::Mutex::new(BTreeMap::new()),
            ledger: Arc::new(Mutex::new(ledger)),
            audit: Arc::new(audit),
            claims: Arc::new(Mutex::new(BTreeMap::new())),
            presence,
            presence_order: AtomicU64::new(0),
            presence_writes: Arc::new(Mutex::new(0)),
            budgets: Arc::new(Mutex::new(crate::queue::Budgets::default())),
            send_permit: Arc::new(Mutex::new(())),
            throttle: Arc::new(Mutex::new(Throttle::default())),
            topic_refused: Arc::new(Mutex::new(BTreeMap::new())),
            gist: crate::summarize::Summarizer::from_env().map(Arc::new),
            down: Arc::new(Mutex::new(VecDeque::new())),
            intents: Arc::new(Mutex::new(IntentLedger::load(
                audit_path.with_file_name(INTENTS_FILE),
            ))),
            generations: Arc::new(std::sync::Mutex::new(Generations::load(
                audit_path.with_file_name(GENERATIONS_FILE),
            ))),
            run_number_writes: Arc::new(Mutex::new(())),
            tap_confirm_window: AtomicU64::new(TAP_CONFIRM_WINDOW.as_millis() as u64),
            mark_permit: Arc::new(Mutex::new(())),
            tap_edits: Arc::new(Mutex::new(())),
            reactions: Arc::new(Mutex::new(crate::queue::ReactionBudget::default())),
            reaction_refusal_said: Arc::new(AtomicBool::new(false)),
            allowed_chats: Arc::new(allowed_chats),
            people: Arc::new(people.into_iter().collect()),
            forum_chat,
            fetch_deadline: AtomicU64::new(FETCH_DEADLINE.as_millis() as u64),
            media,
            outbox,
            settle: DEFAULT_SETTLE,
            pre_pong: (PRE_PONG_FRAMES, PRE_PONG_BYTES),
        }
    }

    /// Shorten the live-window for tests. Test-only on purpose: five seconds is the number that
    /// separates a real bridge from a channel plugin that booted and exited, and a knob for it in
    /// production is a knob someone eventually turns down to zero to make a flake go away.
    #[cfg(test)]
    pub fn with_settle(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }

    /// Give up on one of his files sooner than [`FETCH_DEADLINE`]. Test-only: the real bound is a
    /// minute, and a test that waited it out would be a minute of a suite doing nothing.
    #[cfg(test)]
    pub fn give_up_fetching_after(&self, how_long: Duration) {
        self.fetch_deadline
            .store(how_long.as_millis() as u64, Ordering::SeqCst);
    }

    /// Hold less before the pong than [`PRE_PONG_FRAMES`] and [`PRE_PONG_BYTES`] say. Test-only:
    /// the bound is exactly what a conforming bridge may be holding, so the only way to watch a
    /// REAL bridge trip it is to lower it — and a knob for that in production is a knob someone
    /// turns down to make a flood go away, which turns an honest refusal into a routine one.
    #[cfg(test)]
    pub fn with_pre_pong_hold(mut self, frames: usize, bytes: usize) -> Self {
        self.pre_pong = (frames, bytes);
        self
    }

    /// Decide whether a `hello` may become a project.
    ///
    /// Split out from the connection so the decision can be tested without a transport at all, and
    /// so the order of the gates is visible in one place. It takes the identity rather than a pair
    /// of uids because "is this peer allowed here" is the transport's question to answer — over a
    /// gateway it would not be a uid comparison, and the gates below must not have to care.
    pub async fn admit(
        &self,
        who: &ConnectionIdentity,
        hello: &BridgeFrame,
        version: u16,
    ) -> Admission {
        if !who.is_this_user() {
            tracing::warn!(
                peer_uid = who.uid_for_the_log(),
                "a connection from another user was closed without a reply"
            );
            return Admission::ClosedSilently;
        }
        if version != VERSION {
            return Admission::Refused(RefusedReason::VersionSkew);
        }
        let BridgeFrame::Hello {
            token, pid, lane, ..
        } = hello
        else {
            // The first frame must be `hello`. Anything else is a bridge that does not speak this
            // protocol, and letting it continue would mean guessing which project it is.
            return Admission::Refused(RefusedReason::UnknownProject);
        };

        let (id, enabled) = {
            let mut registry = self.registry.lock().await;
            // Fresh, every admission. A secret rotated at the terminal has to take effect on the
            // next connection, not on the next restart — otherwise rotating a LEAKED secret leaves
            // the leaked one working and locks the honest bridge out, which is the opposite of what
            // rotation is for.
            //
            // A failed re-read leaves the previous map in place and admits against THAT. The
            // alternative — refusing everyone because one read failed — turns a transient file
            // error into a fleet-wide outage, and the alternative before that, adopting an empty
            // map, silently un-enrolled every project.
            if let Err(e) = registry.reread() {
                tracing::error!(
                    error = %e,
                    "could not re-read the project registry; admitting against the last good copy"
                );
            }
            match registry.resolve(token) {
                None => return Admission::Refused(RefusedReason::UnknownProject),
                Some(p) => (p.id.clone(), p.enabled),
            }
        };
        if !enabled {
            return Admission::Refused(RefusedReason::NotEnabled);
        }

        // The lane is checked HERE, after the secret has resolved, and the order is deliberate: a
        // caller holding no valid secret learns only "unknown project" and never the difference
        // between a bad lane and a bad token.
        if let Some(lane) = lane
            && !lane_is_addressable(lane)
        {
            tracing::warn!(
                project = %id, bytes = lane.as_str().len(),
                "a bridge named a lane the hub will not address a conversation by; refusing it"
            );
            return Admission::Refused(RefusedReason::BadLane);
        }

        let _ = pid;
        // The address, minted from the RESOLVED project and the wire's lane. Never from the wire's
        // `project_id`, which is exactly why a bridge naming a lane cannot reach another project's.
        Admission::Admitted(Addr {
            project: id,
            lane: lane.clone(),
        })
    }

    /// Is this chat one the bot answers?
    ///
    /// Runs first, before command parsing and before any state is touched. Empty answers nobody:
    /// the opposite convention would turn a misconfiguration into an open bot.
    pub fn chat_is_allowed(&self, chat_id: i64) -> bool {
        self.allowed_chats.contains(&chat_id)
    }

    /// May this person speak here — and if so, here only, or anywhere?
    ///
    /// `user` is the sender as the bot established it: `None` when the update carried no person
    /// it could vouch for (a channel post, a bot, an anonymous admin posting as the group), and
    /// `None` is a stranger, always. `at` is the conversation the words or the tap belong to, when
    /// there is one; a line in General, a command and a tap on a button nothing was written down
    /// beside have none, and for those only the bot-wide list can answer.
    ///
    /// The bot-wide list is read first because it needs no lock. A project's own people are read
    /// from the registry copy this process holds, which the watcher refreshes within about a
    /// second of a terminal write — so `herdr-tg allow` is live without a restart, and there is no
    /// re-read here on the hot path of every line he types.
    pub async fn standing_of(&self, user: Option<i64>, at: Option<&Addr>) -> Standing {
        // Fail closed on the shape of the number itself, before any list is consulted: nothing on
        // any list can be zero or negative — the parser and the setter both refuse them — and
        // refusing here as well means a hand-edited file cannot make "no sender" match.
        let Some(user) = user.filter(|u| crate::config::is_a_persons_id(*u)) else {
            return Standing::Stranger;
        };
        if self.people.contains(&user) {
            return Standing::Anywhere;
        }
        let Some(at) = at else {
            return Standing::Stranger;
        };
        let registry = self.registry.lock().await;
        if registry
            .get(&at.project)
            .is_some_and(|p| p.allowed_users.contains(&user))
        {
            Standing::InThisConversation
        } else {
            Standing::Stranger
        }
    }

    /// Turn a tap into an answer, or into a sentence saying why not.
    pub async fn resolve_tap(
        &self,
        chat_id: i64,
        user: Option<i64>,
        msg_id: &MsgId,
        option_id: &OptionId,
    ) -> Result<(Addr, AskId, OptionId), TapRefusal> {
        // The allowlist first. A tap from a chat this bot does not answer must not even reach the
        // ledger — and it gets silence, not a refusal, because a refusal is a reply.
        if !self.chat_is_allowed(chat_id) {
            return Err(TapRefusal::NotYours);
        }

        let record = {
            let ledger = self.ledger.lock().await;
            ledger.get(chat_id, msg_id).cloned()
        };
        // The person second, and BEFORE "no record": Telegram lets anyone who can see an inline
        // keyboard tap it, and the ledger writes down which topic a question was asked in, never
        // who may answer it — so until this check, anyone in the forum could answer "overwrite
        // it?" for the agent with every appearance of being the operator. Checked here, with the
        // record's conversation in hand, so a project's own people can answer their project's
        // questions; and before the record is judged, so a stranger learns nothing from the
        // difference between a button that was written down and one that was not.
        let at = record.as_ref().map(AskRecord::addr);
        if !self.standing_of(user, at.as_ref()).await.may_speak_here() {
            let _ = self.audit.stranger(user, chat_id, at.as_ref(), "a tap");
            return Err(TapRefusal::NotYours);
        }
        let Some(record) = record else {
            return Err(TapRefusal::NoRecord);
        };
        if !record.options.iter().any(|o| &o.option_id == option_id) {
            return Err(TapRefusal::NotAnOption);
        }
        if let Some(why) = record.refusal_if_closed() {
            return Err(why);
        }

        // The connection is checked BEFORE the record is marked, so a tap that could not be
        // delivered leaves the question answerable — refusing it and then closing it would burn the
        // operator's only way to answer.
        //
        // Looked up by the ADDRESS the record carries, so the lane travels in the record rather
        // than being re-derived here. Keyed on the project alone this read either handed back some
        // arbitrary lane's connection — a `Choice` delivered into a turn that never asked anything,
        // with no error anywhere — or missed and called a lane not connected while it sat waiting.
        let addr = record.addr();
        {
            let claims = self.claims.lock().await;
            let Some(claim) = claims.get(&addr) else {
                return Err(TapRefusal::NotConnected);
            };
            // Within one lane this still means what it always meant. Across lanes it means nothing
            // at all — the opencode adapter is one process holding every lane, so its lanes share
            // an instance and this check passes between them. The address above is what separates
            // them; this separates two runs of the same one.
            if claim.instance != record.instance {
                return Err(TapRefusal::Restarted);
            }
        }

        // Marked HERE, under the ledger lock, as part of resolving. Doing it after the caller has
        // delivered would leave a window in which a second tap resolves too — and the window is
        // exactly as long as a Telegram round trip, on a keyboard the operator is still looking at.
        //
        // Judged AGAIN under the same lock, and against the same predicate as above. The record
        // read at the top was a copy, and the lock was let go between then and now: a second tap,
        // or the terminal's own `ask_resolved`, may have closed the question in between. That one
        // closes under this same lock, so whatever it wrote is what this read sees, and there is
        // no order of the two in which both go through.
        {
            let mut ledger = self.ledger.lock().await;
            match ledger.get(chat_id, msg_id) {
                None => return Err(TapRefusal::NoRecord),
                Some(fresh) => {
                    if let Some(why) = fresh.refusal_if_closed() {
                        return Err(why);
                    }
                }
            }
            if let Err(e) = ledger.mark_answered(chat_id, msg_id, option_id) {
                // Fail closed: if the answer cannot be written down, it must not be sent. An
                // unrecorded answer is one that can be given again.
                tracing::error!(error = %e, "could not write down that a question was answered");
                return Err(TapRefusal::NoRecord);
            }
        }
        Ok((addr, record.ask_id, option_id.clone()))
    }

    /// Hand a frame to a project's live connection.
    ///
    /// The sender is CLONED out and the guard dropped before anything is awaited. Holding the
    /// claims lock across the send was a fleet-wide stall waiting to happen: the outbox is bounded,
    /// so one bridge that stopped reading would park this `.await` with the lock held, and every
    /// other project's admit, claim, release and deliver would queue behind it. One wedged bridge,
    /// every project silent.
    ///
    /// `try_send` rather than `send`, for the same reason from the other direction: a full outbox
    /// means that bridge is not keeping up, and the honest answer is "not delivered" now rather
    /// than an await that might never finish.
    /// Test-only since the tap grew a record of its own: `deliver_tap` is what `bot.rs` uses, and
    /// it mints the id BEFORE the frame goes down so a bridge's answer for it has something to
    /// find. This is the plain form, kept for the round-trip test that only wants a frame to
    /// arrive.
    #[cfg(test)]
    pub async fn deliver(&self, addr: &Addr, frame: HubFrame) -> bool {
        self.deliver_under(addr, Self::mint_frame_id(), frame).await
    }

    /// The envelope id the next frame down goes under — the id a bridge's `ack` for it will name.
    /// Minted apart from the send so the one caller that waits to hear what became of a frame can
    /// write the id down BEFORE the frame is on the wire.
    fn mint_frame_id() -> FrameId {
        FrameId::new(format!("h{}", next_frame_seq()))
    }

    /// Send one frame down under an id the caller already holds.
    ///
    /// Stamped with the lease of the run it is going to, like every other frame the hub sends —
    /// and these are the two the product exists for. Leaving `Message` and `Choice` the only
    /// unstamped frames on the wire made the rule the adapters are written to ("everything after
    /// the welcome carries the lease") false exactly where it matters: a bridge that judges what
    /// it is handed by the stamp would drop his typed words and his taps and keep the acks.
    async fn deliver_under(&self, addr: &Addr, id: FrameId, frame: HubFrame) -> bool {
        let (tx, generation) = {
            let claims = self.claims.lock().await;
            match claims.get(addr) {
                None => return false,
                Some(claim) => (claim.tx.clone(), claim.generation),
            }
        };
        match tx.try_send(Envelope::new(id, frame).with_generation(generation)) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(), error = %e,
                    "a bridge is not keeping up; not delivered"
                );
                false
            }
        }
    }

    /// Take the address for a run that says nothing about itself.
    ///
    /// Test-only, and it is the shape of a bridge from before either field existed: it names no
    /// generation and promises to confirm nothing. `serve_connection` goes through
    /// [`Self::claim_the_address`], which is handed what the arriving `hello` actually said.
    #[cfg(test)]
    pub async fn claim(
        &self,
        addr: Addr,
        pid: u32,
        instance: String,
        tx: mpsc::Sender<Envelope<HubFrame>>,
    ) -> Result<mpsc::Receiver<Kick>, RefusedReason> {
        self.claim_declaring(addr, pid, instance, tx, None).await
    }

    /// The same, for a run that declares what it can be asked to do.
    ///
    /// Test-only beside [`Self::claim`] and delegating to the same critical section, because two
    /// ways to take an address is two fences to keep in step. It exists so a test can put a
    /// CONTROLLER behind a connection this process controls the outbox of — which is the only way
    /// to watch an intention be refused because the controller was not keeping up.
    #[cfg(test)]
    pub async fn claim_declaring(
        &self,
        addr: Addr,
        pid: u32,
        instance: String,
        tx: mpsc::Sender<Envelope<HubFrame>>,
        controls: Option<Vec<Control>>,
    ) -> Result<mpsc::Receiver<Kick>, RefusedReason> {
        // A run that names no generation and promises to confirm nothing — which is every bridge
        // shipped before either field existed, and the shape the room-map tests hold a claim with.
        self.claim_the_address(
            addr,
            pid,
            instance,
            tx,
            Arriving {
                generation: None,
                confirms_choices: false,
                controls,
                speaks_generations: Arc::new(AtomicBool::new(false)),
            },
        )
        .await
        .map(|(_, _, kicked)| kicked)
    }

    /// Take the address, or refuse — the fence, the check and the reservation in ONE critical
    /// section.
    ///
    /// The check and the reservation used to be two: `admit` looked for a live incumbent, dropped
    /// the lock, and `claim` inserted unconditionally some awaits later. Two bridges arriving
    /// inside that window were both admitted and the second silently replaced the first — measured
    /// at roughly one round in three when the two `hello`s land within about 100 µs on a
    /// multi-thread runtime, which is the runtime this binary builds. The consequence is the exact
    /// failure gate 4 exists to prevent: two bridges live on one project, both posting into one
    /// topic, and a tap on the incumbent's still-open question refused with "that session has since
    /// restarted" while it is sitting there waiting for the answer.
    ///
    /// A dead incumbent is evicted rather than honoured: a worker that crashed must not lock its
    /// own project out until someone finds a keyboard. The mint is in here for the same reason the
    /// reservation is — a number handed out beside a claim taken under a different lock is a number
    /// two runs can be given.
    ///
    /// Hands back the generation minted for this run — the number its `welcome` carries and the
    /// number every later frame of that connection is judged against — and the receiver the
    /// connection must listen on for a [`Claim::kick`], which is the one way it can be ended by
    /// something other than its own socket.
    async fn claim_the_address(
        &self,
        addr: Addr,
        pid: u32,
        instance: String,
        tx: mpsc::Sender<Envelope<HubFrame>>,
        said: Arriving,
    ) -> Result<(u64, u64, mpsc::Receiver<Kick>), RefusedReason> {
        let Arriving {
            generation: arriving,
            confirms_choices,
            controls,
            speaks_generations,
        } = said;
        let (kick, kicked) = mpsc::channel(1);
        let generation;
        let highest_was;
        let who;
        // The run that was evicted, for the sweep after the lock — which is scoped to THAT run
        // and not to the address, because by then the address is the successor's.
        let evicted_run: Option<u64>;
        // The address is moved into the claims map below, so the sweep after the lock keeps its
        // own copy rather than the map's.
        let for_the_sweep = addr.clone();
        {
            let mut claims = self.claims.lock().await;
            let highest = self
                .generations
                .lock()
                .expect("the generations are not held across an await")
                .highest_for(&addr);
            highest_was = highest;
            // THE RECLAIM FENCE, and it is read BEFORE the incumbent is.
            //
            // A run whose generation has been replaced is not a rival for the address — it is over.
            // Told "already claimed" it would redial for ever, because that refusal is the one a
            // bridge is supposed to wait out; and with nothing holding the address it would simply
            // be let back in, which is a second voice in a conversation that has moved on.
            //
            // Only a number BEHIND the highest is refused. One ahead of it is a run holding a
            // number this hub never minted — a state directory restored from elsewhere, a file
            // lost — and refusing that would lock a project out of its own hub with no way back
            // from a phone.
            if let Some(arriving) = arriving
                && arriving < highest
            {
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(),
                    generation = arriving, latest = highest,
                    "refused: a run from an earlier generation tried to come back after a later \
                     one had been admitted"
                );
                return Err(RefusedReason::StaleGeneration);
            }
            // Exclusive per ADDRESS. Widening it to the project was the refusal that made a second
            // worktree of one repo unreachable; widening it to nothing would be the takeover this
            // whole gate exists to refuse, so within one lane the rule is untouched.
            //
            // A dead incumbent is evicted rather than honoured — and evicting it is a KICK, not the
            // silent overwrite it used to be. Replacing the map entry alone told the old connection
            // nothing: it went on holding a writer task and draining whatever it had queued into a
            // topic a live successor now owns, until its own socket happened to end.
            let evicted = match claims.get(&addr) {
                Some(old) if fence_is_alive(old.pid) => {
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(),
                        incumbent = old.pid, arriving = pid,
                        "a second bridge tried to take a conversation that is already connected"
                    );
                    return Err(RefusedReason::AlreadyClaimed);
                }
                Some(old) => Some((
                    old.kick.clone(),
                    old.pid,
                    old.generation,
                    old.speaks_generations.load(Ordering::Acquire),
                )),
                None => None,
            };
            generation = self
                .generations
                .lock()
                .expect("the generations are not held across an await")
                .mint(&addr, arriving.unwrap_or(0));
            evicted_run = evicted.as_ref().map(|(_, _, was, _)| *was);
            if let Some((kick, dead, was, speaks)) = evicted {
                tracing::info!(
                    project = %addr.project, lane = addr.lane_field(), dead,
                    generation = was, takes = generation,
                    "evicted a run whose process is gone; a later generation takes the address"
                );
                // Told only if it can read the word. A bridge from before generations renders an
                // unknown refusal as temporary and would redial for ever; and there is nobody
                // behind a corpse's socket to tell anyway, which is what it was told before.
                let _ = kick.try_send(Kick {
                    reason: speaks.then_some(RefusedReason::StaleGeneration),
                    sentence: "a later run took the address while its own process was gone",
                    why: speaks.then_some(AckWhy::StaleGeneration),
                });
            }
            tracing::info!(
                project = %addr.project, lane = addr.lane_field(), pid, generation,
                "claimed"
            );
            claims.insert(
                addr,
                Claim {
                    pid,
                    generation,
                    instance,
                    speaks_generations,
                    confirms_choices,
                    controls,
                    tx,
                    kick,
                },
            );
            who = self.note_who_is_connected(&claims);
        }
        // An evicted run owes a word about anything it was handed and will never say it — its own
        // `release_this_run` finds the address already taken and returns without touching this.
        // Unknown, and never failed: what ended is the conversation, not the work.
        //
        // Named by its own generation, because the successor already holds the address: the lock
        // is let go above and this run's records are the only ones that are waiting on nobody.
        if let Some(was) = evicted_run {
            self.nothing_more_will_be_said_about(&for_the_sweep, was)
                .await;
        }
        // WITH THE CLAIMS LOCK LET GO, and still before the number is handed to anybody. Deciding
        // the number needs the lock — two claims racing must not be given the same one, which is
        // the whole reason the mint is up there — but writing it down needs nothing the lock holds.
        // It used to be written inside it, so a disk that was slow or full stopped every project's
        // delivery, not just this claim: `deliver_under` reads the same map to find the bridge a
        // message is going to. What waits here now is this one connection's welcome, waiting for
        // its own number to reach the disk, which is the order that was always intended.
        self.write_down_the_run_numbers().await;
        // The same move, for the file next door — and AFTER the numbers, deliberately. Both are
        // out of the lock now, so neither is in anybody else's way; what the order decides is
        // which one gets written when a disk takes a write and never finishes it. The numbers are
        // the half that matters, because losing one can hand a second run the lease of the first
        // after a restart, where losing this one only makes `projects --json` say unknown.
        self.put_who_is_connected_on_disk(who).await;
        Ok((generation, highest_was, kicked))
    }

    /// Put the numbers this hub has handed out on disk, holding no lock a delivery wants.
    ///
    /// **Awaited and read, never spawned and forgotten.** Firing the write off into a task nobody
    /// looks at would take the lock off the claim path just as well and would be wrong for a reason
    /// nothing would show: a hub whose disk is refusing writes would go on handing out numbers with
    /// nothing anywhere saying they are not being written down, and the first sign of it would be a
    /// restart re-handing a number it had already given. So the claim waits for its own number, and
    /// a write that failed is said so at `error!` in the journal.
    ///
    /// A write that fails does NOT end the claim. The map in memory is the truth for this hub's own
    /// life, so nothing can be double-handed while it runs; what a failed write costs is the belt
    /// against a clock that steps BACKWARDS across a restart, and refusing every claim on the box
    /// until somebody frees disk space would trade that rare belt for a certain silence on his
    /// phone. The next write that lands carries the whole map, so it repairs every one that did not.
    async fn write_down_the_run_numbers(&self) {
        let _turn = self.run_number_writes.lock().await;
        let (path, file) = self
            .generations
            .lock()
            .expect("the generations are not held across an await")
            .to_write_down();
        // On a thread that is nobody's worker. A blocking write inside a task does not only stall
        // that task: it parks the runtime worker it is running on, and a worker parked in a syscall
        // takes the timer wheel down with it — measured while this was being fixed, a 50 ms
        // `tokio::time::sleep` in another task never returned. Every pacing wait, every deadline
        // and every settling window in this process is that timer.
        let wrote = tokio::task::spawn_blocking({
            let path = path.clone();
            move || write_handed_out(&path, &file)
        })
        .await;
        let refused = match wrote {
            Ok(Ok(())) => return,
            Ok(Err(e)) => e.to_string(),
            // The writer panicked, which is a bug rather than a disk; said the same way, because
            // what the operator's hub does next is the same either way.
            Err(e) => e.to_string(),
        };
        tracing::error!(
            error = refused, path = %path.display(),
            "could not write down the run numbers this hub has handed out; a restart after the \
             clock steps backwards could hand one of them out again"
        );
    }

    /// Has a later run taken this address than the one asking?
    ///
    /// Reads the generations under their own lock and never the claims map, because the two
    /// answer different questions: the claims map says who is connected NOW, and after a release
    /// it is empty — which is exactly the moment a run that is already over is still draining what
    /// it said into a conversation somebody else has taken.
    fn a_newer_run_holds(&self, addr: &Addr, mine: u64) -> Option<u64> {
        let highest = self
            .generations
            .lock()
            .expect("the generations are not held across an await")
            .highest_for(addr);
        (highest > mine).then_some(highest)
    }

    /// Decide what `projects --json` should be told, with the claims lock HELD — and write nothing.
    ///
    /// The snapshot has to be taken under the lock, because a list read off a map something else
    /// is changing is a list of no moment at all. The write must not be: this file used to be put
    /// on disk right here, so a disk that was slow or full stopped every project's delivery and
    /// not just this claim — `deliver_under` reads the same map to find the bridge a message is
    /// going to. What comes back is handed to [`Self::put_who_is_connected_on_disk`] once the lock
    /// is let go, and it is `#[must_use]` so that forgetting to is a build failure rather than a
    /// file that quietly stops following the map.
    fn note_who_is_connected(&self, claims: &BTreeMap<Addr, Claim>) -> WhoIsConnected {
        WhoIsConnected {
            at: self.presence_order.fetch_add(1, Ordering::Relaxed) + 1,
            connected: claims.keys().cloned().collect(),
        }
    }

    /// Put a snapshot of who is connected on disk, holding no lock a delivery wants.
    ///
    /// **Awaited and read, never spawned and forgotten**, for [`Self::write_down_the_run_numbers`]'s
    /// reason: a hub whose disk is refusing writes would otherwise go on claiming and releasing
    /// with nothing anywhere saying the file had stopped following it. What waits is this one
    /// connection's own welcome or goodbye, and nothing else in the process waits with it.
    ///
    /// A write that fails does NOT refuse the claim, and it does not leave the last snapshot in
    /// place either — `presence::Presence::write` unlinks the file, because a stale one carries
    /// this hub's own pid and so passes every check a reader makes while naming bridges that have
    /// gone. Unknown is the honest answer; nothing connected is not.
    async fn put_who_is_connected_on_disk(&self, snapshot: WhoIsConnected) {
        let mut latest = self.presence_writes.lock().await;
        // A LATER SNAPSHOT ALWAYS WINS, and the turn above is not enough on its own to make it so.
        // Two changes to the map decide their snapshots in the order the lock hands them out and
        // then queue here in whatever order they get round to asking — the one decided FIRST can
        // easily be the one that asks SECOND, and writing it would take the file back to naming a
        // bridge that has since gone. So an older snapshot is dropped where it stands. The newest
        // one can never be the one dropped, which is what makes the file end up agreeing with the
        // map: nothing is minted above it.
        //
        // Against what has actually BEEN to the disk, and not against the newest number minted.
        // Skipping because "a newer snapshot exists, let that one write" is the tempting version
        // and it converges only for as long as every snapshot minted really does reach this
        // function — so the day a call site decided one and returned without writing it, the file
        // would stall for good rather than being one change behind. This comparison needs no such
        // promise from anywhere else, which is why it is the one here.
        if snapshot.at <= *latest {
            return;
        }
        // Marked before the write and NOT after it, and it counts a failed write too. A write that
        // failed unlinked the file; letting an older snapshot in afterwards would put a list this
        // hub has already moved past back on disk, which is worse than the unknown the unlink
        // leaves. The next change to the map writes the whole list again and repairs it.
        *latest = snapshot.at;
        // On a thread that is nobody's worker, for the reason measured at the run numbers: a
        // blocking write inside a task parks the runtime worker it is running on, and a parked
        // worker takes the timer wheel with it — every pacing wait, every deadline and every
        // settling window in this process is that timer.
        let presence = self.presence.clone();
        let wrote =
            tokio::task::spawn_blocking(move || presence.write(snapshot.connected.iter())).await;
        let refused = match wrote {
            Ok(Ok(())) => return,
            Ok(Err(e)) => e.to_string(),
            // The writer panicked, which is a bug rather than a disk; said the same way, because
            // what the operator's hub does next is the same either way.
            Err(e) => e.to_string(),
        };
        tracing::error!(
            error = refused, path = %self.presence.path().display(),
            "could not write down who is connected; the file was removed rather than left naming \
             bridges that may have gone, so `projects --json` says unknown until the next claim or \
             release writes it again"
        );
    }

    /// End every live connection whose project has been switched off at the terminal.
    ///
    /// `enabled` is read at `hello`, so the flag on its own turns away the NEXT connection and does
    /// nothing to one already on the socket — the thing the operator actually reached for the
    /// switch to stop. This is the other half: the registry is re-read, and every claim under a
    /// project that is now off is told why and ended, through the connection's own kick. The
    /// bridge gets `refused{not_enabled}` and a close, exactly what it would get dialling fresh.
    ///
    /// Called by [`Self::watch_the_registry`] whenever the file changes, so the operator's write at
    /// a terminal reaches a connection this process holds within about a second.
    pub async fn drop_connections_of_switched_off_projects(&self) {
        let off: BTreeSet<ProjectId> = {
            let mut registry = self.registry.lock().await;
            if let Err(e) = registry.reread() {
                // The last good copy has nothing new to say, and a stale read must never switch
                // anything off: refusing to act is the only safe answer to a file that cannot be
                // read, and admission already treats it the same way.
                tracing::error!(
                    error = %e,
                    "could not re-read the project registry; not switching anything off"
                );
                return;
            }
            registry
                .all()
                .filter(|p| !p.enabled)
                .map(|p| p.id.clone())
                .collect()
        };
        if off.is_empty() {
            return;
        }
        // Claims first, registry second, and never both at once — the same order every other
        // reader keeps. The kicks are cloned out and the guard dropped before anything is awaited.
        let (kicked, forgotten, who): (Vec<Addr>, Vec<(Addr, u64)>, WhoIsConnected) = {
            let mut claims = self.claims.lock().await;
            let mut kicked = Vec::new();
            // The runs this removes ITSELF, rather than through their own connection. Everything
            // else that takes a claim out of this map sweeps what that run was owed a word about;
            // this branch did not, so a record of one was left saying a live controller is working
            // on it — unsettleable by anybody, and unforgettable by the ledger's own bound.
            let mut forgotten = Vec::new();
            claims.retain(|addr, claim| {
                if !off.contains(&addr.project) {
                    return true;
                }
                // The connection is told THROUGH its kick and removes its own claim on the way out.
                // One that cannot be told — nothing listening on the far end of the kick — is
                // forgotten here instead, so a switched-off project is never shown as connected
                // and never handed his words; that shape is only ever a test's own claim.
                match claim.kick.try_send(Kick {
                    reason: Some(RefusedReason::NotEnabled),
                    sentence: "its project was switched off at the terminal",
                    // The closed set has no word for it, and a wrong one sends the agent the wrong
                    // way. The refusal above is what says why; each frame is simply not delivered.
                    why: None,
                }) {
                    Ok(()) => {
                        kicked.push(addr.clone());
                        true
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => true,
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        tracing::warn!(
                            project = %addr.project, lane = addr.lane_field(),
                            "a switched-off project's claim had nothing behind it to tell; forgotten"
                        );
                        forgotten.push((addr.clone(), claim.generation));
                        false
                    }
                }
            });
            (kicked, forgotten, self.note_who_is_connected(&claims))
        };
        // With the claims lock let go, which is the one lock order in this file: the ledger, then
        // the claims. Never failed — what ended is the conversation, not the work.
        for (addr, run) in forgotten {
            self.nothing_more_will_be_said_about(&addr, run).await;
        }
        self.put_who_is_connected_on_disk(who).await;
        for addr in kicked {
            tracing::info!(
                project = %addr.project, lane = addr.lane_field(),
                "its project was switched off at the terminal; ending its connection"
            );
        }
    }

    /// Watch the registry file and act on a project switched off at the terminal.
    ///
    /// A stat every `every`, and a re-read only when the stat says something changed: the file is
    /// rewritten by temp-and-rename, so a change is a new inode, a new size, or a new mtime, and
    /// this process's own writes (a topic binding) look the same and cost one harmless re-read.
    /// Spawned once for the life of the hub; the handle is returned so a test can hold it.
    pub fn watch_the_registry(self: Arc<Self>, every: Duration) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let path = self.registry.lock().await.path().to_path_buf();
            // Nothing seen yet, so the first tick always re-reads once. The baseline used to be
            // taken here, on the task's first run — which is not when it was spawned: a write
            // that lands between the two is baked into the baseline and never noticed, so a
            // person let in at the terminal in that window stayed a stranger until the next
            // unrelated write. One re-read of a few kilobytes at boot is the price of no window.
            let mut seen: Option<(u64, u64, Option<std::time::SystemTime>)> = None;
            loop {
                tokio::time::sleep(every).await;
                let now = registry_fingerprint(&path);
                if now == seen {
                    continue;
                }
                seen = now;
                self.drop_connections_of_switched_off_projects().await;
            }
        })
    }

    /// Take the keyboard off every question a run of this CONVERSATION other than this one left
    /// open.
    ///
    /// Run when a bridge ARRIVES, because that is the one moment the hub can prove those sessions
    /// are gone: a claim is exclusive, so nothing else holds this conversation now. "Conversation"
    /// and not "project" is load-bearing — see [`AskLedger::open_for_other_instances`], where the
    /// same widening turned this sweep into a thing that took live lanes' questions off the phone. Every other place it
    /// could have gone is wrong. At `release` a bridge that merely lost its socket would have its
    /// live questions taken off the phone, because a bridge keeps its instance across a reconnect
    /// and is still waiting for those answers. At eviction — which is where this started — it fires
    /// only when a claim was left behind by a pid that is no longer running, and every ordinary way
    /// a bridge goes away reaches `release` first, so the successor finds an empty claims map and
    /// nothing is ever swept. Sessions open when the hub itself restarted are missed the same way.
    ///
    /// Doing it here also means a retirement Telegram refused is tried again by the next session,
    /// which matters because a record left behind by a failed edit has nothing else that would ever
    /// come back to it.
    ///
    /// **Spawned, never awaited on the handshake.** It is one Telegram edit per abandoned question
    /// and nothing bounds how many there are; awaited before `Welcome`, six of them measured 1.8
    /// seconds, and the bridge cannot do anything at all until `Welcome` arrives — it holds
    /// everything the agent says until then, and starts dropping it after sixty-four. The session
    /// paying that would be the one that just came back.
    async fn retire_what_other_sessions_left(&self, addr: &Addr, instance: &str) {
        // First, the ledger's own shelf life. A question whose message Telegram will no longer let
        // anyone edit cannot have its keyboard taken off by this sweep or any later one, so it is
        // dropped rather than carried in a file every ask of every project rewrites whole.
        {
            self.ledger
                .lock()
                .await
                .drop_what_can_no_longer_be_retired(now_secs());
        }

        let (mine, orphaned) = {
            // The claims map is read while the ledger lock is NOT held, and it is authoritative:
            // this connection's own claim is in it, which is what keeps a live sibling — and this
            // conversation itself — out of the second list.
            let live: BTreeSet<Addr> = self.claims.lock().await.keys().cloned().collect();
            let ledger = self.ledger.lock().await;
            (
                ledger.open_for_other_instances(addr, instance),
                ledger.open_where_the_asker_is_gone(&addr.project, &live),
            )
        };

        // What is said is that the session restarted, and nothing more. Never an outcome — no
        // question retired here was ever answered, and saying otherwise is the exact misinformation
        // the instance filter exists to stop.
        if !mine.is_empty() {
            self.retire_each(
                addr,
                mine,
                "the session that asked this restarted, so it is not waiting for an answer any more",
            )
            .await;
        }
        // A different sentence, because it is a different thing that happened. A worktree is not
        // dispatched twice under one name, so what is being cleared here did not restart and is not
        // coming back — saying it restarted would tell him to expect the question again.
        if !orphaned.is_empty() {
            self.retire_each(
                addr,
                orphaned,
                "the session that asked this has ended, so it is not waiting for an answer any more",
            )
            .await;
        }
    }

    /// Drop a connection's claim, but only if it is still the one holding it.
    ///
    /// The guard matters: a bridge that was evicted and then finished shutting down would otherwise
    /// remove its successor's claim on the way out, leaving a live worker unreachable.
    /// Guarded on the generation as well as the pid, because the pid is not enough on its own: a
    /// run that was evicted and redialled is the same process, so its own shutdown would otherwise
    /// take its successor's claim away and leave a live worker unreachable.
    pub async fn release_this_run(&self, addr: &Addr, pid: u32, generation: u64) {
        let who = {
            let mut claims = self.claims.lock().await;
            if !claims
                .get(addr)
                .is_some_and(|c| c.pid == pid && c.generation == generation)
            {
                return;
            }
            claims.remove(addr);
            self.note_who_is_connected(&claims)
        };
        // The connection is gone and whatever it owed a word about is now unknown — never failed.
        // This run's records and no others: a wall that restarts within a second can already hold
        // the address by the time this line runs, and its intentions are not this one's to close.
        self.nothing_more_will_be_said_about(addr, generation).await;
        // Outside the block, so the lock is gone before the disk is touched. A departure is the
        // half of this that MUST reach the file: a snapshot still naming a bridge that has left is
        // what sends the operator to a conversation nothing is listening to.
        self.put_who_is_connected_on_disk(who).await;
    }

    /// The same by pid alone — for a claim taken through the four-argument [`Self::claim`], whose
    /// caller was never handed a generation to give back. Test-only, like that form is: every
    /// connection the hub really serves knows its own run number.
    #[cfg(test)]
    pub async fn release(&self, addr: &Addr, pid: u32) {
        // The run number is read under the same lock that removes the claim, because the sweep
        // below is scoped to the run that left and this form's caller was never handed one.
        let (who, generation) = {
            let mut claims = self.claims.lock().await;
            let Some(generation) = claims
                .get(addr)
                .filter(|c| c.pid == pid)
                .map(|c| c.generation)
            else {
                return;
            };
            claims.remove(addr);
            (self.note_who_is_connected(&claims), generation)
        };
        self.nothing_more_will_be_said_about(addr, generation).await;
        self.put_who_is_connected_on_disk(who).await;
    }

    /// Give the hub a different outbound budget. Test-only.
    ///
    /// The real limits are one message a second and eighteen a minute, so a flow test at those
    /// values would spend a second per message. The limits themselves are tested at their real
    /// values — `queue.rs`'s own tests, and `pacing_waits_but_a_real_flood_is_shed` below.
    #[cfg(test)]
    pub fn with_budget(self, per_minute: u32, min_gap: Duration) -> Self {
        Self {
            budgets: Arc::new(Mutex::new(crate::queue::Budgets::new(per_minute, min_gap))),
            ..self
        }
    }

    /// Would a chat take an agent's message right now? Test-only.
    ///
    /// The one way to observe from outside this module that a flood wait actually reached the
    /// budget — which is the property `bot.rs`'s three unrefusable sends now have to hold.
    #[cfg(test)]
    pub async fn a_send_would_be_refused(&self, chat_id: i64) -> bool {
        self.budgets
            .lock()
            .await
            .would_refuse(
                chat_id,
                std::time::Instant::now(),
                crate::queue::Spender::AnAgent,
            )
            .is_some()
    }

    /// Is the chat shut for the agents' sends — at its per-minute ceiling, or under a flood wait?
    /// The one-second gap between sends is deliberately not counted: a test asking this a moment
    /// after a send wants to know whether the BUDGET moved, not whether a send just went.
    #[cfg(test)]
    pub async fn the_chat_is_shut_for_sends(&self, chat_id: i64) -> bool {
        matches!(
            self.budgets.lock().await.would_refuse(
                chat_id,
                std::time::Instant::now(),
                crate::queue::Spender::AnAgent,
            ),
            Some(crate::queue::Refusal::Ceiling(_))
        )
    }

    /// Has the journal been told that Telegram refuses this bot's reactions? Test-only: the only
    /// way to see from outside that a refusal was told apart from the ceiling.
    #[cfg(test)]
    pub fn a_reaction_refusal_was_said(&self) -> bool {
        self.reaction_refusal_said.load(Ordering::Acquire)
    }

    /// How many of his typed lines are still waiting for a bridge's answer. A fence for a test
    /// that has to know every ack it sent has been handled, without sending anything to find out.
    #[cfg(test)]
    pub async fn words_awaiting_an_answer(&self) -> usize {
        self.down
            .lock()
            .await
            .iter()
            .filter(|d| matches!(d.what, His::Words { .. }))
            .count()
    }

    /// Carry one intention to the controller that said it could do the thing, or refuse and say
    /// why in the audit.
    ///
    /// Every gate here fails closed, and the order is [`Self::resolve_tap`]'s: everything that
    /// could refuse is asked BEFORE anything is written down, the record exists before the frame is
    /// on the wire, and a frame nothing took takes its record with it.
    ///
    /// **Routing reads the claim and never the frame.** The controller is found by asking each of
    /// this project's live connections whether IT declared this (spec, domain, op); the intention
    /// then goes to that connection's address. A `domain` on the frame is what the operation is
    /// ABOUT, and it cannot retarget the frame it rides on — which is the first thing an attacker
    /// would try, and the reason the two addresses are separate fields of the record.
    ///
    /// No `Down` record is kept for it. The controller's `ack` for the intent frame says the frame
    /// arrived and is bookkeeping the hub does not act on; what the hub waits for is the
    /// `intent_outcome`, correlated by `intent_id` and by nothing else.
    // No shipped caller until the keyboard is drawn; see the note at the head of `hub::intent`.
    #[allow(dead_code)]
    pub async fn intend(&self, wanted: Wanted) -> Result<Intended, IntentRefusal> {
        let Wanted {
            project,
            op,
            spec,
            domain,
            count,
            for_lease,
            offer: (chat_id, ref msg_id),
            ref option_id,
            from,
        } = wanted;

        // The two handles first, and shape-checked HERE as well as at admission. Everything below
        // can write a line before anything has compared them against a declaration, and
        // `HubAudit::intent`'s claim — "the domain by `lane_is_addressable`" — holds only if
        // nothing reaches that file which has not been through it. A domain carrying a newline
        // writes a whole second record of the caller's choosing into the one file an incident is
        // read from; the domain is dropped from the refusal line for the same reason it is refused.
        if !spec_is_addressable(&spec) || domain.as_ref().is_some_and(|d| !lane_is_addressable(d)) {
            return self.did_not_carry(&project, &None, IntentRefusal::HandleRefused);
        }

        // What it is ABOUT, which is not where it is going. The one address whose run number
        // `for_lease` can mean anything about.
        let about = match &domain {
            None => Addr::project_itself(project.clone()),
            Some(lane) => Addr::lane_of(project.clone(), lane.clone()),
        };

        // WHO TAPPED, before anything else is judged. Every other operator-initiated path asks
        // this at its own door — `resolve_tap` says why in its own words, that a second caller is
        // a second way around — and an intention is the most consequential of them. Asked before
        // the self-checks below so that a stranger learns nothing from the difference between a
        // request this hub would have carried and one it would have refused anyway.
        //
        // `may_speak_here` and not `may_command`: what this authority really is, is answering a
        // question in that conversation, which a project's own people already hold. `may_command`
        // is the wider bot-wide gate, and it guards facts about EVERY project rather than an act
        // in one.
        if !self
            .standing_of(Some(from.user_id), Some(&about))
            .await
            .may_speak_here()
        {
            // The one line a stranger leaves, with the id the operator can copy — and then the
            // refusal line, so the intent family keeps its own rule that a branch which carries
            // nothing still writes one.
            let _ = self.audit.stranger(
                Some(from.user_id),
                from.chat_id,
                Some(&about),
                "an intention",
            );
            return self.did_not_carry(&project, &domain, IntentRefusal::Stranger);
        }

        // The hub refusing ITSELF: an operation and a run that do not go together is a request
        // this hub built wrong, and carrying it would put a number that means nothing in front of
        // a check.
        if changes_a_running_thing(op) != for_lease.is_some() {
            return self.did_not_carry(&project, &domain, IntentRefusal::CouldNotBuildIt);
        }
        // The same for the count. `scale` is the one op with a number in it; a number on any other
        // is a field nobody would read, and a `scale` without one is an operation with no operand.
        if (op == Op::Scale) != count.is_some() {
            return self.did_not_carry(&project, &domain, IntentRefusal::CountRefused);
        }

        // WHO declared it, and IS THE SUBJECT STILL THE RUN HE WAS SHOWN — both under ONE hold of
        // the claims lock. Two holds would be `claim_the_address`'s old defect in a new place: the
        // subject can restart between them, and the fence would then be reading a world the
        // declaration check never saw.
        //
        // The controller is the connection ITSELF, never `Addr { project, domain }` — that would be
        // routing by a field on the frame with a record wrapped round it, and it is also the only
        // routing that works for `start`, whose domain may have nothing at it yet.
        let found = {
            let claims = self.claims.lock().await;
            let mut of_this_project = claims
                .iter()
                .filter(|(a, _)| a.project == project)
                .peekable();
            if of_this_project.peek().is_none() {
                Err(IntentRefusal::NotConnected)
            } else {
                let mut declared = of_this_project.filter_map(|(a, c)| {
                    // Read through the crate's own reader, which asks whether the declaration names
                    // THIS operation rather than whether anything was declared — and which declines
                    // to answer at all when two entries of one declaration name it, because picking
                    // either bound is the hub guessing which one the controller meant.
                    control_for(&c.controls, &spec, domain.as_ref(), op)
                        .map(|found| (a.clone(), c.generation, found.max))
                });
                match (declared.next(), declared.next()) {
                    (None, _) => Err(IntentRefusal::NotDeclared),
                    (Some(_), Some(_)) => Err(IntentRefusal::MoreThanOneCouldDoIt),
                    // The bound the controller set on ITSELF. The hub compares against it and
                    // refuses; it never raises one and never invents one, so a controller that
                    // named no bound has named no bound and a count against it is a number nothing
                    // checked.
                    //
                    // A CEILING AND NO FLOOR, knowingly. A controller that declares `scale` with a
                    // bound of four, and deliberately does not declare `stop`, can still be asked
                    // for zero of that spec — and whether zero of a thing is a stop is the
                    // controller's own semantics, which this hub holds no table to decide. Adding
                    // a floor here would be the hub inventing a meaning for somebody else's verb,
                    // which is the mistake the whole contract is shaped to avoid. It is recorded
                    // as an open question for whoever writes the controller — either a floor rides
                    // beside `max` on the declaration, or `scale` is documented as consenting to
                    // zero — and not decided here.
                    (Some((_, _, max)), None)
                        if count.is_some_and(|c| max.is_none_or(|max| c > max)) =>
                    {
                        Err(IntentRefusal::CountRefused)
                    }
                    (Some((to, run, _)), None) => {
                        // THE FENCE. He is shown three workers, walks away, the wall restarts, he
                        // comes back and taps *Scale to 1*; without this that scales the NEW run to
                        // one.
                        //
                        // Compared against a LIVE claim and against nothing else. `highest_for`
                        // answers `0` for an address that was never claimed, and a zero is read as
                        // "holds none" everywhere else on this wire — so a subject with no run is
                        // its own refusal rather than a comparison something could win.
                        match (for_lease, claims.get(&about).map(|c| c.generation)) {
                            (None, _) => Ok((to, run)),
                            (Some(_), None) => Err(IntentRefusal::NothingIsConnectedThere),
                            (Some(named), Some(running)) if named != running => {
                                Err(IntentRefusal::TheWorldMoved)
                            }
                            (Some(_), Some(_)) => Ok((to, run)),
                        }
                    }
                }
            }
        };
        let (to, run) = match found {
            Ok(found) => found,
            Err(why) => return self.did_not_carry(&project, &domain, why),
        };

        // From the ledger's own counter, behind the ledger's own boot token — never the frame
        // counter, which starts at one with the process. An id that recurred across a restart let
        // an outcome a controller still owed for one intention settle a different one minted
        // after it. See [`IntentLedger::boot`].
        let id = self.intents.lock().await.mint_an_id();
        let key = mint_idempotency_key(&project, chat_id, msg_id, option_id, for_lease);

        // Written down BEFORE the frame is on the wire, which is `resolve_tap`'s rule and its
        // reason: an unrecorded intention is one that can be carried again. The audit line goes
        // with it, for `HubAudit::sent`'s reason — a line with no outcome after it means the
        // process died in the middle of writing, and means nothing else.
        let record = Intent {
            id: id.clone(),
            key,
            to: to.clone(),
            run,
            about,
            op,
            spec,
            count,
            for_lease,
            user: from.user_id,
            state: IntentState::Sent,
            said: None,
            said_digest: None,
        };
        let frame = HubFrame::Intent {
            intent_id: id.clone(),
            idempotency_key: record.key.clone(),
            op,
            spec_id: record.spec.clone(),
            domain: domain.clone(),
            count,
            for_lease,
            from,
            valid_for_ms: INTENT_VALID_FOR.as_millis() as u64,
        };
        // The look-up, the room, the line, the record AND THE HAND-OVER are one critical section,
        // for the reason the claim's check and reservation are: two taps a moment apart that each
        // read the list and then wrote to it would both find nothing and both be carried, which is
        // the exact duplicate the key exists to prevent, arranged by the code that exists to
        // prevent it. The hand-over is inside it because the two conditions are one condition — a
        // controller that is not keeping up is why the receipt was slow, which is why he tapped
        // again — so a second tap in that window was told the button had been carried while the
        // first was about to fail and take its own record away.
        //
        // THE ONE LOCK ORDER IN THIS FILE: the ledger, then the claims. Nothing may take the
        // claims lock and then want the ledger, which is why the outcome path reads the claim it
        // needs before it opens the ledger at all.
        {
            let mut intents = self.intents.lock().await;
            // The record this one replaces, if any — given up below, and only once the frame that
            // replaces it is on the wire.
            let mut replacing = None;
            match intents.already_carried(&record.key) {
                // Offered AGAIN rather than answered from, and this is the one state where that is
                // right: nothing was ever said about this one, so the hub does not know what
                // became of it, and answering a fresh tap from that would be the hub claiming
                // knowledge it has not got — for ever, since the key of an op that carries no
                // lease never moves and that button would be dead for the life of the process.
                // At-least-once is what this wire promises and the key rides on the frame, so a
                // controller that did act on it answers from its own record and does not act
                // twice.
                //
                // A record whose controller had ACCEPTED does not come here, deliberately: the hub
                // was told that work began, and re-carrying it would ask for a second run of an
                // operation known to have started — at a fresh controller with no record of the
                // key, which is precisely where the key protects nobody. That one is answered from
                // instead, and the honest answer is the state itself. It survives a restart for
                // that reason: see [`IntentLedger`].
                Some(AlreadyCarried::NothingWasEverSaidAboutIt(old)) => {
                    let _ = self.audit.intent_again_carried(&old.to, &old.id, &old.key);
                    replacing = Some(old.id.clone());
                }
                // Not a refusal: this button has been carried, and the honest answer is what became
                // of it. **No second frame** — at-least-once is what this wire promises, and
                // nothing here claims exactly-once.
                Some(AlreadyCarried::AnswerFromIt(id, state)) => {
                    let _ = self.audit.intent_again(&record.to, &id, &record.key);
                    return Ok(Intended::AlreadyAsked(id, state));
                }
                None => {}
            }
            // Room, and never at the cost of a record nobody has answered for. The bound is what
            // stops a controller that never answers turning a record nobody will read into a leak,
            // but forgetting the OLDEST regardless of its state made the bound a way to carry one
            // button twice: past it, a repeat was carried afresh instead of answered from the
            // record. So when there is none to forget the intention is refused — which the
            // operator can read, where a duplicate restart is something he cannot see.
            //
            // A REPEAT IS NOT ONE MORE. The record it replaces is the room it needs, and that
            // record is not given up until the hand-over below has succeeded — so asking the bound
            // for a slot here would let it choose the predecessor itself, and a hand-over that
            // then failed would take that away too. The ledger holds one over its bound for the
            // rest of this critical section, which nothing outside it can see.
            if replacing.is_none() && !intents.room_for_one_more() {
                return self.did_not_carry(&project, &domain, IntentRefusal::TooManyWaiting);
            }
            let _ = self.audit.intent(&record);
            intents.write_down(record);
            if let Err(why) = self.hand_to_the_run(&to, run, frame).await {
                // Taken back, so the key is free to be used again: nothing was carried, and a
                // record waiting for a word about a frame nothing received would answer a second
                // tap from it. `deliver_tap` does exactly this, for exactly this reason. What is
                // NOT taken back is the record this one was replacing: nothing replaced it, a
                // controller may still be acting on it, and the audit line above points at it.
                intents.take_the_last_one_back();
                return self.did_not_carry(&project, &domain, why);
            }
            // On the wire, so the record it replaced can be given up.
            if let Some(old) = replacing {
                intents.forget(&old);
            }
        }
        Ok(Intended::Carried(id))
    }

    /// Hand one intention to the RUN that declared it, and to no other.
    ///
    /// `deliver_under`'s move with the fence `release_this_run` already makes over `release`: an
    /// address is not a run. The claim can be replaced between the fence reading it and the frame
    /// going out — a wall restarting is the ordinary case, not an exotic one — and a successor
    /// that never declared this capability would otherwise be handed the intention anyway,
    /// stamped with its own lease so that nothing downstream could tell.
    // No shipped caller until the keyboard is drawn; see the note at the head of `hub::intent`.
    #[allow(dead_code)]
    async fn hand_to_the_run(
        &self,
        addr: &Addr,
        run: u64,
        frame: HubFrame,
    ) -> Result<(), IntentRefusal> {
        let tx = {
            let claims = self.claims.lock().await;
            match claims.get(addr) {
                Some(claim) if claim.generation == run => claim.tx.clone(),
                _ => return Err(IntentRefusal::TheControllerWentAway),
            }
        };
        tx.try_send(Envelope::new(Self::mint_frame_id(), frame).with_generation(run))
            .map_err(|e| {
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(), error = %e,
                    "a controller is not keeping up; an intention was not handed on"
                );
                IntentRefusal::CouldNotHandItOn
            })
    }

    /// A controller has said what became of one of the intentions this hub carried.
    ///
    /// Two confinements, and neither is optional. The id must be one this hub minted — a controller
    /// that could put a line on the operator's phone by naming an id would be a way to reach him
    /// that nothing authorised, which is the general form of the non-bound event. And the outcome
    /// must arrive on the connection the intention was SENT to, or one conversation of a project
    /// answers for another's work, which is a worktree closing a question asked somewhere it cannot
    /// see. Both write one line and act on nothing.
    ///
    /// A third confinement, and it is what the address alone was missing. While the intention is
    /// still live, only the RUN it was handed to may speak about it. Afterwards — once the hub has
    /// written `Unknown` because that connection ended — a late correction is taken only from a
    /// connection that DECLARED the same thing, because an ordinary bridge that took the address
    /// has no business settling work it never offered to do, and turning an honest "I do not know"
    /// into "Done" on his phone is the worst answer this system has.
    ///
    /// Nothing reaches Telegram from here. The keyboard these outcomes belong under is a later
    /// slice, so what this one owes the operator is the audit line.
    async fn what_became_of_an_intention(
        &self,
        addr: &Addr,
        id: &IntentId,
        status: IntentStatus,
        reason: Option<String>,
    ) -> (Delivered, Option<AckWhy>) {
        // Read BEFORE the ledger is opened, never inside it: the one lock order in this file is
        // the ledger and then the claims, because `intend` holds the ledger while it hands a frame
        // to a claim. Taking them the other way round here would be a deadlock waiting for two
        // ordinary events to coincide.
        let speaking = {
            let claims = self.claims.lock().await;
            claims.get(addr).map(|c| (c.generation, c.controls.clone()))
        };
        // Clamped BEFORE anything is compared, because the clamp is where the sentence is kept:
        // the record holds what `queue::fit` made of it and the text before that is never
        // retained, so "the same word again" can only ever mean the same as the hub wrote it down.
        // Comparing the two ends of the clamp would answer "you said something different" to a
        // controller that said the same thing.
        //
        // Never written to the audit either way: a sentence from another process can carry a
        // newline, which in a file of one record per line is a second record of its choosing.
        // `queue::fit` is the same clamp every other word from a bridge gets before it reaches a
        // topic.
        let said = reason.map(|r| crate::queue::fit(&r, crate::queue::MAX_TEXT).0);
        let heard =
            self.intents
                .lock()
                .await
                .a_word_about(id, addr, status, said, speaking.as_ref());
        match heard {
            Heard::NeverSent => {
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(),
                    "a connection reported an outcome for an intention this hub never sent"
                );
                let _ = self
                    .audit
                    .refused(addr, "an outcome named an intention this hub did not send");
                (Delivered::No, Some(AckWhy::NoSuchIntent))
            }
            Heard::NotThisConversation => {
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(),
                    "a connection reported an outcome for an intention carried somewhere else"
                );
                let _ = self.audit.refused(
                    addr,
                    "an outcome for an intention this hub carried to another conversation",
                );
                (Delivered::No, Some(AckWhy::NoSuchIntent))
            }
            Heard::NotItsToSettle => {
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(),
                    "a connection reported an outcome for an intention it was never handed"
                );
                let _ = self.audit.refused(
                    addr,
                    "an outcome from a connection that was never handed that intention",
                );
                (Delivered::No, Some(AckWhy::NoSuchIntent))
            }
            Heard::SameObservationAgain => {
                // One observation, arriving twice, and the controller's retry has SUCCEEDED: the
                // hub holds exactly what it was told, so `yes` is the true answer and there is
                // nothing to add to it. This wire is at-least-once in both directions and the
                // late-correction rule invites a controller to replay from its own record, so a
                // lost `ack` puts a correct controller here — and answering it the way a
                // self-contradicting one is answered told it its frame had died, with no reason
                // it could act on.
                //
                // No second audit line, deliberately: the file records what became of an
                // intention, and a second line about one observation makes a reader counting
                // outcomes count the network's retries instead.
                (Delivered::Yes, None)
            }
            Heard::ContradictsWhatItAlreadySaid => {
                // Not `no_such_intent`: the intention is real and is this connection's. What is
                // wrong is the WORD — the controller said `completed` and now says `failed` — and
                // the first word stands. It is told which kind of wrong this is, because a
                // controller that cannot tell a contradiction from a lost frame cannot find its
                // own defect.
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(),
                    "a controller said something different about an intention it had already \
                     answered for; the first word stands"
                );
                let _ = self.audit.refused(
                    addr,
                    "a second word about an intention that was already answered for",
                );
                (Delivered::No, Some(AckWhy::AlreadyAnswered))
            }
            Heard::Took => {
                let _ = self.audit.intent_outcome(addr, id, status);
                (Delivered::Yes, None)
            }
        }
    }

    /// A connection has ended with words still owed about intentions it was handed.
    ///
    /// They become [`IntentState::Unknown`], keeping the phase they had reached, and never
    /// `failed`. What the hub observed is that the
    /// CONVERSATION ended; it observed nothing whatever about the work, and a hub that read one as
    /// the other would tell the operator something changed when nothing may have — which is the
    /// worst thing this system can tell him, because his next action is chosen on it.
    ///
    /// **The RUN that left, never the address it left.** An address is not a run here either: on
    /// the eviction path the successor's claim is already in the map when this lands, and on the
    /// ordinary path a wall that restarts within a second — the ordinary case, not the exotic
    /// one — can have claimed and been handed an intention before this gets the ledger. Sweeping
    /// by address wrote off a live intention of the run that is still working on it: its record
    /// then said the connection had ended, so a repeat of that button was carried a SECOND time
    /// to a controller already doing the first, and who may settle it widened from that one run
    /// to anything at the address.
    ///
    /// **Why it cannot simply be done under the claims lock**, so that a later reader does not
    /// close the window by moving it in: `intend` holds the LEDGER and then reaches for the
    /// claims, which is the one lock order in this file. Taking the claims and then the ledger
    /// here is the other order, and two ordinary events — one connection leaving while another
    /// is handed an intention — would deadlock the hub. The window stays; what changed is that
    /// nothing in it belongs to the sweep.
    async fn nothing_more_will_be_said_about(&self, addr: &Addr, run: u64) {
        self.intents.lock().await.nothing_more_from(addr, run);
    }

    /// Where one intention has got to, as the hub observed it.
    ///
    /// Test-only. The keyboard that would show this to the operator is a later slice, so nothing
    /// shipped reads it yet; what this slice owes him is the audit line and the journal.
    #[cfg(test)]
    pub async fn what_became_of(&self, id: &IntentId) -> Option<IntentState> {
        self.intents.lock().await.state_of(id)
    }

    /// The controller's own sentence about one intention, as the hub kept it.
    ///
    /// Test-only, for the same reason [`Self::what_became_of`] is: the surface that would put this
    /// in front of the operator is a later slice.
    #[cfg(test)]
    pub async fn what_was_said_about(&self, id: &IntentId) -> Option<String> {
        self.intents.lock().await.said_about(id)
    }

    /// One refusal, said once in the journal and once in the audit, and handed back.
    ///
    /// A branch that carries nothing still writes a line, so silence in that file always means the
    /// process stopped rather than that the hub decided something quietly.
    // No shipped caller until the keyboard is drawn; see the note at the head of `hub::intent`.
    #[allow(dead_code)]
    fn did_not_carry(
        &self,
        project: &ProjectId,
        domain: &Option<LaneId>,
        why: IntentRefusal,
    ) -> Result<Intended, IntentRefusal> {
        // `domain` and not `lane`, in the journal as in the audit: the two mean different things
        // on this path — a lane is a conversation, a domain is what an operation is about — and one
        // field name for both is how a search for one silently answers with the other.
        tracing::warn!(
            project = %project, domain = domain.as_ref().map_or("-", LaneId::as_str),
            "an intention was not carried: {}", why.written_down()
        );
        let _ = self
            .audit
            .intent_refused(project, domain.as_ref(), why.written_down());
        Err(why)
    }

    /// Hand one of his taps to the project's live connection, and write it down first.
    ///
    /// The record has to exist BEFORE the frame is on the wire, for the reason his typed words'
    /// does: a tool server acks a choice within a millisecond of reading it, and an ack that
    /// arrives before the record does names a frame the hub holds nothing for and is dropped on the
    /// floor. That is what "Sent" meant until now — the outbox took the frame — and it stayed on
    /// his phone whatever the agent then did with the answer.
    ///
    /// Hands back the id it went down under, which is the id an `ack` for it names, or `None` when
    /// nothing took it. Taken back on that path, so a record never waits for an answer to a frame
    /// nothing received.
    pub async fn deliver_tap(
        self: &Arc<Self>,
        addr: &Addr,
        chat_id: i64,
        msg_id: &MsgId,
        ask_id: AskId,
        option_id: OptionId,
        label: &str,
    ) -> Option<FrameId> {
        // Read from the claim rather than remembered anywhere else: the promise belongs to the
        // connection that made it, and a run that has since been replaced cannot have its
        // successor nagged for it.
        let promised = {
            self.claims
                .lock()
                .await
                .get(addr)
                .is_some_and(|c| c.confirms_choices)
        };
        let frame = Self::mint_frame_id();
        {
            let mut down = self.down.lock().await;
            down.push_back(Down {
                frame: frame.clone(),
                addr: addr.clone(),
                chat_id,
                msg_id: msg_id.clone(),
                what: His::Tap(HisTap {
                    label: label.to_owned(),
                    receipt: None,
                    said: None,
                    promised,
                    overdue: false,
                    no_receipt_is_coming: false,
                }),
            });
            while down.len() > DOWN_KEPT {
                down.pop_front();
            }
        }
        let went = self
            .deliver_under(
                addr,
                frame.clone(),
                HubFrame::Choice {
                    msg_id: msg_id.clone(),
                    ask_id,
                    option_id,
                },
            )
            .await;
        if !went {
            self.down.lock().await.retain(|d| d.frame != frame);
            return None;
        }
        // The window, in a task of its own. It cannot be awaited here: this runs inside Telegram's
        // per-chat dispatcher, which handles one update at a time, so waiting out the window here
        // would hold every later tap and every line he types behind it.
        {
            let hub = Arc::clone(self);
            let frame = frame.clone();
            let window = Duration::from_millis(self.tap_confirm_window.load(Ordering::Relaxed));
            tokio::spawn(async move {
                tokio::time::sleep(window).await;
                hub.the_session_never_confirmed(&frame).await;
            });
        }
        Some(frame)
    }

    /// Which message his receipt for a tap is — the line that says what he chose.
    ///
    /// `bot.rs` is the only place that can ever know it: the hub did not send it, and Telegram only
    /// says which message it made once the send has come back. That is a round trip AFTER the
    /// answer went down, so an ack can be here first — and when it is, this is where what the
    /// bridge said finally reaches the line it is about.
    pub async fn his_receipt_for_a_tap(&self, frame: &FrameId, receipt: &MsgId) {
        // Its turn among the things that edit this receipt. See `tap_edits`.
        let _in_order = self.tap_edits.lock().await;
        let now = {
            let mut down = self.down.lock().await;
            let Some(at) = down.iter().position(|d| &d.frame == frame) else {
                return;
            };
            let Some(Down {
                addr,
                chat_id,
                msg_id,
                what: His::Tap(tap),
                ..
            }) = down.get_mut(at)
            else {
                return;
            };
            tap.receipt = Some(receipt.clone());
            // The window ran out before there was a line to change. Said now, on the line that has
            // just come into existence — the alternative is the window silently doing nothing
            // whenever Telegram is slower over his receipt than the bridge is over its answer.
            if tap.said.is_none() && tap.overdue {
                let label = tap.label.clone();
                drop(down);
                let text =
                    format!("Sent: {label}. The session has not confirmed it took your answer.");
                if let Err(e) = self.surface.rewrite(receipt, &text).await {
                    tracing::warn!(error = %e, "could not tell him a tap has not been confirmed");
                }
                return;
            }
            match tap.said.take() {
                None => None,
                Some(said) => {
                    let it = (
                        addr.clone(),
                        *chat_id,
                        msg_id.clone(),
                        tap.label.clone(),
                        said,
                    );
                    down.remove(at);
                    Some(it)
                }
            }
        };
        if let Some((addr, chat_id, question, label, said)) = now {
            self.say_what_became_of_his_tap(&addr, chat_id, &question, Some(receipt), &label, said)
                .await;
        }
    }

    /// Telegram refused the send that would have been his receipt for a tap. There is no line, and
    /// there never will be.
    ///
    /// Until this existed the record simply waited: the ack arrived, found no receipt, parked what
    /// the agent said and returned; the window fired, found no receipt, wrote it down and returned.
    /// Both were waiting for a round trip that had already failed. So an agent's "I could not act on
    /// this" was swallowed, and the operator was left with a question whose buttons had gone and not
    /// one word about what became of his answer — and the send that fails is the one made while the
    /// forum is busy, which is exactly when he is tapping.
    ///
    /// A tap the agent TOOK needs nothing said: "Sent" was never written, so nothing on his phone
    /// is false. A refusal is said the only way left, under the question itself.
    pub async fn his_receipt_never_arrived(&self, frame: &FrameId) {
        // Its turn among the things that edit this receipt, exactly as the arrival of one is. See
        // `tap_edits`.
        let _in_order = self.tap_edits.lock().await;
        let now = {
            let mut down = self.down.lock().await;
            let Some(at) = down.iter().position(|d| &d.frame == frame) else {
                return;
            };
            let Some(Down {
                addr,
                chat_id,
                msg_id,
                what: His::Tap(tap),
                ..
            }) = down.get_mut(at)
            else {
                return;
            };
            tap.no_receipt_is_coming = true;
            // Nothing has been heard from the bridge yet. The record stays where it is: an answer
            // arriving later finds this flag and says its piece rather than parking it for ever.
            let Some(said) = tap.said.take() else {
                return;
            };
            let it = (
                addr.clone(),
                *chat_id,
                msg_id.clone(),
                tap.label.clone(),
                said,
            );
            down.remove(at);
            Some(it)
        };
        if let Some((addr, chat_id, question, label, said)) = now {
            self.say_what_became_of_his_tap(&addr, chat_id, &question, None, &label, said)
                .await;
        }
    }

    /// The window on a tap ran out. Say so, but only where saying it is true and useful.
    ///
    /// Three ways this ends in silence, and each is a case where the sentence would be a worry with
    /// nothing behind it: a bridge that never promised to confirm anything (every bridge shipped so
    /// far), a receipt Telegram refused to send so there is no line to change, and an answer that
    /// got here first.
    ///
    /// The record is LEFT where it is, exactly as an unanswered record of his words is: a session
    /// that finally says it took the answer, a minute late, then corrects the line rather than
    /// leaving him reading "has not confirmed" about something that was confirmed. What bounds it
    /// is [`DOWN_KEPT`], the same bound that has always bounded the other half.
    async fn the_session_never_confirmed(&self, frame: &FrameId) {
        // Held across the read AND the edit below. Without it an answer arriving while this edit
        // was in flight took the record away, made its own edit, and landed first — leaving him
        // reading that a tap the agent took was never confirmed. See `tap_edits`.
        let _in_order = self.tap_edits.lock().await;
        let waiting = {
            let mut down = self.down.lock().await;
            let Some(at) = down.iter().position(|d| &d.frame == frame) else {
                return;
            };
            let addr = down[at].addr.clone();
            let His::Tap(tap) = &mut down[at].what else {
                return;
            };
            if tap.said.is_some() {
                return;
            }
            match (tap.receipt.clone(), tap.promised) {
                (Some(receipt), true) => (addr, receipt, tap.label.clone()),
                (receipt, promised) => {
                    // No line to change YET is not the same as nothing to say. Remembered, so the
                    // receipt says it the moment Telegram tells the bot which message it is.
                    tap.overdue = receipt.is_none() && promised;
                    tracing::debug!(
                        project = %addr.project, lane = addr.lane_field(),
                        promised, receipt = receipt.is_some(),
                        "nothing said about a tap that was never confirmed"
                    );
                    return;
                }
            }
        };
        let (addr, receipt, label) = waiting;
        let text = format!("Sent: {label}. The session has not confirmed it took your answer.");
        if let Err(e) = self.surface.rewrite(&receipt, &text).await {
            tracing::warn!(
                error = %e, project = %addr.project, lane = addr.lane_field(),
                "could not tell him a tap has not been confirmed"
            );
        }
    }

    /// What the operator reads on the line he is already looking at, once the session has said what
    /// became of his answer.
    ///
    /// An EDIT, never a send. Telegram charges a chat twenty messages a minute and charges nothing
    /// for editing one it already has, and a tap is made precisely while he is looking at a busy
    /// forum — so a second message per tap would come out of the budget an agent's questions need.
    /// `receipt` is `None` when Telegram refused the send that would have been it. There is then no
    /// line carrying "Sent" at all, which is the same position as an edit Telegram will not make and
    /// takes the same way out.
    async fn say_what_became_of_his_tap(
        &self,
        addr: &Addr,
        chat_id: i64,
        question: &MsgId,
        receipt: Option<&MsgId>,
        label: &str,
        said: WhatBecameOfTheTap,
    ) {
        let text = match &said {
            WhatBecameOfTheTap::Took => {
                let _ = self
                    .audit
                    .outcome(addr, &SendOutcome::Sent(question.clone()));
                format!("Taken: {label}")
            }
            WhatBecameOfTheTap::Refused(why) => {
                let _ = self.audit.refused(
                    addr,
                    &format!("his answer to question {question} was not taken: {why}"),
                );
                // Taken back BEFORE the line that says so, so the two arrive in the order he reads
                // them. Usually there is nothing left to take back — the keyboard came off the
                // moment he tapped — and that is why it says the answer did not reach the agent
                // rather than inviting him to tap a menu that is not there any more.
                //
                // NOT by the path a delivery that never left takes: that one writes "nothing here
                // could be reached" onto the question, and on this path something WAS reached and
                // said no.
                self.take_the_question_back(
                    chat_id,
                    question,
                    "not taken — the agent could not act on your answer",
                )
                .await;
                format!("Not taken: {label} — {why}. The agent has not got your answer.")
            }
        };
        // The free edit first, wherever there is a line to edit.
        let no_line = match receipt {
            Some(receipt) => match self.surface.rewrite(receipt, &text).await {
                Ok(()) => return,
                Err(e) => e.to_string(),
            },
            None => "the line that would have said it was never sent".to_owned(),
        };
        tracing::warn!(
            error = %no_line, project = %addr.project, lane = addr.lane_field(),
            "could not tell him what became of his answer"
        );
        // A refusal is the one of the two he must not miss: "Sent" is not false about a tap the
        // agent took — and where his receipt never went at all it was never even claimed — and it
        // IS false about one the agent said no to. The free edit is always tried first and this
        // runs only when Telegram refused it or never made the line, so the ordinary tap still
        // costs nothing — and a send that keeps him from acting on an answer nobody has is worth
        // one of the twenty.
        if matches!(said, WhatBecameOfTheTap::Refused(_)) {
            let outcome = self.say_under(addr, &text, question).await;
            if !matches!(outcome, SendOutcome::Sent(_) | SendOutcome::Clamped(_)) {
                tracing::error!(
                    project = %addr.project, lane = addr.lane_field(), outcome = ?outcome,
                    "his answer was refused by the agent and there is no way left to tell him"
                );
            }
        }
    }

    /// How many of his taps are still waiting for a bridge's answer. A fence for a test that has to
    /// know a tap was written down, without sending anything to find out.
    #[cfg(test)]
    pub async fn taps_awaiting_an_answer(&self) -> usize {
        self.down
            .lock()
            .await
            .iter()
            .filter(|d| matches!(d.what, His::Tap(_)))
            .count()
    }

    /// Is a tap written down under exactly this id — the id it went down the wire under?
    #[cfg(test)]
    pub async fn a_tap_is_awaited_under(&self, frame: &FrameId) -> bool {
        self.down
            .lock()
            .await
            .iter()
            .any(|d| &d.frame == frame && matches!(d.what, His::Tap(_)))
    }

    /// Which line the hub believes his receipt for that tap is.
    #[cfg(test)]
    pub async fn the_receipt_written_down_for(&self, frame: &FrameId) -> Option<MsgId> {
        self.down
            .lock()
            .await
            .iter()
            .find(|d| &d.frame == frame)
            .and_then(|d| match &d.what {
                His::Tap(tap) => tap.receipt.clone(),
                His::Words { .. } => None,
            })
    }

    /// Give a promising bridge less than [`TAP_CONFIRM_WINDOW`] to confirm a tap. Test-only, for
    /// the reason `with_settle` is: the only way to see a bound bite is to move it, and waiting the
    /// real twenty seconds out is twenty seconds of a suite doing nothing.
    #[cfg(test)]
    pub fn confirm_taps_within(&self, how_long: Duration) {
        self.tap_confirm_window
            .store(how_long.as_millis() as u64, Ordering::Relaxed);
    }

    /// Move an address on to a later run without touching the claim. Test-only.
    ///
    /// It is the state a connection is really in for the instant between a successor taking the
    /// address and the kick reaching it — and the state a released connection's drain is in for as
    /// long as it takes to finish. Over a socket that instant is microseconds and cannot be held
    /// open, and it is exactly the window the delivery fence exists for.
    #[cfg(test)]
    pub fn a_newer_run_has_taken(&self, addr: &Addr) -> u64 {
        self.generations
            .lock()
            .expect("the generations are not held across an await")
            .mint(addr, 0)
    }

    /// Test-only, and it stays that way. The DELIVERY path must never ask this: it asks by trying
    /// to deliver, which is the question it actually has, and a "is it connected" read taken a
    /// moment earlier is a fact that can already be wrong by the time it is acted on.
    ///
    /// [`Self::connected_ids`] answers a different question — what to show a person — and there a
    /// snapshot is the honest answer rather than a stale one.
    #[cfg(test)]
    pub async fn is_claimed(&self, addr: &Addr) -> bool {
        self.claims.lock().await.contains_key(addr)
    }

    /// Is this conversation's project switched off?
    ///
    /// For the sentence `bot.rs` says back when his words reach nobody. Off is his own decision at
    /// a terminal, and "not connected" — true, since the switch drops the connection — would send
    /// him to restart a bridge the hub is going to refuse. Read from the copy the registry watcher
    /// keeps fresh rather than from disk: a message is not the moment to re-read a file, and a
    /// second's lag on a sentence costs nothing.
    pub async fn is_switched_off(&self, addr: &Addr) -> bool {
        self.registry
            .lock()
            .await
            .get(&addr.project)
            .is_some_and(|p| !p.enabled)
    }

    /// Which projects have a bridge on the socket right now, for a human reading a list.
    ///
    /// A snapshot, deliberately, and it is the right shape for this one caller: by the time he has
    /// read the message anything in it may have changed, and he knows that about a status list. The
    /// alternative on offer was worse than stale — the list rendered a project's topic binding,
    /// which is permanent from its first connection onward and says nothing whatever about now.
    pub async fn connected_ids(&self) -> BTreeSet<Addr> {
        self.claims.lock().await.keys().cloned().collect()
    }

    /// Which conversation owns a topic, if any.
    ///
    /// A message typed in a topic belongs to that conversation and to no other. This is the whole
    /// of routing: rule 0 and nothing else. Every supergroup numbers its reply threads from one
    /// counter, which is how a swipe-reply on a direct message once reached a forum pane — deleting
    /// the other rules deletes that failure rather than testing against it.
    ///
    /// A lane's topic answers with the LANE. There is deliberately no falling back to the project
    /// when a lane's topic is not found: that would put what the operator typed at a worktree into
    /// the project's own turn, which is an agent reading an instruction meant for someone else.
    pub async fn addr_for_topic(&self, topic_id: i32) -> Option<Addr> {
        let registry = self.registry.lock().await;
        for p in registry.all() {
            if p.topic_id == Some(topic_id) {
                return Some(Addr::project_itself(p.id.clone()));
            }
            // In the same pass, because the answer has to be ONE address and a second store would
            // need a rule about which of the two wins.
            if let Some((lane, _)) = p.lane_topics.iter().find(|(_, t)| **t == topic_id) {
                return Some(Addr::lane_of(p.id.clone(), lane.clone()));
            }
        }
        None
    }

    /// Hand the operator's own words to a project, verbatim and exactly once.
    ///
    /// **Opaque.** The hub does not parse it, does not act on it, and does not let it name
    /// anything. Inbound content selects; it never names. What the agent receives is a MESSAGE in
    /// its own turn — never a keystroke — which is why the operator's phone and his laptop are no
    /// longer two writers fighting over one keyboard.
    ///
    /// Returns whether it was delivered, so the caller can say so rather than guess. A project that
    /// is not connected is told to the operator visibly, in the topic, and never queued: a message
    /// held for a worker that may never return is a message he believes was sent.
    ///
    /// `replied_to` is the message he swiped to reply to, if any. When it is one of the questions
    /// this conversation's live session asked, the bridge is told which — `in_reply_to_ask` — and
    /// that is the one time he says which question, and so which session, his words are for. The
    /// adapter side decides what to do with it; this side hardcoded the field to nothing for a
    /// slice, while three documents described the reply path as built.
    ///
    /// Words alone. The update handler calls [`Self::relay_with`], which is this with the files
    /// he sent beside his words; this spelling stays for the tests, which are about the words.
    #[cfg(test)]
    pub async fn relay(
        &self,
        addr: &Addr,
        chat_id: i64,
        user: Option<i64>,
        msg_id: &MsgId,
        text: &str,
        replied_to: Option<&MsgId>,
    ) -> bool {
        self.relay_with(addr, chat_id, user, msg_id, text, replied_to, Vec::new())
            .await
    }

    /// What the test-only `relay` does — his words — with the files he sent beside them.
    ///
    /// Each file is fetched into the conversation's own media directory FIRST, and what went down
    /// is a path the hub minted — never the bytes, which do not fit a frame, and never a name
    /// anybody else chose. A file that did not come through still lets the words go: its entry
    /// carries `why` instead of a path, and one line under his message says the same to him.
    ///
    /// Fetched before delivery is tried, not after: the delivery path must not ask whether
    /// anybody is connected, because a read taken a moment earlier can be wrong by the time it is
    /// acted on, and the honest way to find out is to deliver. So a file sent to a conversation
    /// with nobody in it is fetched and then swept with everything else — twenty megabytes of
    /// disk for two days, against a check this code has a documented reason not to make.
    ///
    /// Fetched INSIDE the update handler rather than in a task of its own, which holds this chat's
    /// updates behind a download of up to twenty megabytes. Deliberate: Telegram's dispatcher
    /// runs one chat's updates in order, and that order is the only thing that keeps "see the
    /// screenshot above" from reaching the agent before the screenshot does.
    #[allow(clippy::too_many_arguments)]
    pub async fn relay_with(
        &self,
        addr: &Addr,
        chat_id: i64,
        user: Option<i64>,
        msg_id: &MsgId,
        text: &str,
        replied_to: Option<&MsgId>,
        files: Vec<SentFile>,
    ) -> bool {
        if !self.chat_is_allowed(chat_id) {
            return false;
        }
        // The person, checked HERE and not only in the handler above, for the reason the chat is:
        // a second caller is a second way around. `From.user_id` used to be whatever the update
        // carried, written down for an audit nothing read; now it is a person who has passed this
        // check, or the frame is never built.
        let standing = self.standing_of(user, Some(addr)).await;
        let Some(user_id) = user.filter(|_| standing.may_speak_here()) else {
            let _ = self.audit.stranger(user, chat_id, Some(addr), "words");
            return false;
        };
        let in_reply_to_ask = match replied_to {
            Some(under) => self.ask_replied_to(addr, chat_id, under).await,
            None => None,
        };
        // The bytes, onto the hub's own disk, before the frame that names them exists.
        let mut carried = Vec::with_capacity(files.len());
        for sent in &files {
            let fetched = self.fetch(addr, sent).await;
            let _ = self.audit.file(addr, &fetched.file);
            carried.push(fetched);
        }
        let files_on_disk = carried.iter().filter(|f| f.file.path.is_some()).count() as u32;
        // Written down BEFORE the frame is on the wire, so the bridge's `ack` — which can arrive
        // the moment it is — always finds the record it names. Taken back if the send fails, so a
        // record never waits for an answer to a frame nothing received.
        let frame = Self::mint_frame_id();
        {
            let mut down = self.down.lock().await;
            down.push_back(Down {
                frame: frame.clone(),
                addr: addr.clone(),
                chat_id,
                msg_id: msg_id.clone(),
                what: His::Words { files_on_disk },
            });
            while down.len() > DOWN_KEPT {
                down.pop_front();
            }
        }
        // Taken BEFORE the frame goes down and held until the eyes have landed, so the bridge's
        // ack — which can arrive within a millisecond of the frame — cannot put its thumb on the
        // message ahead of the eyes and then have the eyes cover it. See `mark_permit`. Owned,
        // because it travels into the task that lands the eyes and is dropped there.
        let in_order = Arc::clone(&self.mark_permit).lock_owned().await;
        let delivered = self
            .deliver_under(
                addr,
                frame.clone(),
                HubFrame::Message {
                    msg_id: msg_id.clone(),
                    text: text.to_owned(),
                    from: hub_proto::From { chat_id, user_id },
                    in_reply_to_ask,
                    files: (!carried.is_empty())
                        .then(|| carried.iter().map(|f| f.file.clone()).collect()),
                },
            )
            .await;
        if !delivered {
            self.down.lock().await.retain(|w| w.frame != frame);
        }
        let _ = if delivered {
            self.audit.outcome(addr, &SendOutcome::Sent(msg_id.clone()))
        } else {
            self.audit.refused(addr, "the project was not connected")
        };
        // The eyes: his words are on their way, and nothing has answered for them yet. Only when
        // they actually went — a message nobody received gets its line in the topic, not a mark
        // that reads as a receipt.
        //
        // In a task of their own, and `relay` returns the moment the frame is down. Awaited here
        // they were one HTTPS round trip inside the update handler — which Telegram's dispatcher
        // runs one at a time per chat, so his next line and every tap in the forum waited behind
        // them, seventeen seconds of it when Telegram stalls — with the mark permit held the whole
        // time, so every other bridge's ack waited too. Before the receipts existed a successful
        // relay made no Telegram call at all. The permit goes with the eyes and is released when
        // they have landed, which is what keeps the thumb behind them.
        if delivered {
            let surface = Arc::clone(&self.surface);
            let reactions = Arc::clone(&self.reactions);
            let said = Arc::clone(&self.reaction_refusal_said);
            let msg_id = msg_id.clone();
            tokio::spawn(async move {
                mark_his_message(
                    &*surface,
                    &reactions,
                    &said,
                    chat_id,
                    &msg_id,
                    Mark::HandedOn,
                )
                .await;
                drop(in_order);
            });
        } else {
            drop(in_order);
        }
        // A file that did not come through, said under his message — only once the words have
        // gone, because when they have not he is already being told that nothing was sent, and
        // "the file did not reach the agent" under "nothing reached the agent" reads as two
        // failures where there was one.
        //
        // AFTER the permit has gone to the eyes, and never inside it. This is a full budgeted
        // send whose deadline is ninety seconds; held under the permit, one failed file stopped
        // every ack in the hub from putting a thumb on anything and every other conversation's
        // words from going down at all — for as long as the chat was thin, which is exactly when
        // he sends a screenshot at a busy forum. The permit exists to keep the thumb behind the
        // eyes; a sentence about a file is neither of them.
        if delivered {
            for f in carried.iter().filter(|f| f.file.path.is_none()) {
                self.say_the_file_did_not_come(addr, msg_id, &f.file, f.how_big)
                    .await;
            }
        }
        delivered
    }

    /// Put the stage a line of his has reached on the line itself.
    ///
    /// Through no SEND budget, by measurement: twenty reactions and a send still went through
    /// (`docs/RATE-PROBE.md` §3), so a reaction spends nothing an agent could have had. Reactions
    /// have a ceiling of their own, kept in [`Self::reactions`], and a mark past it — or one
    /// Telegram refuses anyway — is simply a mark that does not appear: never retried, never
    /// waited for, and never fed into the send budget as a flood wait. The measurement showed a
    /// chat shut for reactions still taking sends, and holding every agent off for a refused
    /// decoration would be a loss with nothing behind it. Nothing is said in the topic in its
    /// place: the line an agent's refusal earns is posted whether or not the mark landed.
    async fn mark_his_message(&self, chat_id: i64, msg_id: &MsgId, mark: Mark) {
        mark_his_message(
            &*self.surface,
            &self.reactions,
            &self.reaction_refusal_said,
            chat_id,
            msg_id,
            mark,
        )
        .await;
    }

    /// The question one of his replies is under, when it is one THIS conversation's live session
    /// asked — else nothing, and his words are a line like any other.
    ///
    /// Looked up by the message he replied to, in this chat, in the ledger every keyboard is written
    /// down in. Two things have to hold before the bridge is told, and both fail closed:
    ///
    /// * The record is this conversation's. A project and a lane of it are two agents, and a reply
    ///   under one's question must not reach the other as a reply to something it asked.
    /// * The record was written by the run that is connected NOW. A bridge mints its ask ids from
    ///   a counter that starts over with the process, so `a1` from a session that has since
    ///   restarted names a different question in the one running — and a reply under the old
    ///   question would be handed to the new session as a reply to whatever it called `a1`.
    ///
    /// Every message in a forum topic carries the topic's root as its reply, so most of the time
    /// this is asked about a message nothing was written down beside, and answers nothing.
    async fn ask_replied_to(&self, addr: &Addr, chat_id: i64, under: &MsgId) -> Option<AskId> {
        let record = self.ledger.lock().await.get(chat_id, under).cloned()?;
        if !record.addr_is(addr) {
            return None;
        }
        let claims = self.claims.lock().await;
        let claim = claims.get(addr)?;
        (claim.instance == record.instance).then(|| record.ask_id.clone())
    }

    /// The topic a conversation's messages go in, created and greeted on first use.
    ///
    /// Created here rather than at `hello` for one reason: a topic with no messages is invisible in
    /// Telegram's topic list, so a topic created for a bridge that then vanished is the same as no
    /// topic at all to the person looking for it — except that it is now bound.
    ///
    /// A LANE GETS ITS OWN, so a worktree's rolling context is in one place. The cost was weighed
    /// and accepted: roughly twelve permanent topics a day, and nothing here ever deletes one.
    ///
    /// **Three things here exist because a lane needs a new topic twelve times a day where a
    /// project needed one once in its life**, which turned each of them from a once-per-project
    /// hazard into a daily one:
    ///
    /// * The creation is METERED, so a burst of arriving worktrees cannot spend the chat's real
    ///   ceiling behind the back of the budget that is supposed to be guarding it.
    /// * A creation Telegram refused is REMEMBERED for a short while, because this runs on every
    ///   message: without it, a worktree whose topic could not be made asked Telegram for one per
    ///   message it sent, with no backoff, for as long as its agent kept talking.
    /// * A creation that FAILED is logged with the title, because a `createForumTopic` whose reply
    ///   was lost may have made the topic anyway — and that topic is in his forum with nothing
    ///   pointing at it. Greppable is the least this can be.
    pub async fn topic_for(&self, addr: &Addr, until: std::time::Instant) -> Result<i32, NoTopic> {
        let (existing, title, colour, project_title) = {
            let registry = self.registry.lock().await;
            let p = registry
                .get(&addr.project)
                .ok_or_else(|| NoTopic::Failed("that project is not enrolled".to_owned()))?;
            let title = self.display_title(p, addr.lane.as_ref());
            // The COLOUR stays the project's. Telegram gives six, and a project with its lanes
            // beneath it reads as one block in the list only if they share one.
            (
                registry.topic_of(addr),
                title,
                p.icon_color,
                self.display_title(p, None),
            )
        };
        if let Some(id) = existing {
            return Ok(id);
        }

        // Refused a moment ago, so do not ask again yet. Nothing about this conversation has
        // changed since, and the cost of asking is a Telegram call per message the agent sends.
        if let Some(when) = self.topic_refused.lock().await.get(addr) {
            if when.elapsed() < TOPIC_RETRY_AFTER {
                return Err(NoTopic::Failed(
                    "Telegram would not make a topic for this a moment ago".to_owned(),
                ));
            }
        }

        // One token before the call, not after. Refused, nothing is created at all — which is the
        // right answer: a topic made in a minute whose budget cannot pay to greet it is bound,
        // permanent, and invisible, because Telegram does not show an empty topic in the list.
        // Any refusal at all stops this, not only the one shape it returns today. Matching a single
        // variant here would mean a future one fell through and made the topic anyway, which is the
        // fail-open half of exactly the thing being closed.
        if let Err(refused) = self.take_a_turn(addr, until).await {
            return Err(match refused {
                SendOutcome::TooFast(wait) => NoTopic::TooFast(wait),
                SendOutcome::SwitchedOff => NoTopic::SwitchedOff,
                other => NoTopic::Failed(format!("{other:?}")),
            });
        }

        let id = match self.surface.create_topic(&title, colour).await {
            Ok(id) => id,
            Err(e) => {
                // A flood wait discovered HERE is still the whole chat's, and it used to be lost:
                // this call is metered like a send but its failure was an opaque sentence, so a
                // `429` on it left every other project sending into a chat that was already shut.
                // The sixty-second memo below happened to be roughly the right backoff for this
                // address; it did nothing whatever for any other.
                if let Some(wait) = e.flood_wait {
                    self.budgets.lock().await.flood_wait(
                        self.forum_chat,
                        std::time::Instant::now(),
                        wait,
                    );
                    // NOT memoised, and NOT a refusal. Draining the budget was only half of this:
                    // the other half returned `Failed`, which the bridge renders as "his messaging
                    // app would not take it — it will not be tried again" about a chat that reopens
                    // in under a minute. And the memo made a temporary CHAT-wide condition look
                    // like a permanent per-ADDRESS one, so a worktree arriving during a flood wait
                    // had its whole first minute of output acked as permanently dead. The block
                    // just set is the correct backoff and it covers every conversation, which is
                    // the scope the trouble actually has.
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(), title = %title,
                        seconds = wait.as_secs(),
                        "Telegram is refusing this chat for flooding, discovered while making a \
                         topic; the whole chat is held off for that long"
                    );
                    let _ = self.audit.refused(addr, &e.why);
                    return Err(NoTopic::TooFast(wait));
                }
                self.topic_refused
                    .lock()
                    .await
                    .insert(addr.clone(), std::time::Instant::now());
                let _ = self.audit.refused(addr, &e.why);
                tracing::error!(
                    project = %addr.project, lane = addr.lane_field(), title = %title, error = %e.why,
                    "asked Telegram for a topic and did not get one; if it was made anyway it is \
                     in the forum with nothing pointing at it"
                );
                return Err(NoTopic::Failed(e.why));
            }
        };
        self.topic_refused.lock().await.remove(addr);
        // A bind that fails must not be reported as a topic. Returning Ok here meant the next
        // message created ANOTHER topic, and the one before it was orphaned — one new empty topic
        // per message, for as long as the registry stayed unreadable, with every message dropped.
        if let Err(e) = self.registry.lock().await.bind_topic(addr, id) {
            tracing::error!(
                project = %addr.project, lane = addr.lane_field(), topic = id, error = %e,
                "made a topic and could not write it down; it is orphaned and no message will be \
                 sent until the registry is readable again"
            );
            return Err(NoTopic::Failed(e.to_string()));
        }
        // Greeted immediately, in the same breath as being created — the greeting is what makes the
        // topic appear in the list at all. Through the SAME budgeted, audited path as everything
        // else: a greeting that skipped the budget was a writer the ceiling could not see, and one
        // that skipped the audit was a send with no record, which is the one thing the audit
        // discipline exists to make impossible.
        //
        // A lane's greeting NAMES the lane, because it is the first thing in a brand-new topic
        // and twelve of them arrive on a dispatch day. Identical, they are twelve notifications he
        // cannot tell apart, and the only thing left carrying which one is a topic title that a
        // phone row truncates. The name is the same string `/projects` and the title show him.
        let greeting = match &addr.lane {
            None => format!("{project_title} is connected."),
            Some(lane) => format!(
                "{lane} is connected — a separate conversation of {project_title}. It talks here, \
                 not in the project's own topic."
            ),
        };
        // The greeting's outcome is READ, not discarded. A topic that was made, written down, and
        // never greeted is a conversation he cannot find at all: Telegram does not list an empty
        // one. Without this the hub could not tell that state from a healthy topic.
        let greeting = Outbound::Words {
            text: &greeting,
            buttons: &[],
            reply_to: None,
        };
        match self.send_into(addr, id, &greeting, until).await {
            SendOutcome::Sent(_) | SendOutcome::Clamped(_) => {}
            outcome => tracing::error!(
                project = %addr.project, lane = addr.lane_field(), topic = id, title = %title,
                ?outcome,
                "made a topic and could not greet it, so it is bound but does not appear in his \
                 forum list; the next message into it is what will make it visible"
            ),
        }
        Ok(id)
    }

    /// What a conversation is CALLED, wherever a person reads it: the topic's title, the greeting,
    /// the log's subject, the throttle notice. One function, because three places composed it on
    /// their own and would otherwise disagree about what a room is called.
    ///
    /// A conversation's own name is the title whoever holds it wrote — display only, read once
    /// per run and then remembered (see `titles`), shape-refused — and the registry's when there
    /// is none it can show. A lane's name hangs off that, clipped the way `lane_title` clips.
    fn display_title(&self, p: &crate::registry::Project, lane: Option<&LaneId>) -> String {
        let own = {
            // A poisoned lock holds a map, not a half-written one: every write here is one
            // insert, so what a panicked holder left behind is still what was read.
            let mut titles = self.titles.lock().unwrap_or_else(|e| e.into_inner());
            titles
                .entry(p.id.clone())
                .or_insert_with(|| self.conversations.title_of(&p.id))
                .clone()
        }
        .unwrap_or_else(|| p.title.clone());
        match lane {
            None => own,
            Some(lane) => crate::registry::lane_title(&own, lane),
        }
    }

    /// The one place an agent's message actually goes out: budget, clip, audit, send, audit.
    ///
    /// **Every hub-owned message that can be REFUSED goes through here**, and the ceiling is per
    /// chat — so a writer this cannot see does not cost itself, it costs whichever project happens
    /// to send next. That sentence used to claim every hub-owned WRITE, which was untrue and is what
    /// let unmetered writers accumulate. The list from `docs/MULTIPLEXER-READINESS.md` §3, settled:
    ///
    /// * **Topic creation** does not come through here — it is not a message and has no text to
    ///   clip — but it takes a token through [`Self::take_a_turn`] first, so the ceiling sees it.
    /// * **The retirement edits** are not metered and must not be. `editMessageText` is not charged
    ///   against the per-minute ceiling at all: measured 3 September, thirty edits straight after
    ///   five sends, none refused, and a send still went through afterwards. The audit graded these
    ///   as the write that scales, on the stated assumption that an edit spends from the send
    ///   budget; it does not, so the fix there was this paragraph rather than the code. Metering
    ///   them would buy nothing and cost the one write that must never be refused — a keyboard that
    ///   cannot be taken off is a menu answering a question nobody is waiting for any more.
    /// * **The three real sends in `bot.rs`** — the tap confirmation, the reply to something typed
    ///   at a disconnected topic, and a command's answer — cannot come through here either, because
    ///   they are the operator's own and refusing him is not on the table. They call
    ///   [`Self::account_for_a_send_that_could_not_be_refused`] instead, which takes the token
    ///   without the right to say no, **for the chat the message is actually going to** — the
    ///   allowlist holds more than the forum. They also read what Telegram answered and hand a
    ///   flood wait to [`Self::telegram_shut_this_chat`]: metering them and then discarding their
    ///   `429` left the backpressure below with two blind spots out of five paths, and they fire
    ///   precisely when the chat is thin, because the traffic is what made him tap.
    /// * **The throttle line** is a send and is metered, out of the reserve that exists for it.
    /// * **`answerCallbackQuery`** is deliberately outside all of this. Whether it is charged at
    ///   all is unmeasured (`docs/RATE-PROBE.md`), it is one per tap and paced by a thumb, and
    ///   spending a send for something that may cost nothing takes it off an agent for no reason.
    /// * **The reactions on his own messages** are not sends and are not metered here — measured
    ///   5 September (`docs/RATE-PROBE.md` §3): twenty of them and a send still went through. They
    ///   have a ceiling of their own and a ledger of their own, [`Self::reactions`], and a mark
    ///   that ledger or Telegram refuses does not appear and costs nobody anything.
    async fn send_into(
        &self,
        addr: &Addr,
        topic_id: i32,
        out: &Outbound<'_>,
        until: std::time::Instant,
    ) -> SendOutcome {
        // Clipped here rather than by the surface, because whether anything was lost is a fact the
        // BRIDGE has to be told, and only this side is holding the ack. A caption has a ceiling of
        // its own, and the caller has already chosen to send the words separately when they are
        // over it, so the clip on a caption is a backstop rather than the rule.
        let (text, clamped) = match out {
            Outbound::Words { text, .. } => crate::queue::fit(text, crate::queue::MAX_TEXT),
            Outbound::File { caption, .. } => crate::queue::fit(caption, CAPTION_MAX),
        };

        // A loop, because Telegram's own answer can be "not yet" — and when it is, the wait it
        // names goes into the budget and this frame queues again behind it, exactly like any other
        // waiter. It is bounded twice over: by the shelf life, which `take_a_turn` checks before it
        // sleeps at all, and by the flood wait itself, which can only ever move further out. Two
        // attempts past the first is already more than any real 429 sequence needs, and a fixed
        // ceiling means a Telegram that answered strangely cannot spin this.
        for _ in 0..3 {
            if let Err(too_fast) = self.take_a_turn(addr, until).await {
                return too_fast;
            }
            let mut outcome = match out {
                Outbound::Words {
                    buttons, reply_to, ..
                } => {
                    let _ = self.audit.sent(addr, topic_id, text.len());
                    self.surface.send(topic_id, &text, buttons, *reply_to).await
                }
                // A file is a send, audited and metered exactly like words: it takes the turn
                // above, and the flood-wait drain and the rebinding below read its outcome too.
                Outbound::File { upload, name, .. } => {
                    let _ = self
                        .audit
                        .sent_file(addr, topic_id, upload.bytes.len(), name);
                    self.surface.send_file(topic_id, upload, &text).await
                }
            };
            if clamped && let SendOutcome::Sent(id) = outcome {
                outcome = SendOutcome::Clamped(id);
            }
            let _ = self.audit.outcome(addr, &outcome);

            // Our budget said yes and Telegram said no, so Telegram is right and our accounting was
            // wrong. Draining the chat by what it named is the whole of the backpressure: without
            // it every other project kept sending into the same wall, each one earning the next
            // refusal, for as long as the herd had anything to say.
            //
            // Both write the same `shed` line, and the audit still tells them apart: a flood wait
            // always follows a `sent` line for the same subject, because it happened after a real
            // attempt, and our own give-up never does.
            if let SendOutcome::TooFast(wait) = outcome {
                self.budgets.lock().await.flood_wait(
                    self.forum_chat,
                    std::time::Instant::now(),
                    wait,
                );
                continue;
            }
            if matches!(outcome, SendOutcome::Sent(_) | SendOutcome::Clamped(_)) {
                self.something_got_through().await;
            }
            return outcome;
        }
        // Three times into a wall that keeps moving. Told the chat's own answer to "when could
        // anything go out", which is the only honest number available here.
        let outcome = SendOutcome::TooFast(self.what_the_queue_costs().await);
        let _ = self.audit.outcome(addr, &outcome);
        outcome
    }

    /// A frame refused because its project was switched off while it waited. Written down, like
    /// every other way a message does not go out.
    fn switched_off_under(&self, addr: &Addr) -> SendOutcome {
        let outcome = SendOutcome::SwitchedOff;
        let _ = self.audit.outcome(addr, &outcome);
        outcome
    }

    /// Wait for this sender's turn in the queue and spend one of the chat's tokens, or shed.
    ///
    /// Lifted out of `send_into` so that MAKING A TOPIC can pay for one too. `create_forum_topic`
    /// is a write to the same chat and Telegram counts it whether this code does or not; unmetered,
    /// twelve worktrees arriving on a dispatch day spent twelve calls the ceiling could not see, and
    /// the ceiling is per chat — so what an unmetered writer costs is not itself, it is whichever
    /// project sends next. A brand-new conversation now pays two tokens before its first word, and
    /// that is the honest price of a topic per lane rather than a hidden one.
    ///
    /// `until` is the whole message's deadline, shared with every turn it has to take. A brand-new
    /// conversation queues three times — the topic, the greeting, then the message — and each of
    /// those used to start a fresh ten-second clock of its own, so its real bound was whatever
    /// nobody had added up. One deadline, minted once in [`Self::say`], is the honest version.
    async fn take_a_turn(&self, addr: &Addr, until: std::time::Instant) -> Result<(), SendOutcome> {
        // PACE, then shed — and pace in a QUEUE, not as a crowd.
        //
        // Telegram's limit has two halves: no more than one message a second, and about twenty a
        // minute. The first is a rhythm — wait a beat and the message still arrives — the second is
        // a real ceiling. Only the ceiling should ever lose a message.
        //
        // The first attempt at this let each sender sleep once and then shed. That reads fine and
        // is wrong the moment there are two projects: every waiter sleeps the SAME second, they all
        // wake together, one wins the token, and the rest have already used their single sleep and
        // fall through to the shed. Measured on the real code: ten projects saying one thing each
        // produced two sends and eight sheds, with sixteen of the eighteen per-minute tokens
        // unspent. Six bridges opening with a question left five agents blocked and four topics
        // bound-but-empty, which Telegram does not show in the topic list at all.
        //
        // The permit is what makes it a queue, and a strictly FIFO one: `tokio::sync::Mutex`
        // documents that guarantee rather than merely having it, so a turn belongs to whoever asked
        // for it first and no project's backlog can push another project's single line behind it.
        // Each sender waits its own turn once instead of racing the others for one token.
        //
        // **The deadline is a shelf life now, not a queue position.** It used to be a flat ten
        // seconds starting here, which is about ten waiters — so a message was thrown away for
        // being eleventh, with the per-minute ceiling untouched, and its agent was told it had been
        // sent too fast. Neither half of that was true. What replaces it is how long the message is
        // still worth sending, which is a property of the message and not of the crowd in front of
        // it. The deadline still covers the QUEUE and not merely the sleep, because the queue is
        // most of the wait: bounding only the sleep let a bridge sit behind nine others for a minute
        // and then be told it was too fast.
        //
        // **Both waits below end early if the project is switched off** — see `ConnectionSwitch`.
        // Nothing has been spent at either, so the frame is refused like the ones queued behind it
        // rather than posted into a topic the operator has just turned off.
        let switch = SWITCH.try_with(Arc::clone).ok();
        let turn = match unless_switched_off(
            switch.as_deref(),
            tokio::time::timeout_at(until.into(), self.send_permit.lock()),
        )
        .await
        {
            None => return Err(self.switched_off_under(addr)),
            Some(Ok(t)) => t,
            Some(Err(_)) => {
                // Told how long it actually waited, not a constant. `MAX_PACE_WAIT` went out here
                // as the retry_after, which was a made-up number unrelated to the real wait.
                let outcome = SendOutcome::TooFast(self.what_the_queue_costs().await);
                let _ = self.audit.outcome(addr, &outcome);
                return Err(outcome);
            }
        };
        let mut gave_up = None;
        loop {
            let verdict = {
                let mut budgets = self.budgets.lock().await;
                budgets.take(
                    self.forum_chat,
                    std::time::Instant::now(),
                    crate::queue::Spender::AnAgent,
                )
            };
            match verdict {
                Ok(()) => break,
                // Both refusals are now WAITED OUT rather than one of them being a give-up. The
                // ceiling used to end a message on the spot however briefly it had to wait, which
                // is what made a chat one message over its budget lose the next thing anyone said.
                // What decides is whether the wait fits inside what this message is worth, and the
                // second half of that condition is the one that used to lie: a sender that had
                // queued nine and a half seconds was refused for needing a one-second gap and told
                // to come back in under a second, when the truth was its position in the queue.
                //
                // Sleeping out a CEILING while holding the permit is deliberate and costs nothing:
                // while the chat's bucket is empty the waiters behind cannot send either, and when
                // a token does arrive the head of the queue is exactly who should have it.
                Err(refusal) if std::time::Instant::now() + refusal.wait() <= until => {
                    if unless_switched_off(switch.as_deref(), tokio::time::sleep(refusal.wait()))
                        .await
                        .is_none()
                    {
                        drop(turn);
                        return Err(self.switched_off_under(addr));
                    }
                }
                // Past its shelf life. For prose that means a minute and a half of a chat that
                // would not take it; for a question it means the answer would arrive too late to be
                // worth having.
                Err(refusal) => {
                    gave_up = Some(refusal);
                    break;
                }
            }
        }

        // The permit is released HERE, before the network call. It exists to order the waiting, not
        // to serialise Telegram: holding it across `surface.send` made one slow round trip a pause
        // for every other project's read loop.
        drop(turn);
        if let Some(refusal) = gave_up {
            let outcome = SendOutcome::TooFast(refusal.wait());
            let _ = self.audit.outcome(addr, &outcome);
            return Err(outcome);
        }
        Ok(())
    }

    /// Send into a conversation's topic, with the audit around it and one rebinding if the topic is
    /// gone.
    ///
    /// How long it is worth waiting to send is read off the message's own shape — see
    /// [`Perishable::of`]. The one caller that knows better than its buttons uses [`Self::say_as`].
    pub async fn say(&self, addr: &Addr, text: &str, buttons: &[AskOption]) -> SendOutcome {
        self.say_as(addr, text, buttons, Perishable::of(buttons))
            .await
    }

    /// One line about one of HIS messages, threaded under it.
    ///
    /// For the one thing the hub says that is about a message he typed rather than something an
    /// agent said: that his words reached nobody. Two lines typed a second apart, one of them
    /// refused, and a bare post names neither; under the line it is about, it cannot be read as
    /// being about the other.
    pub async fn say_under(&self, addr: &Addr, text: &str, reply_to: &MsgId) -> SendOutcome {
        self.say_as_under(addr, text, &[], Perishable::of(&[]), Some(reply_to))
            .await
    }

    /// The same, for a caller that knows what kind of thing it is saying.
    ///
    /// There is exactly one: an `ask` whose agent minted no options is still a QUESTION — the
    /// operator answers it by typing rather than by tapping — and its buttons cannot say so.
    pub async fn say_as(
        &self,
        addr: &Addr,
        text: &str,
        buttons: &[AskOption],
        kind: Perishable,
    ) -> SendOutcome {
        self.say_as_under(addr, text, buttons, kind, None).await
    }

    /// Everything a send is, with the one thing only [`Self::say_under`] supplies.
    async fn say_as_under(
        &self,
        addr: &Addr,
        text: &str,
        buttons: &[AskOption],
        kind: Perishable,
        reply_to: Option<&MsgId>,
    ) -> SendOutcome {
        self.say_outbound(
            addr,
            &Outbound::Words {
                text,
                buttons,
                reply_to,
            },
            kind,
        )
        .await
    }

    /// A file an agent attached, with the words as its caption, into the conversation's topic.
    /// Prose: late is fine, missing is not — and a file that missed its turn is said to have.
    async fn say_file(
        &self,
        addr: &Addr,
        upload: &Upload,
        name: &str,
        caption: &str,
    ) -> SendOutcome {
        self.say_outbound(
            addr,
            &Outbound::File {
                upload,
                name,
                caption,
            },
            Perishable::Prose,
        )
        .await
    }

    /// One thing somebody wanted to say — words or a file — with its deadline and its loss count.
    async fn say_outbound(&self, addr: &Addr, out: &Outbound<'_>, kind: Perishable) -> SendOutcome {
        // Minted ONCE, here, and shared by every turn this message has to take. A brand-new
        // conversation queues three times before its first word — the topic, the greeting, then the
        // message — and each of those used to start a deadline of its own.
        let until = std::time::Instant::now() + kind.shelf_life();
        let outcome = self.try_to_say(addr, out, until).await;
        // THE ONE PLACE A LOST MESSAGE IS COUNTED. One call to this function is one thing somebody
        // wanted to say, however many turns it took, so counting here is what makes the number he
        // reads the number of messages he missed. Every give-up below funnels into exactly one
        // `TooFast`, and the writes that are not a message — the topic taken at connection time,
        // the greeting inside it — never come through here at all.
        if let SendOutcome::TooFast(_) = outcome {
            self.note_a_message_nobody_will_see(addr).await;
        }
        outcome
    }

    /// Everything [`Self::say_as`] does except counting the loss. Split out so that the counting
    /// has exactly one place to happen and every early return passes through it.
    async fn try_to_say(
        &self,
        addr: &Addr,
        out: &Outbound<'_>,
        until: std::time::Instant,
    ) -> SendOutcome {
        let topic_id = match self.topic_for(addr, until).await {
            Ok(id) => id,
            // The two are not one: a budget shed mends itself in a minute and is acked
            // `too_fast`, a Telegram refusal does not and is acked as one.
            Err(NoTopic::TooFast(wait)) => {
                let outcome = SendOutcome::TooFast(wait);
                let _ = self.audit.outcome(addr, &outcome);
                return outcome;
            }
            // Already written down by `take_a_turn`, and not Telegram's refusal: the ack must
            // not blame Telegram for the operator's own switch.
            Err(NoTopic::SwitchedOff) => return SendOutcome::SwitchedOff,
            Err(e) => {
                let _ = self.audit.refused(addr, &e.to_string());
                return SendOutcome::Refused(e.to_string());
            }
        };
        let outcome = self.send_into(addr, topic_id, out, until).await;

        if outcome == SendOutcome::TopicGone {
            // Exactly once, and never as a retry: Telegram gives no service message when a topic is
            // deleted and no way to list them, so this is first-class rebinding. Treating it as a
            // transient would make the project's messages disappear quietly and forever.
            tracing::warn!(
                project = %addr.project, lane = addr.lane_field(),
                "the topic is gone; making a new one"
            );
            let _ = self.registry.lock().await.unbind_topic(addr);
            // The SAME deadline. A rebinding is three more turns — a topic, a greeting, and the
            // message again — and giving them a fresh shelf life would let one message spend two of
            // them, which is the doubling this deadline was moved out of `take_a_turn` to stop.
            return match self.topic_for(addr, until).await {
                Ok(fresh) => self.send_into(addr, fresh, out, until).await,
                // The ceiling refusing the rebinding is a busy chat, not a missing topic, and
                // saying "there is nowhere in his chat to put it" about it tells the agent to give
                // up on a thing that mends itself inside a minute.
                Err(NoTopic::TooFast(wait)) => SendOutcome::TooFast(wait),
                Err(NoTopic::SwitchedOff) => SendOutcome::SwitchedOff,
                Err(_) => outcome,
            };
        }
        outcome
    }

    /// Roughly how long a turn is worth to whoever could not get one, for the ack's `retry_after`.
    ///
    /// The queue's depth is not knowable from inside it — the permit does not count its waiters —
    /// so this is the chat's own answer to "when could anything go out", which is the thing the
    /// caller actually wants and is honest either way. It never returns zero: a caller told to come
    /// back immediately comes back immediately, and does the same thing again.
    async fn what_the_queue_costs(&self) -> Duration {
        let refusal = self.budgets.lock().await.would_refuse(
            self.forum_chat,
            std::time::Instant::now(),
            crate::queue::Spender::AnAgent,
        );
        // No refusal means there is room right now and the whole wait was the crowd, so the rhythm
        // is the honest floor: one more turn is at least one gap away.
        refusal.map_or(crate::queue::MIN_GAP, |r| {
            r.wait().max(crate::queue::MIN_GAP)
        })
    }

    /// Account for a send the hub has already decided to make and cannot call back.
    ///
    /// The operator tapped something, or typed something, and the answer to it is not an agent's
    /// message to be rationed — but it IS a real message against a ceiling that belongs to the whole
    /// chat, so an unmetered one does not cost itself. It costs whichever project sends next, and
    /// these fire precisely while he is looking at a busy forum, which is exactly when the budget is
    /// thin. They do not scale with the size of the herd: one per tap, one per line he types at a
    /// topic with nothing behind it, one per command.
    /// `chat_id` is the chat the message actually went to, and passing the wrong one is not a
    /// rounding error. The allowlist can hold more than the forum — the live box's does — so a
    /// `/help` typed in the operator's other chat used to take a send off the forum's ceiling for a
    /// message the forum never carried, and impose the one-second rhythm on it too. `Budgets` is
    /// keyed per chat precisely so that does not have to happen.
    pub async fn account_for_a_send_that_could_not_be_refused(&self, chat_id: i64) {
        self.budgets
            .lock()
            .await
            .spend(chat_id, std::time::Instant::now());
    }

    /// Telegram has shut a chat for flooding. Hold everything off it for as long as it said.
    ///
    /// Public because the three sends the hub is not allowed to refuse live in `bot.rs`, and until
    /// this existed they threw their `429` away: the budget heard about flood waits from the agent
    /// path and from nowhere else, so a refusal discovered on the operator's own tap left the very
    /// next agent message walking into the same wall. Those fire exactly when the chat is thin,
    /// because he is tapping in response to traffic.
    pub async fn telegram_shut_this_chat(&self, chat_id: i64, wait: Duration) {
        tracing::warn!(
            seconds = wait.as_secs(),
            "Telegram is refusing this chat for flooding; holding everything off it"
        );
        self.budgets
            .lock()
            .await
            .flood_wait(chat_id, std::time::Instant::now(), wait);
    }

    /// One more message the operator will never see. Open the window, or move the count in it.
    ///
    /// **Called once per MESSAGE somebody wanted to send, from [`Self::say_as`] and from nowhere
    /// else.** It used to live inside [`Self::take_a_turn`] and [`Self::send_into`], which is one
    /// layer too low: those are the turn-taking primitives for every hub-owned write, so a brand-new
    /// conversation counted the topic's turn AND its greeting AND the message as three losses for
    /// one frame, and a worktree that merely connected during a busy minute counted one for a
    /// message no agent had said and no agent was told about. The number he reads has to be the
    /// number of things he missed, and the sentence saying nothing is waiting on him has to be true
    /// of every one of them.
    async fn note_a_message_nobody_will_see(&self, addr: &Addr) {
        // Named BEFORE the throttle lock is taken. Two locks held at once is two locks somebody has
        // to reason about the order of, and there is no reason to here.
        let named = {
            let registry = self.registry.lock().await;
            registry
                .get(&addr.project)
                .map(|p| self.display_title(p, addr.lane.as_ref()))
        };

        let deferred = {
            let mut throttle = self.throttle.lock().await;
            // A window that has been quiet for its cooling-off period is over, whatever is
            // happening now. Closed first so that a new one starts from one rather than inheriting
            // a count from something the operator watched end ten minutes ago — and never while a
            // debt is outstanding, which `close_the_window_if_it_has_passed` refuses.
            self.close_the_window_if_it_has_passed(&mut throttle).await;

            throttle.lost += 1;
            throttle.last_loss = Some(std::time::Instant::now());
            match named {
                Some(title) => {
                    throttle.from.insert(title);
                }
                None => throttle.a_loss_it_could_not_name = true,
            }
            self.book_or_write(&mut throttle)
        };
        self.write_the_count_when_the_rhythm_allows(deferred).await;
    }

    /// Something got through, so the chat may have caught up. Free to check, free to act on.
    async fn something_got_through(&self) {
        let mut throttle = self.throttle.lock().await;
        if throttle.notice.is_none() && !throttle.owed {
            return;
        }
        // THE DEBT FIRST. A window that could not be reported when it happened is still a window he
        // was never told about, and closing it first is what destroyed the only record that it
        // happened: the reset clears `owed` and `lost` together, and the `if` below then saw a debt
        // that had already been thrown away. Paying first and closing second leaves him with one
        // message that says what was lost, immediately edited into the line saying it is over —
        // a send and a free edit, which is the shape this design already pays for.
        if throttle.owed {
            self.open_or_update(&mut throttle).await;
        }
        self.close_the_window_if_it_has_passed(&mut throttle).await;
    }

    /// Decide who writes the count and when: now, later, or not at all.
    ///
    /// Returns how long to wait before writing, or `None` for "somebody else is already going to".
    /// Opening the window is never deferred — it is a send, and the send is the only part of this
    /// he ever feels.
    fn book_or_write(&self, throttle: &mut Throttle) -> Option<Duration> {
        if throttle.notice.is_none() {
            return Some(Duration::ZERO);
        }
        let since = throttle
            .last_edit
            .map_or(THROTTLE_EDIT_GAP, |when| when.elapsed());
        if since >= THROTTLE_EDIT_GAP {
            return Some(Duration::ZERO);
        }
        if throttle.flush_booked {
            return None;
        }
        throttle.flush_booked = true;
        Some(THROTTLE_EDIT_GAP - since)
    }

    /// Wait out whatever [`Self::book_or_write`] asked for, then put the current count in front of
    /// him.
    ///
    /// The wait is at most the chat's own rhythm, it falls on a frame that was already being given
    /// up on, and only one task in the whole hub is ever doing it — so what it costs is a second on
    /// one connection's read loop, once per second, in exchange for the number in front of him
    /// being the number he actually lost.
    async fn write_the_count_when_the_rhythm_allows(&self, deferred: Option<Duration>) {
        let Some(wait) = deferred else { return };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        let mut throttle = self.throttle.lock().await;
        throttle.flush_booked = false;
        // Re-read rather than assume. Anything at all may have happened while this was asleep, and
        // the one that would be unforgivable is a window closing in the meantime: the reset takes
        // the count back to nothing, and writing then would put a brand-new message in his forum
        // saying no messages did not get through. Nothing left to say is a perfectly good answer.
        if throttle.lost > throttle.last_written {
            self.open_or_update(&mut throttle).await;
        }
    }

    /// Put the line in front of him, or move the number in the line already there.
    ///
    /// The send is the only part that costs anything, it happens once per window, and it is paid
    /// for out of the token held back from the agents for exactly this. Everything after it is an
    /// edit, which is free.
    async fn open_or_update(&self, throttle: &mut Throttle) {
        match throttle.notice.clone() {
            None => {
                // The reserved token, and the one-second rhythm is WAITED OUT rather than treated
                // as a refusal. That distinction is load-bearing here in a way it is nowhere else:
                // the likeliest moment to want this line is immediately after a send — a message
                // got through, which is how the hub notices it is owed one — and at that instant
                // the gap is refusing everybody by construction. Read as a refusal, the line would
                // be owed for ever while the chat was busy, which is exactly when it is needed.
                //
                // A CEILING refusal is different and is not waited out: it means Telegram has the
                // chat shut, and while that is true nothing at all goes out, so the reserve buys
                // nothing. That is owed rather than lost, and the next opportunity takes it.
                let mut allowed = false;
                for _ in 0..3 {
                    let verdict = {
                        let mut budgets = self.budgets.lock().await;
                        budgets.take(
                            self.forum_chat,
                            std::time::Instant::now(),
                            crate::queue::Spender::TheHub,
                        )
                    };
                    match verdict {
                        Ok(()) => {
                            allowed = true;
                            break;
                        }
                        Err(crate::queue::Refusal::Gap(wait)) => {
                            tokio::time::sleep(wait).await;
                        }
                        Err(crate::queue::Refusal::Ceiling(_)) => break,
                    }
                }
                if !allowed {
                    throttle.owed = true;
                    return;
                }
                let whose = conversations_that_lost_something(
                    &throttle.from,
                    !throttle.a_loss_it_could_not_name,
                );
                match self
                    .surface
                    .say_in_general(&throttle_line(throttle.lost, &whose))
                    .await
                {
                    SendOutcome::Sent(id) | SendOutcome::Clamped(id) => {
                        throttle.notice = Some(id);
                        throttle.owed = false;
                        throttle.last_written = throttle.lost;
                        throttle.last_edit = Some(std::time::Instant::now());
                    }
                    outcome => {
                        // Not lost quietly. This is the one message whose whole purpose is that a
                        // silence gets explained, so a silence here is the worst one in the file.
                        if let SendOutcome::TooFast(wait) = outcome {
                            self.budgets.lock().await.flood_wait(
                                self.forum_chat,
                                std::time::Instant::now(),
                                wait,
                            );
                        }
                        throttle.owed = true;
                        tracing::error!(
                            ?outcome,
                            lost = throttle.lost,
                            "the chat is over its ceiling and the line saying so could not go out; \
                             he is watching messages not arrive with nothing to explain it"
                        );
                    }
                }
            }
            Some(id) => {
                // Nothing to say if the number has not moved since it was last written. That is
                // the ordinary case for every loss in a burst except the one that booked the
                // write: they all queue behind it, and by the time they get here it has already
                // put their number in front of him.
                if throttle.lost == throttle.last_written {
                    return;
                }
                // Free against the per-minute ceiling, but still an HTTPS call — and fourteen
                // agents shedding at once should not be fourteen of them. He is reading a number,
                // not a ticker. Deferred rather than dropped: see [`THROTTLE_EDIT_GAP`] and
                // [`Self::book_or_write`], which is what decides who arrives here and when.
                let whose = conversations_that_lost_something(
                    &throttle.from,
                    !throttle.a_loss_it_could_not_name,
                );
                if let Err(e) = self
                    .surface
                    .rewrite(&id, &throttle_line(throttle.lost, &whose))
                    .await
                {
                    tracing::warn!(error = %e, "the count of what he is missing could not be updated");
                    return;
                }
                throttle.last_written = throttle.lost;
                throttle.last_edit = Some(std::time::Instant::now());
            }
        }
    }

    /// If nothing has been lost for a whole cooling-off window, say so and close it.
    ///
    /// One last free edit, on the message that is already there. Deliberately NOT a new message: a
    /// second notification saying the trouble is over is a second interruption for a fact he did not
    /// ask to be woken for, and it would cost a token from the budget that has just recovered.
    async fn close_the_window_if_it_has_passed(&self, throttle: &mut Throttle) {
        let quiet = throttle
            .last_loss
            .is_some_and(|when| when.elapsed() >= THROTTLE_WINDOW);
        if !quiet {
            return;
        }
        // A window nobody has ever been shown cannot be closed, because closing resets the record
        // and the record is the only account he will ever get of that minute. This is the ordinary
        // aftermath of a flood, not a corner: being told too_fast is precisely what makes an agent
        // stop talking, so a herd that has just been silenced going quiet for a cooling-off window
        // is the shape of the event. The debt waits for the next opportunity instead.
        if throttle.owed && throttle.notice.is_none() {
            return;
        }
        if let Some(id) = throttle.notice.take() {
            let whose = conversations_that_lost_something(
                &throttle.from,
                !throttle.a_loss_it_could_not_name,
            );
            if let Err(e) = self
                .surface
                .rewrite(&id, &throttle_cleared_line(throttle.lost, &whose))
                .await
            {
                tracing::warn!(error = %e, "the line about the chat being full could not be closed off");
            }
        }
        *throttle = Throttle::default();
    }

    /// Handle one bridge, from `hello` to the connection closing.
    ///
    /// The order here is the design, so it is worth reading as one sequence: identify the peer,
    /// admit or refuse, admit BEFORE creating anything, prove the far end is really there, and only
    /// then create the topic that makes the project visible.
    pub async fn serve_connection(self: Arc<Self>, accepted: Accepted) -> anyhow::Result<()> {
        // The transport's own answer, not the bridge's. The pid a bridge puts in its `hello` is a
        // number it chose; this one is a fact about the process on the other end of THIS
        // connection. Gate 4's liveness check runs on it, so a bridge cannot make itself look dead
        // — or make an incumbent look dead — by reporting a pid that is not its own.
        let Accepted { stream, who } = accepted;
        // `split` rather than the transport's own halves: a stream the hub was handed is one value
        // whatever it is underneath, and the writer half has to move into a task of its own.
        let (rx_half, mut tx_half) = tokio::io::split(stream);
        let mut reader = hub_proto::FrameReader::new(rx_half);

        // A connection that never says hello used to sit here forever, holding a task and a file
        // descriptor. Nothing had authenticated at that point, so anything that can reach the
        // socket could park as many as it liked until the process ran out of descriptors — and the
        // accept loop's first error would then have been fatal.
        let first = match tokio::time::timeout(self.settle, reader.next::<BridgeFrame>()).await {
            Err(_) => {
                tracing::debug!("a connection was opened and never said anything");
                return Ok(());
            }
            Ok(Err(hub_proto::ProtoError::Oversize { max })) => {
                // Told, not just closed. A bridge that knows its frame was too big can split it;
                // a bridge handed a closed socket can only guess.
                let env = Envelope::new(
                    FrameId::new("h-refused"),
                    HubFrame::Refused {
                        reason: RefusedReason::FrameTooLarge,
                    },
                );
                let _ = hub_proto::write_frame(&mut tx_half, &env).await;
                tracing::warn!(max, "a bridge's first frame was over the ceiling");
                return Ok(());
            }
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "a connection ended before it said hello");
                return Ok(());
            }
            Ok(Ok(None)) => return Ok(()),
            Ok(Ok(Some(f))) => f,
        };

        let addr = match self.admit(&who, &first.payload, first.v).await {
            // No reply at all. A refusal would confirm that something is listening here.
            Admission::ClosedSilently => return Ok(()),
            Admission::Refused(reason) => {
                let env = Envelope::new(FrameId::new("h-refused"), HubFrame::Refused { reason });
                let _ = hub_proto::write_frame(&mut tx_half, &env).await;
                return Ok(());
            }
            Admission::Admitted(addr) => addr,
        };

        let BridgeFrame::Hello {
            instance,
            pid: claimed_pid,
            confirms,
            controls,
            ..
        } = first.payload.clone()
        else {
            unreachable!("admit only admits a hello");
        };
        // The generation is on the ENVELOPE, in both directions, and there is no payload field
        // carrying it — `flatten` puts a payload field of that name under the same key, so a peer
        // that set both would emit a duplicate key serde refuses to read, and one that set only
        // the payload's would have it swallowed. See `Envelope::generation`.
        let arriving = first.generation;
        // Whether this connection may ever be told `stale_generation`. A run that has never been
        // welcomed holds no number to stamp on its first `hello`, so this starts false for a brand
        // new bridge and is set by the read loop the first time a frame carries one.
        let speaks_generations = Arc::new(AtomicBool::new(arriving.is_some()));
        // Read through the crate's own helper and never as "was the field there": an adapter that
        // builds the list by filtering sends an empty one when it promises nothing.
        let confirms_choices = hub_proto::promises_to_confirm(&confirms, "choice");
        // Filtered HERE, once, and what comes out is both what the claim holds and what the
        // welcome echoes — so the echo is what was admitted rather than a second reading of what
        // was sent, and the two cannot come to disagree about what this hub will ask for.
        let controls = admit_controls(&controls, &addr);
        // Only when the bridge named one. A hello that says nothing about the machine it is on is
        // not a bridge disagreeing with the kernel about its pid, and writing the disagreement
        // down for one would put a line in the journal saying a number was offered when none was.
        if let Some(claimed) = claimed_pid
            && claimed != who.fence()
        {
            // Not fatal — a bridge behind a wrapper legitimately does not know its own outermost
            // pid. It IS worth a line, because the audit trail should record which number was
            // believed and which was merely offered.
            tracing::debug!(
                claimed,
                actual = who.fence(),
                "a bridge reported a pid that is not the one on its connection; using the \
                 connection's"
            );
        }
        let pid = who.fence();

        // The name the REGISTRY holds, composed for the conversation this is — so a lane's own log
        // says the same thing as the topic the operator is looking at — and beside it the project
        // this conversation belongs to.
        //
        // Both read under one lock, because they are one answer about one row: a title and a
        // relation taken from two separate reads could straddle a re-enrolment at the terminal and
        // tell the bridge a name from before it and a project from after.
        let (title, seed) = {
            let registry = self.registry.lock().await;
            let title = registry
                .get(&addr.project)
                .map(|p| self.display_title(p, addr.lane.as_ref()))
                .unwrap_or_default();
            // A seed is its own; a room's is the row it was granted in. `Registry::seed_of` is the
            // only place that relation is worked out, so this and `projects --json` can never come
            // to disagree about whose room this is.
            (title, registry.seed_of(&addr.project).clone())
        };

        // The writer is a task of its own so that a slow Telegram send can never block reading the
        // socket. A bridge that cannot be read is a bridge whose `bye` is missed.
        let (tx, mut outbox) = mpsc::channel::<Envelope<HubFrame>>(64);
        let mut writer = tokio::spawn(async move {
            while let Some(frame) = outbox.recv().await {
                if hub_proto::write_frame(&mut tx_half, &frame).await.is_err() {
                    break;
                }
            }
        });

        let (generation, highest_was, mut kicked) = match self
            .claim_the_address(
                addr.clone(),
                pid,
                instance.clone(),
                tx.clone(),
                Arriving {
                    generation: arriving,
                    confirms_choices,
                    controls: controls.clone(),
                    speaks_generations: Arc::clone(&speaks_generations),
                },
            )
            .await
        {
            Ok(both) => both,
            Err(reason) => {
                let env = Envelope::new(FrameId::new("h-refused"), HubFrame::Refused { reason });
                let _ = tx.send(env).await;
                // Give the writer a moment to put the refusal on the wire before the task is
                // dropped; a refusal nobody receives is the same as the silent takeover this
                // replaced.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                writer.abort();
                // The sentence has to match the refusal. A run turned away because its generation
                // is over is turned away with NOTHING holding the address — that is the whole
                // point of the fence — so writing every refusal down as a rival sent whoever
                // reads an incident out of this file looking for a second bridge that does not
                // exist.
                let _ = self.audit.refused(
                    &addr,
                    match reason {
                        RefusedReason::StaleGeneration => {
                            "a run from an earlier generation tried to come back after a later \
                             one had been admitted"
                        }
                        _ => "another bridge already holds this conversation",
                    },
                );
                return Ok(());
            }
        };

        // The conversation's two directories, made at ADMISSION rather than at the topic: a `say`
        // carrying a file may legally be queued before the pong, and the adapter needs somewhere to
        // copy it to. Two empty directories for a bridge that boots and exits are nothing, where an
        // empty topic is a scar. An outbox that cannot be made, or is not the hub's own, is not
        // named — absence is "this hub carries no files" on the wire — so the adapter says so in
        // its tool result rather than sending files the hub would refuse one by one.
        //
        // And named only to a peer that can OPEN it. A directory path is worth nothing to a bridge
        // that does not share this filesystem, and offering one would have it copy a file into a
        // place the hub will never look and report the send as done. Absence already means "this
        // hub carries no files", so a peer that cannot reach the tree is simply told that.
        let outbox = if !who.shares_this_filesystem() {
            tracing::info!(
                project = %addr.project, lane = addr.lane_field(),
                "this bridge cannot reach the files on this machine, so it was offered no \
                 place to put them"
            );
            None
        } else {
            // Asked for only now, and not before the identity has been consulted: `dir_for` CREATES
            // the tree, so asking first left two directories on disk for a peer that was in the
            // same breath told it had been offered none.
            match self.outbox.dir_for(&addr) {
                Ok(dir) => Some(dir.to_string_lossy().into_owned()),
                Err(e) => {
                    tracing::error!(
                        project = %addr.project, lane = addr.lane_field(), error = %e,
                        "the conversation's outbox is not one this hub will read from, so it was \
                         not named to the adapter and no file of the agent's will be sent; fix the \
                         directory and restart the session"
                    );
                    None
                }
            }
        };
        if let Err(e) = self.media.dir_for(&addr) {
            tracing::error!(
                project = %addr.project, lane = addr.lane_field(), error = %e,
                "the conversation's media directory is not one this hub will write into; a file \
                 he sends will not be fetched until it is fixed"
            );
        }
        // Admitted, with no topic yet. See `HubFrame::Welcome` for why that is not an omission.
        //
        // The lease is on this envelope and on every envelope this hub sends the connection after
        // it. There is no payload field carrying it and there cannot be: `flatten` puts one under
        // the same key as this, so a hub that set both would emit a word serde refuses to read.
        let _ = tx
            .send(
                Envelope::new(
                    FrameId::new(format!("h{}", next_frame_seq())),
                    HubFrame::Welcome {
                        project: title,
                        // The two ids, and both from the row the SECRET resolved to — never from
                        // the `project_id` on the hello, which is a name a bridge chose for
                        // itself and which the hub has ignored since the day it was added.
                        //
                        // They are here so that nothing downstream has to relate this connection
                        // to a row of `projects --json` by the one string both ends could
                        // otherwise see, the repo path: a path moves when a folder moves, differs
                        // inside a wall, and is the operator's own filesystem going somewhere it
                        // need not go.
                        project_id: Some(seed),
                        conversation: Some(addr.project.clone()),
                        // Echoed from the ADMITTED address, never from the wire. It is the only thing
                        // that tells a bridge which named a lane that this hub understood it, rather
                        // than ignoring the word and handing the worktree the project's own place.
                        lane: addr.lane.clone(),
                        topic_id: None,
                        limits: LIMITS,
                        outbox,
                        // Echoed from what was ADMITTED, never from the wire — the `lane` echo's
                        // argument, on a field whose absence a controller has to be able to read
                        // as "this hub is older than I am" and act on.
                        controls,
                    },
                )
                .with_generation(generation),
            )
            .await;

        // Prove the far end is really there before anything is created for it. The ping's own
        // envelope id is the nonce; a pong naming it is the proof.
        let ping_id = FrameId::new(format!("h{}", next_frame_seq()));
        let _ = tx
            .send(Envelope::new(ping_id.clone(), HubFrame::Ping).with_generation(generation))
            .await;

        // Anything the bridge says before its pong is KEPT, not dropped on the floor.
        //
        // A bridge that opens with a question — which is the whole point of the product — used to
        // have it read, discarded, and never acked. No message, no record, no reply, and the agent
        // sitting blocked on an answer that could never come. Buffered here and replayed the
        // moment the project is live.
        //
        // BOUNDED, by count and by bytes, at exactly what a conforming bridge may be holding when
        // it dials (`PRE_PONG_FRAMES`, `PRE_PONG_BYTES`). An unbounded Vec here let one connection
        // hand the hub as much as it could write in the settling window — measured at 18 MB in
        // 450 ms — before it had proved it was even there. A bound of 256 KiB, sixteen times under
        // what the bridge may legally hold, refused every bridge that carried an ordinary backlog
        // into a reconnect. The count matches the outbox's own 64.
        let (hold_frames, hold_bytes) = self.pre_pong;
        let mut waiting: Vec<Envelope<BridgeFrame>> = Vec::new();
        let mut waiting_bytes = 0usize;
        let settled = tokio::time::timeout(self.settle, async {
            loop {
                // The kick is listened for HERE too, not only once the connection is live. A project
                // switched off inside the settling window used to go on to get a topic, a greeting
                // and a minute of delivery before anything noticed.
                let next = tokio::select! {
                    next = reader.next::<BridgeFrame>() => next,
                    kick = kicked.recv() => return match kick {
                        Some(kick) => Settled::Kicked(kick),
                        // The claim was taken from under this connection: its process is gone
                        // from /proc and a successor evicted it. Nobody is behind the socket to
                        // tell, and "switched off" would be untrue — it went the way a dead
                        // bridge goes.
                        None => Settled::Gone,
                    },
                };
                match next {
                    Ok(Some(frame)) => {
                        // The first frame carrying a generation is the proof that this bridge knows
                        // the word — its `hello` could not carry one if it had never been welcomed.
                        if frame.generation.is_some() {
                            speaks_generations.store(true, Ordering::Release);
                        }
                        if let BridgeFrame::Pong { r#ref } = &frame.payload
                            && r#ref == &ping_id
                        {
                            return Settled::Live;
                        }
                        // A goodbye is the end of what it has to say, not a frame to hold for a
                        // pong that is not coming. `kickoff-hub-attach --check` connects, reads
                        // the welcome and says `bye` on purpose, so that proving an environment can
                        // reach the hub makes no topic; read as silence, every such check left
                        // "never answered; probably not allowed to talk to me" in the audit, and a
                        // wrapper that checks before trusting a wall looked like an intruder each
                        // time. Kept with the others: it was read and has an id, so it is owed its
                        // ack like the rest.
                        if let BridgeFrame::Bye { .. } = &frame.payload {
                            waiting.push(frame);
                            return Settled::SaidGoodbye;
                        }
                        waiting_bytes += frame_cost(&frame.payload);
                        // The frame that trips the bound is kept WITH the others, not dropped on
                        // the way out: it was read and it has an id, so it is owed an answer like
                        // the rest of them.
                        let over = waiting.len() >= hold_frames || waiting_bytes > hold_bytes;
                        waiting.push(frame);
                        if over {
                            return Settled::Overflowed;
                        }
                    }
                    // A line this build cannot DECODE is one bad frame, not a dead peer — and it is
                    // exactly what a bridge one version ahead sends. The post-pong loop survives it;
                    // this one used to end the connection and then audit it as "never answered",
                    // which blames the bridge for the hub's own strictness.
                    Err(hub_proto::ProtoError::Decode { source, len }) => {
                        tracing::warn!(len, error = %source, "a frame this build cannot read, before the pong; ignoring it");
                        continue;
                    }
                    // Over the ceiling before the pong. This used to fall into "never answered",
                    // which is not what happened, and the bridge got a closed socket instead of the
                    // `frame_too_large` the document promises it.
                    Err(hub_proto::ProtoError::Oversize { .. }) => return Settled::Oversize,
                    _ => return Settled::Gone,
                }
            }
        })
        .await
        .unwrap_or(Settled::Gone);

        if settled != Settled::Live {
            // Three different failures, said differently. A bridge that filled the buffer is talking
            // too much before it has proved it is there; one that said nothing is probably a channel
            // plugin that is not allowlisted, which boots and exits in about a tenth of a second.
            // Reporting the first as the second sends the operator looking in the wrong place.
            //
            // A goodbye is not a failure at all, so it is not written to the audit as one: that file
            // is where a person looks for bridges that were not allowed in, and a check that ran
            // cleanly does not belong among them. It is said at info in the journal, and nothing
            // else about the close changes — released, every queued frame answered `no`, no topic.
            let why = match settled {
                Settled::Overflowed => {
                    Some("sent more before answering than the hub will hold for it")
                }
                Settled::Oversize => Some("a frame was over the size ceiling"),
                Settled::Kicked(kick) => Some(kick.sentence),
                Settled::Gone | Settled::Live => {
                    Some("connected but never answered; it is probably not allowed to talk to me")
                }
                Settled::SaidGoodbye => None,
            };
            match why {
                Some(why) => {
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(), why,
                        "a bridge did not become live"
                    );
                    let _ = self.audit.refused(&addr, why);
                }
                None => {
                    // With the reason it gave, so a reader can tell `--check` ("just checking")
                    // from an adapter that dialled and bailed. It is the last frame read.
                    let reason = waiting.iter().find_map(|f| match &f.payload {
                        BridgeFrame::Bye { reason } => Some(reason.as_str()),
                        _ => None,
                    });
                    tracing::info!(
                        project = %addr.project, lane = addr.lane_field(), reason,
                        "a bridge said goodbye before it became live"
                    );
                }
            }
            // Released FIRST, as the oversize path below does: what follows is writing, and a
            // bridge that redials in a second must not find its own dead connection still holding
            // the address.
            self.release_this_run(&addr, pid, generation).await;
            // And the number goes back with the address. A connection that never became live
            // never held the conversation, and leaving the floor where its claim put it refuses a
            // run that was only redialling — for ever, with the one word its own table treats as
            // permanent. `--check` is the everyday shape of it: it is run precisely when a wall
            // looks broken, which is when a session is most likely to be between sockets.
            self.generations
                .lock()
                .expect("the generations are not held across an await")
                .give_back(&addr, generation, highest_was);
            // And the file follows the map, here as at the mint. A floor put back in memory and
            // left high on disk is the same refusal-for-ever after the next restart that giving it
            // back is here to prevent.
            self.write_down_the_run_numbers().await;
            // Every frame it said is answered for BEFORE the socket goes. Each was read and has an
            // id, and each is about to be destroyed; the wire's rule is one ack per frame, and a
            // bridge keys "which of mine reached him" by exactly these ids. `writer.abort()` used
            // to come straight after the release, and the whole buffer went down with the socket:
            // sixty-four messages, measured, the agent told of none of them, the bridge reconnecting
            // to do it again. `too-fast` for the overflow, because that is what it was; no reason
            // for the rest, because the closed set has none for "the connection never became live"
            // and a wrong one sends the agent the wrong way.
            let ack_why = match settled {
                Settled::Overflowed => Some(AckWhy::TooFast),
                // Whatever ended it says what its frames are answered with — `stale-generation`
                // for a run a later one replaced, and only where that run can read the word.
                Settled::Kicked(kick) => kick.why,
                _ => None,
            };
            let goodbye = async {
                for frame in std::mem::take(&mut waiting) {
                    let env = Envelope::new(
                        FrameId::new(format!("h{}", next_frame_seq())),
                        HubFrame::Ack {
                            r#ref: frame.id,
                            delivered: Delivered::No,
                            why: ack_why,
                        },
                    )
                    .with_generation(generation);
                    if tx.send(env).await.is_err() {
                        break;
                    }
                }
                // The two refusals with a reason the bridge can branch on are told it. The
                // closed set has none for "never answered", so that one stays a close.
                let reason = match settled {
                    Settled::Oversize => Some(RefusedReason::FrameTooLarge),
                    Settled::Kicked(kick) => kick.reason,
                    _ => None,
                };
                if let Some(reason) = reason {
                    let _ = tx
                        .send(
                            Envelope::new(
                                FrameId::new(format!("h{}", next_frame_seq())),
                                HubFrame::Refused { reason },
                            )
                            .with_generation(generation),
                        )
                        .await;
                }
            };
            // Bounded, twice: a peer that has stopped reading must not hold this task open for
            // ever, and the writer is ended rather than left to find that out on its own.
            let _ = tokio::time::timeout(GOODBYE_SHELF_LIFE, goodbye).await;
            drop(tx);
            let _ = tokio::time::timeout(GOODBYE_SHELF_LIFE, &mut writer).await;
            writer.abort();
            return Ok(());
        }

        // Live. NOW the topic exists, and the greeting is what makes it visible in the list.
        // Two levels, because they are two different things. A busy minute is not a fault and the
        // next message this bridge sends will open the topic; anything else is a conversation that
        // cannot be seen and is not going to mend itself.
        // A short shelf life, and the reason is right above `GREETING_SHELF_LIFE`: this runs before
        // the read loop, so waiting here is this bridge's first frame going unread — and the next
        // message it sends opens the topic anyway.
        match self
            .topic_for(&addr, std::time::Instant::now() + GREETING_SHELF_LIFE)
            .await
        {
            Ok(_) => {}
            Err(NoTopic::TooFast(wait)) => tracing::warn!(
                project = %addr.project, lane = addr.lane_field(), wait = ?wait,
                "the chat's budget could not open a topic for a live project yet"
            ),
            Err(e) => tracing::error!(
                project = %addr.project, lane = addr.lane_field(), error = %e,
                "could not make a topic for a live project"
            ),
        }

        // Whatever the last run of this project left open comes off the phone now — in a task of
        // its own, so this session's own first question is never queued behind the cleanup of one
        // that is already gone. Nothing else reads those records, so it does not matter whether it
        // finishes before or after anything below.
        {
            let hub = Arc::clone(&self);
            let addr = addr.clone();
            let instance = instance.clone();
            tokio::spawn(async move {
                hub.retire_what_other_sessions_left(&addr, &instance).await;
            });
        }

        // FRAMES ARE HANDLED IN A TASK OF THEIR OWN, in the order they arrived.
        //
        // They used to be handled inline, which made "how long is this message worth holding" and
        // "is this bridge still there" the same question. They are not the same question, and the
        // shelf lives above made the difference an order of magnitude: a frame the chat cannot take
        // yet is worth ninety seconds, and for all ninety of them the socket went unread — so the
        // EOF that says the bridge is gone was unread too, and the claim it holds is what refuses
        // the session when it comes back. `hub-link.ts` redials in the same process after a second,
        // so what the operator saw was a project that went quiet for no visible reason.
        //
        // The channel is bounded at the same 64 as the outbox and the pre-pong buffer: a backlog
        // deeper than a bridge is allowed to hold still blocks the read loop, and that is correct —
        // it is the only backpressure there is.
        let (frames_tx, mut frames_rx) = mpsc::channel::<Envelope<BridgeFrame>>(64);
        // Thrown when the project is switched off under this connection. What the handler still
        // holds is then answered `no` without a send: draining sixty-four queued frames into the
        // topic at one a second would be a minute of the flood the switch was thrown to stop. The
        // frame already inside `handle` sees the switch too, at its pacer wait — see
        // `ConnectionSwitch` for why that one matters most.
        let switch = Arc::new(ConnectionSwitch::default());
        let mut handler = {
            let hub = Arc::clone(&self);
            let addr = addr.clone();
            let instance = instance.clone();
            let tx = tx.clone();
            let switch = Arc::clone(&switch);
            let speaks = Arc::clone(&speaks_generations);
            // Once per connection, not once per frame. A run that has been replaced usually has a
            // backlog, and a journal that says the same sentence sixty-four times is one nobody
            // reads to the end of.
            let said_once = AtomicBool::new(false);
            tokio::spawn(async move {
                while let Some(frame) = frames_rx.recv().await {
                    let ack_ref = frame.id.clone();
                    // THE DELIVERY FENCE, and it is here rather than inside `handle` on purpose:
                    // this loop is the one place every frame of a connection passes through, and
                    // it goes on running AFTER the claim has been released — draining what was
                    // queued behind a send that was in flight. That drain is where a run which is
                    // already over finishes its backlog into a conversation a later run now holds,
                    // and the claims map cannot see it because by then the map is empty. So the
                    // question asked is "has a later generation taken this address", which stays
                    // answerable when nothing is connected at all.
                    //
                    // The second half is a frame stamped with a number this hub never granted this
                    // connection and could only have granted a LATER one — a bridge that muddled
                    // two connections, or a relay forwarding a producer that has moved on. Its own
                    // stamp says it is ahead of us, so it is not ours.
                    //
                    // Greater than, never "different from". A bridge that lost its socket redials
                    // holding the number it had, is admitted, and flushes the backlog it kept —
                    // and it wrote those bytes before it could possibly have read the new welcome,
                    // so every one of them carries the OLD number and none can be re-stamped. On
                    // "different from" a live, conforming run had its whole backlog refused with
                    // the one word its own table treats as permanent: the redial loop, with
                    // nothing on his phone. An older number is left to `a_newer_run_holds`, which
                    // is the authoritative "this run is over" question and already covers it.
                    let superseded = hub
                        .a_newer_run_holds(&addr, generation)
                        .map(|took| (generation, took))
                        .or_else(|| {
                            frame
                                .generation
                                .filter(|stamped| *stamped > generation)
                                .map(|stamped| (stamped, generation))
                        });
                    let (delivered, why) = if let Some((from, took)) = superseded {
                        if !said_once.swap(true, Ordering::Relaxed) {
                            tracing::warn!(
                                project = %addr.project, lane = addr.lane_field(),
                                generation = from, latest = took,
                                "a run kept talking after a later generation took the address; \
                                 refusing what it says"
                            );
                        }
                        // The word only where it can be read. A bridge that stamped no generation
                        // renders an unknown `why` as "his phone did not take it", which would tell
                        // an agent that the operator's messaging app refused a frame his phone
                        // never saw.
                        (
                            Delivered::No,
                            speaks
                                .load(Ordering::Acquire)
                                .then_some(AckWhy::StaleGeneration),
                        )
                    } else if switch.is_off() {
                        (Delivered::No, None)
                    } else {
                        SWITCH
                            .scope(
                                Arc::clone(&switch),
                                hub.handle(&addr, &instance, frame.payload),
                            )
                            .await
                    };
                    let _ = tx
                        .send(
                            Envelope::new(
                                FrameId::new(format!("h{}", next_frame_seq())),
                                HubFrame::Ack {
                                    r#ref: ack_ref,
                                    delivered,
                                    why,
                                },
                            )
                            .with_generation(generation),
                        )
                        .await;
                }
            })
        };

        // The kick is listened for while these are queued, as it is below: a reconnect carrying
        // sixty-five frames into a full queue parks here exactly as the live loop does.
        let mut waiting: VecDeque<Envelope<BridgeFrame>> = waiting.into();
        while !waiting.is_empty() {
            let kicked_with = tokio::select! {
                slot = frames_tx.reserve() => match slot {
                    Ok(slot) => {
                        slot.send(waiting.pop_front().expect("checked non-empty"));
                        None
                    }
                    Err(_) => break,
                },
                kick = kicked.recv() => match kick {
                    None => break,
                    Some(kick) => Some(kick),
                },
            };
            if let Some(kick) = kicked_with {
                self.end_from_outside(
                    &addr,
                    pid,
                    generation,
                    kick,
                    waiting.into(),
                    &switch,
                    tx,
                    frames_tx,
                    handler,
                    writer,
                )
                .await;
                return Ok(());
            }
        }

        // The claim is released on EVERY way out of this loop, not only the tidy one.
        //
        // It used to be released after a `?`, which meant a read error skipped it entirely — and a
        // read error is the ORDINARY way a bridge goes away. A peer that closes with bytes still
        // unread in its receive buffer makes the kernel send an RST, and the hub's next read fails
        // with a connection reset rather than a clean end-of-file. A bridge that has not been
        // reading its acks does exactly that. The cost of getting this wrong is a project holding a
        // claim nobody is behind: its worker restarts, is refused as already-claimed, and the
        // operator is left with a project that has gone quiet for no visible reason.
        loop {
            // Two things can end this loop from outside the bridge's own frames: the socket, and a
            // kick. The kick is the operator switching the project off at the terminal, and it is
            // the only way a connection is ever ended by this side.
            let next = tokio::select! {
                next = reader.next::<BridgeFrame>() => next,
                kick = kicked.recv() => {
                    // `None` is the claim taken from under this connection by a successor that
                    // found its process dead. Nothing is behind the socket, and the successor
                    // holds the address now, so this ends the way EOF does: the release below is
                    // pid-guarded and leaves the successor's claim alone.
                    let Some(kick) = kick else { break };
                    self.end_from_outside(
                        &addr,
                        pid,
                        generation,
                        kick,
                        Vec::new(),
                        &switch,
                        tx,
                        frames_tx,
                        handler,
                        writer,
                    )
                    .await;
                    return Ok(());
                }
            };
            match next {
                Ok(None) => break,
                // A line that will not DECODE is one bad frame, not a dead peer — and this is
                // exactly what a bridge one version ahead sends. Tearing the connection down for it
                // makes an additive change on the other side a project that goes silent. The
                // transport failures do end it, because after those there is nothing to read.
                Err(hub_proto::ProtoError::Decode { source, len }) => {
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(), len, error = %source,
                        "a frame this build cannot read; ignoring it"
                    );
                    continue;
                }
                Err(hub_proto::ProtoError::Oversize { max }) => {
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(), max,
                        "a frame over the ceiling; refusing it"
                    );
                    let _ = tx
                        .send(
                            Envelope::new(
                                FrameId::new(format!("h{}", next_frame_seq())),
                                HubFrame::Refused {
                                    reason: RefusedReason::FrameTooLarge,
                                },
                            )
                            .with_generation(generation),
                        )
                        .await;
                    let _ = self
                        .audit
                        .refused(&addr, "a frame was over the size ceiling");
                    // Queued is not sent. `break` used to fall straight into `writer.abort()`,
                    // which destroyed this refusal before the writer task could put it on the wire
                    // — so the bridge got the closed socket the refusal existed to replace. A
                    // bridge told its frame was too big can split it; one handed a dead socket can
                    // only guess. Dropping `tx` ends the writer's loop; the timeout is there
                    // because a peer that has stopped reading must not hold this open forever.
                    // Released FIRST. Waiting on the writer while still holding the claim meant a
                    // bridge doing the documented thing — split the frame, reconnect — was refused
                    // `already_claimed` for two seconds by the connection it had just been told to
                    // abandon. And releasing drops the claim's own `Sender`, so the writer's channel
                    // really closes and it ends at once rather than at the timeout.
                    self.release_this_run(&addr, pid, generation).await;
                    drop(frames_tx);
                    let _ = tokio::time::timeout(PROSE_SHELF_LIFE, &mut handler).await;
                    handler.abort();
                    drop(tx);
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), writer).await;
                    return Ok(());
                }
                Err(e) => {
                    tracing::debug!(
                        project = %addr.project, lane = addr.lane_field(), error = %e,
                        "the bridge's connection ended"
                    );
                    break;
                }
                Ok(Some(frame)) => {
                    if frame.generation.is_some() {
                        speaks_generations.store(true, Ordering::Release);
                    }
                    // The kick is listened for HERE too, not only between frames. With the queue
                    // full — a loud bridge whose minute is spent, which is the bridge the switch
                    // exists for — this send parks the loop for as long as the pacer holds the
                    // frame at the head of the queue, and the switch waited behind it: measured
                    // at fifty-seven seconds, claim held, his words still handed to it, one more
                    // line posted. The frame in hand is answered `no` with the rest.
                    let kicked_with = tokio::select! {
                        slot = frames_tx.reserve() => match slot {
                            Ok(slot) => {
                                slot.send(frame);
                                None
                            }
                            Err(_) => break,
                        },
                        kick = kicked.recv() => match kick {
                            None => break,
                            Some(kick) => Some((kick, frame)),
                        },
                    };
                    if let Some((kick, frame)) = kicked_with {
                        self.end_from_outside(
                            &addr,
                            pid,
                            generation,
                            kick,
                            vec![frame],
                            &switch,
                            tx,
                            frames_tx,
                            handler,
                            writer,
                        )
                        .await;
                        return Ok(());
                    }
                }
            }
        }

        // Released the instant the socket ends, and BEFORE waiting on anything in flight. That
        // ordering is the whole of the fix above: liveness is a fact about the socket, not about
        // how long the last thing said is worth holding.
        self.release_this_run(&addr, pid, generation).await;
        drop(frames_tx);
        // Bounded, and not aborted outright. What is usually in flight when a bridge goes away is
        // the session's own last `done`, and dropping that mid-send loses a message for nothing —
        // but a backlog belonging to a session that has been gone for a minute and a half is not
        // worth another project's turn.
        if tokio::time::timeout(PROSE_SHELF_LIFE, &mut handler)
            .await
            .is_err()
        {
            handler.abort();
        }
        writer.abort();
        Ok(())
    }

    /// End a live connection from outside its own read loop: its project switched off at the
    /// terminal, or a later run of the address taking it over.
    ///
    /// Everything read from the bridge is answered for before the socket ends, in this order: the
    /// refusal first, so the bridge reads every `no` after it in that light and stops promising its
    /// agent anything; then `no` for each frame in hand — read off the socket and not yet queued;
    /// then, from the handler, `no` for everything queued behind the frame it was inside, and for
    /// that frame too if it was still waiting for its turn (see `ConnectionSwitch`). A frame
    /// already inside a send is finished, because cancelling a Telegram call mid-flight lands a
    /// message whose ack says it did not land. Then the close, bounded like every other goodbye.
    ///
    /// Both the refusal and the sentence come from the [`Kick`], because the two callers differ in
    /// exactly those: a project switched off is told `not_enabled` and every run of it is told the
    /// same, while a run a later generation replaced is told `stale_generation` — and told nothing
    /// at all if it is old enough to read an unknown refusal as one worth redialling on.
    #[allow(clippy::too_many_arguments)]
    async fn end_from_outside(
        &self,
        addr: &Addr,
        pid: u32,
        generation: u64,
        kick: Kick,
        in_hand: Vec<Envelope<BridgeFrame>>,
        switch: &ConnectionSwitch,
        tx: mpsc::Sender<Envelope<HubFrame>>,
        frames_tx: mpsc::Sender<Envelope<BridgeFrame>>,
        mut handler: tokio::task::JoinHandle<()>,
        mut writer: tokio::task::JoinHandle<()>,
    ) {
        tracing::info!(
            project = %addr.project, lane = addr.lane_field(), generation,
            why = kick.sentence,
            "ending a live connection"
        );
        // Released FIRST, so `/projects` stops calling it connected and his typed words stop
        // reaching it the instant the switch is thrown, before any goodbye. Guarded on this
        // connection's own generation, so a run that has ALREADY been evicted cannot take its
        // successor's claim away on the way out.
        self.release_this_run(addr, pid, generation).await;
        // Told why BEFORE the switch is thrown inside this connection, so the refusal is on the
        // wire ahead of every `no` the switch causes. Bounded: a bridge that has stopped reading
        // has a full outbox, and the handler is already parked on it — nothing more can post.
        if let Some(reason) = kick.reason {
            let _ = tokio::time::timeout(
                GOODBYE_SHELF_LIFE,
                tx.send(
                    Envelope::new(
                        FrameId::new(format!("h{}", next_frame_seq())),
                        HubFrame::Refused { reason },
                    )
                    .with_generation(generation),
                ),
            )
            .await;
        }
        switch.throw();
        let _ = self.audit.refused(addr, kick.sentence);
        for frame in in_hand {
            let env = Envelope::new(
                FrameId::new(format!("h{}", next_frame_seq())),
                HubFrame::Ack {
                    r#ref: frame.id,
                    delivered: Delivered::No,
                    why: kick.why,
                },
            )
            .with_generation(generation);
            if tokio::time::timeout(GOODBYE_SHELF_LIFE, tx.send(env))
                .await
                .is_err()
            {
                break;
            }
        }
        drop(frames_tx);
        if tokio::time::timeout(PROSE_SHELF_LIFE, &mut handler)
            .await
            .is_err()
        {
            handler.abort();
        }
        drop(tx);
        let _ = tokio::time::timeout(GOODBYE_SHELF_LIFE, &mut writer).await;
        writer.abort();
    }

    /// One frame from a bridge. Returns what to put in its ack.
    ///
    /// **Every frame gets exactly one ack.** A rejected send used to be a single error log and a
    /// drop, which already lost 5,164 characters of a real agent's longest message. Backpressure
    /// now reaches the only party that can do anything about it.
    async fn handle(
        &self,
        addr: &Addr,
        instance: &str,
        frame: BridgeFrame,
    ) -> (Delivered, Option<hub_proto::AckWhy>) {
        match frame {
            BridgeFrame::Say {
                text,
                file: Some(file),
                ..
            }
            | BridgeFrame::Done {
                text,
                file: Some(file),
            } => self.say_with_file_and_ack(addr, &text, &file).await,
            BridgeFrame::Say { text, .. } | BridgeFrame::Done { text, .. } => {
                self.say_and_ack(addr, &text, &[]).await
            }
            BridgeFrame::Ask {
                ask_id,
                text,
                options,
            } => {
                let options = options.unwrap_or_default();

                // A one line summary above the question, when one is configured. The operator reads
                // this on a phone and a long question is a wall of text he has to open before he
                // can decide; the gist is what makes the notification itself useful.
                //
                // It NEVER replaces the question. A summary standing in for what was actually said
                // is a defect this repo has already shipped once, and the eight voice rules that
                // came out of it start with that one.
                let text = match &self.gist {
                    None => text,
                    Some(g) => {
                        let summarised = g.one_line(&text).await;
                        // The trip-off is ONE WAY and it has to be said out loud, once. Until this
                        // existed the only place it was ever said was the journal, which is not on
                        // the phone he is reading — so a refusal that protected him and a gateway
                        // that had merely gone quiet looked identical from the outside: summaries
                        // simply stopped, with nothing saying why or that they were not coming back.
                        if g.newly_off() {
                            let _ = self
                                .say(addr, "I have stopped summarising. Questions still reach you in full.", &[])
                                .await;
                        }
                        match summarised {
                            None => text,
                            Some(line) => format!("{line}\n\n{text}"),
                        }
                    }
                };

                // Telegram gives a button 64 bytes of `callback_data` and no more. The option id is
                // minted by the BRIDGE, so it is agent-authored and arbitrary: too long and the API
                // refuses the whole message, leaving an agent blocked on a question that was never
                // asked. A `|` is worse than that — it survives the send and then splits wrong on
                // the way back, so the tap resolves to nothing while looking perfectly fine.
                //
                // Refused here, where the bridge can still be told and can ask again differently.
                if let Some(bad) = options.iter().find(|o| {
                    let data = o.option_id.as_str();
                    data.contains('|') || data.len() + 2 > CALLBACK_DATA_MAX
                }) {
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(), option = %bad.option_id,
                        "a question's answer ids will not fit in a Telegram button; refusing it"
                    );
                    let _ = self
                        .audit
                        .refused(addr, "an answer id was too long or contained a separator");
                    // One plain line where the operator can see it: an agent blocked on a question
                    // that never arrived is the failure this product exists to prevent, and it must
                    // not be visible only in a log.
                    //
                    // Subject the TOPIC, not the project — the same rule as `TapRefusal::say`. This
                    // lands in a worktree's own topic as often as a project's, where "this project"
                    // is a thing he can read as false with the project's topic busy beside it, and
                    // "the terminal" is one of twelve he could go to and find nothing waiting.
                    let _ = self
                        .say(
                            addr,
                            "Something here asked me a question I could not put on a button, so it \
                             is still waiting. Answer it where it is running.",
                            &[],
                        )
                        .await;
                    return (Delivered::No, Some(hub_proto::AckWhy::TelegramRefused));
                }

                // A QUESTION however the agent minted it. An `ask` with no options is still
                // something he is expected to answer — by typing rather than by tapping — so its
                // shelf life is a question's, and the buttons cannot say so on their own.
                let outcome = self
                    .say_as(addr, &text, &options, Perishable::Question)
                    .await;

                // The record is written for the message that actually exists. Recording before the
                // send would leave a ledger entry for a message nobody can see; recording against a
                // guessed id would let a tap resolve against the wrong question, which is the exact
                // shape of the defect where a button reading "Reject" confirmed "Allow always".
                if let SendOutcome::Sent(msg_id) | SendOutcome::Clamped(msg_id) = &outcome {
                    // Read back for THIS conversation, and refused rather than defaulted. This
                    // ended in `.unwrap_or_default()`, which wrote a zero on a miss: a record
                    // naming no topic at all, whose later retirement edits into the forum's General
                    // or fails outright. A record that cannot be written correctly must not be
                    // written, and `resolve_tap` then answers the tap with "I have no record of
                    // that question" — which is the fail-closed half, and true.
                    let topic_id = { self.registry.lock().await.topic_of(addr) };
                    match topic_id {
                        None => tracing::error!(
                            project = %addr.project, lane = addr.lane_field(),
                            "sent a question and then could not find which topic it went to, so \
                             what its buttons mean was not written down; a tap will be refused"
                        ),
                        Some(topic_id) => {
                            // The pid of the connection that is asking, read from the claim it is
                            // holding right now. Not passed down from `hello`, because the claim is
                            // the thing the eviction rule already trusts and a second copy of the
                            // same number is a second thing that can disagree with it.
                            let pid = { self.claims.lock().await.get(addr).map(|c| c.pid) };
                            let record = AskRecord {
                                project: addr.project.clone(),
                                lane: addr.lane.clone(),
                                ask_id,
                                topic_id,
                                options,
                                instance: instance.to_owned(),
                                pid,
                                at: now_secs(),
                                // The CLIPPED text, because that is what the operator is actually
                                // looking at. Storing the original meant the retirement rebuilt the
                                // message from text longer than the one that was sent — and a
                                // retirement body over Telegram's 4096 is an edit that fails, which
                                // leaves the answered keyboard live and still offering choices that
                                // have already been made.
                                text: crate::queue::fit(&text, crate::queue::MAX_TEXT).0,
                                answered: None,
                                closed: None,
                            };
                            if let Err(e) =
                                self.ledger
                                    .lock()
                                    .await
                                    .record(self.forum_chat, msg_id, record)
                            {
                                // A keyboard whose meaning was not written down must not stay
                                // tappable, so this is loud. `resolve_tap` refuses it, which is the
                                // fail-closed half.
                                tracing::error!(
                                    project = %addr.project, lane = addr.lane_field(), error = %e,
                                    "sent a question but could not write down what its buttons mean"
                                );
                            }
                        }
                    }
                }
                self.ack_for(&outcome)
            }
            BridgeFrame::AskResolved {
                ask_id,
                how,
                outcome,
            } => {
                self.retire(addr, instance, &ask_id, how, outcome.as_deref())
                    .await;
                (Delivered::Yes, None)
            }
            // The bridge saying what became of a frame the hub sent it. The only frames anybody
            // is waiting on an answer for are his typed words and his taps; everything else about
            // it is bookkeeping. Acked like any frame, so "every frame gets exactly one" stays true.
            BridgeFrame::Ack {
                r#ref,
                status,
                reason,
                files,
            } => {
                self.what_became_of_it(addr, &r#ref, status, reason.as_deref(), files)
                    .await;
                (Delivered::Yes, None)
            }
            // Liveness and bookkeeping. Acked so that "every frame gets exactly one" stays true
            // without exception, which is what makes a missing ack mean something.
            // A controller saying what became of something this hub carried for the operator. The
            // hub reads the status and acts on it; an id it did not mint reaches nobody.
            BridgeFrame::IntentOutcome {
                intent_id,
                status,
                reason,
            } => {
                self.what_became_of_an_intention(addr, &intent_id, status, reason)
                    .await
            }
            BridgeFrame::Beat { .. } | BridgeFrame::Pong { .. } => (Delivered::Yes, None),
            BridgeFrame::Bye { .. } => (Delivered::Yes, None),
            BridgeFrame::Hello { .. } => {
                // A second hello on a live connection. Not a takeover and not an error worth
                // closing over; it is simply not a thing this protocol has.
                (Delivered::No, Some(hub_proto::AckWhy::TelegramRefused))
            }
            BridgeFrame::Unknown => (Delivered::Yes, None),
        }
    }

    /// A bridge has said what became of a frame the hub handed it.
    ///
    /// `accepted` is the end of it: the agent's own answer is the acknowledgement, and a line under
    /// everything he types would turn the conversation into a receipt printer. `refused` is said
    /// in the topic he typed in, with the adapter's reason, and says the words will not be
    /// delivered later — the sentence he already gets when nothing is connected there. It is the
    /// only place he can learn that a line he wrote reached nobody: an opencode worker with no
    /// session open used to say so on the wire and he went on looking at a line he believed was
    /// read.
    ///
    /// Only for a frame this hub handed THIS conversation as his words. A refusal naming an id the
    /// bridge made up, or a frame of any other kind, writes nothing — a bridge cannot put text in
    /// his topic under the hub's name by refusing things it was never sent. The record is matched
    /// on the frame id and the address, and NOT on the run: a record outlives the run it was handed
    /// to, so a later run of the same address that names an id handed to an earlier one is answered
    /// as if it were that run. Ids are sequential, so guessing one is not hard — but it takes a
    /// bridge that has already authenticated for this project and is then deliberately naming a
    /// frame it was never sent, which is a lie and not an accident. Left as it is: closing it means
    /// carrying the minting run's generation on every record and through this handler, and the
    /// price of that is paid by every honest frame. And the FIRST answer
    /// for a message is the one that counts: the record goes with it, so a second cannot write a
    /// second line. Behind attach's door several producers may answer one message, and the door
    /// folds them into one before this hub hears it; the Claude tool server answers `accepted`
    /// the moment it has handed the words into the agent's turn.
    async fn what_became_of_it(
        &self,
        addr: &Addr,
        frame: &FrameId,
        status: AckStatus,
        reason: Option<&str>,
        files: Option<u32>,
    ) {
        // A tap is answered for differently from a line he typed, and the answer may have to WAIT:
        // his receipt is a message the bot sends after the answer is already on the wire, so there
        // may be no line to change yet. That branch keeps the record; the words branch below takes
        // it, because nothing about a line he typed is still to be learned.
        //
        // The permit is a TAP's, and is not taken until the record is known to be one. Taken
        // first, it was taken by every ack of every project on the box — and it is held across
        // Telegram round trips — so one refused tap in one topic parked every other connection's
        // handler for the length of two network calls. The peek below cannot go stale: nothing
        // removes a tap record while its receipt is still unknown. See `tap_edits`.
        let it_is_a_tap = {
            let down = self.down.lock().await;
            down.iter()
                .any(|d| &d.frame == frame && &d.addr == addr && matches!(d.what, His::Tap(_)))
        };
        if it_is_a_tap {
            let _in_order = self.tap_edits.lock().await;
            let mut down = self.down.lock().await;
            if let Some(at) = down
                .iter()
                .position(|d| &d.frame == frame && &d.addr == addr)
                && matches!(down[at].what, His::Tap(_))
            {
                let said = match status {
                    AckStatus::Accepted => WhatBecameOfTheTap::Took,
                    AckStatus::Refused => WhatBecameOfTheTap::Refused(plain_reason(reason)),
                };
                let Some(Down {
                    chat_id,
                    msg_id,
                    what: His::Tap(tap),
                    ..
                }) = down.get_mut(at)
                else {
                    return;
                };
                if tap.said.is_some() {
                    // A second answer for one tap. The first is the one he reads, exactly as it is
                    // for a line he typed: there the record goes with the first answer so a second
                    // finds nothing, and here the record has to stay until his receipt exists, so
                    // the rule has to be said out loud instead.
                    return;
                }
                let receipt = tap.receipt.clone();
                if receipt.is_none() && !tap.no_receipt_is_coming {
                    // Nowhere to say it YET. Kept beside the tap, and said the moment `bot.rs`
                    // hands over which message his receipt is. Where the bot has already said no
                    // line is coming, waiting is waiting for ever, so it falls through and is said
                    // the only way left.
                    tap.said = Some(said);
                    return;
                }
                let (chat_id, question, label) = (*chat_id, msg_id.clone(), tap.label.clone());
                down.remove(at);
                drop(down);
                self.say_what_became_of_his_tap(
                    addr,
                    chat_id,
                    &question,
                    receipt.as_ref(),
                    &label,
                    said,
                )
                .await;
                return;
            }
        }
        let his = {
            let mut down = self.down.lock().await;
            let Some(at) = down
                .iter()
                .position(|w| &w.frame == frame && &w.addr == addr)
            else {
                return;
            };
            down.remove(at)
        };
        let Some(his) = his else { return };
        let His::Words { files_on_disk } = his.what else {
            return;
        };
        // The tick, or the cross, in place of the eyes. Marked BEFORE the line for a refusal, so
        // the two arrive in the order he reads them: the glance, then the sentence.
        let mark = match status {
            AckStatus::Accepted => Mark::Accepted,
            AckStatus::Refused => Mark::Refused,
        };
        {
            // Behind the eyes, always: `relay` holds this until they have landed.
            let _in_order = self.mark_permit.lock().await;
            self.mark_his_message(his.chat_id, &his.msg_id, mark).await;
        }
        if status == AckStatus::Accepted {
            // The words were taken. Were the files? An adapter older than files does not know the
            // field, so its ack says nothing about them — and nothing is the honest count for a
            // bridge that ignored the entry. The thumb stays: his words did reach the agent. The
            // line says what did not.
            let handed_on = files.unwrap_or(0);
            if handed_on < files_on_disk {
                let _ = self.audit.refused(
                    addr,
                    &format!(
                        "the worker took his words (message {}) and {} of {} files",
                        his.msg_id, handed_on, files_on_disk
                    ),
                );
                // What this line may NOT say is why. A short count means the worker is older
                // than files — or that the relay in front of it is, since a door from before
                // files rebuilds exactly this one frame and drops the field on the way. The
                // first is mended by starting the session again and the second is not, the hub
                // cannot tell them apart from one number, and "restart the session" sent him
                // round for ever against a long-running attach service he cannot restart from a
                // phone at all. So it says what happened and stops there.
                let text = "That file did not reach the agent — the worker here is too old to \
                            take files, and it got only your words. Sending it again will not \
                            help until it has been started fresh.";
                let outcome = self.say_under(addr, text, &his.msg_id).await;
                if !matches!(outcome, SendOutcome::Sent(_) | SendOutcome::Clamped(_)) {
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(), outcome = ?outcome,
                        "could not tell him the adapter dropped his file"
                    );
                }
            }
            return;
        }
        let why = plain_reason(reason);
        let _ = self.audit.refused(
            addr,
            &format!(
                "what he typed (message {}) was not handed on: {why}",
                his.msg_id
            ),
        );
        // Only into a topic that is already bound. It always is — the words came from a message
        // he typed in it — and a refusal must never be the reason a topic gets made.
        if self.registry.lock().await.topic_of(addr).is_none() {
            tracing::warn!(
                project = %addr.project, lane = addr.lane_field(),
                "an adapter refused his words for a conversation with no topic; nowhere to say so"
            );
            return;
        }
        let text = format!(
            "What you typed did not reach the agent — {why}. It will not be delivered later."
        );
        // Under the line it is about, so two lines typed a second apart cannot be confused.
        let outcome = self.say_under(addr, &text, &his.msg_id).await;
        if !matches!(outcome, SendOutcome::Sent(_) | SendOutcome::Clamped(_)) {
            tracing::warn!(
                project = %addr.project, lane = addr.lane_field(), outcome = ?outcome,
                "could not tell him his words were refused"
            );
        }
    }

    /// One of his files, from Telegram onto the hub's own disk — or the reason it is not there.
    ///
    /// The ceiling is checked three times, and each catches a case the others cannot: the size on
    /// the message, before any call is made; the size `getFile` answers, before any byte moves;
    /// and the stream itself, because both sizes are optional and the client library reads an
    /// absent one as four gigabytes. A file cut off by the stream is removed, not kept: a half
    /// screenshot at a path the agent was never told about is disk and nothing else.
    ///
    /// Nothing Telegram or the phone said is in the path. The extension is keyed on a declared
    /// mime through a short table; for a photo, which declares none, the mime is read off the
    /// extension of Telegram's OWN storage path in the `getFile` answer, per file, rather than
    /// assumed for every photo ever.
    async fn fetch(&self, addr: &Addr, sent: &SentFile) -> Fetched {
        use crate::media::{Capped, FETCH_CEILING, MediaStore};
        use hub_proto::FileWhy;
        let mut file = hub_proto::MessageFile {
            kind: sent.kind,
            path: None,
            mime: sent.mime.clone(),
            bytes: None,
            filename: sent.filename.clone(),
            why: None,
        };
        if let Some(size) = sent.size.filter(|s| *s > FETCH_CEILING) {
            file.why = Some(FileWhy::TooBig);
            return Fetched::too_big(file, HowBig::Reported(size));
        }
        self.media.sweep(std::time::SystemTime::now());
        let dir = match self.media.dir_for(addr) {
            Ok(dir) => dir,
            Err(e) => {
                tracing::error!(
                    project = %addr.project, lane = addr.lane_field(), error = %e,
                    "the conversation's media directory is not one this hub will write into, so \
                     his file was not fetched; nothing he does from his phone can mend it"
                );
                // NOT `download-failed`. Nothing was downloaded — no call was made at all — and
                // the sentence that word earns is "send it again", which is a loop with no end
                // in it: every file he sends meets the same directory.
                file.why = Some(FileWhy::NotStored);
                return Fetched::plain(file);
            }
        };
        // One deadline for the whole of one file — `getFile` and the body together — because what
        // is being bounded is the time this conversation's next update waits, and that does not
        // care which half of the fetch stopped moving.
        let by_then = tokio::time::Instant::now()
            + Duration::from_millis(self.fetch_deadline.load(Ordering::SeqCst));
        let gave_up = |what: &str| Refused {
            why: format!("Telegram did not finish {what} before the hub gave up on it"),
            flood_wait: None,
        };
        let located = match tokio::time::timeout_at(by_then, self.surface.locate(&sent.file_id))
            .await
            .unwrap_or_else(|_| Err(gave_up("saying where his file is")))
        {
            Ok(located) => located,
            Err(refused) => {
                // Telegram refuses `getFile` for a file over its ceiling with one sentence, and
                // that is the honest reason rather than "the download failed, send it again" —
                // which would send him round once more for the same answer. Nothing measured a
                // byte here, so the line that follows claims no number: the size on the message
                // is what Telegram said about a file it is now refusing to hand over, and
                // repeating it back as if it were the reason reads as the opposite of the truth.
                let too_big = telegram_said_too_big(&refused.why);
                file.why = Some(if too_big {
                    FileWhy::TooBig
                } else {
                    FileWhy::DownloadFailed
                });
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(), error = %refused,
                    "Telegram would not say where his file is"
                );
                return if too_big {
                    Fetched::too_big(file, HowBig::OnlyTelegramSaysSo)
                } else {
                    Fetched::plain(file)
                };
            }
        };
        if let Some(size) = located.file_size.filter(|s| *s > FETCH_CEILING) {
            file.why = Some(FileWhy::TooBig);
            return Fetched::too_big(file, HowBig::Reported(size));
        }
        if file.mime.is_none() && sent.kind == hub_proto::FileKind::Photo {
            file.mime =
                crate::media::mime_from_telegram_path(&located.file_path).map(str::to_owned);
        }
        let path = MediaStore::mint(&dir, file.mime.as_deref());
        let opened = match MediaStore::create(&path) {
            Ok(opened) => opened,
            Err(e) => {
                tracing::error!(
                    project = %addr.project, lane = addr.lane_field(), error = %e,
                    path = %path.display(), "could not open a file to fetch his file into"
                );
                // A full disk, or a directory that changed under the hub between the check and
                // here. Nothing was downloaded, so this is the store's failure and not Telegram's.
                file.why = Some(FileWhy::NotStored);
                return Fetched::plain(file);
            }
        };
        let mut into = Capped::new(opened, FETCH_CEILING);
        match tokio::time::timeout_at(
            by_then,
            self.surface.download(&located.file_path, &mut into),
        )
        .await
        .unwrap_or_else(|_| Err(gave_up("sending his file")))
        {
            Ok(()) => {
                file.bytes = Some(into.written());
                file.path = Some(path.to_string_lossy().into_owned());
                tracing::info!(
                    project = %addr.project, lane = addr.lane_field(),
                    bytes = into.written(), path = %path.display(), "fetched a file he sent"
                );
            }
            Err(refused) => {
                let ceiling = into.hit_the_ceiling();
                // Read BEFORE the writer is dropped: with no size on the message and none in the
                // `getFile` answer this is the only number anybody has, and `docs/ATTACHING.md`
                // §14.4 promises him the number it reached rather than the ceiling read back.
                let reached = into.written();
                drop(into);
                if let Err(e) = fs::remove_file(&path) {
                    tracing::warn!(
                        error = %e, path = %path.display(),
                        "could not remove the partial file; the sweep will take it"
                    );
                }
                file.why = Some(if ceiling {
                    FileWhy::TooBig
                } else {
                    FileWhy::DownloadFailed
                });
                if !ceiling {
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(), error = %refused,
                        "the download of his file from Telegram broke"
                    );
                    return Fetched::plain(file);
                }
                return Fetched::too_big(file, HowBig::Counted(reached));
            }
        }
        Fetched::plain(file)
    }

    /// One line under his message for a file that is not on the hub's disk, in words.
    ///
    /// Under the message it is about, so two files a second apart cannot be confused. The
    /// sentences are the ones `docs/ATTACHING.md` §14.4 promises, in the register the refusal of
    /// his typed words already uses.
    async fn say_the_file_did_not_come(
        &self,
        addr: &Addr,
        msg_id: &MsgId,
        file: &hub_proto::MessageFile,
        how_big: HowBig,
    ) {
        let ceiling = megabytes(crate::media::FETCH_CEILING);
        let text = match file.why {
            // Three roads to "too big" and they know different things. The size the message
            // reported is a number to say back; the stream running past the ceiling with no size
            // reported leaves only the count the hub took, which is the number §14.4 promises;
            // and a `getFile` that refused for size measured nothing at all, so it claims no
            // number — reading back the size on the message there would name a figure UNDER the
            // ceiling as the reason it is over it.
            Some(hub_proto::FileWhy::TooBig) => match how_big {
                HowBig::Reported(size) => format!(
                    "That file did not reach the agent — it is {}, and the most the bot may fetch \
                     is {ceiling}. It will not be fetched later.",
                    megabytes(size)
                ),
                HowBig::Counted(reached) => format!(
                    "That file did not reach the agent — it was still coming at {}, and the most \
                     the bot may fetch is {ceiling}. It will not be fetched later.",
                    megabytes(reached)
                ),
                HowBig::OnlyTelegramSaysSo | HowBig::NotAsked => format!(
                    "That file did not reach the agent — Telegram says it is over the {ceiling} \
                     the bot may fetch. It will not be fetched later."
                ),
            },
            // No retry, because there is nothing on his end to retry: the bytes were never asked
            // for, and every file he sends will meet the same directory until somebody with a
            // terminal fixes it. Saying "send it again" here is a loop with no end in it.
            Some(hub_proto::FileWhy::NotStored) => "That file did not reach the agent — this \
                 machine had nowhere to put it. Sending it again will not help; whoever looks \
                 after this machine has the reason."
                .to_owned(),
            Some(hub_proto::FileWhy::DownloadFailed) | None => {
                "That file did not reach the agent — the download from Telegram failed. Send it \
                 again."
                    .to_owned()
            }
        };
        let outcome = self.say_under(addr, &text, msg_id).await;
        if !matches!(outcome, SendOutcome::Sent(_) | SendOutcome::Clamped(_)) {
            tracing::warn!(
                project = %addr.project, lane = addr.lane_field(), outcome = ?outcome,
                "could not tell him his file did not come through"
            );
        }
    }

    async fn say_and_ack(
        &self,
        addr: &Addr,
        text: &str,
        options: &[AskOption],
    ) -> (Delivered, Option<hub_proto::AckWhy>) {
        let outcome = self.say(addr, text, options).await;
        self.ack_for(&outcome)
    }

    /// A `say` or a `done` that carries a file: the words, and the file from the outbox with
    /// them — or the words with one line saying the file did not come, never the words alone
    /// in silence (`docs/ATTACHING.md` §14.4).
    ///
    /// One send when the words fit a caption, the file with the words under it. Two when they do
    /// not — the words first, then the file — and the ack is for the pair: `yes` only when both
    /// landed. A file refused before any upload goes out as the words with the line appended, in
    /// one send. A file Telegram refused is the same line: appended to the words sent again when
    /// they were its caption and so never landed, alone under them when they had already gone.
    /// Whatever happened to the file, the ack tells the truth about the WORDS first, and says
    /// `no-file` only when they reached him.
    async fn say_with_file_and_ack(
        &self,
        addr: &Addr,
        text: &str,
        file: &hub_proto::SayFile,
    ) -> (Delivered, Option<hub_proto::AckWhy>) {
        use hub_proto::AckWhy;
        let upload = match self.open_for_sending(addr, file).await {
            Ok(upload) => upload,
            Err(line) => {
                let outcome = self.say(addr, &with_the_line(text, &line), &[]).await;
                return match self.ack_for(&outcome) {
                    // Clamped clips the END of a message and the line is the end of this one, so
                    // he has the words and may not have the sentence that explains the gap. Only
                    // a whole send is proof he can read the reason.
                    (Delivered::Yes, _) => (
                        Delivered::Yes,
                        Some(no_file(matches!(outcome, SendOutcome::Sent(_)))),
                    ),
                    other => other,
                };
            }
        };
        // Counted the way Telegram counts — UTF-16 code units, not characters. A caption of seven
        // hundred emoji is seven hundred characters and fourteen hundred units: by characters it
        // fits, and Telegram refuses it, so the words went out again alone with a line explaining
        // the gap — one send wasted and a caption he reads as a separate message. Deciding by units
        // sends such a caption words-first from the start, which is the same shape with nothing
        // wasted. `fit` below still clips by character; that is safe here because anything that
        // reaches it has at most 1024 units and therefore at most 1024 characters.
        let words_first = text.encode_utf16().count() > CAPTION_MAX;
        let mut words_clamped = false;
        if words_first {
            match self.say(addr, text, &[]).await {
                SendOutcome::Sent(_) => {}
                SendOutcome::Clamped(_) => words_clamped = true,
                // The words themselves did not go; nothing is said about a file under words he
                // never got, and the ack is theirs.
                other => return self.ack_for(&other),
            }
        }
        let caption = if words_first { "" } else { text };
        let outcome = self.say_file(addr, &upload, &file.name, caption).await;
        match outcome {
            SendOutcome::Sent(_) | SendOutcome::Clamped(_) => (
                Delivered::Yes,
                (words_clamped || matches!(outcome, SendOutcome::Clamped(_)))
                    .then_some(AckWhy::Clamped),
            ),
            // It went out and could not be checked. Nothing is said and nothing is retried: it
            // may be on his phone, and a second copy of a picture is a second picture.
            SendOutcome::Unseen => (Delivered::Unseen, None),
            SendOutcome::Refused(why) => {
                let _ = self.audit.refused(
                    addr,
                    &format!(
                        "the file the agent attached ({}) was refused by Telegram",
                        file.name
                    ),
                );
                let line = telegram_refused_the_file(&why, upload.as_photo);
                let said = if words_first {
                    with_the_line("", &line)
                } else {
                    with_the_line(text, &line)
                };
                let outcome = self.say(addr, &said, &[]).await;
                // The line is its own send into the same chat, and it can be shed or refused like
                // any other. When it is, he has words and no explanation, and the agent has to
                // know that rather than be told the reason is waiting on his phone.
                let landed = matches!(outcome, SendOutcome::Sent(_));
                if words_first || matches!(outcome, SendOutcome::Sent(_) | SendOutcome::Clamped(_))
                {
                    (Delivered::Yes, Some(no_file(landed)))
                } else {
                    self.ack_for(&outcome)
                }
            }
            // Shed, switched off, or nowhere to put it. When the words had already gone the truth
            // is "he has the words and not the file"; no line, because whatever refused the file
            // refuses a line too, and the audit already says `shed`. So the ack says NOT-SAID as
            // well as NOT-SENT: this is the one `no-file` where there is nothing on his phone to
            // explain the gap, and the one that mends itself if the agent attaches it again in a
            // minute.
            other => {
                if words_first {
                    (Delivered::Yes, Some(AckWhy::NoFileUnsaid))
                } else {
                    self.ack_for(&other)
                }
            }
        }
    }

    /// The file an agent named, opened from the conversation's outbox and read — or the sentence
    /// that goes under his words instead, with the reason in the audit.
    ///
    /// The name is checked against the address rules BEFORE the outbox is touched, so nothing an
    /// agent says can be a path; then `media.rs` opens it following no link and checks what it
    /// opened; then the bytes are read, bounded once more on the stream — the wall can append
    /// between the `fstat` and the read — and never by the size it reported.
    async fn open_for_sending(
        &self,
        addr: &Addr,
        file: &hub_proto::SayFile,
    ) -> Result<Upload, String> {
        use crate::media::{MediaStore, NotSendable, PHOTO_CEILING, SEND_CEILING};
        const NOT_A_FILE: &str = "it was not a file the bot may send";
        if !MediaStore::name_is_openable(&file.name) {
            // The name is not echoed anywhere: it can carry a tab, which forges a record in the
            // audit, or a control character, which reorders what a person reads in the journal.
            tracing::warn!(
                project = %addr.project, lane = addr.lane_field(), len = file.name.len(),
                "an agent named a file the hub will not open; the words went without it"
            );
            let _ = self.audit.refused(
                addr,
                "the file the agent attached was not sent: the name is not one the hub will open",
            );
            return Err(NOT_A_FILE.to_owned());
        }
        self.outbox.sweep(std::time::SystemTime::now());
        let opened = match self.outbox.open_for_sending(addr, &file.name, SEND_CEILING) {
            Ok(opened) => opened,
            Err(NotSendable::TooBig(size)) => {
                let _ = self.audit.refused(
                    addr,
                    &format!(
                        "the file the agent attached ({}) was not sent: it is {size} bytes, over \
                         the {SEND_CEILING} the bot may send",
                        file.name
                    ),
                );
                return Err(format!(
                    "it is {}, and the most the bot may send is {}",
                    megabytes(size),
                    megabytes(SEND_CEILING)
                ));
            }
            Err(NotSendable::NotAFile(why)) => {
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(), name = %file.name, why,
                    "the file an agent attached is not one the hub will send; the words went \
                     without it"
                );
                let _ = self.audit.refused(
                    addr,
                    &format!(
                        "the file the agent attached ({}) was not sent: {why}",
                        file.name
                    ),
                );
                return Err(NOT_A_FILE.to_owned());
            }
        };
        // Read off the checked descriptor, on a thread that may block: fifty megabytes from the
        // page cache is milliseconds, but not milliseconds every other connection's read loop
        // should wait through. Bounded at one byte past the ceiling, so a file that grew between
        // the `fstat` and this read is refused rather than sent at whatever size it reached.
        let size = opened.size;
        let read = tokio::task::spawn_blocking(move || {
            use std::io::Read as _;
            let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
            (&opened.file)
                .take(SEND_CEILING + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        })
        .await;
        let bytes = match read {
            Ok(Ok(bytes)) if bytes.len() as u64 <= SEND_CEILING => bytes,
            Ok(Ok(bytes)) => {
                let _ = self.audit.refused(
                    addr,
                    &format!(
                        "the file the agent attached ({}) was not sent: it grew past the ceiling \
                         while being read",
                        file.name
                    ),
                );
                return Err(format!(
                    "it is over {}, the most the bot may send (it was {} when read)",
                    megabytes(SEND_CEILING),
                    megabytes(bytes.len() as u64)
                ));
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    project = %addr.project, lane = addr.lane_field(), name = %file.name,
                    error = %e, "could not read the file an agent attached"
                );
                let _ = self.audit.refused(
                    addr,
                    &format!(
                        "the file the agent attached ({}) was not sent: it could not be read",
                        file.name
                    ),
                );
                return Err(NOT_A_FILE.to_owned());
            }
            Err(e) => {
                tracing::error!(error = %e, "the thread reading an agent's file did not finish");
                return Err(NOT_A_FILE.to_owned());
            }
        };
        let is_picture = matches!(
            file.mime.as_deref(),
            Some("image/jpeg" | "image/png" | "image/webp")
        );
        let as_photo = match file.r#as {
            Some(hub_proto::FileAs::Document) => false,
            Some(hub_proto::FileAs::Photo) => true,
            None => is_picture,
        } && bytes.len() as u64 <= PHOTO_CEILING;
        Ok(Upload {
            bytes,
            filename: shown_as(file.filename.as_deref(), &file.name),
            mime: file.mime.clone(),
            as_photo,
        })
    }

    /// Turn what actually happened into the three-valued answer the bridge branches on.
    ///
    /// `Unseen` is not a hedge. Telegram has no idempotency key, so a send that times out may or
    /// may not have landed and there is no way to ask. Reporting that as `No` would invite a retry,
    /// and a retried question with buttons is two live menus for one question, both tappable
    /// forever — a misfire this system would have manufactured for itself.
    fn ack_for(&self, outcome: &SendOutcome) -> (Delivered, Option<hub_proto::AckWhy>) {
        match outcome {
            SendOutcome::Sent(_) => (Delivered::Yes, None),
            SendOutcome::Clamped(_) => (Delivered::Yes, Some(hub_proto::AckWhy::Clamped)),
            SendOutcome::TooFast(_) => (Delivered::No, Some(hub_proto::AckWhy::TooFast)),
            SendOutcome::TopicGone => (Delivered::No, Some(hub_proto::AckWhy::NoTopic)),
            SendOutcome::Refused(_) => (Delivered::No, Some(hub_proto::AckWhy::TelegramRefused)),
            SendOutcome::Unseen => (Delivered::Unseen, None),
            // The same answer every frame queued behind it gets, for the same reason.
            SendOutcome::SwitchedOff => (Delivered::No, None),
        }
    }

    /// The operator answered from his phone: take the keyboard away and say what he chose.
    ///
    /// This used to just delete the ledger record. That left the keyboard live forever — the record
    /// the retirement needed was gone, so the `ask_resolved` that followed found nothing to retire —
    /// and a menu that stays tappable after it has been answered is the second live menu this
    /// design went out of its way not to manufacture.
    pub async fn answered_from_phone(&self, chat_id: i64, msg_id: &MsgId, label: &str) {
        let record = { self.ledger.lock().await.get(chat_id, msg_id).cloned() };
        let Some(record) = record else {
            let _ = self.ledger.lock().await.forget(chat_id, msg_id);
            return;
        };
        let note = format!("answered from your phone — {label}");
        let retired = self
            .surface
            .retire_buttons(record.topic_id, msg_id, &record.text, &note)
            .await;
        // The record is forgotten only when the buttons are actually gone. Forgetting first left a
        // live keyboard with nothing behind it: still tappable, still offering a choice already
        // made, and the next tap answered "I have no record of that question". Fail closed — the
        // order is "the buttons are gone, therefore the record may go", never the reverse.
        match retired {
            Ok(()) => {
                let _ = self.ledger.lock().await.forget(chat_id, msg_id);
            }
            Err(e) => {
                // Written down, not merely logged. "Leaving the record so it can be retired later"
                // was half a plan: the record says `answered`, and every sweep and the terminal's
                // own `ask_resolved` step over an answered record on purpose — so nothing was ever
                // coming, and the menu sat on his phone refusing every tap until the ledger dropped
                // it two days on. This mark is the difference between a retirement in flight and
                // one that failed, and it carries HIS words so whoever finally makes the edit signs
                // it off with the button he pressed rather than a sweep's guess.
                let _ = self.ledger.lock().await.mark_closed(
                    chat_id,
                    msg_id,
                    Closed {
                        how: hub_proto::AskEnd::Answered,
                        note: note.clone(),
                    },
                );
                tracing::error!(
                    error = %e, project = %record.project, lane = record.addr().lane_field(),
                    "answered from the phone but the keyboard is still there; it stays written down \
                     so the next thing that can take it off does, with what he chose"
                );
            }
        }
    }

    /// A tap resolved, and then the project could not be told. Take the question back.
    ///
    /// A tap is written down as answered BEFORE the caller delivers, and it has to be: the window
    /// between the two is a Telegram round trip on a keyboard the operator is still looking at, and
    /// a second tap inside it would deliver twice. The cost was that a delivery which then failed
    /// burned the question for good — nothing ever cleared `answered` — so he was told first that
    /// the project was not connected (false when it was merely behind, which is the outbox-full
    /// case this code documents as expected) and then, on the same button, that it had already been
    /// answered and nothing had been sent. Only the second half of that was true.
    ///
    /// **Nothing went out on this path.** `deliver` reports false only when there is no connection
    /// or the frame never entered the outbox, never after a frame has gone, so withdrawing cannot
    /// contradict something an agent has already been told.
    ///
    /// **Returns which of the three actually happened**, because the caller has to say so and the
    /// sentences are not interchangeable. Two would not do: `deliver` reports false when the
    /// bridge's outbox is full, and that bridge is LIVE — it can be sending `ask_resolved` for this
    /// very question in the same instant, which retires the keyboard and forgets the record. So
    /// "there was nothing left to take back" is a real outcome on this path and not a defensive
    /// branch, and reporting it as "the buttons are still there" would be a plain untruth. The record follows the same order as every
    /// other retirement here: it goes only once the buttons have. Forgetting it either way looked
    /// safe — nothing was sent, so nothing can be contradicted — but the record is also the only
    /// handle any LATER retirement has on that message, so throwing it away while the menu is
    /// still live means nothing can ever take the menu off: not the session's own withdrawal, not
    /// a timeout, not the next session. What is undone instead is the authorisation, so the
    /// still-live keyboard can be tapped again rather than answering "that was already answered".
    pub async fn withdraw_undelivered(&self, chat_id: i64, msg_id: &MsgId) -> Withdrawal {
        // Written into the message he is looking at, in whichever topic that is. A worktree's
        // topic and its project's sit side by side, and the project may be perfectly reachable
        // while the thing that asked this is not.
        self.take_the_question_back(chat_id, msg_id, "not sent — nothing here could be reached")
            .await
    }

    /// Take a question back with the sentence that is TRUE of the path taking it back.
    ///
    /// Two paths take a question back and only one of them reached nobody. A tap the session
    /// answered `refused` DID reach something — it read the answer off the wire and said no — so
    /// telling him nothing could be reached is a plain untruth about the one thing he is looking
    /// at. Usually there is no record left and this does nothing at all; the case it is not a
    /// no-op is a keyboard edit Telegram refused, which is exactly the network trouble that causes
    /// the refusal too, so the two co-occur rather than being independent.
    async fn take_the_question_back(&self, chat_id: i64, msg_id: &MsgId, note: &str) -> Withdrawal {
        let record = { self.ledger.lock().await.get(chat_id, msg_id).cloned() };
        let Some(record) = record else {
            return Withdrawal::NothingLeftToTakeBack;
        };
        match self
            .surface
            .retire_buttons(record.topic_id, msg_id, &record.text, note)
            .await
        {
            Ok(()) => {
                let _ = self.ledger.lock().await.forget(chat_id, msg_id);
                Withdrawal::Retired
            }
            Err(e) => {
                tracing::error!(
                    error = %e, project = %record.project, lane = record.addr().lane_field(),
                    "a question could not be taken back and its keyboard is still on his phone"
                );
                let _ = self.ledger.lock().await.mark_unanswered(chat_id, msg_id);
                Withdrawal::StillOnHisPhone
            }
        }
    }

    /// Strip a stale keyboard, because a screen could never tell you a question stopped being asked.
    ///
    /// `addr` and `instance` are the two halves of who is saying so, and neither can be dropped.
    /// Ask ids repeat across sessions, so an outcome matched on the ask id alone landed on a
    /// question another session was still waiting on — and they repeat across LANES too, where the
    /// instance cannot separate them because one opencode process holds every lane it serves.
    async fn retire(
        &self,
        addr: &Addr,
        instance: &str,
        ask_id: &AskId,
        how: hub_proto::AskEnd,
        outcome: Option<&str>,
    ) {
        let note = match (how, outcome) {
            (hub_proto::AskEnd::Answered, Some(o)) => format!("answered at the terminal — {o}"),
            (hub_proto::AskEnd::Answered, None) => "answered at the terminal".to_owned(),
            // The bridge's own sentence for why the question went — "the session that asked has
            // ended" from a door whose engine exited — is what he reads, in place of the hub's,
            // which is true and says nothing. Unless it is blank: no words is not a sentence.
            (hub_proto::AskEnd::Withdrawn, Some(o)) if !o.trim().is_empty() => o.to_owned(),
            (hub_proto::AskEnd::Withdrawn, _) => "no longer being asked".to_owned(),
            (hub_proto::AskEnd::Timeout, _) => "timed out".to_owned(),
        };
        // Clipped before it is kept, not just before it is shown. `outcome` is an adapter's free
        // text and the wire bounds it only at a whole frame; stored whole it would put 64 KiB of
        // someone's paragraph into a file that is rewritten on every ask and every tap of every
        // project on this box, and onto a message where every character of it is one the QUESTION
        // does not get.
        let note = crate::queue::fit(&note, RETIREMENT_NOTE_ROOM).0;
        // Written down BEFORE the first edit, and for every message of the ask at once. The edit
        // is a Telegram round trip on a menu the operator is looking at; until this change nothing
        // was marked before it came back, so a tap inside that window — or after an edit Telegram
        // refused — resolved and delivered a phone answer into an agent that had already answered
        // at its own terminal. `resolve_tap` reads this mark under the same lock it writes under.
        let targets = {
            self.ledger.lock().await.close_all(
                addr,
                instance,
                ask_id,
                Closed {
                    how,
                    note: note.clone(),
                },
            )
        };
        if targets.is_empty() {
            // Two different things, and the wrong one sends whoever reads this looking for a bug
            // that is not there. Usually the keyboard is long gone. But a question the operator
            // answered from his phone a moment ago still has a record here with its menu coming
            // off, and that retirement is the tap's — saying there was nothing left to take off
            // would be plainly false about a message he is looking at.
            let still_here = {
                self.ledger
                    .lock()
                    .await
                    .messages_for(addr, instance, ask_id)
            };
            if still_here.is_empty() {
                tracing::debug!(
                    project = %addr.project, lane = addr.lane_field(), ask = %ask_id,
                    "a question ended at the terminal with no keyboard of its own left to take off"
                );
            } else {
                tracing::debug!(
                    project = %addr.project, lane = addr.lane_field(), ask = %ask_id,
                    "a question ended at the terminal that the operator had already answered from \
                     his phone; taking its keyboard off belongs to that tap"
                );
            }
        }
        self.retire_each(addr, targets, &note).await;
    }

    /// Take the keyboard off each of these messages and leave the note in its place.
    async fn retire_each(&self, addr: &Addr, targets: Vec<(i64, MsgId)>, note: &str) {
        for (chat, msg) in targets {
            // The record carries both the topic and the question's own words, so the retirement
            // does not have to go back to the registry for one and cannot leave the other out.
            let record = { self.ledger.lock().await.get(chat, &msg).cloned() };
            let retired = match &record {
                Some(record) => {
                    // What HE did outranks everything, then the record's own note, then the
                    // caller's. Both marks can sit on one record — he tapped, and the agent then
                    // said the question was over — and only one of them is a thing he did; writing
                    // "answered at the terminal" over a button he pressed is the two-truths defect
                    // from the other side. Below that, the record's note beats the caller's,
                    // because this retirement may be a sweep that only knows the question is not
                    // open — and its sentence, "the session that asked this restarted", is true of
                    // an abandoned question and false of one that was answered and then would not
                    // let go of its keyboard.
                    let note = record
                        .answered
                        .as_ref()
                        .and_then(|chosen| record.options.iter().find(|o| &o.option_id == chosen))
                        .map(|o| format!("answered from your phone — {}", o.label))
                        .or_else(|| record.closed.as_ref().map(|c| c.note.clone()))
                        .unwrap_or_else(|| note.to_owned());
                    self.surface
                        .retire_buttons(record.topic_id, &msg, &record.text, &note)
                        .await
                }
                None => Ok(()),
            };
            // Same order as a tap: the record goes only once the keyboard has. A retirement that
            // failed and forgot anyway leaves a menu that outlives the question it belonged to.
            match retired {
                Ok(()) => {
                    let _ = self.ledger.lock().await.forget(chat, &msg);
                }
                // Named for the conversation the RECORD belongs to, not the one that triggered the
                // sweep: a bridge arriving now clears what other worktrees of its project left, and
                // a log line naming the sweeper sends whoever reads it to the wrong topic.
                Err(e) => {
                    let whose = record
                        .as_ref()
                        .map_or_else(|| addr.clone(), AskRecord::addr);
                    tracing::error!(
                        error = %e, project = %whose.project, lane = whose.lane_field(),
                        "a question stopped being asked but its keyboard is still there; it stays \
                         written down as closed, so a tap on it is refused, and the next session's \
                         arrival tries the edit again"
                    );
                }
            }
        }
    }
}

/// Roughly what one buffered frame costs to hold, for the pre-pong bound.
///
/// The text is the whole of it in practice; the rest is a fixed handful of bytes. Exact accounting
/// would mean encoding a frame this side is about to hand straight to `handle`, which is a cost
/// paid on every frame to make a bound slightly tighter.
/// How the settling window ended: the one way in, the three ways a connection is refused without
/// ever having been live, and the one way it leaves of its own accord. Each is said differently,
/// and each answers for whatever was buffered before the socket goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Settled {
    /// The pong named the ping. The topic is made after this, and nothing before it.
    Live,
    /// More than [`PRE_PONG_FRAMES`] or [`PRE_PONG_BYTES`] before the pong.
    Overflowed,
    /// A frame over [`hub_proto::MAX_FRAME_BYTES`] before the pong.
    Oversize,
    /// The window passed, or the socket ended, with no pong.
    Gone,
    /// It said `bye` before its pong. Not a refusal: a bridge that only wanted to know the hub was
    /// there — `kickoff-hub-attach --check` — ends this way on purpose, and is owed the same acks
    /// as the others and no line in the audit.
    SaidGoodbye,
    /// It was ended from outside its own read loop before it ever became live — its project
    /// switched off at the terminal, or a later run taking the address. Nothing is made for it: a
    /// topic created and greeted for a project he had just turned off would be the switch producing
    /// the one thing it exists to stop.
    Kicked(Kick),
}

/// What a frame costs against the pre-pong hold: its text plus an allowance for the envelope, and
/// never more than one frame can be on the wire. The codec refuses anything past
/// [`hub_proto::MAX_FRAME_BYTES`] before it gets here, so [`PRE_PONG_FRAMES`] of the largest frames
/// possible cost at most [`PRE_PONG_BYTES`] — the count and the byte bound say the same thing,
/// rather than the bytes tripping a frame early on the last of a legal backlog.
fn frame_cost(frame: &BridgeFrame) -> usize {
    let cost = match frame {
        BridgeFrame::Say { text, .. }
        | BridgeFrame::Done { text, .. }
        | BridgeFrame::Ask { text, .. } => text.len() + 64,
        _ => 64,
    };
    cost.min(hub_proto::MAX_FRAME_BYTES)
}

/// An adapter's reason for refusing his words, made fit to put in front of him.
///
/// It is the adapter's own text and it lands in his topic, so it gets what a lane name gets: no
/// control character survives (a newline here would forge a line in the audit file, which is one
/// tab-separated record per line), it is one line's worth and no more, and it is never empty —
/// "did not reach the agent — ." is a sentence with a hole in it.
fn plain_reason(reason: Option<&str>) -> String {
    const MOST: usize = 200;
    let cleaned: Vec<&str> = reason
        .unwrap_or("")
        .split(|c: char| c.is_control() || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect();
    if cleaned.is_empty() {
        return "the worker did not say why".to_owned();
    }
    crate::queue::fit(&cleaned.join(" "), MOST).0
}

/// Did Telegram refuse `getFile` because the file is over its own ceiling?
///
/// Deliberately narrow, like the deleted-topic match: the Bot API answers exactly "Bad Request:
/// file is too big" for a file over 20 MB, and a broader match would turn some other refusal into
/// "too big", which tells him a smaller file would have worked when it would not.
fn telegram_said_too_big(why: &str) -> bool {
    why.to_lowercase().contains("file is too big")
}

/// One of his files after the hub has tried to fetch it: the frame's entry, and what the hub
/// measured on the way — which the entry cannot carry, because `bytes` on the wire means "what
/// is on disk" and there is nothing on disk here.
struct Fetched {
    file: hub_proto::MessageFile,
    how_big: HowBig,
}

impl Fetched {
    fn plain(file: hub_proto::MessageFile) -> Self {
        Self {
            file,
            how_big: HowBig::NotAsked,
        }
    }
    fn too_big(file: hub_proto::MessageFile, how_big: HowBig) -> Self {
        Self { file, how_big }
    }
}

/// What is known about the size of a file that was not fetched — which is not always a number.
///
/// Three roads reach `too-big` and only two of them have measured anything. Keeping them apart is
/// the difference between a true sentence and one that names a figure under the ceiling as the
/// reason a file is over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HowBig {
    /// Telegram reported this size, and it is over the ceiling.
    Reported(u64),
    /// Nobody reported one. The hub counted this many bytes before the ceiling stopped the stream.
    Counted(u64),
    /// `getFile` refused, saying only that the file is too big. Nothing here measured a byte.
    OnlyTelegramSaysSo,
    /// Not a size failure at all.
    NotAsked,
}

/// What one send carries: an agent's words with their buttons, or an agent's file with the words
/// as its caption. One type so that `send_into` — the one place a message goes out — stays one
/// place, and a file takes its turn, its audit pair and its flood-wait drain exactly as words do.
enum Outbound<'a> {
    Words {
        text: &'a str,
        buttons: &'a [AskOption],
        reply_to: Option<&'a MsgId>,
    },
    File {
        upload: &'a Upload,
        /// Its name in the outbox, for the audit line; past the address rules by now.
        name: &'a str,
        caption: &'a str,
    },
}

/// Which of the two file reasons an ack carries: whether the operator can SEE why.
///
/// `no-file` promises an adapter that the hub's reason is in his topic, and an adapter says so to
/// its agent. When the line did not land — shed by the same budget, refused by the same topic —
/// that promise is false, and an agent that answers "as you saw, the screenshot did not come
/// through" is talking to somebody who saw nothing.
fn no_file(the_line_landed: bool) -> hub_proto::AckWhy {
    if the_line_landed {
        hub_proto::AckWhy::NoFile
    } else {
        hub_proto::AckWhy::NoFileUnsaid
    }
}

/// The words, and under them in the same message the one line saying the file did not come.
///
/// The sentence is `docs/ATTACHING.md` §14.4's, in the register the refusal of his typed words
/// already uses. When there were no words the line is the message.
fn with_the_line(text: &str, why: &str) -> String {
    let line = format!("(The file the agent attached did not come through: {why}.)");
    if text.trim().is_empty() {
        line
    } else {
        format!("{text}\n\n{line}")
    }
}

/// Telegram's refusal of an upload, as one reason under his words.
///
/// A picture refused for its shape gets the one sentence that names the fix — send it as a
/// document — because the hub does not try twice on its own: a second try is a second send from
/// a budget every conversation shares. Anything else is Telegram's own words, minus the "Bad
/// Request:" every one of them starts with.
fn telegram_refused_the_file(why: &str, as_photo: bool) -> String {
    let said = why.to_lowercase();
    if as_photo && (said.contains("photo") || said.contains("image")) {
        return "Telegram would not take it as a picture; ask for it as a document".to_owned();
    }
    let plain = why
        .trim()
        .trim_start_matches("Bad Request:")
        .trim_start_matches("Bad Request")
        .trim();
    // Somebody else's string, on its way into his topic. It gets what an adapter's refusal gets
    // (`plain_reason`): no control character survives, because a newline here reads as the bot
    // speaking a second time and forges a record in the audit, which is one line per record; and
    // one line's worth and no more, because nobody has promised this string a length.
    let plain = crate::queue::fit(
        &plain
            .split(|c: char| c.is_control() || c.is_whitespace())
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        200,
    )
    .0;
    let plain = plain.trim_end_matches('.');
    if plain.is_empty() {
        "Telegram would not take it".to_owned()
    } else {
        format!("Telegram would not take it — {plain}")
    }
}

/// What he sees a document called: the adapter's `filename` when it is something a phone can
/// show, otherwise the outbox name. Control characters are dropped and it is clipped to what a
/// file name can be, because it lands in a multipart header and on his screen, never on a path.
fn shown_as(filename: Option<&str>, name: &str) -> String {
    let cleaned: String = filename
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    if cleaned.trim().is_empty() {
        name.to_owned()
    } else {
        cleaned
    }
}

/// A byte count as he reads it: megabytes, one decimal when it is not a round number.
fn megabytes(bytes: u64) -> String {
    let tenths = bytes / 100_000;
    if tenths % 10 == 0 {
        format!("{} MB", tenths / 10)
    } else {
        format!("{}.{} MB", tenths / 10, tenths % 10)
    }
}

/// Per-process frame counter. Opaque and monotonic is all the protocol asks for.
fn next_frame_seq() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

mod intent;
#[cfg(test)]
mod tests;

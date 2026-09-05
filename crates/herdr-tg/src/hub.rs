//! The socket the bridges connect to, and everything that decides what they may do.
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
//! 1. **Who is on the other end.** `SO_PEERCRED` off the connection. A different uid is closed
//!    without a reply — an answer, even a refusal, is information. Mode 0600 on the socket already
//!    implies this; reading the credential back means the check survives a permissions mistake, and
//!    it yields the pid the single-claim rule needs.
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
//! process died mid-write and nothing else. It deliberately mirrors `audit.rs` rather than
//! extending it: that file's subject is a pane and a keystroke, and the hub has neither.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hub_proto::{
    AckStatus, AckWhy, AskId, AskOption, BridgeFrame, Delivered, Envelope, FrameId, HubFrame,
    LaneId, Limits, MsgId, OptionId, ProjectId, RefusedReason, VERSION,
};
use serde::{Deserialize, Serialize};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, mpsc};

use crate::registry::Registry;

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
}

impl AskRecord {
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
    /// still there because taking it away failed.
    AlreadyAnswered,
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
            r.answered = None;
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
    /// An already-answered record is left alone. Its keyboard is still live only because taking it
    /// away failed, and the outcome written on it is the true one — replacing that with a note
    /// about a restart would be the same misinformation from the other direction.
    pub fn open_for_other_instances(&self, addr: &Addr, instance: &str) -> Vec<(i64, MsgId)> {
        self.matching(|r| r.addr_is(addr) && r.instance != instance && r.answered.is_none())
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
                && r.answered.is_none()
                && !live.contains(&r.addr())
                && r.pid.is_some_and(|pid| !pid_is_alive(pid))
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
    /// whole and renamed into place on every `record`, `mark_answered`, `mark_unanswered` and
    /// `forget` — so every ask and every tap of every project on the box pays for whatever is in
    /// it, on a blocking write with the ledger mutex held. Measured: a record holding a
    /// worst-case-length question costs about 3.9 KB, and one further ask on top of a 360-record
    /// backlog costs about 37 ms under that lock.
    ///
    /// Two things bound it, and both are here because a topic per lane made "the next session of
    /// this project will collect it" stop being true: [`Self::open_where_the_asker_is_gone`] takes
    /// what a worktree left when its process is provably gone, and
    /// [`Self::drop_what_can_no_longer_be_retired`] takes what has aged past the point of being
    /// useful to anybody. `projects.json` grows too, and is the one people notice — but a lane row
    /// there costs 40 bytes and is only read on admission. This is the one to watch.
    fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
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
            fs::create_dir_all(dir)?;
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
    instance: String,
    tx: mpsc::Sender<Envelope<HubFrame>>,
    /// The one way to end this connection from OUTSIDE its own read loop, and why.
    ///
    /// A read loop is a bare `reader.next()`, so nothing but the socket ending could ever stop it —
    /// and removing the map entry alone does not: the send path never consults this map, so a
    /// connection whose claim was taken away kept posting into its topic and spending the chat's
    /// budget until its bridge happened to hang up. Switching a project off has to end the
    /// connection, not merely forget it.
    kick: mpsc::Sender<RefusedReason>,
}

/// How often the hub looks at the registry file for a project switched off at the terminal.
///
/// A second, because "off" has to mean the flood stops NOW, and a poll is the only way a separate
/// process's write reaches a connection this process is holding: the CLI does not hold the socket,
/// and a signal would tie two processes together by pid for the one fact a file already carries. A
/// stat a second on a file of a few kilobytes is nothing; the registry is re-read only when the stat
/// says it changed.
pub const REGISTRY_WATCH_EVERY: Duration = Duration::from_secs(1);

/// One of his typed messages on its way to a bridge: the envelope id it went down under — the id
/// the bridge's `ack` for it names — and which of his messages, in which conversation, it was.
#[derive(Debug)]
struct WordsDown {
    frame: FrameId,
    addr: Addr,
    /// The chat he typed in, so the mark on his message can find it. A message id is only half
    /// an address on Telegram: every chat numbers its own.
    chat_id: i64,
    msg_id: MsgId,
}

/// How many of his messages the hub keeps waiting for an answer about, before the oldest is
/// forgotten. A bridge that never answers must not turn a record nobody will read into a leak.
const WORDS_DOWN_KEPT: usize = 256;

/// The longest lane name the hub will address a conversation by.
///
/// Well past anything kickoff mints — `lane-0902-201212-2783563` is twenty-four characters — and
/// short enough that a lane cannot crowd its project's own name out of a topic title.
pub const MAX_LANE: usize = 64;

/// Which conversation a connection is: a project speaking for itself, or one worktree of it.
///
/// Built ONLY from the project a SECRET resolved to plus the lane the bridge named. That
/// construction is the entire security argument: the project half never comes from the wire, so a
/// bridge naming a lane can only ever reach a lane of the project it has already proved it is. A
/// lane is an address, never a credential.
///
/// `Ord` puts `None` before `Some(_)`, so a project sorts immediately above its own lanes and the
/// list the operator reads comes out grouped without anything sorting it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
/// before a byte is audited. Four things it stops, and only the first is obvious:
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
fn lane_is_addressable(lane: &LaneId) -> bool {
    let s = lane.as_str();
    !s.is_empty()
        && s.len() <= MAX_LANE
        && s != "."
        && s != ".."
        && !s.contains('/')
        && !s.contains('\\')
        && !s.chars().any(char::is_control)
}

/// Is a pid still a process on this machine?
///
/// The evict-a-corpse rule depends on this being a fact rather than a hope. `/proc/<pid>` is the
/// fact; a signal-0 probe would answer "yes" for a pid this user does not own.
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
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

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The socket.

/// `/run/user/<uid>/kickoff/hub.sock`.
///
/// Derived on both sides, never configured. `XDG_RUNTIME_DIR` is not on kickoff's list of variables
/// that survive its `env -i` boundary, so a bridge started by a worker would not see it — the two
/// would derive different paths and neither would be wrong.
pub fn socket_path() -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    PathBuf::from(format!("/run/user/{uid}/kickoff")).join("hub.sock")
}

/// Bind the listener, with the directory and the socket locked down before anything can connect.
///
/// A stale socket file from a previous run is removed first. That is safe because the hub lock is
/// already held by this process — nothing else can be listening — and skipping it would make a
/// crash require a manual `rm` from a keyboard.
pub fn bind(path: &Path) -> anyhow::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
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
    pub ledger: Arc<Mutex<AskLedger>>,
    pub audit: Arc<HubAudit>,
    /// One live connection per ADDRESS, not per project. Two worktrees of one repo are two agents
    /// that can each block on a question of their own, and the second used to be turned away.
    claims: Arc<Mutex<BTreeMap<Addr, Claim>>>,
    /// The claims map, written down for a process that is not this one.
    ///
    /// `herdr-tg projects --json` runs at a terminal and has to say who is connected from the one
    /// place that knows, which is here. Rewritten whole every time the map changes, under the
    /// claims lock so two changes cannot publish out of order; a claim or release happens a dozen
    /// times a day, so the write is nowhere near a hot path. See `presence.rs` for why a file and
    /// what a reader must check before believing it.
    presence: crate::presence::Presence,
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
    /// His typed words that went down to a bridge and may still be answered for.
    ///
    /// The wire lets a bridge answer a `message` with `ack{status, reason}`, and until this
    /// existed the hub read the status of no ack at all — so an adapter with nothing to hand the
    /// words to could say so, honestly, on the wire, and he was told nothing. Bounded at
    /// [`WORDS_DOWN_KEPT`], oldest first out.
    words_down: Arc<Mutex<VecDeque<WordsDown>>>,
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
    settle: Duration,
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
        let presence =
            crate::presence::Presence::new(audit.path().with_file_name(crate::presence::FILE));
        if let Err(e) = presence.write(std::iter::empty()) {
            tracing::error!(
                error = %e, path = %presence.path().display(),
                "could not write down that nothing is connected yet; `projects --json` will say \
                 unknown until a bridge arrives or leaves"
            );
        }
        Self {
            surface,
            registry: Arc::new(Mutex::new(registry)),
            ledger: Arc::new(Mutex::new(ledger)),
            audit: Arc::new(audit),
            claims: Arc::new(Mutex::new(BTreeMap::new())),
            presence,
            budgets: Arc::new(Mutex::new(crate::queue::Budgets::default())),
            send_permit: Arc::new(Mutex::new(())),
            throttle: Arc::new(Mutex::new(Throttle::default())),
            topic_refused: Arc::new(Mutex::new(BTreeMap::new())),
            gist: crate::summarize::Summarizer::from_env().map(Arc::new),
            words_down: Arc::new(Mutex::new(VecDeque::new())),
            mark_permit: Arc::new(Mutex::new(())),
            reactions: Arc::new(Mutex::new(crate::queue::ReactionBudget::default())),
            reaction_refusal_said: Arc::new(AtomicBool::new(false)),
            allowed_chats: Arc::new(allowed_chats),
            people: Arc::new(people.into_iter().collect()),
            forum_chat,
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
    /// Split out from the connection so the decision can be tested without a socket, and so the
    /// order of the gates is visible in one place.
    pub async fn admit(
        &self,
        peer_uid: u32,
        our_uid: u32,
        hello: &BridgeFrame,
        version: u16,
    ) -> Admission {
        if peer_uid != our_uid {
            tracing::warn!(
                peer_uid,
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
        if record.answered.is_some() {
            return Err(TapRefusal::AlreadyAnswered);
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
        {
            let mut ledger = self.ledger.lock().await;
            match ledger.get(chat_id, msg_id) {
                None => return Err(TapRefusal::NoRecord),
                Some(fresh) if fresh.answered.is_some() => {
                    return Err(TapRefusal::AlreadyAnswered);
                }
                Some(_) => {}
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
    async fn deliver_under(&self, addr: &Addr, id: FrameId, frame: HubFrame) -> bool {
        let tx = {
            let claims = self.claims.lock().await;
            match claims.get(addr) {
                None => return false,
                Some(claim) => claim.tx.clone(),
            }
        };
        match tx.try_send(Envelope::new(id, frame)) {
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

    /// Take the project, or refuse — the check and the reservation in ONE critical section.
    ///
    /// This used to be two: `admit` looked for a live incumbent, dropped the lock, and `claim`
    /// inserted unconditionally some awaits later. Two bridges arriving inside that window were
    /// both admitted and the second silently replaced the first — measured at roughly one round in
    /// three when the two `hello`s land within about 100 µs on a multi-thread runtime, which is the
    /// runtime this binary builds. The consequence is the exact failure gate 4 exists to prevent:
    /// two bridges live on one project, both posting into one topic, and a tap on the incumbent's
    /// still-open question refused with "that session has since restarted" while it is sitting
    /// there waiting for the answer.
    ///
    /// A dead incumbent is evicted rather than honoured: a worker that crashed must not lock its
    /// own project out until someone finds a keyboard.
    ///
    /// Returns the receiver the connection must listen on for a [`Claim::kick`]: the one way this
    /// connection can be ended by something other than its own socket.
    pub async fn claim(
        &self,
        addr: Addr,
        pid: u32,
        instance: String,
        tx: mpsc::Sender<Envelope<HubFrame>>,
    ) -> Result<mpsc::Receiver<RefusedReason>, RefusedReason> {
        let (kick, kicked) = mpsc::channel(1);
        {
            let mut claims = self.claims.lock().await;
            // Exclusive per ADDRESS. Widening it to the project was the refusal that made a second
            // worktree of one repo unreachable; widening it to nothing would be the takeover this
            // whole gate exists to refuse, so within one lane the rule is untouched.
            if let Some(old) = claims.get(&addr) {
                if pid_is_alive(old.pid) {
                    tracing::warn!(
                        project = %addr.project, lane = addr.lane_field(),
                        incumbent = old.pid, arriving = pid,
                        "a second bridge tried to take a conversation that is already connected"
                    );
                    return Err(RefusedReason::AlreadyClaimed);
                }
                tracing::info!(
                    project = %addr.project, lane = addr.lane_field(), dead = old.pid,
                    "evicting a bridge that is no longer running"
                );
            }
            claims.insert(
                addr,
                Claim {
                    pid,
                    instance,
                    tx,
                    kick,
                },
            );
            self.note_who_is_connected(&claims);
        }
        Ok(kicked)
    }

    /// Write the claims map down for `projects --json`, which runs in another process.
    ///
    /// Called with the claims lock HELD, deliberately: two changes racing to write would otherwise
    /// publish whichever finished last, and a snapshot that says a bridge is connected after it
    /// has gone is the one thing the reader's own checks cannot catch. A failed write is logged
    /// and nothing else — the map in memory is the truth and delivery does not depend on this.
    fn note_who_is_connected(&self, claims: &BTreeMap<Addr, Claim>) {
        if let Err(e) = self.presence.write(claims.keys()) {
            tracing::error!(
                error = %e, path = %self.presence.path().display(),
                "could not write down who is connected; `projects --json` will be stale"
            );
        }
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
        let kicked: Vec<Addr> = {
            let mut claims = self.claims.lock().await;
            let mut kicked = Vec::new();
            claims.retain(|addr, claim| {
                if !off.contains(&addr.project) {
                    return true;
                }
                // The connection is told THROUGH its kick and removes its own claim on the way out.
                // One that cannot be told — nothing listening on the far end of the kick — is
                // forgotten here instead, so a switched-off project is never shown as connected
                // and never handed his words; that shape is only ever a test's own claim.
                match claim.kick.try_send(RefusedReason::NotEnabled) {
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
                        false
                    }
                }
            });
            self.note_who_is_connected(&claims);
            kicked
        };
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
    pub async fn release(&self, addr: &Addr, pid: u32) {
        let mut claims = self.claims.lock().await;
        if claims.get(addr).is_some_and(|c| c.pid == pid) {
            claims.remove(addr);
            self.note_who_is_connected(&claims);
        }
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
        self.words_down.lock().await.len()
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
    pub async fn relay(
        &self,
        addr: &Addr,
        chat_id: i64,
        user: Option<i64>,
        msg_id: &MsgId,
        text: &str,
        replied_to: Option<&MsgId>,
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
        // Written down BEFORE the frame is on the wire, so the bridge's `ack` — which can arrive
        // the moment it is — always finds the record it names. Taken back if the send fails, so a
        // record never waits for an answer to a frame nothing received.
        let frame = Self::mint_frame_id();
        {
            let mut down = self.words_down.lock().await;
            down.push_back(WordsDown {
                frame: frame.clone(),
                addr: addr.clone(),
                chat_id,
                msg_id: msg_id.clone(),
            });
            while down.len() > WORDS_DOWN_KEPT {
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
                },
            )
            .await;
        if !delivered {
            self.words_down.lock().await.retain(|w| w.frame != frame);
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
            let title = match &addr.lane {
                None => p.title.clone(),
                Some(lane) => crate::registry::lane_title(&p.title, lane),
            };
            // The COLOUR stays the project's. Telegram gives six, and a project with its lanes
            // beneath it reads as one block in the list only if they share one.
            (
                registry.topic_of(addr),
                title,
                p.icon_color,
                p.title.clone(),
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
        // A lane's greeting NAMES the worktree, because it is the first thing in a brand-new topic
        // and twelve of them arrive on a dispatch day. Identical, they are twelve notifications he
        // cannot tell apart, and the only thing left carrying which one is a topic title that a
        // phone row truncates. The name is the same string `/projects` and the title show him.
        let greeting = match &addr.lane {
            None => format!("{project_title} is connected."),
            Some(lane) => format!(
                "{lane} is connected — a separate worktree of {project_title}. It talks here, not \
                 in the project's own topic."
            ),
        };
        // The greeting's outcome is READ, not discarded. A topic that was made, written down, and
        // never greeted is a conversation he cannot find at all: Telegram does not list an empty
        // one. Without this the hub could not tell that state from a healthy topic.
        match self.send_into(addr, id, &greeting, &[], None, until).await {
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
        text: &str,
        buttons: &[AskOption],
        reply_to: Option<&MsgId>,
        until: std::time::Instant,
    ) -> SendOutcome {
        // Clipped here rather than by the surface, because whether anything was lost is a fact the
        // BRIDGE has to be told, and only this side is holding the ack.
        let (text, clamped) = crate::queue::fit(text, crate::queue::MAX_TEXT);

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
            let _ = self.audit.sent(addr, topic_id, text.len());
            let mut outcome = self.surface.send(topic_id, &text, buttons, reply_to).await;
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
        // Minted ONCE, here, and shared by every turn this message has to take. A brand-new
        // conversation queues three times before its first word — the topic, the greeting, then the
        // message — and each of those used to start a deadline of its own.
        let until = std::time::Instant::now() + kind.shelf_life();
        let outcome = self.try_to_say(addr, text, buttons, reply_to, until).await;
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
        text: &str,
        buttons: &[AskOption],
        reply_to: Option<&MsgId>,
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
        let outcome = self
            .send_into(addr, topic_id, text, buttons, reply_to, until)
            .await;

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
                Ok(fresh) => {
                    self.send_into(addr, fresh, text, buttons, reply_to, until)
                        .await
                }
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
            registry.get(&addr.project).map(|p| match &addr.lane {
                None => p.title.clone(),
                Some(lane) => crate::registry::lane_title(&p.title, lane),
            })
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
    pub async fn serve_connection(self: Arc<Self>, stream: UnixStream) -> anyhow::Result<()> {
        // The kernel's own answer, both fields. The pid a bridge puts in its `hello` is a number it
        // chose; this one is a fact about the process on the other end of THIS socket. Gate 4's
        // liveness check runs on it, so a bridge cannot make itself look dead — or make an
        // incumbent look dead — by reporting a pid that is not its own.
        let peer = peer_cred(&stream)?;
        let (rx_half, mut tx_half) = stream.into_split();
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

        let addr = match self
            .admit(peer.uid, our_uid(), &first.payload, first.v)
            .await
        {
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
            ..
        } = first.payload.clone()
        else {
            unreachable!("admit only admits a hello");
        };
        if claimed_pid != peer.pid {
            // Not fatal — a bridge behind a wrapper legitimately does not know its own outermost
            // pid. It IS worth a line, because the audit trail should record which number was
            // believed and which was merely offered.
            tracing::debug!(
                claimed = claimed_pid,
                actual = peer.pid,
                "a bridge reported a pid that is not the one on its socket; using the socket's"
            );
        }
        let pid = peer.pid;

        // The name the REGISTRY holds, composed for the conversation this is — so a lane's own log
        // says the same thing as the topic the operator is looking at.
        let title = {
            let registry = self.registry.lock().await;
            registry
                .get(&addr.project)
                .map(|p| match &addr.lane {
                    None => p.title.clone(),
                    Some(lane) => crate::registry::lane_title(&p.title, lane),
                })
                .unwrap_or_default()
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

        let mut kicked = match self
            .claim(addr.clone(), pid, instance.clone(), tx.clone())
            .await
        {
            Ok(kicked) => kicked,
            Err(reason) => {
                let env = Envelope::new(FrameId::new("h-refused"), HubFrame::Refused { reason });
                let _ = tx.send(env).await;
                // Give the writer a moment to put the refusal on the wire before the task is
                // dropped; a refusal nobody receives is the same as the silent takeover this
                // replaced.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                writer.abort();
                let _ = self
                    .audit
                    .refused(&addr, "another bridge already holds this conversation");
                return Ok(());
            }
        };

        // Admitted, with no topic yet. See `HubFrame::Welcome` for why that is not an omission.
        let _ = tx
            .send(Envelope::new(
                FrameId::new(format!("h{}", next_frame_seq())),
                HubFrame::Welcome {
                    project: title,
                    // Echoed from the ADMITTED address, never from the wire. It is the only thing
                    // that tells a bridge which named a lane that this hub understood it, rather
                    // than ignoring the word and handing the worktree the project's own place.
                    lane: addr.lane.clone(),
                    topic_id: None,
                    limits: LIMITS,
                },
            ))
            .await;

        // Prove the far end is really there before anything is created for it. The ping's own
        // envelope id is the nonce; a pong naming it is the proof.
        let ping_id = FrameId::new(format!("h{}", next_frame_seq()));
        let _ = tx
            .send(Envelope::new(ping_id.clone(), HubFrame::Ping))
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
                        Some(_) => Settled::SwitchedOff,
                        // The claim was taken from under this connection: its process is gone
                        // from /proc and a successor evicted it. Nobody is behind the socket to
                        // tell, and "switched off" would be untrue — it went the way a dead
                        // bridge goes.
                        None => Settled::Gone,
                    },
                };
                match next {
                    Ok(Some(frame)) => {
                        if let BridgeFrame::Pong { r#ref } = &frame.payload
                            && r#ref == &ping_id
                        {
                            return Settled::Live;
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
            let why = match settled {
                Settled::Overflowed => "sent more before answering than the hub will hold for it",
                Settled::Oversize => "a frame was over the size ceiling",
                Settled::SwitchedOff => "its project was switched off at the terminal",
                Settled::Gone | Settled::Live => {
                    "connected but never answered; it is probably not allowed to talk to me"
                }
            };
            tracing::warn!(
                project = %addr.project, lane = addr.lane_field(), why,
                "a bridge did not become live"
            );
            let _ = self.audit.refused(&addr, why);
            // Released FIRST, as the oversize path below does: what follows is writing, and a
            // bridge that redials in a second must not find its own dead connection still holding
            // the address.
            self.release(&addr, pid).await;
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
                    );
                    if tx.send(env).await.is_err() {
                        break;
                    }
                }
                // The two refusals with a reason the bridge can branch on are told it. The
                // closed set has none for "never answered", so that one stays a close.
                let reason = match settled {
                    Settled::Oversize => Some(RefusedReason::FrameTooLarge),
                    Settled::SwitchedOff => Some(RefusedReason::NotEnabled),
                    _ => None,
                };
                if let Some(reason) = reason {
                    let _ = tx
                        .send(Envelope::new(
                            FrameId::new(format!("h{}", next_frame_seq())),
                            HubFrame::Refused { reason },
                        ))
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
            tokio::spawn(async move {
                while let Some(frame) = frames_rx.recv().await {
                    let ack_ref = frame.id.clone();
                    let (delivered, why) = if switch.is_off() {
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
                        .send(Envelope::new(
                            FrameId::new(format!("h{}", next_frame_seq())),
                            HubFrame::Ack {
                                r#ref: ack_ref,
                                delivered,
                                why,
                            },
                        ))
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
                    Some(reason) => Some(reason),
                },
            };
            if let Some(reason) = kicked_with {
                self.end_switched_off(
                    &addr,
                    pid,
                    reason,
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
                    let Some(reason) = kick else { break };
                    self.end_switched_off(
                        &addr,
                        pid,
                        reason,
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
                        .send(Envelope::new(
                            FrameId::new(format!("h{}", next_frame_seq())),
                            HubFrame::Refused {
                                reason: RefusedReason::FrameTooLarge,
                            },
                        ))
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
                    self.release(&addr, pid).await;
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
                            Some(reason) => Some((reason, frame)),
                        },
                    };
                    if let Some((reason, frame)) = kicked_with {
                        self.end_switched_off(
                            &addr,
                            pid,
                            reason,
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
        self.release(&addr, pid).await;
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

    /// End a live connection whose project was switched off at the terminal.
    ///
    /// Everything read from the bridge is answered for before the socket ends, in this order:
    /// `refused{not_enabled}` first, so the bridge reads every `no` after it in that light and
    /// stops promising its agent anything; then `no` for each frame in hand — read off the socket
    /// and not yet queued; then, from the handler, `no` for everything queued behind the frame it
    /// was inside, and for that frame too if it was still waiting for its turn (see
    /// `ConnectionSwitch`). A frame already inside a send is finished, because cancelling a
    /// Telegram call mid-flight lands a message whose ack says it did not land. Then the close,
    /// bounded like every other goodbye.
    #[allow(clippy::too_many_arguments)]
    async fn end_switched_off(
        &self,
        addr: &Addr,
        pid: u32,
        reason: RefusedReason,
        in_hand: Vec<Envelope<BridgeFrame>>,
        switch: &ConnectionSwitch,
        tx: mpsc::Sender<Envelope<HubFrame>>,
        frames_tx: mpsc::Sender<Envelope<BridgeFrame>>,
        mut handler: tokio::task::JoinHandle<()>,
        mut writer: tokio::task::JoinHandle<()>,
    ) {
        tracing::info!(
            project = %addr.project, lane = addr.lane_field(), ?reason,
            "ending a live connection: its project was switched off at the terminal"
        );
        // Released FIRST, so `/projects` stops calling it connected and his typed words stop
        // reaching it the instant the switch is thrown, before any goodbye.
        self.release(addr, pid).await;
        // Told why BEFORE the switch is thrown inside this connection, so the refusal is on the
        // wire ahead of every `no` the switch causes. Bounded: a bridge that has stopped reading
        // has a full outbox, and the handler is already parked on it — nothing more can post.
        let _ = tokio::time::timeout(
            GOODBYE_SHELF_LIFE,
            tx.send(Envelope::new(
                FrameId::new(format!("h{}", next_frame_seq())),
                HubFrame::Refused { reason },
            )),
        )
        .await;
        switch.throw();
        let _ = self
            .audit
            .refused(addr, "its project was switched off at the terminal");
        for frame in in_hand {
            let env = Envelope::new(
                FrameId::new(format!("h{}", next_frame_seq())),
                HubFrame::Ack {
                    r#ref: frame.id,
                    delivered: Delivered::No,
                    why: None,
                },
            );
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
            BridgeFrame::Say { text, .. } | BridgeFrame::Done { text } => {
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
            // is waiting on an answer for are his typed words; everything else about it is
            // bookkeeping. Acked like any frame, so "every frame gets exactly one" stays true.
            BridgeFrame::Ack {
                r#ref,
                status,
                reason,
            } => {
                self.what_became_of_his_words(addr, &r#ref, status, reason.as_deref())
                    .await;
                (Delivered::Yes, None)
            }
            // Liveness and bookkeeping. Acked so that "every frame gets exactly one" stays true
            // without exception, which is what makes a missing ack mean something.
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
    /// his topic under the hub's name by refusing things it was never sent. And the FIRST answer
    /// for a message is the one that counts: the record goes with it, so a second cannot write a
    /// second line. Behind attach's door several producers may answer one message, and the door
    /// folds them into one before this hub hears it; the Claude tool server answers `accepted`
    /// the moment it has handed the words into the agent's turn.
    async fn what_became_of_his_words(
        &self,
        addr: &Addr,
        frame: &FrameId,
        status: AckStatus,
        reason: Option<&str>,
    ) {
        let his = {
            let mut down = self.words_down.lock().await;
            let Some(at) = down
                .iter()
                .position(|w| &w.frame == frame && &w.addr == addr)
            else {
                return;
            };
            down.remove(at)
        };
        let Some(his) = his else { return };
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

    async fn say_and_ack(
        &self,
        addr: &Addr,
        text: &str,
        options: &[AskOption],
    ) -> (Delivered, Option<hub_proto::AckWhy>) {
        let outcome = self.say(addr, text, options).await;
        self.ack_for(&outcome)
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
        let retired = self
            .surface
            .retire_buttons(
                record.topic_id,
                msg_id,
                &record.text,
                &format!("answered from your phone — {label}"),
            )
            .await;
        // The record is forgotten only when the buttons are actually gone. Forgetting first left a
        // live keyboard with nothing behind it: still tappable, still offering a choice already
        // made, and the next tap answered "I have no record of that question". Fail closed — the
        // order is "the buttons are gone, therefore the record may go", never the reverse.
        match retired {
            Ok(()) => {
                let _ = self.ledger.lock().await.forget(chat_id, msg_id);
            }
            Err(e) => tracing::error!(
                error = %e, project = %record.project, lane = record.addr().lane_field(),
                "answered from the phone but the keyboard is still there; leaving the record so it \
                 can be retired later"
            ),
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
        let record = { self.ledger.lock().await.get(chat_id, msg_id).cloned() };
        let Some(record) = record else {
            return Withdrawal::NothingLeftToTakeBack;
        };
        match self
            .surface
            .retire_buttons(
                record.topic_id,
                msg_id,
                &record.text,
                // Written into the message he is looking at, in whichever topic that is. A
                // worktree's topic and its project's sit side by side, and the project may be
                // perfectly reachable while the thing that asked this is not.
                "not sent — nothing here could be reached",
            )
            .await
        {
            Ok(()) => {
                let _ = self.ledger.lock().await.forget(chat_id, msg_id);
                Withdrawal::Retired
            }
            Err(e) => {
                tracing::error!(
                    error = %e, project = %record.project, lane = record.addr().lane_field(),
                    "a tap reached nobody and its keyboard is still on his phone"
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
            (hub_proto::AskEnd::Withdrawn, _) => "no longer being asked".to_owned(),
            (hub_proto::AskEnd::Timeout, _) => "timed out".to_owned(),
        };
        let targets = {
            self.ledger
                .lock()
                .await
                .messages_for(addr, instance, ask_id)
        };
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
                    self.surface
                        .retire_buttons(record.topic_id, &msg, &record.text, note)
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
                        "a question stopped being asked but its keyboard is still there"
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
/// How the settling window ended: the one way in, and the three ways a connection is refused
/// without ever having been live. Each of the three is said differently, and each answers for
/// whatever was buffered before the socket goes.
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
    /// Its project was switched off at the terminal while it was still settling. Nothing is made
    /// for it: a topic created and greeted for a project he had just turned off would be the switch
    /// producing the one thing it exists to stop.
    SwitchedOff,
}

/// What a frame costs against the pre-pong hold: its text plus an allowance for the envelope, and
/// never more than one frame can be on the wire. The codec refuses anything past
/// [`hub_proto::MAX_FRAME_BYTES`] before it gets here, so [`PRE_PONG_FRAMES`] of the largest frames
/// possible cost at most [`PRE_PONG_BYTES`] — the count and the byte bound say the same thing,
/// rather than the bytes tripping a frame early on the last of a legal backlog.
fn frame_cost(frame: &BridgeFrame) -> usize {
    let cost = match frame {
        BridgeFrame::Say { text, .. }
        | BridgeFrame::Done { text }
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

/// Per-process frame counter. Opaque and monotonic is all the protocol asks for.
fn next_frame_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Who is on the other end of a connection, according to the kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerCred {
    pub uid: u32,
    /// The process on the other end of THIS socket. Not the pid the bridge says it is: gate 4's
    /// liveness check runs on this one, so a wrong number here would let a bridge make an incumbent
    /// look dead and take its project.
    pub pid: u32,
}

/// Read the credentials of whoever is on the other end of a connection.
pub fn peer_cred(stream: &UnixStream) -> std::io::Result<PeerCred> {
    let cred = rustix::net::sockopt::socket_peercred(stream)?;
    Ok(PeerCred {
        uid: cred.uid.as_raw(),
        pid: cred.pid.as_raw_nonzero().get() as u32,
    })
}

/// This process's uid, for comparison against the peer's.
pub fn our_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

#[cfg(test)]
mod tests;

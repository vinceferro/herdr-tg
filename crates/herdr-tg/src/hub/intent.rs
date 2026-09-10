//! The intentions this hub has carried, and the state machine that decides what may be said
//! about one.
//!
//! **Part relocation, part new, and the split is worth knowing.** Roughly the first half of this
//! is the intent band lifted out of `hub.rs` — a file already past eight thousand lines — and the
//! rest (the ledger on disk, the phase a disconnect keeps, the two ways one word can already be
//! written down) was written for the review that followed. Every line of it answers one question —
//! what the hub knows about an intention — which is the seam a module is for. What deliberately
//! did NOT come with it is the three methods of [`super::Hub`] that reach the wire (`intend`,
//! `hand_to_the_run`, `what_became_of_an_intention`) and the audit lines beside them: they own the
//! claims lock and the one lock order in this crate is only readable while those and the claims
//! map are in one file.

use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use hub_proto::frame::{Control, IntentStatus, Op, control_for};
use hub_proto::ids::{IdempotencyKey, IntentId, LaneId, MsgId, OptionId, ProjectId, SpecId};
use serde::{Deserialize, Serialize};

use super::{Addr, now_millis};

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Carrying one intention to the controller that said it could do the thing.
//
// The whole of what the hub knows about this: a controller declared, in advance, on a connection
// that had already proved a secret, that it can be asked to do a named thing to a named handle. The
// operator picked one of those things. The hub carries the pick, fenced, idempotent and written
// down — and carries nothing that could name an image, a command, a mount, a secret, an environment
// variable or a host path, because the frame has no field for one.
//
// What the hub does NOT do here, and each is a line rather than an omission: it does not resolve a
// spec (it holds no table and no way to get one), it does not schedule, it does not start or stop
// anything, and it does not infer a workload's state from a connection's.
//
// # Why half of this carries `#[allow(dead_code)]`
//
// **The keyboard is a later slice.** Reading an outcome is wired into the shipped binary — a
// controller can declare, be admitted, be echoed, and answer — but nothing in the binary CALLS
// `Hub::intend` yet, because the only thing that ever should is a tap on buttons the hub drew, and
// how those are drawn needs decisions nobody has made. So the outbound half is exercised by tests
// and by nothing else, and it is marked rather than deleted or hidden behind `cfg(test)`: this is
// the contract another org builds against, it has to ship, and an `allow` a reviewer can see is
// more honest than code that quietly is not in the binary. Every one of these disappears on the
// day one call site draws the keyboard.
//
// The marked items are exactly the outbound half — `Wanted`, `Intended`, `IntentRefusal`,
// `Hub::intend` and what only they reach. Nothing on the INBOUND path is marked, and if a later
// change makes something there unreachable, the gate says so rather than this note covering it.

/// How many intentions the hub keeps a record of before the oldest is forgotten.
///
/// [`super::DOWN_KEPT`]'s sibling, and the same sentence: a controller that never answers must not turn a
/// record nobody will read into a leak.
///
/// What is forgotten to make room is the oldest record that has been ANSWERED FOR, and after that
/// the oldest one the hub knows nothing whatever about — never one whose controller said the work
/// had started, and never one still waiting on a live connection. Forgetting by age alone made the
/// bound a way to carry one button twice: past it a repeat was carried afresh instead of answered
/// from the record, and `start` and `inspect` carry no run, so the fence that bounds every other op
/// does not bound those two at all. When there is nothing of either kind, an intention is refused
/// rather than carried — a refusal is something the operator can read, and a second restart is not.
/// [`IntentLedger::room_for_one_more`] holds the order and what it costs.
///
/// What the bound still costs: a very old outcome, about an intention two hundred and fifty-six
/// finished ones ago, reads as one this hub never sent.
// No shipped caller until the keyboard is drawn; see the section note at the head of this file.
#[allow(dead_code)]
pub(crate) const INTENTS_KEPT: usize = 256;

/// How long the hub is willing for an intention to be acted on, told to the controller as a
/// DURATION rather than an instant — the two ends of this wire are two processes that have never
/// agreed on a clock.
///
/// The hub does not enforce it and deliberately does not expire its own record at it. This is the
/// controller's deadline to act; the record's job is to recognise a repeat, and a record dropped at
/// the deadline would let a late duplicate be carried out a second time, which is the one thing the
/// idempotency key exists to prevent.
// No shipped caller until the keyboard is drawn; see the section note at the head of this file.
#[allow(dead_code)]
pub const INTENT_VALID_FOR: Duration = Duration::from_secs(120);

/// Where one intention has got to, as the hub OBSERVED it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentState {
    /// Handed to the controller's connection, and nothing has been said about it.
    // No shipped caller until the keyboard is drawn; see the section note at the head of this file.
    #[allow(dead_code)]
    Sent,
    /// The controller said the work had started, and owes exactly one terminal word.
    Accepted,
    /// The controller said what became of it. Nothing follows a terminal word.
    Settled(IntentStatus),
    /// The connection ended with the word still owed, and **what the controller had already said
    /// is kept**.
    ///
    /// **Not a failure, and deliberately not a word on the wire.** It says what became of the
    /// CONVERSATION and nothing at all about the work: reading a disconnect as a failure is the
    /// error `Delivered::Unseen` was written to prevent one wire over, and a controller that could
    /// SAY this would have the hub repeat a rung it never observed. A controller answering late
    /// from its own record still corrects it.
    ///
    /// It carries the phase because a disconnect that forgot it was a way ROUND the state
    /// machine: with `Sent` and `Accepted` collapsed into one word, `accepted` then a dropped
    /// socket then `refused` was a legal path, and `refused` means nothing happened about work
    /// the controller had said was under way. Which is the worst thing this system can tell him,
    /// because his next action is chosen on it.
    Unknown { after: Phase },
}

/// How far an intention had got when the connection carrying it ended.
///
/// Two, and only ever these two: a settled intention needs no disconnect state, and the hub
/// observes no rung between "handed over" and "the controller said it had started".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Handed over, and the controller never said anything about it.
    // No shipped caller until the keyboard is drawn; see the section note at the head of this file.
    #[allow(dead_code)]
    Sent,
    /// The controller had said the work was under way.
    Accepted,
}

/// One intention the hub carried, and what it is waiting to hear about it.
///
/// Written down, and [`IntentLedger`] holds the whole of what survives a restart and what does
/// not. One field does not: see [`Intent::said`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Intent {
    pub(crate) id: IntentId,
    /// What makes a repeat safe. Minted by the hub from what he was looking at when he tapped.
    pub(crate) key: IdempotencyKey,
    /// The claim it was handed to: the connection that DECLARED it, looked up when the intention
    /// was built and never re-derived. An outcome arriving anywhere else is not about this.
    pub(crate) to: Addr,
    /// WHICH RUN of that address declared it — read in the same critical section as the
    /// declaration and the fence, and carried through the hand-over.
    ///
    /// An address is not a run. Between the fence reading the claims map and the frame going out,
    /// the wall can restart: without this the intention lands on whatever holds the address by
    /// then, stamped with the successor's own lease so nothing downstream can tell — which is
    /// verbatim the failure `for_lease` exists to prevent, arriving one moment later than it
    /// looks. It is also what says who may settle it while the conversation is still going.
    pub(crate) run: u64,
    /// What it is ABOUT: the address whose lease `for_lease` named. The same as `to` when the
    /// control named no domain. Never used for routing; routing reads `to`.
    pub(crate) about: Addr,
    pub(crate) op: Op,
    pub(crate) spec: SpecId,
    pub(crate) count: Option<u32>,
    /// The hub's own delivery lease for [`Intent::about`], as it stood when the keyboard was
    /// drawn. Never a number about the WORK — see [`hub_proto::HubFrame::Intent::for_lease`].
    pub(crate) for_lease: Option<u64>,
    /// Who tapped, for the audit line. The allowlist decision was made long before this exists.
    pub(crate) user: i64,
    pub(crate) state: IntentState,
    /// The controller's own sentence about it, when it sent one — clamped to what a topic can
    /// carry, at the moment it is kept.
    ///
    /// Kept for the slice that draws this on his phone, and **never written to the audit**: that
    /// file is one record per line and a sentence from another process can carry a newline, which
    /// would be a second record of the sender's choosing. Clamped where it is kept rather than
    /// where it is drawn, because this is the one string in the family whose length nobody on this
    /// side chose and the surface must not be the first place that is noticed.
    ///
    /// **The one field that never reaches the disk**, and the only `skip` in this record: it is
    /// another process's prose, with a retention question attached, and nothing decides anything
    /// on it. [`IntentLedger`] names what that costs.
    #[serde(skip)]
    pub(crate) said: Option<String>,
    /// The digest of that sentence, which DOES reach the disk. See [`digest_of`].
    ///
    /// Kept beside the prose rather than derived from it at each comparison, because after a
    /// restart there is no prose to derive it from — which is the whole point of it. Nothing is
    /// drawn from this and nothing is decided on it except whether two frames are one observation.
    #[serde(default)]
    pub(crate) said_digest: Option<u64>,
}

/// One thing the operator picked off a keyboard the hub drew.
///
/// Every field is either something he SELECTED from what a controller declared, or something this
/// hub minted. None of it is anything he could type: the op is a variant of a closed set, and the
/// spec and the domain are strings a controller put on a connection that had already proved a
/// secret. That is the oldest line in this repo — inbound content selects, it never names.
#[derive(Clone, Debug)]
// No shipped caller until the keyboard is drawn; see the section note at the head of this file.
#[allow(dead_code)]
pub struct Wanted {
    /// Whose conversation the keyboard was drawn in. Never from a frame.
    pub project: ProjectId,
    pub op: Op,
    pub spec: SpecId,
    /// Which conversation of the project it is about. Absence is the only spelling of "the
    /// conversation itself"; `-` is not a domain and never reaches a claim.
    pub domain: Option<LaneId>,
    pub count: Option<u32>,
    /// The hub's own delivery lease for the conversation the keyboard was drawn about — never a
    /// number about the work. See [`hub_proto::HubFrame::Intent::for_lease`] for the four axes and
    /// which one this is.
    pub for_lease: Option<u64>,
    /// The message the keyboard is on, and the button he pressed. Two of the three things the
    /// idempotency key is minted from.
    pub offer: (i64, MsgId),
    pub option_id: OptionId,
    /// Who tapped, and where. The same type and spelling the hub already puts on every `message`.
    pub from: hub_proto::From,
}

/// What became of an intention the hub was asked to carry.
#[derive(Clone, Debug, PartialEq, Eq)]
// No shipped caller until the keyboard is drawn; see the section note at the head of this file.
#[allow(dead_code)]
pub enum Intended {
    /// Handed to the controller that declared it, under this id.
    Carried(IntentId),
    /// This key has been carried before AND something is known about what became of it. What the
    /// record says now, and **no second frame** — which is what makes at-least-once delivery
    /// survivable without anybody claiming exactly-once.
    ///
    /// Never [`IntentState::Unknown`] with [`Phase::Sent`]: about that one the hub knows nothing
    /// at all, and answering a fresh tap from it would be claiming knowledge it has not got. Such
    /// a button is offered again instead.
    ///
    /// [`Phase::Accepted`] IS answered from, and the difference is the whole reason the phase is
    /// kept: there the hub knows the controller said the work had begun. Carrying it again would
    /// be asking for a second run of an operation known to have started — the duplicate the key
    /// exists to prevent — where what the operator is owed is the honest state, which is that it
    /// started and the connection carrying it ended before anything said how it finished.
    AlreadyAsked(IntentId, IntentState),
}

/// Why an intention was not carried. Every one of them is fail-closed: nothing was handed on.
///
/// There is deliberately no `say()` here yet. The keyboard these refusals belong under is a later
/// slice and the sentences are its decision to make; what this slice owes the operator is the audit
/// line and the journal, which is what [`IntentRefusal::written_down`] is for. A sentence invented
/// now would be one written against a surface nobody has designed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// No shipped caller until the keyboard is drawn; see the section note at the head of this file.
#[allow(dead_code)]
pub enum IntentRefusal {
    /// The person named on the tap is on no list that covers that conversation.
    ///
    /// Asked at this door and not left to the caller, because a second caller is a second way
    /// around the first — the sentence `resolve_tap` already has over its own allowlist check.
    Stranger,
    /// The spec or the domain is not a handle this hub will write down.
    ///
    /// The same shape check every admitted control passes, applied to what a caller in this
    /// process built, because every refusal below writes a line before anything has compared the
    /// handles against a declaration — and a handle carrying a newline writes a record of the
    /// caller's own choosing into the one file an incident is read from.
    HandleRefused,
    /// Nothing at all is connected in that conversation, so there is nobody to ask.
    NotConnected,
    /// Something is connected and none of it said it could do this. Read from the claim, never
    /// remembered anywhere else.
    NotDeclared,
    /// Two live connections of one project each said they could do it. Refused rather than
    /// disambiguated: picking the newer would be the hub guessing which he meant.
    MoreThanOneCouldDoIt,
    /// The lease he was shown for that conversation is not the lease it is held under now.
    ///
    /// About the CONNECTION and never about the work: the wall the operator is watching may not
    /// have moved at all. See [`hub_proto::HubFrame::Intent::for_lease`].
    TheWorldMoved,
    /// Nothing is connected in the conversation the operation is about, so there is no lease for
    /// `for_lease` to have named. Its own refusal, and never a comparison against the zero
    /// `highest_for` returns for an address that was never claimed — a zero is read as "holds
    /// none" everywhere else on this wire.
    ///
    /// It says nothing about whether a workload is running there. This hub holds no table that
    /// could answer that, and a refusal that claimed to would be read as one by whoever acts on
    /// the audit.
    NothingIsConnectedThere,
    /// The count is past the bound the controller set on itself, is on an op that has no number in
    /// it, or is missing from the one that does.
    CountRefused,
    /// The hub refusing ITSELF: a `for_lease` on an op that has no conversation to be about, or
    /// none on an op that changes something already connected. A number that means nothing is how
    /// a check becomes decorative.
    CouldNotBuildIt,
    /// The controller's outbox would not take it. Nothing was carried, and the key is free again.
    CouldNotHandItOn,
    /// The connection that declared it ended between the fence and the hand-over.
    ///
    /// Its own refusal rather than a silent hand-over to whoever holds the address now: the
    /// successor never declared this, and the run the operator's view was built against is gone.
    TheControllerWentAway,
    /// There are already as many intentions waiting for a word as this hub will keep.
    ///
    /// Fail closed: forgetting one nobody has answered for to make room is how one button gets
    /// carried out twice. A refusal he can read is the honest end of a controller that has stopped
    /// answering.
    TooManyWaiting,
}

impl IntentRefusal {
    /// What the journal and the audit say happened. **Never operator-facing** — the same rule
    /// `Kick::sentence` holds to.
    // No shipped caller until the keyboard is drawn; see the section note at the head of this file.
    #[allow(dead_code)]
    pub(crate) fn written_down(self) -> &'static str {
        match self {
            Self::Stranger => "the person who tapped may not speak in that conversation",
            Self::HandleRefused => "the spec or the domain is not one this hub will write down",
            Self::NotConnected => "nothing was connected in that conversation",
            Self::NotDeclared => "nothing connected there had said it could do that",
            Self::MoreThanOneCouldDoIt => "more than one connection said it could do that",
            Self::TheWorldMoved => {
                "the run of the connection it was built against is not the run holding that \
                 conversation now"
            }
            Self::NothingIsConnectedThere => {
                "nothing is connected at the conversation that operation is about"
            }
            Self::CountRefused => "the count is not one the controller said it would take",
            Self::CouldNotBuildIt => "the operation and the run it named do not go together",
            Self::CouldNotHandItOn => "the controller was not keeping up and did not take it",
            Self::TheControllerWentAway => {
                "the connection that said it could do that ended before it was handed on"
            }
            Self::TooManyWaiting => "as many intentions are waiting for a word as this hub keeps",
        }
    }
}

/// Does this op change something that is already running?
///
/// The whole of the `for_lease` rule, in one place so the frame builder and the refusal cannot
/// come to disagree. `start` may name a conversation nothing is connected to yet and `inspect`
/// changes nothing, so a lease on either would be a number that means nothing — and a check on a
/// meaningless number is a check that gets deleted. The other four each change something in a
/// conversation that is connected now, and *which run of it* is the entire question.
///
/// **The hole this leaves, named rather than left to be found.** Those two ops carry no fence at
/// all, so nothing but the idempotency ledger stands between a repeated tap and a second `start`.
/// That is why the ledger is written down — see [`IntentLedger`].
// No shipped caller until the keyboard is drawn; see the section note at the head of this file.
#[allow(dead_code)]
pub(crate) fn changes_a_running_thing(op: Op) -> bool {
    match op {
        Op::Start | Op::Inspect => false,
        Op::Scale | Op::Drain | Op::Stop | Op::Restart => true,
    }
}

/// Which word may follow which, and the state it leaves behind.
///
/// `accepted` says the work has STARTED and owes exactly one terminal word; a terminal word may
/// also arrive first, from a controller that never accepted. Nothing follows a terminal word: a
/// controller that could keep re-answering could turn one intention into a stream of them, and the
/// last one to arrive would silently be the truth.
///
/// `refused` after `accepted` is not a transition either, and the reason is the whole point of
/// having four words: `refused` means NOTHING happened, and by then something had.
///
/// [`IntentState::Unknown`] takes the words that COMPLETE what was already said, because it is not
/// a state of the WORK — it says the connection ended with the word still owed, and a controller
/// answering late from its own record is exactly the thing that can correct it. But it is not a
/// blank slate: an intention the controller had already accepted keeps `refused` out after the
/// disconnect exactly as it did before, because the disconnect changed what the hub knows about
/// the CONVERSATION and nothing about what was said about the work. Without that the state machine
/// had a way round itself — accept, drop the socket, refuse — and nothing in the audit would look
/// wrong.
///
/// `accepted` is kept out of that state for the same reason it is kept out of
/// [`IntentState::Accepted`], and the disconnect changes nothing about it: the hub was told the
/// work had begun, and a controller replaying its last word after it reconnects — which is the one
/// thing the late-correction rule invites — is saying that same thing again rather than saying
/// something new. Taken as a transition it moved the record back into a live state under a run
/// that is gone by definition, where nothing could ever settle it, forget it, or stop it answering
/// every later tap "still running".
fn may_follow(from: &IntentState, status: IntentStatus) -> Option<IntentState> {
    match (from, status) {
        (IntentState::Settled(_), _) => None,
        (IntentState::Accepted, IntentStatus::Accepted | IntentStatus::Refused) => None,
        (
            IntentState::Unknown {
                after: Phase::Accepted,
            },
            IntentStatus::Refused | IntentStatus::Accepted,
        ) => None,
        (_, IntentStatus::Accepted) => Some(IntentState::Accepted),
        (_, terminal) => Some(IntentState::Settled(terminal)),
    }
}

/// Is this word the one the record already holds — the same observation, arriving twice?
///
/// Asked only where [`may_follow`] said no word may follow, and it splits that one answer in two.
/// A repeat and a contradiction are not the same event: the first is a controller replaying what
/// it already sent, which is what an at-least-once wire invites when an `ack` dies on a socket
/// that ended; the second is a controller saying `completed` and then `failed` about one piece of
/// work. Answering both with a bare "your frame died" told a correct controller nothing it could
/// act on, in the same words a broken one gets.
///
/// The sentence counts as well as the status, and it is compared AFTER the clamp for the reason
/// its caller gives: what the record holds is what `queue::fit` made of it, and nothing else was
/// kept. So a controller that replays a word with a different sentence attached has said
/// something different, which is the fail-closed reading. What is compared is the DIGEST of it and
/// not the prose — see [`digest_of`], which is what makes that comparison survive a restart.
fn is_the_word_already_written_down(
    record: &Intent,
    status: IntentStatus,
    said: &Option<String>,
) -> bool {
    let the_same_word = match (&record.state, status) {
        (IntentState::Settled(written), arriving) => *written == arriving,
        // `accepted` is not terminal, but a second one may not follow it either — and a repeat of
        // it is the same observation for the same reason a repeated terminal word is.
        (IntentState::Accepted, IntentStatus::Accepted) => true,
        // Its twin across a disconnect. `Unknown { after: Accepted }` says the controller told
        // this hub the work had begun, so a controller replaying that word after it reconnects is
        // the same observation arriving twice — not a new one, and not a contradiction.
        (
            IntentState::Unknown {
                after: Phase::Accepted,
            },
            IntentStatus::Accepted,
        ) => true,
        _ => false,
    };
    the_same_word && record.said_digest == digest_of(said)
}

/// A fixed-shape stand-in for the controller's own sentence, kept so that "the same word again"
/// can still be answered after a restart.
///
/// The prose itself never reaches the disk and that has not changed — see [`Intent::said`]. What
/// changed is what the comparison reads: comparing an arriving sentence against the `None` a
/// restart leaves behind answered a correct controller replaying its terminal word with "you said
/// something different", and wrote that accusation into the one file an incident is read from.
///
/// Not a secret and nothing needs it to be. A controller that found a colliding sentence would
/// have its second word read as a repeat rather than a contradiction, and both of those leave the
/// first word standing and the record where it is.
fn digest_of(said: &Option<String>) -> Option<u64> {
    said.as_ref().map(|s| fnv1a(FNV_OFFSET, s.as_bytes()))
}

/// May the connection now holding the address say what became of this intention?
///
/// Two cases, and they are not the same question. While the intention is still live, the answer is
/// the RUN it was handed to and nothing else: the address it went to can be held by a successor
/// within a second of the original leaving, and a successor speaking for its predecessor's work is
/// the hub believing a run about work it never took.
///
/// Once the connection has ended, the hub has written `Unknown` — and a controller answering LATE,
/// from its own record after its own restart, is the one thing that can correct that. So a late
/// word is taken from any run that DECLARED the same thing, and from nothing else: an ordinary
/// bridge holding the address declared nothing, and the id counter is one anybody on the wire can
/// follow, so without this a plain connection could turn an honest "I do not know" into "Done" on
/// the operator's phone.
///
/// A late word that puts the record back into a LIVE state moves this question with it: see
/// [`IntentLedger::a_word_about`], which re-homes the record to the run that spoke. Without that,
/// the record went back to waiting on a run that had already gone, where the first branch here
/// could never be satisfied by anybody again.
///
/// **Not the instance, and that is not an oversight.** The obvious tightening is to require the
/// late word to come from the same `Claim::instance` the intention was handed to. It would break
/// the only case this branch exists for: `docs/ATTACHING.md` §3b tells every adapter to mint an
/// instance once per process and **change it when it restarts**, so a controller answering from
/// its own record after its own restart is by definition a DIFFERENT instance. The fence that
/// closed the reachable hole here is the run-scoped sweep — see
/// [`super::Hub::nothing_more_will_be_said_about`] — which stops a live intention of one run ever being
/// written off by another's departure. What is left is deliberate: a late word is taken from a
/// connection that proved this project's secret AND declared this same operation, and from
/// nothing else.
///
/// `None` for the claim — nothing is connected at that address at all — is refused like everything
/// else this file cannot prove.
fn may_speak_about(record: &Intent, speaking: Option<&(u64, Option<Vec<Control>>)>) -> bool {
    let Some((generation, controls)) = speaking else {
        return false;
    };
    match record.state {
        IntentState::Sent | IntentState::Accepted => *generation == record.run,
        IntentState::Unknown { .. } | IntentState::Settled(_) => control_for(
            controls,
            &record.spec,
            record.about.lane.as_ref(),
            record.op,
        )
        .is_some(),
    }
}

/// The key that makes a repeat safe, minted by the HUB from what he was looking at when he tapped.
///
/// Three things: the message the keyboard is on, the button he pressed, and the run it was drawn
/// against. So two taps on one button are one key — the receipt was slow and he tapped *Restart*
/// again — and the same button drawn again after the address rolled is a different one, because the
/// second is a different operation on a different run and deduplicating it into the first would
/// silently drop it.
///
/// The conversation goes in beside them, and it is belt against braces: one chat numbers its own
/// messages, so two keyboards can already never share a message id. It is here so that the ledger's
/// look-up — which is by key alone, over every conversation at once — cannot be made ambiguous by
/// some later change to what a keyboard is drawn on. One conversation must never be able to answer
/// for another's button.
///
/// Hashed rather than concatenated, and the hash is not decoration: this token is written into the
/// audit, and a token of fixed length over a fixed alphabet needs no second argument about what a
/// message id from Telegram could carry into that file. It is not a secret and nothing about it
/// needs to be — a controller supplies no part of the input, so there is nothing here to collide
/// with on purpose.
// No shipped caller until the keyboard is drawn; see the section note at the head of this file.
#[allow(dead_code)]
pub(crate) fn mint_idempotency_key(
    project: &ProjectId,
    chat_id: i64,
    msg: &MsgId,
    option: &OptionId,
    for_lease: Option<u64>,
) -> IdempotencyKey {
    let mut h: u64 = FNV_OFFSET;
    let mut eat = |bytes: &[u8]| h = fnv1a(h, bytes);
    eat(project.as_str().as_bytes());
    eat(b"\x1f");
    eat(&chat_id.to_be_bytes());
    eat(b"\x1f");
    eat(msg.as_str().as_bytes());
    eat(b"\x1f");
    eat(option.as_str().as_bytes());
    eat(b"\x1f");
    // Spelled apart from any number, so "no run" can never collide with a run that happens to be
    // zero — which is the number this wire already reads as "holds none".
    match for_lease {
        None => eat(b"-"),
        Some(g) => eat(&g.to_be_bytes()),
    }
    IdempotencyKey::new(format!("k{h:016x}"))
}

/// FNV-1a's starting value, and one copy of it: two things in this file need the same fixed shape
/// over a fixed alphabet, and two spellings of one hash is how they come to disagree.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a, spelled out rather than pulled in: the property wanted is a fixed shape over a fixed
/// alphabet, and a dependency for that is a supply-chain decision for a log field.
fn fnv1a(from: u64, bytes: &[u8]) -> u64 {
    let mut h = from;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Where the intentions this hub has carried are written down.
pub const INTENTS_FILE: &str = "hub.intents.json";

/// What one word about an intention turned out to be.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// No intention of this hub's has that id.
    NeverSent,
    /// One does, and it was carried to somebody else.
    NotThisConversation,
    /// One does, in this conversation, and this connection is not the one that may say.
    NotItsToSettle,
    /// The word the hub already holds, said again — one observation arriving twice.
    SameObservationAgain,
    /// A word that is not the one already written down. The first one stands.
    ContradictsWhatItAlreadySaid,
    /// Taken, and the record has moved.
    Took,
}

/// What the ledger already holds about a button that has been tapped before.
#[derive(Debug)]
pub(crate) enum AlreadyCarried {
    /// Something is known about what became of it, and the honest answer is that.
    AnswerFromIt(IntentId, IntentState),
    /// It was carried and NOTHING was ever said about it, so the button is offered again — see
    /// [`super::Hub::intend`], which is the only place that may act on this.
    ///
    /// The record is handed over as a COPY and stays in the ledger: it is given up by
    /// [`IntentLedger::forget`] only once the frame that replaces it is on the wire. Removing it
    /// here meant a repeat the controller's outbox would not take destroyed its predecessor as
    /// well as itself — and the predecessor is an intention a controller may still be acting on,
    /// with a line in the audit pointing at it.
    ///
    /// Boxed because the record is many times the size of the other variant, and this value is
    /// returned on the ordinary path where the answer is `None`.
    NothingWasEverSaidAboutIt(Box<Intent>),
}

/// Every intention this hub has carried, and what it is waiting to hear about each.
///
/// # What survives a restart, and why anything does
///
/// The whole ledger does, minus one field. It used to be memory alone, defended like this: across
/// a restart every claim is gone and every address's generation has moved, so a repeat of a key
/// minted before it is refused by the fence before the key is ever consulted. **That was wrong in
/// two places**, and both are ordinary rather than exotic:
///
/// * `start` and `inspect` carry no fence at all (see [`changes_a_running_thing`]), and every
///   input the key is minted from — the conversation, the chat, the message, the button — comes
///   back identically after a restart. So the only thing standing between a slow receipt and a
///   second `start` of one spec was a record that a restart deleted.
/// * An intention the controller had ACCEPTED is not re-carried, because the hub was told the work
///   began. Forgetting that across a restart turns "he tapped again while it was running" into a
///   second run of an operation known to have started.
///
/// # What does NOT survive, deliberately
///
/// [`Intent::said`] — the controller's own sentence — is `#[serde(skip)]`. It is another process's
/// prose with a retention question attached, it is already kept out of the audit for the same
/// family of reasons, and nothing decides anything on it.
///
/// What DOES survive of it is [`Intent::said_digest`], and that is not a hedge: comparing an
/// arriving sentence against the `None` a restart leaves behind answered a correct controller
/// replaying its terminal word with "you said something different" — and wrote that accusation
/// about it into the one file an incident is read from. The retention answer is unchanged, since a
/// digest is not prose and nothing can be read back out of it; what it buys is that a restart no
/// longer changes what the hub makes of two identical frames.
///
/// # What a load makes of what it reads
///
/// Anything still `Sent` or `Accepted` becomes [`IntentState::Unknown`] of that phase. Nothing is
/// connected in the first moment of a hub's life, so every intention still waiting for a word is
/// waiting on a connection that is gone — which is exactly what that state says. Left as `Sent`,
/// the record would be unsettleable for ever: only the run it was handed to may speak about a live
/// intention, and no run of that address will ever hold that number again.
pub(crate) struct IntentLedger {
    path: PathBuf,
    /// This boot's own token, in front of every id this run mints.
    ///
    /// The counter alone was `next_frame_seq`, which starts at one with the process — so `i7`
    /// named one intention before a restart and a different one after it, and a controller
    /// replaying the outcome it still owed for the first would settle the second. Nothing in the
    /// id, the key or the state could tell them apart. Random rather than a clock: two hubs
    /// started in one millisecond is not a case anybody should have to think about.
    boot: String,
    next: AtomicU64,
    records: VecDeque<Intent>,
}

/// What the file holds. `hub_pid` and `at` are for a person reading it; nothing decides on them —
/// [`super::HandedOut`]'s shape, for the reader who has both files open.
#[derive(Debug, Serialize, Deserialize)]
struct IntentsWrittenDown {
    hub_pid: u32,
    at: u64,
    intents: Vec<Intent>,
}

impl IntentLedger {
    pub(crate) fn load(path: PathBuf) -> Self {
        let mut boot = [0u8; 4];
        // A hub that cannot get four random bytes still gets a token, from the clock. The property
        // wanted is only that two boots of one hub do not share one — nothing here is a secret.
        let boot = if getrandom::fill(&mut boot).is_ok() {
            format!(
                "{:02x}{:02x}{:02x}{:02x}",
                boot[0], boot[1], boot[2], boot[3]
            )
        } else {
            format!("{:08x}", now_millis())
        };
        let records = match fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                // Not fatal and not silent, which is `AskLedger::load`'s rule: what is lost is the
                // hub's memory of buttons it has already carried, and the operator finds that out
                // by tapping one.
                tracing::error!(
                    error = %e, path = %path.display(),
                    "could not read the intentions this hub has carried; a button tapped again \
                     from before the restart will be carried a second time"
                );
                Vec::new()
            }
            Ok(raw) => match serde_json::from_slice::<IntentsWrittenDown>(&raw) {
                Err(e) => {
                    tracing::error!(
                        error = %e, path = %path.display(),
                        "the intentions this hub had carried are not readable; a button tapped \
                         again from before the restart will be carried a second time"
                    );
                    Vec::new()
                }
                Ok(file) => file
                    .intents
                    .into_iter()
                    .map(|mut i| {
                        // Nothing is connected yet, so nothing that was still waiting for a word
                        // is waiting on anybody. See the type's own note.
                        i.state = match i.state {
                            IntentState::Sent => IntentState::Unknown { after: Phase::Sent },
                            IntentState::Accepted => IntentState::Unknown {
                                after: Phase::Accepted,
                            },
                            settled => settled,
                        };
                        i
                    })
                    .collect::<Vec<_>>(),
            },
        };
        // The bound is re-applied on the way IN as well as on the way out: a file that grew — by
        // hand, or by a build with a larger bound — must not make this hub's memory unbounded.
        // What is dropped is the OLDEST, which is the same end the bound takes from while the hub
        // runs: the newest records are the ones whose buttons are still on his phone.
        let over = records.len().saturating_sub(INTENTS_KEPT);
        let records = records.into_iter().skip(over).collect();
        Self {
            path,
            boot,
            next: AtomicU64::new(1),
            records,
        }
    }

    /// An id no other boot of this hub can mint.
    pub(crate) fn mint_an_id(&self) -> IntentId {
        IntentId::new(format!(
            "i{}-{}",
            self.boot,
            self.next.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// What is already known about this button, if anything. Reads only: nothing is given up here.
    pub(crate) fn already_carried(&self, key: &IdempotencyKey) -> Option<AlreadyCarried> {
        let seen = self.records.iter().find(|i| &i.key == key)?;
        if seen.state == (IntentState::Unknown { after: Phase::Sent }) {
            Some(AlreadyCarried::NothingWasEverSaidAboutIt(Box::new(
                seen.clone(),
            )))
        } else {
            Some(AlreadyCarried::AnswerFromIt(
                seen.id.clone(),
                seen.state.clone(),
            ))
        }
    }

    /// Give up the record a repeat has replaced, now that the repeat is on the wire.
    ///
    /// Called only after the hand-over succeeded, which is the whole point of it being its own
    /// step: until then the predecessor is the only record of an intention a controller may still
    /// be acting on, and a repeat that nothing took must leave it exactly where it was.
    pub(crate) fn forget(&mut self, id: &IntentId) {
        self.records.retain(|i| &i.id != id);
        self.save();
    }

    /// Make room for one more, and say whether there was any to make.
    ///
    /// Never at the cost of a record nobody has answered for: what goes is the oldest record that
    /// is already finished with, and when there is none the caller refuses. Forgetting the oldest
    /// regardless of state made the bound a way to carry one button twice.
    ///
    /// **In an order, and not whatever comes first.** A record that has been answered for goes
    /// before one that has not, and among those that have not, the one the hub knows nothing about
    /// goes before the one it was TOLD had started. Taking the first that merely qualified dropped
    /// an operation known to be under way in front of hundreds of finished ones — which is the one
    /// thing writing this ledger down was for, defeated without any restart at all. A record whose
    /// controller said the work had begun is the last thing forgotten and, when it is all there
    /// is, nothing is forgotten and the intention is refused.
    ///
    /// What that costs, named rather than found: a hub whose whole ledger is operations reported
    /// as started and never finished refuses new ones until one of them is answered for. A
    /// refusal is something the operator can read; a second run of something already running is
    /// not.
    pub(crate) fn room_for_one_more(&mut self) -> bool {
        if self.records.len() < INTENTS_KEPT {
            return true;
        }
        let oldest_that_is = |records: &VecDeque<Intent>, what: fn(&IntentState) -> bool| {
            records.iter().position(|i| what(&i.state))
        };
        let forget = oldest_that_is(&self.records, |s| matches!(s, IntentState::Settled(_)))
            .or_else(|| {
                oldest_that_is(&self.records, |s| {
                    matches!(s, IntentState::Unknown { after: Phase::Sent })
                })
            });
        match forget {
            Some(finished) => {
                self.records.remove(finished);
                true
            }
            None => false,
        }
    }

    /// Write one intention down. On the disk before the frame is on the wire, which is the rule
    /// the whole family holds to: an unrecorded intention is one that can be carried again.
    pub(crate) fn write_down(&mut self, record: Intent) {
        self.records.push_back(record);
        self.save();
    }

    /// Take back the last thing written down, because it never reached the wire.
    pub(crate) fn take_the_last_one_back(&mut self) {
        self.records.pop_back();
        self.save();
    }

    /// One word from a controller about one intention.
    ///
    /// The state machine lives here rather than at the call site because every one of its answers
    /// is decided from the record alone; what the caller adds is the line in the audit.
    pub(crate) fn a_word_about(
        &mut self,
        id: &IntentId,
        addr: &Addr,
        status: IntentStatus,
        said: Option<String>,
        speaking: Option<&(u64, Option<Vec<Control>>)>,
    ) -> Heard {
        let Some(record) = self.records.iter_mut().find(|i| &i.id == id) else {
            return Heard::NeverSent;
        };
        if &record.to != addr {
            return Heard::NotThisConversation;
        }
        if !may_speak_about(record, speaking) {
            return Heard::NotItsToSettle;
        }
        match may_follow(&record.state, status) {
            None if is_the_word_already_written_down(record, status, &said) => {
                Heard::SameObservationAgain
            }
            None => Heard::ContradictsWhatItAlreadySaid,
            Some(next) => {
                // A record that is waiting on a connection must name the connection it is waiting
                // on. For a LIVE record this changes nothing — `may_speak_about` has just proved
                // the speaker IS the run it was handed to — but a record the hub had written off
                // as `Unknown` is being moved back into a live state by a run that is not that
                // one, and leaving the old number there left it waiting on a run that is gone by
                // definition: nothing could settle it, its own speaker's departure swept nothing,
                // and the bound could not forget it either.
                if let Some((generation, _)) = speaking {
                    record.run = *generation;
                }
                record.state = next;
                record.said_digest = digest_of(&said);
                record.said = said;
                self.save();
                Heard::Took
            }
        }
    }

    /// A run has left with words still owed. See [`super::Hub::nothing_more_will_be_said_about`], which
    /// is where the reasoning for the scope lives.
    pub(crate) fn nothing_more_from(&mut self, addr: &Addr, run: u64) {
        let mut moved = false;
        for i in self.records.iter_mut() {
            if &i.to == addr && i.run == run {
                // The phase is carried across, never flattened: `refused` may not follow
                // `accepted`, and a state that forgot which one this was would let a disconnect
                // launder exactly that transition.
                i.state = match i.state {
                    IntentState::Sent => IntentState::Unknown { after: Phase::Sent },
                    IntentState::Accepted => IntentState::Unknown {
                        after: Phase::Accepted,
                    },
                    ref settled => settled.clone(),
                };
                moved = true;
            }
        }
        if moved {
            self.save();
        }
    }

    /// Where one intention has got to, as the hub observed it.
    #[cfg(test)]
    pub(crate) fn state_of(&self, id: &IntentId) -> Option<IntentState> {
        self.records
            .iter()
            .find(|i| &i.id == id)
            .map(|i| i.state.clone())
    }

    /// The controller's own sentence about one intention, as the hub kept it.
    #[cfg(test)]
    pub(crate) fn said_about(&self, id: &IntentId) -> Option<String> {
        self.records
            .iter()
            .find(|i| &i.id == id)
            .and_then(|i| i.said.clone())
    }

    /// Temp-and-rename, 0600, the shape [`super::AskLedger::save`] already uses — one hub's state
    /// directory, one way of writing a file in it.
    ///
    /// A failure is said and never propagated: what is on the disk is behind what is in memory,
    /// which costs a duplicate after a restart, and refusing to carry the intention costs the
    /// operator the thing he asked for now.
    fn save(&self) {
        if let Err(e) = self.write_it() {
            tracing::error!(
                error = %e, path = %self.path.display(),
                "could not write down what this hub has carried; a button tapped again after a \
                 restart may be carried a second time"
            );
        }
    }

    fn write_it(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            crate::conversations::private_state_dir(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(&IntentsWrittenDown {
            hub_pid: std::process::id(),
            at: now_millis(),
            intents: self.records.iter().cloned().collect(),
        })?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_a_tap_mints_is_the_same_key_after_a_rebuild_or_a_button_is_carried_twice() {
        // This value is not decoration. The key is written into `hub.intents.json` and a repeat of
        // the same tap is recognised by minting the key AGAIN and comparing it with the persisted
        // one — so the digest is a compatibility surface between two BUILDS, not a detail inside
        // one run. Move a digit of the offset or the prime, or reorder what is eaten, and every
        // button he tapped before the upgrade mints a different key after it, matches no record,
        // and is carried a second time. That is the duplicate this whole mechanism exists to stop,
        // reintroduced by an upgrade rather than a restart, with nothing anywhere going red.
        //
        // The near miss that earned this test: the digest was refactored out of an inline loop
        // while this correction was being written, and the whole suite stayed green. It happened
        // to preserve the value. Nothing would have said so if it had not.
        let key = mint_idempotency_key(
            &ProjectId::new("p-0123456789ab"),
            -1001,
            &MsgId::new("m7"),
            &OptionId::new("y"),
            Some(42),
        );
        assert_eq!(
            key.as_str(),
            "kfd2614817b25e46c",
            "the digest that recognises a repeated tap changed, so every button tapped before this \
             build would be carried a second time after it"
        );

        // And the sentinel really is apart from every number: "no run" must not collide with a run
        // numbered zero, which is the value this wire already reads as holding no lease at all.
        let none = mint_idempotency_key(
            &ProjectId::new("p-0123456789ab"),
            -1001,
            &MsgId::new("m7"),
            &OptionId::new("y"),
            None,
        );
        let zero = mint_idempotency_key(
            &ProjectId::new("p-0123456789ab"),
            -1001,
            &MsgId::new("m7"),
            &OptionId::new("y"),
            Some(0),
        );
        assert_ne!(
            none.as_str(),
            zero.as_str(),
            "a tap against no run and a tap against run zero mint one key, so one of them answers \
             from the other's record"
        );
    }
}

//! The frames, and the envelope every one of them travels in.
//!
//! # Authority flows one way
//!
//! The hub decides where a bridge's words go. A bridge decides nothing about the hub. That is why
//! no frame here carries addressing: there is no `project` field on `say`, no `chat_id` on `ask`,
//! no topic anywhere. The hub knows which connection is which project because it resolved a token
//! at `hello`, and a bridge that tries to name a project is refused.
//!
//! `hello` is the proof: it carries `repo` and `instance` for the audit log and for a human
//! reading it, and it deliberately does NOT carry a display name. The name comes from the
//! registry. A bridge that could name itself could impersonate another project's topic.
//!
//! # Version skew is first-class
//!
//! The hub and the bridge ship from different repositories and are updated on different days, so
//! skew is the normal case rather than the exceptional one. Three rules, and all three are
//! deliberately different:
//!
//! * A major `v` mismatch on `hello` is refused, naming the command that fixes it.
//! * An unknown frame kind is logged and ignored — [`BridgeFrame::Unknown`] exists so that
//!   receiving one is a value this code can hold, not a parse error that kills the connection.
//! * An unknown field inside a known kind is ignored, which is serde's default and is left alone
//!   on purpose: `deny_unknown_fields` here would turn every additive change on the other side
//!   into a dead worker.

use serde::{Deserialize, Serialize};

use crate::ids::{AskId, FrameId, MsgId, OptionId, ProjectId};

/// The protocol version carried in every envelope's `v`.
///
/// A single integer, not a semver triple: the only distinction that changes behaviour is
/// "can these two speak at all", and a second number invites a compatibility matrix nobody
/// maintains.
pub const VERSION: u16 = 1;

/// Every frame, in both directions, is one of these objects on one line.
///
/// `#[serde(flatten)]` on the payload puts `v`, `id` and the payload's own `t` in one flat object,
/// which is what the wire spec says. It also means unknown fields are ignored rather than
/// rejected, which is the version-skew rule above.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope<P> {
    /// Protocol version. See [`VERSION`].
    pub v: u16,
    /// Opaque, per-connection, monotonic. What an `ack` refers back to.
    pub id: FrameId,
    /// Which run of this address the sender believes it is talking as.
    ///
    /// **Both directions, one meaning: the generation this frame's sender holds for this address.**
    /// The hub mints it when it grants a claim and stamps the `welcome` that grants it — that
    /// stamp *is* the lease, there is no second field carrying it — and stamps everything it sends
    /// to that connection afterwards. A bridge remembers the number it was welcomed with and
    /// stamps every frame it sends, the `hello` it redials with included. A peer that holds none
    /// (every bridge shipped before this field existed, and every hub) stamps none, and a peer
    /// reading a frame from one is told nothing about fencing by its absence.
    ///
    /// It rides on the ENVELOPE rather than inside a payload so that one field covers every frame
    /// in both directions — and because a second field of the same name inside a payload could not
    /// work: `flatten` puts both in one object, so a peer that set both would emit a duplicate key
    /// and serde refuses to read that, while a peer that set only the payload's would have it
    /// swallowed by this one and read back as nothing. This protocol has met that collision once
    /// already, when ping and pong tried to name their nonce `id`. **No payload field in either
    /// direction may be named `generation`**; `a_generation_rides_on_every_frame_in_both_directions_and_is_named_exactly_once`
    /// fails the day one is.
    ///
    /// Absent means "I hold no generation". A zero says the same thing and is read as absent: a
    /// zero is a number a fence can compare and it would lose every comparison it was ever in, and
    /// `welcome.generation ?? 0` is what a bridge written against the document in the language
    /// every bridge is written in puts on the wire before it has been welcomed. Being fenced for
    /// ever on every run is too high a price for a defaulting operator.
    ///
    /// The mint is bounded by [`MAX_GENERATION`], because the only bridges that exist read frames
    /// with `JSON.parse`.
    ///
    /// Skipped when absent, so a peer that names none puts BYTE FOR BYTE what it always put on
    /// the wire — on EVERY frame, which is the widest blast radius any field in this crate has.
    #[serde(
        default,
        deserialize_with = "a_zero_is_no_generation",
        skip_serializing_if = "holds_no_generation"
    )]
    pub generation: Option<u64>,
    #[serde(flatten)]
    pub payload: P,
}

impl<P> Envelope<P> {
    /// Wraps a payload at the current version, naming no generation.
    pub fn new(id: FrameId, payload: P) -> Self {
        Self {
            v: VERSION,
            id,
            generation: None,
            payload,
        }
    }

    /// Stamps this frame with the generation its sender believes it holds.
    ///
    /// A builder rather than a second constructor, because the overwhelming majority of call sites
    /// hold no generation and every one of them would otherwise have to pass a `None` — and a
    /// `None` passed by hand at fifty sites is a `Some` waiting to be pasted into the wrong one.
    ///
    /// A zero stamps nothing, so that the one value the field cannot mean cannot reach the wire
    /// from this side either. See [`Envelope::generation`].
    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = Some(generation).filter(|g| *g != 0);
        self
    }
}

/// The largest generation a bridge can be trusted to hold.
///
/// `2^53 - 1`. Every bridge that exists reads frames with `JSON.parse`, which has no integers —
/// past this a number comes back as the nearest one a double can hold, and the bridge then stamps
/// a generation the hub never minted. That failure has no wrong-looking value anywhere in it: the
/// run is simply fenced for ever. The hub's mint (`max(latest + 1, now_ms)`) is a quarter of a
/// million years short of this in milliseconds, crosses it in the twenty-third century in
/// microseconds, and is past it today in nanoseconds — so the ceiling is here to be checked by
/// whoever changes the unit.
pub const MAX_GENERATION: u64 = (1 << 53) - 1;

/// Reads a generation, taking a zero for silence. See [`Envelope::generation`].
fn a_zero_is_no_generation<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u64>::deserialize(d)?.filter(|g| *g != 0))
}

/// True when there is no generation to put on the wire — none held, or the zero that means none.
///
/// A predicate rather than `Option::is_none`, so that a `Some(0)` built by hand cannot put a
/// number on the wire that every reader of it is required to ignore.
fn holds_no_generation(generation: &Option<u64>) -> bool {
    !matches!(generation, Some(g) if *g != 0)
}

/// Does this `hello`'s promise cover the named down-frame?
///
/// The only way to read [`BridgeFrame::Hello::confirms`]. A reader that asks whether the promise
/// was *present* rather than whether it *names this frame* will hold a tap open for a bridge that
/// promised something else, and then tell the operator his answer was never taken by a session
/// that took it. Names are compared exactly: a name this hub does not send is a promise about
/// nothing, which is the same as no promise, and both fail towards saying less rather than more.
pub fn promises_to_confirm(confirms: &Option<Vec<String>>, frame_kind: &str) -> bool {
    confirms.iter().flatten().any(|w| w == frame_kind)
}

/// Reads a promise, taking an empty list for silence. See [`BridgeFrame::Hello::confirms`].
fn an_empty_promise_is_no_promise<'de, D>(d: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(d)?.filter(|c| !c.is_empty()))
}

/// True when this `hello` promised to answer nothing — unsaid, or the empty list that says it.
fn promises_nothing(confirms: &Option<Vec<String>>) -> bool {
    !matches!(confirms, Some(c) if !c.is_empty())
}

/// How a send ended, in the only three values that can be told apart.
///
/// Two values would be a lie. Telegram has no idempotency key, so a send that times out may or may
/// not have landed, and there is no way to ask. `Unseen` is that state named. It is the surviving
/// principle of a four-rung delivery ladder that used to be two thousand lines: never claim a rung
/// you did not observe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Delivered {
    /// Observed to have landed.
    Yes,
    /// Observed not to have landed.
    No,
    /// Went out and could not be checked. Never retried when the message carried buttons: two live
    /// menus for one question, both tappable forever, is a misfire this system would have built.
    Unseen,
}

/// Why the hub did not do what a frame asked. A closed set, because the bridge branches on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AckWhy {
    /// The project's own rate budget refused it.
    TooFast,
    /// It was sent, but shortened to fit.
    Clamped,
    /// There is no topic to put it in.
    NoTopic,
    /// Telegram itself refused it.
    TelegramRefused,
    /// The words reached him and the file the frame carried did not. Paired with `yes` only.
    ///
    /// Safe to add to a closed set the bridge branches on, for the reason `bad_lane` was: only a
    /// frame that CARRIED a file can be answered with it, and a bridge old enough not to know the
    /// word cannot have sent one. The reason is in his topic, in words; this is the agent's copy
    /// of the fact.
    NoFile,
    /// The same, except that nothing could be put in his topic to say so either.
    ///
    /// The words landed and then his messaging app shed the file — a flood wait, a project
    /// switched off, a topic deleted — and whatever refused the file refuses a sentence about it
    /// just as fast. So he is looking at words with nothing to explain the gap, which is the one
    /// case where an adapter must NOT tell its agent that the reason is on his phone. It is also
    /// the case that mends itself: a shed file is worth attaching again in a minute, where the
    /// refusals behind [`AckWhy::NoFile`] are permanent for that file.
    NoFileUnsaid,
    /// The frame came from a run of this address that a later run has replaced. Nothing it says is
    /// acted on. Paired with [`Delivered::No`].
    ///
    /// It means the connection is over, not that this one frame was unlucky: the next frame gets
    /// the same answer, and so does the one after it. An adapter that reads it should stop rather
    /// than re-send — re-sending is how one project ends up with two voices in one topic, which is
    /// the whole thing the generation exists to prevent.
    ///
    /// A DUTY on the hub, which this crate cannot enforce: send it only to a connection that
    /// stamped a generation. A bridge old enough not to know the word renders an unknown `why` as
    /// "his phone did not take it", so sending this to one tells the operator's agent that his
    /// messaging app refused a frame his phone never saw. Such a connection is fenced by its
    /// socket and its pid alone, which is what it was before this field existed.
    StaleGeneration,
}

/// Why the hub refused a connection outright. The frame is followed by a close.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusedReason {
    /// The token resolved to nothing. Enrolment is terminal-only.
    UnknownProject,
    /// The token did not match the registry's hash.
    BadToken,
    /// Another live connection already holds this project.
    ///
    /// A refusal, never a takeover. A takeover is what bridge-murder felt like from the inside:
    /// the incumbent kept running and stopped being heard.
    AlreadyClaimed,
    /// The major version does not match.
    VersionSkew,
    /// Enrolled, but not switched on.
    NotEnabled,
    /// The frame was larger than the ceiling. Never truncated — a half message is worse than none.
    FrameTooLarge,
    /// The `lane` on the `hello` is not a name the hub will address a conversation by.
    ///
    /// Safe to add to a closed set the bridge branches on, because only a bridge that SENT a lane
    /// can ever receive it — and a bridge old enough not to know the word cannot send one. It is
    /// permanent, not temporary: the same lane name will be refused every time, so a bridge that
    /// treats an unknown reason as worth retrying must be taught this one or it spins for ever.
    BadLane,
    /// The connection named a generation, and a later run of the same address has since been
    /// admitted. The number it holds is over.
    ///
    /// PERMANENT for that run, and the one refusal where redialling is exactly the wrong move: the
    /// address has an incumbent that is not going away, so a bridge that retries spins until
    /// somebody kills it. What ends it is a new run — a fresh instance, redialling from nothing.
    ///
    /// Two duties, neither of which this crate can enforce. The HUB sends it only to a connection
    /// that stamped a generation, for the reason [`RefusedReason::BadLane`] gives — a bridge old
    /// enough not to know the word cannot have stamped one, and both refusal tables written
    /// against this protocol so far treat a reason they do not know as worth retrying, on purpose.
    /// A BRIDGE that stamps a generation must therefore add this to the set it treats as
    /// permanent before it stamps its first one, or the first time it is fenced it redials until
    /// somebody kills it.
    StaleGeneration,
}

/// One answer button, as the bridge minted it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AskOption {
    /// Opaque. What comes back in a [`HubFrame::Choice`].
    pub option_id: OptionId,
    /// What the operator reads on the button.
    pub label: String,
}

/// What a `say` is, so the hub can render it without guessing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SayHint {
    /// Sentences meant for a person.
    Prose,
    /// A command's output. Monospace, and clipped rather than reflowed.
    Output,
}

/// What Telegram called a file the operator sent. A sticker and a video note are not carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Photo,
    Document,
    Video,
    Animation,
    Audio,
    Voice,
}

/// Why a file he sent is not on disk. The words beside it still went, and he has already been
/// told in his topic; this is the agent's copy of the same fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileWhy {
    /// Larger than the most a bot may fetch from Telegram. Never fetched, never retried.
    TooBig,
    /// Telegram would not hand it over, or the transfer broke. He was asked to send it again.
    DownloadFailed,
    /// The download was never attempted, because this machine had nowhere to put the bytes.
    ///
    /// Separate from [`FileWhy::DownloadFailed`] because the two need opposite advice and the hub
    /// cannot give both from one word: a broken transfer is worth sending again, and a media
    /// directory the hub will not write into is not — every file he sends will meet it, so "send
    /// it again" would be a loop with no end in it. Whoever runs the machine has a line in the
    /// journal naming the directory; nobody holding a phone can do anything at all.
    NotStored,
}

/// One file the operator sent, as it reached the hub's own disk — or did not.
///
/// **The bytes are never on the wire.** A frame is 64 KiB and a screenshot is a megabyte, so what
/// travels is a PATH the hub minted, inside a directory a wall mounts read-only at the same path.
/// `path` is present exactly when the bytes are there and `why` exactly when they are not, and a
/// reader that finds both or neither is looking at a hub this crate did not build.
///
/// `filename` is what the sender's client reported, carried verbatim **as data**. It is never a
/// segment of `path`, and nothing on either side may join it onto one: a name from a phone is a
/// string somebody else chose, and `../../.ssh/id_ed25519` is a name a phone can send.
///
/// `why` is written as a closed set and must be READ as an open one: it travels down only, to
/// adapters that are not this crate, and every one of them needs a fallback arm for a word it
/// does not know — which is what let [`FileWhy::NotStored`] be added without a version bump.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageFile {
    pub kind: FileKind,
    /// Absolute, minted by the hub, inside the conversation's own media directory.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<String>,
    /// What the sender's client declared, or for a photo what Telegram's own path says. Data,
    /// never a verdict on the bytes.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mime: Option<String>,
    /// What the hub wrote, counted by the hub. Present only with `path`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub why: Option<FileWhy>,
}

/// How an agent wants a file shown: a picture, or a document at full size.
///
/// A picture is downscaled on the phone, and a tall page sent as one is refused outright — a
/// photo's width plus height may not pass 10 000 and its ratio may not pass 20 — so an agent that
/// wants him to READ a page says `document`. The hub sniffs no pixels; the agent is the one that
/// knows what it rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileAs {
    Photo,
    Document,
}

/// A file an agent attached to a `say` or a `done`.
///
/// **The bytes are never on the wire.** `name` is the name of a file in THIS conversation's
/// outbox — the directory the `welcome` named in `outbox`, which a wall mounts read-write at the
/// same path — one path segment under the address rules, and nothing else: no `/`, no `..`, no
/// control character. The hub opens what the name says, following no link, and checks what it
/// opened rather than the name; a name that breaks the rules is refused before the disk is
/// touched. `filename` is what he sees a document called, as data, defaulting to `name`; `mime`
/// is the adapter's word for the bytes and decides picture or document unless `as` says.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SayFile {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub filename: Option<String>,
    #[serde(rename = "as", skip_serializing_if = "Option::is_none", default)]
    pub r#as: Option<FileAs>,
}

/// What an agent is doing, as the bridge sees it from inside its own turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BeatState {
    Working,
    Idle,
    Blocked,
    Done,
}

/// How an ask stopped being open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AskEnd {
    /// Someone answered it, possibly at the terminal rather than on the phone.
    Answered,
    /// The agent stopped asking.
    Withdrawn,
    /// It aged out.
    Timeout,
}

/// Bridge → hub.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum BridgeFrame {
    /// First frame on a connection, exactly once.
    ///
    /// No display name, by design — see the module docs.
    Hello {
        project_id: ProjectId,
        /// The enrolment secret. Compared against a stored hash, in constant time.
        token: String,
        /// This run of the worker. A new instance invalidates every outstanding ask, so a tap on a
        /// menu drawn for a dead session is refused with a reason the operator can read.
        instance: String,
        /// The repo path, for the audit record and for a human reading it. Never for routing.
        repo: String,
        pid: u32,
        /// Absent means the project's own voice — which is every bridge shipped before this field
        /// existed, and is why it is optional rather than required.
        ///
        /// Present means one worktree of that project, speaking for itself. The hub reads nothing
        /// OUT of the string; it is an address, exactly as an [`crate::ids::AskId`] is. It cannot
        /// widen a bridge's reach, because the token beside it still resolves to the PROJECT and
        /// the hub builds the address from that resolved project plus this name — so a lane named
        /// here is always a lane of the project the secret already proved.
        ///
        /// `skip_serializing_if`, so a bridge that names no lane puts BYTE FOR BYTE what it always
        /// put on the wire. A `"lane":null` would be a field an older hub has to tolerate for no
        /// reason at all, on the one frame whose failure is a project that can never connect.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        lane: Option<crate::ids::LaneId>,
        /// Which of the hub's own frames this bridge will answer with a [`BridgeFrame::Ack`],
        /// named by their `t` — today at most `["choice"]`.
        ///
        /// A promise, and the hub holds nothing open waiting for an answer it was not promised.
        /// It has to be on the wire because the alternative is inferring it from a version number
        /// no bridge sends: a hub that assumed every bridge answers would sooner or later tell the
        /// operator his tap was never taken, about a bridge that took it and had no word for
        /// saying so.
        ///
        /// Unknown names are ignored rather than refused — a bridge that promises to confirm
        /// something this hub never sends has promised nothing, which is harmless, where a
        /// refusal would be a project that cannot connect because it was too new. Read it with
        /// [`promises_to_confirm`] and never by asking whether it is present.
        ///
        /// Absent means "I answer none of them", which is every bridge shipped before this field
        /// existed. An EMPTY list says the same thing and is read as absent: an adapter that
        /// builds the list by filtering writes the empty one every time it promises nothing, and
        /// two spellings of one meaning is how a reader ends up branching on the wrong one.
        /// Skipped when it promises nothing, so such a bridge puts BYTE FOR BYTE what it always
        /// put on the wire.
        #[serde(
            default,
            deserialize_with = "an_empty_promise_is_no_promise",
            skip_serializing_if = "promises_nothing"
        )]
        confirms: Option<Vec<String>>,
    },
    /// Something the agent said. Does not buzz.
    Say {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        hint: Option<SayHint>,
        /// A file to go with the words — see [`SayFile`]. `text` may be empty when this is
        /// present; the file is then the message.
        ///
        /// `skip_serializing_if`, so a say with no file puts BYTE FOR BYTE what it always put on
        /// the wire, and a hub older than files reads it as the words alone.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        file: Option<SayFile>,
    },
    /// A question the agent is waiting on. Buzzes.
    Ask {
        ask_id: AskId,
        text: String,
        /// Absent means a free-text answer. Present means buttons.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        options: Option<Vec<AskOption>>,
    },
    /// The question stopped being open.
    ///
    /// This is the frame no pane-reading design could ever produce: a screen cannot tell you that
    /// a question is no longer being asked. The hub edits the original message and strips its
    /// buttons, so a stale keyboard cannot be tapped an hour later.
    AskResolved {
        ask_id: AskId,
        how: AskEnd,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        outcome: Option<String>,
    },
    /// The turn finished. Buzzes.
    Done {
        text: String,
        /// As on [`BridgeFrame::Say`]: what was built, rendered.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        file: Option<SayFile>,
    },
    /// Liveness and state. Does not buzz.
    Beat {
        state: BeatState,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        note: Option<String>,
    },
    /// The bridge's answer to something the hub sent it.
    Ack {
        #[serde(rename = "ref")]
        r#ref: FrameId,
        status: AckStatus,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        reason: Option<String>,
        /// Answering a [`HubFrame::Message`] that carried `files`: how many of them the bridge
        /// handed to its engine.
        ///
        /// Absent means NONE did — which is what every bridge shipped before files existed says,
        /// since it does not know the field — and the hub then tells him, in his topic, that the
        /// agent got only his words. Without this the hub could not tell an old bridge that took
        /// the caption and dropped the picture from a new one that took both, and would put the
        /// thumb on his message for a file nobody received.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        files: Option<u32>,
    },
    /// Going away. Inside the grace window after a clean `bye` the hub says nothing at all: a
    /// phone that buzzes on every context refresh is worse than useless.
    Bye { reason: String },
    /// Answer to a [`HubFrame::Ping`], naming the ping's own frame id.
    ///
    /// The design sketch gave ping and pong a payload field called `id`. It cannot be called that:
    /// every frame already carries an envelope `id`, and flattening the two together produces a
    /// duplicate key that serde refuses. The correlation is `ref`, exactly as it is for an `ack`,
    /// and the ping's nonce is simply its envelope id — one fewer id to mint and one fewer to
    /// confuse with another.
    Pong {
        #[serde(rename = "ref")]
        r#ref: FrameId,
    },
    /// A kind this build does not know.
    ///
    /// Logged and ignored. It is a variant rather than a parse error so that a bridge shipped
    /// after this hub cannot kill the connection just by being newer.
    #[serde(other)]
    Unknown,
}

/// Whether the far side took a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AckStatus {
    Accepted,
    Refused,
}

/// Hub → bridge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum HubFrame {
    /// The connection is admitted. Carries the name the REGISTRY holds, not anything the bridge sent.
    ///
    /// `topic_id` is `None` at this point and usually stays that way for a few seconds: a topic is
    /// created when the connection becomes LIVE — after a settling window and one answered ping —
    /// not when it is admitted. A channel plugin that is not allowlisted boots and exits in about a
    /// tenth of a second, and a topic created for one of those is an empty topic bound forever to a
    /// project whose bridge was never there.
    ///
    /// A bridge does not need it and must not use it to address anything. It is here because the
    /// number is useful in a log when someone is working out where a message went.
    Welcome {
        project: String,
        /// The lane the hub actually admitted, echoed back.
        ///
        /// It exists so a bridge that NAMED a lane can tell it was heard. An unknown field inside a
        /// known kind is ignored on purpose, which is what lets a new bridge talk to an old hub —
        /// but it also means an old hub admits a lane silently as the project itself, giving the
        /// worktree the project's one claim and its topic while the project's own session is turned
        /// away. Nothing else in this frame can distinguish that from being given a place of one's
        /// own: `project` is a registry-owned title the bridge cannot predict.
        ///
        /// Absent means "no lane", which is both what an ordinary session is and what every hub
        /// built before lanes says about everything. A bridge that named one and gets no echo has
        /// learned the hub is older than it is, and must refuse rather than impersonate.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        lane: Option<crate::ids::LaneId>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        topic_id: Option<i32>,
        limits: Limits,
        /// The absolute path of THIS conversation's outbox, as the hub sees it and as a wall must
        /// mount it: where an adapter copies a file before it sends the name on a `say`.
        ///
        /// It has to be told. An adapter never learns its project id — `hello` carries a
        /// placeholder and this frame carries a title — and inside a wall its own `$HOME` is not
        /// the hub's, so nothing it holds can derive the path. Absent on every hub before files,
        /// and **absence means this hub carries no files**: an adapter asked to send one then
        /// sends the words alone and says so in its own tool result, rather than sending a field
        /// the hub would strip in silence.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        outbox: Option<String>,
        //
        // The generation this welcome grants is [`Envelope::generation`] on the welcome itself,
        // and there is deliberately no field for it here: the envelope's own `generation` and a
        // payload one flatten into the same key, so a hub that set both would emit a duplicate key
        // no reader can read, and one that set only this would have it swallowed on the way in and
        // read back as nothing. A bridge takes its lease from the envelope of the frame that
        // grants it, exactly as it takes it from every frame afterwards.
    },
    /// Not admitted. The connection closes immediately after.
    Refused { reason: RefusedReason },
    /// The operator's own words, relayed verbatim.
    ///
    /// **Opaque.** The hub does not parse it, does not act on it, and does not let it name
    /// anything. Inbound content selects; it never names.
    Message {
        msg_id: MsgId,
        /// His caption, verbatim, or the empty string when he sent a file and wrote nothing. A
        /// file with no words is still a message.
        text: String,
        from: From,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        in_reply_to_ask: Option<AskId>,
        /// The files he sent with it, one entry each — see [`MessageFile`]. A Telegram message
        /// carries one file, so today this holds one; an album arrives as one message per file.
        ///
        /// `skip_serializing_if`, so a message with no file puts BYTE FOR BYTE what it always put
        /// on the wire: the bridge in the operator's own session is older than this field and
        /// restarts only with his conversation.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        files: Option<Vec<MessageFile>>,
    },
    /// A tap, resolved against the record written down beside the message.
    Choice {
        msg_id: MsgId,
        ask_id: AskId,
        option_id: OptionId,
    },
    /// What became of one of the bridge's frames. Every frame gets exactly one.
    ///
    /// Backpressure reaches the only party that can act on it. A rejected send used to be one
    /// error log and a drop, which already lost 5,164 characters of a real agent's longest message.
    Ack {
        #[serde(rename = "ref")]
        r#ref: FrameId,
        delivered: Delivered,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        why: Option<AckWhy>,
    },
    /// Liveness probe. Its envelope `id` is the nonce; the answer names it in `ref`.
    ///
    /// A project counts as live only after `hello`, a settling window, and one answered ping — a
    /// channel that is not allowlisted boots and exits in about a tenth of a second, and would
    /// otherwise look exactly like a healthy worker for as long as anyone cared to watch.
    Ping,
    /// A kind this build does not know. Logged and ignored.
    #[serde(other)]
    Unknown,
}

/// Who sent an inbound message. For the audit record and for the allowlist decision that has
/// already been made by the time this frame exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct From {
    pub chat_id: i64,
    pub user_id: i64,
}

/// What the hub will accept from this connection, told to the bridge rather than discovered by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub max_frame: usize,
    pub max_text: usize,
    pub frames_per_min: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<P>(p: P) -> Envelope<P> {
        Envelope::new(FrameId::new("f1"), p)
    }

    #[test]
    fn an_envelope_is_one_flat_object_carrying_v_and_id_and_the_kind() {
        let json = serde_json::to_string(&env(BridgeFrame::Done {
            text: "built it".into(),
            file: None,
        }))
        .expect("serialises");
        assert_eq!(json, r#"{"v":1,"id":"f1","t":"done","text":"built it"}"#);
    }

    #[test]
    fn an_unknown_frame_kind_parses_rather_than_killing_the_connection() {
        // The whole point: a bridge shipped after this hub sends something newer, and the
        // connection survives it. A parse error here would be a deaf worker on upgrade day.
        let f: Envelope<BridgeFrame> =
            serde_json::from_str(r#"{"v":1,"id":"f9","t":"telepathy","mood":"blue"}"#)
                .expect("an unknown kind is a value, not an error");
        assert_eq!(f.payload, BridgeFrame::Unknown);
        assert_eq!(f.v, VERSION);
    }

    #[test]
    fn an_unknown_field_inside_a_known_kind_is_ignored() {
        let f: Envelope<BridgeFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f2","t":"say","text":"hi","hint":"prose","colour":"red"}"#,
        )
        .expect("additive fields do not break a known kind");
        assert_eq!(
            f.payload,
            BridgeFrame::Say {
                text: "hi".into(),
                hint: Some(SayHint::Prose),
                file: None,
            }
        );
    }

    #[test]
    fn a_hello_carries_no_display_name_for_the_topic() {
        // Pinned as a property of the TYPE, not of a code path: a bridge that could name itself
        // could claim another project's topic, and no amount of escaping downstream would help.
        // A lane is not an exception to this. It names WHICH CONVERSATION a connection is, and the
        // hub still takes every title it prints from its own registry.
        let json = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("p-herdr-tg"),
            token: "s3cret".into(),
            instance: "i1".into(),
            repo: "/home/u/Projects/herdr-tg".into(),
            pid: 42,
            lane: Some(crate::ids::LaneId::new("lane-0902-201212-2783563")),
            confirms: None,
        }))
        .expect("serialises");
        for forbidden in ["\"name\"", "\"project\":", "\"title\"", "\"topic\""] {
            assert!(
                !json.contains(forbidden),
                "hello must not carry {forbidden}: {json}"
            );
        }
    }

    #[test]
    fn a_hello_without_a_lane_is_byte_for_byte_the_hello_this_protocol_has_always_sent() {
        // The upgrade that matters most is the one nobody performs: a bridge already installed in a
        // running session keeps sending exactly this, and it must go on being the project's own
        // voice. Pinned as BYTES rather than as a round trip, because a round trip is green even
        // when a `"lane":null` has appeared on the wire — and a null here is a field an older hub
        // would have to be tolerant of for no reason at all.
        let json = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("unknown-until-the-hub-says"),
            token: "s3cret".into(),
            instance: "i1".into(),
            repo: "/home/u/Projects/herdr-tg".into(),
            pid: 42,
            lane: None,
            confirms: None,
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"hello","project_id":"unknown-until-the-hub-says","token":"s3cret","instance":"i1","repo":"/home/u/Projects/herdr-tg","pid":42}"#
        );
    }

    #[test]
    fn a_hello_from_a_bridge_that_has_never_heard_of_lanes_is_still_a_hello() {
        // The upgrade day that actually happens: the hub is replaced and the bridge is not, because
        // a channel plugin restarts only when its session does. The hello already on the wire has no
        // `lane` in it at all, and if this build required one, every project on the box would be
        // refused at the first gate with nothing to say why.
        let f: Envelope<BridgeFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f2","t":"hello","project_id":"p","token":"s","instance":"i","repo":"/r","pid":1}"#,
        )
        .expect("a hello with no lane must not be a parse error");
        assert_eq!(
            f.payload,
            BridgeFrame::Hello {
                project_id: ProjectId::new("p"),
                token: "s".into(),
                instance: "i".into(),
                repo: "/r".into(),
                pid: 1,
                lane: None,
                confirms: None,
            },
            "a bridge that named no lane stopped being the project's own voice"
        );
    }

    #[test]
    fn a_hello_that_names_a_lane_still_parses_on_a_build_that_has_never_heard_of_lanes() {
        // The other direction of the same skew, and the only one that cannot be run end to end,
        // because the hub that would have to be old is the one being replaced. `hello` is where a
        // wrong answer costs most: it is refused before anything else can happen and the bridge is
        // left with a closed socket and no reason.
        //
        // What makes it safe is structural rather than a policy anyone could flip. The escape hatch
        // — `deny_unknown_fields` on this variant — DOES NOT COMPILE here: serde refuses it on an
        // internally tagged enum reached through the envelope's `flatten`, with 49 errors. So the
        // tolerance is a property of the shape of this type, and the assertion below is what a
        // reader gets to see rather than the whole of the guarantee.
        let f: Envelope<BridgeFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f2","t":"hello","project_id":"p","token":"s","instance":"i","repo":"/r","pid":1,"lane":"lane-0902-201212-2783563"}"#,
        )
        .expect("a hello naming a lane must not be a parse error");
        let BridgeFrame::Hello { instance, .. } = f.payload else {
            panic!("a hello that names a lane stopped being a hello");
        };
        assert_eq!(instance, "i");
    }

    #[test]
    fn an_ack_says_which_of_the_three_delivery_states_it_observed() {
        let json = serde_json::to_string(&env(HubFrame::Ack {
            r#ref: FrameId::new("f7"),
            delivered: Delivered::Unseen,
            why: Some(AckWhy::TooFast),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"ack","ref":"f7","delivered":"unseen","why":"too-fast"}"#
        );
    }

    #[test]
    fn every_bridge_frame_round_trips() {
        let frames = vec![
            BridgeFrame::Say {
                text: "x".into(),
                hint: None,
                file: None,
            },
            BridgeFrame::Done {
                text: "the chart".into(),
                file: Some(SayFile {
                    name: "3c9e1b7a.png".into(),
                    mime: Some("image/png".into()),
                    filename: None,
                    r#as: Some(FileAs::Document),
                }),
            },
            BridgeFrame::Ask {
                ask_id: AskId::new("a1"),
                text: "ok?".into(),
                options: Some(vec![AskOption {
                    option_id: OptionId::new("y"),
                    label: "Yes".into(),
                }]),
            },
            BridgeFrame::AskResolved {
                ask_id: AskId::new("a1"),
                how: AskEnd::Answered,
                outcome: Some("No".into()),
            },
            BridgeFrame::Beat {
                state: BeatState::Blocked,
                note: None,
            },
            BridgeFrame::Ack {
                r#ref: FrameId::new("f3"),
                status: AckStatus::Refused,
                reason: Some("busy".into()),
                files: Some(1),
            },
            BridgeFrame::Bye {
                reason: "refresh".into(),
            },
            BridgeFrame::Pong {
                r#ref: FrameId::new("f4"),
            },
        ];
        for f in frames {
            let json = serde_json::to_string(&env(f.clone())).expect("serialises");
            let back: Envelope<BridgeFrame> = serde_json::from_str(&json).expect("deserialises");
            assert_eq!(back.payload, f, "round trip changed the frame: {json}");
        }
    }

    #[test]
    fn no_payload_field_collides_with_the_envelope_id() {
        // The collision this pins is not hypothetical: the first cut of ping/pong named its nonce
        // `id`, which flattens on top of the envelope's own `id`, and serde refused every such
        // frame with "duplicate field `id`". Cheap to reintroduce, invisible until a round trip.
        let ping = serde_json::to_string(&env(HubFrame::Ping)).expect("serialises");
        assert_eq!(ping, r#"{"v":1,"id":"f1","t":"ping"}"#);
        let back: Envelope<HubFrame> = serde_json::from_str(&ping).expect("round trips");
        assert_eq!(back.payload, HubFrame::Ping);

        let pong = serde_json::to_string(&env(BridgeFrame::Pong {
            r#ref: FrameId::new("f1"),
        }))
        .expect("serialises");
        assert_eq!(pong, r#"{"v":1,"id":"f1","t":"pong","ref":"f1"}"#);
        serde_json::from_str::<Envelope<BridgeFrame>>(&pong).expect("round trips");
    }

    #[test]
    fn a_refusal_names_a_reason_the_bridge_can_branch_on() {
        let json = serde_json::to_string(&env(HubFrame::Refused {
            reason: RefusedReason::AlreadyClaimed,
        }))
        .expect("serialises");
        assert!(json.contains(r#""reason":"already_claimed""#), "{json}");
    }

    // ── files ─────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_message_without_files_is_byte_for_byte_the_message_this_protocol_has_always_sent() {
        // The bridge in the operator's own session predates files and restarts only with his
        // conversation. It keeps receiving exactly this. Pinned as BYTES rather than as a round
        // trip, because a round trip is green even when a `"files":null` or `"files":[]` has
        // appeared on the wire — a field an older bridge has to tolerate for no reason at all.
        let json = serde_json::to_string(&env(HubFrame::Message {
            msg_id: MsgId::new("m-4412"),
            text: "try it with --dry-run first".into(),
            from: From {
                chat_id: -1001,
                user_id: 7,
            },
            in_reply_to_ask: None,
            files: None,
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"message","msg_id":"m-4412","text":"try it with --dry-run first","from":{"chat_id":-1001,"user_id":7}}"#
        );
    }

    #[test]
    fn a_message_carrying_a_file_names_a_path_a_mime_a_count_and_the_reported_name_as_data() {
        // The worked frame in `docs/ATTACHING.md` §14.2, byte for byte. `bytes` is a number and
        // `filename` is a string that is never joined onto the path: a bridge reading this has
        // the path to open and the name to show, and no reason to build one from the other.
        let json = serde_json::to_string(&env(HubFrame::Message {
            msg_id: MsgId::new("m-4412"),
            text: "this is what the login page looks like now".into(),
            from: From {
                chat_id: -1001,
                user_id: 7,
            },
            in_reply_to_ask: None,
            files: Some(vec![MessageFile {
                kind: FileKind::Photo,
                path: Some("/state/media/p-9f3a1c2e5b7d/-/20260905-231455-9f3a1c2e.jpg".into()),
                mime: Some("image/jpeg".into()),
                bytes: Some(1_183_412),
                filename: Some("../../.ssh/id_ed25519".into()),
                why: None,
            }]),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"message","msg_id":"m-4412","text":"this is what the login page looks like now","from":{"chat_id":-1001,"user_id":7},"files":[{"kind":"photo","path":"/state/media/p-9f3a1c2e5b7d/-/20260905-231455-9f3a1c2e.jpg","mime":"image/jpeg","bytes":1183412,"filename":"../../.ssh/id_ed25519"}]}"#
        );
        // And the one that did not come through: no path, a reason, and the words beside it.
        let json = serde_json::to_string(&env(HubFrame::Message {
            msg_id: MsgId::new("m-4413"),
            text: String::new(),
            from: From {
                chat_id: -1001,
                user_id: 7,
            },
            in_reply_to_ask: None,
            files: Some(vec![MessageFile {
                kind: FileKind::Document,
                path: None,
                mime: None,
                bytes: None,
                filename: Some("build.log".into()),
                why: Some(FileWhy::TooBig),
            }]),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"message","msg_id":"m-4413","text":"","from":{"chat_id":-1001,"user_id":7},"files":[{"kind":"document","filename":"build.log","why":"too-big"}]}"#
        );
    }

    #[test]
    fn an_old_bridge_ignores_the_attachment_and_still_gets_the_text() {
        // The skew that actually happens: the hub is replaced, the bridge in his session is not.
        // "A build that has never heard of files" is modelled here as a TYPE — the `message`
        // variant exactly as this crate shipped it before the field existed, same tag, same
        // envelope, same flatten — reading the frame the new hub sends. It must come out as his
        // words with the file simply absent, never as a parse error that ends his conversation.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum HubFrameBeforeFiles {
            Message {
                msg_id: MsgId,
                text: String,
                from: From,
                #[serde(default)]
                in_reply_to_ask: Option<AskId>,
            },
            #[serde(other)]
            Unknown,
        }
        let sent = serde_json::to_string(&env(HubFrame::Message {
            msg_id: MsgId::new("m-4412"),
            text: "this is what the login page looks like now".into(),
            from: From {
                chat_id: -1001,
                user_id: 7,
            },
            in_reply_to_ask: None,
            files: Some(vec![MessageFile {
                kind: FileKind::Photo,
                path: Some("/state/media/p-9f3a1c2e5b7d/-/20260905-231455-9f3a1c2e.jpg".into()),
                mime: Some("image/jpeg".into()),
                bytes: Some(1_183_412),
                filename: None,
                why: None,
            }]),
        }))
        .expect("serialises");
        let old: Envelope<HubFrameBeforeFiles> =
            serde_json::from_str(&sent).expect("a bridge older than files must still read this");
        assert_eq!(
            old.payload,
            HubFrameBeforeFiles::Message {
                msg_id: MsgId::new("m-4412"),
                text: "this is what the login page looks like now".into(),
                from: From {
                    chat_id: -1001,
                    user_id: 7,
                },
                in_reply_to_ask: None,
            },
            "the words beside the file did not survive a bridge that ignores the file"
        );
    }

    #[test]
    fn an_ack_that_counts_files_parses_on_a_hub_that_has_never_heard_of_them() {
        // The other direction, modelled the same way: the `ack` variant as it was before `files`,
        // reading what a new bridge sends. And on THIS build, an ack from an old bridge — no
        // `files` at all — reads as none handed on, which is the truth about that bridge.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum BridgeFrameBeforeFiles {
            Ack {
                #[serde(rename = "ref")]
                r#ref: FrameId,
                status: AckStatus,
                #[serde(default)]
                reason: Option<String>,
            },
            #[serde(other)]
            Unknown,
        }
        let new = serde_json::to_string(&env(BridgeFrame::Ack {
            r#ref: FrameId::new("h7"),
            status: AckStatus::Accepted,
            reason: None,
            files: Some(1),
        }))
        .expect("serialises");
        assert_eq!(
            new,
            r#"{"v":1,"id":"f1","t":"ack","ref":"h7","status":"accepted","files":1}"#
        );
        let old: Envelope<BridgeFrameBeforeFiles> =
            serde_json::from_str(&new).expect("a hub older than files must still read this");
        assert!(
            matches!(
                old.payload,
                BridgeFrameBeforeFiles::Ack {
                    status: AckStatus::Accepted,
                    ..
                }
            ),
            "{old:?}"
        );

        let from_an_old_bridge: Envelope<BridgeFrame> =
            serde_json::from_str(r#"{"v":1,"id":"b3","t":"ack","ref":"h7","status":"accepted"}"#)
                .expect("the ack every bridge has always sent");
        assert_eq!(
            from_an_old_bridge.payload,
            BridgeFrame::Ack {
                r#ref: FrameId::new("h7"),
                status: AckStatus::Accepted,
                reason: None,
                files: None,
            }
        );
    }

    // ── files, up ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_say_without_a_file_is_byte_for_byte_the_say_this_protocol_has_always_sent() {
        // Every bridge that never attaches anything — which is every bridge until today — keeps
        // putting exactly this on the wire. Pinned as BYTES: a round trip is green even when a
        // `"file":null` has appeared, which a hub older than files would have to tolerate for
        // nothing.
        let json = serde_json::to_string(&env(BridgeFrame::Say {
            text: "hi".into(),
            hint: Some(SayHint::Prose),
            file: None,
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"say","text":"hi","hint":"prose"}"#
        );
    }

    #[test]
    fn a_say_carrying_a_file_names_it_in_the_outbox_and_the_reported_name_as_data() {
        // The worked frame in `docs/ATTACHING.md` §14.2, byte for byte. `name` is one segment in
        // the outbox the welcome named; `filename` is what he sees it called and is never joined
        // onto anything; `as` is absent unless the agent chose, and it is spelt `as` on the wire.
        let json = serde_json::to_string(&env(BridgeFrame::Say {
            text: "the chart, rebuilt".into(),
            hint: None,
            file: Some(SayFile {
                name: "3c9e1b7a.png".into(),
                mime: Some("image/png".into()),
                filename: Some("latency-p99.png".into()),
                r#as: None,
            }),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"say","text":"the chart, rebuilt","file":{"name":"3c9e1b7a.png","mime":"image/png","filename":"latency-p99.png"}}"#
        );
        let json = serde_json::to_string(&env(BridgeFrame::Done {
            text: String::new(),
            file: Some(SayFile {
                name: "page.png".into(),
                mime: None,
                filename: None,
                r#as: Some(FileAs::Document),
            }),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"done","text":"","file":{"name":"page.png","as":"document"}}"#
        );
    }

    #[test]
    fn a_new_bridge_sending_an_attachment_to_an_old_hub_still_delivers_its_text() {
        // The direction that cannot be run end to end, because the hub old enough to test against
        // is the one being replaced. "A hub that has never heard of files" is modelled as a TYPE:
        // the `say` and `done` variants exactly as this crate shipped them before `file` existed,
        // same tag, same envelope, same flatten. The words must come out as a say of the words,
        // never as a parse error that ends the agent's connection — the file is simply not there,
        // and the bridge, which was told no outbox, has already said so in its tool result.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum BridgeFrameBeforeFiles {
            Say {
                text: String,
                #[serde(default)]
                hint: Option<SayHint>,
            },
            Done {
                text: String,
            },
            #[serde(other)]
            Unknown,
        }
        let file = SayFile {
            name: "3c9e1b7a.png".into(),
            mime: Some("image/png".into()),
            filename: Some("latency-p99.png".into()),
            r#as: Some(FileAs::Photo),
        };
        let sent = serde_json::to_string(&env(BridgeFrame::Say {
            text: "the chart, rebuilt".into(),
            hint: Some(SayHint::Prose),
            file: Some(file.clone()),
        }))
        .expect("serialises");
        let old: Envelope<BridgeFrameBeforeFiles> =
            serde_json::from_str(&sent).expect("a hub older than files must still read a say");
        assert_eq!(
            old.payload,
            BridgeFrameBeforeFiles::Say {
                text: "the chart, rebuilt".into(),
                hint: Some(SayHint::Prose),
            },
            "the words did not survive a hub that ignores the file"
        );
        let sent = serde_json::to_string(&env(BridgeFrame::Done {
            text: "built it".into(),
            file: Some(file),
        }))
        .expect("serialises");
        let old: Envelope<BridgeFrameBeforeFiles> =
            serde_json::from_str(&sent).expect("a hub older than files must still read a done");
        assert_eq!(
            old.payload,
            BridgeFrameBeforeFiles::Done {
                text: "built it".into()
            }
        );

        // And the case §14.2 allows and nothing else here covers: NO WORDS, the file being the
        // message. `text` must still go on the wire as the empty string, because a hub older than
        // files REQUIRES the field — dropping it as "nothing to say" turns the frame into a parse
        // error, and a parse error on a `say` is the agent's connection ending mid-turn. This is
        // the one edit that would break this skew and read like a tidy-up while doing it.
        for empty in [
            env(BridgeFrame::Say {
                text: String::new(),
                hint: None,
                file: Some(SayFile {
                    name: "page.png".into(),
                    mime: None,
                    filename: None,
                    r#as: None,
                }),
            }),
            env(BridgeFrame::Done {
                text: String::new(),
                file: Some(SayFile {
                    name: "page.png".into(),
                    mime: None,
                    filename: None,
                    r#as: None,
                }),
            }),
        ] {
            let sent = serde_json::to_string(&empty).expect("serialises");
            assert!(
                sent.contains(r#""text":"""#),
                "a file with no words dropped `text`, which a hub older than files requires: {sent}"
            );
            let old: Envelope<BridgeFrameBeforeFiles> = serde_json::from_str(&sent)
                .unwrap_or_else(|e| panic!("a hub older than files could not read {sent}: {e}"));
            assert!(
                matches!(
                    old.payload,
                    BridgeFrameBeforeFiles::Say { ref text, .. }
                        | BridgeFrameBeforeFiles::Done { ref text }
                        if text.is_empty()
                ),
                "{old:?}"
            );
        }
    }

    #[test]
    fn a_welcome_without_an_outbox_is_byte_for_byte_the_welcome_this_protocol_has_always_sent() {
        let json = serde_json::to_string(&env(HubFrame::Welcome {
            project: "A Title".into(),
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"welcome","project":"A Title","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}"#
        );
    }

    #[test]
    fn a_welcome_naming_an_outbox_still_parses_on_a_bridge_that_has_never_heard_of_files() {
        // The bridge in the operator's own session predates files and restarts only with his
        // conversation. The welcome it reads now names an outbox it does not know the word for,
        // and it must go on being welcomed — a parse error at `welcome` is a project that can
        // never connect. Modelled as a type, like the `lane` case above.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum HubFrameBeforeFiles {
            Welcome {
                project: String,
                #[serde(default)]
                lane: Option<crate::ids::LaneId>,
                #[serde(default)]
                topic_id: Option<i32>,
                limits: Limits,
            },
            #[serde(other)]
            Unknown,
        }
        let sent = serde_json::to_string(&env(HubFrame::Welcome {
            project: "A Title".into(),
            lane: Some(crate::ids::LaneId::new("engineering")),
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: Some("/state/outbox/p-9f3a1c2e5b7d/engineering".into()),
        }))
        .expect("serialises");
        assert_eq!(
            sent,
            r#"{"v":1,"id":"f1","t":"welcome","project":"A Title","lane":"engineering","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20},"outbox":"/state/outbox/p-9f3a1c2e5b7d/engineering"}"#
        );
        let old: Envelope<HubFrameBeforeFiles> =
            serde_json::from_str(&sent).expect("a bridge older than files must still be welcomed");
        assert!(
            matches!(old.payload, HubFrameBeforeFiles::Welcome { ref project, .. } if project == "A Title"),
            "{old:?}"
        );
    }

    #[test]
    fn an_ack_saying_the_file_did_not_go_spells_it_the_way_the_document_does() {
        let json = serde_json::to_string(&env(HubFrame::Ack {
            r#ref: FrameId::new("f12"),
            delivered: Delivered::Yes,
            why: Some(AckWhy::NoFile),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"ack","ref":"f12","delivered":"yes","why":"no-file"}"#
        );
        // And the one that says nothing reached his topic either. Two words, not one, because an
        // adapter that told its agent "the reason is on his phone" would be wrong in this case and
        // right in the other, and it has only this field to tell them apart.
        let json = serde_json::to_string(&env(HubFrame::Ack {
            r#ref: FrameId::new("f13"),
            delivered: Delivered::Yes,
            why: Some(AckWhy::NoFileUnsaid),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"ack","ref":"f13","delivered":"yes","why":"no-file-unsaid"}"#
        );
    }

    #[test]
    fn a_file_this_machine_could_not_store_is_spelt_the_way_the_document_does() {
        // Its own word, not `download-failed`: nothing was downloaded, and the two need opposite
        // advice — send it again, against nothing you can do from a phone.
        let json = serde_json::to_string(&env(HubFrame::Message {
            msg_id: MsgId::new("m-4414"),
            text: "have a look".into(),
            from: From {
                chat_id: -1001,
                user_id: 7,
            },
            in_reply_to_ask: None,
            files: Some(vec![MessageFile {
                kind: FileKind::Photo,
                path: None,
                mime: None,
                bytes: None,
                filename: None,
                why: Some(FileWhy::NotStored),
            }]),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"message","msg_id":"m-4414","text":"have a look","from":{"chat_id":-1001,"user_id":7},"files":[{"kind":"photo","why":"not-stored"}]}"#
        );
    }

    // ── the generation, and what a bridge promises to confirm ─────────────────────────────────

    #[test]
    fn the_generation_stamped_on_an_envelope_survives_a_round_trip() {
        // A number the hub minted is worthless if the crate that carries it quietly drops it: the
        // fence would then admit every stale run and nobody would see a wrong answer until two
        // sessions were writing into one topic.
        let on_the_wire = r#"{"v":1,"id":"f1","generation":7,"t":"ping"}"#;
        let f: Envelope<HubFrame> =
            serde_json::from_str(on_the_wire).expect("a stamped frame must parse");
        assert_eq!(f.payload, HubFrame::Ping);
        assert_eq!(f.generation, Some(7));
        let back = serde_json::to_string(&f).expect("serialises");
        assert_eq!(
            back, on_the_wire,
            "the generation did not survive being read and written again"
        );
    }

    #[test]
    fn a_frame_from_a_peer_that_has_never_heard_of_generations_names_no_generation_here() {
        // Every bridge shipped so far sends exactly this. It must go on meaning "I hold no
        // generation" rather than being a parse error or a zero, because a zero would be a number
        // the fence could compare and would lose every comparison.
        let on_the_wire = r#"{"v":1,"id":"f1","t":"ping"}"#;
        let f: Envelope<HubFrame> =
            serde_json::from_str(on_the_wire).expect("an unstamped frame must parse");
        assert_eq!(f.generation, None);
        let back = serde_json::to_string(&f).expect("serialises");
        assert_eq!(
            back, on_the_wire,
            "reading and writing an unstamped frame put something new on the wire"
        );
    }

    #[test]
    fn a_generation_named_inside_a_hello_is_the_envelopes_own_and_there_is_only_one_of_them() {
        // There is ONE generation and it lives on the envelope, including on the `hello` a bridge
        // redials with. A second field of the same name inside the payload cannot exist: `flatten`
        // puts both in one object, so a peer that set both would emit a duplicate key and serde
        // refuses to read it — the `id` collision this protocol already hit once, with a number
        // that fences claims instead of a nonce.
        let on_the_wire = r#"{"v":1,"id":"f1","t":"hello","project_id":"p","token":"s","instance":"i","repo":"/r","pid":1,"generation":9}"#;
        let f: Envelope<BridgeFrame> =
            serde_json::from_str(on_the_wire).expect("a redialling hello must parse");
        assert_eq!(f.generation, Some(9));
        let back = serde_json::to_string(&f).expect("serialises");
        assert!(
            back.contains(r#""generation":9"#),
            "the generation a redialling bridge named was dropped: {back}"
        );
        assert_eq!(
            back.matches(r#""generation""#).count(),
            1,
            "two generations in one frame is a duplicate key nobody can read: {back}"
        );
    }

    #[test]
    fn a_generation_arriving_beside_a_field_this_build_has_never_heard_of_is_still_read() {
        // The skew rule of this crate applied to the new field: a newer peer adds something else
        // beside the generation, and the generation still arrives. Ignoring the unknown must not
        // mean ignoring its neighbour.
        let f: Envelope<HubFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f1","generation":7,"t":"ping","telepathy":"blue"}"#,
        )
        .expect("an unknown field beside a generation must not be a parse error");
        assert_eq!(f.generation, Some(7));
        assert_eq!(f.payload, HubFrame::Ping);
        let back = serde_json::to_string(&f).expect("serialises");
        assert_eq!(back, r#"{"v":1,"id":"f1","generation":7,"t":"ping"}"#);
    }

    #[test]
    fn a_refusal_for_a_run_a_newer_one_has_replaced_is_spelt_the_way_the_document_does() {
        let f: Envelope<HubFrame> =
            serde_json::from_str(r#"{"v":1,"id":"f1","t":"refused","reason":"stale_generation"}"#)
                .expect("a bridge must be able to read the reason it is being sent away for");
        assert_eq!(
            f.payload,
            HubFrame::Refused {
                reason: RefusedReason::StaleGeneration
            }
        );
        let back = serde_json::to_string(&f).expect("serialises");
        assert_eq!(
            back,
            r#"{"v":1,"id":"f1","t":"refused","reason":"stale_generation"}"#
        );
    }

    #[test]
    fn an_ack_saying_a_newer_run_has_taken_the_address_is_spelt_the_way_the_document_does() {
        let f: Envelope<HubFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f1","t":"ack","ref":"f7","delivered":"no","why":"stale-generation"}"#,
        )
        .expect("a bridge must be able to read why its frame went nowhere");
        assert_eq!(
            f.payload,
            HubFrame::Ack {
                r#ref: FrameId::new("f7"),
                delivered: Delivered::No,
                why: Some(AckWhy::StaleGeneration),
            }
        );
        let back = serde_json::to_string(&f).expect("serialises");
        assert_eq!(
            back,
            r#"{"v":1,"id":"f1","t":"ack","ref":"f7","delivered":"no","why":"stale-generation"}"#
        );
    }

    #[test]
    fn a_frame_a_newer_peer_stamped_still_parses_on_a_build_that_has_never_heard_of_generations() {
        // The direction that cannot be run end to end, because the peer that would have to be old
        // is the one being replaced. Every frame in both directions carries this field now, so a
        // build that rejected it would go deaf on the first ping rather than on some rare frame.
        #[derive(Debug, PartialEq, Deserialize)]
        struct EnvelopeBeforeGenerations<P> {
            v: u16,
            id: FrameId,
            #[serde(flatten)]
            payload: P,
        }
        let sent =
            serde_json::to_string(&env(HubFrame::Ping).with_generation(7)).expect("serialises");
        assert_eq!(sent, r#"{"v":1,"id":"f1","generation":7,"t":"ping"}"#);
        let old: EnvelopeBeforeGenerations<HubFrame> =
            serde_json::from_str(&sent).expect("a build older than generations must still read it");
        assert_eq!(old.payload, HubFrame::Ping);
        assert_eq!(old.v, VERSION);
    }

    #[test]
    fn a_hello_that_promises_to_confirm_nothing_is_byte_for_byte_the_hello_this_protocol_has_always_sent()
     {
        // A bridge that answers no down-frame says nothing about it, and a hub reading that hello
        // sees exactly the bytes it has always seen. Pinned as BYTES rather than as a round trip,
        // because a round trip stays green when a `"confirms":null` or a `"confirms":[]` has
        // appeared — and either would make an older hub tolerate a field for no reason at all, on
        // the one frame whose failure is a project that can never connect. Both spellings of the
        // empty promise are pinned to the same bytes, so this cannot quietly become a second copy
        // of the lane pin standing beside it.
        let hello = |confirms| {
            serde_json::to_string(&env(BridgeFrame::Hello {
                project_id: ProjectId::new("unknown-until-the-hub-says"),
                token: "s3cret".into(),
                instance: "i1".into(),
                repo: "/home/u/Projects/herdr-tg".into(),
                pid: 42,
                lane: None,
                confirms,
            }))
            .expect("serialises")
        };
        let always = r#"{"v":1,"id":"f1","t":"hello","project_id":"unknown-until-the-hub-says","token":"s3cret","instance":"i1","repo":"/home/u/Projects/herdr-tg","pid":42}"#;
        assert_eq!(hello(None), always);
        assert_eq!(
            hello(Some(vec![])),
            always,
            "a bridge that promised nothing still put a promise on the wire"
        );
    }

    #[test]
    fn a_hello_that_names_which_frames_it_will_confirm_carries_them_on_the_wire() {
        // The hub has to know WHICH bridges will answer before it can hold anything open waiting
        // for an answer. A bridge that promises nothing is never waited on, so the promise has to
        // be on the wire rather than inferred from a version number nobody sends.
        let json = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("p"),
            token: "s".into(),
            instance: "i".into(),
            repo: "/r".into(),
            pid: 1,
            lane: None,
            confirms: Some(vec!["choice".into()]),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"hello","project_id":"p","token":"s","instance":"i","repo":"/r","pid":1,"confirms":["choice"]}"#
        );
    }

    #[test]
    fn a_hello_that_names_what_it_will_confirm_still_parses_on_a_hub_that_has_never_heard_of_confirming()
     {
        // Same shape as the lane case: an adapter is upgraded and the hub in front of it is not.
        // A refusal here is a project that can never connect, so the tolerance is pinned rather
        // than assumed.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum BridgeFrameBeforeConfirming {
            Hello {
                project_id: ProjectId,
                token: String,
                instance: String,
                repo: String,
                pid: u32,
                #[serde(default)]
                lane: Option<crate::ids::LaneId>,
            },
            #[serde(other)]
            Unknown,
        }
        let sent = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("p"),
            token: "s".into(),
            instance: "i".into(),
            repo: "/r".into(),
            pid: 1,
            lane: None,
            confirms: Some(vec!["choice".into()]),
        }))
        .expect("serialises");
        let old: Envelope<BridgeFrameBeforeConfirming> =
            serde_json::from_str(&sent).expect("a hub older than confirming must still welcome it");
        assert!(
            matches!(old.payload, BridgeFrameBeforeConfirming::Hello { ref instance, .. } if instance == "i"),
            "{old:?}"
        );
    }

    #[test]
    fn a_welcome_that_names_no_generation_is_byte_for_byte_the_welcome_this_protocol_has_always_sent()
     {
        // A hub that mints no generation — which is every hub before this change, and this one on
        // a path that has not minted yet — puts exactly what it always put on the wire. The zero
        // is pinned here rather than only on a ping because `welcome` is the frame whose failure
        // is a project that can never connect, and because a hub reading a lease out of a file
        // that has never been written is the way a zero gets minted in the first place.
        let plain = env(HubFrame::Welcome {
            project: "A Title".into(),
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
        });
        let json = serde_json::to_string(&plain).expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"welcome","project":"A Title","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}"#
        );
        let zeroed = serde_json::to_string(&plain.with_generation(0)).expect("serialises");
        assert_eq!(
            zeroed, json,
            "a hub that granted a zero granted a number every fence would refuse: {zeroed}"
        );
    }

    #[test]
    fn a_welcome_carrying_a_generation_still_parses_on_a_bridge_that_has_never_heard_of_generations()
     {
        // The bridge in the operator's own session restarts only when his conversation does, so it
        // will read a welcome carrying a number it has no word for. It must still be welcomed: a
        // parse error at `welcome` is a project that can never connect. Read through an envelope
        // from before the field as well as a payload from before it — the new envelope would eat
        // the key on the way in and prove nothing about the bridge that actually has to read this.
        #[derive(Debug, PartialEq, Deserialize)]
        struct EnvelopeBeforeGenerations<P> {
            v: u16,
            id: FrameId,
            #[serde(flatten)]
            payload: P,
        }
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum HubFrameBeforeGenerations {
            Welcome {
                project: String,
                #[serde(default)]
                lane: Option<crate::ids::LaneId>,
                #[serde(default)]
                topic_id: Option<i32>,
                limits: Limits,
                #[serde(default)]
                outbox: Option<String>,
            },
            #[serde(other)]
            Unknown,
        }
        let sent = serde_json::to_string(
            &env(HubFrame::Welcome {
                project: "A Title".into(),
                lane: None,
                topic_id: None,
                limits: Limits {
                    max_frame: 65536,
                    max_text: 3500,
                    frames_per_min: 20,
                },
                outbox: None,
            })
            .with_generation(1_757_000_000_000),
        )
        .expect("serialises");
        assert_eq!(
            sent,
            r#"{"v":1,"id":"f1","generation":1757000000000,"t":"welcome","project":"A Title","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}"#
        );
        let old: EnvelopeBeforeGenerations<HubFrameBeforeGenerations> =
            serde_json::from_str(&sent).expect("a bridge older than generations must be welcomed");
        assert_eq!(old.v, VERSION);
        assert!(
            matches!(old.payload, HubFrameBeforeGenerations::Welcome { ref project, .. } if project == "A Title"),
            "{old:?}"
        );
    }

    #[test]
    fn a_bridge_that_promises_an_empty_list_has_promised_nothing() {
        // Two spellings of one meaning is how a reader ends up branching on "did it say anything"
        // instead of "did it promise THIS", and the operator is then told his tap was never
        // confirmed by a bridge that never promised to confirm it. An adapter that builds the list
        // by filtering — which is the idiom the worked example in the documents uses — writes the
        // empty one whenever it promises nothing, so this is the ordinary case, not the odd one.
        let empty = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("p"),
            token: "s".into(),
            instance: "i".into(),
            repo: "/r".into(),
            pid: 1,
            lane: None,
            confirms: Some(vec![]),
        }))
        .expect("serialises");
        assert!(
            !empty.contains("confirms"),
            "a bridge that promised nothing said something about it: {empty}"
        );
        let read: Envelope<BridgeFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f1","t":"hello","project_id":"p","token":"s","instance":"i","repo":"/r","pid":1,"confirms":[]}"#,
        )
        .expect("an empty promise must not be a parse error");
        let BridgeFrame::Hello { confirms, .. } = read.payload else {
            panic!("a hello stopped being a hello")
        };
        assert_eq!(
            confirms, None,
            "an empty list of promises read back as a promise"
        );
    }

    #[test]
    fn a_generation_of_zero_is_read_as_naming_no_generation_at_all() {
        // A bridge written in TypeScript against the document writes `welcome.generation ?? 0`
        // without thinking about it, and a zero that reaches a fence loses every comparison it is
        // ever in — so that bridge would be sent away on every run, for ever. A zero means the
        // same as silence: this peer holds no generation and is fenced by its socket and its pid.
        let read: Envelope<HubFrame> =
            serde_json::from_str(r#"{"v":1,"id":"f1","generation":0,"t":"ping"}"#)
                .expect("a zero must not be a parse error");
        assert_eq!(read.generation, None, "a zero was taken for a generation");
        let back = serde_json::to_string(&read).expect("serialises");
        assert_eq!(back, r#"{"v":1,"id":"f1","t":"ping"}"#);
        let stamped =
            serde_json::to_string(&env(HubFrame::Ping).with_generation(0)).expect("serialises");
        assert_eq!(
            stamped, r#"{"v":1,"id":"f1","t":"ping"}"#,
            "a zero was put on the wire as though it were a lease"
        );
    }

    #[test]
    fn the_number_the_hub_grants_is_on_the_welcomes_envelope_and_a_welcome_never_names_it_twice() {
        // The welcome is where the hub says which run the bridge is. If the number lived in the
        // payload as well as on the envelope, the one frame that grants the lease would carry the
        // word twice — a duplicate key serde refuses outright — and a hub that set only the
        // payload's would have it swallowed by the envelope and read back as nothing.
        let stamped = env(HubFrame::Welcome {
            project: "A Title".into(),
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
        })
        .with_generation(5);
        let json = serde_json::to_string(&stamped).expect("serialises");
        assert_eq!(
            json.matches(r#""generation""#).count(),
            1,
            "the welcome names the generation more than once: {json}"
        );
        let back: Envelope<HubFrame> =
            serde_json::from_str(&json).expect("the welcome the hub grants must be readable");
        assert_eq!(
            back.generation,
            Some(5),
            "the number the hub granted did not survive the round trip: {json}"
        );
    }

    #[test]
    fn a_generation_rides_on_every_frame_in_both_directions_and_is_named_exactly_once() {
        // The fence reads the generation off whatever frame arrived, so it has to survive every
        // one of them — and the five the other tests use carry no traffic. The count is the half
        // that matters later: the day somebody adds a payload field called `generation` to any
        // frame in either direction, that frame starts going out with the word twice and serde
        // refuses to read it back. This is the test that says so, rather than the operator.
        let bridge = vec![
            BridgeFrame::Hello {
                project_id: ProjectId::new("p"),
                token: "s".into(),
                instance: "i".into(),
                repo: "/r".into(),
                pid: 1,
                lane: None,
                confirms: Some(vec!["choice".into()]),
            },
            BridgeFrame::Say {
                text: "x".into(),
                hint: None,
                file: None,
            },
            BridgeFrame::Done {
                text: "built it".into(),
                file: None,
            },
            BridgeFrame::Ask {
                ask_id: AskId::new("a1"),
                text: "ok?".into(),
                options: None,
            },
            BridgeFrame::AskResolved {
                ask_id: AskId::new("a1"),
                how: AskEnd::Answered,
                outcome: None,
            },
            BridgeFrame::Beat {
                state: BeatState::Blocked,
                note: None,
            },
            BridgeFrame::Ack {
                r#ref: FrameId::new("f3"),
                status: AckStatus::Accepted,
                reason: None,
                files: None,
            },
            BridgeFrame::Bye {
                reason: "refresh".into(),
            },
            BridgeFrame::Pong {
                r#ref: FrameId::new("f4"),
            },
        ];
        for f in bridge {
            let json =
                serde_json::to_string(&env(f.clone()).with_generation(7)).expect("serialises");
            assert_eq!(
                json.matches(r#""generation""#).count(),
                1,
                "this frame names the generation more than once: {json}"
            );
            let back: Envelope<BridgeFrame> =
                serde_json::from_str(&json).expect("a stamped frame must be readable");
            assert_eq!(back.generation, Some(7), "the stamp was lost: {json}");
            assert_eq!(back.payload, f, "the stamp changed the frame: {json}");
        }

        let hub = vec![
            HubFrame::Welcome {
                project: "A Title".into(),
                lane: None,
                topic_id: None,
                limits: Limits {
                    max_frame: 65536,
                    max_text: 3500,
                    frames_per_min: 20,
                },
                outbox: None,
            },
            HubFrame::Refused {
                reason: RefusedReason::StaleGeneration,
            },
            HubFrame::Message {
                msg_id: MsgId::new("m1"),
                text: "hi".into(),
                from: From {
                    chat_id: -1001,
                    user_id: 7,
                },
                files: None,
                in_reply_to_ask: None,
            },
            HubFrame::Choice {
                msg_id: MsgId::new("m2"),
                ask_id: AskId::new("a1"),
                option_id: OptionId::new("y"),
            },
            HubFrame::Ack {
                r#ref: FrameId::new("f7"),
                delivered: Delivered::No,
                why: Some(AckWhy::StaleGeneration),
            },
            HubFrame::Ping,
        ];
        for f in hub {
            let json =
                serde_json::to_string(&env(f.clone()).with_generation(7)).expect("serialises");
            assert_eq!(
                json.matches(r#""generation""#).count(),
                1,
                "this frame names the generation more than once: {json}"
            );
            let back: Envelope<HubFrame> =
                serde_json::from_str(&json).expect("a stamped frame must be readable");
            assert_eq!(back.generation, Some(7), "the stamp was lost: {json}");
            assert_eq!(back.payload, f, "the stamp changed the frame: {json}");
        }
    }

    #[test]
    fn a_generation_a_bridge_reading_it_with_json_parse_could_not_hold_is_over_the_ceiling() {
        // The ceiling is not decoration: past it a bridge reads back a number that is not the one
        // it was sent, stamps that, and is fenced for ever without a single wrong-looking value
        // anywhere. Milliseconds since the epoch sit ages short of it; nanoseconds
        // are already past it, which is one edit away.
        assert_eq!(MAX_GENERATION, 9_007_199_254_740_991);
        const {
            assert!(
                1_757_000_000_000_u64 < MAX_GENERATION,
                "a generation minted from the clock in milliseconds must be safe to hold"
            )
        };
        // The first number past the ceiling that a bridge reads back as a DIFFERENT number, which
        // is the failure the ceiling exists to keep on this side of the wire.
        let too_big = MAX_GENERATION + 2;
        assert_eq!(
            (too_big as f64) as u64,
            too_big - 1,
            "a number over the ceiling must be the kind that comes back changed"
        );
    }

    #[test]
    fn a_bridge_that_promised_to_confirm_another_frame_promised_nothing_about_this_one() {
        // The reading that must be impossible to get wrong: the hub holds a tap open only for a
        // bridge that named THIS frame, never for one that merely said something. Getting it
        // wrong tells the operator his answer was not taken by a session that took it.
        assert!(promises_to_confirm(&Some(vec!["choice".into()]), "choice"));
        assert!(!promises_to_confirm(
            &Some(vec!["message".into()]),
            "choice"
        ));
        assert!(!promises_to_confirm(&None, "choice"));
        assert!(
            !promises_to_confirm(&Some(vec![]), "choice"),
            "an empty promise covered a frame"
        );
        assert!(
            !promises_to_confirm(&Some(vec!["CHOICE".into()]), "choice"),
            "a name that is not the frame's own name promised something"
        );
    }
}

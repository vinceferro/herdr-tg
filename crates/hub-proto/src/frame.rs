//! The frames, and the envelope every one of them travels in.
//!
//! # Authority flows one way
//!
//! The hub decides where a bridge's words go. A bridge decides nothing about the hub. That is why
//! no frame here carries addressing: there is no `project` field on `say`, no `chat_id` on `ask`,
//! no topic anywhere. The hub knows which connection is which project because it resolved a token
//! at `hello`, and a bridge that tries to name a project is refused.
//!
//! `hello` is the proof: it carries `instance`, and MAY carry `repo` and `pid`, for the audit log
//! and for a human reading it, and it deliberately does NOT carry a display name. The name comes
//! from the registry. A bridge that could name itself could impersonate another project's topic.
//!
//! Both are optional for the same reason the display name is absent: a path and a pid are facts
//! about one machine, and neither is who anybody is. Identity on this wire is the pair of ids the
//! hub names in `welcome` — the project and the conversation — which a bridge is TOLD and never
//! works out for itself. A bridge that filled an absent id in from its own directory would put
//! that directory back at the centre of fleet identity, which is the whole of what they remove.
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

use crate::ids::{AskId, FrameId, IdempotencyKey, IntentId, MsgId, OptionId, ProjectId, SpecId};

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
    /// direction may be named `generation`** — nor `v`, nor `id`, for exactly the same reason;
    /// `no_field_of_any_frame_shares_a_name_with_a_field_of_the_envelope_it_flattens_into` fails
    /// the day one is, over every frame in both directions, and
    /// `a_generation_rides_on_every_frame_in_both_directions_and_is_named_exactly_once` fails
    /// beside it for this field. A field whose name merely CONTAINS it — `expected_generation`,
    /// say — does not collide today and is one careless shortening away from doing so, so
    /// `no_payload_field_may_contain_the_one_name_the_envelope_owns_even_with_a_qualifier_in_front_of_it`
    /// refuses that whole family too.
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

/// The control that covers this (spec, domain, op), or `None` when the connection declared none.
///
/// The only way to read [`BridgeFrame::Hello::controls`], and it takes the whole question rather
/// than answering half of it. A reader that asks whether the field was PRESENT rather than whether
/// it names THIS operation would carry an intention to a controller that never said it could do
/// that thing — and a controller answering "I cannot" to something it never declared is the polite
/// spelling of the hub having invented the request. `promises_to_confirm` above is the same rule
/// for the same reason; this one returns the control itself because the caller also needs the
/// bound the controller set on itself.
///
/// A name this build has never heard of can never match, because the match runs through
/// [`Op::named`] rather than over the raw strings.
///
/// `None` when MORE THAN ONE entry matches, too. Two entries naming one (spec, domain, op) set two
/// different bounds on one operation, and picking either is the hub guessing which bound the
/// controller meant. It declines to guess; the controller reads the difference in the echo.
pub fn control_for<'a>(
    controls: &'a Option<Vec<Control>>,
    spec_id: &SpecId,
    domain: Option<&crate::ids::LaneId>,
    op: Op,
) -> Option<&'a Control> {
    let mut matched = controls.iter().flatten().filter(|c| {
        &c.spec_id == spec_id
            && c.domain.as_ref() == domain
            && c.allowed.iter().any(|name| Op::named(name) == Some(op))
    });
    match (matched.next(), matched.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// Reads a declaration, taking an empty list for silence. See [`BridgeFrame::Hello::controls`].
fn an_empty_declaration_is_no_declaration<'de, D>(d: D) -> Result<Option<Vec<Control>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<Control>>::deserialize(d)?.filter(|c| !c.is_empty()))
}

/// True when this peer declared no controls at all — unsaid, or the empty list that says it.
fn declares_nothing(controls: &Option<Vec<Control>>) -> bool {
    !matches!(controls, Some(c) if !c.is_empty())
}

/// One of the six things a controller can be asked to do. A CLOSED set, compiled in.
///
/// It is an enum and never a string from the wire, because a string is how an arbitrary command
/// surface begins: a hub that carried the op as text would carry whatever the sender wrote. Adding
/// a seventh is a decision, not a refactor.
///
/// **No catch-all**, like every closed set this wire branches on. `#[serde(other)]` compiles and
/// looks right and DISCARDS the wire string — measured in this repo, on herdr's own status enum —
/// so a word this build has never heard of would come back out as one it has. A controller names
/// what it can do in [`Control::allowed`], which is a list of plain strings for exactly that
/// reason: an unknown name there is dropped and the declaration survives, where inventing an op
/// from an unknown word would have the hub ask for something nobody described.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// Bring up a predeclared spec. The one op whose domain may not exist yet.
    Start,
    /// Change how many of a predeclared spec are running. The only op that carries a count.
    Scale,
    /// Stop taking new work, finish what is in hand.
    Drain,
    /// Stop.
    Stop,
    /// Stop and start again, as one operation the controller owns the meaning of.
    Restart,
    /// Report what is running. Changes nothing.
    Inspect,
}

impl Op {
    /// Every op, so a walk over them cannot silently miss the seventh the day there is one.
    pub const EVERY: [Op; 6] = [
        Op::Start,
        Op::Scale,
        Op::Drain,
        Op::Stop,
        Op::Restart,
        Op::Inspect,
    ];

    /// This op's name on the wire, which is also the name a controller declares it by.
    ///
    /// One spelling for both directions. Two would drift, and a declaration that no longer matched
    /// the frame it authorises would take capabilities away from a controller silently.
    pub fn name(self) -> &'static str {
        match self {
            Op::Start => "start",
            Op::Scale => "scale",
            Op::Drain => "drain",
            Op::Stop => "stop",
            Op::Restart => "restart",
            Op::Inspect => "inspect",
        }
    }

    /// The op a declared name means, or `None` when this build has never heard of it.
    ///
    /// `promises_to_confirm`'s rule in another spelling: a name this hub does not know is a
    /// declaration about nothing, which is the same as declaring nothing, and both fail towards
    /// offering the operator less rather than more. Refusing the connection instead would be a
    /// controller that cannot connect because it was newer than the hub.
    pub fn named(name: &str) -> Option<Op> {
        Op::EVERY.into_iter().find(|op| op.name() == name)
    }
}

/// What became of an intention, in the controller's own words about its own work.
///
/// Four rather than two, for the reason [`Delivered`] is three rather than two: a vocabulary that
/// cannot express a state the system really enters is a vocabulary that lies about it. `refused`
/// means nothing happened and nothing will; `failed` means something did happen and did not
/// finish. Collapsing those two tells the operator nothing changed when something did, which is
/// the worst thing this system can tell him, because his next action is chosen on it.
///
/// Every one of these is RELAYED by the hub and none is minted by it. There is deliberately no
/// word here for "the connection ended and I do not know what became of it": that is a fact about
/// this wire rather than about the work, and a controller that could assert it would have the hub
/// repeat to the operator a rung it never observed. [`Delivered::Unseen`] exists for the same
/// reason on the other side of the same argument.
///
/// **No catch-all** — see [`Op`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntentStatus {
    /// The work has started, and exactly one terminal status will follow.
    ///
    /// It means *started*, not *I read the frame*. Without it a controller that has begun a
    /// thirty-second restart has to choose between two lies, because the only other words it holds
    /// are "done" and "not done".
    Accepted,
    /// It is not happening, and it will not happen later. Nothing was done.
    Refused,
    /// It finished, and did what was asked.
    Completed,
    /// It started and did not finish.
    Failed,
}

/// One thing a controller says IN ADVANCE it can be asked to do.
///
/// The whole capability handshake, and deliberately the same shape as
/// [`BridgeFrame::Hello::confirms`]: a declaration belonging to the connection that made it, read
/// only by asking whether it names this exact operation.
///
/// Nothing the operator types can add to this list. He picks from what a controller already
/// declared on a connection that had already proved a secret; he never names anything. That is the
/// oldest line in this repo — inbound content SELECTS, it never NAMES — and this type is what
/// makes it structural for lifecycle work rather than a rule someone remembers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Control {
    /// Which predeclared thing this is about. Opaque here — see [`crate::ids::SpecId`].
    pub spec_id: SpecId,
    /// Which conversation of the project it is about. Absent means the conversation itself.
    ///
    /// Absence is the ONLY spelling of that meaning, exactly as it is for `hello.lane`. The hub
    /// refuses `-` as a lane name because `-` IS the project's own voice on disk in both of its
    /// file trees, so a lane admitted under that name would be handed the project's own media and
    /// outbox directories.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub domain: Option<crate::ids::LaneId>,
    /// The ops this controller can really perform for that pair, by name.
    ///
    /// Plain strings rather than [`Op`]s, so that a controller shipped after this hub is dropped a
    /// name and kept as a controller, instead of being unreadable. Read through [`control_for`],
    /// which maps a name to an op and therefore can never match one this build does not know.
    pub allowed: Vec<String>,
    /// A bound the controller sets on ITSELF, for the one op that carries a count.
    ///
    /// The hub only ever compares against it and refuses; it never raises it and never invents
    /// one. Absent means the controller named no bound, which is not the same as a bound of zero.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max: Option<u32>,
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
    /// An [`BridgeFrame::IntentOutcome`] naming an intention this hub did not send to this
    /// connection. Nothing was put in the operator's topic. Paired with [`Delivered::No`].
    ///
    /// It is an `ack` and not a [`HubFrame::Refused`] on purpose: a refusal closes the connection,
    /// and one stray outcome — a controller answering late from its own ledger after a redial, or
    /// answering for a lane that is not this one — is not a reason to end a conversation. The
    /// frame is dropped, the connection lives, and the controller is told which frame died.
    ///
    /// Safe to add to a closed set the bridge branches on, for the reason [`AckWhy::NoFile`] gives:
    /// only a peer that SENT an outcome can ever be told this, and a peer old enough not to know
    /// the word cannot have sent one.
    NoSuchIntent,
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
        /// The repo path, for the audit record and for a human reading it. Never for routing —
        /// the token beside it resolves the conversation, and it always did.
        ///
        /// Absent because a path is a LOCAL fact and not every bridge has one worth saying: inside
        /// a wall its own directory is not the hub's, and a bridge on another machine names a path
        /// that exists nowhere the hub can look. A hub that routed on this would be routing on a
        /// string the sender chose; a hub that merely logs it loses nothing when it is absent.
        ///
        /// Optional here is additive only for a NEW hub reading an OLD bridge. An old hub declares
        /// it required and refuses a hello without it before anything else happens, so every
        /// adapter this repo ships keeps naming it for one release — see
        /// `a_hub_from_before_this_change_still_requires_the_repo_and_the_pid_so_an_adapter_keeps_sending_them`.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        repo: Option<String>,
        /// This bridge's process id on the hub's own machine, when it has one to give.
        ///
        /// The hub compares it against what the kernel says about the peer, and says so in the
        /// journal when the two disagree — a debugging aid, never a decision. A bridge that is not
        /// on this machine has no pid this comparison could mean anything about, and says nothing
        /// rather than a number that would make the journal lie. Same one-release rule as `repo`.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pid: Option<u32>,
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
        /// What this connection can be ASKED to do, declared in advance — see [`Control`].
        ///
        /// A bridge that names none is not a controller and will never be sent a
        /// [`HubFrame::Intent`], which is every bridge shipped before this field existed. Read it
        /// with [`control_for`] and never by asking whether it is present: a reader that asked
        /// only whether something was declared would carry an intention to a connection that
        /// declared something else entirely.
        ///
        /// An EMPTY list says the same as silence and is read as silence, for the reason
        /// `confirms` gives above it — a controller that builds the list by filtering writes the
        /// empty one every time it can do nothing, and two spellings of one meaning is how a
        /// reader ends up branching on the wrong one.
        ///
        /// Skipped when it declares nothing, so a bridge that is not a controller puts BYTE FOR
        /// BYTE what it always put on the wire. A `"controls":null` would be a key an older hub
        /// has to tolerate for no reason at all, on the one frame whose failure is a project that
        /// can never connect.
        #[serde(
            default,
            deserialize_with = "an_empty_declaration_is_no_declaration",
            skip_serializing_if = "declares_nothing"
        )]
        controls: Option<Vec<Control>>,
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
    /// What became of a [`HubFrame::Intent`]. Every intent gets one, and then one more.
    ///
    /// The correlation is `intent_id` and not the intent frame's `ref`, because the two answer
    /// different questions: the `ack` says the frame arrived and is bookkeeping the hub does not
    /// act on, while this says what became of the WORK, arrives later, and may arrive twice —
    /// `accepted` and then one terminal status.
    ///
    /// A controller answers this from its own record. An outcome for an intention this hub never
    /// sent reaches nobody's phone: the hub carries words to the operator only for things it
    /// asked for, or a controller could put a line in his topic by naming an id.
    IntentOutcome {
        intent_id: IntentId,
        status: IntentStatus,
        /// One short sentence a person can read, in the controller's words.
        ///
        /// Never written to the audit by the hub — that file is one record per line and a
        /// sentence from another process can carry a newline, which would be a second record of
        /// the sender's choosing. It is for his topic, where a newline is only a newline.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        reason: Option<String>,
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
        /// The fleet's name for the PROJECT this conversation belongs to: the seed's own id.
        ///
        /// For a project's own voice this is the same id as [`Self::Welcome::conversation`]. For a
        /// room — a conversation minted beside a project — it is the project the room hangs off,
        /// which is the only way a reader relates the two without looking at a directory.
        ///
        /// It is here because until now the only thing a bridge learned about which conversation
        /// it was, was `project`: a display title the registry owns, that the bridge cannot
        /// predict and two projects may share. So anything that had to join two facts about one
        /// conversation joined them on the one string both ends could see — the repo path — and a
        /// path is not identity. It moves when the operator moves a directory, it differs inside a
        /// wall, and it is his own filesystem going somewhere it need not go.
        ///
        /// Opaque, exactly like every other id here. Nothing may be READ out of it, and nothing
        /// may be DERIVED for it: absence means this hub does not name its ids, never "work it out
        /// from your own path".
        ///
        /// `skip_serializing_if`, so a hub that names none puts BYTE FOR BYTE what it always put
        /// on the wire. A `"project_id":null` would be a key an older bridge has to tolerate for
        /// no reason at all, on the one frame whose failure is a project that can never connect.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        project_id: Option<ProjectId>,
        /// The id the secret resolved to: THIS conversation, whether that is a project's own voice
        /// or a room. With [`Self::Welcome::lane`] beside it, the whole of this connection's
        /// address as the hub knows it.
        ///
        /// Same rules as [`Self::Welcome::project_id`]: opaque, never derived, absent on every hub
        /// built before it and skipped when absent.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        conversation: Option<ProjectId>,
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
        /// It has to be told. Inside a wall an adapter's own `$HOME` is not the hub's, so nothing
        /// it holds can derive the path — and the two ids above do not help, because the
        /// id-shaped segments are the smaller half of a path rooted in a state directory the
        /// adapter cannot see. Absent on every hub before files,
        /// and **absence means this hub carries no files**: an adapter asked to send one then
        /// sends the words alone and says so in its own tool result, rather than sending a field
        /// the hub would strip in silence.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        outbox: Option<String>,
        /// The controls the hub actually ADMITTED, echoed back — see [`Control`].
        ///
        /// Echoed as admitted and never as sent, so the difference is readable: an op missing from
        /// the echo is one this hub does not know and will never ask for, and a control missing
        /// altogether had a shape the hub refused.
        ///
        /// It exists for the same reason the `lane` echo above it does. An unknown field inside a
        /// known kind is ignored on purpose, which is what lets a new bridge talk to an old hub —
        /// but it also means an OLD hub takes a declaration of controls in silence, and nothing
        /// else in this frame can tell that apart from having been heard. **A controller that
        /// declared controls and gets no echo has learned the hub is older than it is, and must
        /// say so in its own log and behave as an ordinary bridge**: a hub that will never send an
        /// intent looks exactly like one that has not decided to yet, and a controller that
        /// assumed the second would wait for ever while the operator's phone showed nothing.
        ///
        /// Absent means no controls were admitted, which is what every hub built before them says
        /// about everything, and it is skipped when absent so such a hub's welcome is BYTE FOR
        /// BYTE the one it always sent.
        #[serde(
            default,
            deserialize_with = "an_empty_declaration_is_no_declaration",
            skip_serializing_if = "declares_nothing"
        )]
        controls: Option<Vec<Control>>,
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
    /// One thing the operator picked off a keyboard the hub drew, carried to the controller that
    /// declared it could do that thing.
    ///
    /// This is the first frame the hub sends BECAUSE A PERSON ASKED IT TO that is not the answer
    /// to a question the far side posed — everything else the hub sends unprompted is a ping. That
    /// is a real change in the direction of authority, and it is why the field set below is closed
    /// and why every field is either one the hub itself minted or one a controller gave it back
    /// verbatim.
    ///
    /// **What it cannot carry, which is the whole point of the frame having a type at all:** an
    /// image, a command, an argv, an entrypoint, a mount, a secret, an environment variable, a
    /// host path, a port, a unit name, or any free-form map. There is no field for one, and
    /// `the_hub_carries_no_field_that_could_name_an_image_a_command_a_mount_a_secret_an_environment_variable_or_a_host_path`
    /// fails the day one is added. The hub carries a handle and a verb; whoever holds the approved
    /// spec owns everything the handle resolves to. The hub cannot look inside a spec because it
    /// has no table to resolve one against and no way to get one.
    ///
    /// Delivery is at-least-once and nothing here claims otherwise, which is what
    /// `idempotency_key` is for.
    Intent {
        /// What an [`BridgeFrame::IntentOutcome`] names. Written down before this frame goes out.
        intent_id: IntentId,
        /// What makes receiving this twice safe — see [`crate::ids::IdempotencyKey`].
        ///
        /// The failure it prevents is ordinary: the receipt is slow, he taps *Restart* again.
        /// Without the key that is two restarts, and the hub cannot tell whether the first one
        /// landed — Telegram has no idempotency key either.
        idempotency_key: IdempotencyKey,
        /// Which of the six things. A variant the hub selected from its own closed set because he
        /// tapped a button the hub drew from an admitted control — never a word from anywhere.
        op: Op,
        /// Which predeclared thing, exactly as the controller declared it. Opaque here.
        spec_id: SpecId,
        /// Which conversation of the project. Absent means the conversation itself, and absence is
        /// the only spelling of that — see [`Control::domain`].
        ///
        /// It does NOT route this frame. Routing reads the address off the claim the hub is
        /// delivering to, so a domain named here cannot retarget the frame it rides on.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        domain: Option<crate::ids::LaneId>,
        /// How many, for the one op that has a number in it. Bounded by [`Control::max`], which
        /// the controller set on itself and the hub only ever compares against.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        count: Option<u32>,
        /// Which run of the subject this was built against: the generation the operator's view was
        /// drawn from, minted by this hub and believed from nowhere else.
        ///
        /// The failure it prevents: he is shown three workers, walks away, the wall restarts, he
        /// comes back and taps *Scale to 1*. Without this that scales the NEW run to one. With it
        /// he is told what he was looking at has changed, in words, and nothing is carried.
        ///
        /// Named `for_run` and not `expected_generation` deliberately. The envelope owns
        /// `generation` (see [`Envelope::generation`]) and a payload field of that name is a
        /// duplicate key no reader can read; a name one qualifier away from the reserved one is
        /// one careless shortening from that failure, on the frame whose whole job is fencing.
        /// `no_payload_field_may_contain_the_one_name_the_envelope_owns_even_with_a_qualifier_in_front_of_it`
        /// now refuses the whole family of names rather than the one word.
        ///
        /// Absent for the ops that have no run to be about, present for the ones that change
        /// something already running. Which is which is the hub's rule, not this crate's.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        for_run: Option<u64>,
        /// Who tapped, for the audit record. The same type and the same spelling the hub already
        /// puts on every [`HubFrame::Message`]: one type, one spelling in the log, no new exposure.
        from: From,
        /// How long the hub is willing for this to be acted on, as a DURATION from arrival.
        ///
        /// Not an instant. A deadline compares two clocks, and the two ends of this wire are two
        /// processes that have never agreed on one; a duration compares one clock with itself.
        valid_for_ms: u64,
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

    /// The envelope's own field names, which its payload flattens in beside.
    ///
    /// Read by `no_field_of_any_frame_shares_a_name_with_a_field_of_the_envelope_it_flattens_into`.
    const ENVELOPE_FIELDS: [&str; 3] = ["v", "id", "generation"];

    /// One of every frame a bridge can send, with every optional field filled.
    ///
    /// ONE list, shared by the two tests that have to walk all of them — the generation stamp and
    /// the envelope-name guard. Two copies would drift, and a frame missing from either list is a
    /// frame whose collision nobody finds until a peer cannot read it.
    fn every_bridge_frame() -> Vec<BridgeFrame> {
        vec![
            BridgeFrame::Hello {
                project_id: ProjectId::new("p"),
                token: "s".into(),
                instance: "i".into(),
                repo: Some("/r".into()),
                pid: Some(1),
                lane: Some(crate::ids::LaneId::new("engineering")),
                confirms: Some(vec!["choice".into()]),
                controls: Some(vec![one_control()]),
            },
            BridgeFrame::Say {
                text: "x".into(),
                hint: Some(SayHint::Prose),
                file: Some(SayFile {
                    name: "3c9e1b7a.png".into(),
                    mime: Some("image/png".into()),
                    filename: Some("chart.png".into()),
                    r#as: Some(FileAs::Document),
                }),
            },
            BridgeFrame::Done {
                text: "built it".into(),
                file: None,
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
                note: Some("waiting".into()),
            },
            BridgeFrame::Ack {
                r#ref: FrameId::new("f3"),
                status: AckStatus::Accepted,
                reason: Some("busy".into()),
                files: Some(1),
            },
            BridgeFrame::Bye {
                reason: "refresh".into(),
            },
            BridgeFrame::Pong {
                r#ref: FrameId::new("f4"),
            },
            BridgeFrame::IntentOutcome {
                intent_id: IntentId::new("i-7a1c"),
                status: IntentStatus::Completed,
                reason: Some("three are running".into()),
            },
        ]
    }

    /// One of every frame the hub can send, with every optional field filled. See
    /// [`every_bridge_frame`] for why there is only one list.
    fn every_hub_frame() -> Vec<HubFrame> {
        vec![
            HubFrame::Welcome {
                project: "A Title".into(),
                project_id: Some(ProjectId::new("p-9f3a1c2e5b7d")),
                conversation: Some(ProjectId::new("c-4d1e6b0a7c22")),
                lane: Some(crate::ids::LaneId::new("engineering")),
                topic_id: Some(41),
                limits: Limits {
                    max_frame: 65536,
                    max_text: 3500,
                    frames_per_min: 20,
                },
                outbox: Some("/state/outbox/p-9f3a1c2e5b7d/engineering".into()),
                controls: Some(vec![one_control()]),
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
                in_reply_to_ask: Some(AskId::new("a1")),
                files: Some(vec![MessageFile {
                    kind: FileKind::Photo,
                    path: Some("/state/media/p-9f3a1c2e5b7d/-/shot.jpg".into()),
                    mime: Some("image/jpeg".into()),
                    bytes: Some(1024),
                    filename: Some("shot.jpg".into()),
                    why: None,
                }]),
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
            an_intent_with_every_field(),
        ]
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
            repo: Some("/home/u/Projects/herdr-tg".into()),
            pid: Some(42),
            lane: Some(crate::ids::LaneId::new("lane-0902-201212-2783563")),
            confirms: None,
            controls: None,
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
            repo: Some("/home/u/Projects/herdr-tg".into()),
            pid: Some(42),
            lane: None,
            confirms: None,
            controls: None,
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
                repo: Some("/r".into()),
                pid: Some(1),
                lane: None,
                confirms: None,
                controls: None,
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
            project_id: None,
            conversation: None,
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
            controls: None,
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
            project_id: None,
            conversation: None,
            lane: Some(crate::ids::LaneId::new("engineering")),
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: Some("/state/outbox/p-9f3a1c2e5b7d/engineering".into()),
            controls: None,
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
                repo: Some("/home/u/Projects/herdr-tg".into()),
                pid: Some(42),
                lane: None,
                confirms,
                controls: None,
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
            repo: Some("/r".into()),
            pid: Some(1),
            lane: None,
            confirms: Some(vec!["choice".into()]),
            controls: None,
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
            repo: Some("/r".into()),
            pid: Some(1),
            lane: None,
            confirms: Some(vec!["choice".into()]),
            controls: None,
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
            project_id: None,
            conversation: None,
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
            controls: None,
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
                project_id: None,
                conversation: None,
                lane: None,
                topic_id: None,
                limits: Limits {
                    max_frame: 65536,
                    max_text: 3500,
                    frames_per_min: 20,
                },
                outbox: None,
                controls: None,
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
            repo: Some("/r".into()),
            pid: Some(1),
            lane: None,
            confirms: Some(vec![]),
            controls: None,
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
            project_id: None,
            conversation: None,
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
            controls: None,
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
        let bridge = every_bridge_frame();
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

        let hub = every_hub_frame();
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

    // ── fleet identity ────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_welcome_names_the_project_and_the_conversation_by_id_so_nothing_has_to_join_on_a_path() {
        // What a bridge could learn about which conversation it is, before this: `project`, a
        // display title the registry owns and the bridge cannot predict. So anything that had to
        // relate two facts about one conversation related them on the only thing both sides could
        // see — the repo path in its own `hello`. A path is not identity: it moves, it differs
        // inside a wall, and it is the operator's own directory going somewhere it need not go.
        // These two are the fleet's names for it: the seed's id, and the id the secret resolved to.
        let json = serde_json::to_string(&env(HubFrame::Welcome {
            project: "A Title".into(),
            project_id: Some(ProjectId::new("p-9f3a1c2e5b7d")),
            conversation: Some(ProjectId::new("c-4d1e6b0a7c22")),
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
            controls: None,
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"welcome","project":"A Title","project_id":"p-9f3a1c2e5b7d","conversation":"c-4d1e6b0a7c22","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}"#
        );
        let back: Envelope<HubFrame> =
            serde_json::from_str(&json).expect("the welcome that admits a bridge must be readable");
        let HubFrame::Welcome {
            project_id,
            conversation,
            ..
        } = back.payload
        else {
            panic!("a welcome stopped being a welcome")
        };
        assert_eq!(project_id, Some(ProjectId::new("p-9f3a1c2e5b7d")));
        assert_eq!(conversation, Some(ProjectId::new("c-4d1e6b0a7c22")));
    }

    #[test]
    fn a_welcome_that_names_no_ids_is_byte_for_byte_the_welcome_this_protocol_has_always_sent() {
        // Every hub built before these fields, and every path inside this one that cannot work out
        // a seed, puts exactly this on the wire. Pinned as BYTES rather than as a round trip,
        // because a round trip stays green when a `"project_id":null` has appeared — two keys an
        // older bridge would have to tolerate for no reason at all, on the one frame whose failure
        // is a project that can never connect.
        let json = serde_json::to_string(&env(HubFrame::Welcome {
            project: "A Title".into(),
            project_id: None,
            conversation: None,
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
            controls: None,
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"welcome","project":"A Title","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}"#
        );
    }

    #[test]
    fn a_welcome_naming_its_ids_still_parses_on_a_bridge_that_has_never_heard_of_them() {
        // The bridge in the operator's own session restarts only when his conversation does, so it
        // will read a welcome carrying two words it has no field for. It must go on being welcomed
        // — a parse error at `welcome` is a project that can never connect. Modelled as a TYPE:
        // `welcome` exactly as this crate shipped it before the ids, same tag, same envelope, same
        // flatten, so the tolerance is read off the shape the old bridge actually has.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum HubFrameBeforeIds {
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
        let sent = serde_json::to_string(&env(HubFrame::Welcome {
            project: "A Title".into(),
            project_id: Some(ProjectId::new("p-9f3a1c2e5b7d")),
            conversation: Some(ProjectId::new("c-4d1e6b0a7c22")),
            lane: Some(crate::ids::LaneId::new("engineering")),
            topic_id: Some(41),
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
            controls: None,
        }))
        .expect("serialises");
        let old: Envelope<HubFrameBeforeIds> = serde_json::from_str(&sent)
            .expect("a bridge older than the ids must still be welcomed");
        assert!(
            matches!(old.payload, HubFrameBeforeIds::Welcome { ref project, ref lane, .. }
                if project == "A Title" && lane.as_ref().is_some_and(|l| l.as_str() == "engineering")),
            "{old:?}"
        );
    }

    #[test]
    fn a_welcome_from_a_hub_that_names_no_ids_reads_back_as_naming_neither_rather_than_guessing_one()
     {
        // The other direction of the same skew, and the one that decides how a new bridge must be
        // written: absence means "this hub does not know its own ids", never "the conversation is
        // whatever I can work out from my own path". A bridge that filled the gap from a path
        // would put the operator's directory back at the centre of fleet identity, which is the
        // whole of what this change removes.
        let read: Envelope<HubFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f1","t":"welcome","project":"A Title","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}"#,
        )
        .expect("the welcome every hub has always sent must not be a parse error");
        assert_eq!(
            read.payload,
            HubFrame::Welcome {
                project: "A Title".into(),
                project_id: None,
                conversation: None,
                lane: None,
                topic_id: None,
                limits: Limits {
                    max_frame: 65536,
                    max_text: 3500,
                    frames_per_min: 20,
                },
                outbox: None,
                controls: None,
            }
        );
    }

    #[test]
    fn a_hello_that_names_no_repo_and_no_pid_is_a_hello_that_named_neither_and_not_a_parse_error() {
        // A path and a pid are LOCAL facts. The hub never routed on either — the token resolves
        // the conversation and the socket proves the peer — so a bridge that has neither to give,
        // because it is inside a wall or on another machine, must still be able to say hello.
        // Neither key goes on the wire when it has nothing to put there: a `"repo":null` is a key
        // an older hub would have to tolerate for nothing, on the frame whose failure is a project
        // that can never connect.
        let json = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("unknown-until-the-hub-says"),
            token: "s3cret".into(),
            instance: "i1".into(),
            repo: None,
            pid: None,
            lane: None,
            confirms: None,
            controls: None,
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"hello","project_id":"unknown-until-the-hub-says","token":"s3cret","instance":"i1"}"#
        );
        let read: Envelope<BridgeFrame> = serde_json::from_str(&json).expect("round trips");
        assert_eq!(
            read.payload,
            BridgeFrame::Hello {
                project_id: ProjectId::new("unknown-until-the-hub-says"),
                token: "s3cret".into(),
                instance: "i1".into(),
                repo: None,
                pid: None,
                lane: None,
                confirms: None,
                controls: None,
            }
        );
    }

    #[test]
    fn a_hello_that_still_names_its_repo_and_its_pid_is_byte_for_byte_the_hello_it_always_was() {
        // Optional is not gone. Every adapter this repo ships keeps naming both for one release,
        // and the bytes it puts on the wire must not move by a single character while it does —
        // a hub older than this change reads them as required fields.
        let json = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("unknown-until-the-hub-says"),
            token: "s3cret".into(),
            instance: "i1".into(),
            repo: Some("/srv/projects/herdr-tg".into()),
            pid: Some(42),
            lane: None,
            confirms: None,
            controls: None,
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"hello","project_id":"unknown-until-the-hub-says","token":"s3cret","instance":"i1","repo":"/srv/projects/herdr-tg","pid":42}"#
        );
        // And the hello already on every wire today reads back as one that named both, never as
        // one that named neither.
        let read: Envelope<BridgeFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f2","t":"hello","project_id":"p","token":"s","instance":"i","repo":"/r","pid":1}"#,
        )
        .expect("the hello every bridge has always sent must not be a parse error");
        assert_eq!(
            read.payload,
            BridgeFrame::Hello {
                project_id: ProjectId::new("p"),
                token: "s".into(),
                instance: "i".into(),
                repo: Some("/r".into()),
                pid: Some(1),
                lane: None,
                confirms: None,
                controls: None,
            }
        );
    }

    #[test]
    fn a_hub_from_before_this_change_still_requires_the_repo_and_the_pid_so_an_adapter_keeps_sending_them()
     {
        // The asymmetry that decides the rollout order, pinned so nobody drops the two fields from
        // an adapter a release early. Making them optional here is additive for a NEW hub reading
        // an OLD bridge; it is not additive the other way — an old hub declares both required, and
        // a hello without them is refused before anything else can happen, leaving the operator a
        // project that will not connect and a message about a secret that is perfectly good.
        // Modelled as a TYPE: `hello` exactly as this crate shipped it, both required.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum BridgeFrameBeforeIds {
            Hello {
                project_id: ProjectId,
                token: String,
                instance: String,
                repo: String,
                pid: u32,
                #[serde(default)]
                lane: Option<crate::ids::LaneId>,
                #[serde(default)]
                confirms: Option<Vec<String>>,
            },
            #[serde(other)]
            Unknown,
        }
        let naming_both = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("p"),
            token: "s".into(),
            instance: "i".into(),
            repo: Some("/r".into()),
            pid: Some(1),
            lane: None,
            confirms: Some(vec!["choice".into()]),
            controls: None,
        }))
        .expect("serialises");
        let old: Envelope<BridgeFrameBeforeIds> = serde_json::from_str(&naming_both)
            .expect("an old hub must still admit an adapter that names both");
        assert!(
            matches!(old.payload, BridgeFrameBeforeIds::Hello { ref instance, .. } if instance == "i"),
            "{old:?}"
        );

        let naming_neither = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("p"),
            token: "s".into(),
            instance: "i".into(),
            repo: None,
            pid: None,
            lane: None,
            confirms: Some(vec!["choice".into()]),
            controls: None,
        }))
        .expect("serialises");
        let refused = serde_json::from_str::<Envelope<BridgeFrameBeforeIds>>(&naming_neither);
        assert!(
            refused.is_err(),
            "an old hub read a hello that named no repo, so an adapter could drop it early: {refused:?}"
        );
    }

    #[test]
    fn no_field_of_any_frame_shares_a_name_with_a_field_of_the_envelope_it_flattens_into() {
        // The collision this crate has now met twice: ping and pong named their nonce `id`, and
        // the generation could only ever live on the envelope. `flatten` puts the payload's keys
        // and the envelope's in ONE object, so a name used by both is one key, not two — a sender
        // that fills both emits a duplicate key serde refuses outright, and a sender that fills
        // only the payload's has it swallowed on the way in and read back as nothing. Neither
        // failure looks wrong until a round trip, which is why the guard is over EVERY frame in
        // both directions rather than over the two that have already been caught by it.
        for f in every_bridge_frame() {
            let payload = serde_json::to_value(&f).expect("serialises");
            for name in ENVELOPE_FIELDS {
                assert!(
                    payload.get(name).is_none(),
                    "this frame has a field named after the envelope's own `{name}`: {payload}"
                );
            }
        }
        for f in every_hub_frame() {
            let payload = serde_json::to_value(&f).expect("serialises");
            for name in ENVELOPE_FIELDS {
                assert!(
                    payload.get(name).is_none(),
                    "this frame has a field named after the envelope's own `{name}`: {payload}"
                );
            }
        }
    }

    // ── the lifecycle-intent contract ─────────────────────────────────────────────────────────

    /// A controller's declaration, as the worked example in the document writes it.
    ///
    /// One helper, so the pins below cannot drift apart from each other into two ideas of what a
    /// declaration looks like.
    fn one_control() -> Control {
        Control {
            spec_id: SpecId::new("spec-worker"),
            domain: Some(crate::ids::LaneId::new("engineering")),
            allowed: vec![
                "start".into(),
                "scale".into(),
                "drain".into(),
                "stop".into(),
            ],
            max: Some(4),
        }
    }

    /// An intention with every optional field filled, which is what makes the closed-field-set
    /// guard below non-vacuous: a field that only appears when it has a value cannot hide from it.
    ///
    /// "Every optional field filled" is not a promise this literal can keep on its own — `None` is
    /// a legal answer to a new field and the compiler accepts it here without a word. The guard
    /// destructures what this returns, naming every field and requiring `Some` of each optional
    /// one, so the day one is added the decision is forced beside the key list rather than here.
    fn an_intent_with_every_field() -> HubFrame {
        HubFrame::Intent {
            intent_id: IntentId::new("i-7a1c"),
            idempotency_key: IdempotencyKey::new("k-3f9e2b18"),
            op: Op::Scale,
            spec_id: SpecId::new("spec-worker"),
            domain: Some(crate::ids::LaneId::new("engineering")),
            count: Some(3),
            for_run: Some(1_757_000_000_098),
            from: From {
                chat_id: -1001,
                user_id: 7,
            },
            valid_for_ms: 120_000,
        }
    }

    /// The answer coming back, with its one optional field filled, for the same reason.
    fn an_outcome_with_every_field() -> BridgeFrame {
        BridgeFrame::IntentOutcome {
            intent_id: IntentId::new("i-7a1c"),
            status: IntentStatus::Failed,
            reason: Some("the wall would not come up".into()),
        }
    }

    #[test]
    fn a_hello_without_controls_is_byte_for_byte_the_hello_this_protocol_has_always_sent() {
        // Every bridge on the box is an ordinary bridge and not a controller, and a channel plugin
        // restarts only when the operator's session does — so the hello this field appears on is
        // the one nobody will re-issue for days. Pinned as BYTES rather than as a round trip,
        // because a round trip stays green when a `"controls":null` or a `"controls":[]` has
        // appeared, and either would be a key an older hub has to tolerate for no reason at all,
        // on the one frame whose failure is a project that can never connect. Both spellings of
        // "I am not a controller" are pinned to the same bytes.
        let hello = |controls| {
            serde_json::to_string(&env(BridgeFrame::Hello {
                project_id: ProjectId::new("unknown-until-the-hub-says"),
                token: "s3cret".into(),
                instance: "i1".into(),
                repo: Some("/home/u/Projects/herdr-tg".into()),
                pid: Some(42),
                lane: None,
                confirms: None,
                controls,
            }))
            .expect("serialises")
        };
        let always = r#"{"v":1,"id":"f1","t":"hello","project_id":"unknown-until-the-hub-says","token":"s3cret","instance":"i1","repo":"/home/u/Projects/herdr-tg","pid":42}"#;
        assert_eq!(hello(None), always);
        assert_eq!(
            hello(Some(vec![])),
            always,
            "a bridge that is not a controller still said something about controlling"
        );
    }

    #[test]
    fn a_welcome_without_controls_is_byte_for_byte_the_welcome_this_protocol_has_always_sent() {
        // The other half, and the more expensive one to get wrong: `welcome` is the frame whose
        // failure is a project that can never connect, and every bridge in the fleet reads one
        // before it can do anything at all.
        let welcome = |controls| {
            serde_json::to_string(&env(HubFrame::Welcome {
                project: "A Title".into(),
                project_id: None,
                conversation: None,
                lane: None,
                topic_id: None,
                limits: Limits {
                    max_frame: 65536,
                    max_text: 3500,
                    frames_per_min: 20,
                },
                outbox: None,
                controls,
            }))
            .expect("serialises")
        };
        let always = r#"{"v":1,"id":"f1","t":"welcome","project":"A Title","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}"#;
        assert_eq!(welcome(None), always);
        assert_eq!(
            welcome(Some(vec![])),
            always,
            "a hub that admitted no controls still put an echo on the wire"
        );
    }

    #[test]
    fn a_controller_that_declares_an_empty_list_of_controls_has_declared_nothing() {
        // The `confirms` failure in a new place: an adapter that builds the list by filtering
        // writes the empty one every time it can do nothing, so the empty list is the ordinary
        // case rather than the odd one. Two spellings of one meaning is how a reader ends up
        // branching on "did it say anything about controls" instead of "can it do THIS", and the
        // second is the only question worth asking.
        let read: Envelope<BridgeFrame> = serde_json::from_str(
            r#"{"v":1,"id":"f1","t":"hello","project_id":"p","token":"s","instance":"i","controls":[]}"#,
        )
        .expect("an empty declaration must not be a parse error");
        let BridgeFrame::Hello { controls, .. } = read.payload else {
            panic!("a hello stopped being a hello")
        };
        assert_eq!(
            controls, None,
            "an empty list of controls read back as a declaration"
        );
    }

    #[test]
    fn a_hello_that_declares_what_it_can_be_asked_to_do_carries_it_on_the_wire() {
        // The capability handshake has to be ON the wire, for the reason `confirms` is: the only
        // alternative is inferring it from a version number nobody sends, and a hub that assumed
        // would sooner or later offer the operator a button for something nothing can do.
        let json = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("p"),
            token: "s".into(),
            instance: "i".into(),
            repo: None,
            pid: None,
            lane: None,
            confirms: Some(vec!["choice".into()]),
            controls: Some(vec![one_control()]),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"hello","project_id":"p","token":"s","instance":"i","confirms":["choice"],"controls":[{"spec_id":"spec-worker","domain":"engineering","allowed":["start","scale","drain","stop"],"max":4}]}"#
        );
    }

    #[test]
    fn a_welcome_echoes_the_controls_the_hub_admitted_so_a_controller_can_tell_it_was_heard() {
        // Without the echo, a controller cannot tell a hub that will never ask it for anything
        // from one that has not asked yet — an unknown field inside a known kind is ignored on
        // purpose, so an older hub takes the whole declaration in silence. A controller that got
        // no echo must behave as an ordinary bridge rather than waiting for an intent that is
        // never coming.
        let json = serde_json::to_string(&env(HubFrame::Welcome {
            project: "A Title".into(),
            project_id: None,
            conversation: None,
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
            controls: Some(vec![one_control()]),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"welcome","project":"A Title","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20},"controls":[{"spec_id":"spec-worker","domain":"engineering","allowed":["start","scale","drain","stop"],"max":4}]}"#
        );

        let older: Envelope<HubFrame> = serde_json::from_str(
            r#"{"v":1,"id":"h1","t":"welcome","project":"A Title","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}"#,
        )
        .expect("a welcome from a hub older than controls must still parse");
        let HubFrame::Welcome { controls, .. } = older.payload else {
            panic!("a welcome stopped being a welcome")
        };
        assert_eq!(
            controls, None,
            "an older hub's silence read back as an echo, so a controller would have acted"
        );
    }

    #[test]
    fn an_intent_puts_a_handle_and_a_verb_on_the_wire_and_never_the_thing_the_handle_names() {
        // The shape a stranger implements from, pinned as bytes. `spec_id` is a handle the hub
        // cannot dereference — it holds no table of specs and has no way to get one — so the image,
        // the command, the mounts and the environment stay entirely on the side that approved them.
        let json = serde_json::to_string(
            &Envelope::new(FrameId::new("h44"), an_intent_with_every_field())
                .with_generation(1_757_000_000_123),
        )
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"h44","generation":1757000000123,"t":"intent","intent_id":"i-7a1c","idempotency_key":"k-3f9e2b18","op":"scale","spec_id":"spec-worker","domain":"engineering","count":3,"for_run":1757000000098,"from":{"chat_id":-1001,"user_id":7},"valid_for_ms":120000}"#
        );

        // The other end of the same frame: an op with no run to be about and no count, in a
        // conversation named by its absence rather than by a word. Absence is the only spelling —
        // the hub refuses `-` as a lane name, because `-` IS the project's own voice on disk.
        let starting = serde_json::to_string(&env(HubFrame::Intent {
            intent_id: IntentId::new("i-9b02"),
            idempotency_key: IdempotencyKey::new("k-11ff"),
            op: Op::Start,
            spec_id: SpecId::new("spec-coordinator"),
            domain: None,
            count: None,
            for_run: None,
            from: From {
                chat_id: -1001,
                user_id: 7,
            },
            valid_for_ms: 120_000,
        }))
        .expect("serialises");
        assert_eq!(
            starting,
            r#"{"v":1,"id":"f1","t":"intent","intent_id":"i-9b02","idempotency_key":"k-11ff","op":"start","spec_id":"spec-coordinator","from":{"chat_id":-1001,"user_id":7},"valid_for_ms":120000}"#
        );
    }

    #[test]
    fn an_intent_outcome_names_the_intention_it_is_about_and_carries_nothing_else() {
        // One correlation and not two. The `ack` for the intent frame says the frame arrived and
        // is bookkeeping; THIS says what became of the work, and joining the two on one id is what
        // keeps there being one place to look an intention up rather than two, one of which wins.
        let accepted = serde_json::to_string(&env(BridgeFrame::IntentOutcome {
            intent_id: IntentId::new("i-7a1c"),
            status: IntentStatus::Accepted,
            reason: None,
        }))
        .expect("serialises");
        assert_eq!(
            accepted,
            r#"{"v":1,"id":"f1","t":"intent_outcome","intent_id":"i-7a1c","status":"accepted"}"#
        );

        let done = serde_json::to_string(&env(BridgeFrame::IntentOutcome {
            intent_id: IntentId::new("i-7a1c"),
            status: IntentStatus::Completed,
            reason: Some("three are running".into()),
        }))
        .expect("serialises");
        assert_eq!(
            done,
            r#"{"v":1,"id":"f1","t":"intent_outcome","intent_id":"i-7a1c","status":"completed","reason":"three are running"}"#
        );
    }

    #[test]
    fn an_intent_and_its_outcome_carry_the_generation_exactly_once_and_name_no_field_the_envelope_owns()
     {
        // The two guards that walk every frame are worth exactly as much as the lists they walk,
        // and a frame missing from a list is a frame nobody checks — which is invisible, because
        // both guards stay green over a shorter list. So membership is asserted here, by the test
        // whose subject is the new frames, rather than left to whoever adds the next one.
        assert!(
            every_hub_frame()
                .iter()
                .any(|f| matches!(f, HubFrame::Intent { .. })),
            "the intent is not in the list the envelope guards walk, so nothing guards it"
        );
        assert!(
            every_bridge_frame()
                .iter()
                .any(|f| matches!(f, BridgeFrame::IntentOutcome { .. })),
            "the outcome is not in the list the envelope guards walk, so nothing guards it"
        );

        let intent = serde_json::to_string(&env(an_intent_with_every_field()).with_generation(7))
            .expect("serialises");
        assert_eq!(
            intent.matches(r#""generation""#).count(),
            1,
            "the frame that fences a run named the generation more than once: {intent}"
        );
        let back: Envelope<HubFrame> =
            serde_json::from_str(&intent).expect("a stamped intent must be readable");
        assert_eq!(back.generation, Some(7), "the stamp was lost: {intent}");
        assert_eq!(
            back.payload,
            an_intent_with_every_field(),
            "the stamp changed the intention: {intent}"
        );
    }

    #[test]
    fn a_status_this_build_has_never_heard_of_is_one_unreadable_frame_and_never_a_fifth_status() {
        // `#[serde(other)]` compiles, looks right, and DISCARDS the wire string — measured in this
        // repo on herdr's own status enum, where an unmodelled word re-serialised as the literal
        // name of the catch-all. On this frame that would be worse than a lost string: the hub
        // would hold a value it could branch on, for a state a controller never claimed, and go on
        // to tell the operator a rung nobody observed. A word this build does not know must not
        // become a value at all. What a reader does with the decode failure is the reader's
        // decision; what this crate guarantees is that there is no fifth status to act on.
        let unreadable = serde_json::from_str::<Envelope<BridgeFrame>>(
            r#"{"v":1,"id":"f1","t":"intent_outcome","intent_id":"i-1","status":"partly"}"#,
        );
        assert!(
            unreadable.is_err(),
            "a status nobody defined became a status this hub could act on: {unreadable:?}"
        );

        // The same rule downward, where getting it wrong would have the hub ask for an operation
        // it has no word for.
        let no_such_op = serde_json::from_str::<Envelope<HubFrame>>(
            r#"{"v":1,"id":"h1","t":"intent","intent_id":"i-1","idempotency_key":"k-1","op":"delete","spec_id":"s","from":{"chat_id":-1,"user_id":1},"valid_for_ms":1}"#,
        );
        assert!(
            no_such_op.is_err(),
            "an op nobody defined became an op: {no_such_op:?}"
        );
    }

    #[test]
    fn no_payload_field_may_contain_the_one_name_the_envelope_owns_even_with_a_qualifier_in_front_of_it()
     {
        // Its neighbour above pins the EXACT names, which is the collision that actually breaks a
        // reader today. This one is about the next one: `expected_generation` does not collide, and
        // is one careless shortening away from a duplicate key nobody can read, on the frames whose
        // whole job is fencing. The guard is deliberately asymmetric — a substring rule for
        // `generation` alone, and exact match for the other two — because `_id` is a suffix half
        // this wire's fields legitimately carry (`msg_id`, `ask_id`, `option_id`, `intent_id`) and
        // `v` is a single letter that appears inside `valid_for_ms`. A rule that cried wolf on
        // either would be deleted by the first person it inconvenienced. `generation` is a
        // nine-letter word no field has another reason to contain, and it is the one whose
        // shadowing is silent.
        const THE_NAME_THE_ENVELOPE_OWNS: &str = "generation";
        let mut walked = 0;
        for payload in every_bridge_frame()
            .iter()
            .map(|f| serde_json::to_value(f).expect("serialises"))
            .chain(
                every_hub_frame()
                    .iter()
                    .map(|f| serde_json::to_value(f).expect("serialises")),
            )
        {
            for name in payload
                .as_object()
                .expect("every frame is one flat object")
                .keys()
            {
                walked += 1;
                assert!(
                    !name.contains(THE_NAME_THE_ENVELOPE_OWNS),
                    "the payload field `{name}` is one rename away from shadowing the \
                     envelope's own `{THE_NAME_THE_ENVELOPE_OWNS}`: {payload}"
                );
            }
        }
        assert!(
            walked > 0,
            "the guard walked no fields at all, which is how a scan passes vacuously"
        );
    }

    #[test]
    fn the_hub_carries_no_field_that_could_name_an_image_a_command_a_mount_a_secret_an_environment_variable_or_a_host_path()
     {
        // The line the whole contract turns on, held as a property of the TYPES rather than as a
        // promise in a document. The hub carries a handle and a verb; everything the handle
        // resolves to — the image, the command, the mounts, the secrets, the environment, the host
        // paths — belongs to whoever approved the spec, and there is no field here for any of it.
        //
        // Three halves, and the first two are what stop the third passing vacuously.
        //
        // ONE, at COMPILE TIME: the three patterns below name every field of the three shapes this
        // contract adds, and require `Some` of every optional one. Without them the scan is blind
        // to exactly the change that most needs watching — an optional field carrying
        // `skip_serializing_if`, which is the house style for every optional field on this wire. A
        // new one is absent from `to_value` whenever the fixture answers it `None`, and `None` is
        // the free answer the compiler drags an author to when it makes them name the field. That
        // was measured, not imagined: an `env_for_the_run: Option<String>` planted on the intent
        // put a database URL with its password on the wire with this whole suite green.
        //
        // TWO, the KEY SET, asserted exactly over what those patterns just proved complete, so a
        // typo in the list cannot quietly check nothing. THREE, each key's SHAPE: a string that is
        // an opaque id or a variant of a hub-owned enum, or a bounded number — never an array,
        // never a nested map, because one free-form map defeats every row of this at once.
        let HubFrame::Intent {
            intent_id: _,
            idempotency_key: _,
            op: _,
            spec_id: _,
            domain: Some(_),
            count: Some(_),
            for_run: Some(_),
            from: _,
            valid_for_ms: _,
        } = an_intent_with_every_field()
        else {
            panic!("the fixture stopped being an intent with every optional field filled")
        };
        let Control {
            spec_id: _,
            domain: Some(_),
            allowed: _,
            max: Some(_),
        } = one_control()
        else {
            panic!("the declaration fixture stopped filling every optional field")
        };
        let BridgeFrame::IntentOutcome {
            intent_id: _,
            status: _,
            reason: Some(_),
        } = an_outcome_with_every_field()
        else {
            panic!("the outcome fixture stopped filling every optional field")
        };

        let forbidden = [
            "image",
            "img",
            "digest",
            "tag",
            "registry",
            "blob",
            "command",
            "cmd",
            "argv",
            "arg",
            "exec",
            "entrypoint",
            "shell",
            "script",
            "mount",
            "volume",
            "bind",
            "secret",
            "token",
            "credential",
            "password",
            "env",
            "path",
            "dir",
            "cwd",
            "url",
            "host",
            "port",
            "socket",
            "unit",
            "user_data",
            "extra",
            "meta",
        ];

        let intent = serde_json::to_value(an_intent_with_every_field()).expect("serialises");
        let intent = intent.as_object().expect("a frame is one flat object");
        let mut keys: Vec<&str> = intent.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "count",
                "domain",
                "for_run",
                "from",
                "idempotency_key",
                "intent_id",
                "op",
                "spec_id",
                "t",
                "valid_for_ms",
            ],
            "the field set of an intent is CLOSED. Adding one is a decision about what this hub \
             is allowed to carry, and this list is where that decision is made."
        );
        for (name, value) in intent {
            for word in forbidden {
                assert!(
                    !name.contains(word),
                    "`{name}` on an intent could name the thing itself rather than a handle to it"
                );
            }
            match name.as_str() {
                // Opaque strings the hub minted or echoed back verbatim, plus the kind. Nothing is
                // read out of any of them here; `spec_id` in particular is a handle this hub has
                // no table to resolve and no way to get one.
                "t" | "intent_id" | "idempotency_key" | "spec_id" | "domain" => {
                    assert!(value.is_string(), "`{name}` stopped being an opaque string")
                }
                // A variant of a set compiled into this binary, never a word from the wire.
                "op" => assert!(
                    Op::EVERY.iter().any(|o| value == o.name()),
                    "`op` carried something that is not one of this hub's own six verbs: {value}"
                ),
                // Numbers, bounded by their own types.
                "count" | "for_run" | "valid_for_ms" => {
                    assert!(value.is_number(), "`{name}` stopped being a number")
                }
                // The one nested object, and it is the type the hub already puts on every message:
                // who tapped, for the audit. Its own field set is pinned here too, because a map
                // that grew a third field would be a map that could grow a fourth.
                "from" => {
                    let mut inner: Vec<&str> = value
                        .as_object()
                        .expect("`from` is an object")
                        .keys()
                        .map(String::as_str)
                        .collect();
                    inner.sort_unstable();
                    assert_eq!(inner, ["chat_id", "user_id"], "`from` grew a field");
                }
                other => panic!("`{other}` is a field of an intent that nothing here classifies"),
            }
        }

        // The declaration the intent is built from, both directions: it rides up on a `hello` and
        // the hub echoes it back down on a `welcome`, so a free-form field here would be one the
        // hub carries too.
        let control = serde_json::to_value(one_control()).expect("serialises");
        let control = control.as_object().expect("a control is an object");
        let mut declared: Vec<&str> = control.keys().map(String::as_str).collect();
        declared.sort_unstable();
        assert_eq!(
            declared,
            ["allowed", "domain", "max", "spec_id"],
            "the field set of a control is CLOSED for the same reason an intent's is"
        );
        for name in control.keys() {
            for word in forbidden {
                assert!(
                    !name.contains(word),
                    "`{name}` on a control could name the thing itself rather than a handle to it"
                );
            }
        }
        assert!(
            control["allowed"]
                .as_array()
                .expect("`allowed` is a list of names")
                .iter()
                .all(serde_json::Value::is_string),
            "`allowed` stopped being a list of plain names"
        );

        // The answer coming back. `reason` is the one free sentence in the family and it is safe
        // for a different argument than the rest: it travels UP, it names nothing — it is shown to
        // a person and read by nobody else — and the hub never writes it to the audit, which is
        // one record per line and would otherwise take a second record of the sender's choosing.
        let outcome = serde_json::to_value(an_outcome_with_every_field()).expect("serialises");
        let outcome = outcome.as_object().expect("a frame is one flat object");
        let mut answered: Vec<&str> = outcome.keys().map(String::as_str).collect();
        answered.sort_unstable();
        assert_eq!(
            answered,
            ["intent_id", "reason", "status", "t"],
            "the field set of an outcome is CLOSED"
        );
        for name in outcome.keys() {
            for word in forbidden {
                assert!(
                    !name.contains(word),
                    "`{name}` on an outcome could name a thing rather than describe what happened"
                );
            }
        }
    }

    #[test]
    fn a_controller_that_declared_another_operation_declared_nothing_about_this_one() {
        // The reading that must be impossible to get wrong, and the exact shape of the `confirms`
        // one: the hub asks whether this connection said it could do THIS, never whether it said
        // anything. Getting it wrong carries an intention to a controller that never offered it,
        // and a controller refusing something it never declared is the polite spelling of the hub
        // having invented the request.
        let declared = Some(vec![one_control()]);
        let engineering = crate::ids::LaneId::new("engineering");
        let worker = SpecId::new("spec-worker");

        assert!(control_for(&declared, &worker, Some(&engineering), Op::Scale).is_some());
        assert!(
            control_for(&declared, &worker, Some(&engineering), Op::Restart).is_none(),
            "an op this controller never listed was covered by the ones it did"
        );
        assert!(
            control_for(&declared, &worker, None, Op::Scale).is_none(),
            "a control for one domain covered the conversation itself"
        );
        assert!(
            control_for(
                &declared,
                &worker,
                Some(&crate::ids::LaneId::new("design")),
                Op::Scale
            )
            .is_none(),
            "a control for one domain covered another"
        );
        assert!(
            control_for(
                &declared,
                &SpecId::new("spec-other"),
                Some(&engineering),
                Op::Scale
            )
            .is_none(),
            "a control for one spec covered another"
        );
        assert!(
            control_for(&None, &worker, Some(&engineering), Op::Scale).is_none(),
            "a bridge that is not a controller was treated as one"
        );
        assert!(
            control_for(&Some(vec![]), &worker, Some(&engineering), Op::Scale).is_none(),
            "an empty declaration covered an operation"
        );
        assert_eq!(
            control_for(&declared, &worker, Some(&engineering), Op::Scale).and_then(|c| c.max),
            Some(4),
            "the bound the controller set on itself did not come back with the control"
        );
    }

    #[test]
    fn a_control_naming_an_operation_this_hub_has_no_word_for_keeps_the_rest_of_the_declaration() {
        // Forward compatibility, on the pattern `confirms` set: a controller shipped after this
        // hub names something newer, and the answer is to drop the NAME rather than the
        // controller. Refusing would be a controller that cannot connect because it was too new;
        // inventing an op from an unknown word would be the hub asking for something nobody
        // described. Dropping fails towards offering the operator less, which is the safe way for
        // this to be wrong.
        let newer = Some(vec![Control {
            spec_id: SpecId::new("spec-worker"),
            domain: None,
            allowed: vec!["teleport".into(), "stop".into()],
            max: None,
        }]);
        let worker = SpecId::new("spec-worker");
        assert!(
            control_for(&newer, &worker, None, Op::Stop).is_some(),
            "a name this hub does not know cost the controller the ops it does know"
        );
        for op in Op::EVERY {
            if op != Op::Stop {
                assert!(
                    control_for(&newer, &worker, None, op).is_none(),
                    "an unknown name was read as {op:?}"
                );
            }
        }
    }

    #[test]
    fn two_entries_setting_two_bounds_on_one_operation_are_not_a_bound_this_hub_will_guess_at() {
        // A controller that lists one (spec, domain, op) twice has told the hub two different
        // things about how far it may go. Taking the first is picking a bound nobody set; taking
        // the larger is raising a bound the controller set on itself. The hub declines, and the
        // controller sees the difference in the echo.
        let twice = Some(vec![
            Control {
                spec_id: SpecId::new("spec-worker"),
                domain: None,
                allowed: vec!["scale".into()],
                max: Some(2),
            },
            Control {
                spec_id: SpecId::new("spec-worker"),
                domain: None,
                allowed: vec!["scale".into()],
                max: Some(9),
            },
        ]);
        assert!(
            control_for(&twice, &SpecId::new("spec-worker"), None, Op::Scale).is_none(),
            "the hub picked one of two bounds a controller set on one operation"
        );
    }

    #[test]
    fn every_op_is_spelt_on_the_wire_exactly_as_a_controller_declares_it() {
        // One spelling for both directions. Two would drift, and the day they did, a controller
        // would keep declaring `restart` while the hub stopped recognising the word — taking a
        // capability away in silence, which is the failure mode this whole handshake exists to
        // avoid.
        for op in Op::EVERY {
            let on_the_wire = serde_json::to_string(&op).expect("serialises");
            assert_eq!(
                on_the_wire,
                format!("\"{}\"", op.name()),
                "an op is spelt one way in a frame and another in a declaration"
            );
            assert_eq!(Op::named(op.name()), Some(op));
        }
        assert_eq!(
            Op::named("teleport"),
            None,
            "a word this hub has never heard of became one of its own verbs"
        );
        assert_eq!(
            Op::named("Scale"),
            None,
            "a name that is not the op's own name declared something"
        );
        assert_eq!(Op::EVERY.len(), 6, "adding a seventh op is a decision");
    }

    #[test]
    fn an_ack_that_knows_of_no_such_intention_is_spelt_the_way_the_document_does() {
        // It is an `ack` and not a refusal on purpose: a refusal closes the connection, and one
        // stray outcome — a controller answering late from its own ledger after a redial — is not
        // a reason to end a conversation.
        let json = serde_json::to_string(&env(HubFrame::Ack {
            r#ref: FrameId::new("f7"),
            delivered: Delivered::No,
            why: Some(AckWhy::NoSuchIntent),
        }))
        .expect("serialises");
        assert_eq!(
            json,
            r#"{"v":1,"id":"f1","t":"ack","ref":"f7","delivered":"no","why":"no-such-intent"}"#
        );
    }

    #[test]
    fn a_hello_declaring_controls_still_parses_on_a_hub_that_has_never_heard_of_them() {
        // The skew that cannot be run end to end, because the hub that would have to be old is the
        // one being replaced. `hello` is where a wrong answer costs most: refused before anything
        // else can happen, and the bridge left with a closed socket and no reason.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum BridgeFrameBeforeControls {
            Hello {
                project_id: ProjectId,
                token: String,
                instance: String,
                #[serde(default)]
                confirms: Option<Vec<String>>,
            },
            #[serde(other)]
            Unknown,
        }
        let sent = serde_json::to_string(&env(BridgeFrame::Hello {
            project_id: ProjectId::new("p"),
            token: "s".into(),
            instance: "i".into(),
            repo: None,
            pid: None,
            lane: None,
            confirms: Some(vec!["choice".into()]),
            controls: Some(vec![one_control()]),
        }))
        .expect("serialises");
        let old: Envelope<BridgeFrameBeforeControls> =
            serde_json::from_str(&sent).expect("a hub older than controls must still welcome it");
        assert_eq!(
            old.payload,
            BridgeFrameBeforeControls::Hello {
                project_id: ProjectId::new("p"),
                token: "s".into(),
                instance: "i".into(),
                confirms: Some(vec!["choice".into()]),
            },
            "an older hub could not read a controller's hello"
        );
    }

    #[test]
    fn a_welcome_echoing_controls_still_parses_on_a_bridge_that_has_never_heard_of_them() {
        // The upgrade day that actually happens: the hub is replaced and the bridge is not,
        // because a channel plugin restarts only when its session does. Every bridge in the fleet
        // reads a welcome before it can do anything at all.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum HubFrameBeforeControls {
            Welcome {
                project: String,
                limits: Limits,
            },
            #[serde(other)]
            Unknown,
        }
        let sent = serde_json::to_string(&env(HubFrame::Welcome {
            project: "A Title".into(),
            project_id: None,
            conversation: None,
            lane: None,
            topic_id: None,
            limits: Limits {
                max_frame: 65536,
                max_text: 3500,
                frames_per_min: 20,
            },
            outbox: None,
            controls: Some(vec![one_control()]),
        }))
        .expect("serialises");
        let old: Envelope<HubFrameBeforeControls> =
            serde_json::from_str(&sent).expect("an older bridge must still read its welcome");
        assert!(
            matches!(old.payload, HubFrameBeforeControls::Welcome { ref project, .. } if project == "A Title"),
            "{old:?}"
        );
    }

    #[test]
    fn an_intent_reaches_a_bridge_from_before_intents_as_a_kind_it_can_hold_and_not_a_parse_error()
    {
        // A frame KIND is a value and never a parse error, which is what lets a hub that has grown
        // a new frame go on talking to every bridge already installed on the box — and a channel
        // plugin restarts only when the operator's session does, so those bridges are the ordinary
        // case for days. An ordinary bridge declares no controls and will never be sent one of
        // these; it must survive reading one anyway, because the day it cannot is the day one
        // misrouted frame takes a working project off the air.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum HubFrameBeforeIntents {
            Ping,
            #[serde(other)]
            Unknown,
        }
        let sent = serde_json::to_string(&env(an_intent_with_every_field())).expect("serialises");
        let old: Envelope<HubFrameBeforeIntents> =
            serde_json::from_str(&sent).expect("an intent must be a value on a build without one");
        assert_eq!(
            old.payload,
            HubFrameBeforeIntents::Unknown,
            "a frame kind killed a connection instead of being logged and ignored"
        );
    }

    #[test]
    fn an_intent_outcome_reaches_a_hub_from_before_intents_as_a_kind_it_can_hold_and_not_an_error()
    {
        // The same in the other direction, and the one that matters more: a controller upgraded
        // ahead of the hub answers an intention nobody asked for, and the hub has to ignore the
        // frame rather than drop the connection under a project that is working perfectly well.
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum BridgeFrameBeforeIntents {
            Pong {
                #[serde(rename = "ref")]
                r#ref: FrameId,
            },
            #[serde(other)]
            Unknown,
        }
        let sent = serde_json::to_string(&env(BridgeFrame::IntentOutcome {
            intent_id: IntentId::new("i-7a1c"),
            status: IntentStatus::Accepted,
            reason: None,
        }))
        .expect("serialises");
        let old: Envelope<BridgeFrameBeforeIntents> =
            serde_json::from_str(&sent).expect("an outcome must be a value on a build without one");
        assert_eq!(
            old.payload,
            BridgeFrameBeforeIntents::Unknown,
            "a frame kind killed a connection instead of being logged and ignored"
        );
    }
}

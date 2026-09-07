//! The Telegram front door: one bot, one forum, one topic per project.
//!
//! # The gate comes first, and it asks two questions
//!
//! Every update passes [`Gate::admit`] before anything else looks at it — before command parsing,
//! before any state is touched. That ordering is the whole security model, and it is why the check
//! is a separate type with its own tests rather than an `if` inside a handler: a handler that grows
//! a second branch is a handler that grows a way around the gate.
//!
//! The chat is the first question: is this somewhere the bot listens. The person is the second,
//! and it is asked before the text is so much as read: may this person speak HERE. "Here" is the
//! conversation the update belongs to — the project's topic or a lane's — and the answer comes
//! from two lists, neither of which a message can change: the people who may speak anywhere
//! (the configuration) and the people a project has let into its own conversations (the registry,
//! written only by `herdr-tg allow` at a keyboard). A command is answered with facts about every
//! project, so it takes the wider standing. Until this existed, anyone who could post in the
//! allowed forum was relayed into an agent's turn and anyone who could see a keyboard could tap
//! it — safe for as long as the forum held one person, and not a moment longer.
//!
//! A rejected chat gets **silence**, not a refusal, and so does a rejected person. A refusal
//! confirms the bot is alive and tells a stranger what it is for. The rejection is logged at
//! `warn` with the id, so the operator can read his own id out of `journalctl` when he has
//! mistyped it — which is the realistic failure here, not an attacker — and a rejected person is
//! also one line in the hub's audit, because that file is the only place he can learn that
//! somebody in his forum is typing at his agents, and who.
//!
//! # This binary cannot type into a terminal
//!
//! Not "does not by default" — cannot. The path that read rendered panes and sent keystrokes has
//! been deleted, along with the modules that served it. There is no flag, no mode and no code path
//! from a Telegram message to a keyboard. An answer reaches an agent as a MESSAGE IN ITS OWN TURN,
//! over the hub's socket, and the two-writer race that made the old path unsafe has no mechanism
//! here.
//!
//! # Why long-poll
//!
//! The bridge dials out and binds nothing. There is no listening port in herdr-tg at all, so the
//! box needs no ingress, no public hostname and no webhook certificate. For one operator's message
//! volume the latency difference is invisible.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures_core::Stream;
use teloxide::prelude::*;
use teloxide::stop::StopToken;
use teloxide::types::AllowedUpdate;
use teloxide::types::{MessageId, ParseMode, ThreadId};
use teloxide::update_listeners::{AsUpdateStream, UpdateListener};
use teloxide::utils::command::BotCommands;
use teloxide::{ApiError, RequestError};

use crate::config::Config;
use crate::heartbeat::{Health, Heartbeat, Verdict};
use crate::render::escape_html;

/// Telegram's hard limit on a message body. Exceeding it is a 400 from the API, which in a
/// long-poll loop looks like "the bot went quiet" rather than like an error.
const TELEGRAM_MAX_CHARS: usize = 4096;

/// Headroom for the wrapper and the truncation notice.
const BODY_BUDGET: usize = TELEGRAM_MAX_CHARS - 256;

/// The identity gate. Fails closed by construction.
///
/// Two questions, asked in this order on every update: is this a chat the bot listens in, and is
/// this a person who may speak in it. The second used to be nobody's question — anyone who could
/// post in the allowed forum was relayed into an agent's turn, and anyone who could see a keyboard
/// could tap it — which was safe only while the forum held one person.
#[derive(Debug, Clone)]
pub struct Gate {
    allowed: BTreeSet<i64>,
    /// The people who may speak anywhere this bot listens. A project's own people are the hub's to
    /// know, from the registry; this is the list that needs no hub, so a command can be answered on
    /// a box with no forum and refused on one for the same person either way.
    people: BTreeSet<i64>,
}

impl Gate {
    pub fn new(allowed: BTreeSet<i64>, people: BTreeSet<i64>) -> Self {
        Self { allowed, people }
    }

    /// Is this chat permitted? An empty allowlist admits nobody.
    pub fn admit(&self, chat_id: i64) -> bool {
        self.allowed.contains(&chat_id)
    }

    /// May this person speak anywhere the bot listens? `None` — no person the update could vouch
    /// for — is never anyone, and an empty list knows nobody.
    pub fn knows(&self, user: Option<i64>) -> bool {
        user.is_some_and(|u| crate::config::is_a_persons_id(u) && self.people.contains(&u))
    }

    pub fn is_deaf(&self) -> bool {
        self.allowed.is_empty()
    }

    /// Nobody at all may speak anywhere: no person listed, and no private chat on the chat
    /// allowlist to name one. Only a project's own people could still be heard.
    pub fn nobody_may_speak(&self) -> bool {
        self.people.is_empty()
    }
}

/// The person who typed a message, when there is one the bot can vouch for.
///
/// `None` in three shapes, and every one of them is a stranger: a channel post carries no sender
/// at all; a bot is not a person; and an admin posting anonymously arrives as Telegram's shared
/// "anonymous admin" service account with `sender_chat` set to the group — so allowing that id
/// would allow every anonymous admin in every group, and the group's id as a sender is refused
/// outright rather than matched against anything.
fn who_sent(msg: &Message) -> Option<i64> {
    if msg.sender_chat.is_some() {
        return None;
    }
    let from = msg.from.as_ref()?;
    if from.is_bot {
        return None;
    }
    i64::try_from(from.id.0).ok()
}

/// The person who tapped a button. Always someone on a callback query — Telegram sends none from a
/// channel or a chat — but a bot is still not a person.
fn who_tapped(q: &CallbackQuery) -> Option<i64> {
    if q.from.is_bot {
        return None;
    }
    i64::try_from(q.from.id.0).ok()
}

/// May a person of this standing do what he typed?
///
/// A command is answered with facts about every project — `/projects` is their names and states,
/// and "I do not have that command" is an answer too — so it takes the wider standing. Words for
/// the agent in this conversation take standing here. A command aimed at another bot is nobody's
/// to act on whatever the standing, and is decided before this is asked.
fn may_act(standing: crate::hub::Standing, typed: &Typed) -> bool {
    match typed {
        Typed::Command(_) | Typed::NotOneOfMine => standing.may_command(),
        Typed::Steering => standing.may_speak_here(),
        Typed::ForAnotherBot => false,
    }
}

/// What kind of thing arrived, for the audit line about the person who was not allowed to send it.
fn what_arrived(text: Option<&str>) -> &'static str {
    match text {
        None => "something that was not text",
        Some(t) if t.starts_with('/') => "a command",
        Some(_) => "words",
    }
}

/// The file he sent with a message, as the message describes it — nothing fetched yet.
///
/// Everything a phone said about the file is carried as DATA: the size is a claim the hub checks
/// again on the stream, the mime is a declaration, and the name is a string somebody chose that
/// never becomes a segment of a path. A photo declares neither a mime nor a name — Telegram
/// re-encodes it — and comes in several sizes, of which the last is the largest.
fn what_he_sent(msg: &Message) -> Option<crate::hub::SentFile> {
    use crate::hub::SentFile;
    use hub_proto::FileKind;
    // The library fills an absent `file_size` with `u32::MAX`. That is "unknown", not a size.
    let size =
        |meta: &teloxide::types::FileMeta| (meta.size != u32::MAX).then_some(u64::from(meta.size));
    if let Some(sizes) = msg.photo() {
        let best = sizes.last()?;
        return Some(SentFile {
            kind: FileKind::Photo,
            file_id: best.file.id.0.clone(),
            size: size(&best.file),
            mime: None,
            filename: None,
        });
    }
    if let Some(d) = msg.document() {
        return Some(SentFile {
            kind: FileKind::Document,
            file_id: d.file.id.0.clone(),
            size: size(&d.file),
            mime: d.mime_type.as_ref().map(|m| m.essence_str().to_owned()),
            filename: d.file_name.clone(),
        });
    }
    if let Some(v) = msg.video() {
        return Some(SentFile {
            kind: FileKind::Video,
            file_id: v.file.id.0.clone(),
            size: size(&v.file),
            mime: v.mime_type.as_ref().map(|m| m.essence_str().to_owned()),
            filename: v.file_name.clone(),
        });
    }
    if let Some(a) = msg.animation() {
        return Some(SentFile {
            kind: FileKind::Animation,
            file_id: a.file.id.0.clone(),
            size: size(&a.file),
            mime: a.mime_type.as_ref().map(|m| m.essence_str().to_owned()),
            filename: a.file_name.clone(),
        });
    }
    if let Some(a) = msg.audio() {
        return Some(SentFile {
            kind: FileKind::Audio,
            file_id: a.file.id.0.clone(),
            size: size(&a.file),
            mime: a.mime_type.as_ref().map(|m| m.essence_str().to_owned()),
            filename: a.file_name.clone(),
        });
    }
    if let Some(v) = msg.voice() {
        return Some(SentFile {
            kind: FileKind::Voice,
            file_id: v.file.id.0.clone(),
            size: size(&v.file),
            mime: v.mime_type.as_ref().map(|m| m.essence_str().to_owned()),
            filename: None,
        });
    }
    None
}

/// Something he could reasonably expect the agent to see, and that the bot does not carry.
///
/// A sticker is a reaction; a video note is a video. Both are answered with what the bot does
/// carry, because silence on something he sent reads as the bot having missed it. A location, a
/// contact or a poll is not something he sent TO the agent, and gets what it always got: nothing.
fn something_the_bot_does_not_carry(msg: &Message) -> bool {
    msg.sticker().is_some() || msg.video_note().is_some()
}

/// What he is told when he sends one of those.
const NOT_CARRIED: &str = "That did not reach the agent — it takes photos, documents, voice \
                           notes, videos and audio, not stickers or video notes.";

/// May this person speak here — asked of the hub when there is one, of the gate alone when not.
///
/// No hub means no registry to hold a project's people, so only the bot-wide list can answer. It
/// must answer rather than refuse outright, because a command is still answered on a box where
/// the forum has never been set up — that is how the operator finds out it has not.
async fn standing_of(
    ctx: &Ctx,
    user: Option<i64>,
    at: Option<&crate::hub::Addr>,
) -> crate::hub::Standing {
    match &ctx.hub {
        Some(hub) => hub.standing_of(user, at).await,
        None if ctx.gate.knows(user) => crate::hub::Standing::Anywhere,
        None => crate::hub::Standing::Stranger,
    }
}

/// A person who may not speak where he did: nothing relayed, nothing answered, one line written.
///
/// Silence on the phone is the point — a reply confirms something is listening — so the journal
/// and the hub's audit are the only places this shows, and both carry the id: the operator is
/// going to want either to let this person in or to find out who it was, and both start with the
/// number. On a box with no forum there is no audit file, and the journal line is all there is.
async fn refused_sender(
    ctx: &Ctx,
    user: Option<i64>,
    chat_id: i64,
    at: Option<&crate::hub::Addr>,
    what: &str,
) {
    // `sender=<number>` or `sender=unknown`, the same as the audit line, and never the `Option`
    // as the compiler prints it: this is the line he copies the id out of.
    tracing::warn!(
        sender = %crate::hub::name_the_sender(user),
        chat_id,
        what,
        "from a person NOT allowed to speak here — ignored. To let this person into one \
         project's conversations: herdr-tg allow <repo> <user id>. To let them speak anywhere \
         this bot listens: add the id to HERDR_TG_ALLOWED_USER_IDS."
    );
    if let Some(hub) = &ctx.hub
        && let Err(e) = hub.audit.stranger(user, chat_id, at, what)
    {
        tracing::error!(error = %e, "could not write down a refused sender");
    }
}

/// What the bot answers. Deliberately short: aiming is a tap you never make, because a message
/// belongs to the topic it was typed in.
#[derive(BotCommands, Clone, Debug, PartialEq, Eq)]
#[command(
    rename_rule = "lowercase",
    description = "herdr-tg — your herd, from your pocket."
)]
pub enum Command {
    #[command(
        description = "which projects are enrolled, which are connected, and which are switched off."
    )]
    Projects,
    #[command(description = "show this help.")]
    Help,
}

/// What a line typed at the bot is, decided before anything is done with it.
///
/// Telegram hands a group bot EVERY `/command` typed in the group, including ones aimed at some
/// other bot by name, so the first question is not "which command" but "is this for us at all".
#[derive(Debug, PartialEq)]
enum Typed {
    /// One of this bot's own commands, whether or not he typed the `@name` after it.
    Command(Command),
    /// Words for whatever is running in the topic. Relayed verbatim; never parsed.
    Steering,
    /// A command aimed, by name, at a different bot in the same group. Not ours to answer and
    /// not his words for an agent either — the only right thing to do with it is nothing.
    ForAnotherBot,
    /// A command aimed at THIS bot by name, that it does not have.
    NotOneOfMine,
}

/// Decide what he typed. Pure, so it can be tested without a bot token.
///
/// `bot_username` is the name Telegram gave this bot, read from `getMe` at startup — never a
/// constant. This was `"herdr_tg"`, a name the bot has never had, so every `@`-suffixed command
/// failed to parse as aimed at the wrong bot and fell through to the relay: `/projects@<the real
/// bot>` went verbatim into a coding agent's turn and into the audit as words he meant to send.
/// Only with the real name in hand is dropping a command aimed at another bot the right thing.
fn what_he_typed(text: &str, bot_username: &str) -> Typed {
    use teloxide::utils::command::ParseError;
    // A command opens with a slash; nothing else is one. The parser is trusted only past that
    // point, because it splits the FIRST WORD at `@` and checks the name before it has looked for
    // a slash at all — so `@alice can you check`, `me@example.com` and `git@github.com:org/repo`
    // all came back "aimed at another bot", and the branch that rightly drops those in silence
    // dropped his words with them: no relay, no reply, nothing in the audit. A bare `@<this bot>`
    // went the other way and was answered "I do not have that command".
    if !text.starts_with('/') {
        return Typed::Steering;
    }
    match Command::parse(text, bot_username) {
        Ok(cmd) => Typed::Command(cmd),
        // Aimed at somebody else, by name. The parser checks the name BEFORE it looks the command
        // up, so this is exactly "not ours" and never "not a command we have".
        Err(ParseError::WrongBotName(_)) => Typed::ForAnotherBot,
        // A command we do not have. Whether it is his words for an agent or a miss aimed at us
        // turns on whether he named us: agents have slash commands of their own, and a bare
        // `/compact` in a topic is for the agent in it.
        Err(ParseError::UnknownCommand(_)) if names_this_bot(text, bot_username) => {
            Typed::NotOneOfMine
        }
        Err(_) => Typed::Steering,
    }
}

/// Is the first word `/something@<this bot>`, whatever the something?
///
/// The same split the parser makes — first word, then `@` — so the two cannot disagree about which
/// bot a line names. Case-insensitive because usernames are.
fn names_this_bot(text: &str, bot_username: &str) -> bool {
    text.split_whitespace()
        .next()
        .and_then(|word| word.split_once('@'))
        .is_some_and(|(_, name)| name.eq_ignore_ascii_case(bot_username))
}

/// Everything a handler needs, cloned per update.
#[derive(Clone)]
struct Ctx {
    /// The hub, when a forum is configured. `None` means the socket half is not running, which is a
    /// real state on a box where the forum has not been set up yet and must not be a crash.
    hub: Option<Arc<crate::hub::Hub<crate::surface::Telegram>>>,
    gate: Arc<Gate>,
    /// This bot's own username, as Telegram reports it. What a `/command@name` is checked against.
    username: Arc<str>,
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// What the accept loop does with what it finds at the door.

/// How long the accept loop waits after the first failed accept.
const FIRST_WAIT: Duration = Duration::from_millis(50);

/// And how long it waits at most. Doubling with no ceiling turns a squeeze that lasted a minute
/// into a hub still asleep an hour later, with the Telegram half running and nothing anywhere
/// saying the bridges can no longer connect.
const LONGEST_WAIT: Duration = Duration::from_secs(5);

/// The wait between a failed accept and the next attempt, and what clears it.
///
/// It is a type rather than a `let mut` in the loop because **the only thing that may make this
/// loop wait is the door itself**, and that rule needs somewhere to be written and tested. Reading
/// the peer's credentials moved from the per-connection task up to accept time when the transport
/// seam went in, and that move creates the hazard: a loop that cannot tell "I could not name this
/// one caller" from "the listener is unwell" does the only thing left to it and sleeps, longer
/// each time, while every healthy bridge on the box queues behind one peer nobody could identify.
/// So there is no way to say "wait" here for anything but the door.
///
/// [`crate::transport::LocalSocket::accept`] is the other half of that: it drops a connection it
/// cannot identify and goes back to waiting, so the failure never arrives here at all.
struct AcceptPacing {
    wait: Duration,
}

impl AcceptPacing {
    fn new() -> Self {
        Self { wait: FIRST_WAIT }
    }

    /// One connection through the door is proof the squeeze is over, so the next failure starts
    /// again at the shortest wait rather than wherever the last one left off.
    fn a_connection_came_through(&mut self) {
        self.wait = FIRST_WAIT;
    }

    /// How long to go deaf for before asking the door again, growing while it stays shut.
    fn wait_after_the_door_would_not_open(&mut self) -> Duration {
        let now = self.wait;
        self.wait = (self.wait * 2).min(LONGEST_WAIT);
        now
    }
}

/// How long the hub waits between one look at itself and the next.
///
/// Two of these fit inside the ninety seconds a fact stays fresh, and three inside the watchdog's
/// three-minute staleness window — so one missed answer is not an alarm and two are.
///
/// What he actually waits is longer than that window, and the four numbers that make it live in
/// three files: a leg that dies just after a good answer is still fresh at the next tick and is
/// stamped over, so the last stamp is one tick late; then the alarm wants its whole staleness
/// window; then it only looks when its timer lets it. Forty-five, a hundred and eighty, sixty and
/// ten seconds — so the operator hears within about five minutes, not three.
/// `tests/the_heartbeat_is_earned_not_scheduled.rs` does that arithmetic and fails if this sentence
/// promises him sooner than the numbers can manage.
const WATCHDOG_TICK: Duration = Duration::from_secs(45);

/// How long a knock has to come out the far end of the door before the loop is called stopped.
///
/// Generous for a local socket the accept loop is sitting on, short enough that the tick it runs
/// in cannot slide out of the freshness window even if it times out every time.
const A_KNOCK_IS_ANSWERED_WITHIN: Duration = Duration::from_secs(2);

/// How often the knock looks to see whether the accept loop took it.
const LOOK_AGAIN_AFTER: Duration = Duration::from_millis(10);

/// Where the agents' door is, as far as the watchdog contract is concerned.
///
/// It is a value passed to the tick rather than a flag the tick reads off the hub, because every
/// way this hub can come up without a door — no forum, a forum it may not write in, a socket that
/// would not open — is a different sentence to the operator and the same silence to the watchdog.
/// Making them one type is what stops a fourth way being added with no sentence at all.
pub(crate) enum Door {
    /// Open, and being accepted on at this path. The tick knocks at it.
    Open(PathBuf),
    /// Never opened, and this is why, in the words the operator reads. No number of Bot API round
    /// trips may cover for it.
    Shut(&'static str),
}

/// Accept connections and serve them, for as long as this process lives.
///
/// A per-connection error must not end the loop. Running out of file descriptors, or a peer that
/// hangs up between the SYN and the accept, is a transient squeeze — and breaking here would leave
/// the socket half gone for the life of the process, with the Telegram half still running and
/// nothing anywhere saying the bridges could no longer connect. That is this system's signature
/// failure: silence that looks exactly like health.
///
/// It is a function of its own, and not four lines inside `serve`, because the heartbeat's claim
/// is about THIS loop: a test that wants to prove "a door nobody is accepting at earns no stamp"
/// has to be able to run the real loop, and to leave it unrun.
pub(crate) async fn hold_the_door<S: crate::hub::Surface>(
    socket: crate::transport::LocalSocket,
    hub: Arc<crate::hub::Hub<S>>,
    health: Arc<Health>,
) {
    let mut pacing = AcceptPacing::new();
    loop {
        match socket.accept().await {
            Ok(accepted) => {
                pacing.a_connection_came_through();
                // The one writer of the door's half of the heartbeat, and it is here rather than
                // in the tick because this is the only place that knows the loop is still turning.
                // A connection the kernel completed into the backlog of a listener whose loop has
                // ended never reaches this line, which is exactly the difference the stamp is
                // claiming to know.
                health.a_connection_came_through_the_door(Instant::now());
                let hub = Arc::clone(&hub);
                tokio::spawn(async move {
                    if let Err(e) = hub.serve_connection(accepted).await {
                        tracing::warn!(error = %e, "a bridge connection ended badly");
                    }
                });
            }
            // Only the door itself reaches here. A connection whose peer the kernel would not name
            // is dropped by the transport and never becomes an error at this level, so one
            // unidentifiable peer can never put the hub to sleep on every other bridge's behalf.
            Err(e) => {
                let wait = pacing.wait_after_the_door_would_not_open();
                tracing::warn!(
                    error = %e, backoff_ms = wait.as_millis(),
                    "the hub could not accept a connection; retrying"
                );
                tokio::time::sleep(wait).await;
            }
        }
    }
}

/// The dispatcher's update listener, with the hub's third fact witnessed as it turns.
///
/// It wraps teloxide's long poll rather than replacing it, so what this bot asks Telegram for is
/// unchanged — the same ten-second timeout, the same webhook deleted on the way in, the same
/// backoff after a refusal. What it adds is the only witness in this process to the difference
/// between "the dispatcher is still going to Telegram for his taps" and "it stopped": a wedged
/// dispatcher stops pulling from this stream and nothing else on the box can tell. His phone line
/// answers, his door accepts, and every tap he makes dies in silence.
pub(crate) struct WatchedUpdates<L> {
    inner: L,
    health: Arc<Health>,
}

impl<L> WatchedUpdates<L> {
    pub(crate) fn new(inner: L, health: Arc<Health>) -> Self {
        Self { inner, health }
    }
}

impl<L> UpdateListener for WatchedUpdates<L>
where
    L: UpdateListener,
    Self: for<'a> AsUpdateStream<'a, StreamErr = L::Err>,
{
    type Err = L::Err;

    fn stop_token(&mut self) -> StopToken {
        self.inner.stop_token()
    }

    /// Handed straight on. Swallowing it would quietly change which update types this bot asks
    /// Telegram for, which is a behaviour change nobody asked for in a health measurement.
    fn hint_allowed_updates(&mut self, hint: &mut dyn Iterator<Item = AllowedUpdate>) {
        self.inner.hint_allowed_updates(hint);
    }
}

impl<'a, L> AsUpdateStream<'a> for WatchedUpdates<L>
where
    L: AsUpdateStream<'a>,
    L::Stream: Send + 'a,
{
    type StreamErr = L::StreamErr;
    type Stream = WatchedUpdateStream<L::Stream>;

    fn as_stream(&'a mut self) -> Self::Stream {
        WatchedUpdateStream {
            inner: Box::pin(self.inner.as_stream()),
            health: Arc::clone(&self.health),
        }
    }
}

/// The stream itself, taking a note every time the dispatcher drives it.
pub(crate) struct WatchedUpdateStream<S> {
    /// Boxed so this wrapper needs no pin projection of its own. One allocation, once, at startup:
    /// the alternative is unsafe code in the middle of the thing that reports whether the hub is
    /// well.
    inner: Pin<Box<S>>,
    health: Arc<Health>,
}

impl<S, E> Stream for WatchedUpdateStream<S>
where
    S: Stream<Item = Result<Update, E>>,
{
    type Item = Result<Update, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        // Every drive counts, not only the ones carrying an update. A forum can be quiet for
        // hours, and "nobody has tapped anything" must never be read as "the hub has stopped
        // asking" — those are the two states this whole leg exists to tell apart.
        this.health.the_hub_looked_for_updates(Instant::now());
        let polled = this.inner.as_mut().poll_next(cx);
        if let Poll::Ready(Some(Ok(_))) = &polled {
            // One of his taps came out the far end. It is the only proof that Telegram is handing
            // updates to THIS copy of the bot, so it is the only thing that can end a run of
            // refusals early.
            this.health.an_update_reached_the_hub(Instant::now());
        }
        polled
    }
}

/// What Telegram refusing to hand the updates over means, in the words the operator reads.
///
/// Worded here rather than in `heartbeat.rs` for the same reason the door's failures are: this is
/// where the reason is known, and "somebody else has your taps" and "the hub cannot get at them at
/// all" are two different mornings and one silence.
fn tell_the_health_what_telegram_refused(health: &Health, error: &RequestError, now: Instant) {
    match error {
        // The conflict item 4 was written for. A second `herdr-tg serve` beside the unit — a stray
        // one started by hand at a keyboard is the likeliest way it happens on this box — takes the
        // update slot, and Telegram answers this copy with a conflict on every call while its phone
        // line and its door go on answering perfectly.
        RequestError::Api(ApiError::TerminatedByOtherGetUpdates) => health
            .the_update_stream_was_refused(
                now,
                "another copy of this bot is taking your taps, so none of them reach the agents \
                 here",
            ),
        // The other way every tap dies while both the other halves answer perfectly. The hub
        // clears the web address at startup, so this one means something set it afterwards — a
        // hosted copy of the bot, a deploy script — and the fix is five seconds and on a different
        // machine from the one the second-copy sentence would send him to.
        RequestError::Api(ApiError::CantGetUpdates) => health.the_update_stream_was_refused(
            now,
            "something has pointed this bot at a web address, so your taps go there instead of \
             here",
        ),
        _ => health.the_update_stream_was_refused(
            now,
            "the hub is being turned away when it goes to collect your taps",
        ),
    }
}

/// One turn of the watchdog contract: ask every leg, write down what they said, and stamp only
/// what they earned.
///
/// Everything it depends on is handed to it, so the decision can be driven by a test with no
/// Telegram, no dispatcher and no forum — the decision being the whole of item 4, and the state it
/// gets wrong (a green stamp over a dead control plane) being one no live system announces.
pub(crate) async fn stamp_what_the_hub_can_prove(
    the_phone_line_answered: bool,
    door: &Door,
    health: &Health,
    heartbeat: &Heartbeat,
) -> Verdict {
    if the_phone_line_answered {
        health.the_phone_line_answered(Instant::now());
    } else {
        health.the_phone_line_did_not_answer();
    }
    knock_at(door, health).await;

    let verdict = health.verdict(Instant::now());
    // The note first: it is the only place that can say WHICH leg failed, and a tick that died
    // between the two writes should leave the explanation behind rather than the stamp.
    if let Err(e) = heartbeat.note(&verdict) {
        tracing::error!(error = %e, path = %heartbeat.note_path().display(),
            "could not write down which half of the hub is unwell");
    }
    if let Err(e) = heartbeat.stamp_if(&verdict) {
        tracing::error!(error = %e, path = %heartbeat.path().display(),
            "could not stamp the heartbeat — the watchdog may raise a false alarm");
    }
    verdict
}

/// Say in the journal, once per flip, which way the verdict went and what every leg said.
///
/// EVERY leg on the line, never only the one that failed. "Telegram is unreachable", "no agent can
/// reach your phone" and "your taps are going to another copy of this bot" are three different
/// outages with three different answers, and a line that says one of them without the others has
/// been read as the whole story before. The operator never sees this — his sentence is the
/// watchdog's — but whoever reads the journal afterwards is reconstructing an outage from it.
fn say_in_the_journal_which_way_the_verdict_went(verdict: &Verdict, heartbeat: &Heartbeat) {
    match verdict.why_withheld() {
        Some(why) => tracing::warn!(
            why = %why,
            phone_line = %verdict.phone_line().said(),
            door = %verdict.door().said(),
            updates = %verdict.updates().said(),
            note = %heartbeat.note_path().display(),
            "the hub is not stamping the heartbeat, so the watchdog will raise the alarm"
        ),
        None => tracing::info!(
            phone_line = %verdict.phone_line().said(),
            door = %verdict.door().said(),
            updates = %verdict.updates().said(),
            "the hub is serving and stamping the heartbeat again"
        ),
    }
}

/// Provoke a connection through the door and find out whether one comes out the far end.
///
/// The connect returning `Ok` is NOT the proof and must never be treated as it: the kernel
/// completes a connection into the backlog of a listener nobody is accepting on, so a hub whose
/// accept loop had ended would pass a test made of connects while every bridge on the box sat in
/// that backlog forever. What is asked instead is the accept loop's own count of connections it
/// took, and the knock is only the thing that gives it something to take.
async fn knock_at(door: &Door, health: &Health) {
    let path = match door {
        Door::Shut(why) => {
            health.the_door_is_not_answering(why);
            return;
        }
        Door::Open(path) => path,
    };

    let before = health.connections_that_came_through();
    if let Err(e) = crate::transport::knock(path).await {
        tracing::debug!(error = %e, path = %path.display(), "the hub could not reach its own door");
        health.the_door_is_not_answering("the agents' door is not there any more");
        return;
    }

    // Any connection counts, not only this one: a busy hub taking a real bridge's dial in the same
    // moment is the same proof, and demanding our own back would fail the tick on a hub that is
    // demonstrably working.
    let give_up_at = Instant::now() + A_KNOCK_IS_ANSWERED_WITHIN;
    while health.connections_that_came_through() == before {
        if Instant::now() >= give_up_at {
            health.the_door_is_not_answering("nothing is answering at the agents' door");
            return;
        }
        tokio::time::sleep(LOOK_AGAIN_AFTER).await;
    }
}

/// Run the bot until the process is asked to stop.
pub async fn serve(config: Config) -> anyhow::Result<()> {
    let people = config.people();
    let gate = Gate::new(config.allowed_chat_ids.clone(), people.clone());
    if gate.is_deaf() {
        tracing::warn!(
            "the chat allowlist is EMPTY — this bot will answer nobody. Set \
             HERDR_TG_ALLOWED_CHAT_IDS or `allowed_chat_ids` in herdr-tg.toml. Re-run \
             scripts/setup-token.sh to discover your chat id."
        );
    } else {
        tracing::info!(chats = ?config.allowed_chat_ids, "allowlist active");
    }
    announce_who_may_speak(&config.allowed_chat_ids, &config.allowed_user_ids, &gate);

    let bot = Bot::new(config.token());
    let me = bot.get_me().await?;
    tracing::info!(bot = %me.username(), "connected to the Bot API; long-polling");

    // ── the hub ───────────────────────────────────────────────────────────────────────────────
    // It needs a forum: routing is a single rule — a topic, inside the one configured chat — and
    // without that chat there is nowhere for a project's topic to live.
    //
    // Both halves of the watchdog contract are decided here, together, and that is deliberate:
    // every way this block can end without a door is a way the operator's agents reach nobody
    // while his phone still answers, and each of them hands the heartbeat a sentence saying so.
    let health = Arc::new(Health::new());
    let (hub, door) = match config.forum_chat_id {
        None => {
            tracing::warn!(
                "no forum chat is configured, so no project can be given a topic. Set \
                 HERDR_TG_FORUM_CHAT_ID to switch the hub on."
            );
            (
                None,
                Door::Shut("no forum is configured, so no agent has anywhere to reach you"),
            )
        }
        Some(forum) if !config.allowed_chat_ids.contains(&forum) => {
            // Every question would go out and every tap would die in silence: the callback handler
            // checks the allowlist first, so a forum that is not on it produces buttons that cannot
            // be answered and say nothing about why. There is no configuration in which that is
            // what someone meant, so it fails closed and names both numbers.
            tracing::error!(
                forum,
                allowed = ?config.allowed_chat_ids,
                "the forum chat is not on the allowlist, so every button in it would be dead. Add \
                 it to HERDR_TG_ALLOWED_CHAT_IDS. The hub will not start."
            );
            (
                None,
                Door::Shut(
                    "the forum this hub was pointed at is not one it may write in, so it did not \
                     start",
                ),
            )
        }
        Some(forum) => {
            let surface = Arc::new(crate::surface::Telegram::new(bot.clone(), ChatId(forum)));
            let hub = Arc::new(crate::hub::Hub::new(
                surface,
                crate::registry::Registry::load(crate::registry::Registry::default_path()),
                crate::hub::AskLedger::load(crate::hub::AskLedger::default_path()),
                crate::hub::HubAudit::new(crate::hub::HubAudit::default_path()),
                config.allowed_chat_ids.iter().copied().collect(),
                people.iter().copied().collect(),
                forum,
            ));
            let sock = crate::transport::socket_path();
            match crate::transport::LocalSocket::bind(&sock) {
                Err(e) => {
                    // Not fatal. The Telegram half still answers `/projects`, and refusing to boot
                    // over the socket would take the operator's only channel down with it. It is
                    // not silent either, which it used to be after this line: the heartbeat is
                    // withheld from here on, so the watchdog buzzes his phone within about five
                    // minutes instead of a journal line nobody reads carrying the whole of the news.
                    tracing::error!(error = %e, path = %sock.display(), "could not open the hub's socket");
                    (None, Door::Shut("the agents' door could not be opened"))
                }
                Ok(socket) => {
                    // The transport says what it is and who may reach it. "It is up" and "it is up
                    // for the right people" are different facts, and the second is the one worth
                    // reading in a journal at three in the morning — the more so now that there is
                    // a name for a transport that would not be this one. The path is not printed
                    // beside it: `describe()` already carries it, and one fact under two field
                    // names is how a reader ends up wondering which of them is the real door.
                    tracing::info!(
                        transport = %socket.describe(),
                        audit = %hub.audit.path().display(),
                        "the hub is listening"
                    );
                    // The other half of `herdr-tg disable`: the flag alone turns away the NEXT
                    // connection, and this is what ends one that is already on the socket.
                    Arc::clone(&hub).watch_the_registry(crate::hub::REGISTRY_WATCH_EVERY);
                    tokio::spawn(hold_the_door(socket, Arc::clone(&hub), Arc::clone(&health)));
                    (Some(hub), Door::Open(sock))
                }
            }
        }
    };

    let ctx = Ctx {
        hub,
        gate: Arc::new(gate),
        username: Arc::from(me.username()),
    };

    // The other half of the watchdog contract. `deploy/herdr-tg-watchdog.sh` buzzes the operator's
    // phone when this file stops being touched, and it arms itself the first time it ever sees it.
    //
    // WHAT A STAMP PROVES: the process is alive, the network is up, the token is still good and
    // Telegram answered — a real round trip, not a local clock read — AND a connection came out
    // the far end of the agents' door inside the last minute and a half — AND the dispatcher went
    // to Telegram for his taps inside that same window and was not turned away every time it did.
    // Every one, every time. The Bot API answering on its own used to be the whole of it, which
    // meant a hub whose socket never opened stamped a green file every forty-five seconds while
    // every agent on the box was talking to nobody; then it was that and the door, which still let
    // a second copy of this bot take the update slot and leave this file green while every tap he
    // made went somewhere else.
    //
    // The third fact is witnessed by the update stream itself (`WatchedUpdates`, above) and by the
    // listener's error handler, never by this tick: a dispatcher wedged behind a stuck handler
    // stops driving that stream, and a timer that voted on its behalf would report health straight
    // through the outage.
    //
    // WHAT IT STILL DOES NOT PROVE: that a tap which arrived was ACTED ON. A handler that took one
    // and hung looks healthy here until the wedge backs up far enough to stop the stream being
    // driven. That gap is named rather than overlooked, and claiming more here would be the exact
    // failure the watchdog exists to prevent — a healthy-looking report from something that is not.
    let heartbeat = Heartbeat::new(Heartbeat::default_path());
    let hb_bot = bot.clone();
    let hb_health = Arc::clone(&health);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(WATCHDOG_TICK);
        // Said on the way in and then only when it changes. A line every forty-five seconds is a
        // line nobody reads, and the transition is the news.
        let mut said_last_time: Option<bool> = None;
        loop {
            tick.tick().await;
            let answered = match hb_bot.get_me().await {
                Ok(_) => true,
                Err(e) => {
                    tracing::debug!(error = %e, "the Bot API did not answer");
                    false
                }
            };
            let verdict =
                stamp_what_the_hub_can_prove(answered, &door, &hb_health, &heartbeat).await;
            if said_last_time != Some(verdict.earned()) {
                say_in_the_journal_which_way_the_verdict_went(&verdict, &heartbeat);
                said_last_time = Some(verdict.earned());
            }
        }
    });

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(on_message))
        .branch(Update::filter_callback_query().endpoint(on_callback));

    // The long poll is teloxide's own, unchanged, with two things listened to as it runs: the
    // stream being driven (the hub is still asking) and every refusal (somebody else may have the
    // answers). `dispatch()` would build the same listener and throw both away.
    let updates = WatchedUpdates::new(
        teloxide::update_listeners::polling_default(bot.clone()).await,
        Arc::clone(&health),
    );
    let refused = Arc::clone(&health);
    let when_telegram_will_not_hand_them_over = Arc::new(move |e: RequestError| {
        let refused = Arc::clone(&refused);
        async move {
            tell_the_health_what_telegram_refused(&refused, &e, Instant::now());
            tracing::warn!(error = %e, "Telegram would not hand over the operator's taps");
        }
    });

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![ctx])
        .distribution_function(which_conversation)
        .enable_ctrlc_handler()
        .build()
        .dispatch_with_listener(updates, when_telegram_will_not_hand_them_over)
        .await;

    Ok(())
}

/// Which conversation an update belongs to, for the one purpose of ordering.
///
/// The client library runs one key's updates strictly one after another and different keys at the
/// same time. Its default key is the CHAT — and this bot's whole herd lives in ONE chat, a forum
/// whose topics are the conversations, so the default made every project's typed line and every
/// tap in the forum queue behind whatever the last one was doing. Fetching a file he sent is
/// twenty megabytes off Telegram, inside that handler, and a tap arriving late is the one thing
/// this product exists to deliver.
///
/// The topic is the right unit and not merely a smaller one: it is exactly the conversation, so
/// "see the screenshot above" still lands behind the screenshot for the agent it was meant for,
/// which is the ordering the fetch was put on this path for in the first place.
///
/// A tap whose message is too old for Telegram to describe carries no topic; it keys on the chat,
/// which orders it with General and is the safe answer rather than a guess.
fn which_conversation(u: &Update) -> Option<(i64, i32)> {
    use teloxide::types::UpdateKind;
    let chat = u.chat()?.id.0;
    let topic = match &u.kind {
        UpdateKind::Message(m) | UpdateKind::EditedMessage(m) => m.thread_id,
        UpdateKind::CallbackQuery(q) => q
            .message
            .as_ref()
            .and_then(|m| m.regular_message())
            .and_then(|m| m.thread_id),
        _ => None,
    };
    Some((chat, topic.map_or(0, |t| t.0.0)))
}

/// Say at startup who may speak anywhere this bot listens, and where each of them came from.
///
/// Said in the same breath as the chats, and said loudly when it is nobody, because this is the
/// line that stops a restart into this build from silently answering no one. The default — a
/// private chat on the chat allowlist names its one person — is what keeps a configuration written
/// before people existed working unchanged, so the line says which people came from where rather
/// than only how many. Split from `serve` so a test can read the lines back without a bot token.
/// The gate answers "is it nobody", not this function: one fail-closed answer, in one place.
fn announce_who_may_speak(chats: &BTreeSet<i64>, listed: &BTreeSet<i64>, gate: &Gate) {
    let from_private_chats = crate::config::people_from_private_chats(chats);
    if gate.nobody_may_speak() {
        tracing::warn!(
            "NOBODY may speak to this bot: no private chat is on the chat allowlist to name a \
             person, and HERDR_TG_ALLOWED_USER_IDS (or `allowed_user_ids` in herdr-tg.toml) is \
             not set. Every typed line and every tap will be dropped, and only a person let into \
             one project with `herdr-tg allow <repo> <user>` can be heard there. Add your own \
             private chat to HERDR_TG_ALLOWED_CHAT_IDS, or list your user id."
        );
    } else {
        tracing::info!(
            people = ?gate.people,
            listed = listed.len(),
            from_private_chats = from_private_chats.len(),
            "these people may speak anywhere this bot listens — the listed ones, and one per \
             private chat on the chat allowlist, because a private chat's id is its person's id"
        );
    }
    // One line PER private chat, because the count above is easy to read past and what it hides
    // is a grant: a private chat on the CHAT allowlist used to mean "may DM the bot", and it now
    // also means "may type at every agent and tap every button". The operator's own chat is the
    // shape this default exists for, and is said at info. Two or more is the exact case where
    // somebody was let in for a narrower reason — a teammate's chat, added so `/projects` works
    // for him in private — and that is said at warn, naming the narrower verb, so the wider grant
    // is a thing he read rather than a thing that happened.
    for id in &from_private_chats {
        if from_private_chats.len() > 1 {
            tracing::warn!(
                chat = id,
                "is a private chat on the chat allowlist, so this person may type at EVERY agent \
                 and tap EVERY button anywhere this bot listens. If they should only reach one \
                 project, take the chat off HERDR_TG_ALLOWED_CHAT_IDS and run: herdr-tg allow \
                 <repo> {id}"
            );
        } else {
            tracing::info!(
                chat = id,
                "is a private chat on the chat allowlist, so this person may type at EVERY agent \
                 and tap EVERY button anywhere this bot listens. If they should only reach one \
                 project, take the chat off HERDR_TG_ALLOWED_CHAT_IDS and run: herdr-tg allow \
                 <repo> {id}"
            );
        }
    }
}

/// A tap on one of the hub's buttons.
async fn on_callback(bot: Bot, q: CallbackQuery, ctx: Ctx) -> anyhow::Result<()> {
    let chat_id = q.message.as_ref().map(|m| m.chat().id.0).unwrap_or(0);
    if !ctx.gate.admit(chat_id) {
        tracing::warn!(
            chat_id,
            "callback from a chat NOT on the allowlist — ignored"
        );
        return Ok(());
    }
    // The person, second — and before the button is so much as read. The conversation the button
    // belongs to comes from the ledger, so a project's own people can answer their project's
    // questions; a button nothing was written down beside has none, and for that only the
    // bot-wide list can answer. A stranger must not learn from a reply whether the button was
    // real, so this is decided ahead of every branch below that says anything back.
    let user = who_tapped(&q);
    let at = match (&ctx.hub, q.message.as_ref()) {
        (Some(hub), Some(m)) => hub
            .ledger
            .lock()
            .await
            .get(chat_id, &hub_proto::MsgId::new(m.id().0.to_string()))
            .map(|r| r.addr()),
        _ => None,
    };
    if !standing_of(&ctx, user, at.as_ref()).await.may_speak_here() {
        refused_sender(&ctx, user, chat_id, at.as_ref(), "a tap").await;
        return Ok(());
    }
    let Some(data) = q.data.clone() else {
        return Ok(());
    };

    // Which topic the answer belongs in. From the update itself, and only then overridden by the
    // ledger: taking it from the ledger alone put every record-less refusal — the most common
    // outcome of a tap that goes wrong — in the forum's General topic instead of beside the
    // question the operator was looking at.
    let mut reply_thread: Option<i32> = q
        .message
        .as_ref()
        .and_then(|m| m.regular_message())
        .and_then(|m| m.thread_id)
        .map(|t| t.0.0);

    // The id his answer went down the wire under, once it has. Kept here because the two halves of
    // one tap are decided in two places: the frame goes down while the answer is being worked out,
    // and which message his receipt IS can only be known after Telegram has made it.
    let mut tap_went_down: Option<hub_proto::FrameId> = None;

    let answer = match data.split('|').collect::<Vec<_>>().as_slice() {
        // The option id is opaque. What it MEANS was written down in the ledger beside the message
        // when the question went out, and that record is what resolves it — never the button's
        // position, which is how one reading "Reject" once confirmed "Allow always".
        [crate::surface::CALLBACK_PREFIX, option] => match &ctx.hub {
            None => WhatToSay::and_in_the_topic(escape_html(
                "The hub is not running, so I cannot pass that on.",
            )),
            Some(hub) => {
                let msg_id = q
                    .message
                    .as_ref()
                    .map(|m| hub_proto::MsgId::new(m.id().0.to_string()));
                let option_id = hub_proto::OptionId::new(*option);
                match msg_id {
                    None => WhatToSay::and_in_the_topic(escape_html(
                        "I cannot tell which question that button belongs to.",
                    )),
                    Some(msg_id) => {
                        if let Some(topic) = hub
                            .ledger
                            .lock()
                            .await
                            .get(chat_id, &msg_id)
                            .map(|r| r.topic_id)
                        {
                            reply_thread = Some(topic);
                        }
                        match hub.resolve_tap(chat_id, user, &msg_id, &option_id).await {
                            // A chat this bot does not answer gets silence, not a refusal.
                            Err(crate::hub::TapRefusal::NotYours) => return Ok(()),
                            Err(why) => a_refused_tap_says(&why),
                            Ok((who, ask_id, option_id)) => {
                                let label = hub
                                    .ledger
                                    .lock()
                                    .await
                                    .get(chat_id, &msg_id)
                                    .and_then(|r| {
                                        r.options
                                            .iter()
                                            .find(|o| o.option_id == option_id)
                                            .map(|o| o.label.clone())
                                    })
                                    .unwrap_or_default();
                                // Down under an id the hub has already written the tap down
                                // beside. Sending it with `deliver` minted that id inside the send
                                // and threw it away, and the `true` that came back meant only that
                                // the outbox took the frame — so a bridge that said on the wire it
                                // could not act on the answer named a frame the hub held no record
                                // of, its ack was dropped, and the receipt below stayed "Sent"
                                // whatever became of the tap. The record has to exist before the
                                // frame is on the wire: an ack can arrive the moment it is.
                                let went_down = hub
                                    .deliver_tap(&who, chat_id, &msg_id, ask_id, option_id, &label)
                                    .await;
                                if let Some(frame) = went_down {
                                    tap_went_down = Some(frame);
                                    // Retire the keyboard rather than only forgetting the record.
                                    // Forgetting alone left the menu live forever: the record the
                                    // later `ask_resolved` needed was already gone, so nothing ever
                                    // took the buttons away.
                                    hub.answered_from_phone(chat_id, &msg_id, &label).await;
                                    WhatToSay::and_in_the_topic(format!(
                                        "Sent: {}",
                                        escape_html(&label)
                                    ))
                                } else {
                                    // Withdrawn, not merely reported. The tap was already written
                                    // down as answered — it has to be, or a second tap in the round
                                    // trip would deliver twice — so leaving it there burned the
                                    // question: the keyboard stayed live and could only ever answer
                                    // "that has already been answered, I have not sent anything",
                                    // which is false in the half he cares about.
                                    let what = hub.withdraw_undelivered(chat_id, &msg_id).await;
                                    WhatToSay::and_in_the_topic(escape_html(
                                        a_tap_that_reached_nobody(what),
                                    ))
                                }
                            }
                        }
                    }
                }
            }
        },
        _ => WhatToSay::and_in_the_topic(escape_html("I don't recognise that button.")),
    };

    // Answer the query first, or Telegram leaves a spinner on the button.
    //
    // NOT metered, and knowingly: whether `answerCallbackQuery` is charged against the per-minute
    // group ceiling was not measured on 3 September and is still open (`docs/RATE-PROBE.md`). It is
    // one per tap and paced by a human thumb, so guessing wrong costs at most one token an operator
    // action — and spending a token for something that may cost nothing would take that token off
    // an agent for no reason.
    let _ = bot
        .answer_callback_query(q.id.clone())
        .text(toast(&answer.answer))
        .await;
    // Not every answer earns a message. A refusal that only says his tap changed nothing is
    // already on the button he is looking at, and repeating it under the question spends a send
    // per tap on a keyboard he is going to keep tapping.
    // Whether the hub was handed the message his receipt is. A tap that went down and whose receipt
    // never existed has to be said so exactly once, below: the hub cannot tell "not yet" from
    // "never" on its own, and it waits for ever on the difference.
    let mut receipt_handed_over = false;
    if let Some(msg) = q.message.as_ref().filter(|_| answer.in_the_topic) {
        // This one IS a message, so it comes out of the chat's budget — through the path that
        // cannot refuse, because he tapped a button and the answer to that is not an agent's
        // message to be rationed. It fires precisely while he is looking at a busy forum, which is
        // exactly when the budget is thin: the tap was caused by traffic.
        let chat = msg.chat().id;
        told_the_operator(&ctx, chat.0).await;
        let mut out = bot
            .send_message(chat, &answer.answer)
            .parse_mode(ParseMode::Html);
        if let Some(thread) = reply_thread {
            out = out.message_thread_id(ThreadId(MessageId(thread)));
        }
        // The answer is READ. It used to be discarded outright, so a flood wait discovered on the
        // one message that must never be lost left him having tapped a button and been told
        // nothing — while the ledger already said the question was answered — and the budget never
        // heard about the refusal either.
        let answered = out.await;
        // Which message his receipt is — the line that says what he chose — written down beside
        // the tap it belongs to. This is the only place that can ever know it: the hub did not
        // send it. Without it, what the bridge says became of the answer has no line to change,
        // and correcting a tap would cost a send per tap on a keyboard he is still looking at.
        // Nothing is handed over when Telegram refused the send: an id invented here would be an
        // edit of somebody else's message.
        if let (Some(hub), Some(frame), Ok(receipt)) = (&ctx.hub, &tap_went_down, &answered) {
            hub.his_receipt_for_a_tap(frame, &hub_proto::MsgId::new(receipt.id.0.to_string()))
                .await;
            receipt_handed_over = true;
        }
        what_telegram_said(&ctx, chat.0, answered).await;
    }
    // Telegram refused the send, or there was no message to send it under. Either way the line that
    // would have said "Sent" does not exist and never will, and the hub is told so — otherwise an
    // agent that refuses the answer is answering into a record that waits for a receipt that is not
    // coming, and the operator is left with a keyboard that has gone and no word about his answer.
    // The refusal happens precisely when the forum is busy, which is when he is tapping.
    if let (Some(hub), Some(frame), false) = (&ctx.hub, &tap_went_down, receipt_handed_over) {
        hub.his_receipt_never_arrived(frame).await;
    }
    tracing::info!(chat_id, "handled a button");
    Ok(())
}

async fn on_message(bot: Bot, msg: Message, ctx: Ctx) -> anyhow::Result<()> {
    let chat_id = msg.chat.id.0;
    if !ctx.gate.admit(chat_id) {
        tracing::warn!(
            chat_id,
            "message from a chat NOT on the allowlist — ignored. If this is you, add this id to \
             the allowlist."
        );
        return Ok(());
    }

    // The person, second — and before the text is looked at, so a stranger's photo is refused and
    // written down the same as his words would be. The conversation the message belongs to is
    // resolved ONCE here, for the standing and for the relay both: a message belongs to the topic
    // it was typed in and to no other, and that lookup is the whole of routing.
    let user = who_sent(&msg);
    let thread = msg.thread_id.map(|t| t.0.0);
    let at = match (&ctx.hub, thread) {
        (Some(hub), Some(t)) => hub.addr_for_topic(t).await,
        _ => None,
    };
    let standing = standing_of(&ctx, user, at.as_ref()).await;
    if !standing.may_speak_here() {
        refused_sender(&ctx, user, chat_id, at.as_ref(), what_arrived(msg.text())).await;
        return Ok(());
    }
    // His words, or his file and the words under it. A photo used to return here, so a screenshot
    // from his phone — the most natural steering there is — reached nobody, and the caption he
    // wrote under it was dropped with it.
    let (text, files) = match msg.text() {
        Some(text) => (text, Vec::new()),
        None => match what_he_sent(&msg) {
            // No caption is still a message: the file is the message.
            Some(file) => (msg.caption().unwrap_or(""), vec![file]),
            None => {
                if something_the_bot_does_not_carry(&msg) && at.is_some() {
                    told_the_operator(&ctx, chat_id).await;
                    reply(&ctx, &bot, msg.chat.id, thread, &escape_html(NOT_CARRIED)).await;
                }
                return Ok(());
            }
        },
    };

    // A caption is never a command. `/help` written under a photo is a photo with `/help` on it,
    // and the photo is the point; answering the command would drop the photo.
    let typed = if files.is_empty() {
        what_he_typed(text, &ctx.username)
    } else {
        Typed::Steering
    };
    // Somebody else's, by name. Nothing is relayed, nothing is said back, nothing is written
    // down: Telegram hands every group bot every command, and this one was not for us.
    if typed == Typed::ForAnotherBot {
        return Ok(());
    }
    // Standing here, but not for this: a person let into one project asked for every project's
    // name and state. Silence, and the same line — a reply would say what the command does.
    if !may_act(standing, &typed) {
        refused_sender(&ctx, user, chat_id, at.as_ref(), "a command").await;
        return Ok(());
    }
    let cmd = match typed {
        Typed::Command(cmd) => cmd,
        Typed::ForAnotherBot => return Ok(()),
        // Aimed at us by name, and not one we have. Answered where he typed it, and never handed
        // to an agent as if it were steering.
        Typed::NotOneOfMine => {
            told_the_operator(&ctx, chat_id).await;
            reply(
                &ctx,
                &bot,
                msg.chat.id,
                msg.thread_id.map(|t| t.0.0),
                &escape_html("I do not have that command. /help lists the ones I answer."),
            )
            .await;
            return Ok(());
        }
        Typed::Steering => {
            // Not a command: it is something the operator typed at a project. A message belongs to the
            // topic it was typed in and to no other — that is the whole of routing, and every other
            // rule this bridge used to have is deleted rather than tested against.
            let body = match (&ctx.hub, thread, &at) {
                (Some(hub), Some(_), Some(who)) => {
                    let mid = hub_proto::MsgId::new(msg.id.0.to_string());
                    // The message he swiped to reply to, if he did. A reply under one of the
                    // agent's questions is the one time he says which question — and which
                    // session — he means, and the hub decides what that is worth (`relay`). In a
                    // forum topic every message carries the topic's root as its reply, so this
                    // is usually a message nothing was written down beside, which counts as no
                    // reply at all.
                    let under = msg
                        .reply_to_message()
                        .map(|m| hub_proto::MsgId::new(m.id.0.to_string()));
                    let n_files = files.len();
                    if hub
                        .relay_with(who, chat_id, user, &mid, text, under.as_ref(), files)
                        .await
                    {
                        tracing::info!(
                            chat_id, who = %who, bytes = text.len(), files = n_files,
                            "relayed to a project"
                        );
                        // Nothing is said back. A confirmation under every line the operator types
                        // turns a conversation into a receipt printer; the agent's own answer is
                        // the acknowledgement, and it is the one he is waiting for.
                        return Ok(());
                    }
                    // Dropped, visibly, where he typed it — never queued. A message held for a
                    // worker that may never come back is a message he believes was sent.
                    // The topic and not "that project": a lane has its own topic, and the project
                    // it belongs to can be connected and busy while this worktree is not.
                    //
                    // Off before "not connected". A project he switched off at the terminal has
                    // had its connection dropped, so "not connected" would be true and would
                    // send him to restart a bridge the hub is going to refuse.
                    if hub.is_switched_off(who).await {
                        escape_html(
                            "This project is switched off, so nothing was sent. It will not be \
                             delivered later. Switch it back on at a terminal with:  herdr-tg \
                             enable <its folder>",
                        )
                    } else {
                        escape_html(
                            "Nothing is connected in this topic right now, so nothing was sent. \
                             It will not be delivered later.",
                        )
                    }
                }
                // Never a fall back to the project when a lane's topic is unknown: that would put
                // what he typed at one worktree into the turn of an agent working on another.
                (Some(_), Some(_), None) => escape_html(
                    "I do not know which project this topic belongs to, so I have not sent anything.",
                ),
                _ => escape_html(
                    "Type inside a project's topic and I will pass it on. Here in General I do not know \
                 who you mean.",
                ),
            };
            told_the_operator(&ctx, chat_id).await;
            let mut out = bot
                .send_message(msg.chat.id, &body)
                .parse_mode(ParseMode::Html);
            if let Some(t) = thread {
                out = out.message_thread_id(ThreadId(MessageId(t)));
            }
            what_telegram_said(&ctx, chat_id, out.await).await;
            return Ok(());
        }
    };

    let body = match cmd {
        Command::Help => escape_html(&Command::descriptions().to_string()),
        Command::Projects => projects_digest(&ctx).await,
    };
    // Answered where it was asked. An unthreaded reply lands in the forum's General, so a `/projects`
    // typed inside a project's topic was answered somewhere the operator was not looking — and once
    // you live inside topics, which is what more than one project means, that is most of the time.
    told_the_operator(&ctx, chat_id).await;
    reply(&ctx, &bot, msg.chat.id, msg.thread_id.map(|t| t.0.0), &body).await;
    Ok(())
}

/// Take a token for a message to the operator that the hub is not allowed to refuse.
///
/// Three sends live in this file and none of them could see the budget: the confirmation under a
/// tap, the answer to a line typed at a topic with nothing behind it, and a command's reply. They do
/// not scale with the herd — one per tap, one per line, one per command — but the ceiling is per
/// CHAT, so an unmetered one does not cost itself. It costs whichever project sends next, and all
/// three fire while the operator is looking at a busy forum, which is when the budget is thinnest.
///
/// It cannot refuse and must not: he did something, and the answer is not an agent's message to be
/// rationed. What it does is make the ceiling able to SEE these, so the pacing that protects
/// everything else is working from the real number.
///
/// A hub of `None` is the box where the forum has never been set up. There is no budget to spend
/// from because there is no socket half running at all, so there is nothing to account for.
async fn told_the_operator(ctx: &Ctx, chat_id: i64) {
    a_send_the_hub_could_not_refuse(ctx.hub.as_ref(), chat_id).await;
}

/// The same, over any surface, so the seam can be tested without a bot token.
///
/// The chat is the one the message is actually going to. It used to be the forum unconditionally,
/// and the allowlist can hold more than the forum: a command typed in the operator's other chat
/// took a send off the forum's ceiling — and imposed the one-second rhythm on it — for a message
/// the forum never carried. `Budgets` is keyed per chat exactly so it does not have to.
async fn a_send_the_hub_could_not_refuse<S: crate::hub::Surface>(
    hub: Option<&Arc<crate::hub::Hub<S>>>,
    chat_id: i64,
) {
    if let Some(hub) = hub {
        hub.account_for_a_send_that_could_not_be_refused(chat_id)
            .await;
    }
}

/// Read what Telegram answered one of the sends the hub is not allowed to refuse.
///
/// All three of them used to throw the answer away — two on a bare `let _ =`, one on a log line —
/// so the `429` the operator's own tap earned never reached the budget, and the very next agent
/// message walked into the same wall. That is the exact failure the backpressure beside this
/// exists to close, and it had two blind spots out of five paths.
async fn what_telegram_said<T>(ctx: &Ctx, chat_id: i64, answered: Result<T, RequestError>) {
    telegram_answered(ctx.hub.as_ref(), chat_id, answered).await;
}

/// The same, over any surface. See [`a_send_the_hub_could_not_refuse`] for why the split exists.
async fn telegram_answered<S: crate::hub::Surface, T>(
    hub: Option<&Arc<crate::hub::Hub<S>>>,
    chat_id: i64,
    answered: Result<T, RequestError>,
) {
    let Err(e) = answered else { return };
    match crate::surface::flood_wait(&e) {
        // Never silent, and never only a log line. A rejected send used to be one error log and a
        // drop, and that had already lost 5,164 characters of a real agent's longest message.
        Some(wait) => {
            tracing::error!(
                chat = chat_id,
                seconds = wait.as_secs(),
                "the operator was NOT told, because Telegram has shut this chat for flooding"
            );
            if let Some(hub) = hub {
                hub.telegram_shut_this_chat(chat_id, wait).await;
            }
        }
        None => tracing::error!(error = %e, chat = chat_id, "the operator was NOT told"),
    }
}

/// Which projects are enrolled, and which have a bridge on the socket right now.
async fn projects_digest(ctx: &Ctx) -> String {
    match &ctx.hub {
        None => escape_html("The hub is not running, so I do not know about any projects."),
        Some(hub) => digest_of(hub.as_ref()).await,
    }
}

/// The digest itself, over any surface — so it can be tested without a bot token, which is the only
/// way a test of this can exist at all: the operator's phone is not a fixture.
///
/// `pub(crate)` for one reason: what this list must SHOW is a property of how the hub addresses a
/// conversation, and proving it needs claims taken by real bridges over a real socket. That harness
/// lives beside the hub, and duplicating it here would be a second imagination of a bridge.
pub(crate) async fn digest_of<S: crate::hub::Surface>(hub: &crate::hub::Hub<S>) -> String {
    // Read who is connected FIRST, and be finished with that lock before the registry is taken.
    // The other order would hold the registry while waiting on the claims map — and every path that
    // sends a message takes the registry, so one contended lock would stall the whole fleet's
    // outgoing messages behind a status list somebody typed.
    let connected = hub.connected_ids().await;
    // Split ONCE, into a project's own voice and the worktrees of it that are live, so each row
    // below costs a lookup rather than a walk. This list is read on a phone and lanes multiply its
    // rows: twelve worktrees in a day, on top of fourteen projects.
    let mut project_is_connected: BTreeSet<&hub_proto::ProjectId> = BTreeSet::new();
    let mut lanes: std::collections::BTreeMap<&hub_proto::ProjectId, Vec<&hub_proto::LaneId>> =
        Default::default();
    for who in &connected {
        match &who.lane {
            None => {
                project_is_connected.insert(&who.project);
            }
            Some(lane) => lanes.entry(&who.project).or_default().push(lane),
        }
    }
    let mut lines: Vec<String> = {
        let mut registry = hub.registry.lock().await;
        // Fresh, because `herdr-tg enroll` runs at a terminal while the hub is running. Rendered
        // from the boot-time snapshot, a project enrolled since was absent from the only fleet view
        // there is — not shown as disconnected, simply not there.
        if let Err(e) = registry.reread() {
            // The last good copy is still better than nothing, and refusing to answer a status
            // question over a transient read error tells him less than a slightly old list does.
            tracing::error!(error = %e, "could not re-read the project list; showing the last good copy");
        }
        registry
            .all()
            // A room nobody has started yet is a row and a secret and nothing he can see. A
            // seed's book is minted whole, so without this his fleet list grows a row of nothing
            // per vacant room; the moment a topic binds, the room appears on its own. The
            // registry's own predicate, decided from facts no writer of the file can drop.
            .filter(|p| !crate::registry::is_nothing_yet(p))
            .map(|p| {
                // One set lookup per project rather than a walk per project: this list is read on a
                // phone and it is meant to grow to fourteen rows.
                // Three states out of what the row already holds. Liveness alone loses the one
                // permanent fact the old row had: a topic exists only once a bridge has been live
                // at least once, so a project whose plugin was never installed is not the same as
                // one that ran this morning and is idle now. At fourteen rows those two printed
                // the same line, and "did it ever dial in at all" is the first question a second
                // enrolment raises.
                // The project's OWN voice, and only that. A project whose worktrees are busy while
                // it is idle is exactly that, and saying "connected" because something of its
                // family is would hide which of them he can actually type at.
                //
                // A repo whose sessions are ALL dispatched into worktrees never binds a topic of
                // its own, so it would say "has never connected" directly above a row saying a
                // worktree of it is connected — a pair that contradicts itself, and the ordinary
                // row for such a repo rather than an edge case.
                //
                // OFF comes first, before liveness. A switched-off project has had its claim
                // dropped within a second of the switch, so "not connected" would be true and
                // useless: it reads as a project between sessions and sends him to restart a
                // bridge that the hub will refuse. Off is his own decision, so the list says so.
                let where_it_is = if !p.enabled {
                    "switched off"
                } else if project_is_connected.contains(&p.id) {
                    "connected"
                } else if lanes.contains_key(&p.id) {
                    "not connected itself — only its other conversations are"
                } else if p.topic_id.is_some() {
                    "not connected"
                } else {
                    "has never connected"
                };
                let mut row = format!(
                    "<b>{}</b> — {}",
                    escape_html(&p.title),
                    escape_html(where_it_is)
                );
                // Its live worktrees, indented under it, so he can tell a project from a worktree of
                // one at a glance — a lane rendered as a peer reads as a repo he never enrolled.
                //
                // Only the LIVE ones. A lane's topic outlives the lane by design, and listing every
                // worktree that ever ran would put a whole day's history in a status message.
                //
                // No ", connected" on the end: only live worktrees are ever listed, so the word
                // carries nothing and costs width on the narrowest screen this is read on.
                for lane in lanes.get(&p.id).into_iter().flatten() {
                    row.push_str(&format!(
                        "\n   ↳ {} — a conversation of it",
                        escape_html(lane.as_str())
                    ));
                }
                row
            })
            .collect()
    };
    // Sorted by the name he gave each project, because the registry hands them over in project-id
    // order — a hashed `p-…` string, neither alphabetical nor the order he enrolled them in. That
    // was survivable at three rows; with a row per live worktree beneath each project he has no way
    // to predict where anything sits. `<b>` is a constant prefix, so sorting the rendered rows
    // sorts by title, and a project's worktrees ride inside its own row string.
    lines.sort();
    if lines.is_empty() {
        return escape_html(
            "Nothing is enrolled yet. Enrol a project at the terminal with: herdr-tg enroll <repo>",
        );
    }
    lines.join("\n")
}

/// What the operator reads when his tap reached nobody.
fn a_tap_that_reached_nobody(what: crate::hub::Withdrawal) -> &'static str {
    // One sentence each. Taking the question back is the keyboard AND the record, and the edit that
    // removes the keyboard fails on any Telegram 5xx, any flood wait, and any message past the
    // 48-hour edit window — so "I have taken the buttons away", said unconditionally, was read by
    // someone looking straight at them. Every one of these says the half he cares about most first:
    // nothing was sent.
    //
    // The subject is the TOPIC, not the project — these are replied into the thread the tap came
    // from, and that is a worktree's own topic as often as a project's. Its project can be
    // connected and busy in the topic right beside it, which makes "that did not reach the
    // project" a sentence he can read as plainly false about the thing he is looking at.
    match what {
        crate::hub::Withdrawal::Retired => {
            "That did not reach whatever is running here, so nothing was sent. I have taken the \
             buttons away — a menu that cannot answer is worse than none."
        }
        crate::hub::Withdrawal::StillOnHisPhone => {
            "That did not reach whatever is running here, so nothing was sent. I could not take \
             the buttons off this one either, so they are still there — tap again and I will try \
             it once more."
        }
        crate::hub::Withdrawal::NothingLeftToTakeBack => {
            "That did not reach whatever is running here, so nothing was sent. It has since \
             finished with that question at its own end, so there is nothing left to answer."
        }
    }
}

async fn reply(ctx: &Ctx, bot: &Bot, chat: ChatId, thread: Option<i32>, html: &str) {
    let mut out = bot
        .send_message(chat, fit(html.to_owned()))
        .parse_mode(ParseMode::Html);
    if let Some(t) = thread {
        out = out.message_thread_id(ThreadId(MessageId(t)));
    }
    what_telegram_said(ctx, chat.0, out.await).await;
}

/// Clip to what Telegram will take, on a character boundary.
///
/// Splitting a multi-byte character produces a body that is not valid UTF-8, which the API rejects
/// outright — turning a long message into one that does not arrive at all.
fn fit(html: String) -> String {
    if html.chars().count() <= BODY_BUDGET {
        return html;
    }
    const TAIL: &str = "\n… (clipped)";
    let room = BODY_BUDGET.saturating_sub(TAIL.chars().count());
    let kept: String = html.chars().take(room).collect();
    format!("{kept}{TAIL}")
}

/// What the operator is told after a tap, and where.
///
/// Two channels, and they are not one message. The toast sits on the button he just pressed and
/// costs nothing; the line under the question is permanent and costs a send out of the chat's
/// budget — spent precisely while he is looking at a busy forum, because the traffic is what made
/// him tap in the first place.
struct WhatToSay {
    /// The HTML he reads. Toasted always, whatever else happens to it.
    answer: String,
    /// Whether it is also posted under the question.
    in_the_topic: bool,
}

impl WhatToSay {
    /// The ordinary case: he reads it on the button and under the question both.
    fn and_in_the_topic(answer: String) -> Self {
        Self {
            answer,
            in_the_topic: true,
        }
    }

    /// The toast is the whole message.
    fn the_toast_alone(answer: String) -> Self {
        Self {
            answer,
            in_the_topic: false,
        }
    }
}

/// What a refused tap says, and whether he reads it under the question as well as on the button.
///
/// Two of these mean "your tap changed nothing", said to a man looking at the button he just
/// pressed. The toast is already on it, so the toast is the whole message: posting the sentence a
/// second time told him nothing he was not reading, and spent a send doing it. On the keyboard he
/// actually retries — the one whose retirement Telegram refused, still live and still answering
/// every tap the same way — that was a send per tap out of the budget the agents share.
///
/// Everything else still posts. "Nothing is connected in this topic" and "I have no record of that
/// question" are news he has to act on, and a toast is gone the moment he looks away.
fn a_refused_tap_says(why: &crate::hub::TapRefusal) -> WhatToSay {
    let answer = escape_html(why.say());
    match why {
        crate::hub::TapRefusal::AlreadyAnswered | crate::hub::TapRefusal::NoLongerAsked => {
            WhatToSay::the_toast_alone(answer)
        }
        _ => WhatToSay::and_in_the_topic(answer),
    }
}

/// A callback answer is a toast: short, plain, and stripped of the markup the message carries.
fn toast(html: &str) -> String {
    let plain = crate::render::plain_text(html);
    plain.chars().take(180).collect()
}

#[cfg(test)]
mod key_tests {
    use super::which_conversation;
    use teloxide::types::Update;

    /// One update, from the shape the Bot API documents for a forum: `message_thread_id` and
    /// `is_topic_message` beside the chat.
    fn in_topic(update_id: i64, message_id: i64, thread: Option<i64>) -> Update {
        let thread = thread.map_or(String::new(), |t| {
            format!(r#""message_thread_id":{t},"is_topic_message":true,"#)
        });
        let json = format!(
            r#"{{"update_id":{update_id},"message":{{"message_id":{message_id},{thread}"date":1675229140,"chat":{{"id":-1001,"type":"supergroup","title":"the forum","is_forum":true}},"from":{{"id":7,"is_bot":false,"first_name":"the operator"}},"text":"a line"}}}}"#
        );
        serde_json::from_str(&json).expect("the Bot API's own shape for a forum message")
    }

    #[test]
    fn two_topics_of_one_forum_are_two_conversations_and_never_one_queue() {
        // The whole herd lives in one chat. Keyed on the chat — the client library's default —
        // every project's typed line and every tap in the forum queued behind whatever the last
        // one was doing, and one of his files is twenty megabytes off Telegram inside that
        // handler. Keyed on the topic, a slow conversation is slow by itself.
        let engineering = which_conversation(&in_topic(1, 5, Some(4))).expect("a key");
        let docs = which_conversation(&in_topic(2, 6, Some(9))).expect("a key");
        assert_ne!(
            engineering, docs,
            "two topics of one forum share a queue, so one file holds the other's taps"
        );
        // And within one conversation the order is still the order: this is what keeps "see the
        // screenshot above" behind the screenshot.
        assert_eq!(
            engineering,
            which_conversation(&in_topic(3, 7, Some(4))).expect("a key")
        );
        // General has no topic of its own and keys on the chat, which is the safe answer.
        assert_ne!(
            engineering,
            which_conversation(&in_topic(4, 8, None)).expect("a key")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The accept loop's pacing was four lines inside a spawned task with no test on it, and the
    /// transport seam rewrote every one of them. These are the four properties those lines had:
    /// the first wait is short, it doubles, it stops doubling, and a connection clears it. Lose
    /// the last one and the hub that had a bad minute at breakfast is still deaf for five seconds
    /// at a time at lunch; lose the ceiling and it is deaf for longer every hour, with the
    /// Telegram half up and nothing anywhere saying the bridges cannot get in.
    #[test]
    fn the_wait_after_a_door_that_would_not_open_doubles_to_a_ceiling_and_one_connection_clears_it()
    {
        let mut pacing = AcceptPacing::new();
        assert_eq!(pacing.wait_after_the_door_would_not_open(), FIRST_WAIT);
        assert_eq!(
            pacing.wait_after_the_door_would_not_open(),
            Duration::from_millis(100)
        );
        assert_eq!(
            pacing.wait_after_the_door_would_not_open(),
            Duration::from_millis(200)
        );

        for _ in 0..10 {
            pacing.wait_after_the_door_would_not_open();
        }
        assert_eq!(
            pacing.wait_after_the_door_would_not_open(),
            LONGEST_WAIT,
            "the wait has to stop growing somewhere"
        );

        pacing.a_connection_came_through();
        assert_eq!(
            pacing.wait_after_the_door_would_not_open(),
            FIRST_WAIT,
            "a connection got through, so the squeeze is over and the next failure starts again \
             at the shortest wait"
        );
    }

    #[test]
    fn a_refusal_that_only_says_his_tap_changed_nothing_never_costs_a_send() {
        use crate::hub::TapRefusal;

        // These two say "your tap changed nothing" to a man looking at the button he just
        // pressed — the toast is already on it. Posting the same sentence under the question as
        // well bought him nothing and cost a send out of the chat's budget, one per retry, on
        // exactly the keyboard he retries: the one whose retirement Telegram refused, which is
        // still live and still answers every tap the same way.
        for quiet in [TapRefusal::AlreadyAnswered, TapRefusal::NoLongerAsked] {
            let said = a_refused_tap_says(&quiet);
            assert!(
                !said.in_the_topic,
                "a stuck keyboard spends a send every time he taps it: {}",
                said.answer
            );
            assert!(
                !said.answer.is_empty(),
                "the toast is the whole message here, so it has to say something"
            );
        }

        // Every other refusal is news he has to act on — nothing is connected, the question is
        // not one this bot wrote down — and a toast is gone the moment he looks away.
        for loud in [
            TapRefusal::NoRecord,
            TapRefusal::NotAnOption,
            TapRefusal::NotConnected,
            TapRefusal::Restarted,
        ] {
            let said = a_refused_tap_says(&loud);
            assert!(
                said.in_the_topic,
                "he can only read this while he is looking at the button: {}",
                said.answer
            );
        }
    }

    #[test]
    fn an_empty_allowlist_answers_nobody() {
        // The opposite convention would turn a misconfiguration into an open bot.
        let g = Gate::new(BTreeSet::new(), BTreeSet::new());
        assert!(g.is_deaf());
        assert!(!g.admit(1));
        assert!(!g.admit(0));
        assert!(!g.admit(-1001));
    }

    #[test]
    fn only_the_listed_chats_are_admitted() {
        let g = Gate::new([7i64, -1001].into_iter().collect(), BTreeSet::new());
        assert!(g.admit(7) && g.admit(-1001));
        assert!(!g.admit(8) && !g.admit(0));
    }

    #[test]
    fn a_long_body_is_clipped_within_telegram_s_limit() {
        let out = fit("x".repeat(TELEGRAM_MAX_CHARS * 2));
        assert!(out.chars().count() <= BODY_BUDGET);
        assert!(out.ends_with("(clipped)"));
    }

    #[test]
    fn a_clip_never_splits_a_character() {
        let out = fit("🙂".repeat(BODY_BUDGET));
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        assert!(out.chars().count() <= BODY_BUDGET);
    }

    /// A hub needs a surface to exist. Nothing below sends, creates or retires anything, and every
    /// method here says so out loud rather than returning a plausible value — his phone is not a
    /// test fixture, and a fake that quietly succeeded would hide a test that had started using it.
    struct NoSurface;

    impl crate::hub::Surface for NoSurface {
        async fn create_topic(
            &self,
            _title: &str,
            _icon_color: u8,
        ) -> Result<i32, crate::hub::Refused> {
            unreachable!("the project list creates no topics")
        }
        async fn send(
            &self,
            _topic_id: i32,
            _text: &str,
            _buttons: &[hub_proto::AskOption],
            _reply_to: Option<&hub_proto::MsgId>,
        ) -> crate::hub::SendOutcome {
            unreachable!("the project list sends nothing")
        }
        async fn say_in_general(&self, _text: &str) -> crate::hub::SendOutcome {
            unreachable!("the project list says nothing in the forum")
        }
        async fn rewrite(&self, _msg_id: &hub_proto::MsgId, _text: &str) -> anyhow::Result<()> {
            unreachable!("the project list rewrites nothing")
        }
        async fn retire_buttons(
            &self,
            _topic_id: i32,
            _msg_id: &hub_proto::MsgId,
            _original: &str,
            _note: &str,
        ) -> anyhow::Result<()> {
            unreachable!("the project list retires nothing")
        }
        async fn mark(
            &self,
            _chat_id: i64,
            _msg_id: &hub_proto::MsgId,
            _mark: crate::hub::Mark,
        ) -> Result<(), crate::hub::Refused> {
            unreachable!("the project list marks nothing")
        }
        async fn locate(&self, _: &str) -> Result<crate::hub::Located, crate::hub::Refused> {
            unreachable!("the project list fetches nothing")
        }
        async fn download(
            &self,
            _: &str,
            _: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
        ) -> Result<(), crate::hub::Refused> {
            unreachable!("the project list downloads nothing")
        }
        async fn send_file(
            &self,
            _: i32,
            _: &crate::hub::Upload,
            _: &str,
        ) -> crate::hub::SendOutcome {
            unreachable!("the project list uploads nothing")
        }
    }

    /// A hub over a surface that never sends, with a forum and one other allowed chat.
    fn a_hub(dir: &std::path::Path, forum: i64, also: i64) -> Arc<crate::hub::Hub<NoSurface>> {
        Arc::new(crate::hub::Hub::new(
            Arc::new(NoSurface),
            crate::registry::Registry::load(dir.join("projects.json")),
            crate::hub::AskLedger::load(dir.join("asks.json")),
            crate::hub::HubAudit::new(dir.join("hub.audit.log")),
            vec![forum, also],
            vec![OPERATOR],
            forum,
        ))
    }

    /// The one person these tests let speak anywhere.
    const OPERATOR: i64 = 7;

    #[tokio::test]
    async fn a_flood_wait_on_a_message_the_hub_could_not_refuse_shuts_the_chat_for_the_agents_too()
    {
        // Three sends in this file are the operator's own and cannot be refused — the confirmation
        // under a tap, the answer to a line typed at a topic with nothing behind it, and a
        // command's reply — and all three used to throw away whatever Telegram answered them. Two
        // on a bare `let _ =`, one on a log line. So the `429` his own tap earned never reached the
        // budget and the very next agent message walked into the same wall, which is the exact
        // failure the backpressure elsewhere exists to close. They fire while he is looking at a
        // busy forum, because the traffic is what made him tap.
        const FORUM: i64 = -1001;
        let dir = tempfile::tempdir().expect("tmp");
        let hub = a_hub(dir.path(), FORUM, 77);
        assert!(
            !hub.a_send_would_be_refused(FORUM).await,
            "this test needs a chat with room left in it"
        );

        telegram_answered::<NoSurface, ()>(
            Some(&hub),
            FORUM,
            Err(RequestError::RetryAfter(
                teloxide::types::Seconds::from_seconds(41),
            )),
        )
        .await;

        assert!(
            hub.a_send_would_be_refused(FORUM).await,
            "a flood wait discovered on the operator's own message never reached the budget, so \
             the next agent send walks into the same wall"
        );
    }

    #[tokio::test]
    async fn a_send_in_another_allowed_chat_is_never_charged_to_the_forum() {
        // The allowlist holds more than the forum on the live box, and every unrefusable send used
        // to be charged to the forum whatever chat it was going to. A `/help` typed in his other
        // chat took a send off the herd's ceiling — and imposed the one-second rhythm on the forum
        // too — for a message the forum never carried.
        const FORUM: i64 = -1001;
        const SOMEWHERE_ELSE: i64 = 77;
        let dir = tempfile::tempdir().expect("tmp");
        let hub = a_hub(dir.path(), FORUM, SOMEWHERE_ELSE);

        for _ in 0..crate::queue::PER_MINUTE {
            a_send_the_hub_could_not_refuse(Some(&hub), SOMEWHERE_ELSE).await;
        }
        assert!(
            !hub.a_send_would_be_refused(FORUM).await,
            "typing in another chat spent the forum's ceiling, so a project would be shed for \
             messages the forum never carried"
        );
        assert!(
            hub.a_send_would_be_refused(SOMEWHERE_ELSE).await,
            "the chat that actually carried them did not pay for them either, so nothing is \
             pacing it at all"
        );
    }

    #[tokio::test]
    async fn the_project_list_calls_a_project_connected_only_while_its_bridge_is_on_the_socket() {
        // The only fleet view there is, and with one project the operator could tell what it meant
        // by knowing. It rendered `topic_id` — which records that a topic was ever created, is
        // permanent from the first connection on, and says nothing whatever about now — and it read
        // a snapshot of the registry taken when the process booted, so a project enrolled since was
        // missing from the list altogether. With two projects, typing at each topic in turn is the
        // only way to learn the truth.
        let dir = tempfile::tempdir().expect("tmp");
        let file = dir.path().join("projects.json");
        let mut registry = crate::registry::Registry::load(&file);

        let idle = dir.path().join("llm-gateway");
        let busy = dir.path().join("herdr-tg");
        std::fs::create_dir_all(&idle).expect("dir");
        std::fs::create_dir_all(&busy).expect("dir");
        let (idle, _) = registry.enrol(&idle).expect("enrols");
        let (busy, _) = registry.enrol(&busy).expect("enrols");
        // The idle one has run before, so it is bound to a topic and keeps it forever. That binding
        // is exactly the fact the list used to print as "connected".
        registry
            .bind_topic(&crate::hub::Addr::project_itself(idle.id.clone()), 1001)
            .expect("binds");

        let hub = crate::hub::Hub::new(
            Arc::new(NoSurface),
            registry,
            crate::hub::AskLedger::load(dir.path().join("asks.json")),
            crate::hub::HubAudit::new(dir.path().join("hub.audit.log")),
            vec![-1001],
            vec![OPERATOR],
            -1001,
        );

        // Only the busy one has a bridge on the socket — and it has never been given a topic.
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        hub.claim(
            crate::hub::Addr::project_itself(busy.id.clone()),
            std::process::id(),
            "i1".into(),
            tx,
        )
        .await
        .expect("claims");

        // Enrolled at a terminal while the hub is running, which is how a second project arrives.
        let late = dir.path().join("obsidian-link-map");
        std::fs::create_dir_all(&late).expect("dir");
        let (late, _) = crate::registry::Registry::load(&file)
            .enrol(&late)
            .expect("enrols");

        let said = digest_of(&hub).await;
        let line = |title: &str| {
            said.lines()
                .find(|l| l.contains(title))
                .unwrap_or_default()
                .to_owned()
        };

        assert!(
            line(&busy.title).contains("connected") && !line(&busy.title).contains("not connected"),
            "a project whose bridge is on the socket is not shown as connected:\n{said}"
        );
        assert!(
            line(&idle.title).contains("not connected"),
            "a project that merely has a topic from a previous run is shown as connected:\n{said}"
        );
        assert!(
            !line(&late.title).is_empty(),
            "a project enrolled while the hub was running is missing from the list:\n{said}"
        );

        // Three states, not two. The row it replaced carried a permanent-history fact next to the
        // wrong liveness one — "has a topic" is true only of a project whose bridge has been live at
        // least once — and rendering liveness alone threw that away. At fourteen rows, a project
        // whose plugin was never installed then reads exactly like one that ran this morning and is
        // idle now, and "has it ever dialled in at all" is the very first question a second
        // enrolment raises. It costs one `else if` over data the row already holds.
        assert_ne!(
            line(&idle.title).replace(&idle.title, ""),
            line(&late.title).replace(&late.title, ""),
            "a project that has never once connected reads exactly like one that is merely idle, \
             so the only fleet view there is cannot answer whether it ever dialled in:\n{said}"
        );
        assert!(
            line(&late.title).contains("never"),
            "a project that has never connected does not say so:\n{said}"
        );
    }

    #[test]
    fn a_tap_that_reached_nobody_never_says_the_buttons_are_gone_when_they_are_not() {
        // Taking a question back is two things — the keyboard off the phone, and the record — and
        // the edit that removes the keyboard fails on any Telegram 5xx, any flood wait, and every
        // message past the 48-hour edit window. Said unconditionally, "I have taken the buttons
        // away" is read by an operator who is looking straight at them.
        use crate::hub::Withdrawal;
        let gone = a_tap_that_reached_nobody(Withdrawal::Retired);
        let still_there = a_tap_that_reached_nobody(Withdrawal::StillOnHisPhone);
        let nothing_left = a_tap_that_reached_nobody(Withdrawal::NothingLeftToTakeBack);
        assert!(
            gone.contains("taken the buttons away"),
            "the ordinary case stopped saying what happened: {gone}"
        );
        for said in [still_there, nothing_left] {
            assert!(
                !said.contains("taken the buttons away"),
                "he is told the buttons are gone when nothing here knows that: {said}"
            );
        }
        // And the promise of a retry is made only where a retry would actually do something. With
        // no record left, tapping again answers "I have no record of that question".
        assert!(still_there.contains("tap again"));
        assert!(!nothing_left.contains("tap again"));
        for said in [gone, still_there, nothing_left] {
            assert!(
                said.contains("nothing was sent"),
                "the half he cares about most is missing: {said}"
            );
        }
    }

    #[test]
    fn a_tap_that_reached_nobody_talks_about_the_topic_and_not_the_project() {
        // These three are replied into the topic the tap came from, which is now a worktree's own
        // as often as it is a project's. "That did not reach the project" is a sentence he can read
        // as false with the project's topic connected and busy right beside the one he is looking
        // at — the same argument that rewrote `TapRefusal::NotConnected` and the relay-drop reply.
        use crate::hub::Withdrawal;
        for what in [
            Withdrawal::Retired,
            Withdrawal::StillOnHisPhone,
            Withdrawal::NothingLeftToTakeBack,
        ] {
            let said = a_tap_that_reached_nobody(what);
            assert!(
                !said.contains("the project"),
                "a reply into a worktree's topic blames the project: {said}"
            );
        }
    }

    #[test]
    fn a_command_aimed_at_this_bot_by_name_is_a_command_not_steering() {
        // Group habit puts the bot's name after a command, and Telegram's own client inserts it
        // when the command is picked from a menu. The parser was handed a username this bot has
        // never had, so every suffixed command failed to parse, fell through to the relay branch,
        // and `/projects@<the real bot>` was delivered verbatim into a coding agent's turn and
        // written into the audit as words he meant to send.
        assert_eq!(
            what_he_typed("/projects@herd_bot", "herd_bot"),
            Typed::Command(Command::Projects)
        );
        // Usernames are case-insensitive on Telegram, and a phone capitalises things.
        assert_eq!(
            what_he_typed("/help@Herd_Bot", "herd_bot"),
            Typed::Command(Command::Help)
        );
        assert_eq!(
            what_he_typed("/projects", "herd_bot"),
            Typed::Command(Command::Projects)
        );
    }

    #[test]
    fn a_command_aimed_at_another_bot_is_ignored_rather_than_relayed() {
        // Telegram hands a group bot every command typed in the group. One aimed at a different
        // bot by name is neither ours to answer nor his words for an agent, and relaying it would
        // put `/start@somebody_else_bot` into a coding agent's turn.
        assert_eq!(
            what_he_typed("/projects@somebody_else_bot", "herd_bot"),
            Typed::ForAnotherBot
        );
        assert_eq!(
            what_he_typed("/start@somebody_else_bot", "herd_bot"),
            Typed::ForAnotherBot
        );
    }

    #[test]
    fn a_line_whose_first_word_carries_an_at_sign_is_his_words_not_another_bots_command() {
        // The parser splits the FIRST WORD at `@` and checks the name before it has looked for a
        // slash at all, so an `@mention` of a person, an email address or an ssh remote read as a
        // command aimed at some other bot — and the fix for `/x@other_bot` then dropped them in
        // silence: no relay, no reply, nothing written down. That is offer 6 broken by a fix for a
        // neighbouring row: a line he wrote reaching nobody without a word. A line that does not
        // open with a slash is never a command, whatever its first word contains.
        for words in [
            "@alice can you check the deploy",
            "me@example.com is the address",
            "git@github.com:org/repo.git is the remote",
            // Tapping the bot's own name in a group inserts `@name ` into the composer, and the
            // words after it were answered "I do not have that command".
            "@herd_bot hello",
            "@herd_bot",
            "hello@herd_bot",
        ] {
            assert_eq!(
                what_he_typed(words, "herd_bot"),
                Typed::Steering,
                "{words:?} was not relayed as his words"
            );
        }
    }

    #[test]
    fn a_slash_word_with_no_bot_named_is_steering_for_the_agent() {
        // Agents have slash commands of their own, and a bare one typed in a topic is for the
        // agent in it. Only a command this bot HAS is taken; the rest is his words, untouched.
        assert_eq!(what_he_typed("/compact", "herd_bot"), Typed::Steering);
        assert_eq!(
            what_he_typed("/clear and start over", "herd_bot"),
            Typed::Steering
        );
        assert_eq!(what_he_typed("plain words", "herd_bot"), Typed::Steering);
    }

    #[test]
    fn a_command_this_bot_does_not_have_but_was_aimed_at_by_name_is_answered_rather_than_relayed() {
        // He named this bot, so it is not the agent he was talking to. Relaying `/compact@herd_bot`
        // into a turn would hand an agent an instruction addressed to somebody else.
        assert_eq!(
            what_he_typed("/compact@herd_bot", "herd_bot"),
            Typed::NotOneOfMine
        );
    }

    #[test]
    fn no_message_can_switch_a_project_off_or_on() {
        // Admission is terminal-only, and the switch is part of admission: a `/disable` here would
        // let anyone the allowlist admits — or anyone who got hold of his phone — silence a project,
        // and a `/enable` would be a way IN that no keyboard was needed for. The command set is
        // pinned as a closed list, not as an absence of two names, so a third command of any name
        // has to come past this test.
        let names: Vec<String> = Command::bot_commands()
            .into_iter()
            .map(|c| c.command)
            .collect();
        assert_eq!(
            names,
            vec!["/projects".to_owned(), "/help".to_owned()],
            "the command set grew: {names:?}. Switching a project off or on is a decision made at \
             a keyboard, never from a message."
        );
    }

    #[test]
    fn the_command_set_offers_no_way_to_aim_at_a_pane() {
        // The whole reason this binary is safe is that there is nowhere for a keystroke to go.
        // A command that took a pane id would be the first step back.
        let d = Command::descriptions().to_string().to_lowercase();
        for gone in ["/target", "/panes", "pane", "keystroke"] {
            assert!(!d.contains(gone), "the command set mentions {gone}: {d}");
        }
    }

    // ── who may speak ─────────────────────────────────────────────────────────────────────────

    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const THE_FORUM: i64 = -1001;
    /// The project's own topic, and one worktree's, as the hub would have bound them.
    const TOPIC: i32 = 42;
    const LANE_TOPIC: i32 = 43;
    /// Somebody in the forum who is on no list at all.
    const A_STRANGER: i64 = 999_001;
    /// Somebody let into the one project, and nothing wider.
    const GUEST: i64 = 555_001;

    /// A Telegram that only counts. Every API call the bot makes is one TCP connection here, and
    /// each is closed unanswered so the call fails at once. Nothing about a request is read: the
    /// property under test is whether the bot spoke to Telegram at all, and a stranger must
    /// produce no call — not a reply, not a toast, not a reaction.
    async fn a_telegram_that_only_counts() -> (Bot, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let url = reqwest::Url::parse(&format!("http://{}/", listener.local_addr().expect("addr")))
            .expect("url");
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                drop(socket);
            }
        });
        (Bot::new("1:not-a-token").set_api_url(url), calls)
    }

    fn a_person(id: i64) -> serde_json::Value {
        serde_json::json!({"id": id, "is_bot": false, "first_name": "someone"})
    }

    fn a_bot(id: i64) -> serde_json::Value {
        serde_json::json!({"id": id, "is_bot": true, "first_name": "a bot"})
    }

    /// A message as Telegram delivers it, with the sender the test chooses (`Null` for none).
    fn message_json(
        chat: i64,
        thread: Option<i32>,
        from: serde_json::Value,
        body: serde_json::Value,
    ) -> serde_json::Value {
        let mut m = serde_json::json!({
            "message_id": 5,
            "date": 1_700_000_000,
            "chat": {"id": chat, "type": "supergroup", "title": "forum", "is_forum": true},
        });
        for (k, v) in body.as_object().expect("a body").iter() {
            m[k] = v.clone();
        }
        if let Some(t) = thread {
            m["message_thread_id"] = t.into();
            m["is_topic_message"] = true.into();
        }
        if !from.is_null() {
            m["from"] = from;
        }
        m
    }

    fn typed(chat: i64, thread: Option<i32>, from: serde_json::Value, text: &str) -> Message {
        serde_json::from_value(message_json(
            chat,
            thread,
            from,
            serde_json::json!({"text": text}),
        ))
        .expect("a message Telegram would send")
    }

    fn a_photo(chat: i64, thread: Option<i32>, from: serde_json::Value) -> Message {
        serde_json::from_value(message_json(
            chat,
            thread,
            from,
            serde_json::json!({"photo": [
                {"file_id": "f", "file_unique_id": "u", "file_size": 1, "width": 1, "height": 1}
            ]}),
        ))
        .expect("a photo Telegram would send")
    }

    /// A tap on a button under a message in the forum, by whoever the test says.
    fn tapped(
        chat: i64,
        thread: Option<i32>,
        from: serde_json::Value,
        data: &str,
    ) -> CallbackQuery {
        serde_json::from_value(serde_json::json!({
            "id": "q1",
            "from": from,
            "chat_instance": "ci",
            "data": data,
            "message": message_json(
                chat,
                thread,
                a_bot(1),
                serde_json::json!({"text": "Overwrite it?"}),
            ),
        }))
        .expect("a tap Telegram would send")
    }

    /// A forum this bot listens in, with one project bound to [`TOPIC`] and one worktree of it to
    /// [`LANE_TOPIC`], over the REAL Telegram surface aimed at the bot that only counts — so a
    /// send through the hub is a counted call exactly as a reply from the handler is. The operator
    /// is the one person who may speak anywhere.
    async fn a_forum_with_one_project(
        dir: &Path,
        bot: &Bot,
    ) -> (Ctx, PathBuf, Arc<crate::hub::Hub<crate::surface::Telegram>>) {
        let repo = dir.join("herdr-tg");
        std::fs::create_dir_all(&repo).expect("dir");
        let mut registry = crate::registry::Registry::load(dir.join("projects.json"));
        let (p, _) = registry.enrol(&repo).expect("enrols");
        registry
            .bind_topic(&crate::hub::Addr::project_itself(p.id.clone()), TOPIC)
            .expect("binds");
        registry
            .bind_topic(
                &crate::hub::Addr::lane_of(
                    p.id.clone(),
                    hub_proto::LaneId::new("lane-0905-120000-1"),
                ),
                LANE_TOPIC,
            )
            .expect("binds");
        let hub = Arc::new(crate::hub::Hub::new(
            Arc::new(crate::surface::Telegram::new(
                bot.clone(),
                ChatId(THE_FORUM),
            )),
            registry,
            crate::hub::AskLedger::load(dir.join("asks.json")),
            crate::hub::HubAudit::new(dir.join("hub.audit.log")),
            vec![THE_FORUM],
            vec![OPERATOR],
            THE_FORUM,
        ));
        let ctx = Ctx {
            hub: Some(Arc::clone(&hub)),
            gate: Arc::new(Gate::new(
                [THE_FORUM].into_iter().collect(),
                [OPERATOR].into_iter().collect(),
            )),
            username: Arc::from("herd_bot"),
        };
        (ctx, repo, hub)
    }

    /// Every audit line about somebody who was not allowed to speak.
    fn refused_senders(hub: &crate::hub::Hub<crate::surface::Telegram>) -> Vec<String> {
        std::fs::read_to_string(hub.audit.path())
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("sender="))
            .map(str::to_owned)
            .collect()
    }

    #[tokio::test]
    async fn a_command_from_a_person_not_on_the_list_is_ignored() {
        // `/projects` answers with every project's name and state. A stranger in the forum was
        // answered, and so would a person let into ONE project be — a command has no project, so
        // only the bot-wide list may open it. Silence to both, because a reply confirms something
        // is listening; and the operator's own command is still answered, so this is a gate and
        // not a wall.
        let (bot, calls) = a_telegram_that_only_counts().await;
        let d = tempfile::tempdir().expect("tmp");
        let (ctx, repo, hub) = a_forum_with_one_project(d.path(), &bot).await;

        on_message(
            bot.clone(),
            typed(THE_FORUM, Some(TOPIC), a_person(A_STRANGER), "/projects"),
            ctx.clone(),
        )
        .await
        .expect("handled");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a stranger's /projects was answered"
        );

        hub.registry
            .lock()
            .await
            .set_may_speak(&repo, GUEST, true)
            .expect("lets the guest into the one project");
        on_message(
            bot.clone(),
            typed(
                THE_FORUM,
                Some(TOPIC),
                a_person(GUEST),
                "/projects@herd_bot",
            ),
            ctx.clone(),
        )
        .await
        .expect("handled");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a person let into one project read every project's name and state"
        );
        on_message(
            bot.clone(),
            typed(THE_FORUM, Some(TOPIC), a_person(GUEST), "/compact@herd_bot"),
            ctx.clone(),
        )
        .await
        .expect("handled");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a person let into one project was told which commands this bot has"
        );

        on_message(
            bot.clone(),
            typed(THE_FORUM, Some(TOPIC), a_person(OPERATOR), "/help"),
            ctx.clone(),
        )
        .await
        .expect("handled");
        assert!(
            calls.load(Ordering::SeqCst) >= 1,
            "the operator's own command went unanswered"
        );

        let lines = refused_senders(&hub);
        assert_eq!(lines.len(), 3, "one audit line each:\n{lines:?}");
        assert!(
            lines
                .iter()
                .all(|l| l.contains("what=a command") && l.contains("not allowed to speak here")),
            "{lines:?}"
        );
        assert!(
            lines[0].contains(&format!("sender={A_STRANGER}")),
            "{}",
            lines[0]
        );
        assert!(
            lines[1].contains(&format!("sender={GUEST}")),
            "{}",
            lines[1]
        );
    }

    #[tokio::test]
    async fn a_refused_sender_leaves_one_audit_line_and_no_trace_on_the_phone() {
        // Every way in, from somebody on no list: words in a project's topic, in a worktree's, in
        // General, in a topic the hub cannot place; something that is not text; a tap; a command.
        // Each leaves exactly one line in the audit — the only place the operator can learn that
        // a stranger is typing at his agents, and who — and not one call to Telegram: no reply,
        // no toast, no reaction. Silence is correct; a reply confirms something is listening.
        let (bot, calls) = a_telegram_that_only_counts().await;
        let d = tempfile::tempdir().expect("tmp");
        let (ctx, _repo, hub) = a_forum_with_one_project(d.path(), &bot).await;
        let stranger = || a_person(A_STRANGER);

        for (thread, words) in [
            (Some(TOPIC), "hello agent"),
            (Some(LANE_TOPIC), "hello worktree"),
            (None, "hello general"),
            (Some(99), "hello nowhere"),
        ] {
            on_message(
                bot.clone(),
                typed(THE_FORUM, thread, stranger(), words),
                ctx.clone(),
            )
            .await
            .expect("handled");
        }
        on_message(
            bot.clone(),
            a_photo(THE_FORUM, Some(TOPIC), stranger()),
            ctx.clone(),
        )
        .await
        .expect("handled");
        on_callback(
            bot.clone(),
            tapped(
                THE_FORUM,
                Some(TOPIC),
                stranger(),
                &format!("{}|y", crate::surface::CALLBACK_PREFIX),
            ),
            ctx.clone(),
        )
        .await
        .expect("handled");
        on_message(
            bot.clone(),
            typed(THE_FORUM, Some(TOPIC), stranger(), "/projects"),
            ctx.clone(),
        )
        .await
        .expect("handled");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a stranger left a trace on the phone"
        );
        let audit = std::fs::read_to_string(hub.audit.path()).unwrap_or_default();
        let lines: Vec<&str> = audit.lines().collect();
        assert_eq!(
            lines.len(),
            7,
            "one line for each of the seven, and nothing else:\n{audit}"
        );
        for l in &lines {
            assert!(
                l.contains(&format!("sender={A_STRANGER}"))
                    && l.contains("not allowed to speak here"),
                "a line that does not say who, or why: {l}"
            );
            assert!(
                !l.contains("delivered") && !l.starts_with("sent"),
                "something of the stranger's was sent: {l}"
            );
        }
        assert!(
            lines[0].contains("what=words") && lines[0].contains("project=p-"),
            "words in a project's topic are not placed: {}",
            lines[0]
        );
        assert!(
            lines[1].contains("lane="),
            "words in a worktree's topic do not name the worktree: {}",
            lines[1]
        );
        assert!(
            lines[2].contains("project=-"),
            "a line in General names a project: {}",
            lines[2]
        );
        assert!(lines[5].contains("what=a tap"), "{}", lines[5]);
        assert!(lines[6].contains("what=a command"), "{}", lines[6]);
    }

    #[tokio::test]
    async fn a_person_let_into_a_project_is_heard_there_and_the_operator_everywhere() {
        // The other half of the gate: it opens. A guest's words in the project's topic and in its
        // worktree's reach the relay — nothing is connected here, so the honest answer goes back
        // to the topic, which is a call to Telegram — while the same guest in General and in a
        // topic the hub cannot place is a stranger. The operator is heard in all four.
        let (bot, calls) = a_telegram_that_only_counts().await;
        let d = tempfile::tempdir().expect("tmp");
        let (ctx, repo, hub) = a_forum_with_one_project(d.path(), &bot).await;
        hub.registry
            .lock()
            .await
            .set_may_speak(&repo, GUEST, true)
            .expect("lets the guest in");

        let mut expected = 0;
        for (who, thread, heard) in [
            (GUEST, Some(TOPIC), true),
            (GUEST, Some(LANE_TOPIC), true),
            (GUEST, None, false),
            (GUEST, Some(99), false),
            (OPERATOR, Some(TOPIC), true),
            (OPERATOR, None, true),
        ] {
            let before = calls.load(Ordering::SeqCst);
            on_message(
                bot.clone(),
                typed(THE_FORUM, thread, a_person(who), "hello"),
                ctx.clone(),
            )
            .await
            .expect("handled");
            let spoke = calls.load(Ordering::SeqCst) > before;
            assert_eq!(
                spoke, heard,
                "person {who} in topic {thread:?}: heard={heard}, but the bot spoke={spoke}"
            );
            if !heard {
                expected += 1;
            }
        }
        assert_eq!(refused_senders(&hub).len(), expected);
    }

    #[tokio::test]
    async fn vacant_rooms_are_hidden_from_the_list_until_one_has_a_topic() {
        // A seed's book is minted whole — eight rows for eight rooms nobody has started — and his
        // fleet list must not grow eight rows of nothing. A room appears the moment a topic binds,
        // and nothing else ever has to clear a flag for it. And the word is "conversation": a
        // room and a dispatcher-named lane are both things the hub cannot call a worktree.
        let dir = tempfile::tempdir().expect("tmp");
        let file = dir.path().join("projects.json");
        let mut registry = crate::registry::Registry::load(&file);
        let org = dir.path().join("org");
        std::fs::create_dir_all(&org).expect("dir");
        let (seed, _) = registry.enrol(&org).expect("enrols");
        let rooms = registry.grant(&org, 8).expect("grants");
        registry
            .bind_topic(&crate::hub::Addr::project_itself(rooms[0].id.clone()), 2001)
            .expect("binds");
        let hub = crate::hub::Hub::new(
            Arc::new(NoSurface),
            registry,
            crate::hub::AskLedger::load(dir.path().join("asks.json")),
            crate::hub::HubAudit::new(dir.path().join("hub.audit.log")),
            vec![-1001],
            vec![OPERATOR],
            -1001,
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        hub.claim(
            crate::hub::Addr::lane_of(seed.id.clone(), hub_proto::LaneId::new("CEO-steering")),
            std::process::id(),
            "i1".into(),
            tx,
        )
        .await
        .expect("claims");

        let said = digest_of(&hub).await;
        assert!(
            said.contains(&rooms[0].title),
            "a room with a topic is missing from the list:\n{said}"
        );
        for room in &rooms[1..] {
            assert!(
                !said.contains(&room.title),
                "a vacant room reached the phone as a row of nothing:\n{said}"
            );
        }
        assert_eq!(
            said.lines().filter(|l| l.contains("<b>")).count(),
            2,
            "the list has more rows than the seed and its one live room:\n{said}"
        );
        assert!(
            said.contains("CEO-steering") && !said.contains("worktree"),
            "the list calls an address a worktree:\n{said}"
        );
    }

    #[tokio::test]
    async fn the_phone_list_names_no_person() {
        // The terminal and `--json` list a project's own people. The phone's `/projects` is read
        // in a group: a list of who may speak must not be posted where they, and everybody else
        // in the forum, can read it.
        let d = tempfile::tempdir().expect("tmp");
        let (bot, _calls) = a_telegram_that_only_counts().await;
        let (_ctx, repo, hub) = a_forum_with_one_project(d.path(), &bot).await;
        hub.registry
            .lock()
            .await
            .set_may_speak(&repo, 555_777, true)
            .expect("lets in");
        let said = digest_of(hub.as_ref()).await;
        assert!(
            !said.contains("555777") && !said.contains("555_777"),
            "a person's id reached the phone: {said}"
        );
    }

    #[test]
    fn a_channel_post_a_bot_and_an_anonymous_admin_are_nobody() {
        // Three shapes Telegram delivers with no person the bot can vouch for. The third is the
        // trap: an admin posting anonymously arrives as Telegram's shared anonymous-admin service
        // account — the same one in every group — with the group as `sender_chat`, so allowing
        // that id would allow every anonymous admin everywhere.
        assert_eq!(
            who_sent(&typed(THE_FORUM, None, a_person(OPERATOR), "hi")),
            Some(OPERATOR)
        );
        assert_eq!(
            who_sent(&typed(THE_FORUM, None, serde_json::Value::Null, "hi")),
            None,
            "a post with no sender is somebody"
        );
        assert_eq!(
            who_sent(&typed(THE_FORUM, None, a_bot(4242), "hi")),
            None,
            "a bot is a person"
        );
        let mut anonymous = message_json(
            THE_FORUM,
            None,
            a_bot(4242),
            serde_json::json!({"text": "hi"}),
        );
        anonymous["sender_chat"] =
            serde_json::json!({"id": THE_FORUM, "type": "supergroup", "title": "forum"});
        let anonymous: Message = serde_json::from_value(anonymous).expect("a message");
        assert_eq!(who_sent(&anonymous), None, "an anonymous admin is somebody");

        // The same shape with a PERSON as `from`. Telegram always pairs `sender_chat` with the
        // anonymous-admin bot, so the case above is refused on `is_bot` before `sender_chat` is
        // ever read — and the `sender_chat` guard was unpinned: removed, nothing failed. This is
        // the one assertion that reaches it. It holds because the guard is defence in depth: a
        // future Telegram that put a real user in `from` beside a `sender_chat` would otherwise
        // let the group's own voice through as that person.
        let mut fronted = message_json(
            THE_FORUM,
            None,
            a_person(OPERATOR),
            serde_json::json!({"text": "hi"}),
        );
        fronted["sender_chat"] =
            serde_json::json!({"id": THE_FORUM, "type": "supergroup", "title": "forum"});
        let fronted: Message = serde_json::from_value(fronted).expect("a message");
        assert_eq!(
            who_sent(&fronted),
            None,
            "a message posted on behalf of a chat is somebody, whoever is named in from"
        );

        assert_eq!(
            who_tapped(&tapped(THE_FORUM, None, a_person(OPERATOR), "h|y")),
            Some(OPERATOR)
        );
        assert_eq!(
            who_tapped(&tapped(THE_FORUM, None, a_bot(4242), "h|y")),
            None,
            "a bot's tap is a person's"
        );
    }

    /// Everything `tracing` wrote while the guard lived, as the journal shows it — no colour, one
    /// line per event — so a test can read the line the operator is told to read.
    struct Journal(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Journal {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("the journal is not poisoned")
                .extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn a_journal() -> (
        tracing::subscriber::DefaultGuard,
        Arc<std::sync::Mutex<Vec<u8>>>,
    ) {
        let buf = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&buf);
        let sub = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || Journal(Arc::clone(&sink)))
            .finish();
        (tracing::subscriber::set_default(sub), buf)
    }

    fn read(journal: &Arc<std::sync::Mutex<Vec<u8>>>) -> String {
        String::from_utf8(journal.lock().expect("not poisoned").clone()).expect("utf-8")
    }

    #[tokio::test]
    async fn the_journal_names_a_refused_sender_by_the_number_or_as_unknown() {
        // The journal line is the one the operator is told to copy an id out of, into
        // `herdr-tg allow <repo> <id>`. It printed `user=Some(999001)` and `user=None` — not a
        // number anyone can paste, and the exact word the bar forbids — while the audit line
        // beside it already said `sender=999001` / `sender=unknown`. The two must agree.
        let (_guard, journal) = a_journal();
        let ctx = Ctx {
            hub: None,
            gate: Arc::new(Gate::new(
                [THE_FORUM].into_iter().collect(),
                [OPERATOR].into_iter().collect(),
            )),
            username: Arc::from("herd_bot"),
        };
        refused_sender(&ctx, Some(A_STRANGER), THE_FORUM, None, "words").await;
        refused_sender(&ctx, None, THE_FORUM, None, "a tap").await;
        let said = read(&journal);
        assert!(
            said.contains(&format!("sender={A_STRANGER}")),
            "the stranger's number is not on the line as a bare number:\n{said}"
        );
        assert!(
            said.contains("sender=unknown"),
            "a sender the bot cannot vouch for is not said as `unknown`:\n{said}"
        );
        assert!(
            !said.contains("Some(") && !said.contains("None"),
            "jargon on the line the operator copies an id from:\n{said}"
        );
    }

    #[test]
    fn every_private_chat_on_the_chat_allowlist_is_announced_as_a_person_who_may_speak_anywhere() {
        // A private chat on the chat allowlist makes its person one who may speak ANYWHERE: type
        // at every agent, tap every button, run every command. That is what keeps the operator's
        // own configuration working with no new setting — and it is also what a teammate's private
        // chat, added so `/projects` works for him in private, grants without anyone having run
        // `allow`. So the startup log says it once per chat, naming the number and the narrower
        // verb, rather than folding the person into a count that is easy to read past.
        let (_guard, journal) = a_journal();
        let chats: BTreeSet<i64> = [THE_FORUM, OPERATOR, GUEST].into_iter().collect();
        let listed = BTreeSet::new();
        let gate = Gate::new(chats.clone(), [OPERATOR, GUEST].into_iter().collect());
        announce_who_may_speak(&chats, &listed, &gate);
        let said = read(&journal);
        for id in [OPERATOR, GUEST] {
            let line = said
                .lines()
                .find(|l| l.contains("EVERY agent") && l.contains(&format!("chat={id}")))
                .unwrap_or_else(|| {
                    panic!("no line says that private chat {id} may type at EVERY agent:\n{said}")
                });
            assert!(
                line.contains(&format!("herdr-tg allow <repo> {id}")),
                "the line does not say the narrower way to let this one person in: {line}"
            );
            assert!(
                line.contains("HERDR_TG_ALLOWED_CHAT_IDS"),
                "the line does not name the setting the chat came from: {line}"
            );
        }
        assert_eq!(
            said.matches("EVERY agent").count(),
            2,
            "one line per private chat, and none for the forum:\n{said}"
        );
        assert!(
            said.lines()
                .any(|l| l.contains("WARN") && l.contains("EVERY agent")),
            "two private chats is the shape where somebody was let in for a narrower reason, and \
             it is said at warn, not folded into info:\n{said}"
        );

        // The operator's own shape — one private chat, his — is not a warning: it is the
        // configuration this default exists for.
        let (_guard, journal) = a_journal();
        let chats: BTreeSet<i64> = [THE_FORUM, OPERATOR].into_iter().collect();
        let gate = Gate::new(chats.clone(), [OPERATOR].into_iter().collect());
        announce_who_may_speak(&chats, &listed, &gate);
        let said = read(&journal);
        assert_eq!(said.matches("EVERY agent").count(), 1, "{said}");
        assert!(
            !said.lines().any(|l| l.contains("WARN")),
            "one private chat is the expected shape and must not warn:\n{said}"
        );
    }

    // ── files: what he sends, as the update handler sees it ───────────────────────────────────

    /// A Telegram that ANSWERS. One canned reply per request, chosen by the request's path, and
    /// every path written down so a test can say what the bot asked for. The token is in every
    /// path — that is how the Bot API is shaped — which is exactly what the test about the token
    /// needs to be true.
    async fn a_telegram_that_answers(
        answer: impl Fn(&str) -> (u16, Vec<u8>) + Send + Sync + 'static,
    ) -> (Bot, Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let url = reqwest::Url::parse(&format!("http://{}/", listener.local_addr().expect("addr")))
            .expect("url");
        let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&asked);
        let answer = Arc::new(answer);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let seen = Arc::clone(&seen);
                let answer = Arc::clone(&answer);
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let head_end = loop {
                        let Ok(n) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                    let want: usize = head
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    while buf.len() < head_end + want {
                        let Ok(n) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let path = head
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("")
                        .to_owned();
                    seen.lock().expect("not poisoned").push(path.clone());
                    let (code, body) = answer(&path);
                    let phrase = match code {
                        200 => "OK",
                        500 => "Internal Server Error",
                        _ => "Bad Request",
                    };
                    let reply = format!(
                        "HTTP/1.1 {code} {phrase}\r\ncontent-type: application/json\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (Bot::new("1:not-a-token").set_api_url(url), asked)
    }

    /// What `getFile` answers for a photo, in the Bot API's own shape, wrapped the way the API
    /// wraps every answer.
    fn a_get_file_answer(size: u64) -> Vec<u8> {
        serde_json::json!({
            "ok": true,
            "result": {
                "file_id": "AgACAgQAAxkBAAIBQ2i9YQ3xkZQAAT0rY4Z0",
                "file_unique_id": "AQADqwEAAr8nCVN9",
                "file_size": size,
                "file_path": "photos/file_12.jpg"
            }
        })
        .to_string()
        .into_bytes()
    }

    /// The API's refusal of anything a test did not expect the bot to ask for.
    fn not_here() -> (u16, Vec<u8>) {
        (
            400,
            br#"{"ok":false,"error_code":400,"description":"Bad Request: chat not found"}"#
                .to_vec(),
        )
    }

    /// A photo with a caption, as Telegram delivers one: several sizes, the largest last, and no
    /// name or mime for any of them.
    fn a_captioned_photo(
        chat: i64,
        thread: Option<i32>,
        from: serde_json::Value,
        caption: &str,
        size: u64,
    ) -> Message {
        serde_json::from_value(message_json(
            chat,
            thread,
            from,
            serde_json::json!({
                "caption": caption,
                "photo": [
                    {"file_id": "AgACAgQAAxkBAAIBQ2i9YQ3xkZQAAT0rY4Z0small", "file_unique_id": "AQADqwEAAr8nCVN9s", "file_size": 1200, "width": 90, "height": 160},
                    {"file_id": "AgACAgQAAxkBAAIBQ2i9YQ3xkZQAAT0rY4Z0", "file_unique_id": "AQADqwEAAr8nCVN9", "file_size": size, "width": 720, "height": 1280}
                ]
            }),
        ))
        .expect("a photo Telegram would send")
    }

    #[tokio::test]
    async fn a_photo_from_the_operator_reaches_the_hub_with_its_caption() {
        // The update handler returned the moment a message had no `text`, so his photo — and
        // the caption he typed under it — reached nobody, with no line in the audit and nothing
        // on his phone. Now the largest size is fetched through the real surface into the hub's
        // own directory, the audit says so, and the caption goes with it as his words.
        let jpeg = b"\xFF\xD8\xFF\xE0 the bytes Telegram serves".repeat(300);
        let served = jpeg.clone();
        let (bot, asked) = a_telegram_that_answers(move |path| {
            let p = path.to_lowercase();
            if p.contains("/getfile") {
                (200, a_get_file_answer(served.len() as u64))
            } else if p.contains("/file/bot") && p.contains("file_12.jpg") {
                // The library puts `file_path` on the URL as ONE segment, so its slash arrives
                // percent-encoded; the route is on the file's name, not on the slash.
                (200, served.clone())
            } else {
                not_here()
            }
        })
        .await;
        let d = tempfile::tempdir().expect("tmp");
        let (ctx, _repo, hub) = a_forum_with_one_project(d.path(), &bot).await;

        on_message(
            bot.clone(),
            a_captioned_photo(
                THE_FORUM,
                Some(TOPIC),
                a_person(OPERATOR),
                "this is what the login page looks like now",
                jpeg.len() as u64,
            ),
            ctx.clone(),
        )
        .await
        .expect("handled");

        let audit = std::fs::read_to_string(hub.audit.path()).unwrap_or_default();
        let fetched = audit
            .lines()
            .find(|l| l.contains("\tfetched\t") && l.contains("kind=photo"))
            .unwrap_or_else(|| panic!("his photo was not fetched:\n{audit}"));
        let path = fetched
            .split('\t')
            .find_map(|f| f.strip_prefix("path="))
            .expect("the audit names the path");
        assert!(
            path.starts_with(&d.path().join("media").to_string_lossy().into_owned()),
            "the file is not in the hub's own media directory: {path}"
        );
        assert_eq!(std::fs::read(path).expect("the file"), jpeg);
        // The LARGEST size was asked for, by the id Telegram gave it.
        let asked = asked.lock().expect("not poisoned").clone();
        assert!(
            asked.iter().any(|p| p.to_lowercase().contains("/getfile")),
            "getFile was never called: {asked:?}"
        );
        // Nobody is connected, so his message went nowhere — and he is told so, as for words.
        assert!(audit.contains("the project was not connected"), "{audit}");
        assert!(
            asked
                .iter()
                .any(|p| p.to_lowercase().contains("sendmessage")),
            "he was not told nothing is connected: {asked:?}"
        );
    }

    #[tokio::test]
    async fn the_download_url_carrying_the_token_never_reaches_a_log_or_the_audit() {
        // The URL a file is fetched from is `file/bot<token>/<path>`. When the fetch fails the
        // error names the URL — and the client library redacts the token only when it looks like
        // a real one, which this test's does not and a misconfigured one might not. So the
        // failure must be said, in the journal and in the audit, with the token gone from both.
        let (_guard, journal) = a_journal();
        let (bot, _asked) = a_telegram_that_answers(|path| {
            let p = path.to_lowercase();
            if p.contains("/getfile") {
                (200, a_get_file_answer(3_000))
            } else if p.contains("/file/bot") {
                (500, b"<html>gateway wept</html>".to_vec())
            } else {
                not_here()
            }
        })
        .await;
        let d = tempfile::tempdir().expect("tmp");
        let (ctx, _repo, hub) = a_forum_with_one_project(d.path(), &bot).await;

        on_message(
            bot.clone(),
            a_captioned_photo(THE_FORUM, Some(TOPIC), a_person(OPERATOR), "", 3_000),
            ctx.clone(),
        )
        .await
        .expect("handled");

        let audit = std::fs::read_to_string(hub.audit.path()).unwrap_or_default();
        let said = read(&journal);
        assert!(
            audit
                .lines()
                .any(|l| l.contains("not-fetched") && l.contains("why=download-failed")),
            "the failure was not written down:\n{audit}"
        );
        assert!(
            said.contains("download"),
            "the failure was not said in the journal:\n{said}"
        );
        assert!(
            !said.contains("not-a-token"),
            "the bot token reached the journal:\n{said}"
        );
        assert!(
            !audit.contains("not-a-token"),
            "the bot token reached the audit:\n{audit}"
        );
    }

    #[tokio::test]
    async fn a_sticker_is_answered_with_what_the_bot_carries_and_a_location_with_nothing() {
        // A sticker is something he could expect the agent to see; silence reads as the bot
        // having missed it, so it gets one line saying what the bot does carry. A location is
        // not something sent TO the agent, and gets what it always got: nothing at all.
        let (bot, calls) = a_telegram_that_only_counts().await;
        let d = tempfile::tempdir().expect("tmp");
        let (ctx, _repo, _hub) = a_forum_with_one_project(d.path(), &bot).await;
        let sticker: Message = serde_json::from_value(message_json(
            THE_FORUM,
            Some(TOPIC),
            a_person(OPERATOR),
            serde_json::json!({"sticker": {
                "file_id": "CAACAgIAAxkBAAIBSmi9", "file_unique_id": "AgADEwADwDZVBw", "file_size": 20000,
                "width": 512, "height": 512, "type": "regular", "is_animated": false, "is_video": false
            }}),
        ))
        .expect("a sticker Telegram would send");
        on_message(bot.clone(), sticker, ctx.clone())
            .await
            .expect("handled");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a sticker in his topic did not earn exactly one line"
        );

        let location: Message = serde_json::from_value(message_json(
            THE_FORUM,
            Some(TOPIC),
            a_person(OPERATOR),
            serde_json::json!({"location": {"longitude": 4.9, "latitude": 52.37}}),
        ))
        .expect("a location Telegram would send");
        on_message(bot.clone(), location, ctx.clone())
            .await
            .expect("handled");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "a location was answered");
    }

    /// The message the question is asked in. [`tapped`] builds its taps under a message with this
    /// id, so the ledger record a tap is judged against has to be written down beside it.
    const THE_QUESTION: i64 = 5;
    /// The message Telegram makes when the bot says what he tapped — his receipt.
    const THE_RECEIPT: i64 = 991;

    /// What Telegram answers a `sendMessage` with: the message it made, under the id the test
    /// chose. The bot can only learn where his receipt is from this answer.
    fn a_sent_message(id: i64, text: &str) -> Vec<u8> {
        let mut made = message_json(
            THE_FORUM,
            Some(TOPIC),
            a_bot(1),
            serde_json::json!({ "text": text }),
        );
        made["message_id"] = id.into();
        serde_json::json!({"ok": true, "result": made})
            .to_string()
            .into_bytes()
    }

    /// A Telegram that answers everything one tap makes the bot do: the toast, the edit that takes
    /// the keyboard off, and the line that says what he chose.
    async fn a_telegram_that_answers_a_tap() -> (Bot, Arc<std::sync::Mutex<Vec<String>>>) {
        a_telegram_that_answers(|path| {
            let p = path.to_lowercase();
            if p.contains("/answercallbackquery") {
                (200, br#"{"ok":true,"result":true}"#.to_vec())
            } else if p.contains("/sendmessage") {
                (200, a_sent_message(THE_RECEIPT, "Sent: Overwrite it"))
            } else if p.contains("/edit") {
                (200, a_sent_message(THE_QUESTION, "Overwrite it?"))
            } else {
                not_here()
            }
        })
        .await
    }

    /// One open question on his phone, asked by a bridge that is on the socket. Hands back that
    /// bridge's end of the wire, so a test can read what the tap sends down it.
    async fn a_question_a_bridge_is_waiting_on(
        hub: &Arc<crate::hub::Hub<crate::surface::Telegram>>,
    ) -> tokio::sync::mpsc::Receiver<hub_proto::Envelope<hub_proto::HubFrame>> {
        let addr = hub
            .addr_for_topic(TOPIC)
            .await
            .expect("the project's own topic is bound");
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        // The instance on the claim and the instance on the record are the same run, or the tap is
        // refused as one aimed at a session that has since restarted.
        let _kick = hub
            .claim(addr.clone(), std::process::id(), "i1".into(), tx)
            .await
            .expect("the bridge claims the project");
        hub.ledger
            .lock()
            .await
            .record(
                THE_FORUM,
                &hub_proto::MsgId::new(THE_QUESTION.to_string()),
                crate::hub::AskRecord {
                    project: addr.project.clone(),
                    lane: None,
                    ask_id: hub_proto::AskId::new("a1"),
                    topic_id: TOPIC,
                    options: vec![hub_proto::AskOption {
                        option_id: hub_proto::OptionId::new("y"),
                        label: "Overwrite it".into(),
                    }],
                    text: "Overwrite it?".into(),
                    instance: "i1".into(),
                    pid: Some(std::process::id()),
                    at: crate::hub::now_secs(),
                    answered: None,
                    closed: None,
                },
            )
            .expect("the question is written down");
        rx
    }

    #[tokio::test]
    async fn a_tap_goes_down_under_an_id_the_hub_is_waiting_to_hear_about() {
        // A tap went down under an id minted inside the send and thrown away there, and the `true`
        // that came back meant only that the outbox took it. So a bridge that said on the wire it
        // could not act on the answer — the reply endpoint refused, the session had closed — named
        // a frame the hub held no record of, and the ack was dropped on the floor: his receipt read
        // "Sent" whatever became of the tap. The id it goes down under is now written down BEFORE
        // the frame is on the wire, which is the only thing an ack can be matched against.
        let (bot, _asked) = a_telegram_that_answers_a_tap().await;
        let d = tempfile::tempdir().expect("tmp");
        let (ctx, _repo, hub) = a_forum_with_one_project(d.path(), &bot).await;
        let mut wire = a_question_a_bridge_is_waiting_on(&hub).await;

        on_callback(
            bot.clone(),
            tapped(THE_FORUM, Some(TOPIC), a_person(OPERATOR), "h|y"),
            ctx.clone(),
        )
        .await
        .expect("handled");

        let went = wire
            .try_recv()
            .expect("his answer never reached the bridge");
        assert!(
            matches!(went.payload, hub_proto::HubFrame::Choice { .. }),
            "what went down the wire was not his answer: {:?}",
            went.payload
        );
        assert_eq!(
            hub.taps_awaiting_an_answer().await,
            1,
            "the tap went down and nothing was written down about it, so a bridge's answer for it \
             has nothing to find and his receipt can never be corrected"
        );
        assert!(
            hub.a_tap_is_awaited_under(&went.id).await,
            "the tap was written down under an id that is not the one it went down under, so the \
             bridge's ack for it can never be matched to it"
        );
    }

    #[tokio::test]
    async fn his_receipt_is_written_down_beside_the_tap_so_the_answer_can_edit_it() {
        // "Sent: <label>" is the line he is looking at after a tap, and it is the line that has to
        // change when the bridge says what became of the answer — an edit costs no send, and a
        // second message under the question would spend one of the twenty a minute per tap. Which
        // message it is can only be known here, a Telegram round trip after the frame went down, so
        // it is handed to the hub the moment Telegram says which message it made.
        let (bot, asked) = a_telegram_that_answers_a_tap().await;
        let d = tempfile::tempdir().expect("tmp");
        let (ctx, _repo, hub) = a_forum_with_one_project(d.path(), &bot).await;
        let mut wire = a_question_a_bridge_is_waiting_on(&hub).await;

        on_callback(
            bot.clone(),
            tapped(THE_FORUM, Some(TOPIC), a_person(OPERATOR), "h|y"),
            ctx.clone(),
        )
        .await
        .expect("handled");

        let went = wire
            .try_recv()
            .expect("his answer never reached the bridge");
        // He was told what he chose. That line stays — it is his receipt, and it is one line.
        let asked = asked.lock().expect("not poisoned").clone();
        assert!(
            asked
                .iter()
                .any(|p| p.to_lowercase().contains("sendmessage")),
            "he was told nothing about the tap he made: {asked:?}"
        );
        assert_eq!(
            hub.the_receipt_written_down_for(&went.id).await,
            Some(hub_proto::MsgId::new(THE_RECEIPT.to_string())),
            "the hub does not know which line his receipt is, so the answer to the tap can only be \
             said in a new message — or not at all"
        );
    }

    /// The three lines of the note beside the heartbeat, as a reader gets them.
    fn what_the_note_says(hb: &Heartbeat) -> Vec<String> {
        std::fs::read_to_string(hb.note_path())
            .expect("the note beside the heartbeat is readable")
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[tokio::test]
    async fn a_hub_whose_socket_could_not_be_opened_never_stamps_the_heartbeat_however_often_telegram_answers()
     {
        // Item 4. Opening the socket fails for ordinary reasons — the runtime directory is gone
        // after a login session ended, a stale hub still holds the path — and the hub deliberately
        // carries on so the operator keeps his `/projects` command. What it used to do as well was
        // go on stamping a green heartbeat every forty-five seconds, because the stamp asked
        // Telegram and nothing else. Every agent on the box was talking to nobody and the one
        // thing watching stayed quiet about it.
        let d = tempfile::tempdir().expect("tmp");
        let hb = Heartbeat::new(d.path().join("hub.heartbeat"));
        let health = Health::new();
        let door = Door::Shut("the agents' door could not be opened");

        for _ in 0..5 {
            let verdict = stamp_what_the_hub_can_prove(true, &door, &health, &hb).await;
            assert!(
                !verdict.earned(),
                "a Bot API round trip on its own was taken as proof that the hub is serving"
            );
            assert!(
                !hb.path().exists(),
                "the watchdog's file was stamped by a hub no agent can reach, so the alarm can \
                 never fire"
            );
        }

        // And the file beside it says which of the legs failed, because "Telegram is
        // unreachable" and "no agent can reach your phone" need different things from him.
        assert_eq!(
            what_the_note_says(&hb),
            vec![
                "not serving".to_string(),
                "the phone line answered 0 seconds ago".to_string(),
                "the agents' door could not be opened".to_string(),
                "the hub has not looked for your taps since it started".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn a_door_nobody_is_accepting_at_earns_no_heartbeat_and_the_same_door_with_its_loop_turning_earns_one()
     {
        // The subtle half of the same defect: the socket bound, the file is on disk, and the
        // accept loop is not running. The kernel completes a connection into the backlog of such a
        // listener, so a probe that only dialled would report health while every bridge on the box
        // sat in that backlog forever. What is counted is what came out the far end.
        let d = tempfile::tempdir().expect("tmp");
        let hb = Heartbeat::new(d.path().join("hub.heartbeat"));
        let health = Arc::new(Health::new());
        let path = d.path().join("hub.sock");
        let socket = crate::transport::LocalSocket::bind(&path).expect("the door opens");
        let door = Door::Open(path);

        let verdict = stamp_what_the_hub_can_prove(true, &door, &health, &hb).await;
        assert!(
            !verdict.earned(),
            "a socket that exists was taken as a hub that is answering at it"
        );
        assert!(!hb.path().exists());
        assert_eq!(
            what_the_note_says(&hb)[2],
            "nothing is answering at the agents' door"
        );

        // The same door, with the real accept loop behind it. The update stream's own note is
        // taken here by hand: in the live hub it is `WatchedUpdateStream` that takes it, and this
        // test has no dispatcher — but without it the stamp is withheld for a reason that has
        // nothing to do with the door, which is the thing being proved.
        tokio::spawn(hold_the_door(
            socket,
            a_hub(d.path(), THE_FORUM, OPERATOR),
            Arc::clone(&health),
        ));
        health.the_hub_looked_for_updates(Instant::now());
        let verdict = stamp_what_the_hub_can_prove(true, &door, &health, &hb).await;
        assert!(
            verdict.earned(),
            "a hub answering at its door and at Telegram is exactly the state the stamp is for: {}",
            verdict.why_withheld().unwrap_or_default()
        );
        assert_eq!(
            std::fs::read_to_string(hb.path()).expect("stamped"),
            "serving\n"
        );
        assert_eq!(
            what_the_note_says(&hb),
            vec![
                "serving".to_string(),
                "the phone line answered 0 seconds ago".to_string(),
                "the agents' door let a connection through 0 seconds ago".to_string(),
                "the hub looked for your taps 0 seconds ago".to_string(),
            ],
            "the note has to keep up with a hub that recovered, or it sends him to look at a door \
             that is open"
        );
    }

    /// Everything one `tracing` event said, as `name=value` pairs plus the message.
    ///
    /// Hand-rolled rather than pulled from a subscriber crate because what is under test is the
    /// exact set of fields on one line, and a formatter would put a rendering of them between the
    /// assertion and the thing asserted.
    #[derive(Default)]
    struct WhatTheJournalHeard(std::sync::Mutex<Vec<String>>);

    /// The borrow the field walk needs, kept apart from the shared record so the visit can take
    /// `&mut` while the subscriber itself is behind an `Arc`.
    struct EachField<'a>(&'a mut Vec<String>);

    impl tracing::field::Visit for EachField<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push(format!("{}={value:?}", field.name()));
        }
    }

    impl tracing::Subscriber for WhatTheJournalHeard {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::Id {
            tracing::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::Id, _: &tracing::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut said = self.0.lock().expect("nothing else holds this");
            event.record(&mut EachField(&mut said));
        }
        fn enter(&self, _: &tracing::Id) {}
        fn exit(&self, _: &tracing::Id) {}
    }

    /// The journal's account of an outage names every half, including the one this slice added.
    ///
    /// The line is written once per flip and it is what an outage is reconstructed from afterwards.
    /// A hub whose taps are all going to a second copy of itself writes a `why` that says nothing
    /// he taps is getting through and then, on the same line, two legs saying they are perfectly
    /// well — and the one sentence naming the thing to go and switch off was left out entirely.
    #[test]
    fn the_journal_line_for_an_outage_names_every_half_including_the_one_that_failed() {
        let t0 = Instant::now();
        let sustained = Duration::from_secs(91);
        let health = Health::new();
        for at in [t0, t0 + Duration::from_secs(45), t0 + sustained] {
            health.the_phone_line_answered(at);
            health.a_connection_came_through_the_door(at);
            health.the_hub_looked_for_updates(at);
            tell_the_health_what_telegram_refused(
                &health,
                &RequestError::Api(ApiError::TerminatedByOtherGetUpdates),
                at,
            );
        }
        let verdict = health.verdict(t0 + sustained);
        assert!(!verdict.earned(), "the outage this test is about");

        let heard = std::sync::Arc::new(WhatTheJournalHeard::default());
        tracing::subscriber::with_default(std::sync::Arc::clone(&heard), || {
            say_in_the_journal_which_way_the_verdict_went(&verdict, &Heartbeat::new("/nowhere/x"));
        });
        let said = heard.0.lock().expect("nothing else holds this").join(" ");

        assert!(
            said.contains("another copy of this bot is taking your taps"),
            "the journal's account of the outage left out the only sentence naming what to go and \
             switch off: {said}"
        );
        assert!(
            said.contains("the phone line answered") && said.contains("the agents' door let a"),
            "the halves that were WELL have to be on the line too, or the reader cannot tell a \
             third of an outage from all of it: {said}"
        );
    }

    /// A stream of exactly what a test hands it, standing in for teloxide's long poll.
    struct AStreamOf(std::vec::IntoIter<Result<Update, RequestError>>);

    impl Stream for AStreamOf {
        type Item = Result<Update, RequestError>;
        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.get_mut().0.next())
        }
    }

    /// One line he typed, in the shape the Bot API sends it.
    fn one_of_his_lines() -> Update {
        serde_json::from_str(
            r#"{"update_id":1,"message":{"message_id":5,"date":1675229140,"chat":{"id":-1001,"type":"supergroup","title":"the forum","is_forum":true},"from":{"id":7,"is_bot":false,"first_name":"the operator"},"text":"a line"}}"#,
        )
        .expect("the Bot API's own shape for a forum message")
    }

    /// The third leg has no witness but this stream. A wedged dispatcher stops driving it and says
    /// nothing about it anywhere else, so what is recorded here is the whole of the difference
    /// between a hub that is still asking for his taps and one that has quietly stopped.
    #[tokio::test]
    async fn the_hub_notices_every_time_its_dispatcher_goes_for_updates_and_when_one_of_his_taps_arrives()
     {
        let t0 = Instant::now();
        let health = Arc::new(Health::new());
        let mut stream = WatchedUpdateStream {
            inner: Box::pin(AStreamOf(
                vec![
                    Err(RequestError::Api(ApiError::TerminatedByOtherGetUpdates)),
                    Ok(one_of_his_lines()),
                ]
                .into_iter(),
            )),
            health: Arc::clone(&health),
        };

        assert_eq!(
            health.verdict(t0).updates().said(),
            "the hub has not looked for your taps since it started",
            "a hub nobody has driven yet claimed to be collecting his taps"
        );

        // Driven once, and what came back was a refusal. The drive still counts: a hub that is
        // asking and being turned away is a different failure from one that has stopped asking,
        // and only the run of refusals can tell the first story.
        let first = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await;
        assert!(matches!(first, Some(Err(_))));
        assert!(
            health.verdict(Instant::now()).updates().is_fresh(),
            "the dispatcher went to Telegram and the hub did not notice"
        );

        // Now the refusals as the dispatcher's error handler reports them, for long enough to be
        // an outage rather than a blip — with the hub still driving the stream throughout, which
        // is exactly why "it looked recently" cannot be the whole of this leg.
        let sustained = Duration::from_secs(91);
        for at in [t0, t0 + Duration::from_secs(45), t0 + sustained] {
            health.the_hub_looked_for_updates(at);
            tell_the_health_what_telegram_refused(
                &health,
                &RequestError::Api(ApiError::TerminatedByOtherGetUpdates),
                at,
            );
        }
        let out = health.verdict(t0 + sustained);
        assert!(
            !out.updates().is_fresh(),
            "the hub was being turned away on every call and still called this half well"
        );
        assert_eq!(
            out.updates().said(),
            "another copy of this bot is taking your taps, so none of them reach the agents here"
        );

        // And one of his lines coming out of the stream ends it at once: it is the only proof that
        // Telegram is handing updates to THIS copy of the bot.
        let second = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await;
        assert!(matches!(second, Some(Ok(_))));
        // The drive that follows it, so the assertion below is about the run of refusals and not
        // about how long ago the stream was last touched.
        health.the_hub_looked_for_updates(t0 + sustained);
        assert!(
            health.verdict(t0 + sustained).updates().is_fresh(),
            "one of his lines arrived and the hub went on reporting that none of them do"
        );
    }

    /// A stray second `herdr-tg serve` beside the unit is the likeliest way this box loses its
    /// taps, and it is the one refusal he can do something about — so it gets its own sentence,
    /// and no other refusal is allowed to borrow it.
    #[test]
    fn a_conflict_from_a_second_copy_of_this_bot_is_worded_for_him_and_no_other_refusal_is_dressed_up_as_one()
     {
        let t0 = Instant::now();
        let sustained = Duration::from_secs(91);
        let said_after = |error: RequestError| {
            let health = Health::new();
            for at in [t0, t0 + Duration::from_secs(45), t0 + sustained] {
                health.the_hub_looked_for_updates(at);
                tell_the_health_what_telegram_refused(&health, &error, at);
            }
            health.verdict(t0 + sustained).updates().said().to_owned()
        };

        assert_eq!(
            said_after(RequestError::Api(ApiError::TerminatedByOtherGetUpdates)),
            "another copy of this bot is taking your taps, so none of them reach the agents here"
        );
        assert_eq!(
            said_after(RequestError::Api(ApiError::Unknown(
                "Bad Gateway".to_owned()
            ))),
            "the hub is being turned away when it goes to collect your taps",
            "an outage nobody can name was reported as a second copy of the bot, and he would go \
             looking for one that is not there"
        );
        assert_eq!(
            said_after(RequestError::Api(ApiError::CantGetUpdates)),
            "something has pointed this bot at a web address, so your taps go there instead of \
             here",
            "a bot whose taps are being posted to a web address is the OTHER way they all die \
             while both the other halves answer, and it has its own five-second fix — telling him \
             to go and hunt for a second copy of the bot sends him to the wrong machine"
        );
    }
}

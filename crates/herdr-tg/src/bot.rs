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
use std::sync::Arc;
use std::time::Duration;

use teloxide::RequestError;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ParseMode, ThreadId};
use teloxide::utils::command::BotCommands;

use crate::config::Config;
use crate::heartbeat::{Heartbeat, HubHealth};
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
    let hub = match config.forum_chat_id {
        None => {
            tracing::warn!(
                "no forum chat is configured, so no project can be given a topic. Set \
                 HERDR_TG_FORUM_CHAT_ID to switch the hub on."
            );
            None
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
            None
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
                    // over the socket would take the operator's only channel down with it.
                    tracing::error!(error = %e, path = %sock.display(), "could not open the hub's socket");
                    None
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
                    let accept = Arc::clone(&hub);
                    tokio::spawn(async move {
                        // A per-connection error must not end the loop. Running out of file
                        // descriptors, or a peer that hangs up between the SYN and the accept, is a
                        // transient squeeze — and breaking here would leave the socket half gone for
                        // the life of the process, with the Telegram half still running and nothing
                        // anywhere saying the bridges could no longer connect. That is this
                        // system's signature failure: silence that looks exactly like health.
                        let mut pacing = AcceptPacing::new();
                        loop {
                            match socket.accept().await {
                                Ok(accepted) => {
                                    pacing.a_connection_came_through();
                                    let hub = Arc::clone(&accept);
                                    tokio::spawn(async move {
                                        if let Err(e) = hub.serve_connection(accepted).await {
                                            tracing::warn!(error = %e, "a bridge connection ended badly");
                                        }
                                    });
                                }
                                // Only the door itself reaches here. A connection whose peer the
                                // kernel would not name is dropped by the transport and never
                                // becomes an error at this level, so one unidentifiable peer can
                                // never put the hub to sleep on every other bridge's behalf.
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
                    });
                    Some(hub)
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
    // WHAT THIS STAMP PROVES: the process is alive, the network is up, the token is still good, and
    // Telegram answered. It is a real round trip, not a local clock read.
    //
    // WHAT IT DOES NOT PROVE: that updates are being DELIVERED. A dispatcher wedged behind a stuck
    // handler would keep this stamping. That gap is deliberate rather than overlooked, and claiming
    // more here would be the exact failure the watchdog exists to prevent — a healthy-looking
    // report from something that is not.
    let heartbeat = Heartbeat::new(Heartbeat::default_path());
    let hb_bot = bot.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(45));
        loop {
            tick.tick().await;
            match hb_bot.get_me().await {
                Ok(_) => {
                    if let Err(e) = heartbeat.stamp(HubHealth::Serving) {
                        tracing::error!(error = %e, path = %heartbeat.path().display(),
                            "could not stamp the heartbeat — the watchdog may raise a false alarm");
                    }
                }
                // Deliberately NOT stamped. An unreachable Bot API is exactly the state the operator
                // needs to hear about, and stamping here would hide it from the one thing watching.
                Err(e) => tracing::warn!(error = %e, "the Bot API did not answer; not stamping"),
            }
        }
    });

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(on_message))
        .branch(Update::filter_callback_query().endpoint(on_callback));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![ctx])
        .distribution_function(which_conversation)
        .enable_ctrlc_handler()
        .build()
        .dispatch()
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
                                let sent = hub
                                    .deliver(
                                        &who,
                                        hub_proto::HubFrame::Choice {
                                            msg_id: msg_id.clone(),
                                            ask_id,
                                            option_id,
                                        },
                                    )
                                    .await;
                                if sent {
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
        what_telegram_said(&ctx, chat.0, out.await).await;
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
}

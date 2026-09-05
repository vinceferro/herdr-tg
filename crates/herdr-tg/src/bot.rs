//! The Telegram front door: one bot, one forum, one topic per project.
//!
//! # The gate comes first
//!
//! Every update passes [`Gate::admit`] before anything else looks at it — before command parsing,
//! before any state is touched. That ordering is the whole security model, and it is why the check
//! is a separate type with its own tests rather than an `if` inside a handler: a handler that grows
//! a second branch is a handler that grows a way around the gate.
//!
//! A rejected chat gets **silence**, not a refusal. A refusal confirms the bot is alive and tells a
//! stranger what it is for. The rejection is logged at `warn` with the chat id, so the operator can
//! read his own id out of `journalctl` when he has mistyped it — which is the realistic failure
//! here, not an attacker.
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
#[derive(Debug, Clone)]
pub struct Gate {
    allowed: BTreeSet<i64>,
}

impl Gate {
    pub fn new(allowed: BTreeSet<i64>) -> Self {
        Self { allowed }
    }

    /// Is this chat permitted? An empty allowlist admits nobody.
    pub fn admit(&self, chat_id: i64) -> bool {
        self.allowed.contains(&chat_id)
    }

    pub fn is_deaf(&self) -> bool {
        self.allowed.is_empty()
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
    #[command(description = "which projects are enrolled, and which are connected.")]
    Projects,
    #[command(description = "show this help.")]
    Help,
}

/// Everything a handler needs, cloned per update.
#[derive(Clone)]
struct Ctx {
    /// The hub, when a forum is configured. `None` means the socket half is not running, which is a
    /// real state on a box where the forum has not been set up yet and must not be a crash.
    hub: Option<Arc<crate::hub::Hub<crate::surface::Telegram>>>,
    gate: Arc<Gate>,
}

/// Run the bot until the process is asked to stop.
pub async fn serve(config: Config) -> anyhow::Result<()> {
    let gate = Gate::new(config.allowed_chat_ids.clone());
    if gate.is_deaf() {
        tracing::warn!(
            "the chat allowlist is EMPTY — this bot will answer nobody. Set \
             HERDR_TG_ALLOWED_CHAT_IDS or `allowed_chat_ids` in herdr-tg.toml. Re-run \
             scripts/setup-token.sh to discover your chat id."
        );
    } else {
        tracing::info!(chats = ?config.allowed_chat_ids, "allowlist active");
    }

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
                forum,
            ));
            let sock = crate::hub::socket_path();
            match crate::hub::bind(&sock) {
                Err(e) => {
                    // Not fatal. The Telegram half still answers `/projects`, and refusing to boot
                    // over the socket would take the operator's only channel down with it.
                    tracing::error!(error = %e, path = %sock.display(), "could not open the hub's socket");
                    None
                }
                Ok(listener) => {
                    tracing::info!(
                        path = %sock.display(),
                        audit = %hub.audit.path().display(),
                        "the hub is listening"
                    );
                    let accept = Arc::clone(&hub);
                    tokio::spawn(async move {
                        // A per-connection error must not end the loop. Running out of file
                        // descriptors, or a peer that hangs up between the SYN and the accept, is a
                        // transient squeeze — and breaking here would leave the socket half gone for
                        // the life of the process, with the Telegram half still running and nothing
                        // anywhere saying the bridges could no longer connect. That is this
                        // system's signature failure: silence that looks exactly like health.
                        let mut backoff = std::time::Duration::from_millis(50);
                        loop {
                            match listener.accept().await {
                                Ok((stream, _)) => {
                                    backoff = std::time::Duration::from_millis(50);
                                    let hub = Arc::clone(&accept);
                                    tokio::spawn(async move {
                                        if let Err(e) = hub.serve_connection(stream).await {
                                            tracing::warn!(error = %e, "a bridge connection ended badly");
                                        }
                                    });
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        error = %e, backoff_ms = backoff.as_millis(),
                                        "the hub could not accept a connection; retrying"
                                    );
                                    tokio::time::sleep(backoff).await;
                                    backoff = (backoff * 2).min(std::time::Duration::from_secs(5));
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
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
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
            None => escape_html("The hub is not running, so I cannot pass that on."),
            Some(hub) => {
                let msg_id = q
                    .message
                    .as_ref()
                    .map(|m| hub_proto::MsgId::new(m.id().0.to_string()));
                let option_id = hub_proto::OptionId::new(*option);
                match msg_id {
                    None => escape_html("I cannot tell which question that button belongs to."),
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
                        match hub.resolve_tap(chat_id, &msg_id, &option_id).await {
                            // A chat this bot does not answer gets silence, not a refusal.
                            Err(crate::hub::TapRefusal::NotYours) => return Ok(()),
                            Err(why) => escape_html(why.say()),
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
                                    format!("Sent: {}", escape_html(&label))
                                } else {
                                    // Withdrawn, not merely reported. The tap was already written
                                    // down as answered — it has to be, or a second tap in the round
                                    // trip would deliver twice — so leaving it there burned the
                                    // question: the keyboard stayed live and could only ever answer
                                    // "that has already been answered, I have not sent anything",
                                    // which is false in the half he cares about.
                                    let what = hub.withdraw_undelivered(chat_id, &msg_id).await;
                                    escape_html(a_tap_that_reached_nobody(what))
                                }
                            }
                        }
                    }
                }
            }
        },
        _ => escape_html("I don't recognise that button."),
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
        .text(toast(&answer))
        .await;
    if let Some(msg) = q.message.as_ref() {
        // This one IS a message, so it comes out of the chat's budget — through the path that
        // cannot refuse, because he tapped a button and the answer to that is not an agent's
        // message to be rationed. It fires precisely while he is looking at a busy forum, which is
        // exactly when the budget is thin: the tap was caused by traffic.
        let chat = msg.chat().id;
        told_the_operator(&ctx, chat.0).await;
        let mut out = bot.send_message(chat, &answer).parse_mode(ParseMode::Html);
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
    let Some(text) = msg.text() else {
        return Ok(());
    };

    let Ok(cmd) = Command::parse(text, "herdr_tg") else {
        // Not a command: it is something the operator typed at a project. A message belongs to the
        // topic it was typed in and to no other — that is the whole of routing, and every other
        // rule this bridge used to have is deleted rather than tested against.
        let thread = msg.thread_id.map(|t| t.0.0);
        let body = match (&ctx.hub, thread) {
            (Some(hub), Some(thread)) => match hub.addr_for_topic(thread).await {
                Some(who) => {
                    let mid = hub_proto::MsgId::new(msg.id.0.to_string());
                    let user = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
                    // The message he swiped to reply to, if he did. A reply under one of the
                    // agent's questions is the one time he says which question — and which
                    // session — he means, and the hub decides what that is worth (`relay`). In a
                    // forum topic every message carries the topic's root as its reply, so this
                    // is usually a message nothing was written down beside, which counts as no
                    // reply at all.
                    let under = msg
                        .reply_to_message()
                        .map(|m| hub_proto::MsgId::new(m.id.0.to_string()));
                    if hub
                        .relay(&who, chat_id, user, &mid, text, under.as_ref())
                        .await
                    {
                        tracing::info!(chat_id, who = %who, bytes = text.len(), "relayed to a project");
                        // Nothing is said back. A confirmation under every line the operator types
                        // turns a conversation into a receipt printer; the agent's own answer is
                        // the acknowledgement, and it is the one he is waiting for.
                        return Ok(());
                    }
                    // Dropped, visibly, where he typed it — never queued. A message held for a
                    // worker that may never come back is a message he believes was sent.
                    // The topic and not "that project": a lane has its own topic, and the project
                    // it belongs to can be connected and busy while this worktree is not.
                    escape_html(
                        "Nothing is connected in this topic right now, so nothing was sent. It \
                         will not be delivered later.",
                    )
                }
                // Never a fall back to the project when a lane's topic is unknown: that would put
                // what he typed at one worktree into the turn of an agent working on another.
                None => escape_html(
                    "I do not know which project this topic belongs to, so I have not sent anything.",
                ),
            },
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
                let where_it_is = if project_is_connected.contains(&p.id) {
                    "connected"
                } else if lanes.contains_key(&p.id) {
                    "not connected itself — only its worktrees are"
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
                        "\n   ↳ {} — a worktree of it",
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

/// A callback answer is a toast: short, plain, and stripped of the markup the message carries.
fn toast(html: &str) -> String {
    let plain = crate::render::plain_text(html);
    plain.chars().take(180).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_allowlist_answers_nobody() {
        // The opposite convention would turn a misconfiguration into an open bot.
        let g = Gate::new(BTreeSet::new());
        assert!(g.is_deaf());
        assert!(!g.admit(1));
        assert!(!g.admit(0));
        assert!(!g.admit(-1001));
    }

    #[test]
    fn only_the_listed_chats_are_admitted() {
        let g = Gate::new([7i64, -1001].into_iter().collect());
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
    }

    /// A hub over a surface that never sends, with a forum and one other allowed chat.
    fn a_hub(dir: &std::path::Path, forum: i64, also: i64) -> Arc<crate::hub::Hub<NoSurface>> {
        Arc::new(crate::hub::Hub::new(
            Arc::new(NoSurface),
            crate::registry::Registry::load(dir.join("projects.json")),
            crate::hub::AskLedger::load(dir.join("asks.json")),
            crate::hub::HubAudit::new(dir.join("hub.audit.log")),
            vec![forum, also],
            forum,
        ))
    }

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
    fn the_command_set_offers_no_way_to_aim_at_a_pane() {
        // The whole reason this binary is safe is that there is nowhere for a keystroke to go.
        // A command that took a pane id would be the first step back.
        let d = Command::descriptions().to_string().to_lowercase();
        for gone in ["/target", "/panes", "pane", "keystroke"] {
            assert!(!d.contains(gone), "the command set mentions {gone}: {d}");
        }
    }
}

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
                            Ok((project, ask_id, option_id)) => {
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
                                        &project,
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
                                    escape_html(crate::hub::TapRefusal::NotConnected.say())
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
    let _ = bot
        .answer_callback_query(q.id.clone())
        .text(toast(&answer))
        .await;
    if let Some(msg) = q.message.as_ref() {
        let mut out = bot
            .send_message(msg.chat().id, &answer)
            .parse_mode(ParseMode::Html);
        if let Some(thread) = reply_thread {
            out = out.message_thread_id(ThreadId(MessageId(thread)));
        }
        let _ = out.await;
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
            (Some(hub), Some(thread)) => match hub.project_for_topic(thread).await {
                Some(project) => {
                    let mid = hub_proto::MsgId::new(msg.id.0.to_string());
                    let user = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
                    if hub.relay(&project, chat_id, user, &mid, text).await {
                        tracing::info!(chat_id, %project, bytes = text.len(), "relayed to a project");
                        // Nothing is said back. A confirmation under every line the operator types
                        // turns a conversation into a receipt printer; the agent's own answer is
                        // the acknowledgement, and it is the one he is waiting for.
                        return Ok(());
                    }
                    // Dropped, visibly, where he typed it — never queued. A message held for a
                    // worker that may never come back is a message he believes was sent.
                    escape_html(
                        "That project is not connected right now, so nothing was sent. It will not \
                         be delivered later.",
                    )
                }
                None => escape_html(
                    "I do not know which project this topic belongs to, so I have not sent anything.",
                ),
            },
            _ => escape_html(
                "Type inside a project's topic and I will pass it on. Here in General I do not know \
                 who you mean.",
            ),
        };
        let mut out = bot
            .send_message(msg.chat.id, &body)
            .parse_mode(ParseMode::Html);
        if let Some(t) = thread {
            out = out.message_thread_id(ThreadId(MessageId(t)));
        }
        let _ = out.await;
        return Ok(());
    };

    let body = match cmd {
        Command::Help => escape_html(&Command::descriptions().to_string()),
        Command::Projects => projects_digest(&ctx).await,
    };
    reply(&bot, msg.chat.id, &body).await;
    Ok(())
}

/// Which projects are enrolled, and which have a bridge on the socket right now.
async fn projects_digest(ctx: &Ctx) -> String {
    let Some(hub) = &ctx.hub else {
        return escape_html("The hub is not running, so I do not know about any projects.");
    };
    let registry = hub.registry.lock().await;
    let mut lines: Vec<String> = Vec::new();
    for p in registry.all() {
        let where_it_is = match p.topic_id {
            Some(_) => "has a topic",
            None => "not connected yet",
        };
        lines.push(format!(
            "<b>{}</b> — {}",
            escape_html(&p.title),
            escape_html(where_it_is)
        ));
    }
    if lines.is_empty() {
        return escape_html(
            "Nothing is enrolled yet. Enrol a project at the terminal with: herdr-tg enroll <repo>",
        );
    }
    lines.join("\n")
}

async fn reply(bot: &Bot, chat: ChatId, html: &str) {
    if let Err(e) = bot
        .send_message(chat, fit(html.to_owned()))
        .parse_mode(ParseMode::Html)
        .await
    {
        // Never silent. A rejected send used to be one error log and a drop, and that had already
        // lost 5,164 characters of a real agent's longest message.
        tracing::error!(error = %e, chat = chat.0, "the operator was NOT told");
    }
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

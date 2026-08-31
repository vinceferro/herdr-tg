//! The Telegram front door: long-poll in, herd state out.
//!
//! # The gate comes first
//!
//! Every update passes [`Gate::admit`] before anything else looks at it — before command parsing,
//! before the herd is touched. That ordering is the whole security model, and it is why the check
//! is a separate type with its own tests rather than an `if` inside the handler: a handler that
//! grows a second branch is a handler that grows a way around the gate.
//!
//! A rejected chat gets **silence**, not a refusal. A refusal confirms the bot is alive and tells a
//! stranger what it is for. The rejection is logged at `warn` with the chat id, so the operator can
//! read their own id out of `journalctl` when they have mistyped it — which is the realistic
//! failure here, not an attacker.
//!
//! # Why long-poll (D7)
//!
//! The bridge dials out and binds nothing. There is no listening port in herdr-tg at all, so the
//! tailnet box needs no ingress, no public hostname, and no webhook certificate. For one operator's
//! message volume the latency difference is invisible.
//!
//! # Slice 2 is read-only
//!
//! This slice answers questions about the herd. It reaches none of herdr's three write RPCs — the
//! ones that put real keystrokes into the operator's real terminals. This crate does not so much
//! as name them, and that is machine-checked by `herdr-client`'s `tests/no_live_write_call_site.rs`,
//! which scans every workspace member and fails on a mention, a doc comment included. The reply
//! path is slice 3, and it arrives together with the sticky-routing state and the append-only audit
//! log that make typing into a terminal accountable.

use std::collections::BTreeSet;
use std::sync::Arc;

use tokio::sync::Mutex;

use herdr_client::{HerdrClient, PaneId, SessionSnapshot};
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};
use teloxide::utils::command::BotCommands;

use crate::audit::Audit;
use crate::config::Config;
use crate::deliver::{self, Settle};
use crate::mirror::Mirror;
use crate::notify::{self, Ask, Beat, Timing};
use crate::permission::Screen;
use crate::render::{self, escape_html};
use crate::routing::{PromptRecord, Routing, Target};
use crate::voice::{self, Place, Reason};

/// Telegram's hard limit on a message body. Exceeding it is a 400 from the API, which in a
/// long-poll loop looks like "the bot went quiet" rather than like an error.
const TELEGRAM_MAX_CHARS: usize = 4096;

/// Headroom for the `<pre>` wrapper and the truncation notice.
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

/// The commands slice 2 answers.
#[derive(BotCommands, Clone, Debug, PartialEq, Eq)]
#[command(
    rename_rule = "lowercase",
    description = "herdr-tg — your herd, from your pocket."
)]
pub enum Command {
    #[command(description = "the herd: workspaces, panes, agent status.")]
    Status,
    #[command(description = "is the bridge talking to herdr, and on what protocol.")]
    Doctor,
    #[command(description = "tap a pane to aim your replies at it.")]
    Panes,
    #[command(description = "aim your replies at a pane by id, e.g. /target wA:p1.")]
    Target(String),
    #[command(description = "show this help.")]
    Help,
}

/// Run the bridge until the process is asked to stop.
/// Everything the handlers share. One Arc, so adding a handler does not mean threading six clones.
#[derive(Clone)]
struct Ctx {
    client: Arc<HerdrClient>,
    gate: Arc<Gate>,
    routing: Arc<Mutex<Routing>>,
    audit: Arc<Audit>,
    submit: herdr_client::Key,
    workspace: Option<String>,
    username: Arc<str>,
    forum: Option<ChatId>,
    gist: Option<Arc<crate::summarize::Summarizer>>,
}

/// Run the bridge until the process is asked to stop.
pub async fn serve(config: Config, client: HerdrClient) -> anyhow::Result<()> {
    // Prove the herd is reachable BEFORE announcing readiness. A bot that answers nothing, with no
    // way to tell why from a phone, is worse than a startup failure.
    let handshake = client.handshake().await?;
    tracing::info!(
        protocol = handshake.pong.protocol,
        version = %handshake.pong.version,
        "herdr reachable"
    );

    let gate = Gate::new(config.allowed_chat_ids.clone());
    if gate.is_deaf() {
        tracing::warn!(
            "the chat allowlist is EMPTY — this bot will answer nobody. Set \
             HERDR_TG_ALLOWED_CHAT_IDS or `allowed_chat_ids` in herdr-tg.toml. \
             Re-run scripts/setup-token.sh to discover your chat id."
        );
    } else {
        tracing::info!(chats = ?config.allowed_chat_ids, "allowlist active");
    }

    let bot = Bot::new(config.token());
    let me = bot.get_me().await?;
    tracing::info!(bot = %me.username(), "connected to the Bot API; long-polling");

    let routing_path = Routing::default_path();
    let audit = Arc::new(Audit::new(Audit::default_path()));

    // A state file written before routing was filed by chat is re-filed as it is read. Write it
    // back straight away: until something saves, the move lives only in memory, so every restart
    // would do it again and repeat its warnings.
    let routing = Routing::load(&routing_path, config.forum_chat_id);
    if let Err(e) = routing.save(&routing_path) {
        tracing::warn!(error = %e, "could not write the routing state back after re-filing it by chat");
    }

    let ctx = Ctx {
        client: Arc::new(client),
        gate: Arc::new(gate),
        routing: Arc::new(Mutex::new(routing)),
        audit: Arc::clone(&audit),
        submit: config.submit_key.clone(),
        workspace: config.workspace.clone(),
        username: Arc::from(me.username()),
        forum: config.forum_chat_id.map(ChatId),
        gist: crate::summarize::Summarizer::from_env().map(Arc::new),
    };
    match &ctx.gist {
        Some(g) => tracing::info!(
            model = g.model.as_deref().unwrap_or("routed by class"),
            class = g.task_class.as_deref().unwrap_or("default"),
            "a one-line gist will be added above each ask"
        ),
        None => tracing::debug!("no summarizer configured; asks go out as-is"),
    }
    match ctx.forum {
        Some(c) => tracing::info!(forum = c.0, "forum mode: one topic per pane"),
        None => {
            tracing::info!("flat mode: no forum group configured; routing uses reply-to + sticky")
        }
    }
    tracing::info!(
        routing = %routing_path.display(),
        audit = %audit.path().display(),
        submit = %config.submit_key,
        "reply path armed"
    );

    // The push loop, spawned before dispatch so an ask that is ALREADY blocked reaches the phone at
    // startup — the filtered subscription's replay, which is what recovers asks raised while the
    // laptop slept.
    {
        let ctx = ctx.clone();
        let bot = bot.clone();
        let chats: Vec<i64> = config.allowed_chat_ids.iter().copied().collect();
        tokio::spawn(async move {
            let client = Arc::clone(&ctx.client);
            let on_beat = move |beat: Beat| {
                let (bot, ctx, chats) = (bot.clone(), ctx.clone(), chats.clone());
                Box::pin(async move { push_beat(&bot, &ctx, &chats, beat).await })
                    as futures_core::future::BoxFuture<'static, ()>
            };
            notify::watch(client, Timing::default(), Arc::new(on_beat)).await
        });
    }

    // A topic for every agent pane, up front. Creating them lazily on the first ask meant a
    // session that never got stuck had no conversation to open — which is exactly the session the
    // operator wants to pick up from their phone. Idempotent: a pane that already has a topic keeps
    // it, so restarts do not multiply topics.
    if ctx.forum.is_some() {
        let ctx2 = ctx.clone();
        let bot2 = bot.clone();
        tokio::spawn(async move {
            // Once at startup, then on a slow timer. A herd changes shape while the bridge runs —
            // a workspace opens, an agent starts in a fresh pane — and doing this only at startup
            // meant a new session had no conversation until it first got stuck. Which is exactly
            // the session you would want to pick up early.
            //
            // Idempotent and cheap: one snapshot, then a map lookup per pane. A pane that already
            // has a topic keeps it, so this never multiplies topics however often it runs.
            loop {
                ensure_all_topics(&bot2, &ctx2).await;
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        });
    }

    // The mirror. Without it a topic is an alert channel that goes quiet exactly while you are
    // working — you open it on a phone and see the last alarm, not the session. This is what makes
    // walking away a non-event.
    if ctx.forum.is_some() {
        let ctx3 = ctx.clone();
        let bot3 = bot.clone();
        tokio::spawn(async move { mirror_loop(&bot3, &ctx3).await });
    }

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

/// Send one ask, with buttons that make the common actions a tap.
///
/// A dialog gets one button per option. A question gets a button that aims replies at its pane, so
/// answering a *later* plain message does not need the operator to remember a pane id — which was
/// the tedious part of the first version.
async fn push_beat(bot: &Bot, ctx: &Ctx, chats: &[i64], beat: Beat) {
    match beat {
        Beat::Asked(ask) => push_ask(bot, ctx, chats, ask).await,
        Beat::Finished {
            pane,
            workspace,
            agent,
            excerpt,
            ..
        } => {
            let _ = agent;
            let body = voice::finished(place(ctx), &workspace, &excerpt);
            say_in_topic(bot, ctx, chats, &pane, &workspace, &body, None).await;
        }
        // Deliberately terse: this is a status line in a conversation, not a notification worth a
        // buzz on its own. It exists so the topic reads continuously.
        Beat::Resumed { pane, workspace } => {
            let body = voice::resumed(place(ctx), &workspace);
            say_in_topic(bot, ctx, chats, &pane, &workspace, &body, None).await;
        }
    }
}

/// Where messages appear, which decides how much context each one must carry.
fn place(ctx: &Ctx) -> Place {
    if ctx.forum.is_some() {
        Place::Topic
    } else {
        Place::Flat
    }
}

/// What the operator is told when they try to aim replies inside the group that has topics.
const TOPIC_IS_THE_AIM: &str =
    "In this group each session has its own topic — open the one you want and reply there.";

/// Whether "send my replies here" means anything in this chat.
///
/// In the group with topics it does not: the topic is the aim, and a message typed outside one now
/// gets a picker. Offering the button there would promise an aim nothing honours.
fn sticky_offered(forum: Option<ChatId>, chat: ChatId) -> bool {
    forum != Some(chat)
}

/// Send into a pane's topic, creating it if this pane has never had one.
///
/// Returns the chat and message id of everything that actually went out, so a caller that drew
/// buttons can write down which menu each one was drawn for. A send that failed is simply absent:
/// there is no message for the operator to tap, so there is nothing to remember.
async fn say_in_topic(
    bot: &Bot,
    ctx: &Ctx,
    chats: &[i64],
    pane: &PaneId,
    workspace: &str,
    body: &str,
    keyboard: Option<InlineKeyboardMarkup>,
) -> Vec<(i64, i64)> {
    let mut sent_to = Vec::new();
    if let Some(forum) = ctx.forum {
        let Some(thread) = topic_for(bot, ctx, forum, pane, workspace).await else {
            return sent_to;
        };
        let mut req = bot
            .send_message(forum, body)
            .parse_mode(ParseMode::Html)
            .message_thread_id(thread);
        if let Some(k) = keyboard {
            req = req.reply_markup(k);
        }
        match req.await {
            Ok(sent) => {
                let mut r = ctx.routing.lock().await;
                r.record_push(forum.0, sent.id.0 as i64, pane);
                let _ = r.save(&Routing::default_path());
                sent_to.push((forum.0, sent.id.0 as i64));
            }
            Err(e) => tracing::error!(error = %e, "could not send into the topic"),
        }
        return sent_to;
    }
    for chat in chats {
        let mut req = bot
            .send_message(ChatId(*chat), body)
            .parse_mode(ParseMode::Html);
        if let Some(k) = keyboard.clone() {
            req = req.reply_markup(k);
        }
        if let Ok(sent) = req.await {
            let mut r = ctx.routing.lock().await;
            r.record_push(*chat, sent.id.0 as i64, pane);
            let _ = r.save(&Routing::default_path());
            sent_to.push((*chat, sent.id.0 as i64));
        }
    }
    sent_to
}

/// What the summariser eventually had to say about one ask — if it ever answered at all.
#[derive(Debug, Default)]
struct Summary {
    /// The one line to put above the agent's own text. `None` covers every refusal: nothing
    /// configured, gateway down, an answer that was not a one-line summary.
    line: Option<String>,
    /// Summaries have just switched themselves off for the rest of this run, and the operator has
    /// not been told yet. See [`crate::summarize::Summarizer::newly_off`].
    stopped: bool,
}

/// Put one ask on the operator's phone, and let a summary catch up with it afterwards.
///
/// # Why the summariser is not on the way to the phone
///
/// It used to be: ask the gateway for a one-line gist, wait for it, then build the message, then
/// send it. The push loop is a single task, so a gateway that accepted the connection and never
/// answered held up every pane's ask for its whole timeout — with three panes blocked at once the
/// last question reached the phone three timeouts late, which is exactly the failure `notify::watch`
/// spawns its debounce timers to avoid. An ask that arrives late is the failure this bridge exists
/// to prevent. A one-line summary is a convenience.
///
/// So the ask goes out first, with the agent's own words in it, and the summary — if one ever
/// arrives — is added above it by rewriting that same message. The operator's phone buzzes once
/// either way, and a summariser that is down, slow or wedged costs nothing but the summary.
///
/// Nothing is rewritten when there is nothing to add, so the ordinary run with no summariser
/// configured writes to Telegram exactly once, as before.
///
/// The send and the rewrite are passed in rather than reached for, so the ordering this function
/// exists to guarantee is testable without a bot token — the same reason `notify::watch` takes its
/// callback instead of a `Bot`.
///
/// Returns where the ask landed, which is what the caller writes its menu down against.
async fn push_ask_with<S, SFut, G, GFut, R, RFut>(
    place: Place,
    ask: &Ask,
    send: S,
    summarize: Option<G>,
    redraw: R,
) -> Vec<(i64, i64)>
where
    S: FnOnce(String) -> SFut,
    SFut: std::future::Future<Output = Vec<(i64, i64)>>,
    G: FnOnce(String) -> GFut + Send + 'static,
    GFut: std::future::Future<Output = Summary> + Send + 'static,
    R: FnOnce(String, Vec<(i64, i64)>) -> RFut + Send + 'static,
    RFut: std::future::Future<Output = ()> + Send + 'static,
{
    let has_options = !ask.options.is_empty();
    let landed = send(voice::asked(
        place,
        &ask.workspace,
        &ask.excerpt,
        has_options,
        None,
    ))
    .await;

    // A send that failed left no message to add a line to, and nothing for the operator to read it
    // on. Asking the summariser anyway would spend the run's one notice on a message nobody has.
    if landed.is_empty() {
        return landed;
    }

    if let Some(summarize) = summarize {
        let (workspace, excerpt) = (ask.workspace.clone(), ask.excerpt.clone());
        let where_it_landed = landed.clone();
        tokio::spawn(async move {
            let summary = summarize(excerpt.clone()).await;
            if summary.line.is_none() && !summary.stopped {
                return;
            }
            let mut body = voice::asked(
                place,
                &workspace,
                &excerpt,
                has_options,
                summary.line.as_deref(),
            );
            if summary.stopped {
                body.push_str(voice::SUMMARIES_OFF);
            }
            redraw(body, where_it_landed).await;
        });
    }
    landed
}

async fn push_ask(bot: &Bot, ctx: &Ctx, chats: &[i64], ask: Ask) {
    // The menu buttons are drawn wherever the push lands. The "send my replies here" button is
    // not: in the group with topics it would aim at something nothing reads.
    let keyboard = if ask.options.is_empty() {
        ctx.forum.is_none().then(|| {
            InlineKeyboardMarkup::new([[InlineKeyboardButton::callback(
                "📌 Send my replies here",
                format!("t|{}", ask.pane.as_str()),
            )]])
        })
    } else {
        Some(InlineKeyboardMarkup::new([ask
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                InlineKeyboardButton::callback(o.clone(), format!("c|{}|{i}", ask.pane.as_str()))
            })
            .collect::<Vec<_>>()]))
    };
    // Best-effort and off the critical path: a summariser that is slow, wedged or gone must not
    // delay the question itself. It also carries the one thing the operator cannot see from their
    // phone — that the bridge has switched its own summaries off — and whoever takes that `true`
    // owns saying it, which is why it is read here, beside the message that will carry it.
    let summarize = ctx.gist.clone().map(|g| {
        move |excerpt: String| async move {
            let line = g.one_line(&excerpt).await;
            let stopped = g.newly_off();
            Summary { line, stopped }
        }
    });
    let redraw = {
        let (bot, keyboard) = (bot.clone(), keyboard.clone());
        move |body: String, landed: Vec<(i64, i64)>| async move {
            for (chat, message_id) in landed {
                let mut req = bot
                    .edit_message_text(
                        ChatId(chat),
                        teloxide::types::MessageId(message_id as i32),
                        body.clone(),
                    )
                    .parse_mode(ParseMode::Html);
                // The same buttons go back on: an edit without them takes them off the message,
                // and the operator would be left reading an ask they can no longer tap.
                if let Some(k) = keyboard.clone() {
                    req = req.reply_markup(k);
                }
                if let Err(e) = req.await {
                    tracing::debug!(
                        error = %e,
                        "could not add the summary to an ask that is already on the phone"
                    );
                }
            }
        }
    };

    // Borrowed, not moved: the ask is still needed below to write the menu down.
    let (pane, workspace) = (&ask.pane, &ask.workspace);
    let sent_to = push_ask_with(
        place(ctx),
        &ask,
        |body: String| async move {
            say_in_topic(bot, ctx, chats, pane, workspace, &body, keyboard).await
        },
        summarize,
        redraw,
    )
    .await;

    // The buttons carry a position, which means nothing once the menu redraws. The labels they
    // showed are written down here, beside the message they are attached to, and a tap is answered
    // against this. A tap with nothing written down is refused — which is the point: an answer must
    // go to the question the operator read.
    if !ask.options.is_empty() && !sent_to.is_empty() {
        let mut r = ctx.routing.lock().await;
        for (chat, message_id) in &sent_to {
            r.record_prompt(*chat, *message_id, &ask.pane, ask.seq, &ask.options);
        }
        let _ = r.save(&Routing::default_path());
    }
}

/// A tap, once it has been matched to the menu its buttons were drawn for.
#[derive(Debug, PartialEq, Eq)]
enum Tap {
    /// The label the tapped button displayed, and the whole menu it was one of.
    Answer {
        pane: PaneId,
        label: String,
        drawn_from: Vec<String>,
        /// Where the session stood when those buttons were drawn, as the herd reported it then.
        /// `None` means it did not report it — see [`still_the_same_question`].
        seq: Option<u64>,
    },
    /// Nothing may be pressed. Carries what the operator is told.
    Refuse(Reason),
}

/// Match `c|<pane>|<position>` to the menu those buttons were drawn for.
///
/// Pure, so every way this can refuse is testable without a terminal. The button's payload carries
/// a POSITION, which is a fact about a menu that was on screen at some point in the past — a
/// Telegram button stays tappable forever. So the position is read against the menu that was
/// written down when the buttons were drawn, and the pane in the payload is only a cross-check.
/// Nothing written down means nothing to answer, and that refuses.
fn resolve_tap(recalled: Option<&PromptRecord>, chat: i64, pane: &str, position: &str) -> Tap {
    let Some(record) = recalled else {
        return Tap::Refuse(Reason::ButtonExpired);
    };
    if record.chat != chat || record.pane != pane {
        tracing::warn!(
            chat,
            pane,
            recorded_chat = record.chat,
            recorded_pane = %record.pane,
            "a tap did not match the question written down for its message — refused"
        );
        return Tap::Refuse(Reason::ButtonExpired);
    }
    let Ok(i) = position.parse::<usize>() else {
        return Tap::Refuse(Reason::ButtonExpired);
    };
    let Some(label) = record.options.get(i) else {
        return Tap::Refuse(Reason::ButtonExpired);
    };
    Tap::Answer {
        pane: PaneId::new(record.pane.clone()),
        label: label.clone(),
        drawn_from: record.options.clone(),
        seq: record.seq,
    }
}

/// Telegram shows the answer to a tap as a small plain-text toast, so the markup a chat message
/// carries has to come out or the operator reads the tags. Also short: the toast is capped.
fn toast(html: &str) -> String {
    render::plain_text(html).chars().take(190).collect()
}

/// Is the session still on the question those buttons were drawn for?
///
/// The only evidence there is: where the herd said the session's run of work stood when the buttons
/// were drawn, against where it says it stands now. A tapped button is evidence about the past and
/// nothing else — it stays tappable for as long as its message exists — and this is the one check
/// that catches a tap landing on a LATER question whose options happen to read the same.
///
/// So a herd that does not report that at all leaves the check with nothing to stand on, and the
/// answer is no. Comparing two absences as though they were the same number is how this guard used
/// to pass for every pane and every question on such a herd, silently and invisibly.
fn still_the_same_question(now: Option<u64>, drawn_at: Option<u64>) -> bool {
    match (now, drawn_at) {
        (Some(now), Some(drawn_at)) => now == drawn_at,
        _ => false,
    }
}

/// Answer one tapped button, after proving the question it was drawn for is still the one being
/// asked.
///
/// "The operator tapped it" is evidence about the past and nothing else — a Telegram button stays
/// tappable for as long as its message exists. So three separate things have to still be true, and
/// each of them refuses rather than guesses:
///
/// 1. the session is still at the same point in its work as when the buttons were drawn,
/// 2. the menu on screen still reads exactly as the buttons did, and
/// 3. the tapped label still names exactly one option on it.
///
/// Checks 2 and 3 belong to [`deliver::choose`], which looks at the pane itself; this function owns
/// the first, and owns the rule that nothing is pressed that could not be written down first.
async fn answer_dialog(
    ctx: &Ctx,
    chat_id: i64,
    pane: &PaneId,
    label: &str,
    drawn_from: &[String],
    seq: Option<u64>,
) -> String {
    let at = timestamp();
    match ctx.client.agents().await {
        Err(e) => {
            tracing::warn!(error = %e, "could not look at the herd for a tapped button");
            let _ = ctx
                .audit
                .refused(&at, chat_id, pane, "the herd was unreachable");
            return voice::nothing_sent(Reason::HerdUnreachable);
        }
        Ok(agents) => match agents.iter().find(|a| a.pane_id == *pane) {
            None => {
                let _ = ctx
                    .audit
                    .refused(&at, chat_id, pane, "that session has left the herd");
                return voice::nothing_sent(Reason::TargetGone);
            }
            Some(a) if !still_the_same_question(a.state_change_seq, seq) => {
                let stood =
                    |s: Option<u64>| s.map_or_else(|| "unknown".to_string(), |n| n.to_string());
                let _ = ctx.audit.refused(
                    &at,
                    chat_id,
                    pane,
                    &format!(
                        "could not prove that session is still on the question those buttons were \
                         drawn for (drawn at {}, now {})",
                        stood(seq),
                        stood(a.state_change_seq)
                    ),
                );
                return voice::nothing_sent(match (a.state_change_seq, seq) {
                    (Some(_), Some(_)) => Reason::PromptChanged,
                    _ => Reason::CannotTellIfMovedOn,
                });
            }
            Some(_) => {}
        },
    }

    // Written once every check has passed and immediately before the keys, so a record that stands
    // alone means the bridge died mid-write and nothing else. And this bridge does not press a key
    // it cannot write down — the typed path has always refused here; this one used to shrug and
    // press anyway.
    if let Err(e) = ctx
        .audit
        .sent(&at, chat_id, pane, &format!("[button] {label}"))
    {
        tracing::error!(error = %e, "could not write the record of what was about to be pressed; refusing");
        return voice::nothing_sent(Reason::NoAudit);
    }

    match deliver::choose(
        &*ctx.client,
        pane,
        deliver::Choice::Button { label, drawn_from },
        Settle::default(),
        |d| Box::pin(tokio::time::sleep(d)),
    )
    .await
    {
        Ok(Ok(c)) => {
            let _ = ctx.audit.outcome(&timestamp(), &c.delivery);
            // The label a settled read showed highlighted just before the confirm key — and it is
            // the agent's own words, so it is escaped before it reaches a message.
            escape_html(&c.delivery.detail)
        }
        Ok(Err(why)) => {
            let _ = ctx
                .audit
                .refused(&timestamp(), chat_id, pane, &format!("{why:?}"));
            voice::nothing_sent(refusal_reason(why))
        }
        Err(e) => {
            let _ = ctx.audit.failed(&timestamp(), pane, &e.to_string());
            tracing::error!(error = %e, "the tapped answer failed to send");
            voice::nothing_sent(Reason::HerdUnreachable)
        }
    }
}

/// A button tap: choose a dialog option, or aim replies at a pane.
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

    let reply = match data.split('|').collect::<Vec<_>>().as_slice() {
        // This also answers taps on buttons still sitting in the group's history from before the
        // topic became the aim.
        ["t", _] if !sticky_offered(ctx.forum, ChatId(chat_id)) => escape_html(TOPIC_IS_THE_AIM),
        ["t", pane] => {
            let p = PaneId::new(*pane);
            let mut r = ctx.routing.lock().await;
            r.set_sticky(chat_id, &p);
            let _ = r.save(&Routing::default_path());
            format!("📌 Replies now go to {}", escape_html(pane))
        }
        ["c", pane, position] => {
            let on_message = q.message.as_ref().map(|m| m.id().0 as i64);
            // Cloned, and the lock dropped, before anything that waits: this is the only thing
            // held while the answer goes out to a terminal.
            let recalled = match on_message {
                Some(id) => ctx.routing.lock().await.prompt_for(chat_id, id).cloned(),
                None => None,
            };
            match resolve_tap(recalled.as_ref(), chat_id, pane, position) {
                Tap::Refuse(reason) => voice::nothing_sent(reason),
                Tap::Answer {
                    pane,
                    label,
                    drawn_from,
                    seq,
                } => answer_dialog(&ctx, chat_id, &pane, &label, &drawn_from, seq).await,
            }
        }
        _ => "I don't recognise that button".to_string(),
    };

    // Answer the query first, or Telegram leaves a spinner on the button.
    let _ = bot
        .answer_callback_query(q.id.clone())
        .text(toast(&reply))
        .await;
    if let Some(msg) = q.message.as_ref() {
        // `reply` is written for the chat and already carries this bridge's markup; anything an
        // agent wrote inside it was escaped where it was put in.
        let _ = bot
            .send_message(msg.chat().id, &reply)
            .parse_mode(ParseMode::Html)
            .await;
    }
    tracing::info!(chat_id, data = %data, "handled a button");
    Ok(())
}

/// An incoming message: a command, or a reply to route into a pane.
async fn on_message(bot: Bot, msg: Message, ctx: Ctx) -> anyhow::Result<()> {
    let chat_id = msg.chat.id.0;
    if !ctx.gate.admit(chat_id) {
        // Silence, deliberately: a refusal confirms the bot exists and says what it is for.
        tracing::warn!(
            chat_id,
            "message from a chat that is NOT on the allowlist — ignored. If this is you, \
             add this id to the allowlist."
        );
        return Ok(());
    }
    let Some(text) = msg.text() else {
        return Ok(());
    };

    let Ok(cmd) = Command::parse(text, &ctx.username) else {
        let body = route_and_deliver(
            &ctx,
            chat_id,
            msg.reply_to_message().map(|m| m.id.0 as i64),
            msg.thread_id.map(|t| t.0.0),
            text,
        )
        .await;
        tracing::info!(chat_id, bytes = body.len(), "answered a reply");
        reply(&bot, msg.chat.id, &body).await;
        return Ok(());
    };

    // `/panes` answers with buttons, so it is sent separately from the text-only commands.
    if matches!(cmd, Command::Panes) {
        return panes_switcher(&bot, &ctx, msg.chat.id).await;
    }

    let body = match cmd {
        Command::Help => escape_html(&Command::descriptions().to_string()),
        Command::Status => match snapshot_for(&ctx.client, ctx.workspace.as_deref()).await {
            Ok(snap) => fit(render::herd_telegram(&snap)),
            Err(e) => escape_html(&format!("herdr unreachable: {e}")),
        },
        Command::Target(ref raw) => {
            if sticky_offered(ctx.forum, msg.chat.id) {
                set_target(&ctx.client, &ctx.routing, chat_id, raw.trim()).await
            } else {
                escape_html(&format!(
                    "{TOPIC_IS_THE_AIM} /target aims replies in the direct chat."
                ))
            }
        }
        Command::Doctor => match ctx.client.handshake().await {
            Ok(h) => escape_html(&format!(
                "herdr {} · protocol {} · socket {}",
                h.pong.version,
                h.pong.protocol,
                ctx.client.socket_path().display()
            )),
            Err(e) => escape_html(&format!("herdr unreachable: {e}")),
        },
        Command::Panes => unreachable!("handled above"),
    };
    tracing::info!(chat_id, command = ?cmd, bytes = body.len(), "answered");
    reply(&bot, msg.chat.id, &body).await;
    Ok(())
}

/// One tappable button per agent pane — the switcher PLAN.md asked for.
///
/// Typing `/target wA:p1` meant finding a cryptic id in `/status` first, which the operator
/// reasonably called tedious. Blocked panes are listed first, because those are the ones a reply is
/// usually meant for.
async fn panes_switcher(bot: &Bot, ctx: &Ctx, chat: ChatId) -> anyhow::Result<()> {
    let snap = match snapshot_for(&ctx.client, ctx.workspace.as_deref()).await {
        Ok(s) => s,
        Err(e) => {
            reply(bot, chat, &escape_html(&format!("herdr unreachable: {e}"))).await;
            return Ok(());
        }
    };
    if !sticky_offered(ctx.forum, chat) {
        reply(
            bot,
            chat,
            &format!(
                "{}\n\n<i>{}</i>",
                fit(render::herd_telegram(&snap)),
                escape_html(TOPIC_IS_THE_AIM)
            ),
        )
        .await;
        return Ok(());
    }

    let labels: std::collections::BTreeMap<_, _> = snap
        .workspaces
        .iter()
        .map(|w| (w.workspace_id.clone(), w.label.clone()))
        .collect();

    let mut panes: Vec<_> = snap
        .panes
        .iter()
        .filter(|p| p.agent.is_some() || p.display_agent.is_some())
        .collect();
    panes.sort_by_key(|p| p.agent_status != herdr_client::AgentStatus::Blocked);

    if panes.is_empty() {
        reply(bot, chat, &escape_html("No agent panes in the herd.")).await;
        return Ok(());
    }

    let rows: Vec<Vec<InlineKeyboardButton>> = panes
        .iter()
        .map(|p| {
            let mark = if p.agent_status == herdr_client::AgentStatus::Blocked {
                "🔴 "
            } else {
                ""
            };
            let label = labels
                .get(&p.workspace_id)
                .cloned()
                .unwrap_or_else(|| p.workspace_id.as_str().to_string());
            vec![InlineKeyboardButton::callback(
                format!("{mark}{label} · {}", p.pane_id.as_str()),
                format!("t|{}", p.pane_id.as_str()),
            )]
        })
        .collect();

    let _ = bot
        .send_message(chat, "Aim your replies at:")
        .reply_markup(InlineKeyboardMarkup::new(rows))
        .await;
    Ok(())
}

/// Fetch the snapshot, narrowed to this bot's workspace if it has one (D2).
async fn snapshot_for(
    client: &HerdrClient,
    workspace: Option<&str>,
) -> anyhow::Result<SessionSnapshot> {
    let mut snap = client.snapshot().await?;
    if let Some(selector) = workspace {
        crate::cmd::status::narrow_to_workspace(&mut snap, selector)?;
    }
    Ok(snap)
}

/// Send a reply, logging rather than propagating a send failure.
///
/// A failed send must not kill the loop: the operator's phone being offline, or one oversized
/// message, would otherwise take the bridge down and every later ask with it.
async fn reply(bot: &Bot, chat: ChatId, html: &str) {
    if let Err(e) = bot
        .send_message(chat, html)
        .parse_mode(ParseMode::Html)
        .await
    {
        tracing::error!(error = %e, "failed to send a reply");
    }
}

/// Clamp already-escaped HTML to Telegram's limit, announcing any truncation.
///
/// The phone view is structured HTML, not monospace, so wrapping
/// it in `<pre>` would re-introduce the fixed-width ribbon it exists to avoid. Truncation cuts at a
/// line boundary so a `<b>` or `<i>` tag is never split — a half-open tag makes Telegram reject the
/// whole message with a 400, which reads to the operator as the bot going silent.
fn fit(html: String) -> String {
    if html.chars().count() <= BODY_BUDGET {
        return html;
    }
    let mut kept = String::new();
    for line in html.lines() {
        if kept.chars().count() + line.chars().count() + 1 > BODY_BUDGET {
            break;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    format!("{kept}\n… truncated to fit Telegram's {TELEGRAM_MAX_CHARS}-character limit.")
}

/// What a reply is allowed to do to a pane.
#[derive(Debug, PartialEq, Eq)]
enum ReplyPath {
    /// A menu we resolved: answer it with keys, never with words.
    Choice(crate::permission::Prompt),
    /// Write nothing at all, and say why.
    Refuse(Reason),
    /// Ordinary output. The text path is safe.
    Text,
}

/// Decide the path from what the pane looks like. `None` means the look itself failed.
///
/// Fails CLOSED in both directions. A pane the bridge could not look at is not evidence of ordinary
/// output, and a menu it could not read is not evidence of ordinary output either — on either,
/// words go nowhere and the confirm key after them presses whatever is highlighted, which on a
/// permission prompt is a grant the operator never made.
fn reply_path(ansi: Option<&str>) -> ReplyPath {
    let Some(ansi) = ansi else {
        // Say the true thing: the bridge could not reach the pane. Telling the operator about a
        // menu it never saw would be a guess dressed up as an observation.
        return ReplyPath::Refuse(Reason::HerdUnreachable);
    };
    match crate::permission::classify(ansi) {
        Screen::Dialog(p) => ReplyPath::Choice(p),
        Screen::UnreadableControl => ReplyPath::Refuse(Reason::UnreadablePrompt),
        Screen::Prose => ReplyPath::Text,
    }
}

/// Say, in the operator's words, why nothing was confirmed.
///
/// Every arm means the same thing about the answer — no option was confirmed — and differs in what
/// the operator can do about it. It does NOT follow that the terminal was left untouched: the
/// highlight may have been moved before the bridge stopped, which is what `keys_sent` carries. An
/// operator told "nothing was typed into that terminal" about a menu whose highlight moved goes to
/// their keyboard expecting it where they left it.
fn refusal_reason(refused: deliver::ChoiceRefused) -> Reason {
    match refused {
        deliver::ChoiceRefused::NotADialog => Reason::NoLongerAsking,
        // Still asking, still unreadable. Saying it stopped asking would be false, and the advice
        // that goes with it — "have a look at what it is showing now" — is not the advice that
        // applies here.
        deliver::ChoiceRefused::Unreadable => Reason::UnreadablePrompt,
        deliver::ChoiceRefused::Unclear { options } => Reason::UnclearChoice(options),
        deliver::ChoiceRefused::Changed { .. } => Reason::PromptChanged,
        deliver::ChoiceRefused::NotConfirmed { why, keys_sent } => {
            Reason::ChoiceNotConfirmed { why, keys_sent }
        }
    }
}

/// Route a reply, deliver it, audit it, and tell the operator exactly what was observed.
///
/// The confirmation is the product's honesty surface. It names the pane, names *why* that pane was
/// chosen, and reports the highest [`deliver::Rung`] actually reached — never more. An operator who
/// is told "sent" when the text is sitting unsubmitted in a pane will wait on an agent that never
/// saw their answer, which costs them exactly the time the bot exists to save.
async fn route_and_deliver(
    ctx: &Ctx,
    chat_id: i64,
    reply_to: Option<i64>,
    thread_id: Option<i32>,
    text: &str,
) -> String {
    let (client, routing, audit, submit) = (&*ctx.client, &*ctx.routing, &*ctx.audit, &ctx.submit);
    let snap = match client.snapshot().await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "herd unreachable while routing a reply");
            return voice::nothing_sent(Reason::HerdUnreachable);
        }
    };

    let pl = place(ctx);
    let target =
        routing
            .lock()
            .await
            .resolve(chat_id, ctx.forum.map(|c| c.0), reply_to, thread_id, &snap);
    let (pane, _why) = match target {
        Target::Pane { pane, why } => (pane, why),
        // PLAN.md's failure table: never silently reroute a dead target.
        Target::Gone { .. } => return voice::nothing_sent(Reason::TargetGone),
        Target::None => return voice::nothing_sent(Reason::NoTarget),
    };

    // The workspace label, for flat mode — in a topic it is redundant and voice omits it.
    let ws_owned = snap
        .panes
        .iter()
        .find(|p| p.pane_id == pane)
        .and_then(|p| {
            snap.workspaces
                .iter()
                .find(|w| w.workspace_id == p.workspace_id)
                .map(|w| w.label.clone())
        })
        .unwrap_or_else(|| pane.as_str().to_string());
    let ws = ws_owned.as_str();

    // What is that pane showing? A menu is answered with keys, never with words: words go nowhere
    // against a menu, and the confirm key after them presses whatever is highlighted — which on a
    // permission prompt is a grant the operator did not make.
    let read = client.read_visible_ansi(&pane).await;
    if let Err(e) = &read {
        tracing::warn!(
            pane = %pane, error = %e,
            "could not see the pane before replying; refusing to answer it blind"
        );
    }
    match reply_path(read.as_ref().ok().map(|r| r.text.as_str())) {
        // The menu itself is deliberately not carried into `choose`: this read is only evidence
        // that words must not go into this pane. `choose` looks again for itself, twice, because
        // the operator's own keyboard may have moved the highlight since.
        ReplyPath::Choice(_) => {
            let at = timestamp();
            if let Err(e) = audit.sent(&at, chat_id, &pane, &format!("[choice] {text}")) {
                tracing::error!(error = %e, "could not write the audit record; refusing to send");
                return voice::nothing_sent(Reason::NoAudit);
            }
            return match deliver::choose(
                client,
                &pane,
                deliver::Choice::Reply(text),
                Settle::default(),
                |d| Box::pin(tokio::time::sleep(d)),
            )
            .await
            {
                Ok(Ok(c)) => {
                    let _ = audit.outcome(&timestamp(), &c.delivery);
                    // The label a settled read showed HIGHLIGHTED just before the confirm key went
                    // out — never the one this function matched from its own earlier read. That
                    // earlier read is the one the operator's own keyboard may have overtaken.
                    voice::choice_made(pl, ws, &c.option, c.afterwards)
                }
                // Not an error — the operator was ambiguous, or the menu moved on. Say which,
                // rather than typing into a terminal on a guess. The record above says an answer
                // was about to go out, so this one has to say that it did not.
                Ok(Err(r)) => {
                    let _ = audit.refused(&timestamp(), chat_id, &pane, &format!("{r:?}"));
                    voice::nothing_sent(refusal_reason(r))
                }
                Err(e) => {
                    let _ = audit.failed(&timestamp(), &pane, &e.to_string());
                    tracing::error!(error = %e, "the choice failed to send");
                    voice::nothing_sent(Reason::HerdUnreachable)
                }
            };
        }
        ReplyPath::Refuse(reason) => return voice::nothing_sent(reason),
        ReplyPath::Text => {}
    }

    let at = timestamp();
    // Recorded BEFORE the write: if the bridge dies mid-attempt, this still says what went in.
    if let Err(e) = audit.sent(&at, chat_id, &pane, text) {
        tracing::error!(error = %e, "could not write the audit record; refusing to send");
        return escape_html(
            "Not sent: the audit log could not be written, and this bridge does not type into a \
             terminal without a record of it.",
        );
    }

    let outcome = deliver::deliver(client, &pane, text, submit, Settle::default(), |d| {
        Box::pin(tokio::time::sleep(d))
    })
    .await;

    match outcome {
        Ok(deliver::Delivered::Watched(d)) => {
            let _ = audit.outcome(&timestamp(), &d);
            voice::reply_landed(pl, ws, &d)
        }
        // The words are in that terminal and the bridge stopped being able to look. "Nothing was
        // sent" is the one thing that must not be said about them: the operator would send the
        // message again, and an agent that already had it would act on it twice.
        Ok(deliver::Delivered::LostSight {
            pane: wrote_to,
            reached,
            detail,
        }) => {
            let _ = audit.unseen(&timestamp(), &wrote_to, reached, &detail);
            tracing::warn!(pane = %wrote_to, detail = %detail, "lost sight of a pane mid-reply");
            voice::lost_sight(pl, ws, reached)
        }
        // By construction this is the look taken BEFORE any byte was written, so the pane really
        // was left alone and the operator can be told so plainly.
        Err(e) => {
            let _ = audit.failed(&timestamp(), &pane, &e.to_string());
            tracing::error!(error = %e, "the reply was not attempted: the herd could not be read");
            voice::nothing_sent(Reason::HerdUnreachable)
        }
    }
}

/// `/target <pane>` — aim replies at a pane, acknowledged by name.
///
/// The pane must exist right now. Accepting an unknown id would let the operator arm a target that
/// silently fails at the moment they most need it to work.
async fn set_target(
    client: &herdr_client::HerdrClient,
    routing: &Mutex<Routing>,
    chat_id: i64,
    raw: &str,
) -> String {
    if raw.is_empty() {
        return escape_html("Usage: /target <pane>, e.g. /target wA:p1. See /status for pane ids.");
    }
    let snap = match client.snapshot().await {
        Ok(s) => s,
        Err(e) => return escape_html(&format!("herdr unreachable: {e}")),
    };
    let Some(found) = snap.panes.iter().find(|p| p.pane_id.as_str() == raw) else {
        return format!(
            "No pane <code>{}</code> in the herd right now. /status lists them.",
            escape_html(raw)
        );
    };
    let pane = found.pane_id.clone();
    {
        let mut r = routing.lock().await;
        r.set_sticky(chat_id, &pane);
        if let Err(e) = r.save(&Routing::default_path()) {
            // Not fatal: routing still works this session, it just will not survive a restart.
            tracing::warn!(error = %e, "could not persist routing state");
        }
    }
    format!(
        "Target set: <code>{}</code>. Replies with no reply-to go here.",
        escape_html(pane.as_str())
    )
}

/// RFC3339-ish UTC stamp for the audit log.
fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", now.as_secs(), now.subsec_millis())
}

/// The pane's topic, creating it the first time.
///
/// Named for the workspace, because that is what the operator recognises — the pane id is in the
/// message body for when it matters. Returns `None` if the topic cannot be made, which is almost
/// always the bot lacking "Manage Topics" in the group; logged with the fix rather than retried,
/// because retrying a permission error only fills the log.
async fn topic_for(
    bot: &Bot,
    ctx: &Ctx,
    forum: ChatId,
    pane: &PaneId,
    workspace: &str,
) -> Option<teloxide::types::ThreadId> {
    if let Some(t) = ctx.routing.lock().await.topic_for(forum.0, pane) {
        return Some(teloxide::types::ThreadId(teloxide::types::MessageId(t)));
    }
    let name = format!("{workspace} · {}", pane.as_str());
    match bot.create_forum_topic(forum, &name).await {
        Ok(topic) => {
            let tid = topic.thread_id;
            let mut r = ctx.routing.lock().await;
            r.bind_topic(forum.0, tid.0.0, pane);
            let _ = r.save(&Routing::default_path());
            tracing::info!(pane = %pane, topic = tid.0.0, name = %name, "created a topic");
            Some(tid)
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                "could not create a forum topic. The chat must be a SUPERGROUP with Topics \
                 enabled, and this bot must be an admin with the \"Manage Topics\" permission."
            );
            None
        }
    }
}

/// Give every agent pane a topic, and say hello in the new ones.
///
/// Run at startup and whenever the herd's shape changes. The greeting matters: a topic with no
/// messages in it is invisible in Telegram's topic list, so a silently-created topic is the same as
/// no topic at all to the person looking for it.
async fn ensure_all_topics(bot: &Bot, ctx: &Ctx) {
    let Some(forum) = ctx.forum else { return };
    let Ok(snap) = snapshot_for(&ctx.client, ctx.workspace.as_deref()).await else {
        tracing::warn!("could not read the herd; topics will be created as asks arrive");
        return;
    };
    let labels: std::collections::BTreeMap<_, _> = snap
        .workspaces
        .iter()
        .map(|w| (w.workspace_id.clone(), w.label.clone()))
        .collect();

    for p in snap
        .panes
        .iter()
        .filter(|p| p.agent.is_some() || p.display_agent.is_some())
    {
        let already = ctx
            .routing
            .lock()
            .await
            .topic_for(forum.0, &p.pane_id)
            .is_some();
        if already {
            continue;
        }
        let ws = labels
            .get(&p.workspace_id)
            .cloned()
            .unwrap_or_else(|| p.workspace_id.as_str().to_string());
        if topic_for(bot, ctx, forum, &p.pane_id, &ws).await.is_some() {
            let agent = p
                .display_agent
                .as_deref()
                .or(p.agent.as_deref())
                .unwrap_or("agent");
            let hello = voice::topic_opened(&ws, agent);
            say_in_topic(bot, ctx, &[], &p.pane_id, &ws, &hello, None).await;
        }
    }
}

/// How often each agent pane is read for new prose.
///
/// A `visible` read is cheap and cannot move the operator's screen, so the cost is one small RPC
/// per pane per tick. The interval is also the debounce: [`Mirror`] only relays a screen that
/// looked the same one tick earlier, so this is how long an agent's output must be still before it
/// counts as something it finished saying.
const MIRROR_TICK: std::time::Duration = std::time::Duration::from_secs(4);

/// Watch every agent pane and relay what it says into that pane's topic.
async fn mirror_loop(bot: &Bot, ctx: &Ctx) {
    let mut mirror = Mirror::default();
    let mut primed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    loop {
        tokio::time::sleep(MIRROR_TICK).await;

        let Ok(snap) = snapshot_for(&ctx.client, ctx.workspace.as_deref()).await else {
            continue;
        };
        let labels: std::collections::BTreeMap<_, _> = snap
            .workspaces
            .iter()
            .map(|w| (w.workspace_id.clone(), w.label.clone()))
            .collect();

        let alive: std::collections::BTreeSet<String> = snap
            .panes
            .iter()
            .filter(|p| p.agent.is_some() || p.display_agent.is_some())
            .map(|p| p.pane_id.as_str().to_string())
            .collect();
        mirror.retain(&alive);
        primed.retain(|p| alive.contains(p));

        for pane in snap
            .panes
            .iter()
            .filter(|p| p.agent.is_some() || p.display_agent.is_some())
        {
            let id = pane.pane_id.as_str().to_string();
            // `visible` only, always: a background read on a timer is exactly the case that must
            // never reach for scrollback, because that scrolls the operator's real terminal.
            let Ok(read) = ctx.client.read_visible(&pane.pane_id).await else {
                continue;
            };
            let cleaned = notify::strip_chrome_public(&read.text);

            // First sight of a pane seeds the baseline without relaying — otherwise a restart
            // dumps a screenful of history into the topic as though it had just been said.
            if !primed.contains(&id) {
                mirror.prime(&id, &cleaned);
                primed.insert(id);
                continue;
            }

            let Some(fresh) = mirror.observe(&id, &cleaned) else {
                continue;
            };
            let ws = labels
                .get(&pane.workspace_id)
                .cloned()
                .unwrap_or_else(|| pane.workspace_id.as_str().to_string());
            tracing::info!(pane = %pane.pane_id, chars = fresh.len(), "relaying");
            let body = voice::said(place(ctx), &ws, &fresh);
            say_in_topic(bot, ctx, &[], &pane.pane_id, &ws, &body, None).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(ids: &[i64]) -> Gate {
        Gate::new(ids.iter().copied().collect())
    }

    /// THE test of this module: the gate fails closed.
    #[test]
    fn an_empty_allowlist_admits_nobody() {
        let g = gate(&[]);
        assert!(g.is_deaf());
        for probe in [0, 1, -1, 878839303, i64::MAX, i64::MIN] {
            assert!(!g.admit(probe), "empty allowlist admitted {probe}");
        }
    }

    #[test]
    fn only_listed_chats_are_admitted() {
        let g = gate(&[878839303, -100200300]);
        assert!(g.admit(878839303));
        assert!(g.admit(-100200300), "group ids are negative");
        assert!(!g.admit(878839304), "an id one digit off must be refused");
        assert!(!g.admit(0));
        assert!(!g.is_deaf());
    }

    /// Pins the rule the four aiming affordances share, in the only shape this module can test
    /// without a live Bot: in the group with topics there is nothing to aim, so nothing offers to.
    #[test]
    fn the_sticky_affordance_is_never_offered_in_the_forum_chat() {
        let forum = ChatId(-100200300);
        let dm = ChatId(878);
        assert!(!sticky_offered(Some(forum), forum));
        assert!(sticky_offered(Some(forum), dm));
        assert!(sticky_offered(None, dm));
        assert!(
            sticky_offered(None, forum),
            "with no group configured for topics, that chat is an ordinary one"
        );
    }

    #[test]
    fn html_metacharacters_cannot_break_out_of_a_message() {
        let hostile = "<b>bold</b> & <script>alert(1)</script>";
        let escaped = escape_html(hostile);
        assert!(!escaped.contains('<'), "an unescaped < survived: {escaped}");
        assert!(!escaped.contains('>'), "an unescaped > survived: {escaped}");
        assert_eq!(escaped.matches("&amp;").count(), 1);
        assert!(escaped.contains("&lt;b&gt;bold&lt;/b&gt;"));
    }

    /// A pane title is whatever an agent printed. It must not be able to inject Telegram markup.
    ///
    /// The escaping lives in `render::herd_telegram`; this asserts the bot's path actually uses it.
    #[test]
    fn a_hostile_pane_title_cannot_inject_markup() {
        let hostile = "</b><a href=\"http://evil\">click</a>";
        let escaped = escape_html(hostile);
        assert!(!escaped.contains('<'), "unescaped < survived: {escaped}");
        assert!(!escaped.contains('>'), "unescaped > survived: {escaped}");
        assert!(escaped.contains("&lt;/b&gt;"));
    }

    #[test]
    fn an_oversized_message_is_truncated_and_says_so() {
        let huge = (0..600)
            .map(|i| format!("<b>ws{i}</b> · <code>w{i}:p1</code>"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = fit(huge);
        assert!(
            out.chars().count() <= TELEGRAM_MAX_CHARS,
            "message is {} chars, over the limit",
            out.chars().count()
        );
        assert!(
            out.contains("truncated"),
            "silent truncation misleads the operator"
        );
    }

    /// Truncation must cut on a LINE boundary. A message split mid-tag is a 400 from Telegram,
    /// which the operator experiences as the bot going silent — the worst failure mode here.
    #[test]
    fn truncation_never_splits_a_tag() {
        let huge = (0..600)
            .map(|i| format!("<b>workspace-number-{i}</b> · <code>w{i}:p1</code>"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = fit(huge);
        let body = out.split("\n… truncated").next().unwrap();
        assert_eq!(
            body.matches("<b>").count(),
            body.matches("</b>").count(),
            "unbalanced <b> after truncation: {body:?}"
        );
        assert_eq!(
            body.matches("<code>").count(),
            body.matches("</code>").count()
        );
    }

    #[test]
    fn a_message_that_fits_is_returned_unchanged() {
        let small = "<b>1 workspace · 1 pane</b>".to_string();
        assert_eq!(fit(small.clone()), small);
    }

    /// Multi-byte input must not panic: pane titles carry emoji and box-drawing glyphs routinely.
    #[test]
    fn truncation_is_char_safe_on_multibyte_input() {
        let wide = (0..900)
            .map(|i| format!("◑ 日本語 —— {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = fit(wide);
        assert!(out.chars().count() <= TELEGRAM_MAX_CHARS);
        assert!(out.contains("truncated"));
    }

    #[test]
    fn the_command_set_is_exactly_slice_twos_read_only_surface() {
        assert_eq!(Command::parse("/status", "b").unwrap(), Command::Status);
        assert_eq!(Command::parse("/doctor", "b").unwrap(), Command::Doctor);
        assert_eq!(Command::parse("/help", "b").unwrap(), Command::Help);
        // No command may reach a write. Slice 3 owns that, with the audit log.
        for w in ["/send", "/reply", "/type", "/keys"] {
            assert!(
                Command::parse(w, "b").is_err(),
                "{w} must not be a command in slice 2"
            );
        }
    }

    /// The blocker this closes: a pane showing a menu must never take the text path. Words go
    /// nowhere against a menu, and the confirm key after them presses whatever is highlighted —
    /// which on a permission prompt is a grant the operator never made.
    #[test]
    fn a_pane_that_looks_like_a_control_never_takes_the_text_path() {
        const REAL: &str = include_str!("../tests/fixtures/opencode-permission.ansi");
        assert!(
            matches!(reply_path(Some(REAL)), ReplyPath::Choice(_)),
            "the captured dialog must be answered with keys"
        );

        let unreadable = "\u{1b}[48;5;1mAllow once\u{1b}[0m \u{1b}[48;5;1mReject\u{1b}[0m  \u{1b}[0m⇆ select  enter confirm";
        assert_eq!(
            reply_path(Some(unreadable)),
            ReplyPath::Refuse(Reason::UnreadablePrompt),
            "a menu nobody can read must be refused, not typed at"
        );

        assert_eq!(
            reply_path(Some("agent: done, 3 files changed\n> ")),
            ReplyPath::Text,
            "only a screen positively judged ordinary output may be typed into"
        );
    }

    use crate::deliver::fake::MenuPane;

    fn drawn(chat: i64, pane: &str, options: &[&str]) -> PromptRecord {
        PromptRecord {
            chat,
            pane: pane.to_string(),
            seq: Some(198),
            options: options.iter().map(|o| o.to_string()).collect(),
        }
    }

    fn no_sleep(_: std::time::Duration) -> futures_core::future::BoxFuture<'static, ()> {
        Box::pin(async {})
    }

    /// The guard that catches a stale tap on a LATER question whose options read the same was
    /// silently inert on any herd that does not report where a session's run of work stands: both
    /// sides were flattened to zero before the comparison, so it passed for every pane and every
    /// question. Its failure mode was invisible — no log, no refusal, nothing to notice.
    ///
    /// A check with no evidence must answer no. The cost is that button taps are refused on such a
    /// herd and the operator replies with the option's name instead; the alternative is a keypress
    /// into a question nobody read.
    #[test]
    fn a_session_that_cannot_say_where_it_stands_is_never_assumed_to_have_stood_still() {
        assert!(
            still_the_same_question(Some(198), Some(198)),
            "a herd that reports the same point must still be answerable"
        );
        assert!(!still_the_same_question(Some(199), Some(198)));
        assert!(
            !still_the_same_question(None, None),
            "two absences are not evidence that nothing changed"
        );
        assert!(!still_the_same_question(None, Some(198)));
        assert!(!still_the_same_question(Some(198), None));
    }

    /// THE test of the tapped button, end to end bar the herd.
    ///
    /// The push drew `Allow once · Allow always · Reject` and the operator tapped the third. By the
    /// time the tap arrives the agent has asked again with the options in a different order, so the
    /// third one on screen is now `Allow always`. The tap must be answered by the label the button
    /// showed, which is not on that menu in that place — so nothing is pressed.
    #[tokio::test]
    async fn a_tap_answers_the_label_its_button_showed_or_nothing_at_all() {
        let record = drawn(42, "wA:p1", &["Allow once", "Allow always", "Reject"]);
        let Tap::Answer {
            pane,
            label,
            drawn_from,
            seq,
        } = resolve_tap(Some(&record), 42, "wA:p1", "2")
        else {
            panic!("a tap on a menu that was written down must be answerable");
        };
        assert_eq!(
            label, "Reject",
            "the position is read against what was drawn"
        );
        assert_eq!(drawn_from, record.options);
        assert_eq!(
            seq,
            Some(198),
            "the point the session was at is carried too"
        );

        let io = MenuPane::showing(&["Reject", "Allow once", "Allow always"]);
        let r = deliver::choose(
            &io,
            &pane,
            deliver::Choice::Button {
                label: &label,
                drawn_from: &drawn_from,
            },
            Settle::default(),
            no_sleep,
        )
        .await
        .unwrap();

        assert!(
            r.is_err(),
            "the third button confirmed the third option of a menu that reordered: confirmed={:?} \
             keys={:?}",
            io.confirmed(),
            io.keys()
        );
        assert!(io.keys().is_empty(), "not one key may reach it");
        assert_eq!(io.confirmed(), None);
    }

    /// Every button drawn before this bridge started writing the labels down, and every one whose
    /// record has since aged out. They refuse; they never guess which question they were.
    #[test]
    fn a_tap_with_nothing_written_down_sends_nothing() {
        assert_eq!(
            resolve_tap(None, 42, "wA:p1", "0"),
            Tap::Refuse(Reason::ButtonExpired)
        );
    }

    /// The cross-checks. Each of these means the tap and the record are about different things, and
    /// there is no version of that where pressing a key is right.
    #[test]
    fn a_tap_that_does_not_match_what_was_written_down_is_refused() {
        let record = drawn(42, "wA:p1", &["Allow once", "Reject"]);
        for (chat, pane, position, what) in [
            (7, "wA:p1", "0", "a tap from a chat the push never went to"),
            (42, "wB:p9", "0", "a payload naming a different session"),
            (
                42,
                "wA:p1",
                "9",
                "a position past the end of what was drawn",
            ),
            (42, "wA:p1", "one", "a position that is not a number"),
        ] {
            assert_eq!(
                resolve_tap(Some(&record), chat, pane, position),
                Tap::Refuse(Reason::ButtonExpired),
                "{what} was answered instead of refused"
            );
        }
    }

    /// A read that failed says nothing about what is on the screen, and the screen might be a
    /// permission prompt. The old code turned that silence into "not a dialog".
    #[test]
    fn a_read_that_failed_is_not_evidence_of_prose() {
        assert_eq!(
            reply_path(None),
            ReplyPath::Refuse(Reason::HerdUnreachable),
            "a pane the bridge could not look at must not be typed into"
        );
    }

    // ── the ask and the summary that may follow it ────────────────────────────────────────────

    /// One blocked agent, as the loop hands it over.
    fn an_ask() -> Ask {
        Ask {
            pane: PaneId::new("wA:p1"),
            workspace: "wA".into(),
            agent: "opencode".into(),
            seq: Some(198),
            excerpt: "Delete the production database? [y/N]".into(),
            options: Vec::new(),
        }
    }

    /// THE property of this path: an ask reaches the phone whatever the summariser is doing.
    ///
    /// A gateway that accepts the connection and never answers used to hold the push for the whole
    /// timeout — and the push loop is one task, so every other pane's ask queued behind it. An ask
    /// that arrives late is the failure this bridge exists to prevent; a summary is a convenience.
    #[tokio::test]
    async fn a_summariser_that_never_answers_does_not_hold_up_the_ask() {
        let ask = an_ask();
        let sent: Arc<Mutex<Option<(String, std::time::Duration)>>> = Arc::new(Mutex::new(None));
        let recorder = Arc::clone(&sent);
        let started = std::time::Instant::now();

        push_ask_with(
            Place::Topic,
            &ask,
            move |body: String| async move {
                *recorder.lock().await = Some((body, started.elapsed()));
                vec![(7, 9)]
            },
            Some(|_excerpt: String| async move {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                Summary::default()
            }),
            |_body, _landed| async {},
        )
        .await;

        let (body, waited) = sent.lock().await.clone().expect("the ask never went out");
        assert!(
            waited < std::time::Duration::from_millis(250),
            "the ask waited {waited:?} on the summariser before reaching the phone"
        );
        assert!(
            body.contains("Delete the production database? [y/N]"),
            "the ask went out without the agent's own words: {body}"
        );
    }

    /// A summary that does arrive is added above the ask that already went out — never instead of
    /// it, and never as a second buzz on the operator's phone.
    #[tokio::test]
    async fn a_summary_that_arrives_late_is_added_above_the_ask_it_belongs_to() {
        let ask = an_ask();
        let (tx, rx) = tokio::sync::oneshot::channel();

        push_ask_with(
            Place::Topic,
            &ask,
            |_body: String| async { vec![(7, 9)] },
            Some(|_excerpt: String| async {
                Summary {
                    line: Some("Delete the live database?".into()),
                    stopped: false,
                }
            }),
            move |body, landed| async move {
                let _ = tx.send((body, landed));
            },
        )
        .await;

        let (body, landed) = tokio::time::timeout(std::time::Duration::from_secs(5), rx)
            .await
            .expect("the summary never reached the message it belongs to")
            .expect("the redraw was dropped");
        assert_eq!(landed, vec![(7, 9)], "redrawn somewhere the ask never went");
        assert!(
            body.contains("Delete the live database?"),
            "the summary is missing: {body}"
        );
        assert!(
            body.find("Delete the live database?") < body.find("Delete the production database?"),
            "the summary must sit above the agent's own words, which must still be there: {body}"
        );
    }

    /// The bridge can switch its own summaries off mid-run to keep the operator's screen off the
    /// network. Said only in the journal, that refusal looked exactly like a gateway gone quiet —
    /// so it is said once on the phone, on the push that lost its summary.
    #[tokio::test]
    async fn the_push_that_lost_its_summary_says_summaries_have_stopped() {
        let ask = an_ask();
        let (tx, rx) = tokio::sync::oneshot::channel();

        push_ask_with(
            Place::Topic,
            &ask,
            |_body: String| async { vec![(7, 9)] },
            Some(|_excerpt: String| async {
                Summary {
                    line: None,
                    stopped: true,
                }
            }),
            move |body, _landed| async move {
                let _ = tx.send(body);
            },
        )
        .await;

        let body = tokio::time::timeout(std::time::Duration::from_secs(5), rx)
            .await
            .expect("the operator was never told summaries stopped")
            .expect("the redraw was dropped");
        assert!(
            body.contains("No more one-line summaries this run"),
            "nothing on the phone says summaries switched themselves off: {body}"
        );
        assert!(
            body.contains("Delete the production database? [y/N]"),
            "the note must sit under the agent's own words, not replace them: {body}"
        );
    }

    /// A summary that never came is not worth a second write to the message. Nothing to add means
    /// nothing is touched.
    #[tokio::test]
    async fn an_ask_with_no_summary_to_add_is_left_alone() {
        let ask = an_ask();
        let redrawn = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&redrawn);

        push_ask_with(
            Place::Topic,
            &ask,
            |_body: String| async { vec![(7, 9)] },
            Some(|_excerpt: String| async { Summary::default() }),
            move |_body, _landed| async move {
                *flag.lock().await = true;
            },
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            !*redrawn.lock().await,
            "the message was rewritten with nothing new to say"
        );
    }
}

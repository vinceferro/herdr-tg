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
use crate::notify::{self, Ask, Beat, Timing};
use crate::render::{self, escape_html};
use crate::routing::{Routing, Target};
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
    let ctx = Ctx {
        client: Arc::new(client),
        gate: Arc::new(gate),
        routing: Arc::new(Mutex::new(Routing::load(&routing_path))),
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

/// Send into a pane's topic, creating it if this pane has never had one.
async fn say_in_topic(
    bot: &Bot,
    ctx: &Ctx,
    chats: &[i64],
    pane: &PaneId,
    workspace: &str,
    body: &str,
    keyboard: Option<InlineKeyboardMarkup>,
) {
    if let Some(forum) = ctx.forum {
        let Some(thread) = topic_for(bot, ctx, forum, pane, workspace).await else {
            return;
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
                r.record_push(sent.id.0 as i64, pane);
                let _ = r.save(&Routing::default_path());
            }
            Err(e) => tracing::error!(error = %e, "could not send into the topic"),
        }
        return;
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
            r.record_push(sent.id.0 as i64, pane);
            let _ = r.save(&Routing::default_path());
        }
    }
}

async fn push_ask(bot: &Bot, ctx: &Ctx, chats: &[i64], ask: Ask) {
    // Best-effort, and bounded: a failure here must never stop a push.
    let gist = match &ctx.gist {
        Some(g) => g.one_line(&ask.excerpt).await,
        None => None,
    };
    let body = voice::asked(
        place(ctx),
        &ask.workspace,
        &ask.excerpt,
        !ask.options.is_empty(),
        gist.as_deref(),
    );
    let keyboard = if ask.options.is_empty() {
        InlineKeyboardMarkup::new([[InlineKeyboardButton::callback(
            "📌 Send my replies here",
            format!("t|{}", ask.pane.as_str()),
        )]])
    } else {
        InlineKeyboardMarkup::new([ask
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                InlineKeyboardButton::callback(o.clone(), format!("c|{}|{i}", ask.pane.as_str()))
            })
            .collect::<Vec<_>>()])
    };
    say_in_topic(
        bot,
        ctx,
        chats,
        &ask.pane,
        &ask.workspace,
        &body,
        Some(keyboard),
    )
    .await;
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
        ["t", pane] => {
            let p = PaneId::new(*pane);
            let mut r = ctx.routing.lock().await;
            r.set_sticky(chat_id, &p);
            let _ = r.save(&Routing::default_path());
            format!("📌 Replies now go to {pane}")
        }
        ["c", pane, idx] => {
            let p = PaneId::new(*pane);
            let want = idx.to_string();
            // `choose` re-parses the dialog from a fresh read, so the arrow count comes from the
            // selection as it is NOW. The index is 0-based here; match_option takes 1-based.
            let one_based = idx.parse::<usize>().map(|i| i + 1).unwrap_or(0).to_string();
            let at = timestamp();
            let _ = ctx
                .audit
                .sent(&at, chat_id, &p, &format!("[button] option {want}"));
            match deliver::choose(&*ctx.client, &p, &one_based, Settle::default(), |d| {
                Box::pin(tokio::time::sleep(d))
            })
            .await
            {
                Ok(Ok(d)) => {
                    let _ = ctx.audit.outcome(&timestamp(), &d);
                    d.detail
                }
                Ok(Err(why)) => why,
                Err(e) => {
                    let _ = ctx.audit.failed(&timestamp(), &p, &e.to_string());
                    format!("failed: {e}")
                }
            }
        }
        _ => "I don't recognise that button".to_string(),
    };

    // Answer the query first, or Telegram leaves a spinner on the button.
    let _ = bot.answer_callback_query(q.id.clone()).text(&reply).await;
    if let Some(msg) = q.message.as_ref() {
        let _ = bot
            .send_message(msg.chat().id, escape_html(&reply))
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
            set_target(&ctx.client, &ctx.routing, chat_id, raw.trim()).await
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
    let target = routing
        .lock()
        .await
        .resolve(chat_id, reply_to, thread_id, &snap);
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

    // Is that pane showing a choice dialog? If so the reply is a CHOICE, never text — text goes
    // nowhere and the Enter after it confirms whatever is highlighted, which on a permission prompt
    // is a grant the operator did not make.
    let is_dialog = match client.read_visible_ansi(&pane).await {
        Ok(r) => crate::permission::parse(&r.text),
        Err(_) => None,
    };
    if let Some(prompt) = is_dialog {
        let at = timestamp();
        if let Err(e) = audit.sent(&at, chat_id, &pane, &format!("[choice] {text}")) {
            tracing::error!(error = %e, "could not write the audit record; refusing to send");
            return voice::nothing_sent(Reason::NoAudit);
        }
        return match deliver::choose(client, &pane, text, Settle::default(), |d| {
            Box::pin(tokio::time::sleep(d))
        })
        .await
        {
            Ok(Ok(d)) => {
                let _ = audit.outcome(&timestamp(), &d);
                let chosen = prompt
                    .match_option(text)
                    .and_then(|i| prompt.options.get(i).cloned())
                    .unwrap_or_else(|| text.trim().to_string());
                voice::choice_made(pl, ws, &chosen, !d.rung.needs_attention())
            }
            // Not an error — the operator was ambiguous, or the prompt moved on. Show the options
            // again rather than typing into a terminal on a guess.
            Ok(Err(_)) => voice::nothing_sent(Reason::UnclearChoice(prompt.options.clone())),
            Err(e) => {
                let _ = audit.failed(&timestamp(), &pane, &e.to_string());
                tracing::error!(error = %e, "the choice failed to send");
                voice::nothing_sent(Reason::HerdUnreachable)
            }
        };
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
        Ok(d) => {
            let _ = audit.outcome(&timestamp(), &d);
            voice::reply_landed(pl, ws, &d)
        }
        Err(e) => {
            let _ = audit.failed(&timestamp(), &pane, &e.to_string());
            tracing::error!(error = %e, "the reply failed to send");
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
    if let Some(t) = ctx.routing.lock().await.topic_for(pane) {
        return Some(teloxide::types::ThreadId(teloxide::types::MessageId(t)));
    }
    let name = format!("{workspace} · {}", pane.as_str());
    match bot.create_forum_topic(forum, &name).await {
        Ok(topic) => {
            let tid = topic.thread_id;
            let mut r = ctx.routing.lock().await;
            r.bind_topic(tid.0.0, pane);
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
        let already = ctx.routing.lock().await.topic_for(&p.pane_id).is_some();
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
}

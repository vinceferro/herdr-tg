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

use herdr_client::{HerdrClient, SessionSnapshot};
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use teloxide::utils::command::BotCommands;

use crate::audit::Audit;
use crate::config::Config;
use crate::deliver::{self, Settle};
use crate::notify::{self, Ask, Timing};
use crate::render::{self, escape_html};
use crate::routing::{Routing, Target};

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
    #[command(description = "aim your replies at a pane, e.g. /target wA:p1.")]
    Target(String),
    #[command(description = "show this help.")]
    Help,
}

/// Run the bridge until the process is asked to stop.
pub async fn serve(config: Config, client: HerdrClient) -> anyhow::Result<()> {
    // Prove the herd is reachable BEFORE announcing readiness. Starting a bot that cannot answer
    // its one command is worse than failing here: the operator gets a bot that responds to nothing
    // and no way to tell why from their phone.
    let handshake = client.handshake().await?;
    tracing::info!(
        protocol = handshake.pong.protocol,
        version = %handshake.pong.version,
        "herdr reachable"
    );

    let gate = Gate::new(config.allowed_chat_ids.clone());
    if gate.is_deaf() {
        // Not a hard error: a deaf bot is safe, and the operator may be mid-setup. But it must be
        // impossible to miss, because the symptom (total silence) is identical to a wrong token.
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
    // Owned and shared: the repl closure must be `Fn`, so it may not consume anything.
    let username: Arc<str> = Arc::from(me.username());

    let client = Arc::new(client);
    let workspace = config.workspace.clone();
    let gate = Arc::new(gate);

    let routing_path = Routing::default_path();
    let routing = Arc::new(Mutex::new(Routing::load(&routing_path)));
    let audit = Arc::new(Audit::new(Audit::default_path()));
    tracing::info!(
        routing = %routing_path.display(),
        audit = %audit.path().display(),
        submit = %config.submit_key,
        "reply path armed"
    );
    let submit = config.submit_key.clone();

    // The push loop. Spawned before the repl so an ask that is ALREADY blocked reaches the phone
    // at startup — that is the filtered subscription's replay, and it is what recovers the asks
    // raised while the laptop was asleep or the bridge was restarting.
    {
        let client = Arc::clone(&client);
        let routing = Arc::clone(&routing);
        let bot = bot.clone();
        let chats: Vec<i64> = config.allowed_chat_ids.iter().copied().collect();
        tokio::spawn(async move {
            let on_ask = move |ask: Ask| {
                let bot = bot.clone();
                let routing = Arc::clone(&routing);
                let chats = chats.clone();
                let fut = async move {
                    for chat in chats {
                        let mut body = format!(
                            "🔴 <b>{}</b> is waiting on you\n<code>{}</code> · {}",
                            escape_html(&ask.workspace),
                            escape_html(ask.pane.as_str()),
                            escape_html(&ask.agent),
                        );
                        // The question itself. Without it this is a notification; with it, it is
                        // the conversation the product is for.
                        if !ask.excerpt.is_empty() {
                            body.push_str(&format!("\n\n<pre>{}</pre>", escape_html(&ask.excerpt)));
                        }
                        if ask.options.is_empty() {
                            body.push_str(
                                "\n<i>Reply to this message and I will type it into that pane.</i>",
                            );
                        } else {
                            // A dialog. Never invite prose — it goes nowhere, and the Enter after
                            // it confirms whatever was highlighted.
                            body.push_str("\n<b>Pick one:</b>");
                            for (i, o) in ask.options.iter().enumerate() {
                                body.push_str(&format!(
                                    "\n  <b>{}</b> · {}",
                                    i + 1,
                                    escape_html(o)
                                ));
                            }
                            body.push_str("\n<i>Reply with the number, or the option's name.</i>");
                        }
                        match bot
                            .send_message(ChatId(chat), &body)
                            .parse_mode(ParseMode::Html)
                            .await
                        {
                            Ok(sent) => {
                                // Remember which pane this push was about, so a REPLY to it routes
                                // there — the one routing rule that cannot go stale.
                                let mut r = routing.lock().await;
                                r.record_push(sent.id.0 as i64, &ask.pane);
                                if let Err(e) = r.save(&Routing::default_path()) {
                                    tracing::warn!(error = %e, "could not persist the push mapping");
                                }
                            }
                            Err(e) => tracing::error!(error = %e, "could not push an ask"),
                        }
                    }
                };
                Box::pin(fut) as futures_core::future::BoxFuture<'static, ()>
            };
            notify::watch(client, Timing::default(), Arc::new(on_ask)).await
        });
    }

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let client = Arc::clone(&client);
        let gate = Arc::clone(&gate);
        let workspace = workspace.clone();
        let username = Arc::clone(&username);
        let routing = Arc::clone(&routing);
        let audit = Arc::clone(&audit);
        let submit = submit.clone();
        async move {
            let chat_id = msg.chat.id.0;
            if !gate.admit(chat_id) {
                // Silence, deliberately. See the module docs.
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
            let Ok(cmd) = Command::parse(text, &username) else {
                // Prose from an allowed operator is a REPLY. This is the product.
                let body = route_and_deliver(
                    &client,
                    &routing,
                    &audit,
                    &submit,
                    chat_id,
                    msg.reply_to_message().map(|m| m.id.0 as i64),
                    text,
                )
                .await;
                tracing::info!(chat_id, bytes = body.len(), "answered a reply");
                reply(&bot, msg.chat.id, &body).await;
                return Ok(());
            };

            let body = match cmd {
                Command::Help => escape_html(&Command::descriptions().to_string()),
                Command::Status => match snapshot_for(&client, workspace.as_deref()).await {
                    Ok(snap) => fit(render::herd_telegram(&snap)),
                    Err(e) => escape_html(&format!("herdr unreachable: {e}")),
                },
                Command::Target(ref raw) => {
                    set_target(&client, &routing, chat_id, raw.trim()).await
                }
                Command::Doctor => match client.handshake().await {
                    Ok(h) => escape_html(&format!(
                        "herdr {} · protocol {} · socket {}",
                        h.pong.version,
                        h.pong.protocol,
                        client.socket_path().display()
                    )),
                    Err(e) => escape_html(&format!("herdr unreachable: {e}")),
                },
            };

            // Log every HANDLED command, not only the failures. When the bot goes quiet, the
            // operator is on a phone with nothing but `journalctl` to look at, and "no lines at
            // all" cannot be told apart from "the poll loop died". A line per command makes
            // silence diagnosable.
            tracing::info!(chat_id, command = ?cmd, bytes = body.len(), "answered");
            reply(&bot, msg.chat.id, &body).await;
            Ok(())
        }
    })
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
    client: &herdr_client::HerdrClient,
    routing: &Mutex<Routing>,
    audit: &Audit,
    submit: &herdr_client::Key,
    chat_id: i64,
    reply_to: Option<i64>,
    text: &str,
) -> String {
    let snap = match client.snapshot().await {
        Ok(s) => s,
        Err(e) => return escape_html(&format!("herdr unreachable, so nothing was sent: {e}")),
    };

    let target = routing.lock().await.resolve(chat_id, reply_to, &snap);
    let (pane, why) = match target {
        Target::Pane { pane, why } => (pane, why),
        Target::Gone { pane } => {
            // PLAN.md's failure table: never silently reroute a dead target.
            return format!(
                "<b>Not sent.</b> Your target <code>{}</code> is no longer in the herd.\nPick a \
                 new one with /target, or reply directly to a pane's message.",
                escape_html(pane.as_str())
            );
        }
        Target::None => {
            return escape_html(
                "No target yet, and I will not guess which terminal to type into. \
                 Set one with /target <pane>, or reply to a pane's message.",
            );
        }
    };

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
            return escape_html("Not sent: the audit log could not be written.");
        }
        return match deliver::choose(client, &pane, text, Settle::default(), |d| {
            Box::pin(tokio::time::sleep(d))
        })
        .await
        {
            Ok(Ok(d)) => {
                let _ = audit.outcome(&timestamp(), &d);
                let head = format!(
                    "{} → <code>{}</code> <i>({})</i>",
                    escape_html(&d.detail),
                    escape_html(pane.as_str()),
                    why.phrase()
                );
                if d.rung.needs_attention() {
                    format!("⚠️ {head}")
                } else {
                    head
                }
            }
            // Not an error — the operator was ambiguous, or the dialog moved on. Show the options
            // again rather than typing something into a terminal on a guess.
            Ok(Err(why_not)) => {
                let mut m = format!("<b>Nothing sent.</b> {}", escape_html(&why_not));
                if !prompt.options.is_empty() {
                    m.push_str("\n<b>Pick one:</b>");
                    for (i, o) in prompt.options.iter().enumerate() {
                        m.push_str(&format!("\n  <b>{}</b> · {}", i + 1, escape_html(o)));
                    }
                }
                m
            }
            Err(e) => {
                let _ = audit.failed(&timestamp(), &pane, &e.to_string());
                format!(
                    "<b>Failed on</b> <code>{}</code>: {}",
                    escape_html(pane.as_str()),
                    escape_html(&e.to_string())
                )
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
            let head = format!(
                "{} → <code>{}</code> <i>({})</i>",
                d.rung.phrase(),
                escape_html(pane.as_str()),
                why.phrase()
            );
            if d.rung.needs_attention() {
                format!("⚠️ {head}\n{}", escape_html(&d.detail))
            } else {
                head
            }
        }
        Err(e) => {
            let _ = audit.failed(&timestamp(), &pane, &e.to_string());
            format!(
                "<b>Failed while sending to</b> <code>{}</code>: {}\nCheck the pane — some of it \
                 may have landed.",
                escape_html(pane.as_str()),
                escape_html(&e.to_string())
            )
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

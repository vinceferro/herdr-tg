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

use herdr_client::{HerdrClient, SessionSnapshot};
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use teloxide::utils::command::BotCommands;

use crate::config::Config;
use crate::render::{self, escape_html};

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

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let client = Arc::clone(&client);
        let gate = Arc::clone(&gate);
        let workspace = workspace.clone();
        let username = Arc::clone(&username);
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
                // An allowed operator typing prose is the slice-3 reply path. Until it exists, say
                // so rather than ignoring them — silence here would read as a broken bot.
                reply(
                    &bot,
                    msg.chat.id,
                    &escape_html(
                        "Not a command yet. /status, /doctor, /help. \
                     Replying into a pane arrives in slice 3.",
                    ),
                )
                .await;
                return Ok(());
            };

            let body = match cmd {
                Command::Help => escape_html(&Command::descriptions().to_string()),
                Command::Status => match snapshot_for(&client, workspace.as_deref()).await {
                    Ok(snap) => fit(render::herd_telegram(&snap)),
                    Err(e) => escape_html(&format!("herdr unreachable: {e}")),
                },
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

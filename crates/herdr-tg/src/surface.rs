//! Telegram, as the hub actually speaks to it.
//!
//! Everything in here is the part [`crate::hub`] deliberately does not know: forum topics, HTML
//! escaping, inline keyboards, and the one error string that means a topic has been deleted. The
//! hub's decisions are tested against a fake implementation of [`crate::hub::Surface`]; this file
//! is what those decisions run against in production, and it holds no decisions of its own.
//!
//! # Two things here are load-bearing and easy to lose
//!
//! **Every agent-authored string passes through `escape_html`.** One function, every path. An
//! agent's message is arbitrary text and the parse mode is HTML, so an unescaped `<` turns a
//! sentence into a malformed entity and Telegram rejects the whole message — which reads to the
//! operator as an agent that went quiet.
//!
//! **A deleted topic is a rebinding, not a retry.** Telegram emits no service message when a forum
//! topic is deleted and offers no way to list topics, so the only evidence is `message thread not
//! found` coming back from a send. Matching that string is unlovely and it is the only signal there
//! is; treating it as a transient would make a project's messages vanish quietly and for good.

use hub_proto::{AskOption, MsgId};
use teloxide::prelude::*;
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ParseMode, Rgb, ThreadId,
};

use crate::hub::{SendOutcome, Surface};
use crate::render::escape_html;

/// The six colours Telegram permits for a topic icon, in the order the design's `hash % 6` indexes.
///
/// Not arbitrary and not ours to choose: anything outside this set is refused by the API. Six
/// projects colour-coded for free, stable forever, and no decision for the operator to make.
const TOPIC_COLOURS: [u32; 6] = [0x6FB9F0, 0xFFD67E, 0xCB86DB, 0x8EEE98, 0xFF93B2, 0xFB6F5F];

/// What marks a `callback_data` as belonging to the hub rather than to the older pane path.
pub const CALLBACK_PREFIX: &str = "h";

/// The real thing.
pub struct Telegram {
    bot: Bot,
    forum: ChatId,
}

impl Telegram {
    pub fn new(bot: Bot, forum: ChatId) -> Self {
        Self { bot, forum }
    }
}

/// Does this error mean the topic is gone, as opposed to anything else?
///
/// Deliberately narrow. A broad match here would turn an ordinary network failure into a topic
/// rebinding, which creates a second topic and splits a project's history in half.
fn is_topic_gone(err: &teloxide::RequestError) -> bool {
    let said = err.to_string().to_lowercase();
    said.contains("message thread not found") || said.contains("topic_deleted")
}

impl Surface for Telegram {
    async fn create_topic(&self, title: &str, icon_color: u8) -> anyhow::Result<i32> {
        let colour = TOPIC_COLOURS[(icon_color as usize) % TOPIC_COLOURS.len()];
        let topic = self
            .bot
            .create_forum_topic(self.forum, title)
            .icon_color(Rgb {
                r: ((colour >> 16) & 0xFF) as u8,
                g: ((colour >> 8) & 0xFF) as u8,
                b: (colour & 0xFF) as u8,
            })
            .await?;
        Ok(topic.thread_id.0.0)
    }

    async fn send(&self, topic_id: i32, text: &str, buttons: &[AskOption]) -> SendOutcome {
        let mut req = self
            .bot
            .send_message(self.forum, escape_html(text))
            .parse_mode(ParseMode::Html)
            .message_thread_id(ThreadId(MessageId(topic_id)));

        if !buttons.is_empty() {
            // `callback_data` is 64 bytes and carries an OPAQUE id, never a decision. What the id
            // means is written down in the hub's ledger beside the message; resolving a tap by the
            // button's position is how one reading "Reject" once confirmed "Allow always".
            // The label goes on RAW. A button's text is plain text, not HTML — escaping it makes
            // the operator read `&amp;` and `&lt;` on the very thing he is about to press. The
            // message BODY is still escaped, above, which is where escaping belongs.
            //
            // One option per row. A row of four is four unreadable slivers on a phone, and a button
            // whose text you cannot read is worse than no button.
            let rows: Vec<Vec<InlineKeyboardButton>> = buttons
                .iter()
                .map(|o| {
                    // `h|` marks it as the hub's. This chat already carries buttons from the older
                    // pane path, whose data starts `t|` or `c|`, and a tap must never be resolved
                    // by the wrong handler just because two schemes happened to overlap.
                    vec![InlineKeyboardButton::callback(
                        o.label.clone(),
                        format!("{CALLBACK_PREFIX}|{}", o.option_id.as_str()),
                    )]
                })
                .collect();
            req = req.reply_markup(InlineKeyboardMarkup::new(rows));
        }

        match req.await {
            Ok(msg) => SendOutcome::Sent(MsgId::new(msg.id.0.to_string())),
            Err(e) if is_topic_gone(&e) => SendOutcome::TopicGone,
            Err(teloxide::RequestError::Network(e)) => {
                // The send went out and could not be checked. Telegram has no idempotency key, so
                // this may or may not have landed and there is no way to ask. Reporting it as a
                // failure would invite a retry, and a retried question with buttons is two live
                // menus for one question, both tappable forever.
                tracing::warn!(error = %e, "a send could not be confirmed either way");
                SendOutcome::Unseen
            }
            Err(e) => SendOutcome::Refused(e.to_string()),
        }
    }

    async fn retire_buttons(
        &self,
        topic_id: i32,
        msg_id: &MsgId,
        original: &str,
        note: &str,
    ) -> anyhow::Result<()> {
        let Ok(raw) = msg_id.as_str().parse::<i32>() else {
            anyhow::bail!("that message id is not one this bot wrote");
        };
        // ONE call that rewrites the body and drops the keyboard together.
        //
        // An earlier version only cleared the markup and logged the note. The operator saw a menu
        // vanish with no explanation, which reads as the bot having lost the question — the exact
        // opposite of the reassurance this exists to give. `edit_message_text` sent without a
        // reply_markup both replaces the text and removes the buttons, so there is no window where
        // one has happened and the other has not.
        let body = format!(
            "{}\n\n\u{2713} {}",
            escape_html(original),
            escape_html(note)
        );
        self.bot
            .edit_message_text(self.forum, MessageId(raw), body)
            .parse_mode(ParseMode::Html)
            .await?;
        tracing::info!(
            topic = topic_id,
            message = raw,
            note,
            "a question stopped being asked"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_colour_the_hub_can_pick_is_one_telegram_accepts() {
        // The hub derives `hash % 6`. If these two ever disagree, topic creation fails for some
        // projects and not others — which reads as an intermittent bug rather than a constant.
        for n in 0u8..=255 {
            let picked = TOPIC_COLOURS[(n as usize) % TOPIC_COLOURS.len()];
            assert!(
                TOPIC_COLOURS.contains(&picked),
                "colour {picked:#x} is not one of the six"
            );
        }
    }

    #[test]
    fn only_a_deleted_topic_looks_like_a_deleted_topic() {
        // A broad match would turn an ordinary network failure into a rebinding, creating a second
        // topic and splitting a project's history in half.
        let gone = teloxide::RequestError::Api(teloxide::ApiError::Unknown(
            "Bad Request: message thread not found".to_owned(),
        ));
        assert!(is_topic_gone(&gone));

        for other in [
            "Bad Request: message text is empty",
            "Too Many Requests: retry after 5",
            "Forbidden: bot was blocked by the user",
            "Bad Request: chat not found",
        ] {
            let e = teloxide::RequestError::Api(teloxide::ApiError::Unknown(other.to_owned()));
            assert!(
                !is_topic_gone(&e),
                "{other} was mistaken for a deleted topic"
            );
        }
    }
}

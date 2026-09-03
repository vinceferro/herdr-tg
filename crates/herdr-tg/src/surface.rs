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

use crate::hub::{Refused, SendOutcome, Surface};
use crate::render::escape_html;

/// The six colours Telegram permits for a topic icon, in the order the design's `hash % 6` indexes.
///
/// Not arbitrary and not ours to choose: anything outside this set is refused by the API. Six
/// projects colour-coded for free, stable forever, and no decision for the operator to make.
const TOPIC_COLOURS: [u32; 6] = [0x6FB9F0, 0xFFD67E, 0xCB86DB, 0x8EEE98, 0xFF93B2, 0xFB6F5F];

/// What marks a `callback_data` as belonging to the hub rather than to the older pane path.
pub const CALLBACK_PREFIX: &str = "h";

/// Turn what Telegram answered a send with into the one word the hub acts on.
///
/// One function, both send paths — a topic's and General's — because a classification that exists
/// twice is a classification that will disagree with itself.
fn what_became_of_it(
    answered: Result<teloxide::types::Message, teloxide::RequestError>,
) -> SendOutcome {
    match answered {
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
        // A flood wait is not a refusal, whatever it looks like from here: it is Telegram saying
        // "not yet", and it mends itself inside a minute. It used to fall through to the arm below
        // and reach the agent as "his messaging app would not take it" — a permanent-sounding
        // sentence about something temporary — while the seconds it came with were destroyed.
        // `TooFast` is the word for that, and it already carries a duration the budget can act on.
        Err(e) => match flood_wait(&e) {
            Some(wait) => {
                tracing::warn!(
                    seconds = wait.as_secs(),
                    "Telegram is refusing sends to this chat for flooding"
                );
                SendOutcome::TooFast(wait)
            }
            None => SendOutcome::Refused(e.to_string()),
        },
    }
}

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

/// Compose the body a retired question is left showing.
///
/// The NOTE is the part that must always survive, so room is reserved for it and the QUESTION is
/// what gets clipped. Clipping the composed string did the opposite: on exactly the long question
/// this was written for, the clip ate the note off the end and the operator watched the keyboard
/// vanish with no word of what had happened to it.
///
/// Escaping happens after clipping and can lengthen — `&` becomes five characters — so the reserve
/// is measured on the escaped note, and the escaped body is clipped again as a backstop.
fn retirement_body(original: &str, note: &str) -> String {
    let note_room = escape_html(note).len() + 8;
    let room = crate::queue::MAX_TEXT.saturating_sub(note_room);
    let head = crate::queue::fit(original, room).0;
    let body = escape_html(&format!("{head}\n\n\u{2713} {note}"));
    crate::queue::fit(&body, crate::queue::MAX_TEXT).0
}

/// Does this error mean the topic is gone, as opposed to anything else?
///
/// Deliberately narrow. A broad match here would turn an ordinary network failure into a topic
/// rebinding, which creates a second topic and splits a project's history in half.
fn is_topic_gone(err: &teloxide::RequestError) -> bool {
    let said = err.to_string().to_lowercase();
    said.contains("message thread not found") || said.contains("topic_deleted")
}

/// What to assume when Telegram refuses for flooding and does not say for how long.
///
/// The measured shape of the refusal is a sixty-second window: twenty sends were accepted in the
/// first nineteen seconds and `retry_after` then counted down the rest of the minute
/// (`docs/RATE-PROBE.md`). So a whole minute is the longest the chat can be shut for by one of
/// these, and assuming the longest is the fail-closed direction — guessing short means walking
/// straight back into the wall and earning a fresh one.
pub const FLOOD_WAIT_WHEN_UNSAID: std::time::Duration = std::time::Duration::from_secs(60);

/// Is this a 429, and for how long?
///
/// **Both shapes, because the repo has a fixture for the one that loses the number.** teloxide
/// carries a flood wait two ways and only one of them is typed: `RequestError::RetryAfter` holds
/// the seconds, while a refusal Telegram sent without `parameters.retry_after` — or one teloxide's
/// error table does not know — arrives as `Api(Unknown("Too Many Requests: retry after 5"))`, where
/// `retry_after()` answers `None` and the seconds exist only inside the sentence. Matching only the
/// typed one would miss exactly the shape `only_a_deleted_topic_looks_like_a_deleted_topic` already
/// pins as a real thing this bot receives.
///
/// Note what the typed variant's `Display` is: `Retry after 41`, with no "too many requests" in it
/// at all. A phrase match on that alone finds the shape that has already lost the number and misses
/// the one that kept it.
///
/// Fails CLOSED. A 429 whose seconds cannot be recovered is still a 429, and the answer is a
/// conservative wait rather than a fall through to "Telegram refused this permanently" — which is
/// what the whole chat used to be told about something that mends itself inside a minute.
pub fn flood_wait(err: &teloxide::RequestError) -> Option<std::time::Duration> {
    if let teloxide::RequestError::RetryAfter(seconds) = err {
        return Some(seconds.duration().max(crate::queue::MIN_FLOOD_WAIT));
    }
    let said = err.to_string().to_lowercase();
    if !said.contains("too many requests") && !said.contains("flood") {
        return None;
    }
    Some(
        seconds_named_in(&said)
            .map(std::time::Duration::from_secs)
            .unwrap_or(FLOOD_WAIT_WHEN_UNSAID)
            .max(crate::queue::MIN_FLOOD_WAIT),
    )
}

/// The number of seconds an error sentence names, when it names one.
///
/// Reads the run of digits after "retry after". Deliberately narrow: the first number in the
/// sentence is not necessarily the wait — a chat id is a number too — and a wait read off the wrong
/// one either reopens the chat far too early or shuts it for a week.
fn seconds_named_in(said: &str) -> Option<u64> {
    let after = said.split("retry after").nth(1)?;
    let digits: String = after
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

impl Surface for Telegram {
    async fn create_topic(&self, title: &str, icon_color: u8) -> Result<i32, Refused> {
        let colour = TOPIC_COLOURS[(icon_color as usize) % TOPIC_COLOURS.len()];
        let made = self
            .bot
            .create_forum_topic(self.forum, title)
            .icon_color(Rgb {
                r: ((colour >> 16) & 0xFF) as u8,
                g: ((colour >> 8) & 0xFF) as u8,
                b: (colour & 0xFF) as u8,
            })
            .await;
        match made {
            Ok(topic) => Ok(topic.thread_id.0.0),
            // The seconds are carried out as a VALUE. This used to be a bare `?`, so a flood wait
            // on topic creation became an opaque sentence and the chat's budget never heard about
            // it — while every other project carried on sending into a chat Telegram had shut.
            Err(e) => Err(Refused {
                why: e.to_string(),
                flood_wait: flood_wait(&e),
            }),
        }
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

        what_became_of_it(req.await)
    }

    async fn say_in_general(&self, text: &str) -> SendOutcome {
        // No `message_thread_id`, which is what puts a message in the forum's General rather than
        // in a topic. Escaped like everything else an agent could have influenced the shape of.
        what_became_of_it(
            self.bot
                .send_message(self.forum, escape_html(text))
                .parse_mode(ParseMode::Html)
                .await,
        )
    }

    async fn rewrite(&self, msg_id: &MsgId, text: &str) -> anyhow::Result<()> {
        let Ok(raw) = msg_id.as_str().parse::<i32>() else {
            anyhow::bail!("that message id is not one this bot wrote");
        };
        // Clipped for the same reason a retirement is: an edit whose body is over Telegram's limit
        // FAILS, and a failed edit here leaves the operator reading a stale count.
        let body = crate::queue::fit(&escape_html(text), crate::queue::MAX_TEXT).0;
        self.bot
            .edit_message_text(self.forum, MessageId(raw), body)
            .parse_mode(ParseMode::Html)
            .await?;
        Ok(())
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
        // Re-clipped AFTER composing. `original` is already the clipped text that was sent, but the
        // note adds length and escaping can multiply it — `&amp;` is five characters where one was.
        // An edit whose body is over Telegram's limit FAILS, and a failed edit leaves the answered
        // keyboard live, still offering a choice that has already been made.
        // The NOTE is the part that must always survive, so room is reserved for it and the
        // QUESTION is what gets clipped. Clipping the composed string did the opposite: on exactly
        // the long question this was written for, the clip ate the note off the end and the operator
        // watched the keyboard vanish with no word of what happened to it.
        //
        // Escaping happens after, and can lengthen — `&` becomes five characters — so the reserve
        // is measured on the escaped note and the escaped body is clipped again as a backstop.
        let body = retirement_body(original, note);
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
    use std::time::Duration;

    #[test]
    fn a_long_question_keeps_its_note_when_its_keyboard_is_retired() {
        // The note is the whole point of the retirement. Clipping the COMPOSED string ate it off
        // the end on exactly the long question the clip was written for, so the operator watched a
        // keyboard vanish with no word of what had happened to it — which reads as the bot losing
        // the question, the opposite of the reassurance this exists to give.
        let long = "q".repeat(crate::queue::MAX_TEXT * 2);
        let body = retirement_body(&long, "answered from your phone — Yes");
        assert!(
            body.contains("answered from your phone"),
            "the note was clipped off the end: {}",
            &body[body.len().saturating_sub(120)..]
        );
        assert!(
            body.chars().count() <= crate::queue::MAX_TEXT,
            "{}",
            body.chars().count()
        );
        assert!(
            body.starts_with("qqq"),
            "the question was thrown away instead of clipped"
        );
    }

    #[test]
    fn a_retirement_body_escapes_the_question_and_the_note() {
        let body = retirement_body("rm -rf <dir> && echo \"done\"", "answered — Yes & no");
        assert!(
            !body.contains("<dir>"),
            "an unescaped tag reached an HTML message: {body}"
        );
        assert!(body.contains("&lt;dir&gt;"), "{body}");
        assert!(body.contains("&amp;"), "{body}");
    }

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
    fn a_flood_wait_is_recognised_in_both_the_shapes_telegram_sends_it() {
        // Two shapes, and only one of them keeps the number where it can be read as a number.
        // teloxide types the one whose JSON carried `parameters.retry_after`; the other arrives as
        // an unknown API error with the seconds buried in the sentence, and `retry_after()` answers
        // None for it. This repo already has a fixture for the second one, four lines below, where
        // it is pinned as a real thing this bot receives and must not mistake for a deleted topic.
        let typed = teloxide::RequestError::RetryAfter(teloxide::types::Seconds::from_seconds(41));
        assert_eq!(
            flood_wait(&typed),
            Some(Duration::from_secs(41)),
            "a 429 that carried its seconds as a number was not recognised as one"
        );

        let in_the_sentence = teloxide::RequestError::Api(teloxide::ApiError::Unknown(
            "Too Many Requests: retry after 5".to_owned(),
        ));
        assert_eq!(
            flood_wait(&in_the_sentence),
            Some(Duration::from_secs(5)),
            "a 429 whose seconds were only in its text was read as an ordinary refusal"
        );
    }

    #[test]
    fn a_flood_wait_that_will_not_say_how_long_is_given_the_longest_it_could_be() {
        // Fail closed. Guessing short here means walking back into the wall and earning a fresh
        // one, so an unreadable number becomes the whole measured window rather than nothing.
        let no_number = teloxide::RequestError::Api(teloxide::ApiError::Unknown(
            "Too Many Requests: flood control exceeded".to_owned(),
        ));
        assert_eq!(
            flood_wait(&no_number),
            Some(FLOOD_WAIT_WHEN_UNSAID),
            "a 429 with no readable number fell through as if it were not a 429 at all"
        );
    }

    #[test]
    fn an_ordinary_refusal_is_never_mistaken_for_a_flood_wait() {
        // The mirror of the deleted-topic test below, and it matters for the same reason: a broad
        // match would silence the whole chat for a minute over a message that was merely malformed,
        // and tell the agent to come back later about something that will never work.
        for other in [
            "Bad Request: message text is empty",
            "Bad Request: message thread not found",
            "Forbidden: bot was blocked by the user",
            "Bad Request: chat not found",
        ] {
            let e = teloxide::RequestError::Api(teloxide::ApiError::Unknown(other.to_owned()));
            assert_eq!(
                flood_wait(&e),
                None,
                "{other} was mistaken for a flood wait, which would silence the chat"
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

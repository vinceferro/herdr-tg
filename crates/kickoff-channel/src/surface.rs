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

use std::sync::atomic::{AtomicU64, Ordering};

use hub_proto::{AskOption, MsgId};
use teloxide::prelude::*;
use teloxide::types::{
    FileId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, MessageId, ParseMode,
    ReactionType, ReplyParameters, Rgb, ThreadId,
};

use crate::hub::{Located, Mark, Refused, SendOutcome, Surface, Upload};
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
            None => SendOutcome::Refused(telegrams_own_words(&e)),
        },
    }
}

/// What Telegram itself said, out of the wrapper the client library puts round it.
///
/// `Refused.why` is read by a person: it lands in his topic under an agent's words when an upload
/// is turned away (`hub::telegram_refused_the_file`). The library's `Display` wraps a description
/// twice — `A Telegram's error: ` round everything, and `Unknown error: "…"`, debug quotes and
/// all, round any description it has no variant of its own for — so passing that on put a
/// library's grammar and an escaped string in front of him, and made the hub's own attempt to trim
/// the `Bad Request:` prefix dead code, because the prefix was no longer at the start.
///
/// Unwrapped HERE rather than in the hub, so the hub is never parsing a `Display` it does not own.
fn telegrams_own_words(e: &teloxide::RequestError) -> String {
    match e {
        // The description the API sent, held verbatim. Its own `Display` is the debug quoting.
        teloxide::RequestError::Api(teloxide::ApiError::Unknown(said)) => said.clone(),
        // A description the library has a variant for renders as a sentence — usually Telegram's
        // words exactly, measured in the test beside this.
        teloxide::RequestError::Api(known) => known.to_string(),
        // Everything else is the library talking about itself — a network failure, a body it
        // could not parse. The hub says "Telegram would not take it" and nothing more for these.
        other => other.to_string(),
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

/// Does this error mean the edit was already applied, as opposed to refused?
///
/// Telegram answers an edit whose text and keyboard are already exactly what it is asked to set
/// with an error. For every edit in this file that is not a failure but the thing the caller
/// wanted: the message already says that, and a retirement's keyboard is already off. Read as a
/// failure, it journalled a line saying the keyboard would not come off while it was off, and kept
/// a record of a question nobody can answer any more — which the next session then tries to retire
/// all over again.
///
/// Both shapes, for the reason `flood_wait` takes both: the client library has a typed variant
/// whose Display carries the whole sentence Telegram sends, and a refusal it does not recognise
/// comes through as `Unknown` holding whatever wording arrived. "message is not modified" is the
/// part the two share.
///
/// Deliberately narrow, for the reason `is_topic_gone` is. A broad match here would swallow the
/// refusals that matter most — a body over the limit, a message this bot did not write — and call
/// a keyboard that is still live on his phone done with.
fn the_edit_was_already_applied(err: &teloxide::RequestError) -> bool {
    err.to_string()
        .to_lowercase()
        .contains("message is not modified")
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
                // Telegram's own words, never the library's wrapper round them: a reason that
                // reaches a person must not read as a Rust type, and the hub matches on phrases
                // in here — "file is too big" — that a wrapper would one day move.
                why: telegrams_own_words(&e),
                flood_wait: flood_wait(&e),
            }),
        }
    }

    async fn send(
        &self,
        topic_id: i32,
        text: &str,
        buttons: &[AskOption],
        reply_to: Option<&MsgId>,
    ) -> SendOutcome {
        let mut req = self
            .bot
            .send_message(self.forum, escape_html(text))
            .parse_mode(ParseMode::Html)
            .message_thread_id(ThreadId(MessageId(topic_id)));

        // Threaded under HIS message when the line is about one. `allow_sending_without_reply`
        // is the fallback the operator would want: he may have deleted the line he typed, and a
        // refusal that then failed to send would be a second silence about the same words. An
        // id this bot cannot read as a Telegram message id is not one of his; the line goes out
        // bare rather than not at all.
        if let Some(under) = reply_to
            && let Ok(raw) = under.as_str().parse::<i32>()
        {
            req = req.reply_parameters(
                ReplyParameters::new(MessageId(raw)).allow_sending_without_reply(),
            );
        }

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
        // An edit that changed nothing is the line already saying that, not a failure.
        match self
            .bot
            .edit_message_text(self.forum, MessageId(raw), body)
            .parse_mode(ParseMode::Html)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) if the_edit_was_already_applied(&e) => Ok(()),
            Err(e) => Err(e.into()),
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
        // An edit that changed nothing means the keyboard is already off and the body already says
        // this. That is the outcome this call exists for, so it is reported as one: read as a
        // failure it wrote a false error line and kept a record of a question nobody can answer,
        // and the caller then tried the same edit again on the next session.
        match self
            .bot
            .edit_message_text(self.forum, MessageId(raw), body)
            .parse_mode(ParseMode::Html)
            .await
        {
            Ok(_) => {}
            Err(e) if the_edit_was_already_applied(&e) => {}
            Err(e) => return Err(e.into()),
        }
        tracing::info!(
            topic = topic_id,
            message = raw,
            note,
            "a question stopped being asked"
        );
        Ok(())
    }

    async fn locate(&self, file_id: &str) -> Result<Located, Refused> {
        match self.bot.get_file(FileId(file_id.to_owned())).await {
            Ok(file) => Ok(Located {
                file_id: file.meta.id.0,
                file_unique_id: file.meta.unique_id.0,
                // The library fills an absent `file_size` with `u32::MAX` rather than leaving it
                // absent. Four gigabytes is not a size, and the hub must not act on it as one.
                file_size: (file.meta.size != u32::MAX).then_some(u64::from(file.meta.size)),
                file_path: file.path,
            }),
            Err(e) => Err(Refused {
                // Telegram's own words, never the library's wrapper round them: a reason that
                // reaches a person must not read as a Rust type, and the hub matches on phrases
                // in here — "file is too big" — that a wrapper would one day move.
                why: telegrams_own_words(&e),
                flood_wait: flood_wait(&e),
            }),
        }
    }

    async fn download(
        &self,
        file_path: &str,
        into: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
    ) -> Result<(), Refused> {
        // The library builds the URL — `file/bot<token>/<file_path>` — and streams the body into
        // the hub's writer. This surface never builds that URL itself: it carries the token, and a
        // hand-rolled fetch would put the token in the first log line of the first failure.
        use teloxide::net::Download as _;
        self.bot
            .download_file(file_path, into)
            .await
            .map_err(|e| Refused {
                why: e.to_string(),
                flood_wait: None,
            })
    }

    async fn send_file(&self, topic_id: i32, file: &Upload, caption: &str) -> SendOutcome {
        // From MEMORY, never from a path. `InputFile::file(path)` opens with a plain `open`, which
        // follows links, and names the upload after the last path segment — exactly the two
        // things the outbox rules forbid. The hub read these bytes off a descriptor it opened
        // without following anything and checked; what he sees the file called is set here, as
        // data, from what the adapter said.
        let input = InputFile::memory(file.bytes.clone()).file_name(file.filename.clone());
        let thread = ThreadId(MessageId(topic_id));
        // The caption is escaped like every other agent-authored string that goes out as HTML;
        // an empty caption is left off entirely rather than sent as an empty string.
        let answered = if file.as_photo {
            let mut req = self
                .bot
                .send_photo(self.forum, input)
                .message_thread_id(thread);
            if !caption.is_empty() {
                req = req
                    .caption(escape_html(caption))
                    .parse_mode(ParseMode::Html);
            }
            req.await
        } else {
            let mut req = self
                .bot
                .send_document(self.forum, input)
                .message_thread_id(thread);
            if !caption.is_empty() {
                req = req
                    .caption(escape_html(caption))
                    .parse_mode(ParseMode::Html);
            }
            req.await
        };
        what_became_of_it(answered)
    }

    async fn mark(&self, chat_id: i64, msg_id: &MsgId, mark: Mark) -> Result<(), Refused> {
        let Ok(raw) = msg_id.as_str().parse::<i32>() else {
            return Err(Refused {
                why: "that message id is not one Telegram gave".to_owned(),
                flood_wait: None,
            });
        };
        // The whole list is SET, not added to — that is what the API call does — and the list is
        // exactly one long, so the eyes come off when the tick goes on. A reaction on his message
        // lands in whichever topic he typed it in; the message id is the address, and no thread
        // id is needed or taken.
        //
        // The seconds come out as a VALUE, as they do for a topic: the hub says nothing about a
        // reaction refused for the ceiling and warns once about one refused for anything else,
        // and it can only tell the two apart if this side does.
        self.bot
            .set_message_reaction(ChatId(chat_id), MessageId(raw))
            .reaction(reaction_for(mark))
            .await
            .map_err(|e| Refused {
                why: e.to_string(),
                flood_wait: flood_wait(&e),
            })?;
        Ok(())
    }
}

/// The one reaction a stage is shown as.
///
/// `✅` and `❌` — what the operator asked for — do not exist as a bot's free reactions: the API
/// refuses them as `REACTION_INVALID` (`docs/RATE-PROBE.md` §3). The eyes are on the list; the
/// thumbs are the nearest honest pair for "the agent has it" and "it did not reach the agent",
/// and the cross's job of saying WHY is done by the line under his message, not by the mark.
///
/// Exactly one element, always. Two would stack, and a bot may set one reaction per message.
fn reaction_for(mark: Mark) -> Vec<ReactionType> {
    let emoji = match mark {
        Mark::HandedOn => "👀",
        Mark::Accepted => "👍",
        Mark::Refused => "👎",
    };
    vec![ReactionType::Emoji {
        emoji: emoji.to_owned(),
    }]
}

/// The operator reads the app, and there is no phone line at all.
///
/// [`Telegram`] above is a CARRIER: it puts words somewhere a person will see them and hands back
/// the id of what it put there. This one carries nothing, and it does not have to — the operator's
/// copy of every question and every line is the ring, which the hub appends to as a frame reaches
/// its handling, before any surface is asked; his answers come back through the drop beside it.
///
/// **So what is left here is bookkeeping, and it SUCCEEDS on purpose.** The hub writes down what a
/// question's buttons mean only where a surface handed back a receipt: a surface that refused would
/// put every question on the ring with nothing written down behind it, and nothing at the door
/// could ever answer one. This plane needed a surface that succeeds, not a hub that changes.
///
/// Nothing here is a stub for a carrier that is coming. The two things it mints — a number per
/// conversation and a receipt per message — are what the hub's own records are keyed on, and they
/// are minted so that a box which later gains a forum cannot collide with them.
///
/// It is what `kickoff-channel serve --to app` puts where Telegram stands, and the only surface on that
/// plane: the hub builds one of these, opens its socket, and never dials anything.
pub struct TheApp {
    /// The second this run started, stamped on every receipt it mints.
    ///
    /// The receipts have to stay apart across the LEDGER's life, not merely this process's: a
    /// question written down by one run is still open when the next starts, and the ledger is
    /// keyed on the receipt, so two runs minting the same one means the second's question
    /// overwrites the first's and the first can never be answered again.
    ///
    /// A counter alone starts over at one every restart. This is what keeps a restart's first
    /// receipt clear of the last run's without a file of its own to keep a counter in: one flock
    /// lets one hub hold the box at a time, and the unit waits five seconds before starting the
    /// next, so the ordinary restart cannot land in the same second. Said plainly because it is
    /// not a proof: somebody restarting by hand inside one second would repeat, and what that
    /// costs is the questions that were open when he did it.
    run_started: u64,
    /// The next number a new conversation gets, or nothing at all once the range is used up.
    ///
    /// Counting DOWN, and negative for a reason that outlives this plane: a forum numbers its
    /// threads from one upward, so a number from here can never be one of those — it cannot
    /// collide with a topic a box already has, it is unmistakable in an audit line, and a box
    /// that later gains a forum cannot bind a real topic on top of one of these.
    next_topic: std::sync::Mutex<Option<i32>>,
    /// How many receipts this run has minted.
    minted: AtomicU64,
}

/// What every receipt minted here starts with.
///
/// Distinct from the phone's ids, which are Telegram's own numbers, and from the door's `d…` and
/// `w…`: a reader of an audit line or an adapter's log can see which surface a message came from
/// without being told. It carries no colon, because the ledger's key is the conversation and the
/// message with a colon between them and every record is found again by splitting it there.
const A_RECEIPT: &str = "app-";

impl TheApp {
    /// `run_started` is the second this hub started; `lowest_topic_bound` is the floor read off
    /// the registry ([`crate::registry::Registry::lowest_topic_bound`]).
    ///
    /// The floor is clamped at zero before it is stepped past, so a box whose registry holds a
    /// real forum's thread numbers — one switched to the app after running on the phone — starts
    /// at −1 rather than counting down from a positive number and walking through the range a
    /// forum mints from.
    pub fn new(run_started: u64, lowest_topic_bound: Option<i32>) -> Self {
        let floor = lowest_topic_bound.unwrap_or(0).min(0);
        Self {
            run_started,
            // `checked_sub`, so a floor at the very bottom of the range leaves nothing rather
            // than wrapping round into the numbers a forum gives out.
            next_topic: std::sync::Mutex::new(floor.checked_sub(1)),
            minted: AtomicU64::new(0),
        }
    }

    /// One receipt, unique for the life of the ledger this run writes into.
    fn a_receipt(&self) -> MsgId {
        let n = self.minted.fetch_add(1, Ordering::Relaxed) + 1;
        MsgId::new(format!("{A_RECEIPT}{}-{n}", self.run_started))
    }
}

impl Surface for TheApp {
    async fn create_topic(&self, title: &str, _icon_color: u8) -> Result<i32, Refused> {
        // The lock is a `std` one and is never held across an await, so there is no order to get
        // wrong: what it guards is one read and one write of a single number.
        let given = {
            let mut next = self.next_topic.lock().unwrap_or_else(|e| e.into_inner());
            let Some(given) = *next else {
                // Fail closed. Wrapping would hand the next conversation a number a forum could
                // have given, which is the one thing the descent exists to make impossible.
                return Err(Refused {
                    why: "this hub has run out of numbers to give new conversations".to_owned(),
                    flood_wait: None,
                });
            };
            *next = given.checked_sub(1);
            given
        };
        tracing::debug!(
            title,
            number = given,
            "a conversation was given a number of its own; nobody is reading a phone, so there is \
             no topic to make"
        );
        Ok(given)
    }

    async fn send(
        &self,
        _topic_id: i32,
        _text: &str,
        _buttons: &[AskOption],
        _reply_to: Option<&MsgId>,
    ) -> SendOutcome {
        // A RECEIPT, and never `Clamped`: nothing here shortened anything. Clamped would tell the
        // bridge its words were cut when the ring holds every one of them, which is the one thing
        // a bridge acts on by saying less next time.
        SendOutcome::Sent(self.a_receipt())
    }

    async fn say_in_general(&self, text: &str) -> SendOutcome {
        // `Sent`, not a refusal, and the difference is a loop. The hub says this when the chat is
        // carrying more than it will take, and it keeps the debt set and retries for ever until
        // something answers; a no-op that leaks a retry loop is worse than a no-op. There is no
        // chat and no ceiling here, so this line has nowhere to be and nothing to be about.
        tracing::debug!(text, "nothing was said about a chat that does not exist");
        SendOutcome::Sent(self.a_receipt())
    }

    async fn rewrite(&self, msg_id: &MsgId, _text: &str) -> anyhow::Result<()> {
        // There is no line to edit, and an error here is not free: at the one site that says what
        // became of a tap, a failed edit falls through to a real send — which on this plane would
        // be a second event on the ring saying what the ring's own next line already says.
        tracing::debug!(message = %msg_id, "nothing was rewritten; the ring carries the follow-up");
        Ok(())
    }

    async fn retire_buttons(
        &self,
        _topic_id: i32,
        msg_id: &MsgId,
        _original: &str,
        note: &str,
    ) -> anyhow::Result<()> {
        // The load-bearing one. Success is the only path on which the hub forgets a question's
        // record, and the only one on which it writes its own retirement onto the ring. An error
        // here would leave every answered question standing open in the app for ever, with its
        // record kept for a keyboard that does not exist to be taken off.
        tracing::debug!(
            message = %msg_id, note,
            "a question stopped being asked; there are no buttons to take off"
        );
        Ok(())
    }

    async fn mark(&self, _chat_id: i64, msg_id: &MsgId, mark: Mark) -> Result<(), Refused> {
        // At DEBUG, because this is not a path a line typed in the app can reach: the hub returns
        // before a mark for anything that came in by the door. A warning nobody can act on is a
        // line a reader learns to skip, and the next real one is skipped with it.
        tracing::debug!(
            message = %msg_id, ?mark,
            "no reaction was put on a message nobody sent from a phone"
        );
        Ok(())
    }

    async fn locate(&self, _file_id: &str) -> Result<Located, Refused> {
        // Refused rather than answered with something made up. A location invented here would
        // have the hub open a file at a path it minted and stream nothing into it, and then tell
        // an agent his picture arrived empty. This ends one fetch, under a deadline the hub
        // already holds.
        Err(Refused {
            why: "there is no messaging app here to ask where his file is".to_owned(),
            flood_wait: None,
        })
    }

    async fn download(
        &self,
        _file_path: &str,
        _into: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
    ) -> Result<(), Refused> {
        Err(Refused {
            why: "there is no messaging app here to fetch his file from".to_owned(),
            flood_wait: None,
        })
    }

    async fn send_file(&self, _topic_id: i32, file: &Upload, _caption: &str) -> SendOutcome {
        // A backstop, and it is meant to be one: [`Surface::the_carrier_can_take_a_file`] is the
        // seam a caller reads to turn a file away before it has been read off the disk. Until
        // something reads it, this is what stops a file being called sent when it went nowhere.
        tracing::debug!(file = %file.filename, "an agent's file reached a surface that carries none");
        SendOutcome::Refused("files do not reach him in the app".to_owned())
    }

    fn the_carrier_has_a_ceiling(&self) -> bool {
        false
    }

    fn the_carrier_can_take_a_file(&self) -> bool {
        false
    }

    fn a_topic_number_this_carrier_could_have_minted(&self, topic_id: i32) -> bool {
        // Exactly the numbers `create_topic` above hands out. A box switched to the app after
        // running on the phone has real thread numbers in its registry, and each of those names a
        // conversation this carrier cannot reach — so it is given one of its own instead.
        topic_id < 0
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
    fn a_reaction_replaces_the_last_one_rather_than_stacking() {
        // `setMessageReaction` SETS the message's whole list of reactions. One element means the
        // eyes come off when the thumb goes on; two would be a stack, and a bot may only set one.
        // And every one of them is an emoji Telegram accepts from a bot — `✅` and `❌` are not,
        // measured (`docs/RATE-PROBE.md` §3), which is why the tick is a thumb.
        for mark in [Mark::HandedOn, Mark::Accepted, Mark::Refused] {
            let list = reaction_for(mark);
            assert_eq!(list.len(), 1, "{mark:?} would stack: {list:?}");
            let emoji = list[0]
                .emoji()
                .expect("an emoji reaction, never a custom one");
            assert!(
                ["👀", "👍", "👎"].contains(&emoji.as_str()),
                "{mark:?} uses {emoji}, which the API refused for a bot"
            );
        }
        // Three stages, three different marks: a stage he cannot tell from the last is no stage.
        let all: std::collections::BTreeSet<String> =
            [Mark::HandedOn, Mark::Accepted, Mark::Refused]
                .into_iter()
                .map(|m| reaction_for(m)[0].emoji().cloned().unwrap_or_default())
                .collect();
        assert_eq!(all.len(), 3);
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
    fn a_refusal_the_hub_reads_is_the_words_telegram_sent_and_not_the_librarys_wrapper() {
        // What comes out of here is what the operator reads in his topic, under an agent's words,
        // when Telegram will not take a file. The client library wraps a description TWICE on the
        // way — "A Telegram's error: " round everything, and `Unknown error: "…"`, with the debug
        // quotes, round any description it has no variant of its own for — so handing its Display
        // on put a library's grammar and an escaped string in front of a person, in the one
        // register this repo says carries no jargon.
        let raw = "Bad Request: file must be non-empty";
        let wrapped = teloxide::RequestError::Api(teloxide::ApiError::Unknown(raw.to_owned()));
        assert!(
            wrapped.to_string().contains("Unknown error:"),
            "the library stopped wrapping, and this is a test about the wrapping: {wrapped}"
        );
        match what_became_of_it(Err(wrapped)) {
            SendOutcome::Refused(why) => assert_eq!(why, raw),
            other => panic!("a refusal came back as {other:?}"),
        }

        // A description the library DOES have a variant for is its own English, which is a
        // sentence rather than a wrapper — measured, not assumed: `WrongFileIdOrUrl` renders the
        // description Telegram sent, verbatim.
        let known: teloxide::ApiError = serde_json::from_value(serde_json::json!(
            "Bad Request: wrong file identifier/HTTP URL specified"
        ))
        .expect("the library knows this one");
        match what_became_of_it(Err(teloxide::RequestError::Api(known))) {
            SendOutcome::Refused(why) => {
                assert!(!why.contains("A Telegram's error"), "{why}");
                assert!(why.contains("wrong file identifier"), "{why}");
            }
            other => panic!("a refusal came back as {other:?}"),
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

    #[test]
    fn an_edit_telegram_calls_unchanged_is_a_keyboard_that_is_already_off() {
        // Telegram refuses an edit whose content is already what it is asked to set. Read as a
        // failure, a retirement that had already happened wrote an error line saying the keyboard
        // would not come off — while it was off — and kept the record, so the menu stayed written
        // down and the next session tried the same edit again.
        //
        // Both shapes: the long sentence Telegram actually sends, which the library types, and the
        // short one it passes through when it has no variant for the wording.
        let long: teloxide::ApiError = serde_json::from_value(serde_json::json!(
            "Bad Request: message is not modified: specified new message content and reply markup \
             are exactly the same as a current content and reply markup of the message"
        ))
        .expect("the library knows this one");
        assert!(
            the_edit_was_already_applied(&teloxide::RequestError::Api(long)),
            "the sentence Telegram actually sends read as a failed edit"
        );
        let short = teloxide::RequestError::Api(teloxide::ApiError::Unknown(
            "Bad Request: message is not modified".to_owned(),
        ));
        assert!(
            the_edit_was_already_applied(&short),
            "the wording the library does not type read as a failed edit"
        );

        // And nothing else. These are the refusals that leave a live keyboard on his phone, and
        // calling one of them done is how a menu with nothing behind it survives.
        for other in [
            "Bad Request: message thread not found",
            "Bad Request: message text is empty",
            "Bad Request: message to edit not found",
            "Too Many Requests: retry after 5",
        ] {
            let e = teloxide::RequestError::Api(teloxide::ApiError::Unknown(other.to_owned()));
            assert!(
                !the_edit_was_already_applied(&e),
                "{other} was mistaken for an edit that changed nothing"
            );
        }
    }
}

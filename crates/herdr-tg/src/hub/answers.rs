//! The answers drop: the inbound half of the door `door.rs` is the outbound half of.
//!
//! The PWA's gateway — a later increment, not built here — writes one file per operator action
//! into `<state>/answers/`, and the hub's own watch loop sweeps that directory about once a
//! second. Each file is one JSON object: a tap (`{"t":"choice",…}`) or typed words
//! (`{"t":"message",…}`). A sibling `.result` file is written beside each consumed answer —
//! accepted, or refused with one plain-word sentence — which is everything the gateway is told
//! and all it needs: it minted the file's name, so it already knows the rest.
//!
//! # Untrusted input at a door
//!
//! The drop sits inside the hub's 0700 state home, so nobody outside this user can reach it — but
//! the FILES are still untrusted input, because the gateway is another process that can be wrong,
//! be old, or be replaying a queue from yesterday. So every law a wire door holds holds here too:
//!
//! * **Ids are shape-refused.** A conversation must be a shape the registry mints
//!   ([`crate::conversations::is_conversation_id`]); an ask or option id must be a shape the hub
//!   already writes down — the button law from `handle`'s own check, since the same ids come BACK
//!   from the door as went out on the buttons. A control character in an id would forge a line in
//!   the audit or the ring, and a `|` in an option id is the separator a button cannot carry. A
//!   `lane`, when a message or a choice names one, gets the same addressability law the wire
//!   applies at `hello` — the door cannot be where a lane stops being a name the hub will address a
//!   conversation by. And a KNOWN field a program wrote wrongly — a reply id, a lane, a receipt
//!   name — is refused, never silently stripped: unknown FIELDS stay ignored so a newer gateway
//!   cannot break the door, but a known one that fails its law is the file saying something the
//!   hub cannot believe, and sending it as though it had said nothing would be the hub rewriting
//!   what he wrote.
//! * **One size bound.** A file past [`AT_MOST`] is refused unread. That bound is half a wire
//!   frame, so the frame built from any answer a sweep accepts cannot exceed the frame bound the
//!   codec enforces on everything else.
//! * **Consumed either way.** A refused answer is as consumed as an accepted one — moved to its
//!   result file, never left to be swept again. A file that wedged the sweep would be a file the
//!   gateway waits on for ever, and a malformed one re-swept every second is unbounded work for
//!   no one's benefit.
//! * **Nothing but routing.** The sweep reads, judges and delivers; it starts nothing, follows no
//!   link (a symlink in the drop is refused and unlinked, never opened), and believes nothing
//!   beyond the parsed shape. File names are opaque — the gateway mints them — except the one
//!   namespace the hub keeps for itself, a `.result` suffix.
//!
//! # Staleness, and whose clock it is
//!
//! Every answer carries its own `ts`, set by the gateway, and the hub distrusts-but-uses it for
//! one thing only: age. An answer older than [`STALE_AFTER`] is refused unread, before so much as
//! a question is looked up — a gateway that was parked for an afternoon replaying yesterday's
//! taps must never answer a question that is live now, and "when it was written" is the only fact
//! that separates a replay from an answer. The clock the age is measured against is the hub's
//! own; a `ts` in the future reads as fresh, because the number is a freshness claim by a writer
//! already inside the state home, not a proof of anything. A file with no `ts` at all is refused:
//! the hub cannot prove it fresh, and fail-closed is the rule everywhere else a door cannot
//! prove what it was about to do.
//!
//! # What a result carries
//!
//! `{"t":"result","status":"accepted"}` or `{"t":"result","status":"refused","why":"…"}` — and
//! nothing else. No conversation, ask or option id beyond what the sentence itself says, no path,
//! no identifier of this machine: the gateway that reads it already knows which of its own files
//! it answers, and a result file is no place to learn the shape of the hub's state directory.
//! Results are swept away once older than [`RESULTS_KEPT_FOR`]; the answers themselves are
//! always consumed, so the directory drains itself.
//!
//! A result is a RECEIPT, write-once: it says what the hub did with the file, at the moment it
//! consumed it. What the BRIDGE later said about a delivered act — a refusal, a silence past the
//! confirm window — is history, not receipt, and history is the ring's job: the follow-up is
//! appended there (`door.rs`'s ack lines), attributed to the conversation and the lane, and the
//! result is never rewritten to match. Two files, two tenses, and neither may do the other's.
//!
//! # Recorded, not fixed: four shapes the door leaves open on purpose
//!
//! The operator's standing settlement for contrived shapes — the same ruling the write guard's
//! file received — is to write them down rather than chase them:
//!
//! * **The sweep is unbounded inside the registry-watch tick.** Every answer in the drop is
//!   consumed before the tick looks at the registry again, so a gateway (or anything else this
//!   uid runs) that drops files faster than the hub refuses them delays the very next thing the
//!   tick does — the kill-switch fingerprint for a project switched off at the terminal. Each
//!   refused file is cheap, but "cheap, unboundedly often" is a delay with no floor. Unbounded
//!   on purpose: the writer is already this user inside the state home, and the honest bound
//!   would punish a healthy gateway's burst to stop a misbehaving one.
//! * **A drop entry that cannot be removed churns.** An answer whose `remove` fails — an
//!   immutable bit, a filesystem in a strange mood — is re-swept every tick, refused again, and
//!   given a FRESH result each time, so the ten-minute result sweep never collects it: one warn
//!   a second, for ever, and a file the gateway is told about once a second. Recorded because
//!   only this uid can build it and no bound would mend the cause.
//! * **Two same-uid TOCTOU windows.** Between the metadata read and the bytes, a plain file can
//!   be swapped for a link (the door then reads through a link once); and a result's bytes are
//!   put down under a staging name first, which is BOTH opened and chmodded, and each of those
//!   follows a link. So a link planted at the staging name takes the receipt's bytes and has its
//!   target narrowed to 0600 — and that target need not be in this directory at all, which is
//!   the reach the chmod adds and the open alone did not have. The half that used to be worth
//!   something here is gone: a result reaches `<name>.result` by rename, which
//!   REPLACES a link pre-planted at that name instead of writing through it — and that name is
//!   the only one of the two the gateway is ever told, so it is the only one anybody could lie
//!   in wait at knowingly; the staging name carries this pid and has to be guessed at blind.
//!   What is left needs a writer that is already this user, inside a 0700
//!   directory — at which point the writer owns the box and the ring is not the secret worth
//!   protecting. Same ruling, same file: recorded, closed by nothing, reopened the day the drop
//!   moves outside the state home.
//! * **A name with no headroom left gets no receipt.** [`where_a_result_is_staged`] adds about
//!   twenty-one bytes to a result's name, and nothing here bounds how long a name in the drop
//!   may be, so a name near the filesystem's own limit cannot be staged. The act still happens —
//!   the file is read, judged and consumed exactly as always — and only the receipt is lost, so
//!   the gateway waits its whole window and answers its caller that nothing has been said yet.
//!   No name this hub or the gateway mints comes near it; it needs a program writing into the
//!   drop directly. Recorded on the same ruling: same uid, inside the 0700 directory.

use std::path::{Path, PathBuf};

use hub_proto::{AskId, LaneId, OptionId, ProjectId};

use super::{CALLBACK_DATA_MAX, lane_is_addressable};

/// The drop directory, beside the audit log and every other state file of this hub.
pub const ANSWERS: &str = "answers";

/// The suffix that marks a result file. The one namespace the hub keeps in the drop: a file
/// wearing it is a result, never an answer, and is swept away when old rather than acted on.
const RESULT_SUFFIX: &str = ".result";

/// The most bytes one answer may be. Half a wire frame, so that the message frame built from the
/// largest answer the sweep accepts — envelope, ids and the JSON around the text included —
/// cannot pass the frame bound the codec enforces on every other frame this hub sends.
pub const AT_MOST: u64 = hub_proto::MAX_FRAME_BYTES as u64 / 2;

/// How old an answer may be, by its own timestamp, before the hub refuses it unread.
///
/// A minute, because that is several sweeps' worth of a healthy gateway's latency and nothing
/// like long enough for yesterday's queue: the number exists to separate a replay from an
/// answer, not to police the gateway's uptime.
pub const STALE_AFTER: u64 = 60;

/// How long a result file is kept before the sweep takes it away.
///
/// Ten minutes: far past any round trip a gateway could still be waiting on, short enough that a
/// dead gateway's last results are not a permanent fixture of the state directory.
pub const RESULTS_KEPT_FOR: u64 = 600;

/// One operator action read out of the drop — already shape-checked, already fresh.
///
/// The conversation is a [`ProjectId`] because that is what a conversation IS on this wire; the
/// hub resolves it to a live session, and the file's other names (ask, option) are the bridge's
/// own opaque ids, resolved against what the hub wrote down when the question went out. Either
/// kind may also name a `lane` — one conversation OF the project, a worktree — which the same
/// addressability law the wire applies is applied to here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    Choice {
        conversation: ProjectId,
        /// Which conversation of the project the answer is for, when the file named one — the
        /// one way out of two live questions minted under one name, which is what the ambiguous
        /// refusal's sentence tells the operator to name. `None` is every question under the
        /// name, and two of those are still refused rather than guessed between.
        lane: Option<LaneId>,
        ask_id: AskId,
        option_id: OptionId,
    },
    Message {
        conversation: ProjectId,
        /// One conversation of the project the words are for, when the file named one. `None`
        /// is the project's own voice — and an answer naming no lane while the voice is dead
        /// and a lane is live is refused with directions rather than guessed for him.
        lane: Option<LaneId>,
        text: String,
        in_reply_to_ask: Option<AskId>,
        /// The receipt name the SENDER minted for its own words — an opaque `w…` that names
        /// nothing on this box, carried so the reader that sent the line can recognise its
        /// echo instead of drawing the line twice. The hub passes it through: it rides the
        /// wire frame's `msg_id` and the ring's receipt line, and it is never one the hub would
        /// have minted itself. A KNOWN optional field, held to the one shape law the door's
        /// other opaque handles keep.
        receipt: Option<String>,
    },
}

/// The sentences a refused result can carry. Plain words, no ids, no jargon: the operator may
/// read one of these in the app that wrote the answer, and a sentence he cannot act on is a
/// sentence the whole result file exists to prevent.
///
/// One string and not an enum, because the caller has nothing to decide from the difference —
/// the sentence IS the product. Every refusal is written once into the result and once into the
/// journal; nothing else branches on which one it was.
pub(crate) mod said {
    // A child module inherits nothing from its parent, and the judgements named below are the
    // hub's own — written here rather than imported at the top of the file so the sentence and
    // the thing it explains sit in one place.
    use crate::hub::{TapRefusal, Withdrawal};

    pub const NOT_READABLE: &str =
        "That answer could not be read as one answer, so it was not sent.";
    pub const NOT_A_PLAIN_FILE: &str =
        "That answer was not a plain file, so it was not read and not sent.";
    pub const TOO_BIG: &str =
        "That answer was too large to be one answer, so it was not read and not sent.";
    pub const NO_CLOCK: &str = "That answer did not say when it was written, so it was not sent.";
    pub const STALE: &str = "That answer was left here more than a minute ago, so it was not \
                             sent — the question may have moved on since it was written.";
    pub const NO_SUCH_CONVERSATION: &str =
        "That answer names a conversation this hub does not know, so it was not sent.";
    pub const NO_KIND: &str =
        "That file did not say whether it was an answer or a message, so it was not sent.";
    pub const BAD_ASK: &str = "That answer names its question in a shape this hub does not write down, so it was not \
         sent.";
    pub const BAD_OPTION: &str = "That answer names the button it chose in a shape this hub does not write down, so it \
         was not sent.";
    pub const NO_WORDS: &str = "That message had no words in it, so it was not sent.";
    pub const BAD_REPLY: &str = "That reply names its question in a shape this hub does not write down, so it was not \
         sent.";
    pub const BAD_RECEIPT: &str = "That message names the receipt its words should come back under in a shape this hub \
         does not write down, so it was not sent.";
    pub const BAD_LANE: &str = "That message names one of the project's own conversations in a shape this hub does not \
         address, so it was not sent.";
    pub const BAD_LANE_ON_AN_ANSWER: &str = "That answer names one of the project's own conversations in a shape this hub does not \
         address, so it was not sent.";
    pub const WRITTEN_DOWN_TWICE: &str = "That question is written down more than once here and the answer does not say which it \
         means, so it was not sent.";
    pub const NO_SUCH_QUESTION_IN_THAT_LANE: &str = "That answer names one of the project's own conversations, and no question open \
         under that name is being asked there, so it was not sent.";
    pub const ASKED_BY_TWO: &str = "That reply names a question two conversations are asking at once, and it does not say \
         which it means, so it was not sent.";
    pub const NOTHING_TO_TAKE_WORDS: &str = "Nothing is connected for that conversation right now, so nothing was sent. It will not \
         be delivered later.";
    pub const ONLY_ITS_OWN_CONVERSATIONS: &str = "Nothing is connected for this conversation itself right now — one of its own \
         conversations is. Name which one and it will be sent; nothing will be delivered later.";

    /// Why a tap the door judged refused did not become an answer, in the door's own words.
    ///
    /// The phone's sentences for the same judgements say "topic" and "button", which are a
    /// phone's words; the reader here is the app that wrote the file, and the sentence has to be
    /// true where HE is reading it.
    pub fn why_a_tap_was_refused(why: &TapRefusal) -> &'static str {
        match why {
            TapRefusal::NoRecord => {
                "There is no question written down under that name, so the answer was not sent."
            }
            TapRefusal::NotAnOption => {
                "That answer is not one of the ones written down for this question, so it was \
                 not sent."
            }
            TapRefusal::NotConnected => {
                "Nothing is connected for that conversation right now, so the answer was not \
                 sent."
            }
            TapRefusal::Restarted => {
                "That question belonged to a session that has since restarted, so the answer \
                 was not sent. Ask again and it will come back."
            }
            TapRefusal::AlreadyAnswered => {
                "That question has already been answered, so it was not answered again and \
                 nothing was sent."
            }
            TapRefusal::NoLongerAsked => {
                "That question is no longer being asked, so the answer was not sent."
            }
            // The door has no person gate — the writer is already inside the state home — so
            // this is a judgement the sweep cannot have made. Refused in plain words rather
            // than leaked: fail closed even on a branch nobody can reach.
            TapRefusal::NotYours => "That answer was not sent.",
        }
    }

    /// What the operator learns when a tap reached the hub, was written down as answered, and
    /// then could not be handed to the session that asked. Three, because the three endings are
    /// not interchangeable and the phone keeps them apart for the same reason
    /// (`a_tap_that_reached_nobody` in `bot.rs`): the difference is whether he may usefully try
    /// again.
    pub fn why_a_tap_reached_nobody(what: Withdrawal) -> &'static str {
        match what {
            Withdrawal::Retired => {
                "That answer did not reach whatever asked, so it was not sent. The question has \
                 been taken away — a question that cannot be answered is worse than none."
            }
            Withdrawal::StillOnHisPhone => {
                "That answer did not reach whatever asked, so it was not sent. It can be \
                 answered again."
            }
            Withdrawal::NothingLeftToTakeBack => {
                "That answer did not reach whatever asked, so it was not sent. That question \
                 has since finished at its own end, so there is nothing left to answer."
            }
        }
    }
}

/// Is this id a shape the hub will write down from the door?
///
/// The button law the hub already enforces where a question arrives (`handle`'s own check): an
/// option id rides a Telegram button, where a `|` splits wrong on the way back and 64 bytes is
/// the whole allowance. The SAME law is applied to the ask id rather than a weaker one, because
/// both are the bridge's own opaque ids the hub stores, echoes and writes into files one record
/// per line — two laws for the same kind of thing is how one of them gets missed — with the one
/// addition both need at a door the wire does not: no control character, which would forge a
/// line in the audit or the ring.
fn an_id_the_hub_writes_down(s: &str) -> bool {
    !s.is_empty()
        && s.len() + 2 <= CALLBACK_DATA_MAX
        && !s.contains('|')
        && !s.chars().any(char::is_control)
}

/// The lane a file names, when it names one — one law, whichever kind of answer named it.
///
/// The caller hands in the refusal sentence, because the only difference between a message naming
/// a lane badly and an answer naming one badly is the word the reader is looking at when the
/// sentence reaches them.
fn the_lane(
    fields: &serde_json::Map<String, serde_json::Value>,
    refused: &'static str,
) -> Result<Option<LaneId>, &'static str> {
    match fields.get("lane") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => {
            let s = value.as_str().ok_or(refused)?;
            let named = LaneId::new(s);
            if !lane_is_addressable(&named) {
                return Err(refused);
            }
            Ok(Some(named))
        }
    }
}

/// One file's bytes, made into an answer — or the sentence its result carries instead.
///
/// Staleness is judged FIRST, before any name in the file is believed, because "refused unread"
/// is the whole of the promise: a stale answer must not even move a question's state by
/// failing to. Unknown fields beyond the parsed shape are ignored, exactly as the wire ignores
/// an unknown field inside a known kind: a gateway newer than this hub may add one, and a reader
/// that refused it would turn every additive change into a dead door.
pub(crate) fn parse(bytes: &str, now: u64) -> Result<Answer, &'static str> {
    let value: serde_json::Value = serde_json::from_str(bytes).map_err(|_| said::NOT_READABLE)?;
    let Some(fields) = value.as_object() else {
        return Err(said::NOT_READABLE);
    };
    // The clock first. No ts, or one that is not a number, is a file the hub cannot prove fresh
    // — and the age is the hub's arithmetic against the hub's own now, never a comparison of two
    // clocks that never agreed.
    let written = fields
        .get("ts")
        .and_then(serde_json::Value::as_u64)
        .ok_or(said::NO_CLOCK)?;
    if now.saturating_sub(written) > STALE_AFTER {
        return Err(said::STALE);
    }
    let conversation = fields
        .get("conversation")
        .and_then(serde_json::Value::as_str)
        .filter(|c| crate::conversations::is_conversation_id(c))
        .ok_or(said::NO_SUCH_CONVERSATION)?;
    let conversation = ProjectId::new(conversation);
    // A KNOWN field a program wrote wrongly is refused, never silently stripped. Unknown FIELDS
    // stay ignored — additive-proof — but a `lane` or an `in_reply_to_ask` that is present and
    // not a shape the hub will act on is the file SAYING something the hub cannot believe, and
    // sending it as though it had said nothing would be the hub rewriting what he wrote.
    match fields.get("t").and_then(serde_json::Value::as_str) {
        Some("choice") => {
            let ask_id = fields
                .get("ask_id")
                .and_then(serde_json::Value::as_str)
                .filter(|s| an_id_the_hub_writes_down(s))
                .ok_or(said::BAD_ASK)?;
            let option_id = fields
                .get("option_id")
                .and_then(serde_json::Value::as_str)
                .filter(|s| an_id_the_hub_writes_down(s))
                .ok_or(said::BAD_OPTION)?;
            let lane = the_lane(fields, said::BAD_LANE_ON_AN_ANSWER)?;
            Ok(Answer::Choice {
                conversation,
                lane,
                ask_id: AskId::new(ask_id),
                option_id: OptionId::new(option_id),
            })
        }
        Some("message") => {
            // Empty words are legal from the phone only because a file can ride with them; the
            // door carries no files, so words are the whole of what a message is.
            let text = fields
                .get("text")
                .and_then(serde_json::Value::as_str)
                .filter(|t| !t.trim().is_empty())
                .ok_or(said::NO_WORDS)?;
            let lane = the_lane(fields, said::BAD_LANE)?;
            let in_reply_to_ask = match fields.get("in_reply_to_ask") {
                None | Some(serde_json::Value::Null) => None,
                Some(value) => {
                    let s = value.as_str().ok_or(said::BAD_REPLY)?;
                    if !an_id_the_hub_writes_down(s) {
                        return Err(said::BAD_REPLY);
                    }
                    Some(AskId::new(s))
                }
            };
            // The sender's own receipt name, when it minted one. The same shape law as every
            // other opaque handle a file can carry: present-and-wrong is refused rather than
            // stripped, because the reader is waiting on exactly the name it wrote.
            let receipt = match fields.get("ref") {
                None | Some(serde_json::Value::Null) => None,
                Some(value) => {
                    let s = value.as_str().ok_or(said::BAD_RECEIPT)?;
                    if !an_id_the_hub_writes_down(s) {
                        return Err(said::BAD_RECEIPT);
                    }
                    Some(s.to_owned())
                }
            };
            Ok(Answer::Message {
                conversation,
                lane,
                text: text.to_owned(),
                in_reply_to_ask,
                receipt,
            })
        }
        _ => Err(said::NO_KIND),
    }
}

/// The result file's body for one consumed answer: what became of it, and nothing else.
pub(crate) fn the_result(of: &Result<(), &'static str>) -> String {
    match of {
        Ok(()) => serde_json::json!({ "t": "result", "status": "accepted" }).to_string(),
        Err(why) => {
            serde_json::json!({ "t": "result", "status": "refused", "why": why }).to_string()
        }
    }
}

/// Where a result file is: beside the answer it answers, wearing the suffix the hub keeps.
pub(crate) fn the_result_of(answer: &Path) -> PathBuf {
    let mut name = answer.file_name().unwrap_or_default().to_os_string();
    name.push(RESULT_SUFFIX);
    // Beside the answer, never in whatever directory this process happens to be running in: a
    // result the gateway cannot find is a result that was never written, and `PathBuf::from`
    // alone would drop the directory the answer was found in.
    answer.with_file_name(name)
}

/// Where a result's bytes are put down BEFORE they are put in place.
///
/// Two things this name has to be at once, and both are load-bearing.
///
/// It ends the way a result's name ends, because [`inventory`] sorts this directory by exactly
/// that one test: a name wearing the suffix is a receipt, and EVERYTHING ELSE is one of his acts.
/// A half-written receipt spelled any other way would be read by the very next sweep as an
/// answer, refused as garbage, consumed, and given a receipt of its own — the hub eating a file
/// nobody wrote. Wearing the suffix it is also collected by the spent-receipt sweep, so a
/// leftover from a rename that failed goes away in ten minutes instead of living for ever.
///
/// And it sits INSIDE the drop, not beside it: a rename is atomic only within one filesystem, and
/// the drop is the one directory here that a foreign program is handed by name — anywhere else is
/// a guess about what is mounted where.
pub(crate) fn where_a_result_is_staged(result: &Path) -> PathBuf {
    let mut name = result.file_name().unwrap_or_default().to_os_string();
    // Pushed onto the name, never `with_extension`: that replaces the LAST extension, which here
    // is the very suffix the sweep sorts by.
    name.push(format!(".staging.{}", std::process::id()));
    name.push(RESULT_SUFFIX);
    result.with_file_name(name)
}

/// What this sweep must look at: the answer files, in a deterministic order, and the result
/// files old enough to take away.
///
/// Answers are sorted by name — names are opaque, so name order is not arrival order, but it IS
/// an order two sweeps agree on, and any order within one sweep is arbitrary anyway: two answers
/// a person meant to order arrive in different sweeps. A result file that cannot be stat'd is
/// left rather than guessed about; the next sweep tries again, which costs nothing and destroys
/// nothing.
pub(crate) fn inventory(dir: &Path, now: u64) -> std::io::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut answers = Vec::new();
    let mut spent = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let is_a_result = name.to_string_lossy().ends_with(RESULT_SUFFIX);
        if is_a_result {
            let age = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if now.saturating_sub(age) > RESULTS_KEPT_FOR {
                spent.push(path);
            }
            continue;
        }
        answers.push(path);
    }
    answers.sort();
    Ok((answers, spent))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(ts: u64) -> String {
        serde_json::json!({
            "t": "choice", "conversation": "p-0123456789ab",
            "ask_id": "a1", "option_id": "y", "ts": ts,
        })
        .to_string()
    }

    #[test]
    fn a_stale_answer_is_refused_before_any_name_in_it_is_believed() {
        let now = 1_000_000;
        // A file whose every field is wrong, so the only thing that can decide the refusal is
        // the age: if staleness were judged after the shape, one of these errors would win and
        // the test would pass for the wrong reason.
        let stale = serde_json::json!({
            "t": "nonsense", "conversation": "not-a-conversation", "ts": now - STALE_AFTER - 1,
        })
        .to_string();
        assert_eq!(parse(&stale, now), Err(said::STALE));
        // Fresh, and every field right: the same clock arithmetic accepts it.
        assert!(parse(&choice(now), now).is_ok());
        // A ts in the future is the writer's own claim about itself, and the only use this hub
        // has for it is age: it reads as fresh, never as an error and never as truth.
        assert!(parse(&choice(now + 3_600), now).is_ok());
    }

    #[test]
    fn an_answer_without_its_own_clock_reading_is_refused() {
        let no_ts = serde_json::json!({
            "t": "message", "conversation": "p-0123456789ab", "text": "hello",
        })
        .to_string();
        assert_eq!(parse(&no_ts, 1_000_000), Err(said::NO_CLOCK));
        let not_a_number = serde_json::json!({
            "t": "message", "conversation": "p-0123456789ab", "text": "hello", "ts": "now",
        })
        .to_string();
        assert_eq!(parse(&not_a_number, 1_000_000), Err(said::NO_CLOCK));
    }

    #[test]
    fn an_id_the_door_will_not_write_down_is_refused() {
        let now = 1_000_000;
        for bad in [
            "",
            "a|b",
            "line\nbreak",
            "tab\there",
            &"x".repeat(CALLBACK_DATA_MAX),
        ] {
            let body = serde_json::json!({
                "t": "choice", "conversation": "p-0123456789ab",
                "ask_id": bad, "option_id": "y", "ts": now,
            })
            .to_string();
            assert_eq!(
                parse(&body, now),
                Err(said::BAD_ASK),
                "ask id {bad:?} slipped through"
            );
            let body = serde_json::json!({
                "t": "choice", "conversation": "p-0123456789ab",
                "ask_id": "a1", "option_id": bad, "ts": now,
            })
            .to_string();
            assert_eq!(
                parse(&body, now),
                Err(said::BAD_OPTION),
                "option id {bad:?} slipped through"
            );
        }
    }
    #[test]
    fn a_result_names_nothing_but_what_became_of_the_answer() {
        let accepted: serde_json::Value =
            serde_json::from_str(&the_result(&Ok(()))).expect("one object");
        assert_eq!(accepted["status"], "accepted");
        assert_eq!(
            accepted.as_object().expect("an object").len(),
            2,
            "a field beyond t and status reached the result: {accepted}"
        );
        let refused = the_result(&Err(said::NO_WORDS));
        let v: serde_json::Value = serde_json::from_str(&refused).expect("one object");
        assert_eq!(v["status"], "refused");
        assert_eq!(v["why"], said::NO_WORDS);
        assert_eq!(
            v.as_object().expect("an object").len(),
            3,
            "a field beyond t, status and why reached the result: {refused}"
        );
    }
}

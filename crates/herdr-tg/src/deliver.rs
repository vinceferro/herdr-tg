//! Putting the operator's words into a pane, and finding out whether they landed.
//!
//! # Why this module exists
//!
//! An `ok` from herdr means **herdr took the bytes**. It does not mean the agent received them,
//! rendered them, parsed them, or acted on them — a focused TUI dialog can swallow both the text and
//! the submit key with both RPCs reporting success. That was settled on the wire, not assumed:
//! `docs/SLICE-3-PROBE.md` P1.
//!
//! So the bridge must never report "delivered" on the strength of an ack. If it does, the operator
//! answers an agent from their phone, is told it worked, and waits on an agent that never saw the
//! reply. That is a worse failure than an obvious error, because it costs them the time they were
//! trying to save.
//!
//! # The confirmation ladder
//!
//! Four rungs, each strictly stronger than the last. The bridge reports the highest one it actually
//! observed, and names it:
//!
//! | rung | what was observed | what it proves |
//! |---|---|---|
//! | [`Rung::Accepted`] | herdr returned `ok` | herdr took the bytes. Nothing about the TUI. |
//! | [`Rung::Echoed`] | the text appeared in the pane | the TUI received and rendered it |
//! | [`Rung::Submitted`] | the pane changed after the submit key | the submit key did *something* |
//! | [`Rung::Acted`] | the agent left `blocked` | the agent took the answer and resumed |
//!
//! `Acted` is the only rung that means what the operator thinks "sent" means, and it is the reason
//! slice 3 subscribes to the pane's status before it writes rather than after. It cannot always be
//! reached — a pane that was never `blocked` has nothing to leave — so the ladder degrades honestly
//! instead of pretending.
//!
//! # The submit key is not a constant we get to be right about
//!
//! `Enter` is the default, and for `opencode` it is confirmed. For `claude` it is **operator-supplied
//! knowledge, not a probed fact** (`docs/SLICE-3-PROBE.md`, "still open"). Rather than bet the
//! product on a constant, the read-back is what settles it at runtime: if the text is still sitting
//! in the pane after the submit key, the bridge says so instead of claiming success.
//!
//! # Why text goes through `send_input`
//!
//! `pane.send_text` writes RAW bytes, so a `\n` in a multi-line reply from a phone is a real Enter
//! that executes a line in the operator's shell. `pane.send_input` does not execute lines — probed,
//! `docs/SLICE-3-PROBE.md` P3. Operator-authored text therefore **always** goes through
//! `send_input`, and the submit key is a separate, deliberate step.

use std::time::Duration;

use herdr_client::{HerdrError, Key, PaneId};

/// How strongly delivery was actually observed. Ordered: higher is stronger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rung {
    /// herdr took the bytes. Says nothing about the TUI.
    Accepted,
    /// The text appeared in the pane — the TUI received and rendered it.
    Echoed,
    /// The pane changed after the submit key.
    Submitted,
    /// The agent left `blocked`. The only rung that means the agent acted.
    ///
    /// Only the push loop can observe this: it requires a subscription that was open BEFORE the
    /// write, so the `blocked -> working` edge is seen. It is constructed there, not here — the
    /// reply path alone cannot reach this rung honestly, and reporting it from a poll would be
    /// exactly the overstatement this ladder exists to prevent.
    #[allow(
        dead_code,
        reason = "constructed by the push loop, which lands with the notifier"
    )]
    Acted,
}

/// What happened when the bridge tried to put text into a pane.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub pane: PaneId,
    pub rung: Rung,
    /// Human-readable account of what was and was not observed. Goes to the audit log verbatim.
    pub detail: String,
}

/// What became of the operator's words.
///
/// Two outcomes, not one, and the second is the whole point.
///
/// The moment the text write goes out, the bridge is past a point of no return: a timeout is the
/// shape of a herd that took the bytes and never answered, so a failure from there on is not
/// evidence that the terminal was left alone. Before that line an error means the pane was never
/// touched, and saying "nothing was sent" is the truth. After it, the bridge does not know that
/// nothing happened — it knows it cannot see what happened, and those are different sentences.
///
/// This is an enum rather than a field so that the difference cannot be dropped on the floor: a
/// caller has to name both cases before it can say anything to the operator.
#[derive(Debug, Clone)]
pub enum Delivered {
    /// Every read the bridge needed came back. [`Delivery`] says how far up the ladder it got.
    Watched(Delivery),
    /// Contact was lost after the operator's words had already left the bridge, so what they did
    /// is simply unknown.
    LostSight {
        pane: PaneId,
        /// The last thing that left the bridge before contact went.
        reached: Reached,
        /// Human-readable account of what was and was not observed. Goes to the audit log verbatim.
        detail: String,
    },
}

/// How far the writes had got when contact was lost.
///
/// Two, not four, though there are four moments this can happen at. A write whose RPC never
/// answered and a write that was acked and then never checked need the SAME thing from the
/// operator — a look at that terminal before they send the message again — and the bridge must not
/// take the weaker reading of its own failure. So "the words may have gone out" and "the words did
/// go out" are one case, named for what the operator has to assume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reached {
    /// The words went out. The submit key did not, so they may be sitting in the input box unsent.
    TheWords,
    /// The words went out and the submit key after them. The agent may already have the message.
    TheWordsAndTheSubmitKey,
}

/// The pane operations delivery needs.
///
/// A trait rather than a concrete client so the verification logic — which is the part that must
/// not be wrong — is testable without a socket, including the paths that only occur when a TUI
/// swallows input. Those are precisely the cases a live test cannot stage on demand.
#[allow(async_fn_in_trait)]
pub trait PaneIo {
    async fn read_visible(&self, pane: &PaneId) -> Result<String, HerdrError>;
    async fn send_input_text(&self, pane: &PaneId, text: &str) -> Result<(), HerdrError>;
    async fn send_submit_key(&self, pane: &PaneId, key: &Key) -> Result<(), HerdrError>;
    /// The visible screen WITH colour, so a dialog's selection can be seen.
    async fn read_visible_ansi(&self, pane: &PaneId) -> Result<String, HerdrError>;
    /// Send several keys in order. Used only to drive a choice dialog.
    async fn send_key_sequence(&self, pane: &PaneId, keys: &[Key]) -> Result<(), HerdrError>;
}

/// The real client. Deliberately in THIS file: `tests/no_live_write_call_site.rs` permits the write
/// methods to be named in exactly one file outside the client crate, and that exemption is what
/// keeps every write on the verified, audited path. Putting this impl anywhere else would either
/// fail the guard or force the exemption wider.
impl PaneIo for herdr_client::HerdrClient {
    async fn read_visible(&self, pane: &PaneId) -> Result<String, HerdrError> {
        Ok(self.read_visible(pane).await?.text)
    }

    async fn send_input_text(&self, pane: &PaneId, text: &str) -> Result<(), HerdrError> {
        // send_input, never send_text: send_text writes RAW bytes, so a newline in a multi-line
        // reply from a phone is a real Enter that executes a line in the operator's shell.
        // Probed, docs/SLICE-3-PROBE.md P3.
        // WriteAccepted is #[must_use] on purpose: it exists so no caller can quietly treat an
        // ack as delivery. Bound and named here, and deliberately NOT returned — the rung this
        // module reports is decided by reading the pane back, never by the ack.
        let _herdr_took_the_bytes = self.send_input(pane, Some(text), &[]).await?;
        Ok(())
    }

    async fn send_submit_key(&self, pane: &PaneId, key: &Key) -> Result<(), HerdrError> {
        let _herdr_took_the_bytes = self.send_keys(pane, std::slice::from_ref(key)).await?;
        Ok(())
    }

    async fn read_visible_ansi(&self, pane: &PaneId) -> Result<String, HerdrError> {
        Ok(self.read_visible_ansi(pane).await?.text)
    }

    async fn send_key_sequence(&self, pane: &PaneId, keys: &[Key]) -> Result<(), HerdrError> {
        let _herdr_took_the_bytes = self.send_keys(pane, keys).await?;
        Ok(())
    }
}

/// How long to wait, and how often to look, for the pane to change after the submit key.
#[derive(Debug, Clone, Copy)]
pub struct Settle {
    pub attempts: u8,
    pub interval: Duration,
}

impl Default for Settle {
    fn default() -> Self {
        // A TUI redraws in tens of milliseconds; six looks over ~900ms is generous without making
        // the operator wait on their phone for a confirmation.
        Self {
            attempts: 6,
            interval: Duration::from_millis(150),
        }
    }
}

/// Put `text` into `pane`, press `submit`, and report how far up the ladder we actually got.
///
/// Never returns a rung it did not observe.
///
/// # What an `Err` from here means
///
/// Exactly one thing: **nothing reached the pane**. The caller renders it as "I can't reach the
/// herd right now, so nothing was sent", so that has to be a fact, not a guess. Only the look taken
/// before the first write can produce it.
///
/// Everything after that write is past the point of no return, and a failure there is reported as
/// [`Delivered::LostSight`] — the words went out, and what they did could not be seen. Reporting
/// those as errors had the operator told nothing was sent about a message that was already typed
/// and submitted; they sent it again, and the agent got it twice.
pub async fn deliver<P: PaneIo>(
    io: &P,
    pane: &PaneId,
    text: &str,
    submit: &Key,
    settle: Settle,
    sleep: impl Fn(Duration) -> futures_core::future::BoxFuture<'static, ()>,
) -> Result<Delivered, HerdrError> {
    // The last read whose failure honestly means "nothing was sent". Nothing has been written yet.
    let before = io.read_visible(pane).await?;

    // THE POINT OF NO RETURN. Below this line no `?` may stand: a timeout is the shape of a herd
    // that took the write and never answered, so from here the bridge can say what it could not
    // see, and never that the terminal was left alone.
    let lost = |reached: Reached, detail: String| Delivered::LostSight {
        pane: pane.clone(),
        reached,
        detail,
    };

    if let Err(e) = io.send_input_text(pane, text).await {
        tracing::warn!(error = %e, "lost contact with the herd as the operator's words went out");
        return Ok(lost(
            Reached::TheWords,
            format!(
                "lost contact as the words went out — I could not check whether they arrived, \
                 and I did not press {submit}: {e}"
            ),
        ));
    }
    let mut rung = Rung::Accepted;
    let mut detail = String::from("herdr accepted the text");

    // Rung 2: did the TUI actually render it? A distinctive slice of the operator's own text is the
    // probe — the whole message may be wrapped, indented, or truncated by the TUI.
    let after_text = match io.read_visible(pane).await {
        Ok(s) => s,
        // The words are in that terminal. What is unknown is whether they rendered — and the
        // submit key is deliberately NOT sent on a screen nobody can see, because a modal that
        // took focus would swallow the text and let the key press whatever is highlighted.
        Err(e) => {
            tracing::warn!(error = %e, "lost sight of the pane after the operator's words went in");
            return Ok(lost(
                Reached::TheWords,
                format!(
                    "the words went in, then I lost sight of that session before I could press \
                     {submit}: {e}"
                ),
            ));
        }
    };
    let needle = echo_needle(text);
    let echoed = needle
        .as_deref()
        .is_some_and(|n| after_text.contains(n) && !before.contains(n));
    if echoed {
        rung = Rung::Echoed;
        detail = String::from("the text appeared in the pane");
    } else {
        detail = format!(
            "{detail}, but it did not appear in the pane — the TUI may have swallowed it, or the \
             pane may render input somewhere this read cannot see"
        );
    }

    if let Err(e) = io.send_submit_key(pane, submit).await {
        tracing::warn!(error = %e, "lost contact with the herd as the submit key went out");
        return Ok(lost(
            Reached::TheWordsAndTheSubmitKey,
            format!("the words went in and I lost contact as {submit} went out — {detail}: {e}"),
        ));
    }

    // Rung 3: did the submit key change anything? Compared against the post-text read, so the
    // change attributable to the submit key is isolated from the change caused by the text.
    for _ in 0..settle.attempts {
        sleep(settle.interval).await;
        let now = match io.read_visible(pane).await {
            Ok(now) => now,
            // The words and the submit key have both landed. The agent may well have the message
            // already, so the one thing that must not be said is that nothing was sent.
            Err(e) => {
                tracing::warn!(error = %e, "lost sight of the pane after the submit key landed");
                return Ok(lost(
                    Reached::TheWordsAndTheSubmitKey,
                    format!(
                        "the words went in and {submit} went out, then I lost sight of that \
                         session before I could see what it did: {e}"
                    ),
                ));
            }
        };
        if now != after_text {
            let cleared = needle.as_deref().is_none_or(|n| !tail_contains(&now, n));
            rung = Rung::Submitted;
            detail = if cleared {
                format!("the pane changed after {submit} and the text left the input area")
            } else {
                format!(
                    "the pane changed after {submit}, but the text still appears near the bottom — \
                     it may not have submitted"
                )
            };
            break;
        }
    }

    if rung < Rung::Submitted {
        detail = format!(
            "{detail}; the pane did not change within {}ms of {submit}. If this repeats, the \
             submit key for this harness is probably not {submit}",
            settle.attempts as u64 * settle.interval.as_millis() as u64,
        );
    }

    Ok(Delivered::Watched(Delivery {
        pane: pane.clone(),
        rung,
        detail,
    }))
}

/// A distinctive slice of the operator's text to look for in the pane.
///
/// The longest line, trimmed, capped. Not the whole message: a TUI wraps, indents and truncates, so
/// matching the full text would report "not echoed" for text that plainly arrived. Not a short
/// fragment either — `"y"` would match anything already on screen, which is the false positive that
/// would make this whole module lie in the operator's favour.
fn echo_needle(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .max_by_key(|l| l.chars().count())?;
    if line.chars().count() < 4 {
        // Too short to be distinctive. Better to report "not echoed" than to match noise.
        return None;
    }
    Some(line.chars().take(40).collect())
}

/// Is `needle` in the last few lines — i.e. still sitting where input sits?
///
/// After a real submit the text usually moves up into the transcript, so its mere presence proves
/// nothing. Its presence *at the bottom* is what a wrong submit key looks like.
fn tail_contains(pane: &str, needle: &str) -> bool {
    let lines: Vec<&str> = pane.lines().collect();
    let start = lines.len().saturating_sub(4);
    lines[start..].iter().any(|l| l.contains(needle))
}

/// What the operator asked for, and where the ask came from.
///
/// The two arrive with different amounts of evidence, and the difference matters. Words were typed
/// while the operator was looking at a push, so they are resolved against whatever dialog is on
/// screen now. A button was DRAWN against a particular list of options, so it is only meaningful
/// while that list is still the one showing.
pub enum Choice<'a> {
    /// The operator's own words: a 1-based number, or a prefix of exactly one option.
    Reply(&'a str),
    /// A button tap: the exact label the button displayed, and the whole list it was drawn from.
    /// Both must still match the dialog on screen, or nothing is pressed.
    Button {
        label: &'a str,
        drawn_from: &'a [String],
    },
}

/// Why nothing was chosen. No option was confirmed in any of these — but see
/// [`ChoiceRefused::NotConfirmed`], which is the one that can follow keys that DID reach the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceRefused {
    /// The pane is not showing a menu any more.
    NotADialog,
    /// It IS still showing something menu-shaped, but which option is highlighted cannot be read.
    ///
    /// Kept apart from [`ChoiceRefused::NotADialog`] because the two need opposite advice: a menu
    /// that has gone is one to look at again, and a menu nobody can read is one to answer at the
    /// keyboard. Telling the operator it stopped asking would be a plain untruth.
    Unreadable,
    /// The reply named no option, or more than one. Carries the options as they are now, so the
    /// operator can be shown what they can actually pick.
    Unclear { options: Vec<String> },
    /// It is showing a menu, but not the one those buttons were drawn for.
    Changed { now: Vec<String> },
    /// The menu would not hold still, would not move, or ended up on the wrong option, so the
    /// confirm key never went out. Carries an operator-readable account of what was seen.
    ///
    /// `keys_sent` says whether ARROW keys had already reached that terminal before the bridge
    /// stopped. Nothing was confirmed either way — but when arrows went out, the highlight in the
    /// operator's own terminal has moved, and telling them nothing was typed there is false.
    NotConfirmed { why: String, keys_sent: bool },
}

impl std::fmt::Display for ChoiceRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChoiceRefused::NotADialog => {
                f.write_str("That session isn't showing a menu any more, so I pressed nothing.")
            }
            ChoiceRefused::Unreadable => f.write_str(
                "I can't tell which option that menu is on, so I pressed nothing. Answer it at \
                 the keyboard.",
            ),
            ChoiceRefused::Unclear { .. } => {
                f.write_str("I didn't catch which one you meant, so I pressed nothing.")
            }
            ChoiceRefused::Changed { .. } => f.write_str(
                "That menu has changed since those buttons were drawn, so I pressed nothing.",
            ),
            ChoiceRefused::NotConfirmed { why, .. } => f.write_str(why),
        }
    }
}

/// What the pane showed after the confirm key went out.
///
/// [`Afterwards::NotSeen`] is the honest third answer. The read that would have checked whether the
/// menu closed can itself fail — a herd restart, a dropped socket — and that failure says nothing
/// about the key, which has already landed. Reporting it as an error had the operator told "nothing
/// was sent" about a permission that had just been granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Afterwards {
    /// Ordinary output is on screen: the menu took the answer and closed.
    MenuClosed,
    /// The SAME menu is still up. It may not have registered.
    MenuStillUp,
    /// That question is gone and the session is asking a different one.
    ///
    /// Kept apart from [`Afterwards::MenuStillUp`] because an agent that touches two files in a
    /// row answers one prompt and draws the next in the same breath. Calling that "the prompt is
    /// still up — it may not have registered" is false about an answer that landed, and the
    /// operator's natural response to it is to answer again — into a question they never read.
    AnotherQuestion,
    /// The pane could not be looked at again at all, so what the key did is simply unknown.
    NotSeen,
}

/// A choice that was actually made.
#[derive(Debug, Clone)]
pub struct Chosen {
    pub delivery: Delivery,
    /// What a look after the confirm key found — including "I could not look".
    pub afterwards: Afterwards,
    /// The label a SETTLED read showed highlighted immediately before the confirm key went out.
    ///
    /// This — never the index a button carried, never a label from an earlier read — is the only
    /// label the operator may be told. Everything else is a guess about a screen someone else is
    /// also typing at.
    pub option: String,
}

/// What a look at the menu found.
enum Look {
    /// Two consecutive reads parsed to the same menu: this is it, standing still.
    Settled(crate::permission::Prompt),
    /// It kept changing for the whole budget — someone is driving it.
    Moving,
    /// Ordinary output: no menu at all.
    Gone,
    /// Something menu-shaped is there, but its highlight cannot be resolved.
    Unreadable,
}

/// Look at the menu until it holds still.
///
/// One read is not a witness. The pane redraws tens of milliseconds after the key that changed it,
/// so a single read can render the frame from BEFORE the operator's own keypress — and the bridge
/// would then move the highlight by the wrong number of steps. Two agreeing reads, an interval
/// apart, are evidence; one read is a guess.
///
/// Compares the PARSED menu, not the raw screen, so a spinner or a clock elsewhere in the pane
/// cannot make a stationary menu look like a moving one.
async fn settled_dialog<P: PaneIo, S>(
    io: &P,
    pane: &PaneId,
    settle: Settle,
    sleep: &S,
) -> Result<Look, HerdrError>
where
    S: Fn(Duration) -> futures_core::future::BoxFuture<'static, ()>,
{
    // `classify`, never `parse`: `parse` collapses "ordinary output" and "a menu I cannot read"
    // into one answer, and those two need opposite things said to the operator.
    fn look(ansi: &str) -> Result<crate::permission::Prompt, Look> {
        match crate::permission::classify(ansi) {
            crate::permission::Screen::Dialog(p) => Ok(p),
            crate::permission::Screen::UnreadableControl => Err(Look::Unreadable),
            crate::permission::Screen::Prose => Err(Look::Gone),
        }
    }

    let mut prev = match look(&io.read_visible_ansi(pane).await?) {
        Ok(p) => p,
        Err(l) => return Ok(l),
    };
    for _ in 0..settle.attempts {
        sleep(settle.interval).await;
        let now = match look(&io.read_visible_ansi(pane).await?) {
            Ok(p) => p,
            Err(l) => return Ok(l),
        };
        if now == prev {
            return Ok(Look::Settled(now));
        }
        prev = now;
    }
    Ok(Look::Moving)
}

/// Turn key names into keys, for names this crate wrote itself.
fn to_keys(names: &[&str]) -> Vec<Key> {
    names
        .iter()
        .map(|k| Key::parse(k).expect("this module only emits keys the probe confirmed"))
        .collect()
}

/// Answer a menu by moving the highlight and then confirming it — never with text.
///
/// # Why this is four steps and not one
///
/// The operator is at their laptop with their phone in their hand: their own keyboard is answering
/// the same menu this function is. On top of that the screen redraws tens of milliseconds after the
/// key that changed it, so a read taken just after their keypress shows the frame from before it.
/// A single read is therefore not a reliable witness even on a quiet machine.
///
/// So: the menu is found, waited on until two reads agree, moved, LOOKED AT AGAIN until it agrees
/// again, and only then confirmed. Nothing is confirmed unless that last settled read showed the
/// same options with the wanted one highlighted. If it did not, nothing is pressed and the operator
/// is told what was actually on screen.
///
/// The moves never run past an end of the row, so none of this depends on what the harness does
/// with an arrow key at an edge — which nobody has probed.
///
/// One window remains, between the last read and the confirm key, and it cannot be closed from
/// outside herdr. What CAN be closed is the bridge claiming an option it never saw highlighted:
/// [`Chosen::option`] is that label, and it is the only one the operator is ever told.
///
/// # What an `Err` from here means
///
/// The same one thing it means from [`deliver`]: **no key reached that terminal**. Only the look
/// taken before the first key can produce it, because the caller renders it as "nothing was sent".
/// Every failure after a key has gone out comes back as a [`Chosen`] or a
/// [`ChoiceRefused::NotConfirmed`] that says so.
pub async fn choose<P: PaneIo>(
    io: &P,
    pane: &PaneId,
    choice: Choice<'_>,
    settle: Settle,
    sleep: impl Fn(Duration) -> futures_core::future::BoxFuture<'static, ()>,
) -> Result<Result<Chosen, ChoiceRefused>, HerdrError> {
    let start = match settled_dialog(io, pane, settle, &sleep).await? {
        Look::Settled(p) => p,
        Look::Gone => return Ok(Err(ChoiceRefused::NotADialog)),
        Look::Unreadable => return Ok(Err(ChoiceRefused::Unreadable)),
        Look::Moving => {
            return Ok(Err(ChoiceRefused::NotConfirmed {
                why: "That menu is moving — someone is answering it at the keyboard. I pressed \
                      nothing."
                    .into(),
                keys_sent: false,
            }));
        }
    };

    let idx = match choice {
        Choice::Reply(words) => match start.match_option(words) {
            Some(i) => i,
            None => {
                return Ok(Err(ChoiceRefused::Unclear {
                    options: start.options.clone(),
                }));
            }
        },
        // A position means nothing across a redraw, and neither does a label on a different menu.
        // The buttons were drawn against one list; if that list has changed at all, this tap is
        // about a question that is no longer being asked.
        Choice::Button { label, drawn_from } => {
            if start.options != drawn_from {
                return Ok(Err(ChoiceRefused::Changed {
                    now: start.options.clone(),
                }));
            }
            match start.exact_option(label) {
                Some(i) => i,
                None => {
                    return Ok(Err(ChoiceRefused::Unclear {
                        options: start.options.clone(),
                    }));
                }
            }
        }
    };

    // From here on the LABEL is what is aimed at. An index is a fact about one read; a label is
    // what the operator asked for.
    let chosen = start.options[idx].clone();

    let aimed = if start.selected == idx {
        start.clone()
    } else {
        // Unreachable as written — `idx` came from this same list. It is a refusal rather than an
        // unwrap so that an edit which breaks that invariant costs the operator a trip to their
        // keyboard instead of pressing a key nobody chose.
        let Some(keys) = start.move_to(idx) else {
            return Ok(Err(ChoiceRefused::NotConfirmed {
                why: "I lost track of which option you meant, so I pressed nothing.".into(),
                keys_sent: false,
            }));
        };
        // A failure on this send is not proof the arrows did not land — a timeout is the shape of
        // a herd that took the write and never answered — so from here on the operator is told
        // that keys went out, not that the terminal was left alone.
        if let Err(e) = io.send_key_sequence(pane, &to_keys(&keys)).await {
            tracing::warn!(error = %e, "lost contact with the herd as the arrow keys went out");
            return Ok(Err(ChoiceRefused::NotConfirmed {
                why: format!(
                    "I lost contact with that session while aiming at \"{chosen}\", so I did not \
                     press the confirm key."
                ),
                keys_sent: true,
            }));
        }
        // From here the arrows have reached a real terminal, so nothing below may tell the operator
        // that nothing was typed into it. The confirm key is still the thing that grants a
        // permission, and it has not gone out — that part stays true in every branch.
        //
        // One aim, then look. No corrective loop: if the highlight did not land where it was
        // asked to, someone else is driving, and pressing more keys at them is how the wrong
        // permission gets granted.
        let looked = match settled_dialog(io, pane, settle, &sleep).await {
            Ok(l) => l,
            // The look failed, not the keys. An error here would be reported to the operator as
            // "nothing was sent", about a terminal whose highlight this function just moved.
            Err(e) => {
                tracing::warn!(error = %e, "lost sight of the menu after moving the highlight");
                return Ok(Err(ChoiceRefused::NotConfirmed {
                    why: format!(
                        "I moved the highlight toward \"{chosen}\" and then lost contact with that \
                         session, so I did not press the confirm key."
                    ),
                    keys_sent: true,
                }));
            }
        };
        match looked {
            Look::Settled(p) => p,
            Look::Gone => {
                return Ok(Err(ChoiceRefused::NotConfirmed {
                    why: "That menu went away while I was answering it, so I confirmed nothing."
                        .into(),
                    keys_sent: true,
                }));
            }
            Look::Unreadable => {
                return Ok(Err(ChoiceRefused::NotConfirmed {
                    why: "I lost track of which option that menu is on, so I confirmed nothing; \
                          answer it at the keyboard."
                        .into(),
                    keys_sent: true,
                }));
            }
            Look::Moving => {
                return Ok(Err(ChoiceRefused::NotConfirmed {
                    why: "That menu kept moving while I was answering it. I confirmed nothing."
                        .into(),
                    keys_sent: true,
                }));
            }
        }
    };
    // True exactly when arrow keys went into that terminal above.
    let moved = start.selected != idx;

    // THE GATE. Everything above is preparation; this is the line that makes the confirmation
    // honest. Nothing is pressed unless a settled read shows the SAME options with the wanted one
    // highlighted. Checking the label alone is not enough — a different menu can offer the same
    // word, and confirming it would answer a question nobody read.
    if aimed.options != start.options {
        return Ok(Err(ChoiceRefused::NotConfirmed {
            why: format!(
                "The choices on that menu changed while I was answering it, so I did not confirm \
                 \"{chosen}\"."
            ),
            keys_sent: moved,
        }));
    }
    if aimed.highlighted() != Some(chosen.as_str()) {
        return Ok(Err(ChoiceRefused::NotConfirmed {
            why: format!(
                "I couldn't get the highlight onto \"{chosen}\" — it is on \"{}\". Nothing was \
                 confirmed; answer it at the keyboard.",
                aimed.highlighted().unwrap_or("something else")
            ),
            keys_sent: moved,
        }));
    }

    // Its own send, which is what makes the check above a gate rather than a comment.
    //
    // A failure here is NOT evidence that the key did not land: a timeout is the shape of a herd
    // that took the write and never answered. So it is reported as "I don't know", never as
    // "nothing was sent" — the operator has to look at that terminal either way, and only one of
    // those two sends them to it.
    let sent = io
        .send_key_sequence(pane, &to_keys(&[crate::permission::CONFIRM]))
        .await;
    if let Err(e) = sent {
        tracing::warn!(error = %e, "lost contact with the herd as the confirm key went out");
        return Ok(Ok(Chosen {
            delivery: Delivery {
                pane: pane.clone(),
                rung: Rung::Accepted,
                detail: format!(
                    "asked that session to confirm \"{chosen}\" and lost contact as the key went \
                     out — I could not check whether it took"
                ),
            },
            afterwards: Afterwards::NotSeen,
            option: chosen,
        }));
    }

    // The key has landed. Nothing from here on may return an error: the caller turns an error into
    // "I can't reach the herd right now, so nothing was sent", and that would be said about a
    // permission that has just been granted. What CAN still fail is looking, and a failure to look
    // is reported as exactly that.
    let mut rung = Rung::Accepted;
    let mut afterwards = Afterwards::MenuStillUp;
    let mut detail = format!("moved the highlight onto \"{chosen}\" and confirmed it");
    for _ in 0..settle.attempts {
        sleep(settle.interval).await;
        let now = match io.read_visible_ansi(pane).await {
            Ok(now) => now,
            Err(e) => {
                tracing::warn!(error = %e, "lost sight of the pane after the confirm key landed");
                afterwards = Afterwards::NotSeen;
                break;
            }
        };
        // Ordinary output is the ONLY evidence that the menu closed. "Nothing parsed" is not: a
        // menu the bridge merely failed to understand reads the same way, and answering it is
        // exactly what did not happen.
        match crate::permission::classify(&now) {
            crate::permission::Screen::Prose => {
                rung = Rung::Submitted;
                afterwards = Afterwards::MenuClosed;
                detail = format!("chose \"{chosen}\" — the menu closed");
                break;
            }
            // A menu offering DIFFERENT options is not the one that was answered. The question
            // this function confirmed is gone from the screen, which is the same evidence a blank
            // screen would have given — an agent that touches two files in a row asks the next
            // question in the same breath. Saying "the prompt is still up" here is false about an
            // answer that landed.
            //
            // The same options are NOT enough: a question redrawn unchanged and a question never
            // answered look identical, and the honest reading of that is the doubtful one.
            crate::permission::Screen::Dialog(p) if p.options != aimed.options => {
                rung = Rung::Submitted;
                afterwards = Afterwards::AnotherQuestion;
                detail = format!(
                    "chose \"{chosen}\" — that question closed and the session is asking another"
                );
                break;
            }
            _ => {}
        }
    }
    detail = match afterwards {
        Afterwards::MenuClosed => detail,
        Afterwards::MenuStillUp => {
            format!("{detail}, but the menu is still on screen — it may not have taken")
        }
        Afterwards::AnotherQuestion => detail,
        // Says only what is known: the key went out, and the pane could not be looked at again.
        Afterwards::NotSeen => format!(
            "{detail}, and then lost contact with that session before I could see what it did"
        ),
    };

    Ok(Ok(Chosen {
        delivery: Delivery {
            pane: pane.clone(),
            rung,
            detail,
        },
        afterwards,
        option: chosen,
    }))
}

/// A menu on a screen, and a record of every key that reached it, for the tests of modules that
/// answer one.
///
/// It lives in THIS file because a fake pane has to name the write methods, and
/// `no_live_write_call_site` permits that in exactly one file outside the client crate — which is
/// the rule that keeps every real write on the read-back-and-audit path. A fake in another module's
/// test would either fail that guard or force the exemption wider.
#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;

    use herdr_client::{HerdrError, Key, PaneId};

    use super::PaneIo;

    pub(crate) struct MenuPane {
        options: Vec<String>,
        selected: RefCell<usize>,
        keys: RefCell<Vec<String>>,
        confirmed: RefCell<Option<String>>,
    }

    impl MenuPane {
        /// A menu with the first option highlighted, as a freshly drawn one is.
        pub(crate) fn showing(options: &[&str]) -> Self {
            Self {
                options: options.iter().map(|o| o.to_string()).collect(),
                selected: RefCell::new(0),
                keys: RefCell::new(Vec::new()),
                confirmed: RefCell::new(None),
            }
        }

        /// Every key that reached the pane, in order.
        pub(crate) fn keys(&self) -> Vec<String> {
            self.keys.borrow().clone()
        }

        /// The option the confirm key actually landed on — the ground truth a test compares what
        /// the operator was told against.
        pub(crate) fn confirmed(&self) -> Option<String> {
            self.confirmed.borrow().clone()
        }
    }

    impl PaneIo for MenuPane {
        async fn read_visible(&self, _pane: &PaneId) -> Result<String, HerdrError> {
            panic!("a menu must be read in colour, or which option is highlighted is invisible")
        }
        async fn send_input_text(&self, _pane: &PaneId, _text: &str) -> Result<(), HerdrError> {
            panic!(
                "a menu is never answered with words: the confirm key after them presses \
                    whatever happens to be highlighted"
            )
        }
        async fn send_submit_key(&self, _pane: &PaneId, _key: &Key) -> Result<(), HerdrError> {
            panic!("the menu path sends its keys as a sequence, so all of them are recorded")
        }
        async fn read_visible_ansi(&self, _pane: &PaneId) -> Result<String, HerdrError> {
            let refs: Vec<&str> = self.options.iter().map(String::as_str).collect();
            Ok(crate::permission::synthetic::render_dialog(
                &refs,
                *self.selected.borrow(),
            ))
        }
        async fn send_key_sequence(&self, _pane: &PaneId, keys: &[Key]) -> Result<(), HerdrError> {
            for k in keys {
                let name = k.to_string();
                match name.as_str() {
                    // Clamps at the ends, like the captured harness does.
                    "Right" => {
                        let at = *self.selected.borrow();
                        *self.selected.borrow_mut() = (at + 1).min(self.options.len() - 1);
                    }
                    "Left" => {
                        let at = *self.selected.borrow();
                        *self.selected.borrow_mut() = at.saturating_sub(1);
                    }
                    "Enter" => {
                        *self.confirmed.borrow_mut() =
                            Some(self.options[*self.selected.borrow()].clone());
                    }
                    _ => {}
                }
                self.keys.borrow_mut().push(name);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A scripted pane. Each `read_visible` returns the next screen, so a test can stage exactly
    /// the sequence a real TUI would produce — including the ones a live test cannot force.
    struct FakePane {
        screens: RefCell<Vec<String>>,
        last: RefCell<String>,
        sent_text: RefCell<Vec<String>>,
        sent_keys: RefCell<Vec<String>>,
    }

    impl FakePane {
        fn new(screens: &[&str]) -> Self {
            Self {
                screens: RefCell::new(screens.iter().rev().map(|s| s.to_string()).collect()),
                last: RefCell::new(String::new()),
                sent_text: RefCell::new(Vec::new()),
                sent_keys: RefCell::new(Vec::new()),
            }
        }
    }

    impl PaneIo for FakePane {
        async fn read_visible(&self, _pane: &PaneId) -> Result<String, HerdrError> {
            let next = self.screens.borrow_mut().pop();
            match next {
                Some(s) => {
                    *self.last.borrow_mut() = s.clone();
                    Ok(s)
                }
                // Ran out of scripted screens: the pane has stopped changing.
                None => Ok(self.last.borrow().clone()),
            }
        }
        async fn send_input_text(&self, _pane: &PaneId, text: &str) -> Result<(), HerdrError> {
            self.sent_text.borrow_mut().push(text.to_string());
            Ok(())
        }
        async fn send_submit_key(&self, _pane: &PaneId, key: &Key) -> Result<(), HerdrError> {
            self.sent_keys.borrow_mut().push(key.to_string());
            Ok(())
        }
        async fn read_visible_ansi(&self, pane: &PaneId) -> Result<String, HerdrError> {
            self.read_visible(pane).await
        }
        async fn send_key_sequence(&self, _pane: &PaneId, keys: &[Key]) -> Result<(), HerdrError> {
            for k in keys {
                self.sent_keys.borrow_mut().push(k.to_string());
            }
            Ok(())
        }
    }

    fn no_sleep(_: Duration) -> futures_core::future::BoxFuture<'static, ()> {
        Box::pin(async {})
    }

    fn pane() -> PaneId {
        PaneId::new("w1:p1")
    }

    fn key() -> Key {
        Key::parse("Enter").expect("Enter is a valid key")
    }

    fn settle() -> Settle {
        Settle {
            attempts: 3,
            interval: Duration::from_millis(0),
        }
    }

    /// The delivery the bridge actually watched from beginning to end.
    ///
    /// A `LostSight` here would mean the fake pane stopped answering, which none of the tests that
    /// use this stage — so it is a panic rather than a silent second path through the assertions.
    fn watched(d: Delivered) -> Delivery {
        match d {
            Delivered::Watched(d) => d,
            Delivered::LostSight { detail, .. } => {
                panic!(
                    "this pane never goes quiet, so nothing should have lost sight of it: {detail}"
                )
            }
        }
    }

    #[tokio::test]
    async fn a_clean_submit_reaches_the_submitted_rung() {
        let io = FakePane::new(&[
            "> ",                       // before
            "> ship it please",         // after the text
            "agent: working on it\n> ", // after the submit key
        ]);
        let d = watched(
            deliver(&io, &pane(), "ship it please", &key(), settle(), no_sleep)
                .await
                .unwrap(),
        );
        assert_eq!(d.rung, Rung::Submitted);
        assert_eq!(io.sent_text.borrow().as_slice(), ["ship it please"]);
        assert_eq!(io.sent_keys.borrow().as_slice(), ["Enter"]);
    }

    /// THE failure this module exists to catch: a wrong submit key.
    ///
    /// The text lands and renders, the submit key is accepted by herdr, and nothing happens. The
    /// old behaviour would report "sent" and leave the operator waiting on an agent that never saw
    /// their answer.
    #[tokio::test]
    async fn a_wrong_submit_key_is_reported_not_papered_over() {
        let io = FakePane::new(&[
            "> ",               // before
            "> ship it please", // after the text — and it never changes again
        ]);
        let d = watched(
            deliver(&io, &pane(), "ship it please", &key(), settle(), no_sleep)
                .await
                .unwrap(),
        );
        assert_eq!(d.rung, Rung::Echoed, "must not claim Submitted");
        assert!(
            d.detail.contains("submit key"),
            "the operator must be told what to suspect: {}",
            d.detail
        );
        // The operator-facing wording is `voice`'s, and it has its own test that only the top
        // rung sounds certain. Here we only pin the rung, which is what that wording reads.
    }

    /// A TUI that swallows the text entirely — a modal dialog had focus.
    #[tokio::test]
    async fn text_that_never_renders_stays_at_accepted() {
        let io = FakePane::new(&["> ", "> "]);
        let d = watched(
            deliver(&io, &pane(), "ship it please", &key(), settle(), no_sleep)
                .await
                .unwrap(),
        );
        assert_eq!(d.rung, Rung::Accepted);
    }

    /// The text is still at the BOTTOM after the submit key: the pane changed for some other
    /// reason (a spinner, a clock) while the reply sat unsent.
    #[tokio::test]
    async fn text_still_in_the_input_area_is_flagged_even_though_the_pane_changed() {
        let io = FakePane::new(&[
            "> ",
            "> ship it please",
            "spinner ⠙\n> ship it please", // changed, but the text is still at the tail
        ]);
        let d = watched(
            deliver(&io, &pane(), "ship it please", &key(), settle(), no_sleep)
                .await
                .unwrap(),
        );
        assert_eq!(d.rung, Rung::Submitted);
        assert!(
            d.detail.contains("still appears"),
            "a change that left the text in place must be described honestly: {}",
            d.detail
        );
    }

    /// A one-letter answer is this product's most common reply, and it must not produce a false
    /// "echoed" by matching a `y` that was already on screen.
    #[tokio::test]
    async fn a_one_letter_reply_does_not_false_positive_on_the_echo_check() {
        assert!(echo_needle("y").is_none());
        let io = FakePane::new(&[
            "Do you want to deploy? [y/N] ",
            "Do you want to deploy? [y/N] y",
        ]);
        let d = watched(
            deliver(&io, &pane(), "y", &key(), settle(), no_sleep)
                .await
                .unwrap(),
        );
        // No needle, so Echoed is unreachable — but the submit rung is still observable, and that
        // is what carries a short reply.
        assert!(d.rung <= Rung::Accepted || d.rung == Rung::Submitted);
    }

    #[tokio::test]
    async fn multi_line_text_uses_the_longest_line_as_the_needle() {
        assert_eq!(
            echo_needle("ok\nplease rebase onto main first\nthanks").as_deref(),
            Some("please rebase onto main first")
        );
        // A very long line is capped so a wrapped render still matches.
        let long = "x".repeat(200);
        assert_eq!(echo_needle(&long).unwrap().chars().count(), 40);
    }

    #[test]
    fn the_rungs_are_ordered_and_only_the_top_one_claims_the_agent_acted() {
        assert!(Rung::Accepted < Rung::Echoed);
        assert!(Rung::Echoed < Rung::Submitted);
        assert!(Rung::Submitted < Rung::Acted);
        // The wording itself lives in `voice`, which has its own test that only the top rung
        // sounds certain. What matters here is the ORDER those rules depend on.
    }

    // ---- the two-writer race ------------------------------------------------------------------

    /// What the harness does to an arrow key at the end of the option row.
    ///
    /// Nobody has probed this against opencode, so nothing this module emits is allowed to depend
    /// on the answer. Every test below therefore runs against both, and both must agree.
    #[derive(Debug, Clone, Copy)]
    enum Edge {
        Clamps,
        Wraps,
    }

    /// What the pane shows once the confirm key has gone out.
    #[derive(Debug, Clone, Copy)]
    enum AfterConfirm {
        /// The dialog closes and leaves ordinary output behind.
        Closes,
        /// It is still up: the confirm key did nothing anyone can see.
        Stays,
        /// Something menu-shaped is still there, but its highlight cannot be read.
        Unreadable,
        /// The question that was answered is gone and the agent is asking the NEXT one. An agent
        /// touching two files in a row does exactly this.
        AsksAnother,
    }

    /// A row that is plainly a control and whose highlight nobody can resolve: every option shares
    /// one background, so there is no selected one to find.
    const UNREADABLE: &str = "\u{1b}[48;5;1mAllow once\u{1b}[0m \u{1b}[48;5;1mReject\u{1b}[0m  \u{1b}[0m⇆ select  enter confirm";

    /// A dialog with a keyboard on it as well as a phone.
    ///
    /// The point of the fake is that WHERE THE HIGHLIGHT REALLY IS and WHAT A READ SHOWS are two
    /// different things. A real TUI redraws tens of milliseconds after the key that moved it, so a
    /// read taken just after the operator's own keypress renders the frame from before it. `truth`
    /// is where the highlight is; `rendered` is the frame the next read will return.
    /// When the herd stops answering reads. A dropped socket or a herd restart does not wait for
    /// a convenient moment, and the two moments that matter are the ones AFTER keys have gone out.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum GoesQuiet {
        Never,
        /// After the arrow keys have moved the highlight, before the gate can look.
        AfterTheArrows,
        /// After the confirm key has landed — a permission that has just been granted.
        AfterTheConfirmKey,
    }

    struct DialogPane {
        options: Vec<String>,
        edge: Edge,
        /// A dialog whose arrows do nothing — one laid out vertically, say.
        arrows_work: bool,
        /// Someone is scrolling it: every read finds the highlight somewhere new.
        restless: bool,
        quiet: GoesQuiet,
        arrow_keys_fail: bool,
        confirm_key_fails: bool,
        after: AfterConfirm,
        truth: RefCell<usize>,
        rendered: RefCell<usize>,
        /// The frame the read AFTER the next one will return. This is the redraw lag.
        pending: RefCell<Option<usize>>,
        confirmed: RefCell<Option<String>>,
        keys: RefCell<Vec<String>>,
    }

    impl DialogPane {
        fn new(options: &[&str], truth: usize, edge: Edge) -> Self {
            Self {
                options: options.iter().map(|o| o.to_string()).collect(),
                edge,
                arrows_work: true,
                restless: false,
                quiet: GoesQuiet::Never,
                arrow_keys_fail: false,
                confirm_key_fails: false,
                after: AfterConfirm::Closes,
                truth: RefCell::new(truth),
                rendered: RefCell::new(truth),
                pending: RefCell::new(None),
                confirmed: RefCell::new(None),
                keys: RefCell::new(Vec::new()),
            }
        }

        /// Stage a screen that has not caught up with the keyboard yet: the next read shows
        /// `stale`, the one after it shows where the highlight really is.
        fn lagging_by_one_frame(mut self, stale: usize) -> Self {
            let truth = *self.truth.borrow();
            self.rendered = RefCell::new(stale);
            self.pending = RefCell::new(Some(truth));
            self
        }

        fn with_inert_arrows(mut self) -> Self {
            self.arrows_work = false;
            self
        }

        fn restless(mut self) -> Self {
            self.restless = true;
            self
        }

        fn goes_quiet(mut self, quiet: GoesQuiet) -> Self {
            self.quiet = quiet;
            self
        }

        /// The send of the arrow keys fails. Whether they moved the highlight is then exactly
        /// what nobody knows.
        fn drops_the_arrow_keys(mut self) -> Self {
            self.arrow_keys_fail = true;
            self
        }

        /// The send of the confirm key itself fails. Whether the key reached the terminal is then
        /// exactly what nobody knows — a timeout is the shape of a herd that took the write and
        /// never answered.
        fn drops_the_confirm_key(mut self) -> Self {
            self.confirm_key_fails = true;
            self
        }

        fn after_confirm(mut self, after: AfterConfirm) -> Self {
            self.after = after;
            self
        }

        /// The option the confirm key actually landed on — the ground truth every test compares the
        /// operator's confirmation against.
        fn confirmed(&self) -> Option<String> {
            self.confirmed.borrow().clone()
        }

        fn keys(&self) -> Vec<String> {
            self.keys.borrow().clone()
        }

        fn screen(&self, selected: usize) -> String {
            let refs: Vec<&str> = self.options.iter().map(String::as_str).collect();
            crate::permission::synthetic::render_dialog(&refs, selected)
        }
    }

    impl PaneIo for DialogPane {
        async fn read_visible(&self, _pane: &PaneId) -> Result<String, HerdrError> {
            panic!("a dialog must be read in colour, or which option is highlighted is invisible")
        }
        async fn send_input_text(&self, _pane: &PaneId, _text: &str) -> Result<(), HerdrError> {
            panic!(
                "a dialog is never answered with words: they go nowhere, and the confirm key after \
                 them presses whatever happens to be highlighted"
            )
        }
        async fn send_submit_key(&self, _pane: &PaneId, _key: &Key) -> Result<(), HerdrError> {
            panic!("the dialog path sends its keys as a sequence, so all of them are recorded")
        }
        async fn read_visible_ansi(&self, _pane: &PaneId) -> Result<String, HerdrError> {
            let gone = HerdrError::Timeout {
                method: "pane.readVisible",
                elapsed: Duration::from_secs(5),
            };
            if self.quiet == GoesQuiet::AfterTheConfirmKey && self.confirmed.borrow().is_some() {
                return Err(gone);
            }
            if self.quiet == GoesQuiet::AfterTheArrows && !self.keys.borrow().is_empty() {
                return Err(gone);
            }
            if self.confirmed.borrow().is_some() {
                return Ok(match self.after {
                    AfterConfirm::Closes => "agent: working on it\n> ".to_string(),
                    AfterConfirm::Stays => self.screen(*self.truth.borrow()),
                    AfterConfirm::Unreadable => UNREADABLE.to_string(),
                    AfterConfirm::AsksAnother => {
                        crate::permission::synthetic::render_dialog(&NEXT_QUESTION_REFS, 0)
                    }
                });
            }
            let frame = *self.rendered.borrow();
            let out = self.screen(frame);
            if let Some(next) = self.pending.borrow_mut().take() {
                *self.rendered.borrow_mut() = next;
            } else if self.restless {
                *self.rendered.borrow_mut() = (frame + 1) % self.options.len();
            }
            Ok(out)
        }
        async fn send_key_sequence(&self, _pane: &PaneId, keys: &[Key]) -> Result<(), HerdrError> {
            if self.arrow_keys_fail && keys.iter().any(|k| k.to_string() != "Enter") {
                return Err(HerdrError::Timeout {
                    method: "pane.sendKeys",
                    elapsed: Duration::from_secs(5),
                });
            }
            if self.confirm_key_fails && keys.iter().any(|k| k.to_string() == "Enter") {
                return Err(HerdrError::Timeout {
                    method: "pane.sendKeys",
                    elapsed: Duration::from_secs(5),
                });
            }
            let n = self.options.len();
            for k in keys {
                let name = k.to_string();
                match name.as_str() {
                    "Right" if self.arrows_work => {
                        let at = *self.truth.borrow();
                        *self.truth.borrow_mut() = match self.edge {
                            Edge::Wraps => (at + 1) % n,
                            Edge::Clamps => (at + 1).min(n - 1),
                        };
                    }
                    "Left" if self.arrows_work => {
                        let at = *self.truth.borrow();
                        *self.truth.borrow_mut() = match self.edge {
                            Edge::Wraps => (at + n - 1) % n,
                            Edge::Clamps => at.saturating_sub(1),
                        };
                    }
                    "Enter" => {
                        *self.confirmed.borrow_mut() =
                            Some(self.options[*self.truth.borrow()].clone());
                    }
                    _ => {}
                }
                self.keys.borrow_mut().push(name);
            }
            // The screen lags the keys, exactly as it lags the operator's own.
            *self.pending.borrow_mut() = Some(*self.truth.borrow());
            Ok(())
        }
    }

    const OPTIONS: [&str; 3] = ["Allow once", "Allow always", "Reject"];

    /// The question an agent asks straight after the one it just had answered. Deliberately a
    /// different list of options: that difference is the only evidence anyone has that the first
    /// question is gone.
    const NEXT_QUESTION_REFS: [&str; 2] = ["Yes", "No"];

    /// THE regression test. The operator is at their keyboard and the phone is in their hand.
    ///
    /// They move the highlight back to `Allow once`; the screen has not redrawn yet, so the first
    /// read the bridge takes still shows `Reject` highlighted. Asking for `Reject` from that read
    /// looks like "already there — just confirm", and the bare confirm key grants `Allow once`.
    #[tokio::test]
    async fn a_screen_that_lagged_the_operators_keypress_still_confirms_what_was_asked() {
        for edge in [Edge::Clamps, Edge::Wraps] {
            let io = DialogPane::new(&OPTIONS, 0, edge).lagging_by_one_frame(2);
            let c = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep)
                .await
                .unwrap()
                .expect("the dialog is there and Reject names exactly one option");
            assert_eq!(
                io.confirmed().as_deref(),
                Some("Reject"),
                "{edge:?}: the key landed on the wrong option"
            );
            assert_eq!(io.keys(), ["Right", "Right", "Enter"], "{edge:?}");
            assert_eq!(c.delivery.rung, Rung::Submitted, "{edge:?}");
            assert_eq!(c.option, "Reject", "{edge:?}");
            assert_eq!(
                Some(c.option.clone()),
                io.confirmed(),
                "{edge:?}: the operator is told an option nobody verified"
            );
        }
    }

    /// A dialog whose arrows do nothing. The old code moved and confirmed in one send, so the
    /// confirm key granted whatever was still highlighted.
    #[tokio::test]
    async fn nothing_is_confirmed_when_the_highlight_will_not_move() {
        for edge in [Edge::Clamps, Edge::Wraps] {
            let io = DialogPane::new(&OPTIONS, 0, edge).with_inert_arrows();
            let r = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep)
                .await
                .unwrap();
            assert_eq!(io.confirmed(), None, "{edge:?}: something was confirmed");
            assert!(
                !io.keys().contains(&"Enter".to_string()),
                "{edge:?}: the confirm key went out anyway: {:?}",
                io.keys()
            );
            let ChoiceRefused::NotConfirmed { why, .. } =
                r.expect_err("nothing was confirmed, so this is a refusal")
            else {
                panic!("{edge:?}: the operator must be told the highlight would not move");
            };
            assert!(
                why.contains("Reject") && why.contains("Allow once"),
                "{edge:?}: the operator must be told what they asked for AND what is actually \
                 highlighted: {why}"
            );
        }
    }

    /// Someone is scrolling the dialog. Not one key may go out.
    #[tokio::test]
    async fn a_prompt_that_will_not_hold_still_is_refused_without_a_keystroke() {
        let io = DialogPane::new(&OPTIONS, 0, Edge::Clamps).restless();
        let r = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep)
            .await
            .unwrap();
        assert!(io.keys().is_empty(), "keys went out: {:?}", io.keys());
        assert_eq!(io.confirmed(), None);
        assert!(
            matches!(r, Err(ChoiceRefused::NotConfirmed { .. })),
            "a menu someone else is driving must be refused, and said so"
        );
    }

    /// The load-bearing invariant: the label the operator is told is the label the confirm key
    /// landed on. Checked across every staging, so "says Reject, confirmed Allow once" is
    /// impossible by construction rather than by inspection.
    #[tokio::test]
    async fn the_reported_option_is_the_one_that_was_verified() {
        for edge in [Edge::Clamps, Edge::Wraps] {
            let staged: Vec<(&str, DialogPane, &str)> = vec![
                (
                    "already on it",
                    DialogPane::new(&OPTIONS, 0, edge),
                    "Allow once",
                ),
                ("needs a move", DialogPane::new(&OPTIONS, 0, edge), "Reject"),
                (
                    "the screen lagged the keyboard",
                    DialogPane::new(&OPTIONS, 0, edge).lagging_by_one_frame(2),
                    "Reject",
                ),
            ];
            for (what, io, want) in staged {
                let c = choose(&io, &pane(), Choice::Reply(want), settle(), no_sleep)
                    .await
                    .unwrap()
                    .expect("each staging resolves to exactly one option");
                assert_eq!(
                    io.confirmed().as_deref(),
                    Some(want),
                    "{what} ({edge:?}): confirmed the wrong option"
                );
                assert_eq!(
                    Some(c.option.clone()),
                    io.confirmed(),
                    "{what} ({edge:?}): the operator is told an option nobody verified"
                );
                assert!(
                    c.delivery.detail.contains(&c.option),
                    "{what} ({edge:?}): {detail}",
                    detail = c.delivery.detail
                );
            }
        }
    }

    /// "No dialog parsed" and "the dialog closed" are different questions. A menu the bridge merely
    /// failed to understand is not evidence that anything was answered.
    #[tokio::test]
    async fn an_unparseable_dialog_is_not_mistaken_for_a_closed_one() {
        let io = DialogPane::new(&OPTIONS, 0, Edge::Clamps).after_confirm(AfterConfirm::Unreadable);
        let c = choose(
            &io,
            &pane(),
            Choice::Reply("Allow once"),
            settle(),
            no_sleep,
        )
        .await
        .unwrap()
        .expect("the dialog was there when we looked");
        assert_eq!(io.confirmed().as_deref(), Some("Allow once"));
        assert_eq!(c.delivery.rung, Rung::Accepted, "{}", c.delivery.detail);
        assert!(
            c.delivery.detail.contains("may not have taken"),
            "{}",
            c.delivery.detail
        );
    }

    /// A confirm key that did nothing visible. The bridge under-claims, but it still names the
    /// option it verified rather than inventing one.
    #[tokio::test]
    async fn a_prompt_still_on_screen_after_the_confirm_is_reported_as_doubtful() {
        let io = DialogPane::new(&OPTIONS, 0, Edge::Clamps).after_confirm(AfterConfirm::Stays);
        let c = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep)
            .await
            .unwrap()
            .expect("the dialog was there when we looked");
        assert_eq!(c.delivery.rung, Rung::Accepted);
        assert!(
            c.delivery.detail.contains("may not have taken"),
            "{}",
            c.delivery.detail
        );
        assert_eq!(io.confirmed().as_deref(), Some("Reject"));
        assert_eq!(c.option, "Reject", "the verified label is still named");
    }

    /// A button is only meaningful while the menu it was drawn against is still the one showing.
    /// A label alone is not enough: another menu can offer the same word, and confirming it would
    /// answer a question nobody read.
    #[tokio::test]
    async fn a_button_drawn_for_a_different_menu_presses_nothing() {
        let drawn_from: Vec<String> = OPTIONS.iter().map(|o| o.to_string()).collect();

        let io = DialogPane::new(&OPTIONS, 0, Edge::Clamps);
        let c = choose(
            &io,
            &pane(),
            Choice::Button {
                label: "Reject",
                drawn_from: &drawn_from,
            },
            settle(),
            no_sleep,
        )
        .await
        .unwrap()
        .expect("the menu on screen is the one the buttons were drawn for");
        assert_eq!(c.option, "Reject");
        assert_eq!(io.confirmed().as_deref(), Some("Reject"));

        // Same label, different question.
        let moved_on = DialogPane::new(&["Reject", "Keep going"], 0, Edge::Clamps);
        let r = choose(
            &moved_on,
            &pane(),
            Choice::Button {
                label: "Reject",
                drawn_from: &drawn_from,
            },
            settle(),
            no_sleep,
        )
        .await
        .unwrap();
        assert!(
            matches!(r, Err(ChoiceRefused::Changed { .. })),
            "a tap on a menu that has moved on must be refused, not resolved"
        );
        assert!(moved_on.keys().is_empty());
        assert_eq!(moved_on.confirmed(), None);
    }

    /// The two ways there is nothing to press. Both must leave the terminal untouched.
    #[tokio::test]
    async fn a_reply_that_names_no_option_presses_nothing() {
        let unclear = DialogPane::new(&OPTIONS, 0, Edge::Clamps);
        let r = choose(
            &unclear,
            &pane(),
            Choice::Reply("allow"),
            settle(),
            no_sleep,
        )
        .await
        .unwrap();
        let ChoiceRefused::Unclear { options } = r.expect_err("\"allow\" names two of them") else {
            panic!("an ambiguous word must be refused, never resolved by order");
        };
        assert_eq!(options, OPTIONS, "the operator is shown what they can pick");
        assert!(unclear.keys().is_empty());
        assert_eq!(unclear.confirmed(), None);

        let gone = FakePane::new(&["agent: working on it\n> "]);
        let r = choose(&gone, &pane(), Choice::Reply("Reject"), settle(), no_sleep)
            .await
            .unwrap();
        assert_eq!(r.expect_err("there is no menu"), ChoiceRefused::NotADialog);
        assert!(gone.sent_keys.borrow().is_empty());
    }

    /// The inversion, end to end. The parser read a two-option dialog backwards whenever the
    /// selected option's highlight bar was the widest background on the row — a long label does it
    /// — and the misread is deterministic, so the settled re-read and the gate both agreed with it.
    /// No arrow key was sent, the bare confirm key went out, the terminal granted the permission,
    /// and the operator was told they had refused it.
    #[tokio::test]
    async fn refusing_a_two_option_grant_never_confirms_the_grant() {
        const GRANT: &str = "Yes, and don't ask me again for this directory in this session";
        for edge in [Edge::Clamps, Edge::Wraps] {
            let io = DialogPane::new(&[GRANT, "No"], 0, edge);
            let r = choose(&io, &pane(), Choice::Reply("no"), settle(), no_sleep)
                .await
                .unwrap();
            assert_ne!(
                io.confirmed().as_deref(),
                Some(GRANT),
                "{edge:?}: refusing the dialog granted the permission"
            );
            match r {
                Ok(c) => {
                    assert_eq!(c.option, "No", "{edge:?}");
                    assert_eq!(
                        Some(c.option),
                        io.confirmed(),
                        "{edge:?}: the operator was told an option the terminal never confirmed"
                    );
                    assert_eq!(io.keys(), ["Right", "Enter"], "{edge:?}");
                }
                Err(_) => assert_eq!(io.confirmed(), None, "{edge:?}"),
            }
        }
    }

    /// A menu whose highlight cannot be read is still a menu. Calling it gone tells the operator
    /// "that session isn't asking that any more" — which is false, it is still asking — and drops
    /// the one piece of advice that applies: answer it at the keyboard.
    #[tokio::test]
    async fn a_menu_nobody_can_read_is_not_reported_as_gone() {
        let io = FakePane::new(&[UNREADABLE]);
        let r = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep)
            .await
            .unwrap();
        let why = r.expect_err("nothing may be pressed at a menu nobody can read");
        assert_eq!(
            why,
            ChoiceRefused::Unreadable,
            "a menu that cannot be read is not a menu that has gone away"
        );
        assert!(
            why.to_string().contains("keyboard"),
            "the one piece of advice that applies was dropped: {why}"
        );
        assert!(io.sent_keys.borrow().is_empty());
        assert!(io.sent_text.borrow().is_empty());
    }

    /// THE regression test for the tapped button.
    ///
    /// The buttons said `Allow once · Allow always · Reject` and the operator tapped the third. By
    /// the time the tap arrives the menu has been redrawn in a different order, so the third option
    /// is now `Allow always` — a reviewer made a button reading "Reject" grant the broadest
    /// permission this way. Answering by position is what does it, and it is measurable: driving
    /// this same screen with the position the button carried confirms "Allow always" and sends
    /// `["Right", "Right", "Enter"]`.
    #[tokio::test]
    async fn a_button_never_resolves_by_position_when_the_menu_reordered() {
        let drawn_from: Vec<String> = OPTIONS.iter().map(|o| o.to_string()).collect();
        let io = DialogPane::new(&["Reject", "Allow once", "Allow always"], 0, Edge::Clamps);

        let r = choose(
            &io,
            &pane(),
            Choice::Button {
                label: "Reject",
                drawn_from: &drawn_from,
            },
            settle(),
            no_sleep,
        )
        .await
        .unwrap();

        let ChoiceRefused::Changed { now } = r.expect_err("the menu is not the one that was drawn")
        else {
            panic!("a reordered menu must be refused, never answered by position");
        };
        assert_eq!(now, ["Reject", "Allow once", "Allow always"]);
        assert!(
            io.keys().is_empty(),
            "not one key may reach a menu the buttons were not drawn for"
        );
        assert_eq!(io.confirmed(), None);
    }

    /// The same defect in its other shape: the agent asked again with one more option, so the third
    /// button now points at `Allow all`. Nothing about the position says so; the labels do.
    #[tokio::test]
    async fn a_button_whose_menu_gained_an_option_presses_nothing() {
        let drawn_from: Vec<String> = OPTIONS.iter().map(|o| o.to_string()).collect();
        let io = DialogPane::new(
            &["Allow once", "Allow always", "Allow all", "Reject"],
            0,
            Edge::Clamps,
        );

        let r = choose(
            &io,
            &pane(),
            Choice::Button {
                label: "Reject",
                drawn_from: &drawn_from,
            },
            settle(),
            no_sleep,
        )
        .await
        .unwrap();

        assert!(
            matches!(r, Err(ChoiceRefused::Changed { .. })),
            "a menu that grew an option must be refused: {r:?}"
        );
        assert!(io.keys().is_empty());
        assert_eq!(io.confirmed(), None);
    }

    /// The other half of not being a false-refusal machine: a tap on the menu that IS on screen
    /// still answers it, and it answers the label the button showed rather than its position.
    #[tokio::test]
    async fn a_button_on_the_menu_it_was_drawn_for_still_answers_it() {
        let drawn_from: Vec<String> = OPTIONS.iter().map(|o| o.to_string()).collect();
        let io = DialogPane::new(&OPTIONS, 0, Edge::Clamps);
        let c = choose(
            &io,
            &pane(),
            Choice::Button {
                label: "Reject",
                drawn_from: &drawn_from,
            },
            settle(),
            no_sleep,
        )
        .await
        .unwrap()
        .expect("the menu on screen is the one those buttons were drawn for");

        assert_eq!(c.option, "Reject");
        assert_eq!(io.confirmed().as_deref(), Some("Reject"));
        assert_eq!(io.keys(), ["Right", "Right", "Enter"]);
    }

    #[test]
    fn tail_contains_only_looks_at_the_bottom_of_the_pane() {
        let pane = "old line with needle\n1\n2\n3\n4\n5\nbottom";
        assert!(
            !tail_contains(pane, "needle"),
            "a needle high up is history"
        );
        assert!(tail_contains(pane, "bottom"));
    }

    /// The herd goes quiet AFTER the confirm key has landed. The permission was granted; the read
    /// that would have checked whether the menu closed is the thing that failed. Handing the caller
    /// an error there had it tell the operator "I can't reach the herd right now, so nothing was
    /// sent" — about a permission that had just been granted.
    ///
    /// Never tell the operator nothing was sent when something was. Say what is known and what is
    /// not.
    #[tokio::test]
    async fn a_herd_that_goes_quiet_after_the_confirm_key_is_never_reported_as_nothing_sent() {
        let io =
            DialogPane::new(&OPTIONS, 0, Edge::Clamps).goes_quiet(GoesQuiet::AfterTheConfirmKey);
        let r = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep).await;
        assert_eq!(
            io.confirmed().as_deref(),
            Some("Reject"),
            "the confirm key landed, so nothing here may report that it did not"
        );
        let c = r
            .expect(
                "the confirm key had already gone out; an error here is reported to the operator \
                 as \"nothing was sent\"",
            )
            .expect("the option was confirmed");
        assert_eq!(c.option, "Reject");
        assert!(
            c.delivery.detail.contains("confirmed"),
            "the operator must be told the key went out: {}",
            c.delivery.detail
        );
        assert!(
            !c.delivery.detail.contains("still on screen"),
            "the pane was never seen again, so nothing may be claimed about it: {}",
            c.delivery.detail
        );
    }

    /// The herd goes quiet after the ARROWS have moved the highlight but before the gate can look.
    /// The confirm key never goes out — that part is true — but the highlight in the operator's
    /// terminal has moved, and telling them nothing was typed there is false.
    #[tokio::test]
    async fn a_herd_that_goes_quiet_after_the_arrows_admits_the_highlight_moved() {
        let io = DialogPane::new(&OPTIONS, 0, Edge::Clamps).goes_quiet(GoesQuiet::AfterTheArrows);
        let r = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep).await;
        assert_eq!(io.confirmed(), None, "nothing may be confirmed here");
        let refused = r
            .expect(
                "arrow keys had already gone into that terminal; an error here is reported to the \
                 operator as \"nothing was sent\"",
            )
            .expect_err("nothing was confirmed, so this is a refusal");
        let ChoiceRefused::NotConfirmed { why, keys_sent } = refused else {
            panic!("the operator must be told what was seen");
        };
        assert!(keys_sent, "the arrows reached that terminal");
        assert!(
            why.contains("Reject"),
            "the operator must be told what they asked for: {why}"
        );
    }

    /// The send of the confirm key itself fails. Whether it reached the terminal is unknowable from
    /// here — a timeout is the shape of a herd that took the write and never answered — so the one
    /// thing that must not be said is that nothing was sent.
    #[tokio::test]
    async fn a_confirm_key_that_may_or_may_not_have_landed_is_never_reported_as_nothing_sent() {
        let io = DialogPane::new(&OPTIONS, 0, Edge::Clamps).drops_the_confirm_key();
        let c = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep)
            .await
            .expect(
                "whether the confirm key landed is unknown; an error here is reported to the \
                 operator as \"nothing was sent\"",
            )
            .expect("the operator must be told what is and is not known");
        assert_eq!(c.afterwards, Afterwards::NotSeen);
        assert_eq!(c.option, "Reject");
        assert!(
            c.delivery.detail.contains("could not check"),
            "the operator must be told what was not established: {}",
            c.delivery.detail
        );
    }

    /// The send of the ARROW keys fails. Nothing was confirmed — that is certain, the confirm key
    /// is a separate send that never happened — but whether the highlight moved is unknown, and
    /// "nothing was sent" is a claim about the terminal that this function cannot make.
    #[tokio::test]
    async fn arrow_keys_that_may_or_may_not_have_landed_are_never_reported_as_nothing_sent() {
        let io = DialogPane::new(&OPTIONS, 0, Edge::Clamps).drops_the_arrow_keys();
        let refused = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep)
            .await
            .expect(
                "whether the arrows landed is unknown; an error here is reported to the operator \
                 as \"nothing was sent\"",
            )
            .expect_err("the confirm key never went out, so nothing was confirmed");
        assert_eq!(io.confirmed(), None);
        let ChoiceRefused::NotConfirmed { why, keys_sent } = refused else {
            panic!("the operator must be told what was and was not done");
        };
        assert!(keys_sent, "keys went out toward that terminal");
        assert!(why.contains("Reject"), "{why}");
    }

    // ---- the text path, after the point of no return ------------------------------------------

    /// Where the herd stops answering, on the path every ordinary typed reply takes.
    ///
    /// All but the first are moments AFTER the operator's words have already left the bridge. A
    /// timeout is the shape of a herd that took the write and never answered, so none of those is
    /// evidence that the terminal was left alone.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Drops {
        /// The look taken BEFORE anything is written. The one failure that really does mean the
        /// pane was never touched.
        BeforeAnyWrite,
        /// The send of the text itself. The words may well be in that terminal.
        AsTheTextGoesOut,
        /// The look that would have checked whether the words rendered.
        LookingAfterTheText,
        /// The send of the submit key, after the words went in.
        AsTheSubmitKeyGoesOut,
        /// The look after the submit key — the words are in and have been submitted.
        LookingAfterTheSubmitKey,
    }

    /// A pane that takes the write and then goes quiet: a herd restart, or a dropped socket.
    struct DroppingPane {
        drops: Drops,
        reads: RefCell<usize>,
        text: RefCell<Vec<String>>,
        keys: RefCell<Vec<String>>,
    }

    impl DroppingPane {
        fn new(drops: Drops) -> Self {
            Self {
                drops,
                reads: RefCell::new(0),
                text: RefCell::new(Vec::new()),
                keys: RefCell::new(Vec::new()),
            }
        }

        fn gone(method: &'static str) -> HerdrError {
            HerdrError::Timeout {
                method,
                elapsed: Duration::from_secs(5),
            }
        }

        /// The words that reached the terminal, whatever the RPC then reported.
        fn text(&self) -> Vec<String> {
            self.text.borrow().clone()
        }

        /// The keys that reached the terminal, whatever the RPC then reported.
        fn keys(&self) -> Vec<String> {
            self.keys.borrow().clone()
        }
    }

    impl PaneIo for DroppingPane {
        async fn read_visible(&self, _pane: &PaneId) -> Result<String, HerdrError> {
            let n = {
                let mut r = self.reads.borrow_mut();
                *r += 1;
                *r
            };
            match n {
                1 if self.drops == Drops::BeforeAnyWrite => Err(Self::gone("pane.readVisible")),
                1 => Ok("> ".to_string()),
                2 if self.drops == Drops::LookingAfterTheText => {
                    Err(Self::gone("pane.readVisible"))
                }
                2 => Ok("> please carry on and delete the branch".to_string()),
                _ if self.drops == Drops::LookingAfterTheSubmitKey => {
                    Err(Self::gone("pane.readVisible"))
                }
                _ => Ok("agent: on it\n> ".to_string()),
            }
        }
        async fn send_input_text(&self, _pane: &PaneId, text: &str) -> Result<(), HerdrError> {
            // Recorded BEFORE the failure on purpose: a herd that took the bytes and never
            // answered has put the operator's words in that terminal either way.
            self.text.borrow_mut().push(text.to_string());
            if self.drops == Drops::AsTheTextGoesOut {
                return Err(Self::gone("pane.sendInput"));
            }
            Ok(())
        }
        async fn send_submit_key(&self, _pane: &PaneId, key: &Key) -> Result<(), HerdrError> {
            self.keys.borrow_mut().push(key.to_string());
            if self.drops == Drops::AsTheSubmitKeyGoesOut {
                return Err(Self::gone("pane.sendKeys"));
            }
            Ok(())
        }
        async fn read_visible_ansi(&self, _pane: &PaneId) -> Result<String, HerdrError> {
            panic!("the text path reads without colour")
        }
        async fn send_key_sequence(&self, _pane: &PaneId, _keys: &[Key]) -> Result<(), HerdrError> {
            panic!("the text path sends its submit key on its own")
        }
    }

    const REPLY: &str = "please carry on and delete the branch";

    /// The guarantee the choice path already keeps, on the path every ordinary reply takes.
    ///
    /// The operator's words are in that terminal — in two of these four they have been submitted as
    /// well — and the only caller turns an error from `deliver` into "I can't reach the herd right
    /// now, so nothing was sent". The operator then sends the message again, and the agent gets it
    /// twice.
    #[tokio::test]
    async fn a_failure_after_the_words_went_out_is_never_reported_as_nothing_sent() {
        for drops in [
            Drops::AsTheTextGoesOut,
            Drops::LookingAfterTheText,
            Drops::AsTheSubmitKeyGoesOut,
            Drops::LookingAfterTheSubmitKey,
        ] {
            let io = DroppingPane::new(drops);
            let out = deliver(&io, &pane(), REPLY, &key(), settle(), no_sleep).await;
            assert_eq!(
                io.text(),
                [REPLY],
                "{drops:?}: the operator's words reached that terminal"
            );
            let Ok(Delivered::LostSight { reached, .. }) = out else {
                panic!(
                    "{drops:?}: this reaches the operator as \"nothing was sent\", about words \
                     that are sitting in their terminal: {out:?}"
                );
            };
            // What the operator has to assume, which is what decides what they are told.
            let expected = match drops {
                Drops::AsTheTextGoesOut | Drops::LookingAfterTheText => Reached::TheWords,
                _ => Reached::TheWordsAndTheSubmitKey,
            };
            assert_eq!(reached, expected, "{drops:?}");
            // The submit key is never sent onto a screen nobody could read: a modal that took
            // focus would swallow the words and let the key press whatever is highlighted.
            assert_eq!(
                io.keys().is_empty(),
                expected == Reached::TheWords,
                "{drops:?}: {:?}",
                io.keys()
            );
            for place in [crate::voice::Place::Topic, crate::voice::Place::Flat] {
                let m = crate::voice::lost_sight(place, "omarchy-lab", reached);
                assert!(!m.contains("nothing was sent"), "{drops:?}: {m:?}");
                assert!(
                    m.contains("look at that session"),
                    "{drops:?}: the operator must be sent to look before they send it again: {m:?}"
                );
            }
        }
    }

    /// The other half, and the reason the guarantee is worth anything: the ONE look that happens
    /// before a single byte is written. Its failure really does mean nothing was sent, and the
    /// operator must still be told so plainly.
    #[tokio::test]
    async fn a_failure_before_anything_is_written_is_still_an_error() {
        let io = DroppingPane::new(Drops::BeforeAnyWrite);
        let out = deliver(&io, &pane(), REPLY, &key(), settle(), no_sleep).await;
        assert!(
            out.is_err(),
            "nothing was written, so \"nothing was sent\" is the truth and must stay reachable"
        );
        assert!(io.text().is_empty(), "nothing may have been typed");
        assert!(io.keys().is_empty(), "no key may have gone out");
    }

    /// An agent that answers one permission prompt and immediately asks the NEXT one.
    ///
    /// The answer landed — the question it was for is gone from the screen — but a menu is still
    /// drawn there, and "the prompt is still up, it may not have registered" is then a false
    /// sentence about a permission that was granted. The operator's natural response is to answer
    /// again, and that answer now goes to a different question.
    #[tokio::test]
    async fn the_next_question_is_not_reported_as_the_answer_not_registering() {
        let io =
            DialogPane::new(&OPTIONS, 0, Edge::Clamps).after_confirm(AfterConfirm::AsksAnother);
        let c = choose(&io, &pane(), Choice::Reply("Reject"), settle(), no_sleep)
            .await
            .unwrap()
            .expect("Reject names exactly one option");
        assert_eq!(
            io.confirmed().as_deref(),
            Some("Reject"),
            "the terminal really did confirm it"
        );
        assert_ne!(
            c.afterwards,
            Afterwards::MenuStillUp,
            "the question that was answered is gone; the menu on screen is a different one"
        );
        assert!(
            !c.delivery.detail.contains("may not have taken"),
            "an answer that landed was written down as doubtful: {}",
            c.delivery.detail
        );
    }
}

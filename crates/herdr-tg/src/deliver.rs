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

impl Rung {
    /// Does this rung warrant telling the operator something looks wrong?
    ///
    /// `Echoed` is the dangerous one: the text is sitting in the input buffer unsent, which is
    /// exactly what a wrong submit key looks like.
    pub fn needs_attention(self) -> bool {
        matches!(self, Rung::Accepted | Rung::Echoed)
    }
}

/// What happened when the bridge tried to put text into a pane.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub pane: PaneId,
    pub rung: Rung,
    /// Human-readable account of what was and was not observed. Goes to the audit log verbatim.
    pub detail: String,
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
/// Never returns a rung it did not observe. A transport failure propagates — the operator must not
/// be told anything about a write whose fate is unknown.
pub async fn deliver<P: PaneIo>(
    io: &P,
    pane: &PaneId,
    text: &str,
    submit: &Key,
    settle: Settle,
    sleep: impl Fn(Duration) -> futures_core::future::BoxFuture<'static, ()>,
) -> Result<Delivery, HerdrError> {
    let before = io.read_visible(pane).await?;

    io.send_input_text(pane, text).await?;
    let mut rung = Rung::Accepted;
    let mut detail = String::from("herdr accepted the text");

    // Rung 2: did the TUI actually render it? A distinctive slice of the operator's own text is the
    // probe — the whole message may be wrapped, indented, or truncated by the TUI.
    let after_text = io.read_visible(pane).await?;
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

    io.send_submit_key(pane, submit).await?;

    // Rung 3: did the submit key change anything? Compared against the post-text read, so the
    // change attributable to the submit key is isolated from the change caused by the text.
    for _ in 0..settle.attempts {
        sleep(settle.interval).await;
        let now = io.read_visible(pane).await?;
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

    Ok(Delivery {
        pane: pane.clone(),
        rung,
        detail,
    })
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
    #[allow(
        dead_code,
        reason = "tapped through the callback payload, which lands with the label-carrying buttons"
    )]
    Button {
        label: &'a str,
        drawn_from: &'a [String],
    },
}

/// Why nothing was chosen. In EVERY variant, not one key reached the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceRefused {
    /// The pane is not showing a menu any more.
    NotADialog,
    /// The reply named no option, or more than one. Carries the options as they are now, so the
    /// operator can be shown what they can actually pick.
    Unclear { options: Vec<String> },
    /// It is showing a menu, but not the one those buttons were drawn for.
    Changed { now: Vec<String> },
    /// The menu would not hold still, would not move, or ended up on the wrong option, so the
    /// confirm key never went out. Carries an operator-readable account of what was seen.
    NotConfirmed(String),
}

impl std::fmt::Display for ChoiceRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChoiceRefused::NotADialog => {
                f.write_str("That session isn't showing a menu any more, so I pressed nothing.")
            }
            ChoiceRefused::Unclear { .. } => {
                f.write_str("I didn't catch which one you meant, so I pressed nothing.")
            }
            ChoiceRefused::Changed { .. } => f.write_str(
                "That menu has changed since those buttons were drawn, so I pressed nothing.",
            ),
            ChoiceRefused::NotConfirmed(why) => f.write_str(why),
        }
    }
}

/// A choice that was actually made.
#[derive(Debug, Clone)]
pub struct Chosen {
    pub delivery: Delivery,
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
    /// No menu could be read out of the pane.
    Gone,
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
    let Some(mut prev) = crate::permission::parse(&io.read_visible_ansi(pane).await?) else {
        return Ok(Look::Gone);
    };
    for _ in 0..settle.attempts {
        sleep(settle.interval).await;
        let Some(now) = crate::permission::parse(&io.read_visible_ansi(pane).await?) else {
            return Ok(Look::Gone);
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
        Look::Moving => {
            return Ok(Err(ChoiceRefused::NotConfirmed(
                "That menu is moving — someone is answering it at the keyboard. I pressed nothing."
                    .into(),
            )));
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
            return Ok(Err(ChoiceRefused::NotConfirmed(
                "I lost track of which option you meant, so I pressed nothing.".into(),
            )));
        };
        io.send_key_sequence(pane, &to_keys(&keys)).await?;
        // One aim, then look. No corrective loop: if the highlight did not land where it was
        // asked to, someone else is driving, and pressing more keys at them is how the wrong
        // permission gets granted.
        match settled_dialog(io, pane, settle, &sleep).await? {
            Look::Settled(p) => p,
            Look::Gone => {
                return Ok(Err(ChoiceRefused::NotConfirmed(
                    "That menu went away while I was answering it, so I confirmed nothing.".into(),
                )));
            }
            Look::Moving => {
                return Ok(Err(ChoiceRefused::NotConfirmed(
                    "That menu kept moving while I was answering it. I confirmed nothing.".into(),
                )));
            }
        }
    };

    // THE GATE. Everything above is preparation; this is the line that makes the confirmation
    // honest. Nothing is pressed unless a settled read shows the SAME options with the wanted one
    // highlighted. Checking the label alone is not enough — a different menu can offer the same
    // word, and confirming it would answer a question nobody read.
    if aimed.options != start.options {
        return Ok(Err(ChoiceRefused::NotConfirmed(format!(
            "The choices on that menu changed while I was answering it, so I did not confirm \
             \"{chosen}\"."
        ))));
    }
    if aimed.highlighted() != Some(chosen.as_str()) {
        return Ok(Err(ChoiceRefused::NotConfirmed(format!(
            "I couldn't get the highlight onto \"{chosen}\" — it is on \"{}\". Nothing was \
             confirmed; answer it at the keyboard.",
            aimed.highlighted().unwrap_or("something else")
        ))));
    }

    // Its own send, which is what makes the check above a gate rather than a comment.
    io.send_key_sequence(pane, &to_keys(&[crate::permission::CONFIRM]))
        .await?;

    let mut rung = Rung::Accepted;
    let mut detail = format!("moved the highlight onto \"{chosen}\" and confirmed it");
    for _ in 0..settle.attempts {
        sleep(settle.interval).await;
        let now = io.read_visible_ansi(pane).await?;
        // Ordinary output is the ONLY evidence that the menu closed. "Nothing parsed" is not: a
        // menu the bridge merely failed to understand reads the same way, and answering it is
        // exactly what did not happen.
        if crate::permission::classify(&now) == crate::permission::Screen::Prose {
            rung = Rung::Submitted;
            detail = format!("chose \"{chosen}\" — the menu closed");
            break;
        }
    }
    if rung < Rung::Submitted {
        detail = format!("{detail}, but the menu is still on screen — it may not have taken");
    }

    Ok(Ok(Chosen {
        delivery: Delivery {
            pane: pane.clone(),
            rung,
            detail,
        },
        option: chosen,
    }))
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

    #[tokio::test]
    async fn a_clean_submit_reaches_the_submitted_rung() {
        let io = FakePane::new(&[
            "> ",                       // before
            "> ship it please",         // after the text
            "agent: working on it\n> ", // after the submit key
        ]);
        let d = deliver(&io, &pane(), "ship it please", &key(), settle(), no_sleep)
            .await
            .unwrap();
        assert_eq!(d.rung, Rung::Submitted);
        assert!(!d.rung.needs_attention());
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
        let d = deliver(&io, &pane(), "ship it please", &key(), settle(), no_sleep)
            .await
            .unwrap();
        assert_eq!(d.rung, Rung::Echoed, "must not claim Submitted");
        assert!(d.rung.needs_attention());
        assert!(
            d.detail.contains("submit key"),
            "the operator must be told what to suspect: {}",
            d.detail
        );
        // The operator-facing wording is `voice`'s, and it has its own test that only the top
        // rung sounds certain. Here we only pin that this outcome is flagged as doubtful.
        assert!(d.rung.needs_attention());
    }

    /// A TUI that swallows the text entirely — a modal dialog had focus.
    #[tokio::test]
    async fn text_that_never_renders_stays_at_accepted() {
        let io = FakePane::new(&["> ", "> "]);
        let d = deliver(&io, &pane(), "ship it please", &key(), settle(), no_sleep)
            .await
            .unwrap();
        assert_eq!(d.rung, Rung::Accepted);
        assert!(d.rung.needs_attention());
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
        let d = deliver(&io, &pane(), "ship it please", &key(), settle(), no_sleep)
            .await
            .unwrap();
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
        let d = deliver(&io, &pane(), "y", &key(), settle(), no_sleep)
            .await
            .unwrap();
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
        assert!(Rung::Accepted.needs_attention() && Rung::Echoed.needs_attention());
        assert!(!Rung::Submitted.needs_attention() && !Rung::Acted.needs_attention());
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
    struct DialogPane {
        options: Vec<String>,
        edge: Edge,
        /// A dialog whose arrows do nothing — one laid out vertically, say.
        arrows_work: bool,
        /// Someone is scrolling it: every read finds the highlight somewhere new.
        restless: bool,
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
            if self.confirmed.borrow().is_some() {
                return Ok(match self.after {
                    AfterConfirm::Closes => "agent: working on it\n> ".to_string(),
                    AfterConfirm::Stays => self.screen(*self.truth.borrow()),
                    AfterConfirm::Unreadable => UNREADABLE.to_string(),
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
            let ChoiceRefused::NotConfirmed(why) =
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
            matches!(r, Err(ChoiceRefused::NotConfirmed(_))),
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
        assert!(c.delivery.rung.needs_attention());
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
        assert!(c.delivery.rung.needs_attention());
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

    #[test]
    fn tail_contains_only_looks_at_the_bottom_of_the_pane() {
        let pane = "old line with needle\n1\n2\n3\n4\n5\nbottom";
        assert!(
            !tail_contains(pane, "needle"),
            "a needle high up is history"
        );
        assert!(tail_contains(pane, "bottom"));
    }
}

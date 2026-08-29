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

/// Answer a choice dialog by moving the selection and confirming — never with text.
///
/// The dialog is re-parsed from a FRESH read immediately before the keys go out, so the arrow count
/// is computed from the selection as it is now rather than as it was when the push was written. A
/// stale index confirms the wrong option, and on a permission prompt the wrong option is a grant.
///
/// Verified the same way as a text reply, by looking: the dialog should be gone afterwards. If it
/// is still there, the operator is told, because a permission prompt that silently did not resolve
/// is worse than an error.
pub async fn choose<P: PaneIo>(
    io: &P,
    pane: &PaneId,
    want: &str,
    settle: Settle,
    sleep: impl Fn(Duration) -> futures_core::future::BoxFuture<'static, ()>,
) -> Result<Result<Delivery, String>, HerdrError> {
    let before = io.read_visible_ansi(pane).await?;
    let Some(prompt) = crate::permission::parse(&before) else {
        return Ok(Err("that pane is no longer showing a choice".into()));
    };
    let Some(idx) = prompt.match_option(want) else {
        return Ok(Err(format!(
            "I don't know which option \"{want}\" is. Reply with a number, or the option's name."
        )));
    };
    let Some(keys) = prompt.keys_to(idx) else {
        return Ok(Err("that option is out of range".into()));
    };

    let parsed: Vec<Key> = keys
        .iter()
        .map(|k| Key::parse(k).expect("the parser only emits keys the probe confirmed"))
        .collect();
    io.send_key_sequence(pane, &parsed).await?;

    let chosen = prompt.options[idx].clone();
    let mut detail = format!("chose \"{chosen}\" with {}", keys.join(" "));
    let mut rung = Rung::Accepted;
    for _ in 0..settle.attempts {
        sleep(settle.interval).await;
        let now = io.read_visible_ansi(pane).await?;
        if crate::permission::parse(&now).is_none() {
            rung = Rung::Submitted;
            detail = format!("chose \"{chosen}\" — the dialog closed");
            break;
        }
    }
    if rung < Rung::Submitted {
        detail = format!("{detail}, but the dialog is still on screen — it may not have taken");
    }

    Ok(Ok(Delivery {
        pane: pane.clone(),
        rung,
        detail,
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

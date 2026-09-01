//! The outbound budget, and the clip that keeps a message inside Telegram's limits.
//!
//! # The ceiling is per CHAT, and topics do not get their own
//!
//! Telegram's group limit is about twenty messages a minute for the whole chat. Forum topics are
//! threads inside one chat, not chats, so six busy projects share one budget rather than having
//! six. Everything here follows from that: the bucket is keyed on the chat, and fairness between
//! projects has to be arranged rather than assumed.
//!
//! Measured before this was written, not inferred from documentation — see the probe in
//! `docs/HUB-DESIGN.md` §12. If it had turned out to be per-thread, this file would be
//! over-engineering; because it is per-chat, it is the difference between working at six projects
//! and not.
//!
//! # Shedding is never silent
//!
//! A refused send used to be one `tracing::error!` and a drop, and that had already lost 5,164
//! characters of a real agent's longest message. Nothing here drops anything quietly: a frame that
//! cannot go out now produces a `retry_after` the caller must act on, and a message too long to
//! send is clipped **and said to be clipped**, so the ack carries `clamped` rather than `yes`.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Telegram's own ceiling for a group is 20 a minute. Sitting exactly on a limit means discovering
/// it from a 429 during an incident, so the budget is deliberately under it.
pub const PER_MINUTE: u32 = 18;

/// And no more than one a second, which is the other half of the same limit.
pub const MIN_GAP: Duration = Duration::from_millis(1000);

/// The longest message body the hub will put in one Telegram message.
///
/// The API's own limit is 4096 characters. The gap is room for the wrapper the renderer adds — a
/// title line, a footer, an HTML tag pair — so that a message sized right up to the limit here
/// cannot become one that is over it after rendering.
pub const MAX_TEXT: usize = 3500;

/// What a caller must do about a send that cannot go out yet — and WHICH limit refused it.
///
/// The two are not interchangeable and a caller that cannot tell them apart gets it wrong. The
/// one-second gap is a rhythm: wait a beat and the message still goes. The per-minute ceiling is a
/// real limit: waiting for it means waiting minutes, and the honest answer is to shed and say when
/// to come back. An earlier version returned only a duration and guessed from its size, which held
/// for one sender and failed the moment there were two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Too soon after the last message. Waiting this long is enough.
    Gap(Duration),
    /// The per-minute budget is spent. This is a real ceiling.
    Ceiling(Duration),
}

impl Refusal {
    /// How long until it is worth trying again.
    pub fn wait(self) -> Duration {
        match self {
            Self::Gap(d) | Self::Ceiling(d) => d,
        }
    }
}

/// One chat's outbound budget: a token bucket plus a minimum gap.
#[derive(Debug)]
pub struct ChatBudget {
    per_minute: u32,
    tokens: f64,
    last_refill: Instant,
    last_send: Option<Instant>,
    min_gap: Duration,
}

impl ChatBudget {
    pub fn new(per_minute: u32, min_gap: Duration) -> Self {
        Self {
            per_minute,
            tokens: per_minute as f64,
            last_refill: Instant::now(),
            last_send: None,
            min_gap,
        }
    }

    /// Try to spend one send.
    ///
    /// `Err(Refusal)` is a real instruction, not advice: sending anyway is how a bot earns a 429,
    /// and a 429 on a shared bot punishes every project rather than the one that caused it.
    pub fn take(&mut self, now: Instant) -> Result<(), Refusal> {
        self.refill(now);

        if let Some(last) = self.last_send {
            let since = now.saturating_duration_since(last);
            if since < self.min_gap {
                return Err(Refusal::Gap(self.min_gap - since));
            }
        }
        if self.tokens < 1.0 {
            // How long until one whole token exists again.
            let need = 1.0 - self.tokens;
            let per_sec = self.per_minute as f64 / 60.0;
            return Err(Refusal::Ceiling(Duration::from_secs_f64(need / per_sec)));
        }
        self.tokens -= 1.0;
        self.last_send = Some(now);
        Ok(())
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        if elapsed <= 0.0 {
            return;
        }
        let per_sec = self.per_minute as f64 / 60.0;
        self.tokens = (self.tokens + elapsed * per_sec).min(self.per_minute as f64);
        self.last_refill = now;
    }
}

impl Default for ChatBudget {
    fn default() -> Self {
        Self::new(PER_MINUTE, MIN_GAP)
    }
}

/// Every chat this hub sends into.
#[derive(Debug)]
pub struct Budgets {
    chats: BTreeMap<i64, ChatBudget>,
    per_minute: u32,
    min_gap: Duration,
}

impl Budgets {
    /// A budget with chosen limits.
    ///
    /// Tests use a short gap so that a flow test is not one second per message; the limits
    /// themselves are still tested, at their real values, by the tests in this module.
    pub fn new(per_minute: u32, min_gap: Duration) -> Self {
        Self {
            chats: BTreeMap::new(),
            per_minute,
            min_gap,
        }
    }

    pub fn take(&mut self, chat_id: i64, now: Instant) -> Result<(), Refusal> {
        let (per_minute, min_gap) = (self.per_minute, self.min_gap);
        self.chats
            .entry(chat_id)
            .or_insert_with(|| ChatBudget::new(per_minute, min_gap))
            .take(now)
    }
}

impl Default for Budgets {
    fn default() -> Self {
        Self::new(PER_MINUTE, MIN_GAP)
    }
}

/// Clip a message to what Telegram will accept, and say whether anything was lost.
///
/// **Clipped on a character boundary, never on a byte.** Splitting a multi-byte character produces
/// a body that is not valid UTF-8, which the API rejects — turning a message that was merely long
/// into one that does not arrive at all.
///
/// The tail marker is part of the budget, not added after it. A clip that then overflows the limit
/// by the length of its own marker is a bug that only shows up on exactly-sized input.
pub fn fit(text: &str, max: usize) -> (String, bool) {
    if text.chars().count() <= max {
        return (text.to_owned(), false);
    }
    const TAIL: &str = "… (clipped)";
    let room = max.saturating_sub(TAIL.chars().count());
    let kept: String = text.chars().take(room).collect();
    (format!("{kept}{TAIL}"), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_never_exceeds_the_chat_budget() {
        // Six projects, forty frames each, ten seconds of wall clock — the shape from the design's
        // own test list. What must hold is that the CHAT's ceiling is respected, because the topics
        // those projects live in do not have ceilings of their own.
        let mut budgets = Budgets::default();
        let start = Instant::now();
        let chat = -1001;

        let mut allowed = 0;
        let mut refused = 0;
        for step_ms in (0..10_000).step_by(25) {
            let now = start + Duration::from_millis(step_ms);
            for _project in 0..6 {
                match budgets.take(chat, now) {
                    Ok(()) => allowed += 1,
                    Err(r) => {
                        let d = r.wait();
                        refused += 1;
                        assert!(d > Duration::ZERO, "a refusal must say when to come back");
                        assert!(
                            d < Duration::from_secs(60),
                            "the wait must be a real number"
                        );
                    }
                }
            }
        }

        // Ten seconds of a bucket that starts full: at most the initial 18, plus 18/min for ten
        // seconds, and never more than one per second.
        assert!(
            allowed <= 18 + 3,
            "the chat budget was exceeded: {allowed} sends in ten seconds"
        );
        assert!(
            allowed >= 10,
            "the budget was so tight nothing got out: {allowed}"
        );
        assert!(refused > 0, "nothing was shed, so this test proved nothing");
    }

    #[test]
    fn two_sends_in_the_same_instant_are_separated_by_the_minimum_gap() {
        let mut b = ChatBudget::default();
        let now = Instant::now();
        assert!(b.take(now).is_ok());
        let Err(refusal) = b.take(now) else {
            panic!("two sends in one instant were both allowed");
        };
        assert!(
            matches!(refusal, Refusal::Gap(_)),
            "the one-second rhythm was reported as the per-minute ceiling: {refusal:?}"
        );
        let wait = refusal.wait();
        assert!(wait <= MIN_GAP && wait > Duration::ZERO, "{wait:?}");
    }

    #[test]
    fn a_bucket_that_starts_full_still_settles_to_the_sustainable_rate() {
        // The first version of this test asserted the bucket was empty after eighteen sends. It is
        // not, and that was the test being wrong rather than the code: a token bucket refills WHILE
        // it is being spent, which is the whole reason to use one. The property worth pinning is
        // the one Telegram actually cares about — that a long run settles to the sustainable rate
        // however hard it is pushed at the start.
        let mut b = ChatBudget::default();
        let start = Instant::now();

        let minutes = 5;
        let mut allowed = 0;
        for tick in 0..(minutes * 60 * 4) {
            let now = start + Duration::from_millis(tick * 250);
            if b.take(now).is_ok() {
                allowed += 1;
            }
        }

        // The initial burst plus the sustainable rate for five minutes, and nothing beyond it.
        let ceiling = PER_MINUTE + PER_MINUTE * minutes as u32;
        assert!(
            allowed <= ceiling,
            "{allowed} sends in {minutes} minutes exceeds the ceiling of {ceiling}"
        );
        assert!(
            allowed >= PER_MINUTE * minutes as u32,
            "{allowed} sends in {minutes} minutes is below the sustainable rate; the bucket is stuck"
        );
    }

    #[test]
    fn one_project_flooding_does_not_lock_another_out_for_the_whole_minute() {
        // The bucket is shared, so a flood does slow everyone. What must not happen is a permanent
        // lockout: the refill has to keep letting messages through at the sustainable rate.
        let mut budgets = Budgets::default();
        let start = Instant::now();
        for n in 0..100 {
            let _ = budgets.take(-1001, start + Duration::from_millis(n * 10));
        }
        let mut got_through = 0;
        for n in 0..10 {
            if budgets
                .take(-1001, start + Duration::from_secs(60 + n * 4))
                .is_ok()
            {
                got_through += 1;
            }
        }
        assert!(
            got_through >= 5,
            "a flood locked the chat out afterwards: {got_through}/10"
        );
    }

    #[test]
    fn a_message_that_fits_is_left_exactly_as_it_was() {
        let (out, clipped) = fit("short enough", MAX_TEXT);
        assert_eq!(out, "short enough");
        assert!(!clipped);
    }

    #[test]
    fn a_long_message_is_clipped_within_the_limit_and_says_so() {
        let long = "x".repeat(MAX_TEXT + 500);
        let (out, clipped) = fit(&long, MAX_TEXT);
        assert!(clipped, "a clipped message must be reported as clipped");
        assert!(
            out.chars().count() <= MAX_TEXT,
            "the clip overflowed the limit by its own marker: {} chars",
            out.chars().count()
        );
        assert!(
            out.ends_with("(clipped)"),
            "the operator must be able to see something is missing"
        );
    }

    #[test]
    fn a_clip_never_splits_a_character() {
        // Splitting a multi-byte character produces a body that is not valid UTF-8, which the API
        // rejects outright — turning a long message into one that does not arrive at all.
        let emoji = "🙂".repeat(200);
        let (out, clipped) = fit(&emoji, 50);
        assert!(clipped);
        assert!(out.chars().count() <= 50);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }

    #[test]
    fn a_message_exactly_at_the_limit_is_not_clipped() {
        let exact = "y".repeat(MAX_TEXT);
        let (out, clipped) = fit(&exact, MAX_TEXT);
        assert!(!clipped, "an exactly-sized message was clipped for nothing");
        assert_eq!(out.chars().count(), MAX_TEXT);
    }
}

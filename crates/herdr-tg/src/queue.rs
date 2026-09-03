//! The outbound budget, and the clip that keeps a message inside Telegram's limits.
//!
//! # The ceiling is per CHAT, and topics do not get their own
//!
//! Telegram's group limit is about twenty messages a minute for the whole chat. Forum topics are
//! threads inside one chat, not chats, so six busy projects share one budget rather than having
//! six. Everything here follows from that: the count is kept per chat, and fairness between
//! projects has to be arranged rather than assumed.
//!
//! **Measured on 3 September 2026, and until then it was not.** This paragraph used to claim the
//! measurement had already happened and cite `docs/HUB-DESIGN.md` §12 for it; §12 describes that
//! probe in the future tense, as work to do, and no result was ever recorded. The number every send
//! is paced against rested on a citation to a plan. It happens to be right, which is why nobody
//! noticed — and being right is not the same as being checked.
//!
//! What the probe found, firing forty sends across FOUR topics of one forum as fast as they would
//! go: twenty accepted, the twenty-first refused with `429` and `retry_after: 41`. Spreading across
//! topics bought nothing, so the ceiling is per CHAT.
//!
//! # It is a WINDOW, and this file used to model a rate
//!
//! The twenty went out in the first nineteen seconds with no pacing at all, and `retry_after` then
//! counted down the remainder of the minute — 41, 40, 39. That is a trailing sixty seconds with a
//! count in it, not a rate, and `retry_after` is when the oldest send in the window ages out.
//!
//! This was a token bucket, and a bucket does not bound a window. What a bucket of capacity `C`
//! refilling at `r` allows in any sixty seconds is `C + 60r`, which here was `18 + 18 = 36` against
//! a ceiling of twenty — measured on this file's own constants at 34. The bucket bounded the
//! SUSTAINED rate, which is not the thing Telegram enforces, so the pacer whose whole job is to
//! prevent a `429` earned one inside about twenty seconds of any busy minute. Everything else in
//! this file — the flood-wait drain, the retry, the line that tells the operator — existed to
//! survive a refusal the pacer was documented as preventing.
//!
//! So the shape is now the shape that was measured: the instants of the sends inside the trailing
//! window, and a refusal when there are already [`PER_MINUTE`] of them. It is smaller than what it
//! replaced, it cannot exceed the ceiling by construction, and it makes [`Refusal::Ceiling`]'s
//! duration exact rather than an estimate — the answer is when a particular send ages out.
//!
//! `editMessageText` is NOT charged against it: thirty edits straight after seeding five messages
//! were all accepted, no `429`, and a send still went through afterwards. So taking a keyboard off
//! costs nothing here, and editing a message in place is the cheap way to say something twice.
//!
//! # Shedding is never silent
//!
//! A refused send used to be one `tracing::error!` and a drop, and that had already lost 5,164
//! characters of a real agent's longest message. Nothing here drops anything quietly: a frame that
//! cannot go out now produces a `retry_after` the caller must act on, and a message too long to
//! send is clipped **and said to be clipped**, so the ack carries `clamped` rather than `yes`.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// The width of the window Telegram counts in, measured: `retry_after` counted down the remainder
/// of the minute that began with the first send of the burst.
pub const WINDOW: Duration = Duration::from_secs(60);

/// Telegram's own ceiling for a group is 20 in a [`WINDOW`]. Sitting exactly on a limit means
/// discovering it from a 429 during an incident, so the budget is deliberately under it.
pub const PER_MINUTE: u32 = 18;

/// And no more than one a second, which is the other half of the same limit.
pub const MIN_GAP: Duration = Duration::from_millis(1000);

/// One send is held back from the agents so the hub can always say the chat is full.
///
/// **This is the way out of a recursion.** The line telling the operator that the herd is over the
/// ceiling is itself a message, and it wants a token from a budget that is by construction empty at
/// the exact moment it is worth sending — so a line that merely tried either failed to go out, or
/// went out in place of an agent's message, which is worse than saying nothing.
///
/// Holding one back closes that. An agent may never take the last send in the window, so there is
/// always one left for the one thing only the hub can say. What it costs is one of the eighteen: the
/// herd gets seventeen in any trailing minute and the hub keeps the eighteenth. That is a real
/// price and it is the right one — the alternative is a chat where the only party who can explain a
/// silence is the one that has been silenced.
pub const RESERVED_FOR_THE_OPERATOR: u32 = 1;

/// The shortest a flood wait is ever taken to be.
///
/// Telegram is believed about how long to wait, but not about zero: a `retry_after` of nothing at
/// all would drain the budget by nothing, which is a wall that does not stop anybody walking into
/// it, and a caller told to come back immediately comes back immediately.
pub const MIN_FLOOD_WAIT: Duration = Duration::from_secs(1);

/// Who is spending, and therefore whether the last send in the window is theirs to take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spender {
    /// An agent's message. Cannot take the last one — see [`RESERVED_FOR_THE_OPERATOR`].
    AnAgent,
    /// The hub, saying something about the chat itself that only it can say. May take the last one,
    /// and there is one precisely because an agent may not.
    TheHub,
}

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

/// One chat's outbound budget: the sends inside the trailing [`WINDOW`], plus a minimum gap.
#[derive(Debug)]
pub struct ChatBudget {
    per_minute: u32,
    /// When each send still inside the window went out, oldest first.
    ///
    /// This is the whole of the accounting, and it is the thing that was measured rather than a
    /// model of it. Bounded by `per_minute` entries plus whatever [`Self::spend`] could not refuse,
    /// and every one of them is dropped as it ages past the window.
    sent: VecDeque<Instant>,
    last_send: Option<Instant>,
    min_gap: Duration,
    /// When Telegram itself said this chat may be written to again, if it has said so.
    ///
    /// Outranks everything else here. The count is our own record of a limit somebody else
    /// enforces; a `429` is that somebody saying the answer out loud, and the two can disagree —
    /// in the measured probe this side believed it had most of the minute left at the moment the
    /// chat was shut for forty-one seconds, because the sends that earned it were not all ours.
    blocked_until: Option<Instant>,
}

impl ChatBudget {
    pub fn new(per_minute: u32, min_gap: Duration) -> Self {
        Self {
            per_minute,
            sent: VecDeque::new(),
            last_send: None,
            min_gap,
            blocked_until: None,
        }
    }

    /// Try to spend one send.
    ///
    /// `Err(Refusal)` is a real instruction, not advice: sending anyway is how a bot earns a 429,
    /// and a 429 on a shared bot punishes every project rather than the one that caused it.
    ///
    /// `now` is expected to move forward across calls — see [`Self::write_it_down`] for what
    /// happens when two callers read the clock and then race for this lock.
    pub fn take(&mut self, now: Instant, who: Spender) -> Result<(), Refusal> {
        if let Some(refusal) = self.would_refuse(now, who) {
            return Err(refusal);
        }
        self.write_it_down(now);
        Ok(())
    }

    /// Would a send be refused right now, and why? Asks WITHOUT spending.
    ///
    /// Split out because a caller that has to tell somebody when to come back should not have to
    /// take a send to find out and then contrive to give it back — a send handed back is a send two
    /// threads can disagree about.
    pub fn would_refuse(&mut self, now: Instant, who: Spender) -> Option<Refusal> {
        // Telegram's own word first, and it is the only thing here that can refuse the hub as well
        // as an agent: while the chat is shut, the reserve buys nothing, because nothing at all
        // will go out. Reported as a `Ceiling` rather than a `Gap` because it is a real limit and
        // not a rhythm — a caller that mistakes it for a beat spends its whole shelf life sleeping
        // a second at a time.
        if let Some(until) = self.blocked_until {
            if now < until {
                return Some(Refusal::Ceiling(until - now));
            }
            self.blocked_until = None;
        }
        self.forget_what_has_aged_out(now);

        if let Some(last) = self.last_send {
            let since = now.saturating_duration_since(last);
            if since < self.min_gap {
                return Some(Refusal::Gap(self.min_gap - since));
            }
        }
        let room = self.room_for(who);
        if self.sent.len() >= room {
            // EXACT, rather than an estimate: what has to happen before this spender has room is
            // that one particular earlier send ages out of the window, and its instant is written
            // down. A caller told the real number can wait the real wait; the arithmetic this
            // replaced was a rate divided into a token deficit, which named a moment nothing was
            // going to happen at.
            let must_age_out = self.sent.len() - room;
            let wait = match self.sent.get(must_age_out) {
                Some(&t) => (t + WINDOW).saturating_duration_since(now),
                // Room of nothing at all: this spender can never send into this chat, whatever
                // ages out. Only reachable with a budget smaller than the reserve, which is a
                // test's doing — and a whole window is the honest answer to "come back when".
                None => WINDOW,
            };
            // Never zero. A caller told to come back immediately comes back immediately, and does
            // the same thing again.
            return Some(Refusal::Ceiling(wait.max(Duration::from_millis(1))));
        }
        None
    }

    /// Telegram refused for flooding and said how long. Believe it over our own accounting.
    ///
    /// **The window is left FULL, dated so that it ages out from the moment Telegram named.** That
    /// is the measured shape rather than a guess at one: `retry_after` is when the oldest send in
    /// Telegram's own trailing window drops out of it, so a chat coming back from a flood wait has
    /// room for one send and the rest arriving behind it. What this replaces did the opposite — it
    /// zeroed the count and restarted the clock at the moment the chat was SHUT, so nothing was
    /// spent while it was closed and the whole of the closure was credited back in one instant.
    /// Measured on the real constants: a `retry_after` of 41 handed back eleven sends the moment it
    /// cleared and twenty-nine over the following minute, against a ceiling of twenty. Backpressure
    /// that ends in a burst is not backpressure — it is the next flood wait being earned.
    ///
    /// A wait already in force is never SHORTENED by a newer one. Two sends can be in flight when
    /// the chat shuts, and the second one's answer is a second or two staler than the first's.
    pub fn flood_wait(&mut self, now: Instant, wait: Duration) {
        let asked = now + wait.max(MIN_FLOOD_WAIT);
        let until = match self.blocked_until {
            Some(already) if already >= asked => already,
            _ => asked,
        };
        self.blocked_until = Some(until);

        // Dated backwards from the reopening so the first slot frees exactly then, and the rest at
        // the sustainable rhythm behind it. A window that cannot be dated backwards — a process up
        // for less than a minute — is dated at the reopening instead, which shuts the chat for a
        // further window rather than opening it early. Fail closed in both directions.
        let first = until.checked_sub(WINDOW).unwrap_or(until);
        let step = WINDOW / self.per_minute.max(1);
        self.sent.clear();
        for k in 0..self.per_minute.saturating_sub(1) {
            self.sent.push_back(first + step * k);
        }
    }

    /// Account for a send that has already been decided and cannot be called back.
    ///
    /// The operator's own replies are like this: he tapped a button, and the confirmation of what
    /// that did is not a thing the hub may refuse him because some agent has been chatty. It is
    /// still a real message against a ceiling that belongs to the whole CHAT, so an unmetered one
    /// does not cost itself — it costs whichever project sends next, which is how a writer nobody
    /// could see turns into a project's message being shed for no reason it can name.
    pub fn spend(&mut self, now: Instant) {
        self.forget_what_has_aged_out(now);
        self.write_it_down(now);
    }

    /// How many sends this spender may have inside one window. See [`RESERVED_FOR_THE_OPERATOR`].
    fn room_for(&self, who: Spender) -> usize {
        match who {
            Spender::AnAgent => self.per_minute.saturating_sub(RESERVED_FOR_THE_OPERATOR),
            Spender::TheHub => self.per_minute,
        }
        .try_into()
        .unwrap_or(usize::MAX)
    }

    /// Drop the sends that have left the trailing window.
    fn forget_what_has_aged_out(&mut self, now: Instant) {
        while let Some(&oldest) = self.sent.front() {
            if now.saturating_duration_since(oldest) < WINDOW {
                break;
            }
            self.sent.pop_front();
        }
    }

    /// Record a send that has happened.
    ///
    /// Never dated EARLIER than one already recorded. Two tasks each read the clock and then queue
    /// for this lock, so the second to arrive can carry the earlier instant — and a deque that is
    /// not in order is one whose oldest entry is not at the front, which would age the wrong send
    /// out. The distortion is the microseconds between two lock acquisitions; the alternative is a
    /// window that quietly stops bounding anything.
    fn write_it_down(&mut self, now: Instant) {
        let at = match self.sent.back() {
            Some(&last) if last > now => last,
            _ => now,
        };
        self.sent.push_back(at);
        self.last_send = Some(at);
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

    pub fn take(&mut self, chat_id: i64, now: Instant, who: Spender) -> Result<(), Refusal> {
        self.chat(chat_id).take(now, who)
    }

    /// When could a send go out, without taking one? See [`ChatBudget::would_refuse`].
    pub fn would_refuse(&mut self, chat_id: i64, now: Instant, who: Spender) -> Option<Refusal> {
        self.chat(chat_id).would_refuse(now, who)
    }

    /// Telegram shut this chat for a while. See [`ChatBudget::flood_wait`].
    pub fn flood_wait(&mut self, chat_id: i64, now: Instant, wait: Duration) {
        self.chat(chat_id).flood_wait(now, wait);
    }

    /// Account for a send that could not be refused. See [`ChatBudget::spend`].
    pub fn spend(&mut self, chat_id: i64, now: Instant) {
        self.chat(chat_id).spend(now);
    }

    fn chat(&mut self, chat_id: i64) -> &mut ChatBudget {
        let (per_minute, min_gap) = (self.per_minute, self.min_gap);
        self.chats
            .entry(chat_id)
            .or_insert_with(|| ChatBudget::new(per_minute, min_gap))
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
                match budgets.take(chat, now, Spender::AnAgent) {
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
        assert!(b.take(now, Spender::AnAgent).is_ok());
        let Err(refusal) = b.take(now, Spender::AnAgent) else {
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
    fn no_sixty_seconds_of_a_long_run_ever_carries_more_than_telegram_will_take() {
        // The property the measurement actually named, and the one this file used to get wrong.
        // Telegram counts a trailing minute, so what has to hold is not "the average is right" —
        // a token bucket's average was right — but that no window ANYWHERE in a long push holds
        // more than the ceiling. The bucket allowed its capacity plus a minute's refill in one
        // window, measured at 34 against a ceiling of 20, which is a 429 about twenty seconds into
        // any busy minute.
        let mut b = ChatBudget::default();
        let start = Instant::now();

        let mut at: Vec<u64> = Vec::new();
        for tick in 0..(5 * 60 * 4u64) {
            let ms = tick * 250;
            if b.take(start + Duration::from_millis(ms), Spender::AnAgent)
                .is_ok()
            {
                at.push(ms);
            }
        }

        let mut worst = 0;
        let mut worst_from = 0;
        for &from in &at {
            let n = at
                .iter()
                .filter(|&&t| t >= from && t < from + WINDOW.as_millis() as u64)
                .count();
            if n > worst {
                (worst, worst_from) = (n, from);
            }
        }
        assert!(
            worst <= 20,
            "{worst} sends left in the sixty seconds starting at {}s, against a measured ceiling \
             of twenty — so the pacer earns the 429 it exists to prevent",
            worst_from / 1000
        );
        // And it is not tight to the point of uselessness: the herd still gets everything the
        // window has, minus the one held back so the hub can say the chat is full.
        let sustainable = (PER_MINUTE - RESERVED_FOR_THE_OPERATOR) * 5;
        assert!(
            at.len() as u32 >= sustainable,
            "{} sends in five minutes is below what the window allows ({sustainable}); the pacer \
             is stuck",
            at.len()
        );
    }

    #[test]
    fn a_chat_comes_out_of_a_flood_wait_without_the_burst_it_saved_up_while_it_was_shut() {
        // The one thing that must not happen on the far side of a flood wait is a burst that earns
        // the next one. This used to zero the count and restart the clock at the moment the chat
        // was SHUT, so every second of the closure was credited back the instant it lifted:
        // eleven sends ready at once after a `retry_after` of 41, and twenty-nine over the minute
        // that followed. The hub waited out Telegram's refusal honestly and then walked straight
        // into the next one.
        let mut b = ChatBudget::new(PER_MINUTE, Duration::ZERO);
        let start = Instant::now();
        b.flood_wait(start, Duration::from_secs(41));

        let reopened = start + Duration::from_secs(41);
        let mut burst = 0;
        while b.take(reopened, Spender::AnAgent).is_ok() {
            burst += 1;
        }
        assert_eq!(
            burst, 1,
            "a chat Telegram shut for forty-one seconds reopened with {burst} sends ready in the \
             same instant"
        );

        // And the minute after it stays under the ceiling too, which is the half that matters:
        // one send at the reopening and a saved-up burst a second later would be the same defect
        // wearing a different number.
        let mut b = ChatBudget::default();
        b.flood_wait(start, Duration::from_secs(41));
        let mut out = 0;
        for tick in 0..600u64 {
            if b.take(
                reopened + Duration::from_millis(tick * 100),
                Spender::AnAgent,
            )
            .is_ok()
            {
                out += 1;
            }
        }
        assert!(
            out <= 20,
            "{out} sends went out in the minute after a flood wait, over Telegram's own measured \
             ceiling of twenty"
        );
    }

    #[test]
    fn one_project_flooding_does_not_lock_another_out_for_the_whole_minute() {
        // The bucket is shared, so a flood does slow everyone. What must not happen is a permanent
        // lockout: the refill has to keep letting messages through at the sustainable rate.
        let mut budgets = Budgets::default();
        let start = Instant::now();
        for n in 0..100 {
            let _ = budgets.take(
                -1001,
                start + Duration::from_millis(n * 10),
                Spender::AnAgent,
            );
        }
        let mut got_through = 0;
        for n in 0..10 {
            if budgets
                .take(
                    -1001,
                    start + Duration::from_secs(60 + n * 4),
                    Spender::AnAgent,
                )
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
    fn there_is_always_a_send_left_for_the_hub_to_say_the_chat_is_full() {
        // The recursion at the heart of telling him anything: the line saying the chat is full is
        // itself a message, and it wants a token from a budget that is empty exactly when it is
        // worth sending. Nothing else in this file can make that token appear, so it is held back
        // from the agents instead — and what must hold is that no amount of agent traffic can ever
        // take the chat to a state where the hub cannot speak.
        let mut b = ChatBudget::new(PER_MINUTE, Duration::ZERO);
        let start = Instant::now();
        let mut agent_sends = 0;
        for tick in 0..200 {
            let now = start + Duration::from_millis(tick);
            if b.take(now, Spender::AnAgent).is_ok() {
                agent_sends += 1;
            }
            assert!(
                b.take(now, Spender::TheHub).is_ok(),
                "the agents spent the chat down to where the hub could not say so — after \
                 {agent_sends} of their messages, at tick {tick}"
            );
            // Take the hub's own back out, so the loop is measuring the agents draining it rather
            // than the two of them racing each other to the bottom. It is the last thing written
            // down, because it was the last send to be allowed.
            b.sent.pop_back();
        }
        assert!(
            agent_sends > 0,
            "the reserve was so tight that no agent could send at all"
        );
    }

    #[test]
    fn a_flood_wait_shuts_the_chat_for_as_long_as_telegram_said_and_not_less() {
        // The bucket is a guess at a limit somebody else enforces. When that somebody says the
        // answer out loud, their word wins: in the measured probe the bucket still believed it had
        // most of its tokens at the moment the chat was shut for forty-one seconds, and every send
        // it let through in that minute was refused.
        let mut b = ChatBudget::new(PER_MINUTE, Duration::ZERO);
        let start = Instant::now();
        assert!(
            b.take(start, Spender::AnAgent).is_ok(),
            "the bucket is full"
        );

        b.flood_wait(start, Duration::from_secs(41));
        for at in [0, 1, 20, 40] {
            let now = start + Duration::from_secs(at);
            let Err(Refusal::Ceiling(left)) = b.take(now, Spender::AnAgent) else {
                panic!("a send went out {at}s into a forty-one second flood wait");
            };
            assert!(
                left > Duration::ZERO && left <= Duration::from_secs(41),
                "{left:?} is not what is left of a forty-one second wait at {at}s"
            );
        }
        // Not even the hub's reserved send, because while the chat is shut nothing at all goes out
        // and pretending otherwise would spend the reserve on a message Telegram refuses.
        assert!(
            b.take(start + Duration::from_secs(5), Spender::TheHub)
                .is_err(),
            "the reserved send was spent into a chat that Telegram had shut"
        );

        assert!(
            b.take(start + Duration::from_secs(42), Spender::AnAgent)
                .is_ok(),
            "the chat never reopened after the wait Telegram named had passed"
        );
        // One, and not a saved-up minute's worth. What Telegram's own `retry_after` names is when
        // the OLDEST send in its window ages out, so exactly one slot frees at the reopening —
        // `a_chat_comes_out_of_a_flood_wait_without_the_burst_it_saved_up_while_it_was_shut` is
        // the same property counted rather than sampled.
        assert!(
            b.take(start + Duration::from_secs(42), Spender::AnAgent)
                .is_err(),
            "the chat reopened with a burst saved up while it was shut, which earns the next 429"
        );
    }

    #[test]
    fn a_second_flood_wait_never_shortens_one_already_in_force() {
        // Two sends can be in flight when the chat shuts, and the second one's answer is a second
        // or two staler than the first's. Taking the newer number would reopen the chat early, on
        // the strength of the older news.
        let mut b = ChatBudget::default();
        let start = Instant::now();
        b.flood_wait(start, Duration::from_secs(41));
        b.flood_wait(start + Duration::from_millis(200), Duration::from_secs(30));
        assert!(
            b.take(start + Duration::from_secs(35), Spender::AnAgent)
                .is_err(),
            "a staler, shorter flood wait reopened the chat while the first one was still running"
        );
    }

    #[test]
    fn a_flood_wait_of_no_seconds_at_all_still_stops_somebody_walking_into_it() {
        // Telegram is believed about how long to wait, and not about zero: a wait of nothing is a
        // wall that stops nobody, and a caller told to come back immediately does.
        let mut b = ChatBudget::default();
        let start = Instant::now();
        b.flood_wait(start, Duration::ZERO);
        assert!(
            b.take(start, Spender::AnAgent).is_err(),
            "a flood wait that named no seconds let the very next send through"
        );
    }

    #[test]
    fn a_send_that_could_not_be_refused_still_comes_out_of_the_chats_budget() {
        // The operator's own replies — the confirmation under a tap, the answer to a command — are
        // not refusable: he did something and the answer to it is not an agent's message to be
        // rationed. They are still real messages against a ceiling that belongs to the whole chat,
        // so an unmetered one does not cost itself, it costs whichever project sends next.
        let mut a = Budgets::new(PER_MINUTE, Duration::ZERO);
        let mut b = Budgets::new(PER_MINUTE, Duration::ZERO);
        let now = Instant::now();
        for _ in 0..5 {
            b.spend(-1001, now);
        }
        let mut left_in_a = 0;
        let mut left_in_b = 0;
        for _ in 0..PER_MINUTE {
            if a.take(-1001, now, Spender::AnAgent).is_ok() {
                left_in_a += 1;
            }
            if b.take(-1001, now, Spender::AnAgent).is_ok() {
                left_in_b += 1;
            }
        }
        assert_eq!(
            left_in_b,
            left_in_a - 5,
            "five sends the hub could not refuse cost the agents nothing, so the ceiling cannot \
             see them"
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

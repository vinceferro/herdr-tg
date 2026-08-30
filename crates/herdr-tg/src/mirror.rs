//! Streaming what an agent says into its topic, so a session can be picked up mid-flight.
//!
//! # Why this is the piece that makes handoff work
//!
//! Without it a topic is an alert channel: it goes quiet exactly while you are working and speaks
//! only when something breaks. Opening it on a phone then shows the last alarm, not the session.
//! The operator's words: *"you need to stream messages back here on the proper chats, otherwise I
//! can't be hand off."*
//!
//! # The hard part: a pane is a screen, not a log
//!
//! herdr hands back the rendered viewport. There is no "give me what is new" — the same read
//! returns the same 60 rows whether the agent wrote nothing or scrolled a page. So "what is new"
//! has to be derived by comparing two reads, and the comparison has to survive scrolling, redraws,
//! and a spinner rewriting one line thirty times a second.
//!
//! [`new_since`] does it by anchoring: find where the previous screen's last real line appears in
//! the new one, and take everything after it. When the anchor is gone the screen has scrolled past
//! what we last saw, and the whole new screen is new.
//!
//! # What is deliberately not relayed
//!
//! "Mirror everything" is the wrong answer — a pane emits thousands of lines an hour and relayed
//! raw it becomes a firehose that gets muted, which is worse than an alert channel. So:
//!
//! - **prose only.** Tool output, diffs, and command lines are available in the pane and are most
//!   of the volume. [`crate::voice`] already knows the difference.
//! - **at a pause, not per token.** A screen that is still changing is a screen mid-thought. The
//!   relay waits for it to settle, which is also what makes one message out of one paragraph.
//! - **nothing already sent.** The anchor handles the common case; a hash of the last relayed text
//!   catches a redraw that reprints the same paragraph.

use std::collections::BTreeMap;

/// The shortest run of new text worth a message. Below this it is a spinner tick or a prompt
/// redraw, not something an agent said.
const MIN_RELAY_CHARS: usize = 40;

/// Tracks what each pane's screen looked like, so the next read can be diffed against it.
#[derive(Debug, Default)]
pub struct Mirror {
    /// pane id → the last cleaned screen we saw.
    seen: BTreeMap<String, String>,
    /// pane id → the screen we saw one tick earlier, for deciding the pane has settled.
    prev_tick: BTreeMap<String, String>,
    /// pane id → hash of the last text relayed, so a redraw does not repeat it.
    relayed: BTreeMap<String, u64>,
}

impl Mirror {
    /// Offer a fresh screen; get back the text worth sending, if any.
    ///
    /// Returns `None` while the pane is still changing — a settled screen is the signal that the
    /// agent finished a thought, and it is what turns a stream of redraws into one message.
    pub fn observe(&mut self, pane: &str, cleaned: &str) -> Option<String> {
        let previous_tick = self.prev_tick.insert(pane.to_string(), cleaned.to_string());

        // Still moving? Wait. This is the debounce, and it is per pane.
        if previous_tick.as_deref() != Some(cleaned) {
            return None;
        }

        let baseline = self.seen.get(pane).cloned().unwrap_or_default();
        if baseline == cleaned {
            return None;
        }

        let fresh = new_since(&baseline, cleaned);
        self.seen.insert(pane.to_string(), cleaned.to_string());

        let fresh = fresh?;
        let worth = worth_relaying(&fresh)?;

        // A redraw can reprint a paragraph we already sent; the anchor cannot see that.
        let h = hash(&worth);
        if self.relayed.get(pane) == Some(&h) {
            return None;
        }
        self.relayed.insert(pane.to_string(), h);
        Some(worth)
    }

    /// Seed a pane without relaying, so the first tick after startup does not dump the whole
    /// screen into the topic as though it had just been said.
    pub fn prime(&mut self, pane: &str, cleaned: &str) {
        self.seen.insert(pane.to_string(), cleaned.to_string());
        self.prev_tick.insert(pane.to_string(), cleaned.to_string());
    }

    /// Forget panes that have left the herd.
    pub fn retain(&mut self, alive: &std::collections::BTreeSet<String>) {
        self.seen.retain(|p, _| alive.contains(p));
        self.prev_tick.retain(|p, _| alive.contains(p));
        self.relayed.retain(|p, _| alive.contains(p));
    }
}

/// What appeared on `now` that was not on `prev`.
///
/// Anchored on the previous screen's last substantial line: everything after that line in the new
/// screen is new. If the anchor is gone the screen scrolled past it, and everything visible counts
/// as new — better to relay a little context twice than to lose what the agent said.
pub fn new_since(prev: &str, now: &str) -> Option<String> {
    let now_lines: Vec<&str> = now.lines().collect();
    if now_lines.is_empty() {
        return None;
    }
    let anchor = prev
        .lines()
        .rev()
        .find(|l| l.trim().chars().count() >= 8)
        .map(str::trim);

    let start = match anchor {
        None => 0,
        Some(a) => match now_lines.iter().rposition(|l| l.trim() == a) {
            Some(i) => i + 1,
            // Scrolled past what we last saw.
            None => 0,
        },
    };
    let fresh = now_lines[start..].join("\n");
    let fresh = fresh.trim();
    (!fresh.is_empty()).then(|| fresh.to_string())
}

/// Is this worth putting in a chat, and what part of it?
///
/// Keeps the prose and drops the rest, rather than rejecting a block because it contains one
/// command line — an agent explaining itself around a snippet is the normal case.
fn worth_relaying(fresh: &str) -> Option<String> {
    let kept: Vec<&str> = fresh
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && crate::voice::looks_like_prose(t)
        })
        .collect();
    if kept.is_empty() {
        return None;
    }
    let text = kept.join("\n");
    (text.chars().count() >= MIN_RELAY_CHARS).then_some(text)
}

fn hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "w1:p1";

    /// The core: a settled screen with new prose on it produces one message.
    #[test]
    fn new_prose_on_a_settled_screen_is_relayed_once() {
        let mut m = Mirror::default();
        let first = "Checking each theme for hue collisions.";
        m.prime(P, first);

        let second = "Checking each theme for hue collisions.\n\
                      retro-82 has blue equal to magenta, so no collision-free set exists.";
        assert!(
            m.observe(P, second).is_none(),
            "must wait for the screen to settle"
        );
        let out = m.observe(P, second).expect("settled, with new prose");
        assert!(out.contains("retro-82"));
        assert!(
            !out.contains("Checking each theme"),
            "resent what was already sent: {out}"
        );

        // Offered again unchanged, it says nothing.
        assert!(m.observe(P, second).is_none());
    }

    /// A screen still changing is a screen mid-thought. Relaying every redraw is the firehose this
    /// exists to avoid.
    #[test]
    fn a_moving_screen_is_never_relayed() {
        let mut m = Mirror::default();
        m.prime(P, "start");
        for frame in [
            "⠋ thinking about the palette problem now",
            "⠙ thinking about the palette problem now",
            "⠹ thinking about the palette problem now",
        ] {
            assert!(
                m.observe(P, frame).is_none(),
                "relayed a moving frame: {frame}"
            );
        }
    }

    /// Startup must not dump a screenful of history into the topic as though it had just been said.
    #[test]
    fn priming_relays_nothing() {
        let mut m = Mirror::default();
        let screen = "A long-running session that has been going for hours and hours already.";
        m.prime(P, screen);
        assert!(m.observe(P, screen).is_none());
    }

    /// Tool output is most of the volume and none of the conversation.
    #[test]
    fn output_is_not_relayed_even_when_it_is_new() {
        let mut m = Mirror::default();
        m.prime(P, "before");
        let noisy = "before\n$ cargo test --workspace\n   Compiling herdr-tg v0.1.0\n\
                     +  return resolve(cls)\nthread 'main' panicked at src/lib.rs:42:9";
        assert!(m.observe(P, noisy).is_none());
        assert!(m.observe(P, noisy).is_none(), "output reached the chat");
    }

    /// An agent explaining itself around a snippet is the normal case — keep the words.
    #[test]
    fn prose_around_a_snippet_keeps_the_prose() {
        let mut m = Mirror::default();
        m.prime(P, "x");
        let mixed = "x\nI rewrote the resolver so the class decides the chain.\n\
                     +  return resolve(cls)\nThat removes the last hard-coded model name.";
        m.observe(P, mixed);
        let out = m.observe(P, mixed).expect("prose present");
        assert!(out.contains("rewrote the resolver"));
        assert!(out.contains("last hard-coded"));
        assert!(
            !out.contains("return resolve"),
            "code leaked into the message: {out}"
        );
    }

    #[test]
    fn a_short_flicker_is_below_the_relay_threshold() {
        let mut m = Mirror::default();
        m.prime(P, "working");
        let tiny = "working\nok";
        m.observe(P, tiny);
        assert!(m.observe(P, tiny).is_none());
    }

    /// When the screen scrolls past everything we last saw, the anchor is gone; relaying the whole
    /// visible screen is better than losing what was said.
    #[test]
    fn a_scrolled_screen_relays_what_is_visible() {
        assert_eq!(
            new_since(
                "an old line that has scrolled away",
                "a completely different screen now"
            )
            .as_deref(),
            Some("a completely different screen now")
        );
    }

    #[test]
    fn the_anchor_finds_the_boundary() {
        let prev = "line one is here\nline two is here";
        let now = "line one is here\nline two is here\nline three is new";
        assert_eq!(new_since(prev, now).as_deref(), Some("line three is new"));
    }

    /// A redraw can reprint a paragraph the anchor cannot recognise as old.
    #[test]
    fn the_same_paragraph_printed_twice_is_sent_once() {
        let mut m = Mirror::default();
        m.prime(P, "");
        let para = "I have finished the migration and every test is green now.";
        m.observe(P, para);
        assert!(m.observe(P, para).is_some());

        // The screen churns, then redraws the same paragraph.
        m.observe(P, "⠋ working");
        m.observe(P, "⠋ working");
        m.observe(P, para);
        assert!(
            m.observe(P, para).is_none(),
            "the same paragraph was sent twice"
        );
    }

    #[test]
    fn panes_that_left_the_herd_are_forgotten() {
        let mut m = Mirror::default();
        m.prime(P, "x");
        m.prime("w2:p1", "y");
        m.retain(&["w2:p1".to_string()].into_iter().collect());
        assert!(!m.seen.contains_key(P));
        assert!(m.seen.contains_key("w2:p1"));
    }
}

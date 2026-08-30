//! Every word the bridge says to the operator, and the rules those words follow.
//!
//! # Why this is a module and not a habit
//!
//! The messages grew one at a time, each written next to the code that sent it, and the result read
//! like a status feed rather than a conversation: `sent → wA:p1 (this topic's pane)`. The operator's
//! verdict was "a bit of a cryptic one", and the fix is not better strings in twelve places — it is
//! one place with rules a test can enforce. Anything the operator reads is built here.
//!
//! # The rules
//!
//! 1. **Never repeat what the container already says.** Inside a pane's topic the operator knows
//!    which pane they are in; restating the id is noise that makes every message longer and less
//!    like a chat. In flat mode the same message must name the pane, because nothing else does.
//! 2. **Lead with the outcome.** The first word says what happened. Detail comes after, and only
//!    when something needs attention.
//! 3. **No implementation vocabulary.** The operator never sees "rung", "RPC", "pane_id", "seq",
//!    "socket", "subscription". They are reading a message from an agent, not a log line.
//! 4. **Say less when it went well.** A clean success is one word. Length is for problems.
//! 5. **Relay words, not a screen.** An agent's prose is a message and is sent as one. A code
//!    block is for things that are actually code — a diff, a command, a stack trace. Wrapping
//!    prose in `<pre>` turns a sentence into a screenshot, and the operator's verdict on that was
//!    that it "sends quoted code back to me, which isn't really a chat vibe".
//! 6. **Say the standing instructions once.** "Reply here and I'll type it in" belongs in the
//!    topic's opening message, not under every push. Repeated on each one it is furniture.
//! 7. **Never claim more than was observed.** This is the one rule that outranks brevity: a
//!    confirmation that overstates costs the operator the time the bridge exists to save.

use crate::deliver::{Delivery, Rung};

/// Where the message will appear, which decides how much context it must carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The pane's own topic. The reader already knows which pane this is.
    Topic,
    /// One flat conversation about the whole herd. The message must say which pane it is about.
    Flat,
}

/// An agent said something, unprompted.
///
/// The plainest message the bridge sends: no marker, no header, nothing. In a session's topic this
/// is simply the agent talking, and anything added would be the bridge talking over it.
pub fn said(place: Place, workspace: &str, text: &str) -> String {
    match place {
        Place::Topic => body(text, None, ""),
        Place::Flat => format!("<b>{}</b>\n{}", esc(workspace), body(text, None, "")),
    }
}

/// An agent needs an answer.
///
/// The body is the agent's own words wherever they are words. Only genuine terminal output — a
/// diff, a command line, a stack trace — goes in a code block, because that is the only kind of
/// text a proportional font would ruin. A header and a footer are deliberately absent: in a
/// session's own topic they say nothing the reader does not already know.
pub fn asked(
    place: Place,
    workspace: &str,
    excerpt: &str,
    has_options: bool,
    gist: Option<&str>,
) -> String {
    let mut m = String::new();
    // In flat mode nothing else says which session this is, so it is named. In a topic it is noise.
    if place == Place::Flat {
        m.push_str(&format!("🔴 <b>{}</b>\n", esc(workspace)));
    }
    m.push_str(&body(excerpt, gist, "🔴 "));
    if has_options {
        m.push_str("\n<i>Tap one below.</i>");
    }
    m
}

/// An agent finished and is waiting to be looked at.
pub fn finished(place: Place, workspace: &str, excerpt: &str) -> String {
    let mut m = String::new();
    if place == Place::Flat {
        m.push_str(&format!("✅ <b>{}</b>\n", esc(workspace)));
    }
    m.push_str(&body(excerpt, None, "✅ "));
    m
}

/// Render what the agent left on screen as a message.
///
/// The gist, when there is one, IS the message — it is the agent's question in one line, which is
/// what a person would have typed. The raw tail follows only when it adds something: as prose if it
/// reads as prose, and in a code block only if it is genuinely output.
fn body(excerpt: &str, gist: Option<&str>, mark: &str) -> String {
    let tail = excerpt.trim();
    let gist = gist.map(str::trim).filter(|g| !g.is_empty());

    let mut m = String::new();
    if let Some(g) = gist {
        m.push_str(&format!("{mark}{}", esc(g)));
        if tail.is_empty() || covered_by(g, tail) {
            return m;
        }
        m.push('\n');
        m.push('\n');
    } else if tail.is_empty() {
        return format!("{mark}<i>waiting on you</i>");
    } else {
        m.push_str(mark);
    }

    if looks_like_prose(tail) {
        m.push_str(&esc(tail));
    } else {
        m.push_str(&format!("<pre>{}</pre>", esc(tail)));
    }
    m
}

/// Would repeating the tail just restate the gist?
///
/// A one-line ask summarised into one line is the same sentence twice. Cheap check: if the tail is
/// short and shares most of its words with the gist, the gist alone says it.
fn covered_by(gist: &str, tail: &str) -> bool {
    if tail.lines().filter(|l| !l.trim().is_empty()).count() > 2 {
        return false;
    }
    let words = |s: &str| -> std::collections::BTreeSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(str::to_string)
            .collect()
    };
    let (g, t) = (words(gist), words(tail));
    if t.is_empty() {
        return true;
    }
    let shared = g.intersection(&t).count();
    shared * 2 >= t.len()
}

/// Does the line carry a `file.ext:line` or `file.ext:line:col` reference?
///
/// A stack trace reads as prose by every other measure — mostly letters, few symbols — and the
/// source location is what gives it away. Missed on the first pass, which sent a panic message as
/// a chat message.
fn has_source_location(line: &str) -> bool {
    for tok in line.split_whitespace() {
        let parts: Vec<&str> = tok.split(':').collect();
        if parts.len() >= 2
            && parts[0].contains('.')
            && parts[1..]
                .iter()
                .take(2)
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            return true;
        }
    }
    false
}

/// Is this the agent talking, or is it terminal output?
///
/// Prose gets sent as a message; output gets a code block. Judged on shape rather than content:
/// output is dense in punctuation and path-like tokens and light on sentences, and its lines rarely
/// end the way a sentence does.
pub fn looks_like_prose(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return false;
    }
    // Anything that opens like a shell, a diff or a log is output, whatever else it looks like.
    for l in &lines {
        if l.starts_with(['$', '+', '-', '#', '>', '|', '/'])
            || l.contains("::")
            || l.contains("@@")
            || has_source_location(l)
        {
            return false;
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let letters = chars.iter().filter(|c| c.is_alphabetic()).count();
    let symbols = chars
        .iter()
        .filter(|c| !c.is_alphanumeric() && !c.is_whitespace() && !",.'\"?!:;()-—…".contains(**c))
        .count();
    // Real sentences are mostly letters, and carry little of the punctuation code needs.
    letters * 2 > chars.len() && symbols * 12 < chars.len()
}

/// An agent went back to work after being stuck.
pub fn resumed(place: Place, workspace: &str) -> String {
    match place {
        Place::Topic => "▶️ <i>Back to work</i>".to_string(),
        Place::Flat => format!("▶️ <i>{} is back to work</i>", esc(workspace)),
    }
}

/// The greeting in a freshly created topic.
pub fn topic_opened(workspace: &str, agent: &str) -> String {
    format!(
        "<b>{}</b> — {}\n<i>Anything you send here goes to this session. Reply to a message to \
         answer it.</i>",
        esc(workspace),
        esc(agent)
    )
}

/// What happened to a reply that was typed into a pane.
///
/// Rule 5 lives here. Only [`Rung::Acted`] may sound certain; everything below it says what was and
/// was not seen, because an operator told "sent" while their words sit unsubmitted will wait on an
/// agent that never read them.
pub fn reply_landed(place: Place, workspace: &str, d: &Delivery) -> String {
    let where_to = match place {
        Place::Topic => String::new(),
        Place::Flat => format!(" to {}", esc(workspace)),
    };
    match d.rung {
        Rung::Acted => format!("✅ <b>Sent{where_to}</b> — it picked it up."),
        Rung::Submitted => format!("✅ <b>Sent{where_to}.</b>"),
        Rung::Echoed => format!(
            "⚠️ <b>Sent{where_to}, but it didn't go through.</b>\nYour text is sitting in the \
             input box. The agent may use a different key to submit."
        ),
        Rung::Accepted => format!(
            "⚠️ <b>Sent{where_to}, but nothing changed on screen.</b>\nThe agent may not have \
             taken it. Worth a look."
        ),
    }
}

/// What happened to a tapped or named option.
pub fn choice_made(place: Place, workspace: &str, option: &str, closed: bool) -> String {
    let where_to = match place {
        Place::Topic => String::new(),
        Place::Flat => format!(" in {}", esc(workspace)),
    };
    if closed {
        format!("✅ <b>{}{where_to}.</b>", esc(option))
    } else {
        format!(
            "⚠️ <b>{}{where_to} — but the prompt is still up.</b>\nIt may not have registered.",
            esc(option)
        )
    }
}

/// Nothing was sent, and why. Always followed by what the operator can do instead.
pub fn nothing_sent(reason: Reason) -> String {
    match reason {
        Reason::NoTarget => "I don't know which session you mean.\n<i>Open a session's topic, or \
             use /panes to pick one.</i>"
            .to_string(),
        Reason::TargetGone => "That session isn't running any more.\n<i>Use /panes to pick \
             another.</i>"
            .to_string(),
        Reason::UnclearChoice(options) => {
            let mut m = "I didn't catch which one you meant.".to_string();
            if !options.is_empty() {
                m.push_str("\n<b>Tap one:</b>");
                for o in &options {
                    m.push_str(&format!("\n · {}", esc(o)));
                }
            }
            m
        }
        Reason::NoAudit => "I couldn't write the record of what I was about to type, so I didn't \
             type it."
            .to_string(),
        Reason::HerdUnreachable => "I can't reach the herd right now, so nothing was sent.\n<i>It \
             will reconnect on its own.</i>"
            .to_string(),
        Reason::UnreadablePrompt => "I didn't type that: that session is showing a menu, and I \
             can't tell which option is highlighted.\n<i>Answer it at the keyboard — anything I \
             sent would press whatever is selected.</i>"
            .to_string(),
    }
}

/// Why nothing was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    NoTarget,
    TargetGone,
    UnclearChoice(Vec<String>),
    NoAudit,
    HerdUnreachable,
    /// The pane is showing a menu whose highlight could not be read — or could not be looked at
    /// at all. Nothing may be typed there, because the confirm key would press an unknown option.
    UnreadablePrompt,
}

/// Escape the three characters Telegram's HTML mode treats as markup.
fn esc(s: &str) -> String {
    crate::render::escape_html(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words that belong in a log, never in a message to a human.
    ///
    /// Checked by a test rather than by review, because this is exactly the kind of rule that holds for
    /// a month and then quietly stops.
    const JARGON: [&str; 10] = [
        "rung",
        "RPC",
        "pane_id",
        "state_change_seq",
        "socket",
        "subscription",
        "envelope",
        "protocol",
        "serde",
        "debounce",
    ];

    use herdr_client::PaneId;

    fn delivery(rung: Rung) -> Delivery {
        Delivery {
            pane: PaneId::new("wA:p1"),
            rung,
            detail: "internal detail that must never reach the operator".into(),
        }
    }

    /// Every message the operator can receive, so the rules below are checked against all of them
    /// rather than the two someone remembered.
    fn every_message(place: Place) -> Vec<String> {
        let mut all = vec![
            asked(place, "omarchy-lab", "Force-push? [y/N]", false, None),
            asked(place, "omarchy-lab", "Allow access?", true, None),
            asked(
                place,
                "omarchy-lab",
                "Force-push? [y/N]",
                false,
                Some("Force-push and drop 2 commits?"),
            ),
            finished(place, "omarchy-lab", "Done: 3 files changed."),
            resumed(place, "omarchy-lab"),
            topic_opened("omarchy-lab", "opencode"),
            choice_made(place, "omarchy-lab", "Reject", true),
            choice_made(place, "omarchy-lab", "Reject", false),
            nothing_sent(Reason::NoTarget),
            nothing_sent(Reason::TargetGone),
            nothing_sent(Reason::NoAudit),
            nothing_sent(Reason::HerdUnreachable),
            nothing_sent(Reason::UnreadablePrompt),
            nothing_sent(Reason::UnclearChoice(vec![
                "Allow once".into(),
                "Reject".into(),
            ])),
        ];
        for r in [Rung::Acted, Rung::Submitted, Rung::Echoed, Rung::Accepted] {
            all.push(reply_landed(place, "omarchy-lab", &delivery(r)));
        }
        all
    }

    /// Rule 3. The operator is reading a message from an agent, not a log line.
    #[test]
    fn no_message_contains_implementation_vocabulary() {
        for place in [Place::Topic, Place::Flat] {
            for m in every_message(place) {
                let low = m.to_lowercase();
                for word in JARGON {
                    assert!(
                        !low.contains(&word.to_lowercase()),
                        "{word:?} reached the operator in: {m:?}"
                    );
                }
            }
        }
    }

    /// Rule 1. Inside a topic the reader knows which pane they are in; repeating the id is the
    /// noise that made the first version read like a status feed.
    #[test]
    fn a_topic_message_never_repeats_the_pane_id() {
        let pane_id = regex_lite_pane_id;
        for m in every_message(Place::Topic) {
            assert!(
                !pane_id(&m),
                "a pane id appeared inside its own topic: {m:?}"
            );
        }
    }

    /// The other half of rule 1: in flat mode nothing else carries the context, so a message about
    /// a specific session must name it.
    #[test]
    fn a_flat_message_about_a_session_names_it() {
        for m in [
            asked(Place::Flat, "omarchy-lab", "x", false, None),
            finished(Place::Flat, "omarchy-lab", "x"),
            resumed(Place::Flat, "omarchy-lab"),
            reply_landed(Place::Flat, "omarchy-lab", &delivery(Rung::Submitted)),
            choice_made(Place::Flat, "omarchy-lab", "Reject", true),
        ] {
            assert!(m.contains("omarchy-lab"), "no session named in: {m:?}");
        }
    }

    /// Rule 5, and the one that outranks brevity. Only the top rung may sound certain.
    #[test]
    fn only_the_strongest_outcome_sounds_certain() {
        for place in [Place::Topic, Place::Flat] {
            let acted = reply_landed(place, "w", &delivery(Rung::Acted));
            assert!(acted.contains("picked it up"));

            for weak in [Rung::Echoed, Rung::Accepted] {
                let m = reply_landed(place, "w", &delivery(weak));
                assert!(m.contains("⚠️"), "a doubtful outcome read as clean: {m:?}");
                assert!(
                    m.contains("didn't go through") || m.contains("nothing changed"),
                    "the doubt was not stated plainly: {m:?}"
                );
            }
        }
    }

    /// Rule 4. A clean success is short; length is reserved for problems.
    #[test]
    fn success_is_short_and_trouble_is_explained() {
        let ok = reply_landed(Place::Topic, "w", &delivery(Rung::Submitted));
        let bad = reply_landed(Place::Topic, "w", &delivery(Rung::Echoed));
        assert!(ok.len() < 40, "a clean success got wordy: {ok:?}");
        assert!(
            bad.len() > ok.len(),
            "a problem was explained in fewer words than a success"
        );
    }

    /// The internal `detail` field is for the audit log. It is written for a developer reading a
    /// file, and must never be relayed verbatim.
    #[test]
    fn internal_detail_never_reaches_the_operator() {
        for place in [Place::Topic, Place::Flat] {
            for r in [Rung::Acted, Rung::Submitted, Rung::Echoed, Rung::Accepted] {
                let m = reply_landed(place, "w", &delivery(r));
                assert!(!m.contains("internal detail"), "leaked: {m:?}");
            }
        }
    }

    /// Rule 2: the first thing on the line says what happened.
    #[test]
    fn every_message_leads_with_its_outcome() {
        for place in [Place::Topic, Place::Flat] {
            for m in every_message(place) {
                let first = m.lines().next().unwrap_or("");
                assert!(
                    first.starts_with(['🔴', '✅', '⚠', '▶', '<', 'I', 'T'])
                        || first.starts_with("▶️"),
                    "the first line buries the outcome: {first:?}"
                );
            }
        }
    }

    /// A refusal must always say what to do instead — a dead end is worse than an error.
    #[test]
    fn every_refusal_offers_a_way_forward() {
        for r in [
            Reason::NoTarget,
            Reason::TargetGone,
            Reason::UnclearChoice(vec!["Allow once".into()]),
        ] {
            let m = nothing_sent(r);
            assert!(
                m.contains("/panes") || m.contains("topic") || m.contains("Tap"),
                "a refusal with no way forward: {m:?}"
            );
        }
    }

    /// The gist is a convenience, never a replacement. A wrong paraphrase must be visible against
    /// the real text, which is why the excerpt is always sent underneath it.
    /// A gist never replaces content the tail carries and it does not.
    ///
    /// Its original tail was a one-line restatement of the gist, which the dedup rule now
    /// (correctly) collapses — so this uses a tail that genuinely adds, and the collapse case has
    /// its own test.
    #[test]
    fn a_gist_is_added_above_the_excerpt_and_never_instead_of_it() {
        let with = asked(
            Place::Topic,
            "ws",
            "Checked all six themes; retro-82 collides.\nForce-push and drop the 2 commits? [y/N]",
            false,
            Some("Force-push, losing 2 commits?"),
        );
        assert!(
            with.contains("Force-push, losing 2 commits?"),
            "no gist: {with}"
        );
        assert!(
            with.contains("retro-82 collides"),
            "the excerpt was replaced by the gist: {with}"
        );
        let gist_at = with.find("Force-push, losing").unwrap();
        let excerpt_at = with.find("Checked all six").unwrap();
        assert!(gist_at < excerpt_at, "the gist must come first");

        // A gist the model returned as whitespace changes nothing.
        let without = asked(Place::Topic, "ws", "x", false, Some("   "));
        assert_eq!(without, asked(Place::Topic, "ws", "x", false, None));
    }

    /// A gist is agent-adjacent text from a model: untrusted like everything else.
    #[test]
    fn a_gist_is_escaped() {
        let m = asked(
            Place::Topic,
            "ws",
            "x",
            false,
            Some("<script>alert(1)</script>"),
        );
        assert!(!m.contains("<script>"));
    }

    // ── rule 5: words, not a screen ───────────────────────────────────────────────────────────

    /// The operator's complaint: it "sends quoted code back to me, which isn't really a chat vibe".
    /// An agent's prose must arrive as a message, not as a screenshot of a terminal.
    #[test]
    fn an_agents_prose_is_sent_as_a_message_not_a_code_block() {
        let prose = "The rebase drops 2 commits from the shipping branch.\n\nForce-push anyway?";
        let m = asked(Place::Topic, "ws", prose, false, None);
        assert!(!m.contains("<pre>"), "prose was quoted as code: {m}");
        assert!(m.contains("Force-push anyway?"));
    }

    /// The other half: real output still needs a monospace block, or a diff becomes soup.
    #[test]
    fn real_terminal_output_still_gets_a_code_block() {
        for output in [
            "--- a/src/router.ts\n+++ b/src/router.ts\n@@ -52,7 +52,9 @@",
            "$ npm install axios\n  added 3 packages",
            "thread 'main' panicked at src/lib.rs:42:9",
        ] {
            let m = asked(Place::Topic, "ws", output, false, None);
            assert!(m.contains("<pre>"), "output was sent as prose: {output:?}");
        }
    }

    /// Rule 6. Standing instructions belong in the topic's opening message; repeated under every
    /// push they are furniture.
    #[test]
    fn the_standing_instruction_is_not_repeated_on_every_push() {
        let m = asked(Place::Topic, "ws", "Force-push anyway?", false, None);
        assert!(!m.contains("Reply here"), "instruction repeated: {m}");
        assert!(!m.contains("Needs you"), "a status header survived: {m}");
        // It IS said once, when the topic opens.
        assert!(topic_opened("ws", "opencode").contains("Reply to a message"));
    }

    /// A one-line ask summarised into one line is the same sentence twice.
    #[test]
    fn a_gist_that_merely_restates_the_tail_does_not_print_it_twice() {
        let m = asked(
            Place::Topic,
            "ws",
            "Force-push anyway? [y/N]",
            false,
            Some("Force-push anyway?"),
        );
        assert_eq!(m.matches("Force-push").count(), 1, "said twice: {m}");
    }

    /// But a gist over a longer tail keeps both — the gist to read at a glance, the tail for detail.
    #[test]
    fn a_gist_over_real_context_keeps_both() {
        let tail = "Checked all six themes for hue collisions.\nretro-82 has blue == magenta.\n                    No collision-free six-key set exists.\n\nChange the base hue, or drop to five?";
        let m = asked(
            Place::Topic,
            "ws",
            tail,
            false,
            Some("Change the base hue, or drop to five?"),
        );
        assert!(m.contains("retro-82"), "the detail was dropped: {m}");
        assert!(m.find("Change the base").unwrap() < m.find("retro-82").unwrap());
    }

    #[test]
    fn a_source_location_marks_a_line_as_output() {
        assert!(has_source_location(
            "thread 'main' panicked at src/lib.rs:42:9"
        ));
        assert!(has_source_location("  at bot.rs:110"));
        assert!(!has_source_location(
            "I finished at 3pm and it took 42 minutes"
        ));
        assert!(!has_source_location("ratio 3:1"));
    }

    #[test]
    fn the_prose_test_is_not_fooled_by_either_side() {
        assert!(looks_like_prose(
            "I checked the themes and two of them collide."
        ));
        assert!(looks_like_prose("Done. 3 files changed, tests green."));
        assert!(!looks_like_prose("$ cargo test --workspace"));
        assert!(!looks_like_prose("+  return resolve(cls)"));
        assert!(!looks_like_prose("herdr_tg::bot::serve at src/bot.rs:42"));
        assert!(!looks_like_prose(""));
    }

    /// An empty tail with no gist must still say something rather than sending a bare marker.
    #[test]
    fn nothing_to_relay_still_reads_as_a_message() {
        let m = asked(Place::Topic, "ws", "", false, None);
        assert!(m.contains("waiting on you"), "{m}");
    }

    /// Agent-authored text is untrusted: a pane title can contain anything.
    #[test]
    fn relayed_text_is_escaped() {
        let m = asked(
            Place::Topic,
            "ws",
            "<b>not bold</b> & <script>",
            false,
            None,
        );
        assert!(!m.contains("<script>"));
        assert!(m.contains("&lt;b&gt;"));
    }

    /// `wA:p1`-shaped, without pulling in a regex crate for one check.
    fn regex_lite_pane_id(s: &str) -> bool {
        let b: Vec<char> = s.chars().collect();
        b.windows(4)
            .any(|w| w[0] == 'w' && w[1].is_ascii_alphanumeric() && w[2] == ':' && w[3] == 'p')
    }

    #[test]
    fn the_pane_id_detector_actually_detects_one() {
        assert!(regex_lite_pane_id("sent to wA:p1 now"));
        assert!(regex_lite_pane_id("w9:p1"));
        assert!(!regex_lite_pane_id("nothing here"));
    }

    /// A refusal is only worth sending if it says what to do instead, and there is exactly one
    /// safe answer here: go to the terminal, where the menu can be answered without guessing.
    #[test]
    fn an_unreadable_prompt_tells_the_operator_to_use_the_keyboard() {
        let m = nothing_sent(Reason::UnreadablePrompt);
        assert!(
            m.starts_with("I didn't type"),
            "the outcome must come first: {m:?}"
        );
        assert!(m.contains("keyboard"), "no way out was offered: {m:?}");
        assert!(
            !regex_lite_pane_id(&m),
            "a pane id reached the operator: {m:?}"
        );
    }
}

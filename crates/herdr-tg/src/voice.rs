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
//! 5. **Never claim more than was observed.** This is the one rule that outranks brevity: a
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

/// An agent needs an answer.
pub fn asked(
    place: Place,
    workspace: &str,
    excerpt: &str,
    has_options: bool,
    gist: Option<&str>,
) -> String {
    let mut m = match place {
        Place::Topic => "🔴 <b>Needs you</b>".to_string(),
        Place::Flat => format!("🔴 <b>{} needs you</b>", esc(workspace)),
    };
    // The gist goes ABOVE the excerpt and never replaces it: if a small model paraphrased the
    // question wrongly, the real text is right underneath and the operator can see that.
    if let Some(g) = gist.filter(|g| !g.trim().is_empty()) {
        m.push_str(&format!("\n{}", esc(g.trim())));
    }
    if !excerpt.is_empty() {
        m.push_str(&format!("\n\n<pre>{}</pre>", esc(excerpt)));
    }
    m.push_str(if has_options {
        "\n<i>Tap one below.</i>"
    } else {
        "\n<i>Reply here and I'll type it in.</i>"
    });
    m
}

/// An agent finished and is waiting to be looked at.
pub fn finished(place: Place, workspace: &str, excerpt: &str) -> String {
    let mut m = match place {
        Place::Topic => "✅ <b>Done</b>".to_string(),
        Place::Flat => format!("✅ <b>{} is done</b>", esc(workspace)),
    };
    if !excerpt.is_empty() {
        m.push_str(&format!("\n\n<pre>{}</pre>", esc(excerpt)));
    }
    m.push_str("\n<i>Reply here with whatever's next.</i>");
    m
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
        "<b>{}</b> — {}\n<i>Anything you send here goes to this session.</i>",
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
    #[test]
    fn a_gist_is_added_above_the_excerpt_and_never_instead_of_it() {
        let with = asked(
            Place::Topic,
            "ws",
            "Force-push and drop the 2 commits? [y/N]",
            false,
            Some("Force-push, losing 2 commits?"),
        );
        assert!(
            with.contains("Force-push, losing 2 commits?"),
            "no gist: {with}"
        );
        assert!(
            with.contains("Force-push and drop the 2 commits? [y/N]"),
            "the excerpt was replaced by the gist: {with}"
        );
        let gist_at = with.find("Force-push, losing").unwrap();
        let excerpt_at = with.find("<pre>").unwrap();
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
}

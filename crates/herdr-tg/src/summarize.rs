//! An optional one-line summary above the excerpt — never instead of it.
//!
//! # What this is for
//!
//! `docs/ASKING-FROM-A-PHONE.md` asks agents to write questions that survive the trip to a phone.
//! Agents will not always comply, and a harness nobody wrote rules for will not comply at all. A
//! small model reading the tail and saying *what is being asked* is the fix that works regardless.
//!
//! # The constraint that makes it safe
//!
//! **It may only ever ADD.** The summary is one line above the raw excerpt, which is always sent
//! unchanged underneath. If the model paraphrases the question wrongly, the operator can see that —
//! the real text is right there. A gate that *replaced* the excerpt would let a bad paraphrase send
//! them to answer a question nobody asked, and they would never know.
//!
//! Three more rules follow from the same thinking:
//!
//! - **Fail open, always.** Gateway down, slow, or answering strangely → the push goes out with no
//!   summary. A push that never arrives is far worse than an untidy one, and this is a convenience.
//! - **Short timeout.** The push is already a debounce window behind the ask; it must not also wait
//!   on inference. Measured locally: `local-coder` answers in ~0.7s, `glm-5.3-flash` in ~3s.
//! - **Never on the reply path.** D3 is that nothing interprets the operator's words before they
//!   reach a terminal. This runs agent→operator only, and there is no function here that touches
//!   anything travelling the other way.
//!
//! # Why the output is validated rather than trusted
//!
//! A model asked for one line will sometimes return a paragraph, a refusal, a markdown block, or a
//! restatement of the prompt. Any of those is worse than nothing at the top of a notification, so
//! [`plausible`] throws them away and the push goes out bare.

use std::time::Duration;

/// Where to ask, and what to ask.
#[derive(Debug, Clone)]
pub struct Summarizer {
    pub endpoint: String,
    pub model: String,
    pub key: String,
    pub timeout: Duration,
}

/// The instruction, and every clause in it is there because a model failed without it.
///
/// Measured against the operator's own panes. Three prompts produced three different failures:
///
/// - "State the decision, not the context" → the model ANSWERED the question. On a real three-way
///   choice it replied "Work E4 clean-install.", which reads as a recommendation rather than a
///   summary. Hence "restate — never answer it, never pick one of its options".
/// - Asking only for a question → generic filler on panes that were not asking at all ("Please
///   provide the necessary information..."). Hence the explicit NONE.
/// - A terse instruction → the model echoed the INSTRUCTION back as its answer. Hence the worked
///   examples, which show the shape of an answer rather than describing it.
const SYSTEM: &str = concat!(
    "You read the tail of a terminal where a coding agent runs. Decide ONE thing: is the agent ",
    "waiting for the human to answer something?\n\n",
    "If yes, restate its question in one short line (max 90 chars). Restate — never answer it, ",
    "never pick one of its options.\n",
    "If it is just working, or showing output, reply with exactly: NONE\n\n",
    "The question, if there is one, is usually on the last lines.\n\n",
    "Examples:\n",
    "  ...\n  Force-push and drop the 2 commits? [y/N]\n",
    "  -> Force-push, dropping 2 commits?\n\n",
    "  ...\n  What do you want to do — tidy the tracker, work E4, or something else?\n",
    "  -> Tidy the tracker, work E4, or something else?\n\n",
    "  ...\n  Running tests... 42 passed\n",
    "  -> NONE\n\n",
    "No preamble, no quotes, no markdown."
);

/// The longest summary worth showing. Beyond this it stops being a glance and becomes a second
/// thing to read.
const MAX_CHARS: usize = 110;

impl Summarizer {
    /// Read the configuration from the environment, or `None` if the gate is not configured.
    ///
    /// Off unless deliberately switched on. The key is its own variable rather than being read out
    /// of some other tool's config file: a bridge that goes looking through the operator's other
    /// credentials to find one it can use is not behaving well, however convenient.
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("HERDR_TG_SUMMARIZER_KEY").ok()?;
        let endpoint = std::env::var("HERDR_TG_SUMMARIZER_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8090/v1/chat/completions".into());
        // Measured on the operator's own panes: `local-coder` (~0.6s) was wrong on all three —
        // filler, an answer instead of a question, and an echo of the instruction. `glm-5.3-flash`
        // (~2s) was right on all three, including preserving every option of a three-way choice.
        // Speed is worth nothing if the line above the excerpt misleads.
        let model =
            std::env::var("HERDR_TG_SUMMARIZER_MODEL").unwrap_or_else(|_| "glm-5.3-flash".into());
        let timeout = std::env::var("HERDR_TG_SUMMARIZER_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(6000));
        Some(Self {
            endpoint,
            model,
            key,
            timeout,
        })
    }

    /// One line describing what the agent needs, or `None`.
    ///
    /// Every failure path returns `None` and the caller sends the excerpt alone. Nothing here can
    /// prevent a push.
    pub async fn one_line(&self, excerpt: &str) -> Option<String> {
        if excerpt.trim().is_empty() {
            return None;
        }
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 500,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": SYSTEM},
                {"role": "user", "content": excerpt},
            ],
        });

        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .ok()?;
        let res = client
            .post(&self.endpoint)
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await;

        let text = match res {
            Ok(r) if r.status().is_success() => r.text().await.ok()?,
            Ok(r) => {
                tracing::debug!(status = %r.status(), "the summarizer declined; pushing without it");
                return None;
            }
            Err(e) => {
                tracing::debug!(error = %e, "the summarizer is unreachable; pushing without it");
                return None;
            }
        };

        let parsed: serde_json::Value = serde_json::from_str(&text).ok()?;
        // Some models put their thinking in `reasoning_content` and leave `content` empty when the
        // token budget runs out. That is not an error, just nothing to show.
        let line = parsed
            .pointer("/choices/0/message/content")?
            .as_str()?
            .trim();

        plausible(line, excerpt)
    }
}

/// Is this actually a one-line summary, or is it something else the model decided to say?
///
/// Rejects rather than repairs. A summary that needs repairing is one the operator would have to
/// second-guess, and the excerpt below it is already the truth.
pub fn plausible(raw: &str, excerpt: &str) -> Option<String> {
    let line = raw.trim().trim_matches('"').trim();
    if line.is_empty() {
        return None;
    }
    // More than one line means it ignored the instruction, and the rest may be commentary.
    if line.lines().count() > 1 {
        return None;
    }
    if line.chars().count() > MAX_CHARS {
        return None;
    }
    // Markdown, code fences and HTML would either render wrongly or fight the message's own markup.
    if line.contains("```") || line.contains('<') || line.contains('>') {
        return None;
    }
    let low = line.to_lowercase();
    // The agreed signal for "this pane is not asking anything". Not an error — most panes are not.
    if low == "none" || low == "none." {
        return None;
    }
    // A model echoing the instruction instead of following it, which `local-coder` did verbatim.
    if low.starts_with("is the agent") || low.contains("waiting for the human to answer") {
        return None;
    }
    // Filler a model reaches for when it has nothing: it looks like a summary and says nothing.
    for filler in [
        "please provide",
        "the necessary information",
        "no question",
        "not asking",
    ] {
        if low.contains(filler) {
            return None;
        }
    }
    // A model talking about itself rather than the excerpt.
    for tell in [
        "as an ai",
        "i cannot",
        "i can't",
        "i'm sorry",
        "sorry,",
        "here is",
        "here's a",
        "summary:",
        "the agent is asking",
        "this excerpt",
    ] {
        if low.starts_with(tell) || low.contains("as an ai") {
            return None;
        }
    }
    // THE rule that matters most, and the reason it is here rather than only in the prompt: a
    // gist that ANSWERS instead of restating is worse than no gist at all. It reads as a
    // recommendation, and the operator may act on it without noticing the real options below.
    //
    // Measured: given a genuine three-way question, one model replied "Work E4 clean-install." —
    // grammatical, confident, and one of the three choices presented as if it were the summary.
    //
    // The prompt forbids this, but a prompt is the part most likely to drift or be swapped for a
    // different model. So it is also checked structurally: if the source asks, the gist must ask.
    if asks_something(excerpt) && !line.contains('?') {
        tracing::debug!(gist = %line, "the summary answered instead of restating; dropped");
        return None;
    }

    Some(line.to_string())
}

/// Does this excerpt put a question to the human?
///
/// Deliberately broad. A false positive costs a dropped gist; a false negative lets an
/// answer-shaped gist through, which is the failure this exists to prevent.
fn asks_something(excerpt: &str) -> bool {
    if excerpt.contains('?') {
        return true;
    }
    let low = excerpt.to_lowercase();
    [
        "[y/n]", "(y/n)", "y/n", "yes/no", "select", "confirm", "choose",
    ]
    .iter()
    .any(|t| low.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape the local model actually returned when this was measured.
    #[test]
    fn a_real_summary_survives() {
        assert_eq!(
            plausible(
                "Force-push and drop the 2 commits? [y/N]",
                "Force-push? [y/N]"
            )
            .as_deref(),
            Some("Force-push and drop the 2 commits? [y/N]")
        );
        assert_eq!(
            plausible(
                "  \"Allow access to ~/.local/share?\"  ",
                "Allow access? [y/N]"
            )
            .as_deref(),
            Some("Allow access to ~/.local/share?")
        );
    }

    /// Everything below is a real way a small model answers badly. Each must produce a bare push
    /// rather than a confusing first line.
    #[test]
    fn a_model_that_ignored_the_instruction_is_thrown_away() {
        for bad in [
            "",
            "   ",
            "Here is a summary of what the agent needs:",
            "Summary: the agent wants to force-push",
            "I'm sorry, I cannot summarise that.",
            "As an AI language model, I would say the agent is stuck.",
            "The agent is asking whether to force-push",
            "line one\nline two",
            // Every one of these came out of a real model against a real pane.
            "NONE",
            "none.",
            "Is the agent waiting for the human to answer something?",
            "Please provide the necessary information for the agent to complete the task.",
            "```\nforce-push?\n```",
            "<b>Force-push?</b>",
        ] {
            assert!(
                plausible(bad, "Force-push? [y/N]").is_none(),
                "this should have been rejected: {bad:?}"
            );
        }
    }

    /// The failure the operator singled out: a gist that answers rather than restates. It reads as
    /// a recommendation, and the real options are below where they may not be read.
    #[test]
    fn a_gist_that_answers_instead_of_restating_is_dropped() {
        let question = "What do you want to do — tidy the tracker, work E4 clean-install, \
                        or something else?";
        // The real bad answer, verbatim from a real model against a real pane.
        assert!(plausible("Work E4 clean-install.", question).is_none());
        assert!(plausible("Force-push.", "Force-push and drop 2 commits? [y/N]").is_none());
        assert!(plausible("Yes", "Allow access? [y/N]").is_none());

        // The good answer to the same excerpt survives, because it is still a question.
        assert_eq!(
            plausible("Tidy the tracker, work E4, or something else?", question).as_deref(),
            Some("Tidy the tracker, work E4, or something else?")
        );
    }

    /// A pane that is not asking has no question to preserve, so a declarative line is fine there.
    #[test]
    fn a_declarative_gist_is_fine_when_nothing_was_asked() {
        assert_eq!(
            plausible("Tests finished, 42 passed.", "Running tests... 42 passed").as_deref(),
            Some("Tests finished, 42 passed.")
        );
    }

    #[test]
    fn the_question_detector_is_broad_on_purpose() {
        for asking in [
            "Force-push? [y/N]",
            "Allow once / Allow always / Reject   select  confirm",
            "Choose one:",
            "yes/no",
        ] {
            assert!(asks_something(asking), "missed a question: {asking:?}");
        }
        assert!(!asks_something("Running tests... 42 passed"));
    }

    #[test]
    fn an_over_long_answer_is_thrown_away() {
        let long = "x".repeat(MAX_CHARS + 1);
        assert!(plausible(&long, "x").is_none());
        let ok = "x".repeat(MAX_CHARS);
        assert!(plausible(&ok, "x").is_some());
    }

    /// The gate is off unless deliberately switched on, and it never goes looking through other
    /// tools' credentials for a key it could use.
    #[test]
    fn the_gate_is_off_without_its_own_key() {
        // SAFETY: single-threaded test, and the variable is read only by from_env.
        unsafe { std::env::remove_var("HERDR_TG_SUMMARIZER_KEY") };
        assert!(Summarizer::from_env().is_none());
    }

    /// An empty excerpt has nothing to summarise, and must not cost a round trip.
    #[tokio::test]
    async fn an_empty_excerpt_never_calls_out() {
        let s = Summarizer {
            // Deliberately unroutable: if this were called the test would hang rather than pass.
            endpoint: "http://127.0.0.1:1/nope".into(),
            model: "m".into(),
            key: "k".into(),
            timeout: Duration::from_millis(50),
        };
        assert!(s.one_line("   \n  ").await.is_none());
    }

    /// Fail open: an unreachable gateway yields no summary and no error, so the push still goes.
    #[tokio::test]
    async fn an_unreachable_gateway_yields_no_summary_rather_than_failing() {
        let s = Summarizer {
            endpoint: "http://127.0.0.1:1/nope".into(),
            model: "m".into(),
            key: "k".into(),
            timeout: Duration::from_millis(200),
        };
        assert!(s.one_line("Force-push? [y/N]").await.is_none());
    }
}

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

/// The instruction. Deliberately narrow: one line, the decision rather than the context, and an
/// explicit fallback for output that is not a question at all.
const SYSTEM: &str = "You turn a terminal excerpt into ONE short line telling a person on a phone \
     what the agent needs from them. Maximum 90 characters. State the decision, not the context. \
     No preamble, no quotes, no markdown. If it is not a question, say what finished.";

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
        let model =
            std::env::var("HERDR_TG_SUMMARIZER_MODEL").unwrap_or_else(|_| "local-coder".into());
        let timeout = std::env::var("HERDR_TG_SUMMARIZER_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(4000));
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

        plausible(line)
    }
}

/// Is this actually a one-line summary, or is it something else the model decided to say?
///
/// Rejects rather than repairs. A summary that needs repairing is one the operator would have to
/// second-guess, and the excerpt below it is already the truth.
pub fn plausible(raw: &str) -> Option<String> {
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
    // A model talking about itself rather than the excerpt.
    let low = line.to_lowercase();
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
    Some(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape the local model actually returned when this was measured.
    #[test]
    fn a_real_summary_survives() {
        assert_eq!(
            plausible("Force-push and drop the 2 commits? [y/N]").as_deref(),
            Some("Force-push and drop the 2 commits? [y/N]")
        );
        assert_eq!(
            plausible("  \"Allow access to ~/.local/share?\"  ").as_deref(),
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
            "```\nforce-push?\n```",
            "<b>Force-push?</b>",
        ] {
            assert!(
                plausible(bad).is_none(),
                "this should have been rejected: {bad:?}"
            );
        }
    }

    #[test]
    fn an_over_long_answer_is_thrown_away() {
        let long = "x".repeat(MAX_CHARS + 1);
        assert!(plausible(&long).is_none());
        let ok = "x".repeat(MAX_CHARS);
        assert!(plausible(&ok).is_some());
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

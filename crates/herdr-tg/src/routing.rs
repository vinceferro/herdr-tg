//! Where an operator's reply goes, and why it can be trusted.
//!
//! # The catastrophic failure this prevents
//!
//! D3: *the catastrophic failure of a remote-control surface is your words landing in the wrong
//! terminal.* A one-letter "y" is this product's most common reply, and it is unrecoverable if it
//! lands on the wrong `[y/N]`. So routing is **deterministic** in v1 — no LLM sits between the
//! operator's words and a pane. Deterministic routing can be *stale*, but it can never *misread*,
//! and stale is a failure the operator can see coming because the target is named every time.
//!
//! # The rules, in priority order
//!
//! 0. **The topic wins, when there is one.** In a forum group each pane has its own topic, so the
//!    conversation you are looking at *is* the target. Nothing to aim, nothing to remember, and no
//!    way for a reply to land somewhere you were not looking. This is the strongest rule and it
//!    comes first.
//! 1. **Reply-to wins.** If the message is a Telegram reply to one of the bridge's own pushes, the
//!    target is the pane that push was about. This is the only rule that needs no memory and cannot
//!    go stale, so it comes first.
//! 2. **Otherwise, the sticky target.** Set by a switcher tap, or by the last pane the bridge
//!    pushed about.
//! 3. **Otherwise, nothing.** The bridge asks rather than guesses. There is no "most recent pane"
//!    fallback: a guess that is right nine times teaches the operator to trust the tenth.
//!
//! A target that no longer exists is [`Target::Gone`], never silently re-pointed — PLAN.md's
//! failure table is explicit that a dead sticky target must produce a picker, not a reroute.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use herdr_client::{PaneId, SessionSnapshot};
use serde::{Deserialize, Serialize};

/// What the bridge decided to do with a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Send it here. Carries how the decision was made, so the confirmation can say so.
    Pane { pane: PaneId, why: Why },
    /// There is a sticky target, but that pane is no longer in the herd.
    Gone { pane: PaneId },
    /// Nothing to route to. Ask.
    None,
}

/// How a target was chosen. Shown to the operator, because a routing decision they cannot see is a
/// routing decision they cannot correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The message was sent inside a pane's own topic.
    Topic,
    /// The operator replied to a specific push.
    ReplyTo,
    /// The remembered target.
    Sticky,
}

/// Per-chat routing state, persisted so a restart does not lose the operator's target.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Routing {
    /// chat id → sticky pane id.
    sticky: BTreeMap<i64, String>,
    /// forum topic id → the pane that topic is for.
    #[serde(default)]
    topics: BTreeMap<i32, String>,
    /// message id of a bridge push → the pane it was about.
    ///
    /// Bounded: Telegram message ids are monotonic per chat, so the oldest entries are the least
    /// useful, and an unbounded map in a long-running bridge is a slow leak.
    pushes: BTreeMap<i64, String>,
}

/// How many push→pane mappings to remember. Beyond this the oldest are dropped.
#[allow(dead_code, reason = "used by record_push, which the push loop calls")]
const MAX_PUSH_MEMORY: usize = 500;

impl Routing {
    /// Remember that `message_id` was a push about `pane`, so a reply to it routes there.
    ///
    /// Called by the push loop, which lands with the notifier. The reply-to rule it feeds is
    /// already implemented and tested here, so the routing decision does not change when the
    /// pushes start arriving — only its inputs do.
    #[allow(
        dead_code,
        reason = "called by the push loop, which lands with the notifier"
    )]
    pub fn record_push(&mut self, message_id: i64, pane: &PaneId) {
        self.pushes.insert(message_id, pane.as_str().to_string());
        while self.pushes.len() > MAX_PUSH_MEMORY {
            let oldest = *self.pushes.keys().next().expect("non-empty");
            self.pushes.remove(&oldest);
        }
    }

    /// Bind a forum topic to a pane. One topic per pane, for the life of the pane.
    pub fn bind_topic(&mut self, thread_id: i32, pane: &PaneId) {
        self.topics.insert(thread_id, pane.as_str().to_string());
    }

    /// The topic this pane already has, if any.
    pub fn topic_for(&self, pane: &PaneId) -> Option<i32> {
        self.topics
            .iter()
            .find(|(_, p)| p.as_str() == pane.as_str())
            .map(|(t, _)| *t)
    }

    pub fn pane_for_topic(&self, thread_id: i32) -> Option<PaneId> {
        self.topics.get(&thread_id).map(|s| PaneId::new(s.clone()))
    }

    pub fn set_sticky(&mut self, chat: i64, pane: &PaneId) {
        self.sticky.insert(chat, pane.as_str().to_string());
    }

    pub fn sticky(&self, chat: i64) -> Option<PaneId> {
        self.sticky.get(&chat).map(|s| PaneId::new(s.clone()))
    }

    /// Decide where a message goes.
    ///
    /// `reply_to` is the message id this message replies to, if any. `snapshot` is the live herd —
    /// passed in rather than fetched here so the decision is a pure function and can be tested
    /// against herds this machine does not have.
    pub fn resolve(
        &self,
        chat: i64,
        reply_to: Option<i64>,
        thread_id: Option<i32>,
        snapshot: &SessionSnapshot,
    ) -> Target {
        let alive = |p: &PaneId| snapshot.panes.iter().any(|pane| pane.pane_id == *p);

        // Rule 0: the topic. Unambiguous by construction — the operator is looking at exactly one
        // pane's conversation, so there is nothing to misroute.
        if let Some(tid) = thread_id
            && let Some(pane) = self.pane_for_topic(tid)
        {
            return if alive(&pane) {
                Target::Pane {
                    pane,
                    why: Why::Topic,
                }
            } else {
                Target::Gone { pane }
            };
        }

        // Rule 1: reply-to. Needs no memory of "current" anything, so it cannot be stale.
        if let Some(mid) = reply_to
            && let Some(raw) = self.pushes.get(&mid)
        {
            let pane = PaneId::new(raw.clone());
            return if alive(&pane) {
                Target::Pane {
                    pane,
                    why: Why::ReplyTo,
                }
            } else {
                Target::Gone { pane }
            };
        }

        // Rule 2: the sticky target.
        match self.sticky(chat) {
            Some(pane) if alive(&pane) => Target::Pane {
                pane,
                why: Why::Sticky,
            },
            Some(pane) => Target::Gone { pane },
            // Rule 3: no guessing.
            None => Target::None,
        }
    }

    /// Load, tolerating absence. A missing file is a first run, not an error.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                // Losing the target is survivable — the operator retargets with one tap. Refusing
                // to start is not: it would take the bridge down over a cache.
                tracing::warn!(error = %e, path = %path.display(), "routing state unreadable; starting empty");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Save atomically: write a temp file beside the target, then rename.
    ///
    /// `Restart=always` means the bridge can be killed at any instant, including mid-write. A
    /// half-written routing file that still parses would point the operator's next reply at a
    /// truncated pane id; rename is atomic on the same filesystem, so a reader sees the old file or
    /// the new one and never a partial.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("state.json.tmp");
        let body = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, path)
    }

    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("herdr-tg").join("routing.state.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with(panes: &[&str]) -> SessionSnapshot {
        let raw = include_str!("../../herdr-client/tests/fixtures/snapshot.json");
        let env: serde_json::Value = serde_json::from_str(raw).expect("fixture parses");
        let mut snap: SessionSnapshot =
            serde_json::from_value(env["result"]["snapshot"].clone()).expect("snapshot decodes");
        snap.panes.retain(|p| panes.contains(&p.pane_id.as_str()));
        snap
    }

    fn any_pane() -> String {
        let raw = include_str!("../../herdr-client/tests/fixtures/snapshot.json");
        let env: serde_json::Value = serde_json::from_str(raw).unwrap();
        env["result"]["snapshot"]["panes"][0]["pane_id"]
            .as_str()
            .expect("the fixture has a pane")
            .to_string()
    }

    /// The strongest rule: inside a pane's topic, the conversation IS the target. Nothing to aim,
    /// and no way for a reply to land somewhere the operator was not looking.
    #[test]
    fn a_topic_beats_both_reply_to_and_sticky() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let mut r = Routing::default();
        r.bind_topic(42, &PaneId::new(a.clone()));
        r.record_push(77, &PaneId::new("wZ:p9"));
        r.set_sticky(1, &PaneId::new("wY:p8"));

        assert_eq!(
            r.resolve(1, Some(77), Some(42), &snap),
            Target::Pane {
                pane: PaneId::new(a),
                why: Why::Topic
            }
        );
    }

    #[test]
    fn a_topic_whose_pane_died_is_gone_not_rerouted() {
        let snap = snapshot_with(&[&any_pane()]);
        let mut r = Routing::default();
        r.bind_topic(42, &PaneId::new("wZ:p9"));
        assert_eq!(
            r.resolve(1, None, Some(42), &snap),
            Target::Gone {
                pane: PaneId::new("wZ:p9")
            }
        );
    }

    #[test]
    fn an_unbound_topic_falls_through_to_the_other_rules() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let mut r = Routing::default();
        r.set_sticky(1, &PaneId::new(a.clone()));
        // thread 99 is not bound to anything — must not swallow the message.
        assert_eq!(
            r.resolve(1, None, Some(99), &snap),
            Target::Pane {
                pane: PaneId::new(a),
                why: Why::Sticky
            }
        );
    }

    #[test]
    fn a_pane_keeps_one_topic() {
        let mut r = Routing::default();
        let p = PaneId::new("wA:p1");
        r.bind_topic(7, &p);
        assert_eq!(r.topic_for(&p), Some(7));
        assert_eq!(r.pane_for_topic(7), Some(p));
        assert_eq!(r.topic_for(&PaneId::new("wB:p1")), None);
    }

    #[test]
    fn reply_to_beats_sticky() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let pane = PaneId::new(a.clone());
        let mut r = Routing::default();
        r.record_push(77, &pane);
        r.set_sticky(1, &PaneId::new("wZ:p9")); // a different, dead pane

        let t = r.resolve(1, Some(77), None, &snap);
        assert_eq!(
            t,
            Target::Pane {
                pane,
                why: Why::ReplyTo
            },
            "a reply-to must win over the sticky target — it is the rule that cannot go stale"
        );
    }

    #[test]
    fn without_reply_to_the_sticky_target_is_used() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let mut r = Routing::default();
        r.set_sticky(1, &PaneId::new(a.clone()));
        assert_eq!(
            r.resolve(1, None, None, &snap),
            Target::Pane {
                pane: PaneId::new(a),
                why: Why::Sticky
            }
        );
    }

    /// THE rule that keeps a one-letter reply safe: with nothing to route to, the bridge asks.
    #[test]
    fn with_no_target_the_bridge_refuses_to_guess() {
        let snap = snapshot_with(&[&any_pane()]);
        let r = Routing::default();
        assert_eq!(
            r.resolve(1, None, None, &snap),
            Target::None,
            "there must be no most-recent-pane fallback: a guess that is right nine times teaches \
             the operator to trust the tenth"
        );
    }

    /// PLAN.md's failure table: a dead target produces a picker, never a silent reroute.
    #[test]
    fn a_target_that_left_the_herd_is_gone_not_rerouted() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let mut r = Routing::default();
        r.set_sticky(1, &PaneId::new("wZ:p9"));
        assert_eq!(
            r.resolve(1, None, None, &snap),
            Target::Gone {
                pane: PaneId::new("wZ:p9")
            }
        );

        // Same for a reply-to whose pane has since closed.
        let mut r2 = Routing::default();
        r2.record_push(5, &PaneId::new("wZ:p9"));
        assert_eq!(
            r2.resolve(1, Some(5), None, &snap),
            Target::Gone {
                pane: PaneId::new("wZ:p9")
            }
        );
    }

    #[test]
    fn each_chat_has_its_own_target() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let mut r = Routing::default();
        r.set_sticky(1, &PaneId::new(a.clone()));
        assert!(matches!(
            r.resolve(1, None, None, &snap),
            Target::Pane { .. }
        ));
        assert_eq!(r.resolve(2, None, None, &snap), Target::None);
    }

    #[test]
    fn push_memory_is_bounded() {
        let mut r = Routing::default();
        for i in 0..(MAX_PUSH_MEMORY as i64 + 50) {
            r.record_push(i, &PaneId::new("w1:p1"));
        }
        assert_eq!(r.pushes.len(), MAX_PUSH_MEMORY);
        assert!(
            !r.pushes.contains_key(&0),
            "the oldest must be dropped first"
        );
        assert!(r.pushes.contains_key(&(MAX_PUSH_MEMORY as i64 + 49)));
    }

    #[test]
    fn state_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("herdr-tg-routing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("routing.state.json");

        let mut r = Routing::default();
        r.set_sticky(878, &PaneId::new("wA:p1"));
        r.record_push(9, &PaneId::new("wB:p1"));
        r.save(&path).unwrap();

        let back = Routing::load(&path);
        assert_eq!(back.sticky(878), Some(PaneId::new("wA:p1")));
        assert_eq!(back.pushes.get(&9).map(String::as_str), Some("wB:p1"));
    }

    #[test]
    fn a_missing_or_corrupt_state_file_starts_empty_rather_than_failing() {
        let dir = std::env::temp_dir().join(format!("herdr-tg-routing-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("nope.json");
        assert!(Routing::load(&missing).sticky(1).is_none());

        let corrupt = dir.join("corrupt.json");
        std::fs::write(&corrupt, "{not json").unwrap();
        assert!(
            Routing::load(&corrupt).sticky(1).is_none(),
            "a corrupt cache must not take the bridge down — the operator retargets with one tap"
        );
    }

    /// The temp file must not be mistaken for the real one, and must be git-ignored. Slice 1 added
    /// `*.state.json.tmp` to .gitignore for exactly this.
    #[test]
    fn the_atomic_write_temp_has_the_ignored_extension() {
        let p = Path::new("/tmp/x/routing.state.json").with_extension("state.json.tmp");
        assert!(p.to_string_lossy().ends_with(".state.json.tmp"));
    }
}

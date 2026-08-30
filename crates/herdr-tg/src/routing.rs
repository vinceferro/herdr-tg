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
//! # A bare Telegram id is not an address
//!
//! Message ids and topic ids are counters that start again in every chat, so the direct chat and
//! the group both hand out low numbers and both hand out the same ones. Everything remembered here
//! is therefore filed under the chat it came from, and looked up under the chat the operator is
//! typing in. Without that, a swipe-to-reply in the direct chat — the ordinary mobile gesture —
//! finds a memory written for the group and sends the operator's words to a session they were not
//! looking at.
//!
//! # The rules, in priority order
//!
//! 0. **The topic wins, when there is one.** In the group each session has its own topic, so the
//!    conversation you are looking at *is* the target. Nothing to aim, nothing to remember, and no
//!    way for a reply to land somewhere you were not looking. This is the strongest rule and it
//!    comes first — but only inside the one group that is configured for topics. Any other
//!    supergroup numbers its reply threads with plain message ids, which are not topic ids.
//! 1. **Reply-to wins.** If the message is a Telegram reply to one of the bridge's own pushes, the
//!    target is the pane that push was about, provided that push went to this same chat. This is
//!    the rule that needs no memory of "current" anything, so it cannot go stale.
//! 2. **Otherwise, the sticky target.** Set by a switcher tap, or by the `/target` command. Not
//!    consulted in the topics group: there the topic is the aim, so a message typed in General or
//!    in a topic the bridge did not make produces a picker rather than a silent send.
//! 3. **Otherwise, nothing.** The bridge asks rather than guesses. There is no "most recent pane"
//!    fallback: a guess that is right nine times teaches the operator to trust the tenth.
//!
//! A target that no longer exists is [`Target::Gone`], never silently re-pointed — PLAN.md's
//! failure table is explicit that a dead sticky target must produce a picker, not a reroute.
//!
//! # Reading a state file written before any of this
//!
//! An older file filed its topics and its pushes under nothing but the bare id. Topic bindings are
//! given to the configured group, because that is provably the only place the bridge has ever made
//! one. Remembered pushes and remembered menus are thrown away, because nothing on disk says which
//! chat they came from and a wrong guess is exactly the misroute above.

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

/// The menu a push's buttons were drawn for.
///
/// A Telegram button carries at most 64 bytes, stays tappable forever, and is evidence about a menu
/// that was on screen at some point in the past. So the labels live HERE, next to the message the
/// buttons are attached to, and a tap is answered against this record — never against whatever the
/// pane happens to be showing when the tap arrives, which is how a button reading "Reject" came to
/// confirm "Allow always".
///
/// Kept on disk deliberately: an outstanding ask is not pushed again after a restart, so its only
/// live buttons are the ones on the message from before it. Forgetting the labels would make every
/// one of those refuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptRecord {
    /// The chat the push went to. A tap from any other chat is refused, never answered.
    pub chat: i64,
    pub pane: String,
    /// Where the pane's run of work stood when the buttons were drawn — the same key the push
    /// dedupe uses. A different value is a different question.
    ///
    /// `None` when the herd did not report it, and on a record written before this was recorded at
    /// all. Either way the tap that reads it back cannot prove the session has not moved on, so it
    /// is refused rather than answered.
    #[serde(default)]
    pub seq: Option<u64>,
    /// The labels, left to right, exactly as the buttons showed them.
    pub options: Vec<String>,
}

/// Per-chat routing state, persisted so a restart does not lose the operator's target.
///
/// Every map is filed under the chat first. Telegram numbers messages and topics separately in
/// every chat, so an id on its own says nothing about where it came from.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Routing {
    /// chat id → sticky pane id. Flat mode only (see rule 2).
    sticky: BTreeMap<i64, String>,
    /// chat id → topic id → the pane that topic is for.
    #[serde(default, rename = "topics_by_chat")]
    topics: BTreeMap<i64, BTreeMap<i32, String>>,
    /// chat id → message id of a bridge push → the pane it was about.
    #[serde(default, rename = "pushes_by_chat")]
    pushes: BTreeMap<i64, BTreeMap<i64, String>>,
    /// chat id → message id of a push that drew menu buttons → the menu they were drawn for.
    #[serde(default, rename = "prompts_by_chat")]
    prompts: BTreeMap<i64, BTreeMap<i64, PromptRecord>>,

    /// The older on-disk shape, which filed these under a bare id. Read once by [`Routing::load`],
    /// emptied by [`Routing::migrate_legacy`], and never written back — so the first save after an
    /// upgrade leaves a file with no bare-keyed memory in it at all. A plain derive reads both
    /// shapes, which is why there is no version number here: the old keys being present *is* the
    /// signal, and clearing them is what makes the move one-way and repeatable.
    #[serde(default, rename = "topics", skip_serializing)]
    legacy_topics: BTreeMap<i32, String>,
    #[serde(default, rename = "pushes", skip_serializing)]
    legacy_pushes: BTreeMap<i64, String>,
    #[serde(default, rename = "prompts", skip_serializing)]
    legacy_prompts: BTreeMap<i64, PromptRecord>,
}

/// How many push→pane mappings to remember **per chat**. Beyond this the oldest are dropped.
#[allow(dead_code, reason = "used by record_push, which the push loop calls")]
const MAX_PUSH_MEMORY: usize = 500;

/// How many drawn menus to remember **per chat**. Lower than [`MAX_PUSH_MEMORY`] because each
/// record carries a list of labels, and a button older than this is one nobody is going to tap.
const MAX_PROMPT_MEMORY: usize = 200;

impl Routing {
    /// Remember that `message_id` in `chat` was a push about `pane`, so a reply to it routes there.
    ///
    /// Called by the push loop, which lands with the notifier. The reply-to rule it feeds is
    /// already implemented and tested here, so the routing decision does not change when the
    /// pushes start arriving — only its inputs do.
    #[allow(
        dead_code,
        reason = "called by the push loop, which lands with the notifier"
    )]
    pub fn record_push(&mut self, chat: i64, message_id: i64, pane: &PaneId) {
        let per_chat = self.pushes.entry(chat).or_default();
        per_chat.insert(message_id, pane.as_str().to_string());
        // Bounded per chat, so a busy group cannot forget the direct chat's pushes out from under
        // the operator. The outer map needs no bound: only allowlisted chats are ever written.
        while per_chat.len() > MAX_PUSH_MEMORY {
            let oldest = *per_chat.keys().next().expect("non-empty");
            per_chat.remove(&oldest);
        }
    }

    /// Remember the menu the buttons on `message_id` in `chat` were drawn for.
    ///
    /// Without this a tap has nothing to answer against and is refused — which is the point. A tap
    /// must answer the question the operator actually read.
    pub fn record_prompt(
        &mut self,
        chat: i64,
        message_id: i64,
        pane: &PaneId,
        seq: Option<u64>,
        options: &[String],
    ) {
        let per_chat = self.prompts.entry(chat).or_default();
        per_chat.insert(
            message_id,
            PromptRecord {
                chat,
                pane: pane.as_str().to_string(),
                seq,
                options: options.to_vec(),
            },
        );
        while per_chat.len() > MAX_PROMPT_MEMORY {
            let oldest = *per_chat.keys().next().expect("non-empty");
            per_chat.remove(&oldest);
        }
    }

    /// The menu remembered for a message in a chat, if it is still remembered.
    pub fn prompt_for(&self, chat: i64, message_id: i64) -> Option<&PromptRecord> {
        self.prompts.get(&chat)?.get(&message_id)
    }

    /// Bind a topic in `chat` to a pane. One topic per pane, for the life of the pane.
    pub fn bind_topic(&mut self, chat: i64, thread_id: i32, pane: &PaneId) {
        self.topics
            .entry(chat)
            .or_default()
            .insert(thread_id, pane.as_str().to_string());
    }

    /// The topic this pane already has in this chat, if any.
    pub fn topic_for(&self, chat: i64, pane: &PaneId) -> Option<i32> {
        self.topics
            .get(&chat)?
            .iter()
            .find(|(_, p)| p.as_str() == pane.as_str())
            .map(|(t, _)| *t)
    }

    pub fn pane_for_topic(&self, chat: i64, thread_id: i32) -> Option<PaneId> {
        self.topics
            .get(&chat)?
            .get(&thread_id)
            .map(|s| PaneId::new(s.clone()))
    }

    pub fn set_sticky(&mut self, chat: i64, pane: &PaneId) {
        self.sticky.insert(chat, pane.as_str().to_string());
    }

    pub fn sticky(&self, chat: i64) -> Option<PaneId> {
        self.sticky.get(&chat).map(|s| PaneId::new(s.clone()))
    }

    /// Decide where a message goes.
    ///
    /// `forum` is the one group that has a topic per session, if one is configured. `reply_to` is
    /// the message id this message replies to, if any. `snapshot` is the live herd — passed in
    /// rather than fetched here so the decision is a pure function and can be tested against herds
    /// this machine does not have.
    pub fn resolve(
        &self,
        chat: i64,
        forum: Option<i64>,
        reply_to: Option<i64>,
        thread_id: Option<i32>,
        snapshot: &SessionSnapshot,
    ) -> Target {
        let alive = |p: &PaneId| snapshot.panes.iter().any(|pane| pane.pane_id == *p);
        let in_forum = forum == Some(chat);

        // Rule 0: the topic. Unambiguous by construction — the operator is looking at exactly one
        // pane's conversation, so there is nothing to misroute. Only in the configured group: every
        // other supergroup numbers its reply threads with ordinary message ids, and reading one of
        // those as a topic id would answer in whichever session happened to draw that number.
        if in_forum
            && let Some(tid) = thread_id
            && let Some(pane) = self.pane_for_topic(chat, tid)
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

        // Rule 1: reply-to, read only against pushes this same chat received. Message numbering
        // starts again in every chat, so a reply to message 20 in the direct chat must not find the
        // group's message 20.
        if let Some(mid) = reply_to
            && let Some(raw) = self.pushes.get(&chat).and_then(|per| per.get(&mid))
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

        // Rule 2: the sticky target — never in the group with topics. There the topic is the aim,
        // so honouring a sticky would let a message typed in General, or in a topic the bridge did
        // not make, land in a session the operator was not looking at: exactly the surprise rule 0
        // exists to remove. A picker is the honest answer.
        if in_forum {
            return Target::None;
        }

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
    ///
    /// `forum` is the group configured for topics right now, and it is what an older file's
    /// unfiled topic bindings are given to.
    pub fn load(path: &Path, forum: Option<i64>) -> Self {
        let mut out = match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                // Losing the target is survivable — the operator retargets with one tap. Refusing
                // to start is not: it would take the bridge down over a cache.
                tracing::warn!(error = %e, path = %path.display(), "routing state unreadable; starting empty");
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        out.migrate_legacy(forum);
        out
    }

    /// Give an older file's unfiled memory a chat, or throw it away when it cannot honestly have
    /// one. Runs on every load and is safe to run twice: it empties what it reads.
    fn migrate_legacy(&mut self, forum: Option<i64>) {
        // A sticky target in the topics group is never consulted again (rule 2). Left on disk it
        // would sit there waiting for some future reader to honour it.
        if let Some(f) = forum
            && self.sticky.remove(&f).is_some()
        {
            tracing::info!(
                chat = f,
                "dropped the remembered target for the group with topics: there the topic is the aim"
            );
        }

        let legacy_pushes = std::mem::take(&mut self.legacy_pushes);
        if !legacy_pushes.is_empty() {
            tracing::warn!(
                dropped = legacy_pushes.len(),
                "forgot which sessions some older pushes were about: those message numbers cannot \
                 be traced to a chat, and guessing would send a reply into a session the operator \
                 was not looking at. Replies to new pushes work straight away."
            );
        }

        // Menus go the same way, and for the same reason. Each record does name its own chat, but
        // it was filed under a bare message number, so two chats' menus can already have collided
        // in the file and the survivor is whichever was written last. The cost of dropping is that
        // a button on an old message says it can no longer be answered, which is the safe half.
        let legacy_prompts = std::mem::take(&mut self.legacy_prompts);
        if !legacy_prompts.is_empty() {
            tracing::warn!(
                dropped = legacy_prompts.len(),
                "forgot the menus behind some older buttons: they were filed without a chat, so \
                 tapping one now says so instead of answering. The next push draws fresh buttons."
            );
        }

        let legacy_topics = std::mem::take(&mut self.legacy_topics);
        if legacy_topics.is_empty() {
            return;
        }
        match forum {
            // Exact, not a guess: the only code that has ever bound a topic runs inside the
            // configured group. Dropping them instead would have a certain cost — within a minute
            // the bridge makes a second topic for every session, and the operator is left with two
            // of each, half of them dead.
            Some(chat) => {
                let slot = self.topics.entry(chat).or_default();
                for (tid, pane) in legacy_topics {
                    // Never over-write a binding that already names its chat.
                    slot.entry(tid).or_insert(pane);
                }
                tracing::info!(
                    chat,
                    topics = slot.len(),
                    "gave the older topic bindings to the group with topics"
                );
            }
            None => tracing::warn!(
                dropped = legacy_topics.len(),
                "dropped older topic bindings: no group with topics is configured, so there is no \
                 chat to file them under"
            ),
        }
    }

    /// Save atomically: write a temp file beside the target, then rename.
    ///
    /// `Restart=always` means the bridge can be killed at any instant, including mid-write. A
    /// half-written routing file that still parses would point the operator's next reply at a
    /// truncated pane id; rename is atomic on the same filesystem, so a reader sees the old file or
    /// the new one and never a partial.
    ///
    /// Readable only by its owner: it names the operator's chats and sessions, and on a shared
    /// machine the default would let any account read them.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        #[cfg(unix)]
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        let tmp = path.with_extension("state.json.tmp");
        let body = serde_json::to_string_pretty(self)?;
        // A temp file left by a run from before this fix is readable by everyone, and opening it
        // again with create+truncate keeps whatever it already had. Remove it and make a new one.
        let _ = std::fs::remove_file(&tmp);
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        opts.mode(0o600);
        {
            use std::io::Write;
            let mut f = opts.open(&tmp)?;
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
        }
        // Rename keeps the mode the temp file was made with.
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

    /// The operator's direct chat, and the group that has a topic per session. Both hand out low
    /// message numbers, which is the whole reason everything here is filed by chat.
    const DM: i64 = 878;
    const FORUM: i64 = -100200300;

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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "herdr-tg-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("routing.state.json")
    }

    /// The real shape an older bridge left on disk, down to the key names.
    const V1_FILE: &str = r#"{
      "sticky": {"878": "wA:p1", "-100200300": "wE:p1"},
      "topics": {"20": "wB:p1"},
      "pushes": {"20": "wC:p2"}
    }"#;

    /// The strongest rule: inside a pane's topic, the conversation IS the target. Nothing to aim,
    /// and no way for a reply to land somewhere the operator was not looking.
    #[test]
    fn a_topic_beats_both_reply_to_and_sticky() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let mut r = Routing::default();
        r.bind_topic(FORUM, 42, &PaneId::new(a.clone()));
        r.record_push(FORUM, 77, &PaneId::new("wZ:p9"));
        r.set_sticky(FORUM, &PaneId::new("wY:p8"));

        assert_eq!(
            r.resolve(FORUM, Some(FORUM), Some(77), Some(42), &snap),
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
        r.bind_topic(FORUM, 42, &PaneId::new("wZ:p9"));
        assert_eq!(
            r.resolve(FORUM, Some(FORUM), None, Some(42), &snap),
            Target::Gone {
                pane: PaneId::new("wZ:p9")
            }
        );
    }

    /// Outside the group with topics a thread number is an ordinary reply thread, not a session.
    #[test]
    fn outside_a_forum_a_thread_id_is_ignored_and_sticky_still_applies() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let mut r = Routing::default();
        r.set_sticky(DM, &PaneId::new(a.clone()));
        // thread 99 is not bound to anything — must not swallow the message.
        assert_eq!(
            r.resolve(DM, None, None, Some(99), &snap),
            Target::Pane {
                pane: PaneId::new(a),
                why: Why::Sticky
            }
        );
        assert_eq!(
            r.resolve(DM, Some(DM), None, Some(99), &snap),
            Target::None,
            "in the group with topics an unknown topic gets a picker, never the remembered target"
        );
    }

    #[test]
    fn a_pane_keeps_one_topic() {
        let mut r = Routing::default();
        let p = PaneId::new("wA:p1");
        r.bind_topic(FORUM, 7, &p);
        assert_eq!(r.topic_for(FORUM, &p), Some(7));
        assert_eq!(r.pane_for_topic(FORUM, 7), Some(p.clone()));
        assert_eq!(r.topic_for(FORUM, &PaneId::new("wB:p1")), None);
        assert_eq!(
            r.topic_for(DM, &p),
            None,
            "a topic belongs to the chat it was made in"
        );
    }

    #[test]
    fn reply_to_beats_sticky() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let pane = PaneId::new(a.clone());
        let mut r = Routing::default();
        r.record_push(DM, 77, &pane);
        r.set_sticky(DM, &PaneId::new("wZ:p9")); // a different, dead pane

        let t = r.resolve(DM, None, Some(77), None, &snap);
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
        r.set_sticky(DM, &PaneId::new(a.clone()));
        assert_eq!(
            r.resolve(DM, None, None, None, &snap),
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
            r.resolve(DM, None, None, None, &snap),
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
        r.set_sticky(DM, &PaneId::new("wZ:p9"));
        assert_eq!(
            r.resolve(DM, None, None, None, &snap),
            Target::Gone {
                pane: PaneId::new("wZ:p9")
            }
        );

        // Same for a reply-to whose pane has since closed.
        let mut r2 = Routing::default();
        r2.record_push(DM, 5, &PaneId::new("wZ:p9"));
        assert_eq!(
            r2.resolve(DM, None, Some(5), None, &snap),
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
            r.resolve(1, None, None, None, &snap),
            Target::Pane { .. }
        ));
        assert_eq!(r.resolve(2, None, None, None, &snap), Target::None);
    }

    /// THE regression test. Telegram numbers messages from 1 again in every chat, so the direct
    /// chat and the group both have a message 20. A swipe-to-reply in the direct chat must not find
    /// the group's memory and type into a session the operator was not even looking at.
    #[test]
    fn a_dm_reply_to_cannot_reach_a_forum_pane() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let pane = PaneId::new(a);
        let mut r = Routing::default();
        r.record_push(FORUM, 20, &pane);

        assert_eq!(
            r.resolve(DM, Some(FORUM), Some(20), None, &snap),
            Target::None,
            "a reply in the direct chat must not be answered from the group's memory"
        );
        assert_eq!(
            r.resolve(FORUM, Some(FORUM), Some(20), None, &snap),
            Target::Pane {
                pane,
                why: Why::ReplyTo
            },
            "the same number in the chat it was written for still works"
        );
    }

    /// Only the configured group numbers its threads by session. Anywhere else a thread number is
    /// the first message of a reply chain, and reading it as a session would be a coin flip.
    #[test]
    fn a_thread_id_only_binds_a_topic_inside_the_forum_chat() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let pane = PaneId::new(a);
        let mut r = Routing::default();
        r.bind_topic(FORUM, 20, &pane);

        const OTHER_GROUP: i64 = -100999888;
        assert_eq!(
            r.resolve(OTHER_GROUP, Some(FORUM), None, Some(20), &snap),
            Target::None
        );
        assert_eq!(
            r.resolve(FORUM, Some(FORUM), None, Some(20), &snap),
            Target::Pane {
                pane,
                why: Why::Topic
            }
        );
    }

    /// In the group the topic is the aim. A message typed outside one gets the picker, because
    /// sending it to the remembered session is the silent surprise topics exist to remove.
    #[test]
    fn in_the_forum_a_general_or_unbound_topic_message_gets_a_picker_not_the_sticky_pane() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let mut r = Routing::default();
        r.set_sticky(FORUM, &PaneId::new(a.clone()));
        r.bind_topic(FORUM, 20, &PaneId::new("wB:p1"));

        assert_eq!(
            r.resolve(FORUM, Some(FORUM), None, None, &snap),
            Target::None,
            "General has no topic, so there is nothing to aim at"
        );
        assert_eq!(
            r.resolve(FORUM, Some(FORUM), None, Some(99), &snap),
            Target::None,
            "a topic the bridge did not make is not a session"
        );

        r.set_sticky(DM, &PaneId::new(a.clone()));
        assert_eq!(
            r.resolve(DM, Some(FORUM), None, None, &snap),
            Target::Pane {
                pane: PaneId::new(a),
                why: Why::Sticky
            },
            "the direct chat still has an aim"
        );
    }

    #[test]
    fn a_v1_state_file_keeps_its_topics_for_the_forum_and_loses_its_reply_memory() {
        let path = scratch("v1-forum");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, V1_FILE).unwrap();

        let snap = snapshot_with(&[&any_pane()]);
        let r = Routing::load(&path, Some(FORUM));

        assert_eq!(r.pane_for_topic(FORUM, 20), Some(PaneId::new("wB:p1")));
        assert_eq!(r.pane_for_topic(DM, 20), None);
        assert_eq!(r.sticky(DM), Some(PaneId::new("wA:p1")));
        assert_eq!(
            r.sticky(FORUM),
            None,
            "a remembered target in the group is never honoured again, so it is not kept"
        );
        assert_eq!(
            r.resolve(DM, Some(FORUM), Some(20), None, &snap),
            Target::Gone {
                pane: PaneId::new("wA:p1")
            },
            "the unfiled reply memory is gone, so a reply to message 20 in the direct chat falls \
             through to that chat's own aim and never to wC:p2"
        );
    }

    #[test]
    fn a_v1_state_file_with_no_forum_configured_drops_its_topics_and_keeps_its_sticky() {
        let path = scratch("v1-no-forum");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, V1_FILE).unwrap();

        let r = Routing::load(&path, None);
        for chat in [DM, FORUM, 0, -1] {
            assert_eq!(
                r.pane_for_topic(chat, 20),
                None,
                "with no group configured there is no chat to file a topic under"
            );
        }
        assert_eq!(r.sticky(DM), Some(PaneId::new("wA:p1")));
        assert_eq!(
            r.sticky(FORUM),
            Some(PaneId::new("wE:p1")),
            "nothing is inert when no group has topics, so nothing is dropped"
        );
    }

    /// A tap on a button from before the upgrade says so rather than answering: the menu behind it
    /// was filed without a chat, and two chats' menus can already have overwritten each other.
    #[test]
    fn a_v1_state_files_menu_memory_is_dropped_rather_than_guessed_into_a_chat() {
        let path = scratch("v1-menus");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"sticky":{},"prompts":{"20":{"chat":878,"pane":"wC:p2","seq":9,
               "options":["Allow once","Reject"]}}}"#,
        )
        .unwrap();

        let r = Routing::load(&path, Some(FORUM));
        for chat in [DM, FORUM] {
            assert_eq!(r.prompt_for(chat, 20), None);
        }
    }

    #[test]
    fn migration_is_one_way_and_idempotent() {
        let path = scratch("v1-once");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, V1_FILE).unwrap();

        let first = Routing::load(&path, Some(FORUM));
        first.save(&path).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("topics_by_chat"));
        assert!(written.contains("pushes_by_chat"));
        assert!(
            !written.contains("\"topics\":"),
            "the old keys must not be written back, or the move happens again every start: {written}"
        );
        assert!(!written.contains("\"pushes\":"), "{written}");
        assert!(!written.contains("\"prompts\":"), "{written}");

        let second = Routing::load(&path, Some(FORUM));
        assert_eq!(
            serde_json::to_string(&second).unwrap(),
            serde_json::to_string(&first).unwrap(),
            "loading the migrated file again must change nothing"
        );
    }

    #[test]
    fn push_memory_is_bounded_per_chat_and_chats_do_not_evict_each_other() {
        let a = any_pane();
        let snap = snapshot_with(&[&a]);
        let pane = PaneId::new(a);
        let mut r = Routing::default();
        for i in 0..(MAX_PUSH_MEMORY as i64 + 50) {
            r.record_push(DM, i, &pane);
            r.record_push(FORUM, i, &pane);
        }

        for chat in [DM, FORUM] {
            let per_chat = r.pushes.get(&chat).expect("both chats remembered");
            assert_eq!(per_chat.len(), MAX_PUSH_MEMORY, "a busy chat is bounded");
            assert!(
                !per_chat.contains_key(&0),
                "the oldest must be dropped first"
            );
            assert!(per_chat.contains_key(&(MAX_PUSH_MEMORY as i64 + 49)));
        }

        // The newest of each is still the newest of each: one chat's traffic must not push the
        // other chat's memory out.
        let newest = MAX_PUSH_MEMORY as i64 + 49;
        assert!(matches!(
            r.resolve(DM, Some(FORUM), Some(newest), None, &snap),
            Target::Pane { .. }
        ));
        assert_eq!(
            r.resolve(1, Some(FORUM), Some(newest), None, &snap),
            Target::None,
            "a chat that was never pushed to remembers nothing"
        );
    }

    #[test]
    fn state_round_trips_through_disk() {
        let path = scratch("routing");

        let mut r = Routing::default();
        r.set_sticky(DM, &PaneId::new("wA:p1"));
        r.record_push(DM, 9, &PaneId::new("wB:p1"));
        r.save(&path).unwrap();

        let back = Routing::load(&path, Some(FORUM));
        assert_eq!(back.sticky(DM), Some(PaneId::new("wA:p1")));
        assert_eq!(
            back.pushes
                .get(&DM)
                .and_then(|m| m.get(&9))
                .map(String::as_str),
            Some("wB:p1")
        );
        assert_eq!(back.pushes.get(&FORUM), None);
    }

    /// A restart must not disarm every button the operator can still see, and a long run must not
    /// grow this map without bound.
    #[test]
    fn a_drawn_menu_survives_a_restart_and_the_memory_is_bounded() {
        let path = scratch("prompts");

        let options = vec!["Allow once".to_string(), "Reject".to_string()];
        let mut r = Routing::default();
        r.record_prompt(DM, 9, &PaneId::new("wB:p1"), Some(198), &options);
        r.save(&path).unwrap();

        let back = Routing::load(&path, Some(FORUM));
        assert_eq!(
            back.prompt_for(DM, 9),
            Some(&PromptRecord {
                chat: DM,
                pane: "wB:p1".to_string(),
                seq: Some(198),
                options,
            }),
            "the labels a button showed must outlive a restart, or every live button refuses"
        );
        assert_eq!(back.prompt_for(DM, 10), None);
        assert_eq!(
            back.prompt_for(FORUM, 9),
            None,
            "the same message number in the other chat is a different message"
        );

        let mut many = Routing::default();
        for i in 0..(MAX_PROMPT_MEMORY as i64 + 50) {
            many.record_prompt(
                DM,
                i,
                &PaneId::new("wB:p1"),
                Some(1),
                &["Reject".to_string()],
            );
            many.record_prompt(
                FORUM,
                i,
                &PaneId::new("wB:p1"),
                Some(1),
                &["Reject".to_string()],
            );
        }
        for chat in [DM, FORUM] {
            assert_eq!(
                many.prompts.get(&chat).map(BTreeMap::len),
                Some(MAX_PROMPT_MEMORY)
            );
            assert!(
                many.prompt_for(chat, 0).is_none(),
                "the oldest must be dropped first"
            );
            assert!(
                many.prompt_for(chat, MAX_PROMPT_MEMORY as i64 + 49)
                    .is_some()
            );
        }
    }

    #[test]
    fn a_missing_or_corrupt_state_file_starts_empty_rather_than_failing() {
        let dir = std::env::temp_dir().join(format!("herdr-tg-routing-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("nope.json");
        assert!(Routing::load(&missing, None).sticky(1).is_none());

        let corrupt = dir.join("corrupt.json");
        std::fs::write(&corrupt, "{not json").unwrap();
        assert!(
            Routing::load(&corrupt, None).sticky(1).is_none(),
            "a corrupt cache must not take the bridge down — the operator retargets with one tap"
        );
    }

    /// The file names the operator's chats and sessions. On a shared machine the default mode would
    /// hand that to every other account.
    #[cfg(unix)]
    #[test]
    fn the_routing_state_file_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;

        let path = scratch("modes");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A temp file left behind by a run from before this fix: opening it again with truncate
        // would keep this mode.
        let tmp = path.with_extension("state.json.tmp");
        std::fs::write(&tmp, "{}").unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644)).unwrap();

        let mut r = Routing::default();
        r.set_sticky(DM, &PaneId::new("wA:p1"));
        r.save(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "the state file was readable beyond its owner"
        );
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            dir_mode & 0o077,
            0,
            "the state directory was open to others"
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

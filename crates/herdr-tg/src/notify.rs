//! The push: noticing an agent is stuck, and buzzing the operator's phone about it — once.
//!
//! # The anti-spam contract
//!
//! A notifier that cries wolf gets muted, and a muted bridge is worse than no bridge: the operator
//! believes they are covered. So every rule here exists to make a buzz *mean something*.
//!
//! - **Push on state transitions, never on output volume.** A build pane printing ten thousand
//!   lines is not asking anything.
//! - **Debounce.** An ask that resolves inside the window never notifies at all. Most agent
//!   questions are answered by the agent itself moments later.
//! - **Dedupe on `(pane_id, state_change_seq)`.** The re-check after the debounce window is a second
//!   observation of the same fact, and must not become a second buzz. `state_change_seq` was probed
//!   stable while a pane sits blocked (`docs/SLICE-3-PROBE.md` P5), which is what makes it a sound
//!   key.
//!
//! # Why one connection with N subscriptions, re-opened when the herd changes
//!
//! Protocol 20 has **no global agent-status subscription** — only 3 of 27 subscription variants
//! take a `pane_id`, and `pane.agent_status_changed` is one of them. So the notifier subscribes once
//! per agent pane, all on a single `events.subscribe` connection.
//!
//! `events.subscribe` also freezes its subscription set at connect: there is no `events.update`. A
//! pane that appears after the connection opened is invisible to it forever. Hence the supervisor
//! loop — it re-snapshots on a timer and, when the agent-pane set has changed, tears the stream down
//! and opens a new one. Re-subscribing is cheap and the filtered replay means nothing is missed
//! across the gap.
//!
//! # Why the filter is `blocked`
//!
//! A filtered subscription whose filter already matches replays immediately at subscribe time, and
//! also fires on later transitions into that status (`docs/SLICE-3-PROBE.md` P4). One subscription
//! therefore covers both jobs: *what am I missing right now* (the laptop-was-asleep case in
//! PLAN.md's failure table) and *tell me the moment it happens*. No second mechanism, no snapshot
//! diffing.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use herdr_client::{AgentStatus, HerdrClient, PaneId};

/// An ask worth telling the operator about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub pane: PaneId,
    pub workspace: String,
    pub agent: String,
    /// The dedupe key. Stable while the pane stays blocked.
    pub seq: u64,
}

/// Tracks which asks have already been pushed, so a re-observation is not a re-buzz.
#[derive(Debug, Default)]
pub struct Seen {
    /// pane id → the `state_change_seq` last pushed for it.
    pushed: BTreeMap<String, u64>,
}

impl Seen {
    /// Should this ask buzz the phone?
    ///
    /// False when this exact `(pane, seq)` was already pushed. A *new* seq on the same pane is a
    /// genuinely new ask and does buzz — that is the case a pane-only key would silently swallow.
    pub fn should_push(&mut self, ask: &Ask) -> bool {
        match self.pushed.get(ask.pane.as_str()) {
            Some(&seq) if seq == ask.seq => false,
            _ => {
                self.pushed.insert(ask.pane.as_str().to_string(), ask.seq);
                true
            }
        }
    }

    /// Forget a pane that is no longer blocked, so its next ask buzzes even if the seq repeats.
    ///
    /// Without this, a pane whose `state_change_seq` ever returned to a previously-pushed value
    /// would go permanently silent — the failure mode that is invisible until an ask is missed.
    pub fn cleared(&mut self, pane: &PaneId) {
        self.pushed.remove(pane.as_str());
    }

    /// Drop panes that have left the herd entirely.
    pub fn retain_alive(&mut self, alive: &BTreeSet<String>) {
        self.pushed.retain(|p, _| alive.contains(p));
    }
}

/// Timing knobs.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How long an ask must persist before it is worth a buzz.
    pub debounce: Duration,
    /// How often to re-snapshot and check whether the agent-pane set changed.
    pub resubscribe_check: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            // Long enough that a self-resolving question never reaches the phone; short enough that
            // a real ask is not left waiting. PLAN.md calls this notify_delay_ms.
            debounce: Duration::from_secs(20),
            resubscribe_check: Duration::from_secs(30),
        }
    }
}

/// Re-check a pane after the debounce window, and report whether it is still an ask.
///
/// Uses `agents()` rather than the event, because the event carries no `state_change_seq` and the
/// dedupe key needs one. One RPC buys both the still-blocked check and the key.
///
/// The known race, stated rather than hidden: this read happens after the event, so a pane that
/// blocked and unblocked and blocked again inside the window is reported once, with the later seq.
/// That is the correct outcome for a notifier — one buzz for one unresolved ask.
pub async fn recheck(client: &HerdrClient, pane: &PaneId) -> Option<Ask> {
    let agents = client.agents().await.ok()?;
    let info = agents.iter().find(|a| a.pane_id == *pane)?;
    if info.agent_status != AgentStatus::Blocked {
        return None;
    }
    Some(Ask {
        pane: pane.clone(),
        workspace: info.workspace_id.as_str().to_string(),
        agent: info.agent.clone().unwrap_or_else(|| "agent".into()),
        seq: info.state_change_seq.unwrap_or(0),
    })
}

/// The agent panes worth subscribing to, as a stable set.
pub fn agent_panes(snapshot: &herdr_client::SessionSnapshot) -> BTreeSet<String> {
    snapshot
        .panes
        .iter()
        .filter(|p| p.agent.is_some() || p.display_agent.is_some())
        .map(|p| p.pane_id.as_str().to_string())
        .collect()
}

// ── the supervisor ───────────────────────────────────────────────────────────────────────────────

/// What the loop does when an ask survives its debounce window.
///
/// A callback rather than a direct Telegram dependency, so the whole loop — the timing, the
/// reconnects, the dedupe — is testable without a bot token.
pub type OnAsk = std::sync::Arc<
    dyn Fn(Ask) -> futures_core::future::BoxFuture<'static, ()> + Send + Sync + 'static,
>;

/// Watch the herd and call `on_ask` once per surviving ask, forever.
///
/// # Why the debounce is spawned rather than awaited inline
///
/// The first version slept inline in the event loop. It worked, and it was wrong in a way only a
/// live run showed: with three blocked panes the first ask arrived after 20s, the second after 40s,
/// the third after 60s, because every event queued behind the previous pane's timer. On a herd of
/// ten that is over three minutes before the last agent's question reaches the phone — and the
/// operator has no way to tell a slow bridge from a broken one.
///
/// So each pane's debounce runs in its own task and reports back on a channel. A pane already
/// waiting does not start a second timer, which is what keeps a flapping agent from spawning one
/// task per flap.
///
/// Never returns in normal operation. Every error path reconnects rather than exiting: staying up
/// through a herdr restart is PLAN.md's "herdr dies / socket gone" row, and `Restart=always` is the
/// backstop, not the plan.
pub async fn watch(client: std::sync::Arc<HerdrClient>, timing: Timing, on_ask: OnAsk) -> ! {
    let mut seen = Seen::default();
    let mut backoff = Duration::from_secs(1);
    // One notice per outage, not one per retry — a reconnect storm must not become a
    // notification storm.
    let mut announced_down = false;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Option<Ask>>(64);
    // Panes with a debounce already in flight.
    let mut pending: BTreeSet<String> = BTreeSet::new();

    loop {
        let snapshot = match client.snapshot().await {
            Ok(s) => s,
            Err(e) => {
                if !announced_down {
                    tracing::warn!(error = %e, "herdr unreachable; the push loop is retrying");
                    announced_down = true;
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        if announced_down {
            tracing::info!("herdr reachable again; the push loop has resubscribed");
            announced_down = false;
        }
        backoff = Duration::from_secs(1);

        let panes = agent_panes(&snapshot);
        seen.retain_alive(&panes);
        if panes.is_empty() {
            tokio::time::sleep(timing.resubscribe_check).await;
            continue;
        }

        let subs: Vec<_> = panes
            .iter()
            .map(|p| {
                herdr_client::Subscription::agent_status(
                    &PaneId::new(p.clone()),
                    AgentStatus::Blocked,
                )
            })
            .collect();

        let mut stream = match client.subscribe(&subs).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "could not subscribe; retrying");
                tokio::time::sleep(backoff).await;
                continue;
            }
        };
        tracing::info!(panes = panes.len(), "subscribed for asks");

        loop {
            tokio::select! {
                // An ask finished its debounce.
                Some(result) = rx.recv() => {
                    match result {
                        Some(ask) => {
                            pending.remove(ask.pane.as_str());
                            if seen.should_push(&ask) {
                                tracing::info!(pane = %ask.pane, seq = ask.seq, "pushing an ask");
                                on_ask(ask).await;
                            }
                        }
                        None => { /* resolved inside the window; the task logged it */ }
                    }
                }

                event = tokio::time::timeout(timing.resubscribe_check, stream.next()) => {
                    match event {
                        // The pane set may have changed. `events.subscribe` freezes its set at
                        // connect, so a new pane is invisible until we re-open.
                        Err(_elapsed) => {
                            if let Ok(now) = client.snapshot().await
                                && agent_panes(&now) != panes
                            {
                                tracing::info!("the agent-pane set changed; resubscribing");
                                break;
                            }
                        }
                        Ok(None) => {
                            tracing::warn!("the event stream closed; resubscribing");
                            break;
                        }
                        Ok(Some(Err(e))) => {
                            tracing::warn!(error = %e, "event decode failed; resubscribing");
                            break;
                        }
                        Ok(Some(Ok(ev))) => {
                            let herdr_client::Event::AgentStatus(changed) = ev else {
                                // Roster frames replay a stale backlog on every connect, each
                                // carrying a historical status. Pushing on them would fire a
                                // phantom burst at every reconnect.
                                continue;
                            };
                            if changed.agent_status != AgentStatus::Blocked {
                                seen.cleared(&changed.pane_id);
                                pending.remove(changed.pane_id.as_str());
                                continue;
                            }
                            let key = changed.pane_id.as_str().to_string();
                            if !pending.insert(key) {
                                // Already waiting on this pane. A flapping agent must not spawn a
                                // timer per flap.
                                continue;
                            }
                            let pane = changed.pane_id.clone();
                            let client = std::sync::Arc::clone(&client);
                            let tx = tx.clone();
                            let debounce = timing.debounce;
                            tokio::spawn(async move {
                                tokio::time::sleep(debounce).await;
                                let ask = recheck(&client, &pane).await;
                                if ask.is_none() {
                                    tracing::debug!(pane = %pane, "the ask resolved inside the debounce window");
                                }
                                let _ = tx.send(ask).await;
                            });
                        }
                    }
                }
            }
        }

        // A resubscribe invalidates nothing about in-flight debounces: they carry their own pane
        // and re-read status when they wake, so a pane that resolved during the gap is dropped.
        pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(pane: &str, seq: u64) -> Ask {
        Ask {
            pane: PaneId::new(pane),
            workspace: "w1".into(),
            agent: "opencode".into(),
            seq,
        }
    }

    /// THE anti-spam property: the re-check after the debounce window is a second observation of
    /// the same fact, not a second ask.
    #[test]
    fn the_same_ask_buzzes_exactly_once() {
        let mut seen = Seen::default();
        assert!(seen.should_push(&ask("w1:p1", 7)));
        assert!(!seen.should_push(&ask("w1:p1", 7)));
        assert!(!seen.should_push(&ask("w1:p1", 7)));
    }

    /// The other half: a genuinely new ask on the same pane MUST buzz. A pane-only dedupe key
    /// would swallow it, and the operator would never learn the agent asked again.
    #[test]
    fn a_new_ask_on_the_same_pane_does_buzz() {
        let mut seen = Seen::default();
        assert!(seen.should_push(&ask("w1:p1", 7)));
        assert!(seen.should_push(&ask("w1:p1", 8)));
    }

    /// Without `cleared`, a pane whose seq ever repeated a pushed value would go permanently
    /// silent — invisible until an ask is missed.
    #[test]
    fn clearing_a_pane_lets_a_repeated_seq_buzz_again() {
        let mut seen = Seen::default();
        assert!(seen.should_push(&ask("w1:p1", 7)));
        seen.cleared(&PaneId::new("w1:p1"));
        assert!(seen.should_push(&ask("w1:p1", 7)));
    }

    #[test]
    fn panes_are_tracked_independently() {
        let mut seen = Seen::default();
        assert!(seen.should_push(&ask("w1:p1", 1)));
        assert!(seen.should_push(&ask("w2:p1", 1)));
        assert!(!seen.should_push(&ask("w1:p1", 1)));
    }

    #[test]
    fn panes_that_left_the_herd_are_forgotten() {
        let mut seen = Seen::default();
        seen.should_push(&ask("w1:p1", 1));
        seen.should_push(&ask("w2:p1", 1));
        seen.retain_alive(&["w2:p1".to_string()].into_iter().collect());
        // w1:p1 is gone; if it ever returns, its ask is new.
        assert!(seen.should_push(&ask("w1:p1", 1)));
    }

    #[test]
    fn agent_panes_ignores_shells() {
        let raw = include_str!("../../herdr-client/tests/fixtures/snapshot.json");
        let env: serde_json::Value = serde_json::from_str(raw).unwrap();
        let mut snap: herdr_client::SessionSnapshot =
            serde_json::from_value(env["result"]["snapshot"].clone()).unwrap();
        let with_agents = agent_panes(&snap);
        assert!(!with_agents.is_empty(), "the fixture has agent panes");

        // Strip the agent off one pane; it must drop out of the subscription set. A shell pane
        // never asks anything, and subscribing to it is a wasted subscription on a connection that
        // has to be torn down whenever the set changes.
        let first = snap.panes[0].pane_id.as_str().to_string();
        snap.panes[0].agent = None;
        snap.panes[0].display_agent = None;
        let without = agent_panes(&snap);
        assert!(!without.contains(&first));
        assert_eq!(without.len(), with_agents.len() - 1);
    }

    #[test]
    fn the_debounce_is_long_enough_to_swallow_a_self_resolving_ask() {
        let t = Timing::default();
        assert!(
            t.debounce >= Duration::from_secs(10),
            "too short and every transient question reaches the phone"
        );
        assert!(
            t.debounce <= Duration::from_secs(60),
            "too long and a real ask waits on a timer instead of on the operator"
        );
    }
}

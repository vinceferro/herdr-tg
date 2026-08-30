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

/// Something worth saying in a pane's topic.
///
/// The bridge started as an alert channel: one message when an agent got stuck. A topic is a
/// standing conversation with that agent, so it needs the beats either side of the question too —
/// otherwise opening it on a phone shows a stuck agent and no idea how it got there, or worse,
/// nothing at all for a session that never blocked.
///
/// Deliberately NOT every transition. `working` ↔ `idle` churns constantly as an agent thinks, and
/// relaying that would make the topic unreadable and the notifications worthless.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Beat {
    /// The agent is stuck and needs an answer.
    Asked(Ask),
    /// The agent finished and is waiting to be looked at.
    ///
    /// PLAN.md's second push trigger, and in a continuity model the most important one: an agent
    /// that finished is invisible otherwise. One pane sat `done` for 17 minutes during sampling.
    Finished {
        pane: PaneId,
        workspace: String,
        agent: String,
        seq: u64,
        excerpt: String,
    },
    /// The agent left `blocked` under its own steam or because an answer landed.
    ///
    /// This is what makes [`crate::deliver::Rung::Acted`] observable: the strongest confirmation
    /// that a reply was received is the agent going back to work.
    Resumed { pane: PaneId, workspace: String },
}

/// An ask worth telling the operator about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub pane: PaneId,
    pub workspace: String,
    pub agent: String,
    /// Where the pane's run of work stands, as the herd reports it — the dedupe key, stable while
    /// the pane stays blocked.
    ///
    /// `None` when the herd does not report it at all. Kept as an option rather than flattened to
    /// zero because a tap on this ask's buttons is checked against it later, and "the herd cannot
    /// tell me" has to be distinguishable there from "it is still at the same point".
    pub seq: Option<u64>,
    /// The tail of the pane: what the agent is actually asking.
    ///
    /// Empty when the read failed — a push with no excerpt is still worth sending, because the
    /// operator can open the pane. A push that never arrives because a read failed is not.
    pub excerpt: String,
    /// The options, when this ask is a choice dialog rather than a question.
    ///
    /// Present means the operator must pick one: free text into a dialog goes nowhere and the
    /// Enter after it confirms whatever was highlighted.
    pub options: Vec<String>,
}

/// How many lines of the pane's tail to relay.
///
/// D4 accepts that messages transit Telegram's servers, with the mitigation that the bridge relays
/// **asks and digests, never full pane dumps**. This cap is that mitigation in code: enough to read
/// the question, never enough to be a transcript.
const EXCERPT_LINES: usize = 12;

/// Hard character cap, below Telegram's 4096 so the surrounding message always fits.
const EXCERPT_CHARS: usize = 900;

/// The tail of a pane, cleaned of TUI chrome and trimmed to something readable on a phone.
///
/// # Why this is more than a tail
///
/// The first version took the last N non-empty lines. On a real agent pane that is not the
/// conversation — it is the harness's furniture. The operator's verdict on it was "the message
/// relay sucks", and they were right; what actually got relayed was:
///
/// ```text
///   ┃  Coordinat ·task class: agentic-coding chain (sticky …)      LLM
///   ┃  or                                                          Gate
///   ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
///    ⬝⬝■■■■■■  esc interrupt            113.0K (11%)  ctrl+p commands
/// ```
///
/// Four kinds of junk, each removed by a rule below: a per-row gutter, a heavy status rule with a
/// footer under it, a right-hand column bleeding into the message area mid-word, and the harness's
/// own model banner.
///
/// Every rule is structural rather than harness-specific. A rule that matched opencode's exact
/// banner text would relay garbage from any other harness — and this bridge's whole point is that
/// it does not care which harness is in the pane.
pub fn excerpt_from(pane_text: &str) -> String {
    let cleaned = strip_chrome(pane_text);
    let lines: Vec<&str> = cleaned.lines().collect();
    let start = lines.len().saturating_sub(EXCERPT_LINES);
    let tail = lines[start..].join("\n");
    let tail = tail.trim().to_string();
    if tail.chars().count() <= EXCERPT_CHARS {
        return tail;
    }
    // Cut from the FRONT: the newest text is at the bottom, and losing it to keep older context
    // would defeat the purpose.
    let cut = tail.chars().count() - EXCERPT_CHARS;
    let kept: String = tail.chars().skip(cut).collect();
    format!("…{kept}")
}

/// Characters a TUI draws frames with. Never part of a message.
const BOX: &str = "─━│┃┄┅┆┇┈┉┊┋┌┐└┘├┤┬┴┼╭╮╯╰═║╔╗╚╝╠╣╦╩╬▀▄█▌▐░▒▓╹╻╺╸▁▂▃▅▆▇⬝▣◇◆■";

/// Remove the harness's furniture, leaving what it actually said.
///
/// Public wrapper for the mirror, which needs the same cleaning on a different cadence.
pub fn strip_chrome_public(text: &str) -> String {
    strip_chrome(text)
}

/// Remove the harness's furniture, leaving what it actually said.
fn strip_chrome(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();

    // 1. A heavy horizontal rule is the top of the status bar. Everything below it is furniture.
    if let Some(bar) = lines.iter().rposition(|l| is_rule(l)) {
        lines.truncate(bar);
    }

    let mut out: Vec<String> = Vec::new();
    for raw in lines {
        let l = strip_gutter(raw);
        let s = l.trim();
        if s.is_empty() {
            out.push(String::new());
            continue;
        }
        // 2. Rows that are only frame.
        if s.chars().all(|c| BOX.contains(c) || c == ' ') {
            continue;
        }
        // 3. The harness's own status and hint rows.
        if s.starts_with(['▣', '◆', '◇', '⬝', '■'])
            || s.contains("esc interrupt")
            || s.starts_with("ctrl+")
        {
            continue;
        }
        // 4. A long run of spaces mid-line is a right-hand column bleeding into the message area —
        //    the TUI drawing two columns into one row of cells, which splits words across rows.
        if has_column_bleed(&l) {
            continue;
        }
        out.push(l);
    }

    // 5. Collapse blank runs.
    let mut res: Vec<String> = Vec::new();
    for l in out {
        if l.trim().is_empty() {
            if res.last().map(|p: &String| p.trim().is_empty()) != Some(true) && !res.is_empty() {
                res.push(String::new());
            }
        } else {
            res.push(l);
        }
    }

    // 6. Whatever the column bleed left behind at the bottom: orphan fragments and the banner.
    while let Some(last) = res.last() {
        let t = last.trim();
        if t.is_empty() {
            res.pop();
            continue;
        }
        let orphan = t.chars().count() <= 12 && !t.contains(' ');
        let banner =
            t.contains('·') && t.chars().count() < 100 && !t.ends_with(['.', '?', '!', ':', ')']);
        if orphan || banner {
            res.pop();
            continue;
        }
        break;
    }
    res.join("\n")
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    ['▀', '━', '═', '─']
        .iter()
        .any(|c| t.matches(*c).count() >= 10)
}

fn strip_gutter(line: &str) -> String {
    let t = line.trim_start();
    for g in ['┃', '│', '┆', '┇', '║', '╹'] {
        if let Some(rest) = t.strip_prefix(g) {
            let indent = line.len() - t.len();
            return format!(
                "{}{}",
                " ".repeat(indent.min(2)),
                rest.strip_prefix(' ').unwrap_or(rest)
            )
            .trim_end()
            .to_string();
        }
    }
    line.trim_end().to_string()
}

fn has_column_bleed(line: &str) -> bool {
    let mut run = 0usize;
    let mut seen_text = false;
    for c in line.trim_end().chars() {
        if c == ' ' {
            if seen_text {
                run += 1;
            }
        } else {
            if seen_text && run >= 12 {
                return true;
            }
            run = 0;
            seen_text = true;
        }
    }
    false
}

/// Tracks which asks have already been pushed, so a re-observation is not a re-buzz.
///
/// **Persisted**, and that is not an optimisation. The first version kept this in memory, and the
/// live push log showed the consequence: every redeploy re-pushed `wA:p1 seq=198` and `wC:p2
/// seq=136` — the same two questions, three times across three restarts. A bridge under
/// `Restart=always` restarts for reasons the operator never sees, and each one would have buzzed
/// them again about a question they had already read. That is the anti-spam contract failing at
/// exactly the moment it matters, and only a real run showed it.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Seen {
    /// pane id → the `state_change_seq` last pushed for it.
    pushed: BTreeMap<String, u64>,
}

impl Seen {
    /// Should this ask buzz the phone?
    ///
    /// False when this exact `(pane, seq)` was already pushed. A *new* seq on the same pane is a
    /// genuinely new ask and does buzz — that is the case a pane-only key would silently swallow.
    pub fn should_push(&mut self, key: (String, u64)) -> bool {
        let (k, seq) = key;
        match self.pushed.get(&k) {
            Some(&s) if s == seq => false,
            _ => {
                self.pushed.insert(k, seq);
                true
            }
        }
    }

    /// Was this pane's last recorded beat a block? Used to tell "went back to work after an ask"
    /// from ordinary churn, so only the first is worth saying.
    pub fn was_blocked(&self, pane: &PaneId) -> bool {
        self.pushed.contains_key(&format!("{}|ask", pane.as_str()))
    }

    /// Forget a pane that is no longer blocked, so its next ask buzzes even if the seq repeats.
    ///
    /// Without this, a pane whose `state_change_seq` ever returned to a previously-pushed value
    /// would go permanently silent — the failure mode that is invisible until an ask is missed.
    pub fn cleared(&mut self, pane: &PaneId) {
        self.pushed.remove(&format!("{}|ask", pane.as_str()));
    }

    /// Drop panes that have left the herd entirely.
    pub fn retain_alive(&mut self, alive: &BTreeSet<String>) {
        self.pushed
            .retain(|k, _| alive.contains(k.split('|').next().unwrap_or("")));
    }

    pub fn default_path() -> std::path::PathBuf {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
            })
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        base.join("herdr-tg").join("pushed.state.json")
    }

    /// Load, tolerating absence and corruption.
    ///
    /// A lost file costs one duplicate buzz per still-blocked pane. Refusing to start would cost
    /// every ask until someone noticed, so this fails toward the cheaper mistake.
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    /// Save atomically — the bridge can be killed at any instant.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("state.json.tmp");
        std::fs::write(&tmp, serde_json::to_string(self)?)?;
        std::fs::rename(&tmp, path)
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
    // `visible` only, always. `recent` harvest-scrolls the operator's real viewport, and this
    // read happens on a timer — exactly the case the crate makes structurally impossible.
    let excerpt = match client.read_visible(pane).await {
        Ok(read) => excerpt_from(&read.text),
        Err(e) => {
            tracing::warn!(pane = %pane, error = %e, "could not read the ask; pushing without an excerpt");
            String::new()
        }
    };

    // A second read, in colour, only to see whether this is a dialog. `visible` either way, so it
    // cannot move the operator's screen.
    let options = match client.read_visible_ansi(pane).await {
        Ok(r) => crate::permission::parse(&r.text)
            .map(|p| p.options)
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    Some(Ask {
        pane: pane.clone(),
        workspace: info.workspace_id.as_str().to_string(),
        agent: info.agent.clone().unwrap_or_else(|| "agent".into()),
        seq: info.state_change_seq,
        excerpt,
        options,
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

/// The dedupe key for a beat: pane, kind and sequence.
///
/// The kind is in the key because an ask and a finish on the same pane at the same `seq` are
/// different things to say, and keying on the pane alone would silently swallow the second.
fn key_of(b: &Beat) -> (String, u64) {
    match b {
        // A herd that does not report a sequence dedupes on the pane alone, which is what it did
        // before this became an option: the same behaviour, now visibly a fallback.
        Beat::Asked(a) => (format!("{}|ask", a.pane.as_str()), a.seq.unwrap_or(0)),
        Beat::Finished { pane, seq, .. } => (format!("{}|done", pane.as_str()), *seq),
        Beat::Resumed { pane, .. } => (format!("{}|resumed", pane.as_str()), 0),
    }
}

/// Build a `Finished` beat, with the tail of what the agent left behind.
async fn finished(client: &HerdrClient, pane: &PaneId) -> Option<Beat> {
    let agents = client.agents().await.ok()?;
    let info = agents.iter().find(|a| a.pane_id == *pane)?;
    if info.agent_status != AgentStatus::Done {
        return None;
    }
    let excerpt = client
        .read_visible(pane)
        .await
        .map(|r| excerpt_from(&r.text))
        .unwrap_or_default();
    Some(Beat::Finished {
        pane: pane.clone(),
        workspace: info.workspace_id.as_str().to_string(),
        agent: info.agent.clone().unwrap_or_else(|| "agent".into()),
        seq: info.state_change_seq.unwrap_or(0),
        excerpt,
    })
}

// ── the supervisor ───────────────────────────────────────────────────────────────────────────────

/// What the loop does when an ask survives its debounce window.
///
/// A callback rather than a direct Telegram dependency, so the whole loop — the timing, the
/// reconnects, the dedupe — is testable without a bot token.
pub type OnBeat = std::sync::Arc<
    dyn Fn(Beat) -> futures_core::future::BoxFuture<'static, ()> + Send + Sync + 'static,
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
pub async fn watch(client: std::sync::Arc<HerdrClient>, timing: Timing, on_beat: OnBeat) -> ! {
    let seen_path = Seen::default_path();
    let mut seen = Seen::load(&seen_path);
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
            // Unfiltered: a filtered subscription only fires on ITS status, and the topic needs
            // `done` and the return to `working` as well. The startup snapshot below covers what a
            // filtered subscription's replay used to give us.
            .map(|p| herdr_client::Subscription::agent_status_any(&PaneId::new(p.clone())))
            .collect();

        let mut stream = match client.subscribe(&subs).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "could not subscribe; retrying");
                tokio::time::sleep(backoff).await;
                continue;
            }
        };
        tracing::info!(panes = panes.len(), "subscribed to every agent-status edge");

        // An unfiltered subscription does not replay, so the state as it is RIGHT NOW comes from
        // the snapshot instead. This is what recovers an ask raised while the laptop slept.
        for info in &snapshot.agents {
            let beat = match info.agent_status {
                AgentStatus::Blocked => recheck(&client, &info.pane_id).await.map(Beat::Asked),
                AgentStatus::Done => finished(&client, &info.pane_id).await,
                _ => None,
            };
            if let Some(b) = beat
                && seen.should_push(key_of(&b))
            {
                let _ = seen.save(&seen_path);
                on_beat(b).await;
            }
        }

        loop {
            tokio::select! {
                // An ask finished its debounce.
                Some(result) = rx.recv() => {
                    match result {
                        Some(ask) => {
                            pending.remove(ask.pane.as_str());
                            if seen.should_push(key_of(&Beat::Asked(ask.clone()))) {
                                if let Err(e) = seen.save(&seen_path) {
                                    tracing::warn!(error = %e, "could not persist the pushed set");
                                }
                                tracing::info!(pane = %ask.pane, seq = ask.seq, "pushing an ask");
                                on_beat(Beat::Asked(ask)).await;
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
                            match changed.agent_status {
                                AgentStatus::Blocked => { /* falls through to the debounce below */ }
                                AgentStatus::Done => {
                                    pending.remove(changed.pane_id.as_str());
                                    if let Some(b) = finished(&client, &changed.pane_id).await
                                        && seen.should_push(key_of(&b))
                                    {
                                        let _ = seen.save(&seen_path);
                                        tracing::info!(pane = %changed.pane_id, "agent finished");
                                        on_beat(b).await;
                                    }
                                    continue;
                                }
                                AgentStatus::Working => {
                                    // Only interesting as the END of a block: it is what makes
                                    // "the agent picked up your answer" observable.
                                    let was_stuck = seen.was_blocked(&changed.pane_id);
                                    seen.cleared(&changed.pane_id);
                                    let _ = seen.save(&seen_path);
                                    pending.remove(changed.pane_id.as_str());
                                    if was_stuck {
                                        on_beat(Beat::Resumed {
                                            pane: changed.pane_id.clone(),
                                            workspace: changed.workspace_id.as_str().to_string(),
                                        })
                                        .await;
                                    }
                                    continue;
                                }
                                // idle / unknown churn constantly. Not a beat.
                                _ => {
                                    seen.cleared(&changed.pane_id);
                                    let _ = seen.save(&seen_path);
                                    pending.remove(changed.pane_id.as_str());
                                    continue;
                                }
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
            seq: Some(seq),
            excerpt: String::new(),
            options: Vec::new(),
        }
    }

    /// THE anti-spam property: the re-check after the debounce window is a second observation of
    /// the same fact, not a second ask.
    #[test]
    fn the_same_ask_buzzes_exactly_once() {
        let mut seen = Seen::default();
        assert!(seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 7)))));
        assert!(!seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 7)))));
        assert!(!seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 7)))));
    }

    /// The other half: a genuinely new ask on the same pane MUST buzz. A pane-only dedupe key
    /// would swallow it, and the operator would never learn the agent asked again.
    /// An ask and a finish on the same pane are different things to say. A pane-only key would
    /// swallow the second, and "the agent finished" is the beat a continuity view most needs.
    #[test]
    fn a_finish_is_not_deduped_against_an_ask_on_the_same_pane() {
        let mut seen = Seen::default();
        let a = ask("w1:p1", 9);
        let done = Beat::Finished {
            pane: PaneId::new("w1:p1"),
            workspace: "w1".into(),
            agent: "opencode".into(),
            seq: 9,
            excerpt: String::new(),
        };
        assert!(seen.should_push(key_of(&Beat::Asked(a))));
        assert!(
            seen.should_push(key_of(&done)),
            "the finish was swallowed by the ask's key"
        );
    }

    /// `was_blocked` is what tells "went back to work after an ask" from ordinary churn — only the
    /// first is worth putting in the topic.
    #[test]
    fn resuming_is_only_reported_after_an_ask() {
        let mut seen = Seen::default();
        let p = PaneId::new("w1:p1");
        assert!(!seen.was_blocked(&p));
        seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 3))));
        assert!(seen.was_blocked(&p));
        seen.cleared(&p);
        assert!(!seen.was_blocked(&p));
    }

    #[test]
    fn a_new_ask_on_the_same_pane_does_buzz() {
        let mut seen = Seen::default();
        assert!(seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 7)))));
        assert!(seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 8)))));
    }

    /// Without `cleared`, a pane whose seq ever repeated a pushed value would go permanently
    /// silent — invisible until an ask is missed.
    #[test]
    fn clearing_a_pane_lets_a_repeated_seq_buzz_again() {
        let mut seen = Seen::default();
        assert!(seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 7)))));
        seen.cleared(&PaneId::new("w1:p1"));
        assert!(seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 7)))));
    }

    #[test]
    fn panes_are_tracked_independently() {
        let mut seen = Seen::default();
        assert!(seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 1)))));
        assert!(seen.should_push(key_of(&Beat::Asked(ask("w2:p1", 1)))));
        assert!(!seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 1)))));
    }

    #[test]
    fn panes_that_left_the_herd_are_forgotten() {
        let mut seen = Seen::default();
        seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 1))));
        seen.should_push(key_of(&Beat::Asked(ask("w2:p1", 1))));
        seen.retain_alive(&["w2:p1".to_string()].into_iter().collect());
        // w1:p1 is gone; if it ever returns, its ask is new.
        assert!(seen.should_push(key_of(&Beat::Asked(ask("w1:p1", 1)))));
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

    /// The excerpt is the difference between a notification and a conversation, so its failure
    /// modes matter: relaying whitespace, or cutting off the very line that is the question.
    #[test]
    fn the_excerpt_keeps_the_bottom_and_drops_blank_rows() {
        let pane = "old context\n\n\nDo you want me to force-push? [y/N]\n\n\n\n";
        let out = excerpt_from(pane);
        assert!(
            out.ends_with("Do you want me to force-push? [y/N]"),
            "the question must survive to the end: {out:?}"
        );
        // A single blank line survives, deliberately: it is paragraph structure, and a message
        // with every break squeezed out reads worse on a phone, not better. What must not survive
        // is a RUN of them, which is the TUI padding its frame.
        assert!(
            !out.contains("\n\n\n"),
            "a run of blank rows was relayed: {out:?}"
        );
    }

    /// The real pane the operator called out, scrubbed. What used to be relayed was the harness's
    /// furniture; what must be relayed is what it said.
    #[test]
    fn a_real_tui_pane_relays_content_not_chrome() {
        let raw = include_str!("../tests/fixtures/tui-pane.txt");
        let out = excerpt_from(raw);

        for junk in [
            "esc interrupt",
            "ctrl+p commands",
            "▀▀▀",
            "┃",
            "Example Plan",
        ] {
            assert!(
                !out.contains(junk),
                "chrome survived the clean: {junk:?} in\n{out}"
            );
        }
        assert!(
            out.contains("worktrees") || out.contains("git -C"),
            "the actual content was thrown away:\n{out}"
        );
    }

    #[test]
    fn the_gutter_is_stripped_but_indentation_survives() {
        let t = "┃  fn main() {\n┃      println!(\"hi\");\n┃  }";
        let out = strip_chrome(t);
        assert!(!out.contains('┃'));
        assert!(
            out.contains("  println!") || out.contains("    println!"),
            "code indentation was flattened: {out:?}"
        );
    }

    /// A right-hand column drawn into the same rows splits words across lines ("Coordinat" /
    /// "or"), which is unreadable and was the worst of it.
    #[test]
    fn a_bleeding_right_column_is_dropped() {
        assert!(has_column_bleed(
            "Coordinat ·task class: chain                LLM"
        ));
        assert!(has_column_bleed(
            "or                                          Gate"
        ));
        assert!(!has_column_bleed(
            "a normal sentence with single spaces in it"
        ));
        assert!(
            !has_column_bleed("  indented code line"),
            "leading indentation is not a column bleed"
        );
    }

    #[test]
    fn a_heavy_rule_cuts_the_footer_off() {
        let t = "real message\n╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀\n ⬝⬝■■  esc interrupt   113.0K";
        let out = strip_chrome(t);
        assert_eq!(out.trim(), "real message");
    }

    #[test]
    fn cleaning_an_empty_or_pure_chrome_pane_yields_nothing_rather_than_panicking() {
        assert_eq!(excerpt_from(""), "");
        assert_eq!(excerpt_from("┃\n┃\n╹▀▀▀▀▀▀▀▀▀▀▀▀"), "");
    }

    #[test]
    fn the_excerpt_is_capped_in_lines_and_characters() {
        let many = (0..200)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = excerpt_from(&many);
        assert!(out.lines().count() <= EXCERPT_LINES);
        assert!(out.ends_with("line 199"), "the tail is what matters");

        let wide = (0..200)
            .map(|i| format!("{i} {}", "x".repeat(200)))
            .collect::<Vec<_>>()
            .join("\n");
        let capped = excerpt_from(&wide);
        assert!(
            capped.chars().count() <= EXCERPT_CHARS + 1,
            "got {}",
            capped.chars().count()
        );
        assert!(
            capped.starts_with('…'),
            "truncation must be visible: {:?}",
            &capped[..8]
        );
    }

    /// D4's mitigation in code: asks and digests, never a full pane dump.
    #[test]
    fn a_busy_pane_cannot_become_a_transcript() {
        let transcript = (0..5000)
            .map(|i| format!("build step {i} ok"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = excerpt_from(&transcript);
        assert!(
            out.chars().count() <= EXCERPT_CHARS + 1,
            "a full pane reached Telegram's servers: {} chars",
            out.chars().count()
        );
    }

    #[test]
    fn an_empty_pane_yields_an_empty_excerpt_rather_than_panicking() {
        assert_eq!(excerpt_from(""), "");
        assert_eq!(excerpt_from("\n\n   \n"), "");
    }

    /// Multi-byte safety: pane tails are full of box-drawing glyphs and emoji.
    #[test]
    fn the_excerpt_is_char_safe() {
        let wide = (0..400)
            .map(|i| format!("│ ◑ 日本語 {i} ──"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = excerpt_from(&wide);
        assert!(out.chars().count() <= EXCERPT_CHARS + 1);
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

#[cfg(test)]
mod persistence_tests {
    use super::*;

    /// A redeploy must not re-buzz a question the operator has already read.
    ///
    /// The live push log showed `wA:p1 seq=198` pushed three times across three restarts, because
    /// the pushed set lived only in memory. Under `Restart=always` the bridge restarts for reasons
    /// the operator never sees.
    #[test]
    fn a_restart_does_not_re_push_an_ask_already_sent() {
        let dir = std::env::temp_dir().join(format!("herdr-tg-seen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("pushed.state.json");

        let ask = Ask {
            pane: PaneId::new("wA:p1"),
            workspace: "wA".into(),
            agent: "opencode".into(),
            seq: Some(198),
            excerpt: String::new(),
            options: Vec::new(),
        };

        let mut before = Seen::default();
        assert!(before.should_push(key_of(&Beat::Asked(ask.clone()))));
        before.save(&path).unwrap();

        // The bridge restarts.
        let mut after = Seen::load(&path);
        assert!(
            !after.should_push(key_of(&Beat::Asked(ask.clone()))),
            "the same question buzzed again after a restart"
        );

        // A genuinely new question on that pane still gets through.
        let newer = Ask {
            seq: Some(199),
            ..ask
        };
        assert!(after.should_push(key_of(&Beat::Asked(newer))));
    }

    #[test]
    fn a_missing_or_corrupt_file_costs_one_duplicate_not_the_bridge() {
        let dir = std::env::temp_dir().join(format!("herdr-tg-seen-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(Seen::load(&dir.join("absent.json")).pushed.is_empty());
        let corrupt = dir.join("corrupt.json");
        std::fs::write(&corrupt, "{not json").unwrap();
        assert!(Seen::load(&corrupt).pushed.is_empty());
    }
}

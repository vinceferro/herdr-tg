//! The slice's own proof: a question reaches a phone, a tap comes back, and the right project
//! hears about it — over a real socket, with Telegram behind a trait and a bridge that is not this
//! process's imagination of one.
//!
//! What is real here: the `UnixListener`, the framing, the handshake, the settling window, the
//! registry, the ledger on disk, and the audit. What is faked is exactly one thing — Telegram —
//! because a test that needed a bot token would never run.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use hub_proto::{
    AskEnd, AskOption, BridgeFrame, Delivered, Envelope, FrameId, FrameReader, HubFrame, MsgId,
    OptionId, write_frame,
};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex as AsyncMutex;

use super::*;
use crate::registry::Registry;

const ALLOWED_CHAT: i64 = -1001;
const SOMEONE_ELSE: i64 = 4242;

/// Telegram, counted. Every assertion about "exactly one" reads off these.
#[derive(Default)]
struct FakeTelegram {
    topics: AsyncMutex<Vec<(String, u8)>>,
    sends: AsyncMutex<Vec<(i32, String, Vec<AskOption>)>>,
    retired: AsyncMutex<Vec<(i32, MsgId, String)>>,
    next_msg: AtomicI64,
    /// Set to make the next send report the topic as deleted, for the rebinding test.
    topic_gone_once: AsyncMutex<bool>,
    /// Set to make retiring a keyboard fail, which is what an edit past Telegram's limit, or on a
    /// message older than 48 hours, actually does.
    retire_fails: AsyncMutex<bool>,
}

impl Surface for FakeTelegram {
    async fn create_topic(&self, title: &str, icon_color: u8) -> anyhow::Result<i32> {
        let mut t = self.topics.lock().await;
        t.push((title.to_owned(), icon_color));
        Ok(1000 + t.len() as i32)
    }

    async fn send(&self, topic_id: i32, text: &str, buttons: &[AskOption]) -> SendOutcome {
        {
            let mut gone = self.topic_gone_once.lock().await;
            if *gone {
                *gone = false;
                return SendOutcome::TopicGone;
            }
        }
        self.sends
            .lock()
            .await
            .push((topic_id, text.to_owned(), buttons.to_vec()));
        let n = self.next_msg.fetch_add(1, Ordering::Relaxed) + 1;
        SendOutcome::Sent(MsgId::new(format!("m{n}")))
    }

    async fn retire_buttons(
        &self,
        topic_id: i32,
        msg_id: &MsgId,
        original: &str,
        note: &str,
    ) -> anyhow::Result<()> {
        if *self.retire_fails.lock().await {
            anyhow::bail!("the edit was refused");
        }
        // Both halves are recorded, because a retirement that drops the question is exactly the
        // defect this signature grew a parameter to close.
        self.retired
            .lock()
            .await
            .push((topic_id, msg_id.clone(), format!("{original} || {note}")));
        Ok(())
    }
}

/// Everything a test needs: a hub on a real socket, and the secret to connect to it with.
struct Harness {
    hub: Arc<Hub<FakeTelegram>>,
    fake: Arc<FakeTelegram>,
    secret: String,
    project: ProjectId,
    sock: PathBuf,
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().expect("tmp");
    let repo = dir.path().join("herdr-tg");
    std::fs::create_dir_all(&repo).expect("repo");

    let mut registry = Registry::load(dir.path().join("projects.json"));
    let (project, secret) = registry.enrol(&repo).expect("enrols");

    let fake = Arc::new(FakeTelegram::default());
    let hub = Arc::new(
        Hub::new(
            Arc::clone(&fake),
            registry,
            AskLedger::load(dir.path().join("asks.json")),
            HubAudit::new(dir.path().join("hub.audit.log")),
            vec![ALLOWED_CHAT],
            ALLOWED_CHAT,
        )
        // The five-second window is the real one; a test that waited it out would be five seconds
        // slower for nothing. What is under test is that the window EXISTS and gates the topic.
        .with_settle(Duration::from_millis(500))
        // Likewise the budget: the real pacing is one message a second, which would make every
        // flow test below a stopwatch exercise. The limits are tested at their real values in
        // `queue.rs` and in `pacing_waits_but_a_real_flood_is_shed`.
        .with_budget(crate::queue::PER_MINUTE, Duration::from_millis(5)),
    );

    let sock = dir.path().join("hub.sock");
    let listener = UnixListener::bind(&sock).expect("bind");
    {
        let hub = Arc::clone(&hub);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let hub = Arc::clone(&hub);
                tokio::spawn(async move {
                    let _ = hub.serve_connection(stream).await;
                });
            }
        });
    }

    Harness {
        hub,
        fake,
        secret,
        project: project.id,
        sock,
        dir,
    }
}

/// A bridge, as a bridge really behaves: connect, say hello, answer the ping.
struct FakeBridge {
    reader: FrameReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
    seq: u64,
}

impl FakeBridge {
    async fn connect(sock: &Path, secret: &str, instance: &str, claimed_id: &str) -> Self {
        let stream = UnixStream::connect(sock).await.expect("connect");
        let (r, w) = stream.into_split();
        let mut me = Self {
            reader: FrameReader::new(r),
            writer: w,
            seq: 0,
        };
        me.send(BridgeFrame::Hello {
            project_id: ProjectId::new(claimed_id),
            token: secret.to_owned(),
            instance: instance.to_owned(),
            repo: "/wherever".into(),
            pid: std::process::id(),
        })
        .await;
        me
    }

    async fn send(&mut self, f: BridgeFrame) -> FrameId {
        self.seq += 1;
        let id = FrameId::new(format!("b{}", self.seq));
        write_frame(&mut self.writer, &Envelope::new(id.clone(), f))
            .await
            .expect("write");
        id
    }

    async fn next(&mut self) -> Option<Envelope<HubFrame>> {
        tokio::time::timeout(Duration::from_secs(3), self.reader.next::<HubFrame>())
            .await
            .ok()?
            .expect("read")
    }

    /// Read frames until one satisfies `want`, so a test is not written against frame order it
    /// does not actually care about.
    async fn wait_for<T>(&mut self, mut want: impl FnMut(&HubFrame) -> Option<T>) -> T {
        for _ in 0..20 {
            let Some(env) = self.next().await else { break };
            if let Some(v) = want(&env.payload) {
                return v;
            }
            if let HubFrame::Ping = env.payload {
                let r = env.id.clone();
                self.send(BridgeFrame::Pong { r#ref: r }).await;
            }
        }
        panic!("the hub never sent the frame this test was waiting for");
    }

    async fn become_live(&mut self) {
        let ping = self.next().await.expect("a welcome or a ping");
        let ping = match ping.payload {
            HubFrame::Welcome { .. } => self.next().await.expect("a ping"),
            _ => ping,
        };
        assert!(
            matches!(ping.payload, HubFrame::Ping),
            "expected a ping, got {:?}",
            ping.payload
        );
        self.send(BridgeFrame::Pong { r#ref: ping.id }).await;
    }
}

/// Wait for a condition the hub reaches asynchronously, rather than sleeping and hoping.
async fn until(mut cond: impl AsyncFnMut() -> bool) {
    for _ in 0..200 {
        if cond().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the hub never reached the state this test was waiting for");
}

#[tokio::test]
async fn an_ask_becomes_a_tap_becomes_a_choice() {
    let h = harness().await;

    // ── 1. hello → welcome, and the name is the REGISTRY's ────────────────────────────────────
    // The hello below claims to be "p-somebody-else". It is ignored: identity comes from the
    // secret, so a bridge cannot talk its way into another project's topic.
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", "p-somebody-else").await;

    let welcome = bridge.next().await.expect("a welcome");
    let HubFrame::Welcome {
        project, topic_id, ..
    } = welcome.payload
    else {
        panic!("expected a welcome, got {:?}", welcome.payload);
    };
    assert_eq!(
        project, "herdr-tg",
        "the title must come from the registry, not the wire"
    );
    assert_eq!(
        topic_id, None,
        "a topic must not exist before the bridge has proved it is there"
    );

    // ── the settling window: the topic appears only after the pong ────────────────────────────
    assert!(
        h.fake.topics.lock().await.is_empty(),
        "a topic was created for a bridge that had not answered yet"
    );
    let ping = bridge.next().await.expect("a ping");
    assert!(matches!(ping.payload, HubFrame::Ping));
    bridge.send(BridgeFrame::Pong { r#ref: ping.id }).await;

    until(async || !h.fake.sends.lock().await.is_empty()).await;

    {
        let topics = h.fake.topics.lock().await;
        assert_eq!(topics.len(), 1, "exactly one topic, got {topics:?}");
        assert_eq!(topics[0].0, "herdr-tg");
        assert!(
            topics[0].1 < 6,
            "colour {} is not one of Telegram's six",
            topics[0].1
        );

        let sends = h.fake.sends.lock().await;
        assert_eq!(sends.len(), 1, "exactly one greeting, got {sends:?}");
        assert!(sends[0].2.is_empty(), "a greeting must not carry buttons");
    }

    // ── 2. ask → exactly one send, verbatim, with two opaque buttons ──────────────────────────
    let words = "Overwrite deploy/prod.yaml?";
    bridge
        .send(BridgeFrame::Ask {
            ask_id: AskId::new("a1"),
            text: words.to_owned(),
            options: Some(vec![
                AskOption {
                    option_id: OptionId::new("y"),
                    label: "Yes, overwrite".into(),
                },
                AskOption {
                    option_id: OptionId::new("n"),
                    label: "No, stop".into(),
                },
            ]),
        })
        .await;

    until(async || h.fake.sends.lock().await.len() == 2).await;

    let (topic, text, buttons) = h.fake.sends.lock().await[1].clone();
    assert_eq!(topic, 1001, "the question went to the wrong topic");
    assert_eq!(
        text, words,
        "the agent's words must reach the operator unchanged"
    );
    assert_eq!(buttons.len(), 2);
    assert_eq!(buttons[0].label, "Yes, overwrite");

    // ── 3. the audit records the send BEFORE it happens, and its outcome after ─────────────────
    let audit = std::fs::read_to_string(h.hub.audit.path()).expect("an audit log");
    let sent_at = audit.find("sent\t").expect("a sent line");
    let done_at = audit.rfind("delivered\t").expect("an outcome line");
    assert!(
        sent_at < done_at,
        "the outcome was recorded before the send:\n{audit}"
    );

    // ── 4. what the buttons mean is written down, on disk, beside the message ─────────────────
    let ledger_raw = std::fs::read_to_string(h.dir.path().join("asks.json")).expect("a ledger");
    assert!(
        ledger_raw.contains("Yes, overwrite"),
        "the labels are not written down: {ledger_raw}"
    );
    assert!(ledger_raw.contains("a1"));

    // ── 5. a tap from the allowed chat becomes a choice the bridge receives ────────────────────
    let msg = MsgId::new("m2");
    let (project, ask_id, option_id) = h
        .hub
        .resolve_tap(ALLOWED_CHAT, &msg, &OptionId::new("y"))
        .await
        .expect("the tap resolves");
    assert_eq!(project, h.project);
    assert_eq!(ask_id, AskId::new("a1"));

    assert!(
        h.hub
            .deliver(
                &project,
                HubFrame::Choice {
                    msg_id: msg.clone(),
                    ask_id: ask_id.clone(),
                    option_id: option_id.clone(),
                }
            )
            .await
    );

    let got = bridge
        .wait_for(|f| match f {
            HubFrame::Choice {
                ask_id, option_id, ..
            } => Some((ask_id.clone(), option_id.clone())),
            _ => None,
        })
        .await;
    assert_eq!(got, (AskId::new("a1"), OptionId::new("y")));

    // ── 6. the SAME tap from a different chat resolves to nothing at all ──────────────────────
    let before = h.fake.sends.lock().await.len();
    let refused = h
        .hub
        .resolve_tap(SOMEONE_ELSE, &msg, &OptionId::new("y"))
        .await
        .expect_err("a stranger's tap must not resolve");
    assert_eq!(refused, TapRefusal::NotYours);
    assert_eq!(
        h.fake.sends.lock().await.len(),
        before,
        "a stranger's tap produced a message; it must produce silence"
    );
}

#[tokio::test]
async fn a_bridge_cannot_claim_another_projects_identity() {
    // Two enrolled projects. The second connects with ITS OWN secret while naming the first, which
    // is the cheapest possible impersonation attempt and the one a wire field invites.
    let h = harness().await;
    let other = h.dir.path().join("llm-gateway");
    std::fs::create_dir_all(&other).expect("repo");
    let (other_project, other_secret) = {
        let mut r = h.hub.registry.lock().await;
        r.enrol(&other).expect("enrols")
    };

    let mut bridge = FakeBridge::connect(&h.sock, &other_secret, "i1", h.project.as_str()).await;
    let welcome = bridge.next().await.expect("a welcome");
    let HubFrame::Welcome { project, .. } = welcome.payload else {
        panic!("expected a welcome");
    };
    assert_eq!(
        project, "llm-gateway",
        "the wire's project_id was believed over the secret"
    );

    bridge.become_live().await;
    until(async || h.hub.is_claimed(&other_project.id).await).await;
    assert!(
        !h.hub.is_claimed(&h.project).await,
        "a bridge took a claim on a project it has no secret for"
    );
}

#[tokio::test]
async fn a_second_bridge_for_a_live_project_is_refused_rather_than_swapped_in() {
    // A refusal, never a takeover. A takeover is what bridge-murder felt like from the inside: the
    // incumbent kept running and quietly stopped being heard.
    let h = harness().await;
    let mut first = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    first.become_live().await;
    until(async || h.hub.is_claimed(&h.project).await).await;

    let mut second = FakeBridge::connect(&h.sock, &h.secret, "i2", h.project.as_str()).await;
    let frame = second.next().await.expect("an answer");
    assert!(
        matches!(
            frame.payload,
            HubFrame::Refused {
                reason: RefusedReason::AlreadyClaimed
            }
        ),
        "the second bridge was not refused: {:?}",
        frame.payload
    );
}

#[tokio::test]
async fn a_secret_that_resolves_to_nothing_is_refused_without_saying_which_part_was_wrong() {
    let h = harness().await;
    let mut bridge =
        FakeBridge::connect(&h.sock, "not-a-real-secret", "i1", h.project.as_str()).await;
    let frame = bridge.next().await.expect("an answer");
    assert!(
        matches!(
            frame.payload,
            HubFrame::Refused {
                reason: RefusedReason::UnknownProject
            }
        ),
        "{:?}",
        frame.payload
    );
    // No topic, no greeting: nothing at all was created for a connection that never authenticated.
    assert!(h.fake.topics.lock().await.is_empty());
}

#[tokio::test]
async fn a_bridge_that_connects_and_never_answers_gets_no_topic() {
    // A channel plugin that is not allowlisted boots and exits in about a tenth of a second. From
    // the process table it looks exactly like a healthy worker, and a topic created for one of
    // those is an empty topic bound forever to a project whose bridge was never there.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    let _welcome = bridge.next().await.expect("a welcome");
    // Deliberately no pong.
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(
        h.fake.topics.lock().await.is_empty(),
        "a topic was created for a bridge that never answered"
    );
    assert!(
        !h.hub.is_claimed(&h.project).await,
        "a silent bridge kept its claim"
    );
}

#[tokio::test]
async fn a_tap_on_a_menu_from_a_session_that_has_restarted_is_refused_with_a_reason() {
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    bridge
        .send(BridgeFrame::Ask {
            ask_id: AskId::new("a1"),
            text: "still there?".into(),
            options: Some(vec![AskOption {
                option_id: OptionId::new("y"),
                label: "Yes".into(),
            }]),
        })
        .await;
    until(async || h.fake.sends.lock().await.len() == 2).await;

    // The worker restarts: same project, new instance. Every outstanding question belongs to a
    // process that no longer exists, and answering into its successor would put a reply somewhere
    // nobody asked for one.
    //
    // Waiting for the claim to clear is the real sequence, not a convenience. The first version of
    // this test connected the successor immediately and was refused `AlreadyClaimed` — correctly,
    // because the hub had not yet noticed the socket close and the pid it was holding was this very
    // test process. In production the two are different processes and the pid check settles it; in
    // a test that shares one pid, the disconnect is what has to be observed.
    drop(bridge);
    until(async || !h.hub.is_claimed(&h.project).await).await;

    let mut fresh = FakeBridge::connect(&h.sock, &h.secret, "i2", h.project.as_str()).await;
    fresh.become_live().await;
    until(async || h.hub.is_claimed(&h.project).await).await;

    let refused = h
        .hub
        .resolve_tap(ALLOWED_CHAT, &MsgId::new("m2"), &OptionId::new("y"))
        .await
        .expect_err("a tap for a dead session must not resolve");
    assert_eq!(refused, TapRefusal::Restarted);
    assert!(
        refused.say().contains("restarted"),
        "the operator is owed a reason: {}",
        refused.say()
    );
}

#[tokio::test]
async fn a_bridge_that_dies_without_reading_its_acks_still_releases_its_project() {
    // The regression, named for itself rather than caught in passing.
    //
    // A peer that closes while bytes are still unread in its receive buffer makes the kernel send
    // an RST, so the hub's next read fails with a connection reset instead of ending cleanly. The
    // release used to sit after a `?`, so that path skipped it and the project kept a claim nobody
    // was behind — its worker would restart, be refused as already-claimed, and go quiet with
    // nothing anywhere saying why. A bridge that ignores its acks is the ordinary case, not a
    // contrived one: acks are backpressure, and a busy bridge reads them late or never.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || h.hub.is_claimed(&h.project).await).await;

    // Send enough that the hub's acks are certainly sitting unread on this side. Most of these
    // are shed by the chat budget, which is fine and is not what this test is about — what matters
    // is that the hub processed them, so the audit is the thing to wait on rather than the sends.
    for n in 0..10 {
        bridge
            .send(BridgeFrame::Say {
                text: format!("line {n}"),
                hint: None,
            })
            .await;
    }
    until(async || {
        std::fs::read_to_string(h.hub.audit.path())
            .map(|a| {
                a.lines()
                    .filter(|l| l.contains("shed") || l.contains("delivered"))
                    .count()
                    >= 10
            })
            .unwrap_or(false)
    })
    .await;

    drop(bridge);

    until(async || !h.hub.is_claimed(&h.project).await).await;
    assert!(
        !h.hub.is_claimed(&h.project).await,
        "the project is still claimed by a bridge that is gone"
    );
}

#[tokio::test]
async fn a_crashed_bridge_does_not_lock_its_own_project_out() {
    // A worker that died without closing cleanly leaves a claim behind. If that claim were honoured
    // the project would be unreachable until someone found a keyboard — which is the opposite of
    // what a phone-only operator can do about it. A pid that is gone from /proc is evicted.
    let h = harness().await;

    // A pid that is genuinely not a process: spawn one, wait for it, and use the number it had.
    let mut child = std::process::Command::new("/bin/true")
        .spawn()
        .expect("spawn");
    let dead_pid = child.id();
    child.wait().expect("reap");
    assert!(
        !super::pid_is_alive(dead_pid),
        "the probe pid is somehow still alive"
    );

    let (tx, _rx) = tokio::sync::mpsc::channel(4);
    h.hub
        .claim(h.project.clone(), dead_pid, "i0".into(), tx)
        .await
        .expect("a dead incumbent must not block a claim");
    assert!(h.hub.is_claimed(&h.project).await);

    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    let welcome = bridge.next().await.expect("an answer");
    assert!(
        matches!(welcome.payload, HubFrame::Welcome { .. }),
        "a live bridge was refused because a dead one held the claim: {:?}",
        welcome.payload
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn two_bridges_arriving_together_do_not_both_get_the_project() {
    // The race the sequential version of this test cannot see, on the runtime the binary actually
    // builds (`main.rs` uses `new_multi_thread`).
    //
    // Gate 4 used to be a check in `admit` and an unconditional insert in `claim`, with several
    // awaits in between. Two bridges landing inside that window were BOTH admitted and the second
    // silently replaced the first — measured at roughly one round in three when the two hellos are
    // within about 100 µs of each other. The consequence is exactly what gate 4 exists to prevent:
    // two bridges live on one project, both posting into one topic, and a tap on the incumbent's
    // still-open question refused with "that session has since restarted" while it sits there
    // waiting for the answer.
    //
    // Both sockets are held open for the whole round on purpose. Dropping one would let the hub see
    // EOF and release the claim, and the second bridge would then be admitted legitimately — which
    // looks like the bug and is not.
    for round in 0..40 {
        let h = harness().await;
        let (sock, secret, project) = (h.sock.clone(), h.secret.clone(), h.project.clone());

        let a = tokio::spawn({
            let (sock, secret, project) = (sock.clone(), secret.clone(), project.clone());
            async move {
                let mut b = FakeBridge::connect(&sock, &secret, "iA", project.as_str()).await;
                let first = b.next().await.map(|e| e.payload);
                (b, first)
            }
        });
        let b = tokio::spawn({
            let (sock, secret, project) = (sock.clone(), secret.clone(), project.clone());
            async move {
                let mut b = FakeBridge::connect(&sock, &secret, "iB", project.as_str()).await;
                let first = b.next().await.map(|e| e.payload);
                (b, first)
            }
        });

        let (_ka, ra) = a.await.expect("bridge a");
        let (_kb, rb) = b.await.expect("bridge b");

        let welcomed = [&ra, &rb]
            .iter()
            .filter(|r| matches!(r, Some(HubFrame::Welcome { .. })))
            .count();
        assert_eq!(
            welcomed, 1,
            "round {round}: {welcomed} bridges were admitted for one project — \
             a={ra:?} b={rb:?}"
        );
        let refused = [&ra, &rb]
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    Some(HubFrame::Refused {
                        reason: RefusedReason::AlreadyClaimed
                    })
                )
            })
            .count();
        assert_eq!(refused, 1, "round {round}: the loser was not told why");
    }
}

#[tokio::test]
async fn a_question_asked_before_the_pong_is_kept_rather_than_swallowed() {
    // A bridge that opens with a question — which is the whole point of the product — used to have
    // it read during the settling window, discarded, and never acked. No message, no record, no
    // reply, and an agent blocked on an answer that could never come.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;

    // Welcome, then the question, and only THEN the pong.
    let welcome = bridge.next().await.expect("a welcome");
    assert!(matches!(welcome.payload, HubFrame::Welcome { .. }));
    bridge
        .send(BridgeFrame::Ask {
            ask_id: AskId::new("a1"),
            text: "asked before I was live".into(),
            options: Some(vec![AskOption {
                option_id: OptionId::new("y"),
                label: "Yes".into(),
            }]),
        })
        .await;

    // Answer the ping explicitly. `wait_for` only auto-pongs frames its predicate REJECTS, so a
    // predicate that matches the ping returns without ever answering it — and the bridge would
    // never become live, which is not what this test is about.
    let ping = bridge.next().await.expect("a ping");
    assert!(matches!(ping.payload, HubFrame::Ping), "{:?}", ping.payload);
    bridge.send(BridgeFrame::Pong { r#ref: ping.id }).await;

    until(async || {
        h.fake
            .sends
            .lock()
            .await
            .iter()
            .any(|(_, t, _)| t == "asked before I was live")
    })
    .await;
}

#[tokio::test]
async fn an_answer_id_that_will_not_fit_on_a_button_is_refused_where_the_bridge_can_hear_it() {
    // Telegram gives a button 64 bytes of callback_data. The option id is minted by the BRIDGE, so
    // it is agent-authored and arbitrary. Too long and the API refuses the whole message, leaving
    // an agent blocked on a question that was never asked; a `|` survives the send and then splits
    // wrong on the way back, so the tap resolves to nothing while looking perfectly fine.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    for bad in ["x".repeat(70), "yes|no".to_owned()] {
        let sent = bridge
            .send(BridgeFrame::Ask {
                ask_id: AskId::new("a1"),
                text: "ok?".into(),
                options: Some(vec![AskOption {
                    option_id: OptionId::new(bad.clone()),
                    label: "Yes".into(),
                }]),
            })
            .await;

        let (delivered, why) = bridge
            .wait_for(|f| match f {
                HubFrame::Ack {
                    r#ref,
                    delivered,
                    why,
                } if r#ref == &sent => Some((*delivered, *why)),
                _ => None,
            })
            .await;
        assert_eq!(
            delivered,
            Delivered::No,
            "an unsendable question was acked as delivered"
        );
        assert_eq!(why, Some(hub_proto::AckWhy::TelegramRefused));

        // And the operator is told, because an agent blocked on a question that never arrived must
        // not be visible only in a log.
        assert!(
            h.fake
                .sends
                .lock()
                .await
                .iter()
                .any(|(_, t, _)| t.contains("could not put on a button")),
            "nothing in the topic said the project was stuck"
        );
    }
}

#[tokio::test]
async fn a_tap_answered_from_the_phone_takes_the_keyboard_away() {
    // Deleting the ledger record alone left the menu live forever: the record the later
    // `ask_resolved` needed was already gone, so nothing ever retired the buttons, and an answered
    // question stayed tappable.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    bridge
        .send(BridgeFrame::Ask {
            ask_id: AskId::new("a1"),
            text: "Overwrite it?".into(),
            options: Some(vec![AskOption {
                option_id: OptionId::new("y"),
                label: "Yes".into(),
            }]),
        })
        .await;
    until(async || h.fake.sends.lock().await.len() == 2).await;

    h.hub
        .answered_from_phone(ALLOWED_CHAT, &MsgId::new("m2"), "Yes")
        .await;

    let retired = h.fake.retired.lock().await;
    let (_, msg, body) = retired.first().expect("the keyboard was never retired");
    assert_eq!(msg, &MsgId::new("m2"));
    assert!(
        body.contains("Overwrite it?"),
        "the question was thrown away instead of kept beside its answer: {body}"
    );
    assert!(body.contains("answered from your phone"), "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn several_projects_sending_at_once_all_get_through() {
    // The half of the pacing fix that a single-sender test cannot see, and it was open.
    //
    // The first version let each sender sleep ONCE and then shed. With more than one project every
    // waiter sleeps the same second, they all wake together, one wins the token, and the rest have
    // already spent their single sleep and fall through to the shed. Measured on the real code:
    // ten projects saying one thing each produced two sends and eight sheds, with sixteen of the
    // eighteen per-minute tokens unspent. Six bridges opening with a question left five agents
    // blocked and four topics bound-but-empty.
    //
    // Real budget, no `with_budget`: what is under test is the rhythm itself.
    let h = harness().await;
    let hub = Arc::new(Hub::new(
        Arc::clone(&h.fake),
        Registry::load(h.dir.path().join("projects.json")),
        AskLedger::load(h.dir.path().join("asks4.json")),
        HubAudit::new(h.dir.path().join("hub4.audit.log")),
        vec![ALLOWED_CHAT],
        ALLOWED_CHAT,
    ));
    // Bind the topic first so the greeting is not part of what is being counted.
    hub.registry
        .lock()
        .await
        .bind_topic(&h.project, 1001)
        .expect("bind");

    const SENDERS: usize = 6;
    let mut tasks = Vec::new();
    for n in 0..SENDERS {
        let hub = Arc::clone(&hub);
        let project = h.project.clone();
        tasks.push(tokio::spawn(async move {
            hub.say(&project, &format!("project {n} says something"), &[])
                .await
        }));
    }

    let mut sent = 0;
    let mut shed = Vec::new();
    for t in tasks {
        match t.await.expect("a sender") {
            SendOutcome::Sent(_) | SendOutcome::Clamped(_) => sent += 1,
            other => shed.push(other),
        }
    }
    assert_eq!(
        sent,
        SENDERS,
        "{} of {SENDERS} messages were shed while the per-minute ceiling was nowhere near: {shed:?}",
        shed.len()
    );
}

#[tokio::test]
async fn a_registry_that_cannot_be_read_is_never_written_over() {
    // A fix that introduced a worse defect than the one it closed. `reread` called `load`, which
    // reports an unreadable file by returning an EMPTY map — right for a constructor, catastrophic
    // here: one failed read un-enrolled every project in the shared handle, and the very next save
    // wrote that emptiness to disk. The hub would have erased the enrolments it exists to protect.
    let d = tempfile::tempdir().expect("tmp");
    let path = d.path().join("projects.json");
    let repo = d.path().join("herdr-tg");
    std::fs::create_dir_all(&repo).expect("repo");

    let mut registry = Registry::load(&path);
    let (project, secret) = registry.enrol(&repo).expect("enrols");
    let good = std::fs::read_to_string(&path).expect("readable");

    // The file becomes unreadable while the hub is running — a truncated write, a bad byte, a disk
    // that answered badly once.
    std::fs::write(&path, "{ not json at all").expect("corrupt it");

    assert!(
        registry.bind_topic(&project.id, 99).is_err(),
        "the hub wrote to a registry it could not read"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("still there"),
        "{ not json at all",
        "the unreadable file was overwritten — the only copy of what was enrolled"
    );
    assert!(
        registry.resolve(&secret).is_some(),
        "a failed read emptied the in-memory registry, so every project was refused"
    );

    // And once the file is good again, everything works and nothing was lost.
    std::fs::write(&path, good).expect("restore");
    registry
        .bind_topic(&project.id, 99)
        .expect("binds once readable");
    assert_eq!(
        Registry::load(&path)
            .get(&project.id)
            .and_then(|p| p.topic_id),
        Some(99)
    );
}

#[tokio::test]
async fn a_retirement_that_fails_keeps_the_record_so_it_can_be_retired_later() {
    // Fail closed: the order is "the buttons are gone, therefore the record may go", never the
    // reverse. Forgetting first left a live keyboard with nothing behind it — still tappable, still
    // offering a choice already made, and the next tap answered "I have no record of that question".
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    bridge
        .send(BridgeFrame::Ask {
            ask_id: AskId::new("a1"),
            text: "Overwrite it?".into(),
            options: Some(vec![AskOption {
                option_id: OptionId::new("y"),
                label: "Yes".into(),
            }]),
        })
        .await;
    until(async || h.fake.sends.lock().await.len() == 2).await;

    *h.fake.retire_fails.lock().await = true;
    h.hub
        .answered_from_phone(ALLOWED_CHAT, &MsgId::new("m2"), "Yes")
        .await;

    assert!(
        h.hub
            .ledger
            .lock()
            .await
            .get(ALLOWED_CHAT, &MsgId::new("m2"))
            .is_some(),
        "the record was forgotten even though the keyboard is still on the operator's phone"
    );
}

#[tokio::test]
async fn a_question_answered_once_can_never_be_answered_twice() {
    // The defect the previous round's own fix created. Keeping the record after a failed
    // retirement was right — the keyboard still needs taking away — but the record's PRESENCE was
    // the authorisation, so the question stayed re-tappable on a keyboard the operator is still
    // looking at. A second tap delivered a second, contradicting answer into a live agent.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    bridge
        .send(BridgeFrame::Ask {
            ask_id: AskId::new("a1"),
            text: "Overwrite it?".into(),
            options: Some(vec![
                AskOption {
                    option_id: OptionId::new("y"),
                    label: "Yes".into(),
                },
                AskOption {
                    option_id: OptionId::new("n"),
                    label: "No".into(),
                },
            ]),
        })
        .await;
    until(async || h.fake.sends.lock().await.len() == 2).await;

    // The retirement fails, which is what a 429, a network blip, or a message past its edit window
    // all look like — so the keyboard is still there.
    *h.fake.retire_fails.lock().await = true;
    let msg = MsgId::new("m2");
    let (project, ask_id, option) = h
        .hub
        .resolve_tap(ALLOWED_CHAT, &msg, &OptionId::new("y"))
        .await
        .expect("the first tap resolves");
    assert!(
        h.hub
            .deliver(
                &project,
                HubFrame::Choice {
                    msg_id: msg.clone(),
                    ask_id,
                    option_id: option,
                }
            )
            .await
    );
    h.hub.answered_from_phone(ALLOWED_CHAT, &msg, "Yes").await;

    // The record survives, so the keyboard can still be retired later.
    assert!(
        h.hub.ledger.lock().await.get(ALLOWED_CHAT, &msg).is_some(),
        "the record was dropped, so the live keyboard can never be retired"
    );

    // And the still-live keyboard answers nothing.
    let refused = h
        .hub
        .resolve_tap(ALLOWED_CHAT, &msg, &OptionId::new("n"))
        .await
        .expect_err("a second, contradicting answer reached the agent");
    assert_eq!(refused, TapRefusal::AlreadyAnswered);
    assert!(
        refused.say().contains("already been answered"),
        "{}",
        refused.say()
    );

    // Not even the same answer again — one tap, one Choice.
    assert!(
        h.hub
            .resolve_tap(ALLOWED_CHAT, &msg, &OptionId::new("y"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_frame_the_hub_cannot_read_before_the_pong_does_not_kill_the_connection() {
    // The post-pong loop was taught to survive a frame it cannot decode — which is exactly what a
    // bridge one version ahead sends — and the settling window was not. It ended the connection and
    // then audited it as "connected but never answered", blaming the bridge for the hub's own
    // strictness on the one path where a bridge has not yet had a chance to say anything.
    let h = harness().await;
    let stream = tokio::net::UnixStream::connect(&h.sock)
        .await
        .expect("connect");
    let (r, mut w) = stream.into_split();
    let mut reader = FrameReader::new(r);

    write_frame(
        &mut w,
        &Envelope::new(
            FrameId::new("b1"),
            BridgeFrame::Hello {
                project_id: ProjectId::new("whatever"),
                token: h.secret.clone(),
                instance: "i1".into(),
                repo: "/wherever".into(),
                pid: std::process::id(),
            },
        ),
    )
    .await
    .expect("hello");

    let welcome = reader
        .next::<HubFrame>()
        .await
        .expect("read")
        .expect("a welcome");
    assert!(matches!(welcome.payload, HubFrame::Welcome { .. }));

    // A line this build cannot decode, before the pong.
    tokio::io::AsyncWriteExt::write_all(
        &mut w,
        b"{\"v\":1,\"id\":\"x\",\"t\":\"say\",\"text\":12345}\n",
    )
    .await
    .expect("write garbage");

    let ping = reader
        .next::<HubFrame>()
        .await
        .expect("read")
        .expect("a ping");
    assert!(matches!(ping.payload, HubFrame::Ping), "{:?}", ping.payload);
    write_frame(
        &mut w,
        &Envelope::new(FrameId::new("b2"), BridgeFrame::Pong { r#ref: ping.id }),
    )
    .await
    .expect("pong");

    // The connection survived the bad frame and became live.
    until(async || !h.fake.topics.lock().await.is_empty()).await;
    assert!(
        h.hub.is_claimed(&h.project).await,
        "one undecodable frame killed a connection that went on to answer"
    );
}

#[tokio::test]
async fn a_bridge_that_talks_before_answering_is_stopped_rather_than_buffered_without_limit() {
    // The buffer that keeps a question asked before the pong was an unbounded Vec — one connection
    // could hand the hub as much as it could write in the settling window, before it had proved it
    // was there at all.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    let welcome = bridge.next().await.expect("a welcome");
    assert!(matches!(welcome.payload, HubFrame::Welcome { .. }));

    // Well past the 64-frame bound, and never a pong.
    for n in 0..200 {
        bridge
            .send(BridgeFrame::Say {
                text: format!("flood {n}"),
                hint: None,
            })
            .await;
    }

    until(async || !h.hub.is_claimed(&h.project).await).await;
    assert!(
        h.fake.topics.lock().await.is_empty(),
        "a bridge that never answered was given a topic"
    );
    let audit = std::fs::read_to_string(h.hub.audit.path()).unwrap_or_default();
    assert!(
        audit.contains("more before answering"),
        "the flood was reported as ordinary silence, which sends the operator the wrong way:\n{audit}"
    );
}

/// The two halves, meeting for the first time.
///
/// Everything else in this file tests the hub against a fake bridge, and
/// `plugins/kickoff-channel/test-against-a-fake-hub.ts` tests the bridge against a fake hub. Both
/// can pass while the two disagree about the wire — which is the failure a pair of fakes is
/// structurally unable to catch, because each was written from the same reading of the spec.
///
/// This is the REAL plugin process, on bun, over a REAL socket, against the REAL hub. Only Telegram
/// is faked, because a test that needed a bot token would never run.
///
/// Ignored by default: it needs bun and a `bun install`, which the Rust suite has no business
/// requiring. Run it deliberately:
///
///     cargo test -p herdr-tg the_real_plugin -- --ignored --nocapture
#[tokio::test]
#[ignore = "needs bun and the plugin's dependencies; run it deliberately"]
async fn the_real_plugin_and_the_real_hub_agree_on_the_wire() {
    let h = harness().await;

    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/kickoff-channel")
        .canonicalize()
        .expect("the plugin is in the repo");

    // The plugin reads its secret from <repo>/.kickoff/hub.token, exactly as a real project does —
    // and `repo` is the directory the harness actually enrolled, not a stand-in beside it.
    let repo = h.dir.path().join("herdr-tg");
    std::fs::create_dir_all(repo.join(".kickoff")).expect("repo");
    std::fs::write(repo.join(".kickoff/hub.token"), &h.secret).expect("token");

    let mut child = tokio::process::Command::new("bun")
        .arg("server.ts")
        .current_dir(&plugin)
        .env("KICKOFF_HUB_SOCKET", &h.sock)
        // Byte for byte how Claude Code starts it: cwd is the plugin, and the project is named only
        // here. The socket is the one thing a tempdir harness cannot help overriding.
        .env("CLAUDE_PROJECT_DIR", &repo)
        // The same hazard as in the test below: a leaked bun holds the inherited stderr pipe open
        // and `cargo test` waits on it forever, so a failure here would hang instead of failing.
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("bun is on PATH");

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout")).lines();

    // The MCP handshake, so the tools and notifications are the real ones.
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"t\",\"version\":\"0\"}}}\n")
        .await
        .expect("initialize");
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .expect("initialized");

    // The hub admits it, settles it, and gives it a topic — all through the real handshake.
    until(async || !h.fake.sends.lock().await.is_empty()).await;
    {
        let topics = h.fake.topics.lock().await;
        assert_eq!(
            topics.len(),
            1,
            "the real plugin did not get a topic: {topics:?}"
        );
        assert_eq!(
            topics[0].0, "herdr-tg",
            "the title did not come from the registry"
        );
    }

    // The agent asks. The question has to reach Telegram with its buttons intact.
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"ask\",\"arguments\":{\"text\":\"Overwrite deploy/prod.yaml?\",\"options\":[{\"id\":\"y\",\"label\":\"Yes, overwrite\"},{\"id\":\"n\",\"label\":\"No, stop\"}]}}}\n")
        .await
        .expect("ask");

    until(async || h.fake.sends.lock().await.len() >= 2).await;
    let (topic, text, buttons) = h.fake.sends.lock().await[1].clone();
    assert_eq!(
        text, "Overwrite deploy/prod.yaml?",
        "the agent's words changed on the way"
    );
    assert_eq!(buttons.len(), 2);
    assert_eq!(buttons[0].label, "Yes, overwrite");
    assert_eq!(topic, 1001);

    // The operator taps. The answer has to arrive as a MESSAGE in the agent's turn — the whole
    // safety story of this design — and it has to name the question it answers.
    let msg = MsgId::new("m2");
    let (project, ask_id, option) = h
        .hub
        .resolve_tap(ALLOWED_CHAT, &msg, &OptionId::new("y"))
        .await
        .expect("the tap resolves against the real plugin's ask");
    assert!(
        h.hub
            .deliver(
                &project,
                HubFrame::Choice {
                    msg_id: msg,
                    ask_id: ask_id.clone(),
                    option_id: option
                }
            )
            .await
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut got = None;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), stdout.next_line()).await {
            Ok(Ok(Some(line))) => {
                if line.contains("notifications/claude/channel") && line.contains("option_id") {
                    got = Some(line);
                    break;
                }
            }
            _ => break,
        }
    }
    let got = got.expect("the answer never reached the agent as a channel message");
    assert!(
        got.contains(ask_id.as_str()),
        "the answer did not name its question: {got}"
    );
    assert!(
        got.contains("\"option_id\":\"y\""),
        "the wrong option reached the agent: {got}"
    );

    // And the other direction: the operator types, and it arrives as a MESSAGE in the agent's own
    // turn. This is the half that used to be keystrokes in a terminal.
    assert!(
        h.hub
            .relay(
                &project,
                ALLOWED_CHAT,
                7,
                &MsgId::new("m9"),
                "use --dry-run first"
            )
            .await
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut typed = None;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), stdout.next_line()).await {
            Ok(Ok(Some(line))) => {
                if line.contains("notifications/claude/channel") && line.contains("--dry-run") {
                    typed = Some(line);
                    break;
                }
            }
            _ => break,
        }
    }
    let typed = typed.expect("what the operator typed never reached the agent");
    assert!(
        typed.contains("use --dry-run first"),
        "his words changed on the way: {typed}"
    );

    let _ = child.kill().await;
}

/// The bridge has to work out which project it is before it can prove it, and for a while it got
/// that from `process.cwd()`. For an MCP server started out of a plugin manifest, cwd is the PLUGIN
/// directory — so it looked for the secret at `<plugin>/.kickoff/hub.token`, found nothing, and
/// said so only on a stderr stream that reaches neither the agent nor the operator. Every message
/// the agent believed it had sent went nowhere, silently, for as long as that lasted.
///
/// So this starts the bridge the way Claude Code starts it and no other way: cwd is the plugin, and
/// the only thing naming the project is `CLAUDE_PROJECT_DIR`. There used to be a second variable
/// that could name it, set by the tests and by nothing in production — which is precisely how a
/// bridge that could never find its secret passed every test it had.
///
/// It shares the `the_real_plugin` prefix with the test above because that string is the filter
/// `scripts/install-channel-plugin.sh` runs, and a bun test outside that filter is one nothing runs.
#[tokio::test]
#[ignore = "needs bun and the plugin's dependencies; run it deliberately"]
async fn the_real_plugin_finds_its_project_in_the_directory_claude_code_names_and_not_its_own() {
    let h = harness().await;

    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/kickoff-channel")
        .canonicalize()
        .expect("the plugin is in the repo");

    // The directory the harness actually enrolled, with its secret where a real project keeps it.
    let repo = h.dir.path().join("herdr-tg");
    std::fs::create_dir_all(repo.join(".kickoff")).expect("repo");
    std::fs::write(repo.join(".kickoff/hub.token"), &h.secret).expect("token");

    let mut child = tokio::process::Command::new("bun")
        .arg("server.ts")
        .current_dir(&plugin)
        .env("KICKOFF_HUB_SOCKET", &h.sock)
        .env("CLAUDE_PROJECT_DIR", &repo)
        // Without this, a FAILING assertion below leaks the bun process, which keeps the inherited
        // stderr pipe open and leaves `cargo test` waiting on it forever. A red test has to be able
        // to go red out loud.
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("bun is on PATH");

    use tokio::io::AsyncWriteExt;
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"t\",\"version\":\"0\"}}}\n")
        .await
        .expect("initialize");
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .expect("initialized");

    // A topic is only ever created for a connection the hub ADMITTED, and it admits on the secret.
    // So a topic here is proof the bridge read the right directory: nothing else could have got it
    // a secret the registry recognises.
    until(async || !h.fake.topics.lock().await.is_empty()).await;
    let topics = h.fake.topics.lock().await;
    assert_eq!(
        topics.len(),
        1,
        "the bridge never proved who it was: it did not find the secret in the directory it was given"
    );
    assert_eq!(
        topics[0].0, "herdr-tg",
        "the title did not come from the registry"
    );

    drop(topics);
    let _ = child.kill().await;
}

/// `CLAUDE_PROJECT_DIR` is the folder `claude` was STARTED in, which is routinely a folder deep
/// inside the repo rather than its top. Joining `.kickoff/hub.token` onto it and stopping there was
/// the original defect with a new wrong directory substituted in: the bridge looked for a secret
/// nobody had enrolled, refused honestly, and the operator's phone stayed exactly as silent. The
/// advice it printed made it worse — it named the subfolder, and enrolling that mints a SECOND
/// project for one repository.
///
/// So the bridge searches upward, bounded by the top of the working tree, and this starts it three
/// folders down to prove it.
#[tokio::test]
#[ignore = "needs bun and the plugin's dependencies; run it deliberately"]
async fn the_real_plugin_finds_its_project_when_the_session_started_in_a_folder_deep_inside_it() {
    let h = harness().await;

    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/kickoff-channel")
        .canonicalize()
        .expect("the plugin is in the repo");

    // A real working tree, because the search stops at the top of one and git is what names it.
    let repo = h.dir.path().join("herdr-tg");
    std::fs::create_dir_all(repo.join(".kickoff")).expect("repo");
    std::fs::write(repo.join(".kickoff/hub.token"), &h.secret).expect("token");
    let git = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["init", "-q"])
        .status()
        .expect("run git");
    assert!(git.success(), "git init failed");

    let deep = repo.join("crates/herdr-tg/src");
    std::fs::create_dir_all(&deep).expect("a folder deep in the repo");

    let mut child = tokio::process::Command::new("bun")
        .arg("server.ts")
        .current_dir(&plugin)
        .env("KICKOFF_HUB_SOCKET", &h.sock)
        // The one difference from the test above, and the whole point of this one.
        .env("CLAUDE_PROJECT_DIR", &deep)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("bun is on PATH");

    use tokio::io::AsyncWriteExt;
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"t\",\"version\":\"0\"}}}\n")
        .await
        .expect("initialize");
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .expect("initialized");

    // Same proof as the test above: only a secret the registry recognises gets a topic created.
    until(async || !h.fake.topics.lock().await.is_empty()).await;
    let topics = h.fake.topics.lock().await;
    assert_eq!(
        topics.len(),
        1,
        "a session started inside the project could not find the project"
    );
    assert_eq!(
        topics[0].0, "herdr-tg",
        "it proved itself as something other than the enrolled project"
    );

    drop(topics);
    let _ = child.kill().await;
}

#[tokio::test]
async fn what_the_operator_types_reaches_the_agent_as_a_message_in_its_own_turn() {
    // The other direction of the round trip, and the whole safety story of the redesign: his words
    // arrive as a MESSAGE the agent reads in its own turn, never as keystrokes in a terminal. The
    // two-writer race that made the old path unsafe has no mechanism here.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    // The topic is bound, so the hub can tell which project a message typed there belongs to.
    let topic = h.hub.topic_for(&h.project).await.expect("a topic");
    assert_eq!(
        h.hub.project_for_topic(topic).await.as_ref(),
        Some(&h.project),
        "the hub could not tell which project owns its own topic"
    );

    assert!(
        h.hub
            .relay(
                &h.project,
                ALLOWED_CHAT,
                7,
                &MsgId::new("m9"),
                "try it with --dry-run first"
            )
            .await
    );

    let got = bridge
        .wait_for(|f| match f {
            HubFrame::Message { text, from, .. } => Some((text.clone(), from.chat_id)),
            _ => None,
        })
        .await;
    assert_eq!(
        got.0, "try it with --dry-run first",
        "his words changed on the way"
    );
    assert_eq!(got.1, ALLOWED_CHAT);
}

#[tokio::test]
async fn a_message_from_a_chat_this_bot_does_not_answer_reaches_nobody() {
    // The allowlist runs first, before anything else looks at the message. It is the ONLY scope
    // boundary now that one bot serves every project, so it is checked in the relay itself rather
    // than only in the handler above it — a second caller is a second way around.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    assert!(
        !h.hub
            .relay(&h.project, 4242, 7, &MsgId::new("m9"), "let me in")
            .await,
        "a stranger's message was relayed to an agent"
    );
}

#[tokio::test]
async fn a_message_for_a_project_that_is_not_connected_is_dropped_rather_than_queued() {
    // Never queued. A message held for a worker that may never come back is a message the operator
    // believes was sent, and he finds out it was not at the worst possible moment.
    let h = harness().await;
    assert!(
        !h.hub
            .relay(
                &h.project,
                ALLOWED_CHAT,
                7,
                &MsgId::new("m9"),
                "anyone there?"
            )
            .await
    );
    let audit = std::fs::read_to_string(h.hub.audit.path()).unwrap_or_default();
    assert!(
        audit.contains("not connected"),
        "a dropped message left no record:\n{audit}"
    );
}

#[tokio::test]
async fn a_tap_on_a_button_nobody_wrote_down_is_refused() {
    let h = harness().await;
    let refused = h
        .hub
        .resolve_tap(ALLOWED_CHAT, &MsgId::new("m404"), &OptionId::new("y"))
        .await
        .expect_err("nothing was written down");
    assert_eq!(refused, TapRefusal::NoRecord);
}

#[tokio::test]
async fn a_deleted_topic_is_rebound_once_rather_than_retried_forever() {
    // Telegram emits no service message when a forum topic is deleted and offers no way to list
    // them, so `message thread not found` on a send is the only way the hub ever finds out.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    *h.fake.topic_gone_once.lock().await = true;
    bridge
        .send(BridgeFrame::Say {
            text: "after the topic went".into(),
            hint: None,
        })
        .await;

    // Wait for the MESSAGE, not for the topic count. The new topic is created and greeted first,
    // so a test that waited on the count could look before the message had been re-sent — and would
    // then report the message lost when it was merely not there yet.
    until(async || {
        h.fake
            .sends
            .lock()
            .await
            .iter()
            .any(|(_, text, _)| text == "after the topic went")
    })
    .await;
    let sends = h.fake.sends.lock().await;
    let (topic, _, _) = sends
        .iter()
        .find(|(_, text, _)| text == "after the topic went")
        .expect("the message was lost in the rebinding");
    assert_eq!(
        *topic, 1002,
        "the message went back into the topic that is gone"
    );
}

#[tokio::test]
async fn a_question_that_stops_being_asked_has_its_buttons_taken_away() {
    // The single biggest thing a socket can do that a rendered screen never could: a screen cannot
    // tell you that a question is no longer being asked.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    bridge
        .send(BridgeFrame::Ask {
            ask_id: AskId::new("a1"),
            text: "ok?".into(),
            options: Some(vec![AskOption {
                option_id: OptionId::new("y"),
                label: "Yes".into(),
            }]),
        })
        .await;
    until(async || h.fake.sends.lock().await.len() == 2).await;

    bridge
        .send(BridgeFrame::AskResolved {
            ask_id: AskId::new("a1"),
            how: AskEnd::Answered,
            outcome: Some("No".into()),
        })
        .await;

    until(async || !h.fake.retired.lock().await.is_empty()).await;
    let retired = h.fake.retired.lock().await;
    assert_eq!(retired[0].1, MsgId::new("m2"));
    assert!(
        retired[0].2.contains("answered at the terminal"),
        "{}",
        retired[0].2
    );
    assert!(retired[0].2.contains("No"));

    // And the record goes with it: a keyboard that is gone must not still resolve.
    let refused = h
        .hub
        .resolve_tap(ALLOWED_CHAT, &MsgId::new("m2"), &OptionId::new("y"))
        .await
        .expect_err("a retired question must not still answer");
    assert_eq!(refused, TapRefusal::NoRecord);
}

#[tokio::test]
async fn every_frame_a_bridge_sends_gets_exactly_one_ack() {
    // A rejected send used to be one error log and a drop, which already lost 5,164 characters of a
    // real agent's longest message. Backpressure has to reach the party that can act on it.
    let h = harness().await;
    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    bridge.become_live().await;
    until(async || !h.fake.sends.lock().await.is_empty()).await;

    let mut sent = Vec::new();
    for n in 0..5 {
        sent.push(
            bridge
                .send(BridgeFrame::Say {
                    text: format!("line {n}"),
                    hint: None,
                })
                .await,
        );
    }

    let mut acked = Vec::new();
    let mut refused = 0;
    while acked.len() < sent.len() {
        let Some(env) = bridge.next().await else {
            break;
        };
        if let HubFrame::Ack {
            r#ref,
            delivered,
            why,
        } = env.payload
        {
            // Not every frame is delivered, and that is the point of the ack having three values.
            // A burst of five lands in a chat budget of eighteen a minute with a one-second gap, so
            // most of them are shed — and each one says so, with a reason the bridge can act on.
            // The old shape of this failure was a single error log and a drop, which had already
            // lost 5,164 characters of a real agent's longest message.
            // Whether these are delivered or shed depends on the budget, and that is not what this
            // test is about. What must hold either way is that a refusal SAYS WHY: the old shape of
            // this failure was one error log and a drop, which had already lost 5,164 characters of
            // a real agent's longest message.
            if delivered == Delivered::No {
                refused += 1;
                assert_eq!(
                    why,
                    Some(hub_proto::AckWhy::TooFast),
                    "a frame was refused without saying why"
                );
            }
            acked.push(r#ref);
        }
    }
    assert_eq!(acked, sent, "acks did not match the frames one for one");
    // Under the test budget these are paced rather than shed, and either is a correct outcome for
    // five frames. The invariant this test exists for is the ack, not the verdict.
    let _ = refused;
}

#[tokio::test]
async fn pacing_waits_but_a_real_flood_is_shed() {
    // At the REAL limits, and the distinction cost a live defect to find.
    //
    // Telegram's ceiling has two halves: no more than one message a second, and about twenty a
    // minute. The gap is a rhythm — waiting a beat costs nothing and the message still arrives —
    // while the per-minute ceiling is a real limit past which something has to give. Treating both
    // as "refuse it" made a project's FIRST question vanish: the greeting goes out the instant a
    // bridge connects, and the question that follows lands inside the one-second gap.
    let h = harness().await;
    let hub = Arc::new(Hub::new(
        Arc::clone(&h.fake),
        Registry::load(h.dir.path().join("projects.json")),
        AskLedger::load(h.dir.path().join("asks2.json")),
        HubAudit::new(h.dir.path().join("hub2.audit.log")),
        vec![ALLOWED_CHAT],
        ALLOWED_CHAT,
    )); // deliberately NOT with_budget: this one runs at the real limits

    let before = h.fake.sends.lock().await.len();
    let started = std::time::Instant::now();
    let first = hub.say(&h.project, "one", &[]).await;
    let second = hub.say(&h.project, "two", &[]).await;

    assert!(matches!(first, SendOutcome::Sent(_)), "{first:?}");
    assert!(
        matches!(second, SendOutcome::Sent(_)),
        "a message inside the one-second gap was DROPPED rather than paced: {second:?}"
    );
    assert!(
        started.elapsed() >= crate::queue::MIN_GAP,
        "the second message did not wait for the gap, so the pacing is not real"
    );
    // Checked by content, not by count: this hub has its own registry handle and no bridge has
    // connected to it, so its first `say` also creates and greets a topic. Counting totals would be
    // counting that greeting.
    let sends = h.fake.sends.lock().await;
    for wanted in ["one", "two"] {
        assert!(
            sends[before..].iter().any(|(_, t, _)| t == wanted),
            "{wanted:?} never reached the operator"
        );
    }
    drop(sends);

    // The other half — a real ceiling still sheds — with a budget of one a minute rather than the
    // real eighteen. At the real rate the bucket refills faster than a paced sender drains it, so
    // draining it honestly takes about ninety seconds of wall clock to prove something `queue.rs`
    // already pins at its real values. What is worth proving HERE is that `send_into` reaches the
    // shed at all rather than pacing forever, and that the refusal says when to come back.
    let tight = Arc::new(
        Hub::new(
            Arc::clone(&h.fake),
            Registry::load(h.dir.path().join("projects.json")),
            AskLedger::load(h.dir.path().join("asks3.json")),
            HubAudit::new(h.dir.path().join("hub3.audit.log")),
            vec![ALLOWED_CHAT],
            ALLOWED_CHAT,
        )
        .with_budget(1, Duration::from_millis(5)),
    );
    let _ = tight.say(&h.project, "the one allowed", &[]).await;
    let mut shed = None;
    for _ in 0..4 {
        if let SendOutcome::TooFast(wait) = tight.say(&h.project, "over the ceiling", &[]).await {
            shed = Some(wait);
            break;
        }
    }
    let wait = shed.expect("a sender past the ceiling was never shed");
    assert!(
        wait > Duration::ZERO,
        "a refusal must say when to come back"
    );
}

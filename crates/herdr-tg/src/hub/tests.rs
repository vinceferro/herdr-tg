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
        note: &str,
    ) -> anyhow::Result<()> {
        self.retired
            .lock()
            .await
            .push((topic_id, msg_id.clone(), note.to_owned()));
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
        .with_settle(Duration::from_millis(500)),
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
        .await;
    assert!(h.hub.is_claimed(&h.project).await);

    let mut bridge = FakeBridge::connect(&h.sock, &h.secret, "i1", h.project.as_str()).await;
    let welcome = bridge.next().await.expect("an answer");
    assert!(
        matches!(welcome.payload, HubFrame::Welcome { .. }),
        "a live bridge was refused because a dead one held the claim: {:?}",
        welcome.payload
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

    until(async || h.fake.topics.lock().await.len() == 2).await;
    let sends = h.fake.sends.lock().await;
    let last = sends.last().expect("a send");
    assert_eq!(
        last.1, "after the topic went",
        "the message was lost in the rebinding"
    );
    assert_eq!(
        last.0, 1002,
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
    let mut shed = 0;
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
            if delivered == Delivered::No {
                shed += 1;
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
    assert!(
        shed > 0,
        "nothing was shed, so this proved nothing about backpressure"
    );
}

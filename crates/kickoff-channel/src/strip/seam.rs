//! The seam — where the strip's frames come from and where its dispatch frames go.
//!
//! The frame-owning half (the lane-relay that mints `wip` rows, the dispatchd that answers)
//! is being built elsewhere; nothing live emits these frames yet. Until it does, the seam is a
//! **replay**: an NDJSON file of byte-spec-shaped frames, handed down at the relay's own
//! cadence, with an outbox file that collects the dispatch frames the strip composes. The
//! board, the renderer and the picker know nothing about files — they know the two operations
//! this seam offers — so the live connection (dial the hub, subscribe, emit) replaces the
//! replay without touching any of them.
//!
//! The seam EXECUTES NOTHING. A dispatch frame appended to the outbox is a frame this strip
//! handed to its transport, exactly as a `say` handed to the socket would be; executing
//! belongs to the frame owner's dispatchd, behind their hub and their dispatch-capable token.
//! That is not a limitation of the fixture — it is the division the spec draws, and it is why
//! this module contains no way to start a process at all.

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, bail};

use super::frames::{DispatchFrame, Down, route_down};

/// The fixture seam: replay down-frames from a file, collect up-frames in another.
pub(crate) struct FixtureSeam {
    lines: VecDeque<String>,
    /// The frame parsed and waiting to be handed down. It lives HERE, in the seam, and not in
    /// the future's locals, because the strip's run loop polls `next_down` inside a
    /// `select!` — a future dropped mid-cadence destroys everything in its own stack, and a
    /// frame that only ever existed there is a frame the strip never sees. (The event-stream
    /// half of this repo holds the same law: partial state belongs to the long-lived thing,
    /// never to the future.)
    staged: Option<Down>,
    /// When the staged frame may be handed down — the cadence gate, re-derived from the clock
    /// on every poll so a cancelled and re-polled wait costs nothing and loses nothing.
    next_due: Option<std::time::Instant>,
    outbox: Option<PathBuf>,
    cadence: Duration,
    served_one: bool,
}

impl FixtureSeam {
    /// Open a replay. The file is read once, whole — a replay that changed under the strip
    /// would be a second source of truth, and the strip has exactly one.
    pub(crate) fn open(
        replay: &Path,
        outbox: Option<PathBuf>,
        cadence: Duration,
    ) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(replay)
            .with_context(|| format!("could not read the replay at {}", replay.display()))?;
        let lines = text
            .lines()
            .map(str::to_owned)
            .filter(|line| !line.trim().is_empty())
            .collect();
        Ok(Self {
            lines,
            staged: None,
            next_due: None,
            outbox,
            cadence,
            served_one: false,
        })
    }

    /// Hand down the next frame, or `None` when the replay is exhausted.
    ///
    /// **Cancel-safe by construction**: every piece of partial state (the staged frame, the
    /// cadence deadline) lives in the seam, and the only `.await` re-derives its remaining
    /// wait from the clock — so the run loop may drop this future at any point, poll it
    /// again later, and receive every frame exactly once, in order, none lost to the drop.
    ///
    /// Between frames the seam waits one cadence — the relay's own coalescing floor, so the
    /// strip's refresh rhythm is the rhythm the live seam will have. A line that names a known
    /// frame kind but will not decode is said loudly and skipped: silent continuation would
    /// hide a contract change behind a strip that looks alive. A frame kind this build has
    /// never heard of is skipped without a word — the skew law, same as seam ①.
    pub(crate) async fn next_down(&mut self) -> Option<Down> {
        loop {
            if self.staged.is_some() {
                if let Some(due) = self.next_due {
                    let remaining = due.saturating_duration_since(std::time::Instant::now());
                    if !remaining.is_zero() {
                        // `continue`, not a straight return: after the wait, the loop
                        // re-checks the staged frame against the (now-passed) deadline, so a
                        // cancellation during the wait leaves both exactly as they were.
                        tokio::time::sleep(remaining).await;
                        continue;
                    }
                }
                self.next_due = None;
                return self.staged.take();
            }

            // From here to `staged = Some(down)` there is no `.await`, so a line popped off
            // the replay is never in flight unprotected.
            let line = self.lines.pop_front()?;
            match route_down(&line) {
                Ok(Some(down)) => {
                    if self.served_one && !self.cadence.is_zero() {
                        self.next_due = Some(std::time::Instant::now() + self.cadence);
                    }
                    self.served_one = true;
                    self.staged = Some(down);
                }
                Ok(None) => continue,
                Err(err) => {
                    eprintln!("kickoff-channel: a replay line would not decode, continuing: {err}");
                }
            }
        }
    }

    /// Hand one dispatch frame to the seam: appended, as one JSON line, to the outbox.
    ///
    /// No outbox configured is a refusal rather than a drop — a dispatch frame this strip
    /// composed and threw away would leave an operator believing a lane was asked for when
    /// nothing was sent, which is the say/ask honesty law run backwards.
    pub(crate) fn emit(&mut self, frame: &DispatchFrame) -> anyhow::Result<()> {
        let Some(outbox) = &self.outbox else {
            bail!("dispatch is not wired on this seam (no outbox was named); nothing was sent");
        };
        let json = serde_json::to_string(frame)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(outbox)
            .with_context(|| format!("could not open the outbox at {}", outbox.display()))?;
        writeln!(file, "{json}")
            .with_context(|| format!("could not append to the outbox at {}", outbox.display()))
    }

    /// Whether this seam can take a dispatch at all — asked before a pick is offered, so the
    /// refusal comes before the questions rather than after them.
    pub(crate) fn can_dispatch(&self) -> bool {
        self.outbox.is_some()
    }
}

/// Mint a dispatch id: `disp-` plus a 26-character ULID (48-bit millisecond timestamp, 80 bits
/// of randomness, Crockford base32) — the spec's `disp-<ulid>` shape.
///
/// Uniqueness is the only property this needs: the receipt correlates on this id, and two
/// dispatches in one millisecond must still be two commands. Randomness comes from the same
/// crate the enrolment secret uses, for the same reason — hand-rolling entropy is how an id
/// becomes predictable in exactly the way nobody tests for.
pub(crate) fn mint_dispatch_id() -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0);

    let mut randomness = [0u8; 10];
    // A failed entropy draw still mints an id — all-zero tail, time-unique — because a strip
    // that stopped offering its one affordance over a transient entropy fault would be refusing
    // the honest thing for a pedantic one. The failure is visible in the id's tail.
    let _ = getrandom::fill(&mut randomness);

    let mut bits = millis as u128; // 48 bits of time…
    for byte in randomness {
        bits = (bits << 8) | byte as u128; // …then 80 bits of randomness: 128 bits, 26 x 5
    }
    // Most-significant first, zero-padded to 26 characters (the top two bits of the 128 are
    // always zero; a ULID is 48 + 80 = 128 bits, and 26 * 5 = 130, so it leads with `0`).
    let mut id = [b'0'; 26];
    let mut position = 26;
    while position > 0 {
        position -= 1;
        id[position] = ALPHABET[(bits & 31) as usize];
        bits >>= 5;
    }
    format!(
        "disp-{}",
        std::str::from_utf8(&id).expect("the alphabet is ASCII")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE cancellation test. The run loop polls `next_down` inside a `select!`, and a select
    /// drops every future whose arm did not win — so a frame popped off the replay and parked
    /// in the future's own locals while its cadence wait ran was a frame destroyed with the
    /// future, never handed down, never mentioned. This test does to the seam exactly what
    /// the loop does (abandon the wait mid-flight, come back later) and holds the property
    /// that made the bug visible: every frame arrives, exactly once, in order.
    ///
    /// Found live, not by inspection: a scripted receipt raced the piped keyboard, lost, and
    /// vanished — the strip ended by telling the operator his accepted dispatch had no
    /// receipt, which was false, and false in the worst direction.
    #[tokio::test]
    async fn a_frame_waited_on_through_a_cadence_is_not_lost_when_the_wait_is_abandoned() {
        let mut seam = FixtureSeam::open(
            Path::new("tests/fixtures/strip/dispatch-accepted.ndjson"),
            None,
            Duration::from_millis(30),
        )
        .expect("opens");

        let mut seen: Vec<String> = Vec::new();
        for _ in 0..80 {
            // Shorter than the cadence, so most polls abandon the wait mid-flight — the same
            // shape as a select! arm losing to the keyboard.
            match tokio::time::timeout(Duration::from_millis(5), seam.next_down()).await {
                Err(_abandoned) => continue,
                Ok(None) => break,
                Ok(Some(down)) => seen.push(match down {
                    Down::Wip(frame) => frame.id,
                    Down::Ack(receipt) => format!("ack:{}", receipt.ref_id),
                }),
            }
        }

        assert_eq!(
            seen,
            vec![
                "wip-70".to_owned(),
                "ack:disp-demo-1".to_owned(),
                "wip-71".to_owned(),
            ],
            "every frame of the replay must survive being waited on and abandoned"
        );
    }

    /// Two ids minted back to back differ — the receipt correlation depends on it.
    #[test]
    fn two_dispatch_ids_are_never_the_same_command() {
        let one = mint_dispatch_id();
        let two = mint_dispatch_id();
        assert!(
            one.starts_with("disp-") && one.len() == 5 + 26,
            "the shape is disp-<ulid>: {one}"
        );
        assert_ne!(one, two);
        assert!(
            one[5..].bytes().all(|b| b.is_ascii_alphanumeric()),
            "Crockford base32, no padding characters: {one}"
        );
    }

    /// The outbox refusal: a dispatch on a seam with no outbox is refused, not dropped.
    #[test]
    fn a_seam_with_no_outbox_refuses_a_dispatch_rather_than_dropping_it() {
        let mut seam =
            FixtureSeam::open(Path::new("/dev/null"), None, Duration::ZERO).expect("opens");
        assert!(!seam.can_dispatch());
        let frame = DispatchFrame {
            v: 1,
            id: "disp-test".to_owned(),
            t: "dispatch".to_owned(),
            action: "spawn".to_owned(),
            lane: None,
            agent: Some("builder".to_owned()),
            task_ref: Some("-x".to_owned()),
            deps: None,
            proof_cmd: None,
            reason: Some("test".to_owned()),
        };
        let err = seam.emit(&frame).expect_err("no outbox means no emit");
        assert!(
            err.to_string().contains("nothing was sent"),
            "the refusal says what did NOT happen:\n{err}"
        );
    }

    /// The outbox is append-only JSON lines: a second emit does not clobber the first, because
    /// every frame handed to the seam is a record of a command somebody composed.
    #[test]
    fn the_outbox_collects_every_frame_as_its_own_line() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let outbox = dir.path().join("outbox.ndjson");
        let mut seam =
            FixtureSeam::open(Path::new("/dev/null"), Some(outbox.clone()), Duration::ZERO)
                .expect("opens");
        assert!(seam.can_dispatch());

        for agent in ["builder", "reviewer"] {
            let frame = DispatchFrame {
                v: 1,
                id: format!("disp-{agent}"),
                t: "dispatch".to_owned(),
                action: "spawn".to_owned(),
                lane: None,
                agent: Some(agent.to_owned()),
                task_ref: Some("-x".to_owned()),
                deps: None,
                proof_cmd: None,
                reason: Some("test".to_owned()),
            };
            seam.emit(&frame).expect("appended");
        }

        let text = std::fs::read_to_string(&outbox).expect("the outbox is readable");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "one line per frame:\n{text}");
        assert!(
            lines[0].contains(r#""agent":"builder""#),
            "spec field names: {text}"
        );
        assert!(lines[1].contains(r#""agent":"reviewer""#));
        for line in lines {
            let value: serde_json::Value = serde_json::from_str(line).expect("each line is JSON");
            assert_eq!(value["t"], "dispatch");
            assert_eq!(value["v"], 1);
            assert_eq!(value["action"], "spawn");
            assert!(
                value.get("lane").is_none(),
                "spawn carries no lane — absent, not null"
            );
        }
    }
}

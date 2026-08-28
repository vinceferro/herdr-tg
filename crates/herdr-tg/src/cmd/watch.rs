//! `herdr-tg watch` — decode the event stream for one pane.

use std::time::{Duration, Instant};

use anyhow::bail;
use herdr_client::{AgentStatus, Event, HerdrClient, PaneId, Subscription};

use crate::render;

/// Subscribe to one pane's status events and print them as they decode.
///
/// # Why this is one pane and not the whole herd
///
/// There is **no global agent-status subscription** in protocol 20: exactly 3 of the 27
/// subscription variants take a `pane_id`, and `pane.agent_status_changed` is one of them. The
/// only globally-subscribable status-bearing event is `pane.updated`, which replays a stale
/// backlog on every connect — so slice 3 must fan out one subscription per agent pane, and slice 1
/// watches exactly one.
///
/// # Why `--once` is deterministic and read-only
///
/// A subscription pinned to a status herdr already holds for that pane **replays it at subscribe
/// time** (verified firing at t=0.00 for `idle` and for `working`; unfiltered and non-matching
/// both fire nothing). So `--once --expect-status <the pane's current status>` needs no transition,
/// no agent activity, and nothing typed into anyone's terminal. That is proof gate 5.
pub(crate) async fn run(
    client: &HerdrClient,
    pane: &str,
    once: bool,
    expect_status: Option<&str>,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    client.handshake().await?;

    let pane = PaneId::new(pane);
    let expected = expect_status.map(parse_status).transpose()?;
    let subscription = match expected.clone() {
        Some(status) => Subscription::agent_status(&pane, status),
        None => Subscription::agent_status_any(&pane),
    };

    let mut stream = client
        .subscribe(std::slice::from_ref(&subscription))
        .await?;

    if once {
        wait_for_one(
            &mut stream,
            expected.as_ref(),
            Duration::from_millis(timeout_ms),
        )
        .await
    } else {
        follow(&mut stream).await
    }
}

/// Print the first matching status event, then return.
///
/// A **decode failure is fatal here**, not skipped. Everywhere else on this stream an
/// undecodable frame is survivable — that is what keeps the bridge alive through a `herdr update`
/// — but `--once` exists to prove the two-envelope decoder works, and swallowing the one error it
/// is meant to catch would turn proof gate 5 into a timeout with no explanation.
async fn wait_for_one(
    stream: &mut herdr_client::EventStream,
    expected: Option<&AgentStatus>,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("{}", timed_out(expected, timeout));
        }

        // `EventStream::next` is cancel-safe (its partial-line state lives in the stream, not in
        // the future), so timing it out here cannot lose half a frame.
        match tokio::time::timeout(remaining, stream.next()).await {
            Err(_elapsed) => bail!("{}", timed_out(expected, timeout)),
            Ok(None) => bail!(
                "herdr closed the event stream before any matching event arrived; the client does \
                 not reconnect by itself, by design"
            ),
            Ok(Some(Err(err))) => return Err(err.into()),
            Ok(Some(Ok(event))) => {
                if matches(&event, expected) {
                    println!("{}", render::event_line(&event));
                    return Ok(());
                }
                tracing::debug!(event = %render::event_line(&event), "not the awaited event");
            }
        }
    }
}

/// Print every decoded event until the server closes the stream.
///
/// The manual smoke test; slice 3's reconnect loop replaces it. **This never reconnects.** `None`
/// means the server closed the stream and that is reported, not repaired — a client that silently
/// re-dialled would re-deliver the replayed roster backlog as a burst of phantom edges, and would
/// make PLAN.md's "a single recovery notice when the stream re-establishes" unimplementable.
async fn follow(stream: &mut herdr_client::EventStream) -> anyhow::Result<()> {
    loop {
        match stream.next().await {
            None => {
                eprintln!(
                    "herdr-tg: herdr closed the event stream (this client does not reconnect)"
                );
                return Ok(());
            }
            // An I/O failure is terminal; a frame this client cannot decode is not. Bucketing the
            // undecodable is the forward-compatibility contract that keeps a bridge alive through
            // a routine `herdr update`, so here it is a loud line and the stream continues.
            Some(Err(err)) if err.is_unreachable() => return Err(err.into()),
            Some(Err(err)) => eprintln!("herdr-tg: undecodable frame, stream continues: {err}"),
            Some(Ok(event)) => println!("{}", render::event_line(&event)),
        }
    }
}

/// Does this event satisfy what we were waiting for?
///
/// Only `pane.agent_status_changed` counts. A filtered subscription should not deliver anything
/// else, but the check is by variant rather than by trust: the roster family carries a *historical*
/// status and must never be mistaken for an edge.
fn matches(event: &Event, expected: Option<&AgentStatus>) -> bool {
    match event {
        Event::AgentStatus(changed) => expected.is_none_or(|want| &changed.agent_status == want),
        _ => false,
    }
}

fn timed_out(expected: Option<&AgentStatus>, timeout: Duration) -> String {
    match expected {
        Some(status) => format!(
            "no pane.agent_status_changed{{{}}} within {} ms (a filtered subscription replays only \
             when the pane ALREADY holds that status)",
            status.as_str(),
            timeout.as_millis()
        ),
        None => format!(
            "no pane.agent_status_changed within {} ms (an unfiltered subscription never replays; \
             it fires only on a real transition)",
            timeout.as_millis()
        ),
    }
}

/// Parse `--expect-status` strictly.
///
/// [`AgentStatus::from_wire`] never fails — an unknown value becomes `Unrecognized`, which is
/// right for the wire (herdr may add a status without a release) and wrong for a CLI flag: a typo
/// would build a subscription that can never match and report itself as a five-second timeout.
///
/// **This matches the five literals itself and does NOT call `AgentStatus::from_wire`.** That is
/// deliberate and load-bearing, not style. `from_wire`'s unrecognised branch fires a `Once`-gated
/// `WARN` that blames herdr ("herdr reported an agent status this client does not model") — for a
/// value herdr never sent — and, worse, *consumes* the crate's one-shot schema-drift alarm. In
/// slice 1 the process exits straight after, so the cost is one misleading line; in slice 2/3, a
/// long-lived bridge where a status string can arrive from a Telegram message or a config file,
/// one bad operator input would permanently silence the alarm that exists to announce a real
/// herdr status this client does not model. Operator input never touches the wire decoder.
fn parse_status(raw: &str) -> anyhow::Result<AgentStatus> {
    match raw {
        "idle" => Ok(AgentStatus::Idle),
        "working" => Ok(AgentStatus::Working),
        "blocked" => Ok(AgentStatus::Blocked),
        "done" => Ok(AgentStatus::Done),
        "unknown" => Ok(AgentStatus::Unknown),
        _ => bail!(
            "unknown agent status {raw:?}; herdr protocol 20 has: idle, working, blocked, done, \
             unknown"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use herdr_client::proto::event::decode_event;

    #[test]
    fn expect_status_rejects_a_typo_instead_of_timing_out_on_it() {
        assert_eq!(parse_status("blocked").unwrap(), AgentStatus::Blocked);
        assert_eq!(parse_status("done").unwrap(), AgentStatus::Done);
        let err = parse_status("blocke").expect_err("a typo must be a usage error, not a filter");
        assert!(err.to_string().contains("blocke"), "{err}");
    }

    /// `parse_status` hand-matches the five literals rather than delegating to
    /// `AgentStatus::from_wire`, so the two can drift. Pin them together: every value the CLI
    /// accepts must decode to the same variant the wire decoder would produce, and every value the
    /// CLI rejects must be exactly the set `from_wire` would have bucketed as `Unrecognized`.
    #[test]
    fn the_cli_parser_and_the_wire_decoder_agree_on_every_modelled_status() {
        for raw in ["idle", "working", "blocked", "done", "unknown"] {
            let parsed = parse_status(raw).expect("a modelled status is accepted");
            assert_eq!(
                parsed,
                AgentStatus::from_wire(raw),
                "parse_status and from_wire disagree on {raw:?}"
            );
            assert_eq!(
                parsed.as_str(),
                raw,
                "and it round-trips back to the wire string"
            );
        }
        // The rejected set. `from_wire` would bucket each of these as `Unrecognized` AND fire the
        // Once-gated drift warning; `parse_status` must reject them without ever calling it.
        for raw in ["", "Idle", "idle ", "nope", "pane.agent_status_changed"] {
            assert!(
                parse_status(raw).is_err(),
                "{raw:?} is not one of the five and must be a usage error"
            );
        }
    }

    /// The roster family carries a historical `agent_status` on the wire. `--once` must never
    /// treat one as the edge it was waiting for.
    #[test]
    fn only_the_status_family_satisfies_the_wait() {
        let frames = include_str!("../../../herdr-client/tests/fixtures/events-mixed.ndjson");
        let mut saw_status = false;
        let mut saw_roster = false;

        for line in frames.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(event) = decode_event(line) else {
                continue;
            };
            match &event {
                Event::AgentStatus(changed) => {
                    saw_status = true;
                    let want = changed.agent_status.clone();
                    assert!(matches(&event, Some(&want)));
                    assert!(matches(&event, None));
                }
                Event::Roster(_) => {
                    saw_roster = true;
                    assert!(
                        !matches(&event, None),
                        "a roster frame is not a status edge"
                    );
                    assert!(!matches(&event, Some(&AgentStatus::Blocked)));
                }
                _ => {}
            }
        }
        assert!(saw_status, "the fixture carries the dot-form status frame");
        assert!(
            saw_roster,
            "the fixture carries the snake_case roster backlog"
        );
    }
}

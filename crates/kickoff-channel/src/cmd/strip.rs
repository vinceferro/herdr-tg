//! `kickoff-channel strip` — the lane board another organisation frames, rendered here.
//!
//! Reads `wip` frames from a replay file (the fixture seam: the live lane-relay connection
//! lands behind the same interface) and renders one line per lane — agent, lane, state, last
//! beat, proof status — refreshing at the relay's own cadence. While it runs, `d` offers the
//! one dispatch affordance and `q` leaves. `--once` renders the first board and exits, which
//! is the pipeable, testable mode.
//!
//! This command executes nothing. A dispatch composed here is a frame appended to the outbox —
//! a command handed to the transport, for the frame owner's executor to take or refuse, with
//! the receipt rendered either way.

use std::time::Duration;

use crate::strip;

pub(crate) async fn run(
    replay: std::path::PathBuf,
    outbox: Option<std::path::PathBuf>,
    once: bool,
    cadence_ms: u64,
    stale_after_ms: Option<u64>,
    dispatch_id: Option<String>,
) -> anyhow::Result<()> {
    let cadence = Duration::from_millis(cadence_ms);
    // Three cadences of silence before the strip calls itself stale, and never less than a
    // second: the relay coalesces (its floor is the cadence itself), so one missed cadence is
    // a coalescing, two is a question, three is a strip saying what it does and does not know.
    let stale_after = Duration::from_millis(
        stale_after_ms.unwrap_or_else(|| cadence_ms.saturating_mul(3).max(1000)),
    );

    strip::run(strip::Options {
        replay,
        outbox,
        once,
        cadence,
        stale_after,
        dispatch_id,
    })
    .await
}

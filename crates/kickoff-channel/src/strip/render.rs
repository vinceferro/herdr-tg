//! The strip a person reads — one line per lane, honest by construction.
//!
//! Deliberately not a general table library, and a sibling of `crate::render`'s herd table: a
//! handful of lanes on a terminal, in the frame owner's own order, with columns measured from
//! the data so a long lane name cannot push the state column out of alignment — state and proof
//! are the columns an operator is actually scanning.
//!
//! The honesty rules this renderer holds, each because a plausible wrong rendering exists:
//!
//! * **`unverified` is a claim of nothing and never looks like success.** A lane with no proof
//!   declared says `none declared`, a world away from the `passed` a proven lane earned.
//! * **Stale is said, never implied.** A strip that has not heard from its seam within its
//!   freshness window says so above the rows, because rows that look live while the feed is
//!   dead are the ghost-row sin one level up.
//! * **Unknown statuses and unknown triage sources render as themselves.** A status the frame
//!   owner added without a release must not be mapped onto the nearest status this build
//!   knows — that would be lying about whose state it is.

use std::fmt::Write as _;

use super::board::Board;
use super::frames::{WipLane, spell_age};

/// What the renderer knows about the world beyond the board: when "now" is, how long since the
/// last frame, whether the feed has said it is finished, and how long silence may last before
/// the strip must call itself stale.
pub(crate) struct Outlook<'a> {
    pub now_secs: i64,
    /// Seconds since the board's frame arrived, when a frame ever has.
    pub secs_since_frame: Option<i64>,
    /// The feed has ended (a replay ran out; a live seam would be a stalled connection).
    pub feed_ended: bool,
    /// Silence beyond this is staleness, and staleness is rendered, not hidden.
    pub stale_after_secs: u64,
    pub _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Outlook<'a> {
    fn is_stale(&self) -> bool {
        match self.secs_since_frame {
            Some(elapsed) if elapsed >= self.stale_after_secs as i64 => true,
            // No frame ever arrived: nothing on the board, so nothing can masquerade as live.
            _ => false,
        }
    }
}

/// Render the whole strip: banner lines (stale / replay-ended), the lane rows, the truncation
/// note, and the proposed triage line when the frame carried one.
pub(crate) fn strip(board: &Board, outlook: &Outlook) -> String {
    let mut out = String::new();

    if board.frame_id().is_none() {
        // Not an error, and not a board: nothing has arrived yet, and saying so beats printing
        // a header with nothing under it that an operator has to interpret.
        out.push_str("waiting for the first frame\n");
        return out;
    }

    if outlook.feed_ended {
        let _ = writeln!(
            out,
            "— the replay has ended; these are the last known states, not live —"
        );
    } else if outlook.is_stale() {
        let _ = writeln!(
            out,
            "— no frame for {}; these are the last known states, not live —",
            spell_age(outlook.secs_since_frame.unwrap_or(0)),
        );
    }

    let lanes = board.lanes();
    let _ = write!(
        out,
        "strip: {} lane{}",
        lanes.len(),
        if lanes.len() == 1 { "" } else { "s" },
    );
    if let Some(frame_id) = board.frame_id() {
        let _ = write!(out, " · frame {frame_id}");
    }
    if let Some(secs) = outlook.secs_since_frame {
        let _ = write!(out, " · {} ago", spell_age(secs));
    }
    let (total, shown) = board.totals();
    if let (Some(total), Some(shown)) = (total, shown) {
        if shown < total {
            let _ = write!(out, " · showing {shown} of {total}");
        }
    }
    out.push('\n');

    if lanes.is_empty() {
        // An empty board is a real, reportable state: the graph has no lanes right now.
        out.push_str("  (no lanes)\n");
    } else {
        let rows: Vec<Row<'_>> = lanes
            .iter()
            .map(|lane| row_for(lane, outlook.now_secs))
            .collect();
        let w_agent = width(rows.iter().map(|r| r.agent));
        let w_lane = width(rows.iter().map(|r| r.lane));
        let w_state = width(rows.iter().map(|r| r.state));
        let w_beat = width(rows.iter().map(|r| r.beat.as_str()));

        for row in &rows {
            let _ = writeln!(
                out,
                "  {:<w_agent$}  {:<w_lane$}  {:<w_state$}  {:>w_beat$}  {}",
                row.agent, row.lane, row.state, row.beat, row.proof,
            );
        }
    }

    if let Some(triage) = board.triage() {
        // PROPOSED, PENDING CONTRACT — rendered only when the frame carried the field, so a
        // frame-owning half that never adopts it changes nothing here. The two sources the
        // proposal names are spelled as sentences; any other value renders as itself, because
        // mapping an unfamiliar source onto a familiar one would misreport whose answer it was.
        let source = match triage.source.as_str() {
            "state" => "answered from state",
            "coordinator" => "woke the coordinator",
            other => other,
        };
        match triage.confidence {
            Some(confidence) => {
                let _ = writeln!(
                    out,
                    "triage: {source} · confidence {:.0}%",
                    (confidence * 100.0).clamp(0.0, 100.0),
                );
            }
            None => {
                let _ = writeln!(out, "triage: {source}");
            }
        }
    }

    out
}

/// One lane, one line, in the columns the spec named: agent, lane, state, last beat, proof.
struct Row<'a> {
    agent: &'a str,
    lane: &'a str,
    state: &'a str,
    beat: String,
    proof: &'static str,
}

fn row_for(lane: &WipLane, now_secs: i64) -> Row<'_> {
    Row {
        agent: lane.agent.as_deref().unwrap_or("—"),
        // "agent + short id" is what §3.2 says a consumer renders; the full id is the machine's
        // business, and a lane with no short name stands for itself.
        lane: lane.short.as_deref().unwrap_or(lane.id.as_str()),
        state: lane.status.as_str(),
        beat: last_beat(lane, now_secs),
        proof: proof_status(lane),
    }
}

/// The lane's last beat, aged honestly: recomputed from the frame's own `updated` stamp so it
/// keeps growing between frames, falling back to the frame's `age_min` — said as an
/// approximation, because that number does not age — and to a dash when the frame said neither.
fn last_beat(lane: &WipLane, now_secs: i64) -> String {
    if let Some(updated) = lane
        .updated
        .as_deref()
        .and_then(super::frames::parse_rfc3339_utc)
    {
        return spell_age(now_secs - updated);
    }
    if let Some(age_min) = lane.age_min {
        return format!("≈{}m", age_min);
    }
    "—".to_owned()
}

/// The proof column. This is where "unverified is a claim of nothing" lives: the derivation is
/// from BOTH the status and the declared-proof boolean, because neither alone tells the truth —
/// `done` without a recorded proof is a contradiction this renders as `none recorded` rather
/// than borrowing the `passed` it did not earn.
fn proof_status(lane: &WipLane) -> &'static str {
    let declared = lane.proof == Some(true);
    match lane.status.as_str() {
        "done" if declared => "passed",
        "done" => "none recorded",
        "proof-failed" => "FAILED",
        "unverified" => "none declared",
        "running" | "claimed" => "not yet run",
        "failed" | "blocked" | "pending" => "not reached",
        // An unknown status: the state column shows it verbatim; the proof column claims
        // nothing, rather than guessing what an unfamiliar state implies about proof.
        _ => "—",
    }
}

/// The widest value a column will hold, so every row's columns line up (crate::render's
/// measured-width rule: a long value cannot push the scanning column out of alignment).
/// Counted in characters, not bytes — a lane name with a wide character in it must not
/// misalign the state column beside it.
fn width<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    values.map(|value| value.chars().count()).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::super::board::Board;
    use super::super::frames::{Down, route_down};
    use super::*;

    fn board_from(frame_json: &str) -> Board {
        let Some(Down::Wip(frame)) = route_down(frame_json).expect("fixture parses") else {
            panic!("not a wip frame");
        };
        let mut board = Board::default();
        board.apply(&frame);
        board
    }

    fn outlook() -> Outlook<'static> {
        Outlook {
            now_secs: 1_000,
            secs_since_frame: Some(2),
            feed_ended: false,
            stale_after_secs: 60,
            _marker: std::marker::PhantomData,
        }
    }

    fn lane_json(id: &str, status: &str, proof: bool) -> String {
        format!(
            r#"{{"id":"{id}","short":"s-{id}","agent":"builder","status":"{status}","proof":{proof},"updated":"2026-10-04T13:00:00Z","age_min":5}}"#
        )
    }

    fn frame_json(lanes: &[String]) -> String {
        format!(
            r#"{{"v":1,"id":"wip-1","t":"wip","full":true,"lanes":[{}],"total":{},"shown":{},"truncated":0,"seq":1}}"#,
            lanes.join(","),
            lanes.len(),
            lanes.len()
        )
    }

    /// `unverified` is a claim of nothing and must never look like success — nor like the
    /// `not yet run` a running lane honestly says, nor like the `FAILED` a failed proof earned.
    #[test]
    fn unverified_is_a_claim_of_nothing_and_renders_as_nothing() {
        let board = board_from(&frame_json(&[
            lane_json("done-proven", "done", true),
            lane_json("unverified", "unverified", false),
            lane_json("running", "running", true),
            lane_json("failed-proof", "proof-failed", true),
        ]));
        let text = strip(&board, &outlook());

        let unverified_line = text
            .lines()
            .find(|l| l.contains("s-unverified"))
            .expect("the unverified lane has a row");
        assert!(
            unverified_line.contains("none declared"),
            "an unverified lane must say no proof was declared:\n{unverified_line}"
        );
        assert!(
            !unverified_line.contains("passed") && !unverified_line.contains("FAILED"),
            "an unverified lane borrowed a word that claims something:\n{unverified_line}"
        );

        let done_line = text.lines().find(|l| l.contains("s-done-proven")).unwrap();
        assert!(
            done_line.contains("passed"),
            "earned proof says passed:\n{done_line}"
        );

        let running_line = text.lines().find(|l| l.contains("s-running")).unwrap();
        assert!(
            running_line.contains("not yet run"),
            "a running lane's proof has not run yet, and says so:\n{running_line}"
        );

        let failed_line = text.lines().find(|l| l.contains("s-failed-proof")).unwrap();
        assert!(
            failed_line.contains("FAILED"),
            "a failed proof says FAILED:\n{failed_line}"
        );
    }

    /// `done` without a recorded proof is a contradiction the lane-runner's own law says cannot
    /// happen; if a frame carries it anyway, the strip does not lend it the word `passed`.
    #[test]
    fn a_done_lane_with_no_recorded_proof_does_not_borrow_the_word_passed() {
        let board = board_from(&frame_json(&[lane_json("odd", "done", false)]));
        let text = strip(&board, &outlook());
        assert!(
            text.contains("none recorded"),
            "done-without-proof is said as none recorded, never as passed:\n{text}"
        );
    }

    /// An unknown status renders as itself and claims nothing about proof — the forward-safe
    /// law that keeps a strip alive through the frame owner's releases.
    #[test]
    fn a_status_this_build_does_not_know_renders_as_itself() {
        let board = board_from(&frame_json(&[lane_json(
            "future",
            "pause-for-review",
            false,
        )]));
        let text = strip(&board, &outlook());
        let line = text
            .lines()
            .find(|l| l.contains("s-future"))
            .expect("the lane has a row");
        assert!(
            line.contains("pause-for-review"),
            "the unknown status renders verbatim:\n{line}"
        );
        assert!(
            line.ends_with("—"),
            "the proof column claims nothing for an unknown status:\n{line}"
        );
    }

    /// Stale is said, never implied: past the freshness window, the banner names the silence
    /// and the rows below it are explicitly the last known.
    #[test]
    fn a_silent_strip_says_so_above_the_rows() {
        let board = board_from(&frame_json(&[lane_json("a", "running", true)]));

        let fresh = strip(
            &board,
            &Outlook {
                secs_since_frame: Some(5),
                ..outlook()
            },
        );
        assert!(
            !fresh.contains("not live"),
            "fresh frames carry no banner:\n{fresh}"
        );

        let stale = strip(
            &board,
            &Outlook {
                secs_since_frame: Some(300),
                ..outlook()
            },
        );
        assert!(
            stale.contains("no frame for 5m") && stale.contains("not live"),
            "a stale strip names its silence:\n{stale}"
        );

        let ended = strip(
            &board,
            &Outlook {
                feed_ended: true,
                ..outlook()
            },
        );
        assert!(
            ended.contains("replay has ended") && ended.contains("not live"),
            "an ended replay says the rows are the last known:\n{ended}"
        );
    }

    /// Truncation is the frame's own honesty (`shown` < `total`) and the strip passes it on:
    /// "showing 3 of 7" tells an operator there are lanes it cannot see.
    #[test]
    fn a_truncated_board_says_how_much_it_is_not_showing() {
        let board = board_from(&format!(
            r#"{{"v":1,"id":"wip-1","t":"wip","full":true,"lanes":[{}],"total":7,"shown":3,"truncated":4,"seq":1}}"#,
            lane_json("a", "running", true)
        ));
        let text = strip(&board, &outlook());
        assert!(
            text.contains("showing 3 of 7"),
            "truncation is carried to the operator:\n{text}"
        );
    }

    /// The proposed triage line: present when present, absent when absent, unknown sources as
    /// themselves.
    #[test]
    fn the_triage_line_renders_only_when_the_frame_carries_it() {
        let plain = board_from(&frame_json(&[lane_json("a", "running", true)]));
        assert!(
            !strip(&plain, &outlook()).contains("triage:"),
            "no triage on the wire, no triage on the strip"
        );

        let from_state = board_from(&format!(
            r#"{{"v":1,"id":"wip-1","t":"wip","full":true,"lanes":[{}],"seq":1,"triage":{{"source":"state","confidence":0.87}}}}"#,
            lane_json("a", "running", true)
        ));
        let text = strip(&from_state, &outlook());
        assert!(
            text.contains("triage: answered from state · confidence 87%"),
            "the proposal's two sources are sentences:\n{text}"
        );

        let woke = board_from(&format!(
            r#"{{"v":1,"id":"wip-1","t":"wip","full":true,"lanes":[{}],"seq":1,"triage":{{"source":"coordinator","confidence":0.12}}}}"#,
            lane_json("a", "running", true)
        ));
        assert!(strip(&woke, &outlook()).contains("woke the coordinator · confidence 12%"));

        let unfamiliar = board_from(&format!(
            r#"{{"v":1,"id":"wip-1","t":"wip","full":true,"lanes":[{}],"seq":1,"triage":{{"source":"psychic","confidence":0.5}}}}"#,
            lane_json("a", "running", true)
        ));
        assert!(
            strip(&unfamiliar, &outlook()).contains("triage: psychic · confidence 50%"),
            "an unknown source renders as itself, not as a known one it resembles"
        );
    }

    /// No board yet is waiting, and a board with no lanes is an empty herd — two different
    /// truths, both said plainly.
    #[test]
    fn waiting_and_empty_are_two_different_honest_lines() {
        assert_eq!(
            strip(&Board::default(), &outlook()),
            "waiting for the first frame\n"
        );

        let empty = board_from(
            r#"{"v":1,"id":"wip-1","t":"wip","full":true,"lanes":[],"total":0,"shown":0,"seq":1}"#,
        );
        let text = strip(&empty, &outlook());
        assert!(
            text.contains("(no lanes)"),
            "an empty graph is a real state, reported:\n{text}"
        );
    }

    /// Columns line up whatever the data holds — the measured-width rule, held by test because
    /// a misaligned state column is unreadable exactly when it matters most: the proof cell
    /// starts at the same column on every row, long lane name or short.
    #[test]
    fn columns_line_up_under_a_long_lane_name() {
        let board = board_from(&frame_json(&[
            lane_json("a-very-long-lane-name-indeed", "running", true),
            lane_json("b", "proof-failed", true),
        ]));
        let text = strip(&board, &outlook());
        let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("  ")).collect();
        assert_eq!(rows.len(), 2, "two lanes, two rows:\n{text}");

        let proof_starts: Vec<usize> = rows
            .iter()
            .map(|row| {
                [
                    "not yet run",
                    "FAILED",
                    "passed",
                    "none declared",
                    "not reached",
                    "none recorded",
                    "—",
                ]
                .iter()
                .find_map(|cell| row.rfind(cell))
                .unwrap_or_else(|| panic!("no proof cell on row: {row}"))
            })
            .collect();
        assert_eq!(
            proof_starts[0], proof_starts[1],
            "the proof column starts at the same column on every row:\n{text}"
        );
    }
}

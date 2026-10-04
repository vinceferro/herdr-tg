//! The strip's board — what the strip currently believes, and what it refuses to believe.
//!
//! A `wip` frame is a **snapshot with a sequence number** (the spec chose snapshot-over-delta
//! precisely to kill missed-delta bugs), so the board's whole job is replacement: a new full
//! snapshot is the new truth, and a lane absent from it is gone — moved to another repo's
//! graph, or stopped. Keeping a lane the newest snapshot did not name is the one sin this
//! board exists not to commit: a row that looks live while its lane is dead or elsewhere is a
//! ghost, and an operator dispatches against ghosts.

use super::frames::{TriageProvenance, WipFrame, WipLane};

/// What the strip currently holds: the last snapshot that said `full: true`, whole.
#[derive(Debug, Default)]
pub(crate) struct Board {
    lanes: Vec<WipLane>,
    frame_id: Option<String>,
    seq: Option<u64>,
    total: Option<u64>,
    shown: Option<u64>,
    truncated: Option<u64>,
    triage: Option<TriageProvenance>,
}

/// What applying a frame did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Applied {
    /// The board now holds this snapshot, whole.
    Snapshot,
    /// The board is unchanged, and the reason is for saying out loud — never for swallowing.
    Refused(&'static str),
}

impl Board {
    /// Apply one frame. A snapshot replaces the board; anything else is refused with a reason
    /// the caller prints, because a strip that silently merged what it could not reconcile
    /// would be the missed-delta bug the snapshot design exists to prevent.
    pub(crate) fn apply(&mut self, frame: &WipFrame) -> Applied {
        match frame.full {
            Some(true) => {
                self.lanes = frame.lanes.clone();
                self.frame_id = Some(frame.id.clone());
                self.seq = frame.seq;
                self.total = frame.total;
                self.shown = frame.shown;
                self.truncated = frame.truncated;
                self.triage = frame.triage.clone();
                Applied::Snapshot
            }
            Some(false) => {
                Applied::Refused("a delta, not a snapshot — this strip renders snapshots")
            }
            None => Applied::Refused(
                "not marked as a snapshot — this strip will not guess what the frame was",
            ),
        }
    }

    /// The lanes of the last snapshot, in the order the frame owner sorted them (running-first,
    /// their renderer's order — the operator's own board shows the same order, and a strip that
    /// re-sorted would disagree with the board beside it about which lane is on top).
    pub(crate) fn lanes(&self) -> &[WipLane] {
        &self.lanes
    }

    pub(crate) fn frame_id(&self) -> Option<&str> {
        self.frame_id.as_deref()
    }

    pub(crate) fn triage(&self) -> Option<&TriageProvenance> {
        self.triage.as_ref()
    }

    /// How many lanes the whole board has, including any the frame did not show. `None` when
    /// the frame did not say — the renderer says "N lanes", not a guess at more.
    pub(crate) fn totals(&self) -> (Option<u64>, Option<u64>) {
        (self.total, self.shown)
    }

    /// The distinct agents on the board, first-seen order — the dispatch affordance's offered
    /// list. What a person picks from is what the strip can see, never a list minted here.
    pub(crate) fn agents(&self) -> Vec<String> {
        let mut agents: Vec<String> = Vec::new();
        for lane in &self.lanes {
            if let Some(agent) = &lane.agent {
                if !agents.iter().any(|seen| seen == agent) {
                    agents.push(agent.clone());
                }
            }
        }
        agents
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(id: &str, lanes: &[(&str, &str, &str)]) -> WipFrame {
        WipFrame {
            v: 1,
            id: id.to_owned(),
            t: "wip".to_owned(),
            lane: None,
            full: Some(true),
            lanes: lanes
                .iter()
                .map(|&(lane_id, agent, status)| WipLane {
                    id: lane_id.to_owned(),
                    short: None,
                    agent: Some(agent.to_owned()),
                    status: status.to_owned(),
                    respawns: None,
                    proof: None,
                    updated: None,
                    age_min: None,
                })
                .collect(),
            total: Some(lanes.len() as u64),
            shown: Some(lanes.len() as u64),
            truncated: Some(0),
            seq: None,
            triage: None,
        }
    }

    /// THE ghost-row test. A lane that left the snapshot — moved to another repo's graph, or
    /// stopped — must leave the strip with the snapshot that stopped naming it. A strip that
    /// keeps rendering it is showing stale state as live, which is the one dishonesty this
    /// surface must not commit.
    #[test]
    fn a_lane_that_left_the_snapshot_must_not_stay_on_the_board() {
        let mut board = Board::default();
        board.apply(&frame(
            "wip-41",
            &[
                ("lane-a", "builder", "running"),
                ("lane-b", "builder", "running"),
            ],
        ));
        board.apply(&frame("wip-42", &[("lane-b", "builder", "done")]));

        let on_the_board: Vec<&str> = board.lanes().iter().map(|l| l.id.as_str()).collect();
        assert_eq!(
            on_the_board,
            vec!["lane-b"],
            "lane-a left the wip-42 snapshot; a board still holding it renders a dead lane as live"
        );
    }

    /// A lane that STAYED must move when its state moved — replacement, not union.
    #[test]
    fn a_lane_that_stayed_shows_the_state_the_newest_snapshot_names() {
        let mut board = Board::default();
        board.apply(&frame("wip-41", &[("lane-b", "builder", "running")]));
        board.apply(&frame("wip-42", &[("lane-b", "builder", "done")]));
        assert_eq!(board.lanes()[0].status, "done");
        assert_eq!(board.frame_id(), Some("wip-42"));
    }

    /// A frame that is not a whole snapshot is refused with a reason, and refuses change
    /// nothing: the board keeps holding what it could prove.
    #[test]
    fn a_frame_that_is_not_a_whole_snapshot_is_refused_and_changes_nothing() {
        let mut board = Board::default();
        board.apply(&frame("wip-41", &[("lane-a", "builder", "running")]));

        let mut delta = frame("wip-50", &[("lane-z", "builder", "running")]);
        delta.full = Some(false);
        assert_eq!(
            board.apply(&delta),
            Applied::Refused("a delta, not a snapshot — this strip renders snapshots")
        );

        let mut unmarked = frame("wip-51", &[("lane-z", "builder", "running")]);
        unmarked.full = None;
        assert_eq!(
            board.apply(&unmarked),
            Applied::Refused(
                "not marked as a snapshot — this strip will not guess what the frame was"
            )
        );

        let on_the_board: Vec<&str> = board.lanes().iter().map(|l| l.id.as_str()).collect();
        assert_eq!(on_the_board, vec!["lane-a"]);
    }

    /// The picker's offered agents are the board's own, distinct, in first-seen order.
    #[test]
    fn the_offered_agents_are_the_ones_the_strip_can_see() {
        let mut board = Board::default();
        board.apply(&frame(
            "wip-41",
            &[
                ("lane-a", "builder", "running"),
                ("lane-b", "builder", "claimed"),
                ("lane-c", "reviewer", "pending"),
            ],
        ));
        assert_eq!(board.agents(), vec!["builder", "reviewer"]);
    }
}

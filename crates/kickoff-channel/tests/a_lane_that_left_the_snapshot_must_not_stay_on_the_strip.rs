//! The ghost-row sin, held at the surface a person actually reads.
//!
//! The unit test in `strip::board` pins the property on the board; this file pins it on the
//! BINARY — the strip a person runs, fed a replay whose lane leaves the snapshot partway
//! through. A strip that keeps rendering the departed lane is showing stale state as live, and
//! the operator dispatches against ghosts.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/strip")
        .join(name)
}

fn run_strip(args: &[&str]) -> (String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_kickoff-channel"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("could not run the strip: {error}"));
    assert!(
        output.status.success(),
        "the strip failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// One board per section, where a section is the lines after each `frame wip-N` header.
fn boards(stdout: &str) -> Vec<Vec<&str>> {
    let mut sections: Vec<Vec<&str>> = Vec::new();
    for line in stdout.lines() {
        if line.contains("· frame wip-") {
            sections.push(Vec::new());
        }
        if let Some(current) = sections.last_mut() {
            current.push(line);
        }
    }
    sections
}

/// The lane that left the wip-42 snapshot is gone from every board after it, and the lanes
/// that stayed show the states the newer snapshots name — a stopped lane's honesty is that it
/// stops, not that it fades.
#[test]
fn a_lane_that_left_the_snapshot_must_not_stay_on_the_strip() {
    let (stdout, _stderr) = run_strip(&[
        "strip",
        "--replay",
        fixture("walk.ndjson").to_str().unwrap(),
        "--cadence-ms",
        "0",
    ]);

    let sections = boards(&stdout);
    assert!(
        sections.len() >= 3,
        "three frames should render three boards:\n{stdout}"
    );

    let first = sections[0].join("\n");
    assert!(
        first.contains("strip-mvp") && first.contains("running"),
        "the first board shows the lane that is about to leave:\n{first}"
    );

    for later in &sections[1..] {
        let board = later.join("\n");
        assert!(
            !board.contains("strip-mvp"),
            "a board after wip-42 still names the lane that left the snapshot — a ghost row:\n{board}"
        );
    }

    let second = sections[1].join("\n");
    assert!(
        second.contains("done") && second.contains("passed"),
        "the lane that stayed shows done and its earned proof:\n{second}"
    );

    let last = sections.last().unwrap().join("\n");
    assert!(
        last.contains("proof-failed") && last.contains("FAILED"),
        "the final board shows the failed proof as failed:\n{last}"
    );
    assert!(
        last.contains("unverified") && last.contains("none declared"),
        "the unverified lane claims nothing:\n{last}"
    );
}

/// A strip whose feed has finished says the rows are the last known, not live — the honesty a
/// finished replay owes, which is the same honesty a stalled live seam will owe.
#[test]
fn a_strip_whose_replay_ends_says_the_rows_are_the_last_known_not_live() {
    let (stdout, _stderr) = run_strip(&[
        "strip",
        "--replay",
        fixture("silent.ndjson").to_str().unwrap(),
        "--cadence-ms",
        "0",
    ]);
    assert!(
        stdout.contains("the replay has ended") && stdout.contains("not live"),
        "an ended replay is announced above rows that are explicitly not live:\n{stdout}"
    );
    assert!(
        stdout.contains("silent-lane") && stdout.contains("running"),
        "the last known board is still shown:\n{stdout}"
    );
}

/// A gap between frames longer than the staleness window draws the stale banner while the
/// feed is still open — rows that look live while nothing arrives are the ghost-row sin one
/// level up, and the banner is what separates "last known" from "live".
#[test]
fn a_silent_gap_between_frames_draws_the_stale_banner_not_live_rows() {
    let (stdout, _stderr) = run_strip(&[
        "strip",
        "--replay",
        fixture("walk.ndjson").to_str().unwrap(),
        "--cadence-ms",
        "2000",
        "--stale-after-ms",
        "150",
    ]);
    assert!(
        stdout.contains("no frame for") && stdout.contains("not live"),
        "a gap longer than the staleness window must be drawn as staleness:\n{stdout}"
    );
}

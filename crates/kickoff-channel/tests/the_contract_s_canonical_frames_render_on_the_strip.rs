//! The frame-owning contract is in-tree now (`hub/FLEET-WIP-FRAMES.md` v1 + slice 1.5), and
//! its repo ships canonical fixture frames captured from the live relay — normalized real
//! bytes, volatile fields pinned. Those bytes are copied verbatim into this repo's fixtures
//! (`contract-plan.ndjson`, `contract-bare.ndjson`), and this test is the end-to-end proof
//! that the strip renders what that system actually puts on the bus: not a synthetic frame
//! this repo invented, but theirs.
//!
//! The slice 1.5 fields (`plan`, `progress_snapshot`) are decoded and carried, not rendered —
//! the render choice is deliberately still open — so the assertion here is the plain-row law:
//! a row carrying either field renders exactly like a row carrying neither (their rule 14: a
//! consumer renders a plain row for the negative control, and a renderer that prints zeros or
//! a spinner for it is lying about evidence that does not exist).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/strip")
        .join(name)
}

fn strip_once(fixture_name: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_kickoff-channel"))
        .arg("strip")
        .arg("--replay")
        .arg(fixture(fixture_name))
        .arg("--once")
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("could not run the strip: {error}"));
    assert_eq!(
        output.status.code(),
        Some(0),
        "the strip leaves cleanly on the contract's own bytes:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The plan board: three rows render — the planful one, the bare negative control, and the
/// snapshot-without-plan one — each as a plain row, none privileged by fields the renderer
/// has not decided how to show.
#[test]
fn the_contract_s_canonical_plan_board_renders_every_row() {
    let stdout = strip_once("contract-plan.ndjson");
    for short in ["1004-plan", "1004-bare", "1004-badp"] {
        assert!(
            stdout.contains(short),
            "lane {short} has its row on the strip:\n{stdout}"
        );
    }
    assert!(
        stdout.contains("running"),
        "the lanes' state renders:\n{stdout}"
    );
    assert!(
        stdout.contains("frame wip-41"),
        "the board names the frame it is showing:\n{stdout}"
    );
}

/// The bare-lane fixture: one row, nothing derivable, no fields — and the strip neither
/// crashes on the absence nor invents a line to soothe it.
#[test]
fn the_contract_s_bare_lane_fixture_renders_as_a_plain_row() {
    let stdout = strip_once("contract-bare.ndjson");
    assert!(
        stdout.contains("1004-bare"),
        "the bare lane has its row:\n{stdout}"
    );
    assert!(
        !stdout.contains("plan") && !stdout.contains("progress"),
        "no soothing line is invented for fields the frame did not carry:\n{stdout}"
    );
}

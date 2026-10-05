//! The proposed triage-provenance line, at the surface a person reads: it renders only when
//! the frame carried the field, and an absent field changes nothing.
//!
//! The field is a proposal pending a contract (the coordinator's composition, folded here as
//! render-only), so the strip's whole obligation is forward-safety: absent means absent, and
//! a frame-owning half that never adopts the field must see zero difference.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/strip")
        .join(name)
}

fn run_strip(args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_triage_line_follows_the_frame_that_carried_it_and_only_that_frame() {
    let stdout = run_strip(&[
        "strip",
        "--replay",
        fixture("triage.ndjson").to_str().unwrap(),
        "--cadence-ms",
        "0",
    ]);

    // The replay is wip-60 (triage: state), wip-61 (none), wip-62 (triage: coordinator). Each
    // board is the lines after its frame header; the triage line must appear under exactly
    // the boards whose frame carried it.
    let mut boards: Vec<(String, Vec<&str>)> = Vec::new();
    for line in stdout.lines() {
        if let Some(at) = line.find("· frame wip-") {
            let frame = line[at + 9..].split(' ').next().unwrap_or("").to_owned();
            boards.push((frame, Vec::new()));
        }
        if let Some((_, lines)) = boards.last_mut() {
            lines.push(line);
        }
    }

    assert!(boards.len() >= 3, "three frames, three boards:\n{stdout}");
    let by_frame = |name: &str| {
        boards
            .iter()
            .filter(|(frame, _)| frame == name)
            .map(|(_, lines)| lines.join("\n"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    assert!(
        by_frame("wip-60").contains("triage: answered from state · confidence 87%"),
        "the frame that carried the field renders it:\n{}",
        by_frame("wip-60")
    );
    assert!(
        !by_frame("wip-61").contains("triage:"),
        "the frame that carried nothing renders nothing:\n{}",
        by_frame("wip-61")
    );
    assert!(
        by_frame("wip-62").contains("triage: woke the coordinator · confidence 12%"),
        "the other proposed source renders as its own sentence:\n{}",
        by_frame("wip-62")
    );
}

//! Frames the strip cannot reconcile are refused and said so — never merged by guesswork.
//!
//! The replay carries a delta (`full: false`), a frame that does not say what it is, a frame
//! kind this build has never heard of, a line that is not JSON at all, and then a good
//! snapshot. The board that survives must be exactly the good snapshot's; everything the strip
//! could not prove is named on the way past.

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

#[test]
fn a_delta_an_unmarked_frame_and_garbage_change_nothing_and_are_all_said() {
    let (stdout, stderr) = run_strip(&[
        "strip",
        "--replay",
        fixture("delta-unknown.ndjson").to_str().unwrap(),
        "--cadence-ms",
        "0",
    ]);

    // The board is the good snapshot's and nothing else's.
    assert!(
        stdout.contains("real-lane") && stdout.contains("passed"),
        "the one good snapshot is the board:\n{stdout}"
    );
    assert!(
        !stdout.contains("delta-ghost") && !stdout.contains("unmarked-ghost"),
        "a lane from a frame the strip refused must not reach the board:\n{stdout}"
    );

    // Both refusals are said, in words, on the strip itself.
    assert!(
        stdout.contains("refused a frame: a delta, not a snapshot"),
        "the delta is refused out loud:\n{stdout}"
    );
    assert!(
        stdout.contains("refused a frame: not marked as a snapshot"),
        "the unmarked frame is refused out loud:\n{stdout}"
    );

    // The undecodable line is loud on the diagnostic stream and the run continues past it —
    // silent continuation would hide a contract change behind a strip that looks alive.
    assert!(
        stderr.contains("would not decode"),
        "a line that is not JSON is said on stderr:\n{stderr}"
    );

    // The unknown frame kind is skipped without a word — the skew law — and the exit is clean.
}

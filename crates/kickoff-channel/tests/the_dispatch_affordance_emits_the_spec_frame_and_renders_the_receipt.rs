//! The one dispatch affordance, end to end through the real binary: a pick composed from a
//! piped keyboard produces exactly the spec's spawn frame in the outbox, and the receipt the
//! seam answers with is rendered as a sentence an operator reads.
//!
//! This is also the headless smoke: the whole affordance — prompts, validation, frame, outbox,
//! receipt — runs under a pipe with no terminal, which is the mode a test and a harness share.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/strip")
        .join(name)
}

struct Run {
    stdout: String,
    stderr: String,
    code: Option<i64>,
}

fn run_strip_with_stdin(fixture_name: &str, typed: &str, extra: &[&str]) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kickoff-channel"))
        .arg("strip")
        .arg("--replay")
        .arg(fixture(fixture_name))
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("could not run the strip: {error}"));

    // A pipe, not a terminal: the typed answers go in whole and close, the way a harness
    // drives the pick.
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(typed.as_bytes())
        .expect("the strip is reading");

    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("could not wait for the strip: {error}"));
    Run {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().map(i64::from),
    }
}

/// One real task file, in a temp directory a test owns — the pick checks the box it runs on,
/// so the file has to exist for the happy path to be the happy path.
fn a_task_file(dir: &Path) -> PathBuf {
    let path = dir.join("LANE-TASK.md");
    std::fs::write(&path, "ship the strip slice\n").expect("write the task file");
    path
}

/// The accepted path: the pick composes the spec's spawn frame, byte for byte, into the
/// outbox; the receipt comes back and is rendered as a sentence.
#[test]
fn a_pick_composes_the_spec_frame_and_renders_the_receipt_that_accepts_it() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let task = a_task_file(dir.path());
    let outbox = dir.path().join("outbox.ndjson");

    let run = run_strip_with_stdin(
        "dispatch-accepted.ndjson",
        &format!("d\n1\n{}\nship the strip\n", task.display()),
        &[
            "--outbox",
            outbox.to_str().unwrap(),
            "--cadence-ms",
            "25",
            "--dispatch-id",
            "disp-demo-1",
        ],
    );

    assert_eq!(
        run.code,
        Some(0),
        "the strip leaves cleanly:\n{}",
        run.stderr
    );
    assert!(
        run.stdout.contains("dispatch disp-demo-1 sent to the seam"),
        "the send is on the strip:\n{}",
        run.stdout
    );
    assert!(
        run.stdout.contains("receipt disp-demo-1: accepted"),
        "the receipt is rendered:\n{}",
        run.stdout
    );

    let sent = std::fs::read_to_string(&outbox).expect("the outbox exists");
    let lines: Vec<&str> = sent.lines().collect();
    assert_eq!(lines.len(), 1, "one command, one line:\n{sent}");
    let frame: serde_json::Value = serde_json::from_str(lines[0]).expect("the line is JSON");

    // The spec's spawn shape, field for field — this is the reconcile-down anchor for the
    // affordance, exactly as the wip decode test is for the board.
    assert_eq!(frame["v"], 1);
    assert_eq!(frame["id"], "disp-demo-1");
    assert_eq!(frame["t"], "dispatch");
    assert_eq!(frame["action"], "spawn");
    assert_eq!(frame["agent"], "builder");
    assert_eq!(frame["task_ref"], task.display().to_string());
    assert_eq!(frame["reason"], "ship the strip");
    assert!(
        frame.get("lane").is_none(),
        "spawn carries no lane — absent, not null"
    );
    assert!(
        frame.get("deps").is_none() && frame.get("proof_cmd").is_none(),
        "optional fields stay absent, not empty"
    );
}

/// The rejected path: the receipt names its refusal in words, not in the wire's code — and
/// the frame was still composed and still sent, because a refusal is an answer, not a
/// non-event.
#[test]
fn a_rejected_receipt_is_rendered_with_its_refusal_in_words() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let task = a_task_file(dir.path());
    let outbox = dir.path().join("outbox.ndjson");

    let run = run_strip_with_stdin(
        "dispatch-rejected.ndjson",
        &format!("d\n1\n{}\nover the line\n", task.display()),
        &[
            "--outbox",
            outbox.to_str().unwrap(),
            "--cadence-ms",
            "25",
            "--dispatch-id",
            "disp-demo-1",
        ],
    );

    assert_eq!(
        run.code,
        Some(0),
        "a rejection is not a crash:\n{}",
        run.stderr
    );
    assert!(
        run.stdout
            .contains("receipt disp-demo-1: rejected — the budget gate said no"),
        "the wire's over_budget becomes the operator's sentence:\n{}",
        run.stdout
    );
    assert!(
        std::fs::read_to_string(&outbox)
            .expect("the outbox exists")
            .contains("disp-demo-1"),
        "the rejected command is still a record in the outbox"
    );
}

/// A task path that is not there is refused before anything is sent — the pick checks the box
/// it runs on because the operator is standing at that box, and an honest pick does not
/// compose a frame it can see is doomed.
#[test]
fn a_pick_for_a_task_file_that_is_not_there_sends_nothing() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let outbox = dir.path().join("outbox.ndjson");
    let nowhere = dir.path().join("definitely-not-here.md");

    let run = run_strip_with_stdin(
        "dispatch-accepted.ndjson",
        &format!("d\n1\n{}\nany reason\n", nowhere.display()),
        &[
            "--outbox",
            outbox.to_str().unwrap(),
            "--cadence-ms",
            "25",
            "--dispatch-id",
            "disp-demo-1",
        ],
    );

    assert!(
        run.stdout.contains("nothing was sent"),
        "the refusal names what did not happen:\n{}",
        run.stdout
    );
    assert!(
        !outbox.exists(),
        "a refused pick leaves no frame behind — nothing was composed"
    );
}

/// No outbox, no dispatch: the strip still renders, and the pick says why it cannot send
/// rather than composing a frame that goes nowhere.
#[test]
fn a_strip_with_no_outbox_offers_no_dispatch_that_could_send() {
    let run = run_strip_with_stdin("dispatch-accepted.ndjson", "d\n", &["--cadence-ms", "25"]);
    assert!(
        run.stdout.contains("dispatch is not wired on this seam"),
        "the unwired pick says so and sends nothing:\n{}",
        run.stdout
    );
}

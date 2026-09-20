//! **Which way a hub reaches the operator is said out loud, at the terminal, every time.**
//!
//! There are two planes now. One dials Telegram and holds a bot token; one carries nothing and
//! lets the app read the ring and write the answers drop. They are not interchangeable and the
//! difference is invisible once the process is up: both open the same socket, both serve the same
//! agents, and only one of them will ever buzz a phone.
//!
//! So the way is an argument, and it is REQUIRED. Inferring it from whether a token happens to be
//! set makes the silent mistake by construction — a typo in the credential file, or a unit whose
//! environment file was never rendered, would quietly become the app plane, and every agent on the
//! box would talk into a file while the operator waited for a phone that was never going to ring.
//! A default makes one of those two mistakes depending which way it points. A required argument
//! makes neither: there is no state reachable by omission.
//!
//! These run the real binary, because what is under test is the refusal a person sees.

use std::path::Path;
use std::process::{Command, Output};

/// Run the binary with no credentials whatsoever in the environment, and a state directory of
/// its own so a lock or a registry on this box cannot decide the answer.
///
/// Both spellings of every setting are removed: `compat.rs` accepts the former `HERDR_TG_` names,
/// so a box where the developer's own shell exports one would otherwise let a test pass on a
/// credential the test never meant it to have.
fn serve(state: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_herdr-tg"));
    cmd.arg("serve");
    cmd.args(args);
    cmd.env("XDG_STATE_HOME", state);
    for suffix in [
        "TOKEN",
        "ALLOWED_CHAT_IDS",
        "ALLOWED_USER_IDS",
        "FORUM_CHAT_ID",
    ] {
        cmd.env_remove(format!("KICKOFF_CHANNEL_{suffix}"));
        cmd.env_remove(format!("HERDR_TG_{suffix}"));
    }
    cmd.output().expect("run the hub")
}

#[test]
fn a_hub_must_be_told_which_way_it_reaches_him_and_will_not_guess() {
    let state = tempfile::tempdir().expect("a temp dir");
    let out = serve(state.path(), &[]);
    let said = String::from_utf8_lossy(&out.stderr).to_lowercase();

    assert!(
        !out.status.success(),
        "a hub started without being told which way it reaches him. stderr: {said}"
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "a missing argument is a usage error, which this binary documents as exit 2. stderr: {said}"
    );
    assert!(
        said.contains("--to"),
        "the refusal does not name the argument that is missing. stderr: {said}"
    );
    // Both ways, by name. A refusal that says an argument is missing without saying what may be
    // written in it sends a reader to `--help` to find out there are exactly two answers.
    assert!(
        said.contains("telegram") && said.contains("app"),
        "the refusal does not name both ways a hub can reach him. stderr: {said}"
    );
    // It must not have got as far as a credential or a lock: nothing was decided, so nothing
    // downstream of the decision may have run.
    assert!(
        !said.contains("token") && !said.contains("already running"),
        "the hub got past the missing argument before refusing. stderr: {said}"
    );
}

#[test]
fn a_hub_told_to_use_the_phone_line_and_given_no_token_still_refuses_to_start() {
    // The refusal that was already there, unchanged. The whole point of adding a second plane is
    // that the first one behaves exactly as it did: a hub told to reach him on his phone and given
    // nothing to reach it with says so, by the name of the setting, and does not start half-alive.
    let state = tempfile::tempdir().expect("a temp dir");
    let out = serve(state.path(), &["--to", "telegram"]);
    let said = String::from_utf8_lossy(&out.stderr);

    assert!(
        !out.status.success(),
        "a hub told to use the phone line started with no token. stderr: {said}"
    );
    assert!(
        said.contains("KICKOFF_CHANNEL_TOKEN is not set"),
        "the refusal no longer names the setting the operator has to write. stderr: {said}"
    );
    assert!(
        said.contains("scripts/setup-token.sh"),
        "the refusal no longer names the way to fix it. stderr: {said}"
    );
}

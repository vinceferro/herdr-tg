//! One hub per state directory — proven against the real binary, not against the lock in isolation.
//!
//! The unit tests in `lock.rs` prove that a second `acquire` is refused. They cannot prove the
//! thing that actually matters, which is an **ordering**: that the refusal happens before this
//! process has a `Bot`, a token, or any claim on Telegram's single long-poll slot.
//!
//! So this test runs `herdr-tg serve` with **no token at all**. If the lock were taken even one
//! step later — after `Config::load`, say — the binary would complain about the missing token and
//! this test would see it. The absence of that complaint is the proof.
//!
//! Why it matters, concretely: two processes polling one bot token do not share it. They take
//! turns losing on HTTP 409, and what the operator sees is a bot that answers sometimes. This
//! system has already paid for that once, with an infinite backoff and a 198-line `/proc` walk
//! written to detect it after the fact.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Command, Output};

use rustix::fs::{FlockOperation, flock};

/// Stand in for a hub that is already running.
///
/// Holding the flock is the whole of what a live hub does to this file, so impersonating one needs
/// nothing else — and doing it in-process means the incumbent cannot exit early and hand the lock
/// over in the middle of a test. `name_itself` mirrors the real hub's second step, writing its pid
/// so a contender can report who to stop; leaving it out reproduces the real window between taking
/// the lock and naming yourself.
fn hold_the_lock(state: &Path, name_itself: bool) -> std::fs::File {
    std::fs::create_dir_all(state).expect("state dir");
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(state.join("hub.lock"))
        .expect("open the lock file");
    flock(&f, FlockOperation::NonBlockingLockExclusive).expect("hold the lock");
    if name_itself {
        write!(f, "{}", std::process::id()).expect("write the pid");
        f.flush().expect("flush");
    }
    f
}

/// Run `herdr-tg serve` against a state dir, with no credentials whatsoever in the environment.
///
/// **Told to reach him on the phone line, explicitly.** Which way a hub reaches him is a required
/// argument with no default, and this whole file is about the ordering on the plane that has a
/// token to reach too late for. Leaving it off would make every run here a usage error — which
/// every assertion below about what the hub did NOT say is satisfied by, so all three tests would
/// go green while proving nothing at all.
///
/// Both spellings of every setting are removed, not only the former ones: `compat.rs` accepts
/// either, so a developer's own shell exporting the current spelling would hand this test the very
/// credential it is proving the hub never reaches.
fn second_hub(home: &Path) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_herdr-tg"));
    cmd.arg("serve").arg("--to").arg("telegram");
    cmd.env("XDG_STATE_HOME", home);
    for suffix in [
        "TOKEN",
        "ALLOWED_CHAT_IDS",
        "ALLOWED_USER_IDS",
        "FORUM_CHAT_ID",
    ] {
        cmd.env_remove(format!("KICKOFF_CHANNEL_{suffix}"));
        cmd.env_remove(format!("HERDR_TG_{suffix}"));
    }
    cmd.output().expect("run the second hub")
}

#[test]
fn a_second_hub_never_reaches_the_token() {
    let home = tempfile::tempdir().expect("a temp dir");
    let _incumbent = hold_the_lock(&home.path().join("herdr-tg"), true);

    let out = second_hub(home.path());
    let said = String::from_utf8_lossy(&out.stderr).to_lowercase();

    assert!(
        !out.status.success(),
        "a second hub started while another held the lock. stderr: {said}"
    );
    assert!(
        said.contains("already running"),
        "the second hub did not say another was running. stderr: {said}"
    );

    // THE ORDERING, and the reason this test runs the binary rather than the module. There is no
    // token in this environment. A hub that read its config first would say so, and that sentence
    // would mean the lock came too late to stop it reaching the Bot API.
    assert!(
        !said.contains("token"),
        "the second hub got as far as looking for a token before the lock stopped it — the lock \
         must be taken FIRST. stderr: {said}"
    );

    // It must also name the holder, because "something else is running" is not actionable from a
    // phone and "process 1234 is running" is.
    assert!(
        said.contains(&std::process::id().to_string()),
        "the refusal did not name the process holding the line. stderr: {said}"
    );
}

#[test]
fn a_holder_that_has_not_named_itself_yet_is_reported_honestly() {
    // A real window, not a contrivance: a hub takes the lock and writes its pid as two steps, and
    // this test found the gap between them the first time it ran. The refusal must still be a
    // refusal, and it must not invent a pid to look more helpful than it is.
    let home = tempfile::tempdir().expect("a temp dir");
    let _incumbent = hold_the_lock(&home.path().join("herdr-tg"), false);

    let out = second_hub(home.path());
    let said = String::from_utf8_lossy(&out.stderr).to_lowercase();

    assert!(
        !out.status.success(),
        "a second hub started. stderr: {said}"
    );
    assert!(
        said.contains("already running") && said.contains("has not named itself yet"),
        "an unnamed holder must still produce a refusal that says so. stderr: {said}"
    );
    assert!(
        !said.contains("process 0") && !said.contains("process ("),
        "a pid was invented to fill the gap. stderr: {said}"
    );
}

#[test]
fn a_hub_starts_when_nothing_holds_the_lock() {
    // The other half, and not a formality: a lock that is never released would make the first test
    // pass forever while the product never started at all.
    let home = tempfile::tempdir().expect("a temp dir");

    let out = second_hub(home.path());
    let said = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(
        !said.contains("already running"),
        "a lone hub was told another was running. stderr: {said}"
    );
    // It still fails — there is no token here — and that failure is the evidence that the lock let
    // it through to the config it was always going to trip over. Asserted on the SENTENCE and not
    // merely on the exit code: every way of never starting at all also exits non-zero, and this
    // test's whole job is to prove the hub got as far as the one thing the other two prove it
    // never reaches.
    assert!(!out.status.success(), "a hub with no token must not start");
    assert!(
        said.contains("token"),
        "a lone hub did not get as far as looking for a token, so this test can no longer tell a \
         lock that let it through from a hub that never started. stderr: {said}"
    );
}

//! The keystroke path is opt-in, and the shipped unit does not opt in.
//!
//! `serve` starts two things: the hub, and the older path that watches herdr's panes and can put
//! keystrokes in a real terminal. The second is stopped by decision — a review found four ways it
//! could type the wrong thing — and for a while starting the service to look at the HUB silently
//! armed it. That is not a hypothetical: it happened the first time the service was started for
//! real, and the operator's phone filled with pane traffic while `reply path armed` went by in the
//! log.
//!
//! So the pane path needs `HERDR_TG_PANES=1`, and these tests pin the two things that keep the
//! default honest: the binary defaults to off, and nothing this repo installs turns it on.

use std::path::Path;

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/herdr-tg is two levels below the repo root")
}

#[test]
fn nothing_this_repo_installs_turns_the_pane_path_on() {
    // The unit is what an install puts in front of systemd, and systemd is what starts this on a
    // reboot nobody is watching. A `HERDR_TG_PANES=1` that crept in here would arm a keystroke path
    // on every boot, with no one having decided anything.
    for unit in [
        "deploy/herdr-tg.service",
        "deploy/herdr-tg-watchdog.service",
    ] {
        let path = repo().join(unit);
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{unit}: {e}"));
        assert!(
            !body.contains("HERDR_TG_PANES"),
            "{unit} arms the pane path. Turning it on is a decision someone makes in their own \
             environment file, not something an install does for them."
        );
    }
}

#[test]
fn the_env_file_the_installer_expects_does_not_arm_it_either() {
    // `scripts/setup-token.sh` writes ~/.config/herdr-tg/env, which the unit reads. If the setup
    // script started writing this variable, every fresh install would arrive armed.
    let path = repo().join("scripts/setup-token.sh");
    let body = std::fs::read_to_string(&path).expect("the setup script is in the repo");
    assert!(
        !body.contains("HERDR_TG_PANES"),
        "setup-token.sh writes the pane path into a fresh install's environment"
    );
}

#[test]
fn the_gate_is_a_single_exact_value_and_not_merely_presence() {
    // `HERDR_TG_PANES=0` and `HERDR_TG_PANES=` must both mean off. A gate that reads as "is the
    // variable set at all" turns an attempt to switch something OFF into switching it on, which is
    // the worst possible direction for this particular flag.
    let src = std::fs::read_to_string(repo().join("crates/herdr-tg/src/bot.rs"))
        .expect("bot.rs is in the repo");
    assert!(
        src.contains(r#"is_ok_and(|v| v == "1")"#),
        "the pane gate must compare the value, not test for the variable's presence"
    );
}

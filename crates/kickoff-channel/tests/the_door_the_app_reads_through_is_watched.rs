//! **The hub cannot witness the program the operator's app actually reaches, so something else must.**
//!
//! On the app plane every leg of the hub's stamp is the hub watching itself: it writes the ring, it
//! accepts at the agents' door, it lists the place his answers land. The whole of the distance
//! between the operator and those three files is a different program — `kickoff-door`, its own
//! unit, its own binary, its own port, its own credential — and it dies on its own: a port already
//! taken, a binary missing after a partial install, a token rotated out from under it, a crash
//! loop.
//!
//! It was proved rather than argued. `serve --to app` started on a box with no `kickoff-door`
//! binary anywhere stamped `hub.heartbeat` within a minute, with all three legs green, and an armed
//! watchdog said nothing at all — for ever. That is this system's signature failure (something
//! answering its own liveness check while every real path through it is dead) recurring one process
//! boundary further out.
//!
//! The hub must not close it: a hub that health-checked an HTTP port would be a hub that had
//! learned what a port is, and it could not close it honestly anyway — every trace the door leaves
//! in the state home appears only when the OPERATOR acts, so "nothing has been written for ten
//! minutes" is a man asleep and a dead door wearing one face. So `deploy/herdr-tg-watchdog.sh`
//! asks the user manager, which is the one place the answer exists.
//!
//! # These run the real script
//!
//! Not a transcription of it. Every case below starts `bash deploy/herdr-tg-watchdog.sh` against a
//! staged state directory with a stand-in `systemctl` first on `PATH` — a stand-in rather than the
//! box's own manager, because a guard whose answer depends on whether this developer happens to
//! have a door enabled is a guard that proves nothing on anybody else's machine.
//!
//! Nothing here can send: every case runs with the credentials pointed at `/dev/null`, so a real
//! alarm attempt fails in `send` and what is under test is always the DECISION.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace root: `crates/kickoff-channel/` → up two.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kickoff-channel sits two levels below the workspace root")
        .to_path_buf()
}

/// The script under test. The real one the installer copies, never a fixture of it.
fn watchdog() -> PathBuf {
    let p = workspace_root().join("deploy/herdr-tg-watchdog.sh");
    assert!(
        p.exists(),
        "the watchdog script is not at {} — this guard proves nothing about a file it cannot find",
        p.display()
    );
    p
}

/// What a stand-in user manager will say about the door's unit when it is asked.
#[derive(Clone, Copy)]
enum TheUserManagerSays {
    /// He enabled it and it is up: `active (running)`.
    ItIsEnabledAndRunning,
    /// He enabled it and it is not up. The pair is given rather than derived, because the two
    /// words come from overlapping vocabularies and the difference between them is the whole
    /// point of one of these cases.
    ItIsEnabledAnd(&'static str, &'static str),
    /// He never enabled it, or he masked it. Not this alarm's business.
    HeNeverEnabledIt,
}

/// A directory holding a stand-in `systemctl`, put first on `PATH`.
///
/// Written as a shell script rather than mocked in-process because the thing under test is a shell
/// script calling a program by name through `PATH` — which is how the installed watchdog reaches
/// the real one, and the unit pins `PATH` so that nothing else can be picked up there.
fn a_user_manager_that_says(dir: &Path, says: TheUserManagerSays) {
    let body = match says {
        TheUserManagerSays::ItIsEnabledAndRunning => {
            "case \"$*\" in\n  *is-enabled*) echo enabled; exit 0 ;;\n  *show*) printf \
             'ActiveState=active\\nSubState=running\\n'; exit 0 ;;\nesac\nexit 1\n"
                .to_owned()
        }
        TheUserManagerSays::ItIsEnabledAnd(state, sub) => format!(
            "case \"$*\" in\n  *is-enabled*) echo enabled; exit 0 ;;\n  *show*) printf \
             'ActiveState={state}\\nSubState={sub}\\n'; exit 0 ;;\nesac\nexit 1\n"
        ),
        TheUserManagerSays::HeNeverEnabledIt => {
            "case \"$*\" in\n  *is-enabled*) echo disabled; exit 1 ;;\n  *show*) printf \
             'ActiveState=inactive\\nSubState=dead\\n'; exit 0 ;;\nesac\nexit 1\n"
                .to_owned()
        }
    };
    let path = dir.join("systemctl");
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).expect("writes a stand-in user manager");
    let mut perms = std::fs::metadata(&path).expect("stats it").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&path, perms).expect("makes it runnable");
}

/// A state directory as the hub and the watchdog would have left it.
struct Staged {
    dir: tempfile::TempDir,
    bin: tempfile::TempDir,
}

impl Staged {
    fn new(says: TheUserManagerSays) -> Self {
        let bin = tempfile::tempdir().expect("a temp dir for the stand-in");
        a_user_manager_that_says(bin.path(), says);
        let me = Self {
            dir: tempfile::tempdir().expect("a temp state dir"),
            bin,
        };
        // Armed, because everything this script judges is inside its arming and the door is no
        // exception: a door with no hub to serve is a machine mid-setup, not an outage.
        me.write("watchdog.armed", "");
        // And past the resume window it gives a hub after any gap in its own checks, or every case
        // here would be the silence that window exists to produce.
        let a_minute_ago = seconds_since_1970() - 60;
        me.write("watchdog.tick", &format!("{a_minute_ago} 0\n"));
        me
    }

    /// What the stand-in manager says from the NEXT check on. A door that flaps cannot be staged
    /// in one sample, and one sample is all a `Staged` used to be able to hold.
    fn and_now_the_user_manager_says(&self, says: TheUserManagerSays) {
        a_user_manager_that_says(self.bin.path(), says);
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.dir.path().join(name), text)
            .unwrap_or_else(|e| panic!("writes {name}: {e}"));
    }

    /// The hub stamped, this many seconds ago. The watchdog reads the modification time.
    fn the_hub_stamped(&self, secs_ago: u64) -> &Self {
        self.write("hub.heartbeat", "serving\n");
        age(&self.dir.path().join("hub.heartbeat"), secs_ago);
        self
    }

    /// The hub's note, written ten seconds ago — young enough for the script to quote.
    fn the_hub_said(&self, lines: &[&str]) -> &Self {
        self.write("hub.health", &format!("{}\n", lines.join("\n")));
        age(&self.dir.path().join("hub.health"), 10);
        self
    }

    /// One check, deciding as usual and sending nothing.
    fn one_check(&self) -> String {
        let out = self.run(&["--dry-run"]);
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new("bash")
            .arg(watchdog())
            .args(args)
            .env("HERDR_TG_STATE_DIR", self.dir.path())
            // No credentials anywhere, so a real send always fails and the decision is what is
            // being read.
            .env("HERDR_TG_ENV_FILE", "/dev/null")
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.bin.path().display()),
            )
            .output()
            .expect("runs the watchdog")
    }
}

fn seconds_since_1970() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs()
}

/// Age a file by setting its modification time, which is the only thing the script reads off it.
fn age(path: &Path, secs_ago: u64) {
    let f = std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap_or_else(|e| panic!("opens {} to age it: {e}", path.display()));
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs_ago))
        .expect("ages it");
}

/// A hub on the app plane with every leg of its own stamp green. Exactly the box the sceptic ran:
/// the ring is being written, the agents' door is letting connections through, and the answers
/// sweep is going round.
const EVERY_LEG_OF_THE_HUB_IS_GREEN: [&str; 4] = [
    "serving",
    "what agents say is being written down for the app",
    "the agents' door let a connection through 12 seconds ago",
    "the hub went to collect your answers 12 seconds ago",
];

/// **The finding, in one test.** The hub is stamping, every leg it has is green, and the operator's
/// app is completely dark because the program it reaches this machine through is not running.
///
/// Before this, the watchdog exited 0 and printed nothing — for ever, on a box that had never once
/// served the app it was installed for.
#[test]
fn a_dead_door_beside_a_hub_whose_every_leg_is_green_is_an_alarm_and_not_silence() {
    let staged = Staged::new(TheUserManagerSays::ItIsEnabledAnd("inactive", "dead"));
    staged.the_hub_stamped(12);
    staged.the_hub_said(&EVERY_LEG_OF_THE_HUB_IS_GREEN);

    let said = staged.one_check();
    assert!(
        said.contains("would send"),
        "the hub was stamping, its door was dead, and the alarm said nothing at all: {said:?}"
    );
    assert!(
        said.contains("the door your app reaches it through has stopped"),
        "the alarm does not say which thing stopped: {said}"
    );
    // The hub is fine and he must not be sent to restart it: that costs every agent on the box a
    // reconnect and fixes nothing.
    assert!(
        said.contains("Restarting kickoff-door"),
        "the alarm does not say what to restart: {said}"
    );
    assert!(
        !said.contains("Restarting the hub is the usual fix"),
        "the alarm sent him to restart a hub that is stamping perfectly: {said}"
    );
    // And it must not blame any half of the hub's own stamp, every one of which the hub says is
    // healthy — the mistake the `unclear` arm exists to avoid, in a new place.
    for blamed in [
        "nothing it says is reaching your app",
        "agents cannot reach the herd",
        "nothing you tap is reaching an agent",
    ] {
        assert!(
            !said.contains(blamed),
            "the alarm blamed a half of the hub that the hub itself says is working: {said}"
        );
    }
}

/// A door that cannot start at all never reaches `failed`, and the check has to know it.
///
/// `kickoff-door.service` sets `Restart=always` with no start limit, deliberately, so a unit whose
/// binary is missing after a partial install restarts every five seconds for the life of the box
/// and sits in `activating (auto-restart)` the whole time. A check written on `is-failed`, or on
/// `ActiveState` alone, calls that healthy — and a missing binary is the exact shape this was
/// written for.
#[test]
fn a_door_crash_looping_on_a_binary_that_is_not_there_is_read_as_dead_and_not_as_starting() {
    let staged = Staged::new(TheUserManagerSays::ItIsEnabledAnd(
        "activating",
        "auto-restart",
    ));
    staged.the_hub_stamped(12);
    staged.the_hub_said(&EVERY_LEG_OF_THE_HUB_IS_GREEN);

    let said = staged.one_check();
    assert!(
        said.contains("the door your app reaches it through has stopped"),
        "a door restarting for ever on a binary that is not there was read as one that is coming \
         up: {said:?}"
    );
}

/// The cry-wolf half, and it is the half that decides whether this alarm is worth having.
///
/// Most boxes have no door: the phone plane is the older product and a hub can serve a herd with no
/// app anywhere near it. Alarming about a program the operator never asked for would be one message
/// a minute for ever on a machine where nothing is wrong, which is not a quieter failure than a
/// missed alarm — it is how he learns to ignore the one message that has to be trusted.
#[test]
fn a_door_nobody_enabled_on_this_box_is_never_alarmed_about() {
    let staged = Staged::new(TheUserManagerSays::HeNeverEnabledIt);
    staged.the_hub_stamped(12);
    staged.the_hub_said(&EVERY_LEG_OF_THE_HUB_IS_GREEN);

    let out = staged.run(&["--dry-run"]);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        said.is_empty(),
        "a box that never ran a door was alarmed about the one it does not have: {said}"
    );
    assert_eq!(out.status.code(), Some(0));
}

/// And a door that is up leaves the watchdog exactly as quiet as it has always been.
#[test]
fn a_door_that_is_running_is_not_reported_as_one_that_has_stopped() {
    let staged = Staged::new(TheUserManagerSays::ItIsEnabledAndRunning);
    staged.the_hub_stamped(12);
    staged.the_hub_said(&EVERY_LEG_OF_THE_HUB_IS_GREEN);

    let out = staged.run(&["--dry-run"]);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.is_empty(), "a running door was alarmed about: {said}");
    assert_eq!(out.status.code(), Some(0));
}

/// Two things broken must never arrive as one of them.
///
/// When the hub's own stamp has stopped as well, the hub's outage is the headline — it is where he
/// has to go first, and its shape is what says so — but the door is named beside it. Dropping the
/// door there would mean he restarts the hub, watches it come back green, and still has a dark app
/// with nothing left to tell him why.
#[test]
fn a_hub_that_has_gone_quiet_is_still_the_headline_when_its_door_has_gone_too_and_the_door_is_named_beside_it()
 {
    let staged = Staged::new(TheUserManagerSays::ItIsEnabledAnd("inactive", "dead"));
    staged.the_hub_stamped(3600);
    staged.the_hub_said(&[
        "not serving",
        "what agents say cannot be written down for the app to read",
        "the agents' door let a connection through 12 seconds ago",
        "the hub went to collect your answers 12 seconds ago",
    ]);

    let said = staged.one_check();
    assert!(
        said.contains("nothing it says is reaching your app"),
        "the hub's own outage stopped being the headline: {said}"
    );
    assert!(
        said.contains("the door your app reaches this machine through has stopped as well"),
        "the door was swallowed by the hub's outage, so fixing the hub would leave him with a dark \
         app and nothing left to tell him why: {said}"
    );
    // The hub said nothing about the door and cannot: quoting it as the hub's own words would be
    // this script inventing a sentence for a program the hub has never touched.
    assert!(
        !said.contains("The hub says: And the door"),
        "a fact the hub cannot witness was attributed to the hub: {said}"
    );
}

/// A door that stays down is one message, not one a minute.
///
/// The repeat interval is the only thing standing between an outage and a phone buzzing every
/// sixty seconds until he silences the alarm for good, and a new shape is exactly where that
/// machinery stops applying: the record of which halves have already been told is a set of shape
/// names, and a shape missing from it resets the throttle on every single check.
#[test]
fn a_door_that_stays_down_is_told_once_and_not_once_a_minute() {
    let staged = Staged::new(TheUserManagerSays::ItIsEnabledAnd("inactive", "dead"));
    staged.the_hub_said(&EVERY_LEG_OF_THE_HUB_IS_GREEN);

    let mut told = 0;
    for _ in 0..10 {
        // The hub goes on stamping and this script goes on checking, exactly as they would.
        staged.the_hub_stamped(12);
        let a_minute_ago = seconds_since_1970() - 60;
        staged.write("watchdog.tick", &format!("{a_minute_ago} 0\n"));
        let out = staged.run(&[]);
        if String::from_utf8_lossy(&out.stderr).contains("operator was NOT told") {
            told += 1;
        }
    }
    assert!(
        told >= 1,
        "ten checks over a dead door and the operator was never told once"
    );
    assert!(
        told <= 2,
        "one door outage sent {told} alarms in ten checks; the repeat interval never applied to \
         the door's own shape"
    );
}

/// **And a door that comes back between two checks is still one outage.**
///
/// The case above drives ten identical samples, which is the one shape a door does not take when
/// it is failing the way its own unit is written to fail: `kickoff-door.service` sets
/// `Restart=always` with `RestartSec=5` and no start limit, so a door that binds and dies tens of
/// seconds later — a token rotated out from under it, a panic on a request, an OOM — is `running`
/// when half the checks look and `dead` when the other half do. A guard that only ever sees one
/// state cannot see that, which is why it was the code AND the test that let this through.
///
/// Measured against the real script before the fix: a door that simply stayed down sent one alarm
/// in ten checks, and a door flapping at the check interval sent SIX in twelve. One sample of
/// `active` wiped the latch and the record of what had been told, so the next check that caught it
/// down started the repeat interval from zero. This file's two older guards name that same shape
/// twice — a stamp flapping at the staleness window, and a leg flapping while the door stayed shut
/// — and it is one message a minute all night either way.
#[test]
fn a_door_that_comes_back_between_two_checks_does_not_start_the_repeat_interval_again() {
    let staged = Staged::new(TheUserManagerSays::ItIsEnabledAndRunning);
    staged.the_hub_said(&EVERY_LEG_OF_THE_HUB_IS_GREEN);

    let mut told = 0;
    for check in 0..12 {
        // The hub is perfectly well throughout: this is the door's own failure, and the hub is
        // blind to it by construction.
        staged.the_hub_stamped(12);
        let a_minute_ago = seconds_since_1970() - 60;
        staged.write("watchdog.tick", &format!("{a_minute_ago} 0\n"));
        staged.and_now_the_user_manager_says(if check % 2 == 0 {
            TheUserManagerSays::ItIsEnabledAnd("inactive", "dead")
        } else {
            TheUserManagerSays::ItIsEnabledAndRunning
        });
        let out = staged.run(&[]);
        if String::from_utf8_lossy(&out.stderr).contains("operator was NOT told") {
            told += 1;
        }
    }
    assert!(
        told >= 1,
        "twelve checks over a door that was down on six of them and the operator was never told"
    );
    assert!(
        told <= 2,
        "a door flapping at the check interval sent {told} alarms in twelve checks. It is one \
         outage: the record of what has already been told must survive a single sample of the \
         door being up, or the repeat interval is off for as long as the flap lasts."
    );
}

/// **The other half of that margin, and the defect a fix for the flap introduces on its own.**
///
/// Holding the record through a flap is right; holding it for ever is a worse fault than the one
/// it cures, because the next outage of the same shape then waits out a whole repeat interval
/// before he hears about it. A door that has really come back — up on every check across the
/// window — clears it, and the outage after that is news again.
#[test]
fn a_door_that_has_really_come_back_clears_the_record_so_the_next_outage_is_told_at_once() {
    let staged = Staged::new(TheUserManagerSays::ItIsEnabledAnd("inactive", "dead"));
    staged.the_hub_said(&EVERY_LEG_OF_THE_HUB_IS_GREEN);
    let a_check = |staged: &Staged| {
        staged.the_hub_stamped(12);
        let a_minute_ago = seconds_since_1970() - 60;
        staged.write("watchdog.tick", &format!("{a_minute_ago} 0\n"));
        staged.run(&[])
    };

    a_check(&staged);
    assert!(
        staged.dir.path().join("watchdog.latch").exists(),
        "a dead door did not latch at all, so this case proves nothing about clearing it"
    );

    // Ten minutes of checks that all found it up. Only the passage of time is staged — the way
    // every other clock in this file is staged — and the decision is the script's own.
    let ten_minutes_ago = seconds_since_1970() - 600;
    staged.write("watchdog.door", &format!("{ten_minutes_ago}\n"));
    staged.and_now_the_user_manager_says(TheUserManagerSays::ItIsEnabledAndRunning);
    a_check(&staged);
    for left in ["watchdog.latch", "watchdog.legs"] {
        assert!(
            !staged.dir.path().join(left).exists(),
            "a door that came back for good left {left} behind, so the next outage of the same \
             shape would wait out a repeat interval nobody is owed"
        );
    }

    staged.and_now_the_user_manager_says(TheUserManagerSays::ItIsEnabledAnd("inactive", "dead"));
    let out = a_check(&staged);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("operator was NOT told"),
        "the door died a second time and the alarm was swallowed by the first outage's throttle"
    );
}

/// Where there is nothing to ask, nothing is claimed.
///
/// A box with no user manager on `PATH` — a container, a machine where this script is being run by
/// hand from a stripped environment — must behave exactly as it did before any of this existed.
/// The failure to avoid is the opposite of the one this file is about: an alarm that fires because
/// it could not look is an alarm that fires for ever on a box where nothing is wrong.
#[test]
fn a_box_with_no_user_manager_to_ask_is_left_exactly_as_quiet_as_it_has_always_been() {
    let staged = Staged::new(TheUserManagerSays::ItIsEnabledAnd("inactive", "dead"));
    staged.the_hub_stamped(12);
    staged.the_hub_said(&EVERY_LEG_OF_THE_HUB_IS_GREEN);
    // Everything the script actually calls, and nothing else. Named one at a time rather than
    // copied wholesale, so a box missing one of them fails here saying which instead of producing
    // a silence that looks like the property under test.
    let only = tempfile::tempdir().expect("a temp bin dir");
    for tool in [
        "bash", "date", "stat", "sed", "head", "hostname", "mktemp", "rm", "mv", "cat", "tr",
        "grep", "cut",
    ] {
        let from = PathBuf::from("/usr/bin").join(tool);
        assert!(
            from.exists(),
            "{} is not on this box, so this case cannot run the script at all",
            from.display()
        );
        std::os::unix::fs::symlink(&from, only.path().join(tool)).expect("links a tool");
    }
    assert!(
        !only.path().join("systemctl").exists(),
        "the one program this case is about must not be on the path it builds"
    );

    let out = Command::new("bash")
        .arg(watchdog())
        .arg("--dry-run")
        .env("HERDR_TG_STATE_DIR", staged.dir.path())
        .env("HERDR_TG_ENV_FILE", "/dev/null")
        .env("PATH", only.path())
        .output()
        .expect("runs the watchdog");
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        said.is_empty(),
        "a box where nothing could be asked was alarmed about anyway: {said}"
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

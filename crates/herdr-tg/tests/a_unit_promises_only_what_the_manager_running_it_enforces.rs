//! **A hardening line that does nothing must not be commented as though it did.**
//!
//! `deploy/herdr-tg-app.service` carried `IPAddressDeny=any` under a comment saying that line was
//! what made "nothing off the box, ever" true rather than merely intended. It is not. IP filtering
//! is a cgroup BPF program, an unprivileged user manager cannot install one, and every install this
//! repo documents is `systemctl --user`. Measured on this box, 19 September 2026:
//!
//! ```text
//! $ systemd-run --user --wait --pipe -q -p IPAddressDeny=any -p IPAddressAllow=localhost \
//!       curl -sS -o /dev/null -w '%{http_code}\n' http://1.1.1.1/
//! 301
//! $ systemd-run --user --wait --pipe -q curl -sS -o /dev/null -w '%{http_code}\n' http://1.1.1.1/
//! 301                       # the control run, no properties at all: the same answer
//! ```
//!
//! The unit starts clean, `systemctl --user show` reads the addresses back as set, and the only
//! trace is one line in the journal — `bpf-firewall: Preparation of BPF allow maps failed:
//! Operation not permitted` — that nothing fails on. A reader of the unit, of `systemctl show`, or
//! of `systemctl status` is told the filter is there in all three places.
//!
//! That is the defect this repo is least willing to ship: a guard that does not prove what it says.
//! So every directive under a unit's `Hardening` heading is classified here by what was MEASURED of
//! it, on the manager that really runs it, and a directive nobody has measured turns the workspace
//! red until somebody does.
//!
//! # And the guard itself had the same shape
//!
//! It held a hand-written list of two units. `deploy/kickoff-door.service` — the unit for the one
//! program in this workspace that listens, edited in this same round — was not on it, and so kept
//! every claim the round had just deleted from the other two: that it was "tighter than"
//! herdr-tg.service, an address-family line headed "Loopback TCP only" as though a family list
//! bounded destinations, and the same inert pair with nothing said about it. Nothing went red,
//! because a list of files stops covering the next file somebody adds without ever saying so —
//! which is a control that reads as though it were in force. The units are now WALKED out of
//! `deploy/`, and three ways round that are held shut below: a unit with no heading, a sandbox
//! line moved out from under one, and a directive nobody has run.
//!
//! # How the classification was measured
//!
//! One probe run twice under `systemd-run --user`: once bare, once under the unit's own directive
//! list read out of the unit file. A directive counts as enforced only where the two runs DISAGREE
//! — a probe that fails both ways proves nothing about the directive and says so.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/herdr-tg sits two levels below the workspace root")
        .to_path_buf()
}

/// Every `*.service` this repo ships, FOUND rather than listed.
///
/// It was a hand-written list of two, and the list was itself a defect. `deploy/kickoff-door.service`
/// — the unit for the one program in this workspace that listens, edited in the very round that
/// measured the inert line — was not in it, so it kept every claim that round deleted from the
/// other two: "tighter than" another unit, an address-family line described as though it bounded
/// destinations, and the deny/allow pair with no word that it does nothing here. Nothing went red,
/// because a guard with a list of files silently stops covering the next file somebody adds, and
/// goes on passing while it does. That is the same shape as the finding this file is about: a
/// control that reads as though it were in force.
///
/// The floor is here because a walk that finds nothing passes everything — the failure this repo
/// already names in `scripts/fleet-trial.sh`, where a filter that matched no test reported green.
fn every_unit_this_repo_ships() -> Vec<String> {
    let dir = workspace_root().join("deploy");
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            panic!(
                "this guard walks {} and cannot: {e}. A guard that cannot read the units it holds \
                 is a guard that holds nothing.",
                dir.display()
            )
        })
        .map(|entry| entry.expect("a readable entry in deploy/").path())
        .filter(|p| p.extension().is_some_and(|x| x == "service"))
        .map(|p| {
            format!(
                "deploy/{}",
                p.file_name().expect("a file name").to_string_lossy()
            )
        })
        .collect();
    out.sort();
    assert!(
        out.len() >= 4,
        "the walk of deploy/ found {} unit files. This repo has shipped more than that since the \
         watchdog existed, so the walk is looking in the wrong place — and a walk that finds \
         nothing is a guard that passes everything.",
        out.len()
    );
    out
}

/// The units this guard holds to the table: every one that sandboxes itself at all.
///
/// A unit that sandboxes nothing claims nothing. `kickoff-hub-attach@.service` says in so many
/// words that it has no hardening block on purpose — it runs an agent's shell, and every line of
/// the hub's block is one that shell would trip — and the desktop-notification unit is four lines.
/// Holding those to a table of sandbox directives would be the guard inventing a requirement.
///
/// **The limit, written down rather than implied.** A unit is recognised as sandboxing itself by
/// its `Hardening` heading or by a directive this table already knows. One that ships only
/// directives nobody has ever measured, under no heading, is invisible here — it is also a unit
/// that makes no hardening claim any reader could check, which is the state the heading exists to
/// leave behind.
fn units_that_sandbox_themselves() -> Vec<String> {
    every_unit_this_repo_ships()
        .into_iter()
        .filter(|u| {
            the_hardening_block(u).is_some()
                || !directives_this_table_knows_anywhere_in(u).is_empty()
        })
        .collect()
}

/// What running the probe proved about a directive on a `--user` manager.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WhatWasMeasured {
    /// The bare run and the hardened run disagreed: the directive does what it says here.
    ItIsEnforcedHere,
    /// The hardened run behaved exactly like the bare one. The line is present, readable back from
    /// `systemctl show`, and absent from the kernel.
    ItDoesNothingHere,
    /// The probe could not tell the two runs apart for a reason that is not the directive's fault,
    /// so nothing is claimed either way. These must never be described in a unit as enforcing
    /// anything, but they need no disclaimer either: they cost nothing and they are real wherever
    /// the missing precondition exists.
    NobodyCanTellOnThisBox,
}

use WhatWasMeasured::*;

/// The measurement, directive by directive. The `why` is what the probe actually did, so that the
/// next person can re-run it rather than take this table's word for it.
fn what_the_probe_found() -> BTreeMap<&'static str, (WhatWasMeasured, &'static str)> {
    BTreeMap::from([
        (
            "NoNewPrivileges",
            (
                ItIsEnforcedHere,
                "/proc/self/status NoNewPrivs: 0 bare, 1 hardened",
            ),
        ),
        (
            "PrivateTmp",
            (
                ItIsEnforcedHere,
                "a marker file in the real /tmp: visible bare, gone hardened",
            ),
        ),
        (
            "ProtectControlGroups",
            (
                ItIsEnforcedHere,
                "/sys/fs/cgroup mounted rw bare, ro hardened",
            ),
        ),
        (
            "ProtectKernelTunables",
            (
                ItIsEnforcedHere,
                "/proc/sys not its own mount bare, a ro mount hardened",
            ),
        ),
        (
            "ProtectKernelModules",
            (
                ItIsEnforcedHere,
                "/usr/lib/modules listable bare, EACCES hardened",
            ),
        ),
        (
            "RestrictSUIDSGID",
            (
                ItIsEnforcedHere,
                "chmod u+s on our own file: allowed bare, EPERM hardened",
            ),
        ),
        (
            "RestrictRealtime",
            (
                NobodyCanTellOnThisBox,
                "sched_setscheduler(SCHED_FIFO) is EPERM both ways — this user's RLIMIT_RTPRIO is \
                 already (0, 0), so the call cannot succeed with or without the directive",
            ),
        ),
        (
            "LockPersonality",
            (
                ItIsEnforcedHere,
                "personality(PER_LINUX32): EINVAL bare, EPERM hardened",
            ),
        ),
        (
            "MemoryDenyWriteExecute",
            (
                ItIsEnforcedHere,
                "mmap PROT_WRITE|PROT_EXEC: mapped bare, EACCES hardened",
            ),
        ),
        (
            "RestrictAddressFamilies",
            (
                ItIsEnforcedHere,
                "socket(AF_NETLINK): allowed bare, EAFNOSUPPORT hardened, while the three families \
                 the units name stayed open both ways",
            ),
        ),
        (
            "IPAddressDeny",
            (
                ItDoesNothingHere,
                "curl to 1.1.1.1 answered 301 under the filter and 301 without it",
            ),
        ),
        (
            "IPAddressAllow",
            (
                ItDoesNothingHere,
                "the allow list is the same BPF program as the deny list and shares its fate",
            ),
        ),
        (
            "SystemCallArchitectures",
            (
                NobodyCanTellOnThisBox,
                "a foreign-architecture call needs a foreign-architecture ABI to make it; the x32 \
                 ABI answers ENOSYS on this kernel bare and hardened alike",
            ),
        ),
        (
            "SystemCallFilter",
            (
                ItIsEnforcedHere,
                "adjtimex() in its read-only mode — outside @system-service, and a call an ordinary \
                 user may make — returned 0 bare and EPERM hardened",
            ),
        ),
        (
            "SystemCallErrorNumber",
            (
                ItIsEnforcedHere,
                "the errno the filter above answered with was EPERM, which is what the line asks for",
            ),
        ),
        (
            "ProtectSystem",
            (
                ItIsEnforcedHere,
                "/usr not its own mount bare, a ro mount hardened",
            ),
        ),
    ])
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "this guard reads {} and it is not there: {e}. A unit that moved without this list \
             moving with it is a unit nobody is holding to its own comments.",
            path.display()
        )
    })
}

/// The lines of a unit's hardening block: from its `Hardening` heading to the end of `[Service]`.
/// `None` where the unit has no such heading at all.
///
/// Scoped this way on purpose. `ExecStart=`, `Restart=` and the rest are not claims about what the
/// kernel will refuse, and holding them to this table would make the guard about the wrong thing.
fn the_hardening_block(unit: &str) -> Option<Vec<(usize, String)>> {
    let text = read(unit);
    let mut out = Vec::new();
    let mut inside = false;
    let mut found_the_heading = false;
    for (i, line) in text.lines().enumerate() {
        if line.starts_with('[') {
            inside = false; // `[Install]` ends it, whatever the block said
            continue;
        }
        if line.contains("Hardening") && line.trim_start().starts_with('#') {
            inside = true;
            found_the_heading = true;
            continue;
        }
        if inside {
            out.push((i + 1, line.to_owned()));
        }
    }
    if !found_the_heading {
        return None;
    }
    assert!(
        !out.is_empty(),
        "{unit} has a `Hardening` heading with nothing under it, so this guard would hold none of \
         its directives to anything. Either the block moved or the unit stopped hardening itself; \
         both are worth stopping for."
    );
    Some(out)
}

/// Every line of a unit — inside its hardening block or anywhere else — that sets a directive this
/// table has measured.
///
/// Read over the whole file rather than the block, because the block is where a reader looks for
/// these lines and the guard has to be able to say when one is not there. A sandbox directive
/// above the heading is a line nobody reading the block would count, and moving one up there is
/// the cheapest way to take it out from under every test below.
fn directives_this_table_knows_anywhere_in(unit: &str) -> Vec<(usize, String)> {
    let measured = what_the_probe_found();
    read(unit)
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let t = line.trim();
            if t.starts_with('#') || t.is_empty() {
                return None;
            }
            let (key, _) = t.split_once('=')?;
            let key = key.trim();
            (measured.contains_key(key) || looks_like_a_sandbox_directive(key))
                .then(|| (i + 1, key.to_owned()))
        })
        .collect()
}

/// Whether a directive NAME is one of systemd's sandboxing family, by its shape alone.
///
/// The table this file holds units to is a table of things somebody MEASURED, so recognising a
/// unit only by what is already in it made the guard blind exactly where it mattered: a unit that
/// ships nothing but never-measured directives, under no heading, is held to nothing. That was not
/// a theoretical gap — a unit planted with `ProtectHome=` and `ProtectHostname=` under two comments
/// crediting them with guaranteeing things passed all five tests. The defence written down at the
/// time, that such a unit "makes no claim a reader could check", is false: the claims are in its
/// comments, which is where every claim this file is about lives.
///
/// So the shape drags the unit in, and the measurement decides what may be said about it. A prefix
/// match rather than a list, because the point is to catch the directive nobody here has heard of
/// yet — a new `Protect…` in a future systemd is precisely the line that would otherwise arrive
/// unmeasured, uncommented and unheld.
fn looks_like_a_sandbox_directive(key: &str) -> bool {
    const FAMILIES: [&str; 9] = [
        "Protect",
        "Private",
        "Restrict",
        "MemoryDeny",
        "SystemCall",
        "IPAddress",
        "Lock",
        "Capability",
        "Device",
    ];
    key == "NoNewPrivileges" || FAMILIES.iter().any(|f| key.starts_with(f))
}

/// The directive names set in a block, in the order they appear, with their line numbers.
fn directives_in(block: &[(usize, String)]) -> Vec<(usize, String)> {
    block
        .iter()
        .filter_map(|(n, line)| {
            let t = line.trim();
            if t.starts_with('#') || t.is_empty() {
                return None;
            }
            t.split_once('=')
                .map(|(key, _)| (*n, key.trim().to_owned()))
        })
        .collect()
}

/// **The guard that outlives this round.** A directive nobody has measured is a directive whose
/// comment nobody can check, and adding one is exactly how the inert line got here in the first
/// place: it reads well, `systemctl show` agrees with it, and nothing says otherwise.
#[test]
fn every_hardening_line_a_unit_ships_has_been_measured_on_the_manager_that_runs_it() {
    let measured = what_the_probe_found();
    for unit in units_that_sandbox_themselves() {
        let unit = unit.as_str();
        let Some(block) = the_hardening_block(unit) else {
            continue; // the guard below owns the missing heading
        };
        for (line_no, name) in directives_in(&block) {
            assert!(
                measured.contains_key(name.as_str()),
                "{unit}:{line_no} ships `{name}=` and nobody has measured it. Run it twice under \
                 `systemd-run --user` — once bare, once under this unit's own directives — and add \
                 what the two runs said to this guard's table. A directive that behaves the same \
                 both ways is not hardening; it is a sentence."
            );
        }
    }
}

/// **The finding itself.** A line that does nothing may stay — it is real under a system manager,
/// and deleting it would delete the measurement with it — but the unit has to say so where it is
/// read, because `systemctl show` and `systemctl status` will both say the opposite.
#[test]
fn a_hardening_line_that_does_nothing_here_says_so_where_it_is_written() {
    let measured = what_the_probe_found();
    for unit in units_that_sandbox_themselves() {
        let unit = unit.as_str();
        let Some(block) = the_hardening_block(unit) else {
            continue;
        };
        let prose: String = block
            .iter()
            .map(|(_, l)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        for (line_no, name) in directives_in(&block) {
            let Some((verdict, _why)) = measured.get(name.as_str()) else {
                continue; // the test above is the one that owns this
            };
            if *verdict != ItDoesNothingHere {
                continue;
            }
            assert!(
                prose.contains("systemctl --user"),
                "{unit}:{line_no} ships `{name}=`, which does nothing under a user manager, and \
                 the block never names `systemctl --user` — the one install path this repo \
                 documents, and the one where the line is inert. A reader has no way to find that \
                 out from the unit, `systemctl show` or `systemctl status`."
            );
            assert!(
                ["do nothing", "does nothing", "inert"]
                    .iter()
                    .any(|s| prose.contains(s)),
                "{unit}:{line_no} ships `{name}=` and the block never says it does nothing here. \
                 The next person to read this unit will count it as a control that is in place."
            );
        }
    }
}

/// The other half, and the shape the defect actually took: not a missing disclaimer but a present
/// claim. The comment did not merely omit that the filter was inert — it said the filter was what
/// made the promise true, which is a stronger statement than the unit could make even if the line
/// worked.
#[test]
fn no_comment_credits_an_inert_hardening_line_with_doing_something() {
    let measured = what_the_probe_found();
    // Verbs that assert a line ACTS. A line that does nothing may be described, kept and explained;
    // it may not be given work.
    const CREDIT: &[&str] = &["makes", "make ", "enforc", "guarantee", "ensures"];
    for unit in units_that_sandbox_themselves() {
        let unit = unit.as_str();
        let Some(block) = the_hardening_block(unit) else {
            continue;
        };
        for (line_no, line) in block {
            let t = line.trim();
            if !t.starts_with('#') {
                continue;
            }
            let inert_named: Vec<&str> = measured
                .iter()
                .filter(|(_, (v, _))| *v == ItDoesNothingHere)
                .map(|(name, _)| *name)
                .filter(|name| t.contains(name))
                .collect();
            if inert_named.is_empty() {
                continue;
            }
            for verb in CREDIT {
                assert!(
                    !t.to_lowercase().contains(verb),
                    "{unit}:{line_no} names {inert_named:?}, which does nothing on the manager \
                     that runs this unit, and credits it with something in the same sentence \
                     (\"{verb}\"):\n    {t}\nSay what the line is for and what it does not do. \
                     Anything stronger is a promise the kernel is not keeping."
                );
            }
        }
    }
}

/// **What makes finding the units sound rather than merely broader.**
///
/// Every test above reads the block under a unit's `Hardening` heading. That is the right scope —
/// `ExecStart=` and `Restart=` are not claims about what the kernel will refuse — but it leaves
/// two ways for a sandbox line to be shipped that no test holds to anything: a unit with no
/// heading at all, and a line moved out from under the heading it belongs to. Both read, in the
/// unit and in `systemctl show`, exactly like a control that is in force.
///
/// So a unit that sandboxes itself must say where: one heading, and every such line under it.
#[test]
fn a_unit_that_sandboxes_itself_keeps_every_such_line_under_its_own_hardening_heading() {
    for unit in units_that_sandbox_themselves() {
        let unit = unit.as_str();
        let ships = directives_this_table_knows_anywhere_in(unit);
        let Some(block) = the_hardening_block(unit) else {
            let (line_no, name) = ships
                .first()
                .cloned()
                .unwrap_or((0, String::from("(none)")));
            panic!(
                "{unit}:{line_no} ships `{name}=` and the unit has no `Hardening` heading, so not \
                 one of its sandbox lines is held to what was measured of it. Add the heading \
                 above them — it is what every test in this file reads, and what tells the next \
                 reader that the lines below it have been run rather than believed."
            );
        };
        let under_the_heading: Vec<usize> = block.iter().map(|(n, _)| *n).collect();
        for (line_no, name) in ships {
            assert!(
                under_the_heading.contains(&line_no),
                "{unit}:{line_no} sets `{name}=` outside the unit's own `Hardening` block, where \
                 no guard in this file reads it and no reader of the block would count it. Move it \
                 under the heading."
            );
        }
    }
}

/// And the unit for the one program in this workspace that listens is held like the rest.
///
/// Named here, once, rather than in a list the guards iterate: the list was what let this unit
/// keep three claims the same round had deleted from its two siblings. This asserts that the walk
/// reaches it — if `deploy/` is reorganised, the reorganisation has to say so here.
#[test]
fn the_door_is_one_of_the_units_this_guard_holds() {
    let held = units_that_sandbox_themselves();
    assert!(
        held.iter().any(|u| u == "deploy/kickoff-door.service"),
        "the walk of deploy/ did not reach the door's own unit; it found {held:?}"
    );
}

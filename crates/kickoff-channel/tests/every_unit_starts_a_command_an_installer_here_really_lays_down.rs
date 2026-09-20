//! **A unit file and the script that installs it are one change, and nothing but this says so.**
//!
//! Renaming a unit is four edits that a reader can do three of. The product name moved from
//! `herdr-tg` to `kickoff-channel` and the five units under `deploy/` moved with it; the two things
//! that could have been left behind are the only two that cost anything:
//!
//! 1. **A unit that starts a binary no installer writes.** `ExecStart=%h/.local/bin/<name>` is not
//!    checked by anything at install time. With `Restart=always` and `RestartSec=5` under it, the
//!    unit does not fail loudly — it fails every five seconds, for ever, on a box the operator only
//!    reaches from his phone. No crate imports a unit file and no gate runs systemd, so nothing else
//!    in this workspace would go red.
//! 2. **Half a `Conflicts=` pair.** Both hubs bind the same socket and take the same single-hub
//!    lock, and the whole of what keeps one plane running is that each unit names the other. Rename
//!    one side and systemd will happily run a plane of each: the second to start is refused in
//!    milliseconds and leaves a failed unit behind, with every agent on the box talking to whichever
//!    one won.
//!
//! # The limits, written down rather than implied
//!
//! What "an installer lays it down" means here is narrow on purpose: some file under `scripts/`
//! that writes into `~/.local/bin` has a line, naming a bin directory, whose last path segment
//! is exactly this command. It does not prove the script really REACHES that line, which is a
//! thing only running it proves — and both installers do prove their own, by starting what they
//! installed. What this closes is the gap with no other witness at all: a name in a unit that
//! appears nowhere in any installer, which is exactly what a rename leaves behind.
//!
//! Units are found by walking `deploy/`, never by a list. A guard with a list of files silently
//! stops covering the next file somebody adds and goes on passing while it does; this repo has
//! already been burnt by that exact shape twice, and by its cousin — a filter that matched nothing
//! reporting green — once. So both walks assert a floor.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kickoff-channel sits two levels below the workspace root")
        .to_path_buf()
}

/// Every unit file this repo ships, FOUND rather than listed: `deploy/<name>` -> its text.
fn units() -> BTreeMap<String, String> {
    let dir = workspace_root().join("deploy");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!(
            "this guard walks {} and cannot: {e}. A guard that cannot read the units it holds is a \
             guard that holds nothing.",
            dir.display()
        )
    }) {
        let path = entry.expect("a readable entry in deploy/").path();
        let is_unit = path
            .extension()
            .is_some_and(|x| x == "service" || x == "timer");
        if !is_unit {
            continue;
        }
        let name = path
            .file_name()
            .expect("a file name")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} is not readable: {e}", path.display()));
        out.insert(name, text);
    }
    assert!(
        out.len() >= 6,
        "the walk of deploy/ found {} unit files, and this repo ships more than that. A walk \
         looking in the wrong place is a guard that passes everything.",
        out.len()
    );
    out
}

/// Everything before the first `#` on a line, which for a unit file is its directives.
///
/// An approximation, and a safe one in this direction: it can only ever read LESS of a file than
/// there is, so it cannot invent a directive. These units discuss their own `Conflicts=` and
/// `ExecStart=` at length in prose, and reading those paragraphs as directives would have the guard
/// fail on the comment that explains why it passes.
fn directives(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(|line| match line.find('#') {
        Some(at) => &line[..at],
        None => line,
    })
}

/// The command names a unit starts out of the operator's own bin directory.
///
/// Read from anywhere in an `ExecStart`/`ExecStartPre` line, not just the head of it: the worker's
/// unit wraps its command in `/bin/sh -c 'exec %h/.local/bin/kickoff-hub-attach …'`, and a guard
/// that only understood the simple shape would silently stop covering it.
fn commands_started_from_the_home_bin(text: &str) -> Vec<String> {
    const MARKER: &str = "%h/.local/bin/";
    let mut out = Vec::new();
    for line in directives(text).filter(|l| l.trim_start().starts_with("ExecStart")) {
        let mut rest = line;
        while let Some(at) = rest.find(MARKER) {
            rest = &rest[at + MARKER.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
                .unwrap_or(rest.len());
            if end > 0 {
                out.push(rest[..end].to_owned());
            }
            rest = &rest[end..];
        }
    }
    out
}

/// Every installer in this repo: `scripts/<name>` -> its text, keeping only the ones that write
/// into the operator's own bin directory at all.
fn installers() -> BTreeMap<String, String> {
    let dir = workspace_root().join("scripts");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("scripts/ is readable") {
        let path = entry.expect("a readable entry in scripts/").path();
        if !path.extension().is_some_and(|x| x == "sh") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !text.contains(".local/bin") {
            continue;
        }
        out.insert(
            path.file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned(),
            text,
        );
    }
    assert!(
        !out.is_empty(),
        "no script under scripts/ writes into ~/.local/bin, so this guard has nothing to hold the \
         units against and would pass whatever they said"
    );
    out
}

/// Does this script write a file whose last path segment is exactly `command`?
///
/// Three conditions, and each one is a false pass this guard had before it was attacked.
///
/// * **Preceded by `/`** — a bare mention is not an install. `scripts/install-watchdog.sh` lists
///   `herdr-tg-watchdog.timer` among the units it RETIRES, and a guard that read a mention as a
///   destination let a unit go on starting a binary the rename had taken away.
/// * **Ends the word** — the next character may not continue the name. `herdr-tg-watchdog.sh` is a
///   source file in `deploy/`, not a command in his bin directory, and `kickoff-channel` is a
///   prefix of `kickoff-channel-watchdog`, which is exactly the pair this rename could confuse.
/// * **On a line that says `bin`** — every install destination in this repo is written through a
///   variable whose name carries it (`$BIN_DIR`, `$BIN_DST`) or through the literal path.
fn installs_the_command(script: &str, command: &str) -> bool {
    script.lines().any(|line| {
        if !line.to_ascii_lowercase().contains("bin") {
            return false;
        }
        let mut from = 0;
        while let Some(at) = line[from..].find(command) {
            let start = from + at;
            let end = start + command.len();
            let preceded_by_a_slash = start > 0 && line.as_bytes()[start - 1] == b'/';
            let word_ends_here = line[end..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'));
            if preceded_by_a_slash && word_ends_here {
                return true;
            }
            from = start + 1;
        }
        false
    })
}

/// **Every command a unit starts is one an installer in this repo writes.**
#[test]
fn every_command_a_unit_starts_from_his_own_bin_is_one_an_installer_here_writes() {
    let installers = installers();
    let mut checked = 0;
    for (unit, text) in units() {
        for command in commands_started_from_the_home_bin(&text) {
            checked += 1;
            let written_by: Vec<&String> = installers
                .iter()
                .filter(|(_, script)| installs_the_command(script, &command))
                .map(|(name, _)| name)
                .collect();
            assert!(
                !written_by.is_empty(),
                "deploy/{unit} starts `{command}` out of his own bin directory, and no script under \
                 scripts/ puts a `{command}` there. Under Restart=always that is not a unit that \
                 fails loudly — it is one that fails every five seconds for ever, on a box he only \
                 reaches from his phone. Either the unit or the installer was renamed alone."
            );
        }
    }
    assert!(
        checked >= 4,
        "this guard found {checked} commands started out of his own bin directory, and this repo \
         ships more than that. A walk that finds nothing passes everything."
    );
}

/// **A unit this repo ships never names a sibling that does not exist.**
///
/// `systemd` does not refuse a dependency on a unit nobody installed: it records it and carries on,
/// so a half-renamed `Conflicts=`, `OnFailure=` or `Requires=` is silent until the day it was
/// supposed to do something.
#[test]
fn a_unit_never_names_a_sibling_of_ours_that_this_repo_does_not_ship() {
    let units = units();
    const RELATIONS: [&str; 6] = [
        "Conflicts=",
        "OnFailure=",
        "Requires=",
        "BindsTo=",
        "PartOf=",
        "Wants=",
    ];
    let mut checked = 0;
    for (unit, text) in &units {
        for line in directives(text) {
            let line = line.trim();
            let Some(relation) = RELATIONS.iter().find(|r| line.starts_with(**r)) else {
                continue;
            };
            for named in line[relation.len()..].split_whitespace() {
                // `.target` names belong to systemd, not to this repo. Anything else named here is
                // a unit somebody has to ship, and the only one we can hold to that is ourselves.
                if named.ends_with(".target") {
                    continue;
                }
                checked += 1;
                assert!(
                    units.contains_key(named),
                    "deploy/{unit} says `{relation}{named}` and this repo ships no deploy/{named}. \
                     systemd records a dependency on a unit nobody installed and says nothing, so \
                     this is silent until the moment it was meant to act."
                );
            }
        }
    }
    assert!(
        checked >= 3,
        "this guard read {checked} relations between units, and this repo ships more than that"
    );
}

/// **A `Conflicts=` between two of our own units is mutual.**
///
/// One-sided is the dangerous half. Systemd does treat `Conflicts=` as mutual once both units are
/// LOADED, but which units are loaded is not a thing a reader of one file can know — and the
/// failure this pins is the rename, where one plane keeps the old name in its `Conflicts=` and the
/// pair stops describing the two units that actually exist. Both hubs bind the same socket and take
/// the same lock, so the cost of getting it wrong is a failed unit and a box whose agents are
/// talking to whichever plane won.
#[test]
fn a_conflict_between_two_units_of_ours_is_written_on_both_of_them() {
    let units = units();
    let mut pairs = 0;
    for (unit, text) in &units {
        for line in directives(text) {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("Conflicts=") else {
                continue;
            };
            for named in rest.split_whitespace() {
                let Some(other) = units.get(named) else {
                    continue; // the guard above owns "names a sibling we do not ship".
                };
                pairs += 1;
                let names_us_back = directives(other)
                    .filter_map(|l| l.trim().strip_prefix("Conflicts="))
                    .any(|rest| rest.split_whitespace().any(|n| n == unit));
                assert!(
                    names_us_back,
                    "deploy/{unit} conflicts with {named}, and deploy/{named} does not say so back. \
                     Each plane naming the other is the whole of what stops two hubs fighting over \
                     one socket, and a rename that moves one side leaves the pair describing units \
                     that no longer both exist."
                );
            }
        }
    }
    assert!(
        pairs >= 2,
        "this guard found {pairs} conflicts between units of ours, and the two hub planes alone are \
         two. A walk that finds nothing passes everything."
    );
}

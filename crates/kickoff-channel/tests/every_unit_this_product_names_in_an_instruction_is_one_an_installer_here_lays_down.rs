//! **An instruction this product prints is a thing he can actually run on the box he is standing at.**
//!
//! The defect this holds shut, in full. `lock.rs` answered the second copy of the hub with
//! `systemctl --user stop herdr-tg`, and `scripts/install-service.sh` — in the same change —
//! RETIRES `herdr-tg.service`: it disables the unit and deletes the file. So on any box installed
//! since, the one remedy the product prints for what `bot.rs` calls the likeliest way this box
//! loses its taps named a unit that is not loaded. `systemctl` answers "Unit herdr-tg.service not
//! loaded", the copy holding the lock is still holding it, and the operator is at a keyboard with
//! nothing left to try.
//!
//! # Why "this repo ships it" was the wrong property
//!
//! The first version of this guard held printed unit names against a walk of `deploy/`, and that
//! walk answers a question nobody asked. A unit file sitting in this repo has never once been the
//! thing that makes `systemctl --user restart <name>` work; what makes it work is that something
//! put the file in **his** unit directory. So the property is the stronger one: **every unit this
//! product tells him to act on is one some installer in this repo really writes into his unit
//! directory.** `lock.rs` names both hub planes, rightly — the copy holding the lock may be either
//! and the lock file carries a pid and nothing else — but `kickoff-channel-app.service` was laid
//! down by no script here at all, its only witness a hand-typed line in a runbook, which is another
//! way of saying nobody. That passed the `deploy/` walk and fails this one.
//!
//! # One sentence of the same family that this guard does NOT catch, said plainly
//!
//! `scripts/install-channel-plugin.sh` also printed a hub unit to restart. That script installs the
//! bridge and nothing else — no binary, no unit — so the name it printed was whatever this repo
//! happened to call the hub that week, and on a box that had only ever been given the former name
//! it answered "Unit kickoff-channel.service not found".
//!
//! **Nothing here can catch that, and no statically decidable rule would.** The name it printed is
//! a name `scripts/install-service.sh` really does lay down, so this guard passes it and is right
//! to: the same sentence printed by `install-service.sh` itself is perfectly good. What made it
//! wrong was which script was speaking — a script that installs no hub cannot know what the hub on
//! this box is called — and "may this file name that unit" is a judgement about intent, not a fact
//! about the repo. A guard that tried it would either have to hard-code which script may name which
//! unit, which is the list-shaped thing this repo keeps getting burnt by, or forbid every installer
//! from naming a unit, which would delete four good sentences to catch one bad one.
//!
//! So that one was fixed by WORDING rather than by a guard: the plugin installer now names the act
//! and not the unit, and prints the listing that shows him whatever his own box has. The honest
//! summary of this file is therefore: it proves a printed unit is one this repo ships and one an
//! installer here writes, and it proves nothing about whether the script doing the printing is
//! entitled to name it.
//!
//! # What counts as "prints", and the limits written down rather than implied
//!
//! Two commands are read, `systemctl --user` and `journalctl --user -u`, because both take a unit
//! name and neither says anything useful about one that does not exist: `systemctl` answers that
//! the unit is not loaded, and `journalctl` prints an empty log and exits zero, which reads exactly
//! like a hub that has been silent.
//!
//! * **Rust**: every line of every `crates/*/src/**.rs` that is not a `//` comment, so the strings
//!   the binary puts in front of him are all held, wherever in the workspace they live.
//! * **Shell**: only the `say`/`echo` lines of `scripts/*.sh` and `deploy/*.sh` — the hints an
//!   installer leaves behind when it is done. A `systemctl` a script RUNS is proved by running it:
//!   under `set -e` it fails loudly, at install time, in front of the person who ran it. A hint it
//!   PRINTS is proved by nothing at all, which is why those are the ones held here.
//! * **A name assembled at runtime is not checked.** `systemctl --user restart "$UNIT"` is text
//!   whose meaning only the shell knows, so the walk steps over it rather than guessing. That is
//!   also the honest escape for a script that cannot know the name — printing no name at all, or
//!   printing one it discovered on the box, is always true and this guard has nothing to say
//!   about it.
//! * **One line at a time.** An instruction whose unit name wraps onto the next source line reads
//!   here as an instruction naming no unit, and passes. Keep the command whole on its line; the
//!   alternative is a guard that reassembles other people's string literals, which is a bigger
//!   thing to get wrong than the defect it would catch.
//!
//! # What "an installer lays it down" means, and what it does NOT prove
//!
//! Narrow, on purpose, and in the same terms as its sibling guard
//! `every_unit_starts_a_command_an_installer_here_really_lays_down`: some file under `scripts/`
//! has a line writing this unit's file name into the unit directory — the name immediately after
//! `UNIT_DIR/` or `systemd/user/`, which is the only spelling an install destination has in this
//! repo. A path under `deploy/` is the SOURCE and is not counted; a bare mention is not counted
//! either, because `install-watchdog.sh` lists the units it RETIRES by name and reading a
//! retirement as an install is how a unit nobody installs passes.
//!
//! **It does not prove the script reaches that line**, which only running it proves, and it does
//! not prove the operator ever ran the script. Those have other witnesses: both installers prove
//! their own by starting what they installed, and the runbook says which to run. What this closes
//! is the gap with no witness at all — a unit name printed at the worst possible moment that
//! nothing in this repo has ever put on a box.
//!
//! A future installer that writes its destination some other way fails this guard rather than
//! slipping past it, which is the direction to fail in: the fix is one literal path.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kickoff-channel sits two levels below the workspace root")
        .to_path_buf()
}

/// Every unit this repo ships, FOUND by walking `deploy/` rather than listed.
///
/// A guard with a list of names silently stops covering the next unit somebody adds and goes on
/// passing while it does. This repo has been burnt by that shape before, so the walk asserts a
/// floor instead.
fn units_we_ship() -> BTreeSet<String> {
    let dir = workspace_root().join("deploy");
    let mut out = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!(
            "this guard walks {} and cannot: {e}. A guard that cannot read the units it holds is a \
             guard that holds nothing.",
            dir.display()
        )
    }) {
        let path = entry.expect("a readable entry in deploy/").path();
        if path
            .extension()
            .is_some_and(|x| x == "service" || x == "timer")
        {
            out.insert(
                path.file_name()
                    .expect("a file name")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    assert!(
        out.len() >= 6,
        "the walk of deploy/ found {} unit files, and this repo ships more than that. A walk \
         looking in the wrong place is a guard that passes everything.",
        out.len()
    );
    out
}

/// Every installer in this repo: `scripts/<name>` -> its text.
///
/// Found rather than listed, for the same reason as the units.
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
        out.insert(
            path.file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned(),
            text,
        );
    }
    assert!(
        out.len() >= 10,
        "the walk of scripts/ found {} shell scripts, and this repo ships more than that. A walk \
         looking in the wrong place is a guard that passes everything.",
        out.len()
    );
    out
}

/// The spellings of one unit name an operator or an installer may write.
///
/// He types the name without its suffix as often as with it and `systemctl` accepts both, so a
/// guard that demanded one spelling would fail perfectly good instructions. A template instance
/// (`kickoff-hub-attach@oc-dogfood`) is answered for by the template file that ships and installs
/// it, so the instance is dropped and the `@` kept.
fn spellings(named: &str) -> Vec<String> {
    let stem = match named.split_once('@') {
        Some((prefix, _instance)) => format!("{prefix}@"),
        None => named.to_owned(),
    };
    ["", ".service", ".timer"]
        .iter()
        .map(|suffix| format!("{stem}{suffix}"))
        .collect()
}

/// Does this repo ship a unit file by this name?
fn ships(named: &str, units: &BTreeSet<String>) -> bool {
    spellings(named).iter().any(|name| units.contains(name))
}

/// Does this script write a file of this name into his unit directory?
///
/// The destination must be spelled out. Two markers are accepted because two are used here —
/// `"$UNIT_DIR/kickoff-channel.service"` and the literal `~/.config/systemd/user/…` — and nothing
/// else counts. Each exclusion below is a false pass this guard had before it was attacked:
///
/// * **A source under `deploy/` is not an install.** `UNIT_SRC="$REPO/deploy/kickoff-channel.service"`
///   names the file this repo ships, which is the weaker property this guard exists to stop
///   standing in for the real one.
/// * **A bare mention is not an install.** `scripts/install-watchdog.sh` loops over
///   `herdr-tg-watchdog.timer` and its siblings to RETIRE them — disable the unit and delete the
///   file — and reading that as an install would credit a script for putting down the very unit it
///   takes away.
/// * **The word must end.** `kickoff-channel` is a prefix of both `kickoff-channel-app` and
///   `kickoff-channel-watchdog`, which is exactly the pair this rename could confuse.
fn lays_the_unit_down(script: &str, named: &str) -> bool {
    const MARKERS: [&str; 2] = ["UNIT_DIR/", "systemd/user/"];
    let wanted = spellings(named);
    script.lines().any(|line| {
        for marker in MARKERS {
            let mut from = 0;
            while let Some(at) = line[from..].find(marker) {
                let start = from + at + marker.len();
                let rest = &line[start..];
                if wanted.iter().any(|name| {
                    rest.strip_prefix(name.as_str()).is_some_and(|after| {
                        after.chars().next().is_none_or(|c| {
                            !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                        })
                    })
                }) {
                    return true;
                }
                from = start;
            }
        }
        false
    })
}

/// Every instruction on one line that names a unit, with whether a verb stands between the
/// command and the unit.
///
/// `journalctl --user -u <unit>` is here for the same reason as `systemctl`: a unit name that no
/// longer exists does not fail loudly there either. It prints nothing and exits zero, so the
/// operator reading the logs of a renamed unit is told, in effect, that his hub has been silent.
fn instructions(line: &str) -> Vec<(&str, bool)> {
    const MARKERS: [(&str, bool); 2] = [
        ("systemctl --user", true),
        // `-u` is part of the marker: what follows it is the unit, with no verb in between.
        ("journalctl --user -u", false),
    ];
    let mut out = Vec::new();
    for (marker, verb_first) in MARKERS {
        let mut rest = line;
        while let Some(at) = rest.find(marker) {
            rest = &rest[at + marker.len()..];
            out.push((rest, verb_first));
        }
    }
    out
}

/// The unit one instruction names, which is often none — `daemon-reload` names no unit at all.
///
/// The first word that is not an option is the verb, and the first unit-shaped word after it is
/// the unit. **One per instruction, and then the scan stops**: a printed command is followed by
/// the prose that explains it, and a scan that ran on would read "(through the app)" as three more
/// unit names and fail a sentence that is perfectly right. The cost is the limit that a second
/// unit on the same command is not checked, which no line in this repo has.
///
/// It also stops at a shell operator, because what follows `&&` is another command entirely.
fn unit_named(instruction: &str, verb_first: bool) -> Option<String> {
    let mut seen_the_verb = !verb_first;
    for word in instruction.split_whitespace() {
        if matches!(word, "&&" | "||" | "|" | ";" | "\\" | "#") || word.starts_with('>') {
            return None;
        }
        // Options, and the values systemd's own option spellings carry (`--lines=20`).
        if word.starts_with('-') {
            continue;
        }
        let word = word.trim_matches(|c| matches!(c, '"' | '\'' | '`' | ',' | ';' | '.' | ')'));
        if word.is_empty() {
            continue;
        }
        if !seen_the_verb {
            seen_the_verb = true;
            continue;
        }
        // Whatever the shell or a formatter fills in later is not text this guard can read.
        if word.contains('$') || word.contains('{') || word.contains('/') || word.contains('*') {
            return None;
        }
        let name = word.split_once('@').map_or(word, |(prefix, _)| prefix);
        let looks_like_a_unit = !name.is_empty()
            && name.starts_with(|c: char| c.is_ascii_alphanumeric())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if looks_like_a_unit {
            return Some(word.to_owned());
        }
    }
    None
}

/// Every file this product prints out of, with whether it is shell.
fn everything_that_prints() -> Vec<(PathBuf, String, bool)> {
    let root = workspace_root();
    let mut paths: Vec<(PathBuf, bool)> = Vec::new();

    // The binary's own strings: every crate's source, found rather than named.
    for entry in std::fs::read_dir(root.join("crates")).expect("crates/ is readable") {
        let src = entry
            .expect("a readable entry in crates/")
            .path()
            .join("src");
        if src.is_dir() {
            let mut found = Vec::new();
            rust_sources(&src, &mut found);
            paths.extend(found.into_iter().map(|p| (p, false)));
        }
    }
    // The installers' parting hints.
    for dir in ["scripts", "deploy"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("scripts/ and deploy/ are readable") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_some_and(|x| x == "sh") {
                paths.push((path, true));
            }
        }
    }

    paths
        .into_iter()
        .filter_map(|(path, shell)| {
            let text = std::fs::read_to_string(&path).ok()?;
            Some((path, text, shell))
        })
        .collect()
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a readable source directory") {
        let path = entry.expect("a readable entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

/// Is this line one the operator ever reads?
fn he_reads_it(line: &str, shell: bool) -> bool {
    let trimmed = line.trim_start();
    if shell {
        // A hint, not a command: the commands a script runs prove themselves by running.
        trimmed.starts_with("say ") || trimmed.starts_with("echo ")
    } else {
        // `///` is NOT a comment for this purpose: on a clap type it is the help text, which is
        // among the most-read things this product says. Blanking every line starting with `//`
        // swallowed it, and a sibling guard was caught making the same mistake in the same week —
        // its report claimed to hold the printed help while a planted name in a `///` stayed green.
        // An ordinary `//` really is invisible to him and several of them name the old verb on
        // purpose, so only the doc form is read.
        trimmed.starts_with("///") || !trimmed.starts_with("//")
    }
}

#[test]
fn every_unit_this_product_tells_him_to_act_on_is_one_an_installer_here_lays_down() {
    let units = units_we_ship();
    let installers = installers();
    let mut files = 0;
    let mut checked = 0;
    let mut distinct = BTreeSet::new();

    for (path, text, shell) in everything_that_prints() {
        files += 1;
        let shown = path
            .strip_prefix(workspace_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        for (index, line) in text.lines().enumerate() {
            if !he_reads_it(line, shell) {
                continue;
            }
            for (instruction, verb_first) in instructions(line) {
                let Some(named) = unit_named(instruction, verb_first) else {
                    continue;
                };
                let at = index + 1;
                checked += 1;
                distinct.insert(named.clone());
                assert!(
                    ships(&named, &units),
                    "{shown}:{at} tells him to act on `{named}`, and this repo ships no such unit \
                     file under deploy/. Either the unit was renamed and this sentence was left \
                     behind, or it names a unit that never existed."
                );
                let laid_down_by: Vec<&String> = installers
                    .iter()
                    .filter(|(_, script)| lays_the_unit_down(script, &named))
                    .map(|(name, _)| name)
                    .collect();
                assert!(
                    !laid_down_by.is_empty(),
                    "{shown}:{at} tells him to act on `{named}`, and no script under scripts/ ever \
                     puts a `{named}` in his unit directory. This repo shipping the file is not \
                     what makes his box have it. Nothing he does next says so usefully: systemctl \
                     answers that the unit is not loaded, journalctl prints an empty log and exits \
                     zero — and whatever the instruction was for is still wrong, with nothing else \
                     printed for him to try. Either an installer should lay this unit down, or the \
                     sentence should stop naming a unit it cannot vouch for."
                );
            }
        }
    }

    assert!(
        files >= 20,
        "this guard read {files} files of source and script, and this workspace has more than \
         that. A walk looking in the wrong place passes everything."
    );
    assert!(
        checked >= 8,
        "this guard checked {checked} unit names, and this repo prints more than that — the hub's \
         own refusal and the installers' parting hints alone. A filter that matches nothing is a \
         pass, which is the shape this repo has already been burnt by."
    );
    assert!(
        distinct.len() >= 3,
        "this guard saw {} distinct unit names printed, and this product prints more than that: \
         both hub planes, the watchdog and a worker. A walk that keeps finding the same sentence \
         is a walk that has stopped reading the others.",
        distinct.len()
    );
}

//! **A tick printed for something that was not run is the shape this repo has already been burnt
//! by.**
//!
//! `scripts/install-watchdog.sh` prints, in one line, that any of the three legs going quiet alarms
//! and that the alarm names which — *on either plane*. It proved that for the phone plane three
//! ways: a dead phone line, a door that let nothing through, an update line that stopped carrying
//! his taps. For the app plane it staged one note — ring fine, door fine, answers sweep refused —
//! and checked the sweep alarm. The ring leg and the door leg on the app plane were never driven at
//! all, and the line said they were.
//!
//! This repo has met that shape before and wrote it down: a `cargo test` filter that matched
//! nothing was a pass, and the step it sat in decided whether a bridge was installed. An operator
//! was told his plugin was unsafe when his plugin was fine. A gate that reports on what it did not
//! run is worse than no gate, because it is believed.
//!
//! # What this holds
//!
//! The installer stages notes by calling its own `stage_hub`, so the notes it drives are readable
//! from the script, and the sentences the watchdog classifies as a failing leg are readable from
//! the watchdog. This guard reads both and holds them together: for each plane, and for each of
//! that plane's three legs, the installer must stage a note in which that leg is the one that is
//! down — **above the line that claims it**, because a case staged below a tick is a case that had
//! not run when the tick was printed.
//!
//! It deliberately does not read the alarm wording: `scripts/watchdog-selftest.sh` and
//! `tests/the_door_the_app_reads_through_is_watched.rs` own what each alarm says. What is owned
//! here is narrower and was the actual defect — whether the claim covers the ground it walked.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kickoff-channel sits two levels below the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "this guard reads {} and it is not there: {e}",
            path.display()
        )
    })
}

const INSTALLER: &str = "scripts/install-watchdog.sh";
const WATCHDOG: &str = "deploy/herdr-tg-watchdog.sh";

/// The line the installer prints about the legs. Everything above it has run when it appears;
/// everything below it has not.
const THE_CLAIM: &str = "any of the three legs going quiet alarms";

/// A leg, as the watchdog classifies it: the shell variable it reads the sentence out of, the
/// plane the leg belongs to, and the plain name to say when it is missing.
struct Leg {
    classifier: &'static str,
    plane: &'static str,
    called: &'static str,
}

/// The two planes' three legs each. The agents' door is one leg with one set of sentences that both
/// planes write, and it still has to be driven twice: the plane decides which alarm it produces and
/// which other legs it is held apart from, so a door outage proved on the phone line proves nothing
/// about the app.
const LEGS: &[Leg] = &[
    Leg {
        classifier: "said_phone",
        plane: "the phone line",
        called: "the line out to Telegram",
    },
    Leg {
        classifier: "said_door",
        plane: "the phone line",
        called: "the agents' door",
    },
    Leg {
        classifier: "said_updates",
        plane: "the phone line",
        called: "the line his taps come back down",
    },
    Leg {
        classifier: "said_ring",
        plane: "the app",
        called: "what agents say being written down for the app",
    },
    Leg {
        classifier: "said_door",
        plane: "the app",
        called: "the agents' door",
    },
    Leg {
        classifier: "said_sweep",
        plane: "the app",
        called: "the sweep of the place his answers arrive",
    },
];

/// Every sentence the watchdog recognises for one leg, split by the verdict it draws from it —
/// pulled out of its own `case "$said_x" in` block rather than copied. A copy is a second place to
/// fix, and the one time this repo kept two copies of a wire they drifted by thirteen
/// already-fixed defects.
///
/// The healthy sentences are read as well as the failing ones, because they are how a staged note
/// says which plane it is from: the two planes' first and third lines share no wording at all, and
/// a note whose door line is the only thing this guard could read would be a note it could not
/// attribute to either.
struct Sentences {
    good: Vec<String>,
    bad: Vec<String>,
}

fn the_sentences_of(classifier: &str) -> Sentences {
    let text = read(WATCHDOG);
    let opener = format!("case \"${classifier}\" in");
    let from = text.find(&opener).unwrap_or_else(|| {
        panic!(
            "{WATCHDOG} no longer has a `{opener}` block. This guard reads the leg's sentences out \
             of that block; without it, it would hold the installer to nothing."
        )
    });
    let rest = &text[from + opener.len()..];
    let to = rest
        .find("\n    esac")
        .or_else(|| rest.find("\nesac"))
        .unwrap_or_else(|| panic!("{WATCHDOG}: the `{opener}` block does not end where expected"));

    let mut out = Sentences {
        good: Vec::new(),
        bad: Vec::new(),
    };
    for line in rest[..to].lines() {
        let t = line.trim();
        let bad = t.contains("_state=bad");
        let good = t.contains("_state=good");
        if !bad && !good {
            continue; // a comment, or the `esac`
        }
        let Some((pattern, _)) = t.split_once(')') else {
            continue;
        };
        let literal: String = pattern
            .trim()
            .trim_end_matches('*')
            .trim_matches(|c| c == '\'' || c == '"')
            .to_owned();
        if literal.is_empty() {
            continue;
        }
        if bad {
            out.bad.push(literal)
        } else {
            out.good.push(literal)
        }
    }
    assert!(
        !out.bad.is_empty(),
        "{WATCHDOG}: the `{opener}` block names no failing sentence at all, so every leg would \
         read as covered by a note that says nothing is wrong."
    );
    out
}

/// Which plane a staged note is from, decided the way the watchdog decides it: off the note's own
/// first line, whose wording the two planes share nothing of.
fn the_plane_of(note: &Staged) -> Option<&'static str> {
    for leg in LEGS {
        if leg.classifier != "said_phone" && leg.classifier != "said_ring" {
            continue;
        }
        let s = the_sentences_of(leg.classifier);
        if s.good
            .iter()
            .chain(s.bad.iter())
            .any(|p| note.legs[0].starts_with(p))
        {
            return Some(leg.plane);
        }
    }
    None
}

/// The installer's own shell variables — `NAME="a whole sentence"` — so a note staged through one
/// is read as the sentence it really carries.
fn the_installers_sentences() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in read(INSTALLER).lines() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        let Some((name, value)) = t.split_once('=') else {
            continue;
        };
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            continue;
        }
        let v = value.trim();
        if v.len() >= 2
            && v.starts_with('"')
            && v.ends_with('"')
            && !v[1..v.len() - 1].contains('"')
        {
            out.insert(name.to_owned(), v[1..v.len() - 1].to_owned());
        }
    }
    out
}

/// One staged note: the three leg sentences the installer handed `stage_hub`, and the line it was
/// staged on.
struct Staged {
    line_no: usize,
    legs: Vec<String>,
}

/// Every `stage_hub` call above the claim, with its arguments resolved to the sentences they carry.
///
/// Continuations are joined first: the calls are wrapped across lines, and a parser that read one
/// physical line at a time would silently see a two-legged note and call the third leg untested.
fn what_the_installer_stages_before_it_ticks() -> Vec<Staged> {
    let text = read(INSTALLER);
    let claim_at = text
        .lines()
        .position(|l| l.contains(THE_CLAIM))
        .unwrap_or_else(|| {
            panic!(
                "{INSTALLER} no longer prints \"{THE_CLAIM}\". If the claim was reworded, reword \
                 it here too; if it was dropped, so is the reason for this guard."
            )
        });

    let vars = the_installers_sentences();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < claim_at {
        let t = lines[i].trim();
        if t.starts_with('#') || !t.starts_with("stage_hub ") {
            i += 1;
            continue;
        }
        let first = i;
        let mut joined = String::new();
        loop {
            let l = lines[i].trim_end();
            if let Some(stripped) = l.strip_suffix('\\') {
                joined.push_str(stripped);
                i += 1;
                assert!(
                    i < lines.len(),
                    "{INSTALLER}:{first}: a stage_hub call runs off the end"
                );
            } else {
                joined.push_str(l);
                break;
            }
        }
        i += 1;

        // `stage_hub <dir> <stamp age> <note age> <leg 1> <leg 2> <leg 3>`: the three legs are the
        // last three arguments, whether they arrived as a literal or through a variable.
        let args = shell_words(&joined, &vars);
        assert!(
            args.len() >= 7,
            "{INSTALLER}:{}: this stage_hub call carries {} arguments, not the six \
             `stage_hub` takes. A note staged with a leg missing is a leg nobody drove.",
            first + 1,
            args.len() - 1
        );
        out.push(Staged {
            line_no: first + 1,
            legs: args[args.len() - 3..].to_vec(),
        });
    }
    assert!(
        !out.is_empty(),
        "{INSTALLER} stages no note at all above the line that claims every leg alarms."
    );
    out
}

/// Split one joined `stage_hub` line into its arguments, expanding `"$NAME"` from the installer's
/// own assignments. Anything it cannot resolve comes back empty, which classifies as no leg and
/// makes the guard report the leg as undriven rather than quietly counting it.
fn shell_words(line: &str, vars: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {
                word.push(c);
                started = true;
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            None => {
                word.push(c);
                started = true;
            }
        }
        let _ = chars.peek();
    }
    if started {
        out.push(word);
    }
    out.into_iter()
        .map(|w| {
            if let Some(name) = w.strip_prefix('$') {
                vars.get(name).cloned().unwrap_or_default()
            } else {
                w
            }
        })
        .collect()
}

/// **The defect, in one test.** Six legs are claimed; the installer must have driven six.
#[test]
fn the_installer_stages_a_failing_leg_for_every_leg_its_tick_claims_on_both_planes() {
    let staged = what_the_installer_stages_before_it_ticks();

    let mut undriven = Vec::new();
    for leg in LEGS {
        let down = the_sentences_of(leg.classifier).bad;
        // The leg must be the one that is down, in a note of its own plane. Two conditions, and
        // both were failing before this: a note where several legs are broken together proves the
        // combination and not the leg — which is why the installer's cases are written
        // single-legged — and a door outage proved on the phone line proves nothing about the app,
        // where the same sentence produces a different alarm beside different healthy legs.
        let driven = staged.iter().any(|note| {
            if the_plane_of(note) != Some(leg.plane) {
                return false;
            }
            let broken: Vec<usize> = (0..note.legs.len())
                .filter(|&n| {
                    LEGS.iter().any(|l| {
                        the_sentences_of(l.classifier)
                            .bad
                            .iter()
                            .any(|s| note.legs[n].starts_with(s))
                    })
                })
                .collect();
            broken.len() == 1 && down.iter().any(|s| note.legs[broken[0]].starts_with(s))
        });
        if !driven {
            undriven.push(format!("  {} — {}", leg.plane, leg.called));
        }
    }

    assert!(
        undriven.is_empty(),
        "{INSTALLER} prints \"{THE_CLAIM} … on either plane\" and never stages these legs as the \
         one that is down:\n{}\n\nEither drive them above that line, or print only what was \
         proved. The notes it does stage are on lines {:?}.",
        undriven.join("\n"),
        staged.iter().map(|s| s.line_no).collect::<Vec<_>>()
    );
}

/// The near miss of the same shape: a note staged and then not looked at. It costs the same minute
/// and reads, in the output, exactly like a leg that was checked.
#[test]
fn every_note_the_installer_stages_is_one_it_then_reads_an_alarm_out_of() {
    let text = read(INSTALLER);
    let lines: Vec<&str> = text.lines().collect();
    let claim_at = lines
        .iter()
        .position(|l| l.contains(THE_CLAIM))
        .expect("the claim is printed");
    let staged: Vec<usize> = what_the_installer_stages_before_it_ticks()
        .iter()
        .map(|s| s.line_no)
        .collect();

    for (n, &start) in staged.iter().enumerate() {
        let end = staged.get(n + 1).copied().unwrap_or(claim_at + 1) - 1;
        let between = lines[start..end].join("\n");
        assert!(
            between.contains("grep") && between.contains("die "),
            "{INSTALLER}:{start} stages a note and nothing between it and the next one reads an \
             alarm out of it or refuses. A staged case nobody asserts on takes the same minute and \
             prints the same tick as one that was checked."
        );
    }

    // And a guard that walked no cases is the failure it is written against.
    let planes: BTreeSet<&str> = LEGS.iter().map(|l| l.plane).collect();
    assert!(
        staged.len() >= planes.len(),
        "{INSTALLER} stages {} notes for {} planes, which cannot cover them.",
        staged.len(),
        planes.len()
    );

    // A note this guard cannot place on a plane counts for no leg, which is the safe direction but
    // a silent one: the test above would report the leg undriven and the author would go looking
    // for a case that is sitting right there. Say it here instead.
    for note in what_the_installer_stages_before_it_ticks() {
        assert!(
            the_plane_of(&note).is_some(),
            "{INSTALLER}:{} stages a note whose first line — {:?} — is a sentence no hub writes \
             for either plane, so nothing can tell which plane the case is about.",
            note.line_no,
            note.legs[0]
        );
    }
}

//! **The privacy guard.** No fixture in this crate may carry the operator's real working context.
//!
//! # Why this is a test and not just a step in the capture script
//!
//! `scripts/capture-fixtures.sh` scrubs unconditionally and refuses to finish dirty, which stops a
//! *fresh capture* from leaking. But that script is run by hand, rarely, and only on the box with a
//! live herd. It cannot stop a fixture that is hand-edited, restored from an older copy, copied in
//! from a scratch directory, or added as a brand-new file by a future slice. Those are the routes
//! that actually reach a commit. So the property is asserted here as well, where it runs on every
//! `cargo test` — and therefore on every commit, which is the only gate the coordinator runs
//! autonomously.
//!
//! This repository is PUBLIC and a committed fixture is public forever, so this test failing means
//! **do not commit** — not "adjust the assertion".
//!
//! # Deny by default
//!
//! The scan enumerates the fixture DIRECTORY at run time rather than listing files it knows about.
//! A fixture added tomorrow is covered the day it is added, not the day someone remembers to add it
//! to a list. (An allowlist of two hard-coded paths is exactly the hole a reviewer found in
//! `no_live_write_call_site.rs`.) `scanned_set_is_not_empty` is the matching vacuity guard: a
//! renamed directory must turn this red, not silently green.
//!
//! # What is checked
//!
//! Structural shapes only, so the checks are machine-independent — they catch a leak captured on
//! any box, not just this one, and this file needs no private string in it to do the catching:
//!
//! | shape | why it is a leak |
//! |---|---|
//! | `/home/<name>` where name is not the placeholder | a real home directory names a real person |
//! | `ses_…` containing a letter | a real agent session id (the placeholders are all digits) |
//! | a UUID outside the placeholder range | a real agent/session UUID |
//! | `user@host:~` or `user@host:/` | a shell prompt captured off a real screen |
//! | `~/Something` | a home-relative path out of a real capture |
//!
//! `scripts/scrub-fixtures.py --check` applies these same shapes PLUS this box's own username and
//! hostname, which a test cannot portably assert on. Run it if this test ever surprises you.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The one home directory a committed fixture is allowed to mention.
const PLACEHOLDER_HOME_USER: &str = "user";

/// EVERY crate's fixtures, not just this one's.
///
/// This scanned only `herdr-client/tests/fixtures` and so never looked at `herdr-tg`'s — which
/// then carried a real session id from the operator's machine right up to the moment of a public
/// push, caught by a manual sweep rather than by this test. Deny-by-omission: the same shape of
/// hole that `no_live_write_call_site.rs` had, fixed the same way. A new crate's fixtures are
/// covered by default, and a directory that cannot be read is a failure rather than a skip.
fn fixture_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<name> sits two levels below the workspace root")
        .join("crates");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).expect("the crates directory is readable") {
        let d = entry
            .expect("a readable crate entry")
            .path()
            .join("tests/fixtures");
        if d.is_dir() {
            out.push(d);
        }
    }
    assert!(
        !out.is_empty(),
        "no fixture directory found — a vacuous scan is not a pass"
    );
    out
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture file in the WORKSPACE, enumerated from the directories themselves.
fn fixture_files() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in fixture_dirs() {
        let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| {
            panic!(
                "the fixture directory {} must be readable: {e}",
                dir.display()
            )
        });
        for entry in entries {
            let p = entry.expect("a readable dir entry").path();
            if p.is_file() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-'
}

/// Every `/home/<name>` whose `<name>` is not the placeholder.
fn real_home_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices("/home/") {
        let rest = &text[i + "/home/".len()..];
        let name: String = rest
            .bytes()
            .take_while(|b| is_ident_byte(*b))
            .map(|b| b as char)
            .collect();
        if !name.is_empty() && name != PLACEHOLDER_HOME_USER {
            out.push(format!("/home/{name}"));
        }
    }
    out
}

/// Every `ses_…` id carrying a letter. The scrubber's placeholders are digits only, so any letter
/// means the value came off a real socket.
fn real_session_ids(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices("ses_") {
        let rest = &text[i + "ses_".len()..];
        let body: String = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric())
            .map(|b| b as char)
            .collect();
        if body.len() >= 8 && body.bytes().any(|b| b.is_ascii_alphabetic()) {
            out.push(format!("ses_{body}"));
        }
    }
    out
}

/// Every 8-4-4-4-12 hex UUID that is not one the scrubber minted.
fn real_uuids(text: &str) -> Vec<String> {
    const SHAPE: [usize; 5] = [8, 4, 4, 4, 12];
    let b = text.as_bytes();
    let mut out = Vec::new();
    let total: usize = SHAPE.iter().sum::<usize>() + 4;
    if b.len() < total {
        return out;
    }
    'outer: for start in 0..=b.len() - total {
        // Must not be preceded by a hex digit, or "…deadbeef-…" matches inside a longer blob.
        if start > 0 && b[start - 1].is_ascii_hexdigit() {
            continue;
        }
        let mut at = start;
        for (n, len) in SHAPE.iter().enumerate() {
            for _ in 0..*len {
                if !b[at].is_ascii_hexdigit() {
                    continue 'outer;
                }
                at += 1;
            }
            if n < SHAPE.len() - 1 {
                if b[at] != b'-' {
                    continue 'outer;
                }
                at += 1;
            }
        }
        let found = &text[start..at];
        // The scrubber mints 00000000-0000-4000-8000-… and nothing else.
        if !found.starts_with("00000000-0000-4000-8000-") {
            out.push(found.to_owned());
        }
    }
    out
}

/// Every `name@host:~` / `name@host:/` — the shape of a shell prompt captured off a real screen.
fn shell_prompts(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    for (i, _) in text.match_indices('@') {
        let mut lo = i;
        while lo > 0 && is_ident_byte(b[lo - 1]) {
            lo -= 1;
        }
        let mut hi = i + 1;
        while hi < b.len() && is_ident_byte(b[hi]) {
            hi += 1;
        }
        if lo == i || hi == i + 1 || hi >= b.len() || b[hi] != b':' {
            continue;
        }
        if hi + 1 < b.len() && (b[hi + 1] == b'~' || b[hi + 1] == b'/') {
            out.push(text[lo..=hi + 1].to_owned());
        }
    }
    out
}

/// Every `~/Something` — a home-relative path only a real capture produces.
fn tilde_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices("~/") {
        let rest = &text[i + 2..];
        let seg: String = rest
            .bytes()
            .take_while(|b| is_ident_byte(*b))
            .map(|b| b as char)
            .collect();
        if !seg.is_empty() {
            out.push(format!("~/{seg}"));
        }
    }
    out
}

fn leaks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(
        real_home_paths(text)
            .into_iter()
            .map(|v| format!("home path {v}")),
    );
    out.extend(
        real_session_ids(text)
            .into_iter()
            .map(|v| format!("session id {v}")),
    );
    out.extend(real_uuids(text).into_iter().map(|v| format!("uuid {v}")));
    out.extend(
        shell_prompts(text)
            .into_iter()
            .map(|v| format!("shell prompt {v}")),
    );
    out.extend(
        tilde_paths(text)
            .into_iter()
            .map(|v| format!("home-relative path {v}")),
    );
    out
}

// ── the tests ───────────────────────────────────────────────────────────────────────────────────

/// **THE test.** Not one fixture may carry a real home path, session id, UUID, prompt or
/// home-relative path.
#[test]
fn no_fixture_carries_the_operators_identity() {
    let mut findings: Vec<String> = Vec::new();
    for path in fixture_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // a binary fixture would be a separate problem; nothing to string-scan
        };
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut distinct: BTreeSet<String> = BTreeSet::new();
        distinct.extend(leaks(&text));
        for leak in distinct {
            findings.push(format!("{name}: {leak}"));
        }
    }
    assert!(
        findings.is_empty(),
        "{} identifying value(s) are about to be committed to a PUBLIC repository.\n  {}\n\n\
         Do NOT relax this assertion. Re-run `scripts/scrub-fixtures.py` (it is what\n\
         `scripts/capture-fixtures.sh` runs unconditionally after every capture), then re-test.",
        findings.len(),
        findings.join("\n  ")
    );
}

/// The vacuity guard. If the fixture directory is ever renamed or emptied, the scan above would
/// pass by scanning nothing — which is the failure mode that lets a leak through unnoticed.
#[test]
fn scanned_set_is_not_empty() {
    let files = fixture_files();
    assert!(
        files.len() >= 5,
        "expected the captured fixture set in {}, found {} file(s) — if the fixtures moved, point \
         this test at them rather than deleting it",
        fixtures_dir().display(),
        files.len()
    );
    let names: BTreeSet<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    for required in ["snapshot.json", "pane_read.json", "events-mixed.ndjson"] {
        assert!(
            names.contains(required),
            "{required} is missing from the fixture set; the privacy scan would not cover it"
        );
    }
}

/// The detectors must actually fire. A privacy guard nobody has attacked is a privacy guard that
/// silently matches nothing — every shape below is a real leak this crate has already carried once.
#[test]
fn the_detectors_catch_a_planted_leak_of_every_shape() {
    // Assembled at run time so this source file never itself contains a specimen leak.
    let home = format!("/home/{}", "somebody");
    assert_eq!(
        real_home_paths(&home).len(),
        1,
        "a real home path must be caught"
    );
    assert!(
        real_home_paths("/home/user/projects/acme").is_empty(),
        "the placeholder home must NOT be flagged, or the guard is always red"
    );

    // Synthetic, not the operator's. The detector is SHAPE-based (>=8 alphanumerics with at
    // least one letter), so an invented body exercises it exactly as a real id would. A real
    // id here would be a live leak that no scanner can see, which is what round-two review
    // found in this very line.
    let ses = format!("ses_{}", "ZqWvXyTr01aBcDeFgHjK23mN");
    assert_eq!(
        real_session_ids(&ses).len(),
        1,
        "a real session id must be caught"
    );
    assert!(
        real_session_ids("ses_00000000000000000000000001").is_empty(),
        "the all-digit placeholder must NOT be flagged"
    );

    // Synthetic 8-4-4-4-12. The previous value was derived from a live session id.
    let uuid = ["1a2b3c4d", "5e6f", "4a7b", "8c9d", "0e1f2a3b4c5d"].join("-");
    assert_eq!(real_uuids(&uuid).len(), 1, "a real uuid must be caught");
    assert!(
        real_uuids("00000000-0000-4000-8000-000000000005").is_empty(),
        "the minted placeholder uuid must NOT be flagged"
    );

    let prompt = format!("{}@{}:~/Projects/x", "someone", "some-host");
    assert_eq!(
        shell_prompts(&prompt).len(),
        1,
        "a shell prompt must be caught"
    );
    assert!(
        shell_prompts("mail: a@b").is_empty(),
        "an @ without a path suffix is not a prompt"
    );

    assert_eq!(
        tilde_paths("~/Projects").len(),
        1,
        "a ~-path must be caught"
    );
    assert!(tilde_paths("~").is_empty(), "a bare tilde is not a path");
}

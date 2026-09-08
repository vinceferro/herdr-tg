//! The two public files must describe the product that ships, not the one that was deleted.
//!
//! `README.md` spent eleven days describing a screen-reading router with a "sticky pane" and a
//! terminal mirror, and `TRACKER.md` spent six days telling a reader that the live round trip to a
//! phone had not happened — it had, the evening the tracker was last written. Neither is a compile
//! error and neither shows up in a diff anybody reads, so it is pinned here instead.
//!
//! # The rule these tests keep
//!
//! **A number in a public file is checked against the thing that produces it, never against a
//! second copy of itself.** That is the whole failure this item exists to close: the tracker was a
//! second copy of `CLAUDE.md`'s state and drifted within eleven hours of being written. So the
//! command list is read out of `main.rs`, the rates are read out of `queue.rs`, and the README is
//! held against those — a sixteenth subcommand or a changed ceiling turns this red rather than
//! quietly making the README wrong.
//!
//! The ceiling arm is the one worth understanding. Telegram's measured limit is twenty sends a
//! minute for a chat (`docs/RATE-PROBE.md` §1) and the hub spends eighteen of them
//! (`queue.rs::PER_MINUTE`). The temptation, whenever the envelope looks tight, is to write a
//! bigger number in the README rather than to change what the product does. This refuses that: no
//! number above Telegram's own twenty may stand beside "a minute" anywhere in the README —
//! **written as a word or as a digit**, because the README's own style is mostly words and a check
//! that only reads digits is a check the next rewrite walks straight past.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/herdr-tg is two levels below the repo root")
        .to_path_buf()
}

fn public(name: &str) -> String {
    let p = repo().join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} is not readable: {e}", p.display()))
}

fn source(rel: &str) -> String {
    let p = repo().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} is not readable: {e}", p.display()))
}

/// Words that only ever appear in a description of the product that was deleted.
const OF_THE_DELETED_PRODUCT: [&str; 6] = [
    "pane",
    "keystroke",
    "collie",
    "sticky",
    "routing agent",
    "has never run",
];

#[test]
fn the_public_docs_do_not_describe_the_product_that_was_deleted() {
    let mut found = Vec::new();
    for name in ["README.md", "TRACKER.md"] {
        let lower = public(name).to_lowercase();
        for word in OF_THE_DELETED_PRODUCT {
            if lower.contains(word) {
                found.push(format!("{name} says \"{word}\""));
            }
        }
    }
    assert!(
        found.is_empty(),
        "the public files still describe the deleted product: {found:?}. The screen-reading path \
         was deleted rather than switched off, so a reader who arrives at this repo through its \
         README must not be told it exists."
    );
}

#[test]
fn the_readme_says_what_the_binary_can_never_do() {
    let readme = public("README.md");
    assert!(
        readme.contains("cannot type into a terminal"),
        "the README does not say the one thing about this binary that a reader must not have to \
         infer: it cannot type into a terminal."
    );
}

#[test]
fn the_readme_states_the_envelope_it_was_measured_at() {
    // Each of these is a rule from `docs/RATE-PROBE.md` or `queue.rs`. A README that drops one of
    // them is a README that lets a reader design against a ceiling nobody measured.
    let readme = public("README.md").to_lowercase();
    for phrase in [
        "sparse control surface",
        "one live connection",
        "answered once can never be answered twice",
    ] {
        assert!(
            readme.contains(phrase),
            "the README no longer states the envelope: \"{phrase}\" is missing."
        );
    }
}

// ───────────────────────────── numbers, and where they come from ─────────────────────────────

/// Telegram's own measured ceiling for one chat: twenty accepted, the twenty-first refused with a
/// `retry_after` that counted down the rest of the minute (`docs/RATE-PROBE.md` §1).
const TELEGRAMS_OWN_CEILING: u32 = 20;

/// The English for a small number, both directions. The README writes its envelope in words far
/// more often than in digits, so a guard that reads only digits reads almost none of it.
const SPELLED: [(&str, u32); 22] = [
    ("zero", 0),
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("eleven", 11),
    ("twelve", 12),
    ("thirteen", 13),
    ("fourteen", 14),
    ("fifteen", 15),
    ("sixteen", 16),
    ("seventeen", 17),
    ("eighteen", 18),
    ("nineteen", 19),
    ("twenty", 20),
    ("thirty", 30),
];

fn spelled(n: u32) -> String {
    SPELLED
        .iter()
        .find(|(_, v)| *v == n)
        .map(|(w, _)| (*w).to_string())
        .unwrap_or_else(|| n.to_string())
}

/// What a token in the README means as a number, whether it is written `18`, `**18**` or
/// `eighteen`. `None` when it is not a number at all.
fn value_of(token: &str) -> Option<u32> {
    let bare = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if bare.is_empty() {
        return None;
    }
    if let Ok(n) = bare.parse::<u32>() {
        return Some(n);
    }
    let lower = bare.to_lowercase();
    SPELLED.iter().find(|(w, _)| *w == lower).map(|(_, v)| *v)
}

/// A named `pub const` out of a source file, as the number it is set to. Fails closed: a constant
/// this cannot find is a constant that has been renamed, and a guard that shrugs at that is a guard
/// that stops comparing anything.
fn shipped_constant(rel: &str, name: &str) -> u32 {
    let text = source(rel);
    let needle = format!("pub const {name}: u32 = ");
    let at = text.find(&needle).unwrap_or_else(|| {
        panic!(
            "{rel} no longer declares {name}. It is one of the numbers the README quotes, so if it \
             has moved, move this reader with it rather than letting the comparison quietly stop."
        )
    });
    let tail = &text[at + needle.len()..];
    let end = tail
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(tail.len());
    tail[..end]
        .parse()
        .unwrap_or_else(|_| panic!("{rel}'s {name} is not a plain number"))
}

#[test]
fn no_number_in_the_readme_claims_more_a_minute_than_telegram_allowed() {
    let readme = public("README.md");
    let words: Vec<&str> = readme.split_whitespace().collect();
    let mut claimed: Vec<String> = Vec::new();
    for w in 0..words.len() {
        let Some(n) = value_of(words[w]) else {
            continue;
        };
        // "<n> [unit] (a|per) minute" — the unit between the number and the window is the part a
        // rewrite changes first, so the look-ahead is three tokens wide.
        let tail: Vec<String> = words[w + 1..]
            .iter()
            .take(3)
            .map(|s| {
                s.trim_matches(|c: char| !c.is_ascii_alphanumeric())
                    .to_lowercase()
            })
            .collect();
        let says_a_minute = tail
            .windows(2)
            .any(|p| (p[0] == "a" || p[0] == "per") && p[1] == "minute")
            || tail.first().is_some_and(|f| f == "minute");
        if says_a_minute && n > TELEGRAMS_OWN_CEILING {
            claimed.push(format!("{} ({n})", words[w]));
        }
    }
    assert!(
        claimed.is_empty(),
        "the README claims {claimed:?} a minute. Telegram accepted twenty and refused the \
         twenty-first, measured; the ceiling is not ours to raise, and a document that raises it \
         is how a fleet gets designed against a limit that does not exist."
    );
}

#[test]
fn the_rates_the_readme_states_are_the_constants_the_binary_ships() {
    let per_minute = shipped_constant("crates/herdr-tg/src/queue.rs", "PER_MINUTE");
    let reserved = shipped_constant("crates/herdr-tg/src/queue.rs", "RESERVED_FOR_THE_OPERATOR");
    let reactions = shipped_constant("crates/herdr-tg/src/queue.rs", "REACTIONS_PER_MINUTE");

    assert!(
        per_minute <= TELEGRAMS_OWN_CEILING && reactions <= TELEGRAMS_OWN_CEILING,
        "the hub now spends {per_minute} sends and {reactions} reactions a minute, and Telegram \
         refused the twenty-first of either. Sitting on a limit means discovering it from a 429 \
         during an incident."
    );

    let readme = public("README.md");
    let agents_share = per_minute - reserved;
    // A brand-new conversation costs three turns before its agent's own words land — the topic, the
    // greeting, then the message (`hub.rs`). The README quotes the quotient, so the quotient is
    // what is checked, not a number somebody typed twice.
    let new_conversations = per_minute / 3;

    for (what, phrase) in [
        (
            "what the hub spends",
            format!("{per_minute} sends a minute"),
        ),
        ("what is left for the agents", spelled(agents_share)),
        (
            "the reaction ceiling the hub asks for",
            format!("at most {reactions}"),
        ),
        (
            "how many new conversations fit in a minute",
            format!("{} new conversations a minute", spelled(new_conversations)),
        ),
    ] {
        assert!(
            readme.contains(&phrase),
            "the README no longer states {what}: it should say {phrase:?}, taken from \
             `queue.rs`. Two copies of one number is precisely the drift this file exists to \
             close — change the constant and this test tells you which sentence to rewrite."
        );
    }
}

// ───────────────────────────── the commands, from clap's own list ─────────────────────────────

/// Every subcommand the binary has, derived from `main.rs` rather than from a list kept beside it.
///
/// A hard-coded list here would be a second copy of the command set, which is exactly the failure
/// this file is about: the drift it is meant to catch — a sixteenth subcommand nobody documented —
/// is the one thing a hard-coded list can never see.
fn subcommands() -> Vec<String> {
    let names = subcommands_in(&source("crates/herdr-tg/src/main.rs"));
    assert!(
        names.len() >= 10,
        "only found {names:?} in `enum Cmd`, which cannot be this binary's whole command set — \
         this guard fails rather than comparing the README against almost nothing"
    );
    names
}

/// Split out from the file it reads so the reader itself can be shown a command set it has never
/// seen. A parser that has only ever been pointed at the one file it passes on is a parser nobody
/// has watched notice anything.
fn subcommands_in(text: &str) -> Vec<String> {
    let at = text
        .find("enum Cmd {")
        .expect("main.rs no longer declares `enum Cmd`; this guard cannot read the command set");
    let body = &text[at..];
    let mut names = Vec::new();
    for line in body.lines().skip(1) {
        if line == "}" {
            break;
        }
        // Variants sit at one level of indentation; their fields sit at two.
        let Some(rest) = line.strip_prefix("    ") else {
            continue;
        };
        if rest.starts_with(' ') {
            continue;
        }
        let name = rest.trim_end_matches([' ', '{', '(', ',']);
        if name.is_empty() || !name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            continue;
        }
        names.push(kebab(name));
    }
    names
}

/// `AdoptSecrets` → `adopt-secrets`, which is what clap derives when nothing renames a variant.
fn kebab(variant: &str) -> String {
    let mut out = String::new();
    for (i, c) in variant.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn the_command_reader_sees_a_command_that_was_just_added() {
    let one_more = "\
enum Cmd {
    Serve {
        /// a field, at two levels of indentation
        forum: i64,
    },
    AdoptSecrets {
        apply: bool,
    },
    SomethingNobodyDocumented,
}
";
    assert_eq!(
        subcommands_in(one_more),
        vec![
            "serve".to_owned(),
            "adopt-secrets".to_owned(),
            "something-nobody-documented".to_owned()
        ],
        "the reader must see every variant, including the one that was added a minute ago — that \
         is the whole reason the list is derived rather than typed out here"
    );
}

#[test]
fn the_readme_names_every_command_the_binary_has() {
    // A command a reader cannot find in the README is a command only the operator's memory knows
    // about, and every one of them is the only way to do the thing it does.
    let readme = public("README.md");
    let all = subcommands();
    // The count is stated in words at the top of that section, and a count is a second copy of the
    // list — so it is held against the list rather than left to be right by luck.
    let counted = format!(
        "{}, and every one of them is run at a keyboard",
        spelled(all.len() as u32)
    );
    assert!(
        readme.to_lowercase().contains(&counted),
        "the README should open its command section with {counted:?}; the binary has {} \
         subcommands.",
        all.len()
    );
    let missing: Vec<String> = subcommands()
        .into_iter()
        .filter(|c| !readme.contains(&format!("`{c}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "the README does not name these commands: {missing:?}. They are all terminal-only, so \
         nothing else in the product will tell a reader they exist."
    );
}

#[test]
fn the_readme_does_not_name_a_command_the_binary_does_not_have() {
    // The other direction, and the one a rename breaks: a README that still documents a verb which
    // no longer exists sends a reader to a terminal to be told there is no such command.
    let readme = public("README.md");
    let real = subcommands();
    // Only the table's own rows are checked, so ordinary prose in backticks is not mistaken for a
    // command that ought to exist.
    let invented: Vec<String> = readme
        .lines()
        .filter(|l| l.starts_with("| `") && l.contains(" | "))
        .filter_map(|l| l.split('`').nth(1).map(str::to_owned))
        // Command-shaped only: the other tables in this README put file names in the same column,
        // and `docs/ATTACHING.md` is not a verb anybody expected to run.
        .filter(|name| name.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
        .filter(|name| !real.contains(name))
        .collect();
    assert!(
        invented.is_empty(),
        "the README's command table documents {invented:?}, and the binary has no such command."
    );
}

#[test]
fn the_tracker_points_at_what_is_maintained_instead_of_keeping_a_second_copy() {
    let tracker = public("TRACKER.md");
    for pointer in ["CLAUDE.md", "docs/CAPABILITIES.md", "git log"] {
        assert!(
            tracker.contains(pointer),
            "the tracker no longer points at {pointer}. It is a pointer rather than a status board \
             precisely because the status board drifted; a pointer that names nothing is a status \
             board with no content."
        );
    }
}

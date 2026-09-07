//! **The watchdog contract, stated once and checked in both directions.**
//!
//! The hub and `deploy/herdr-tg-watchdog.sh` are joined by two files in a state directory and by
//! nothing else — no shared code, no shared process, not even a shared language. That separation is
//! the whole reason the alarm can still speak when the hub cannot, and the price of it is that every
//! sentence the hub writes exists a second time, as a literal, in a shell script. This guard is what
//! stops the two copies drifting.
//!
//! # Why a reword is not a cosmetic change
//!
//! The hub writes `hub.health`: a word, then one sentence for the phone line, one for the agents'
//! door, and one for the stream his taps come back down. The watchdog reads those three sentences
//! ONLY to word an alarm it has already decided to raise — but wording it wrong is not a small
//! failure. Rename the sentence that means "this half is fine" and the script stops recognising it,
//! so every outage is reported as total AND the alarm quotes, as the evidence of the fault, the
//! line saying that half is healthy. It was proved silent:
//! rewording `the phone line answered` in `heartbeat.rs` *and its assertions* — a coherent tidy-up,
//! which is how this actually happens — left the Rust suite green and the shell suite green, and
//! turned a dead door into an alarm saying the phone line was down and quoting the hub saying it had
//! answered twelve seconds ago.
//!
//! This repo has shipped exactly that drift once already: two copies of the bridge's link, thirteen
//! already-fixed defects apart.
//!
//! # It also fails when it cannot look
//!
//! Every lookup here is loud. A file it cannot read, a function whose shape it does not recognise, a
//! sentence it cannot extract, a `case` pattern it cannot parse — each is a FAILURE and never "found
//! nothing". A guard that reports a clean scan of a file it never opened is worse than no guard: it
//! is a green light nobody will question.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where the hub's two facts and their sentences are written.
const HEARTBEAT: &str = "crates/herdr-tg/src/heartbeat.rs";
/// Where the door's troubles are worded, and where the tick that rewrites the note lives.
const BOT: &str = "crates/herdr-tg/src/bot.rs";
/// The other language. Its literals are the second copy this guard exists to hold still.
const WATCHDOG: &str = "deploy/herdr-tg-watchdog.sh";
/// The one place in this binary that reads the stamp back, and holds a copy of the alarm's window.
const DOCTOR: &str = "crates/herdr-tg/src/cmd/doctor.rs";
/// How often the alarm is allowed to look, which is the last term in the wait he actually gets.
const TIMER: &str = "deploy/herdr-tg-watchdog.timer";

/// The workspace root: `crates/herdr-tg/` → up two.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/herdr-tg sits two levels below the workspace root")
        .to_path_buf()
}

/// Read a file, or say why this guard is blind rather than passing.
fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{relative} could not be read ({e}). This guard proves nothing about a file it cannot \
             open, so it fails rather than reporting agreement."
        )
    })
}

// ── pulling sentences out of Rust ────────────────────────────────────────────────────────────────

/// The text from `needle` to the `)` that closes the `(` opened at it, parentheses balanced.
///
/// Used to take one call's argument list. `None` when the needle is absent or the call never
/// closes, and every caller turns that into a failure — a call whose shape this cannot follow is a
/// call whose sentences it has not read.
fn call_args(text: &str, from: usize) -> Option<&str> {
    let open = from + text[from..].find('(')?;
    let mut depth = 0usize;
    let bytes = text.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[open + 1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Every `"…"` literal in a fragment of Rust, with the escapes these sentences may contain.
///
/// A `\` at the end of a line is Rust's line continuation — it eats the newline and the next line's
/// leading whitespace — and `bot.rs` uses it for the longest of the door's sentences, so a reader
/// that did not implement it would compare half a sentence and pass. Any OTHER escape is a panic
/// rather than a guess: this guard's whole job is comparing exact text.
fn string_literals(fragment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = fragment.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '"' {
            i += 1;
            continue;
        }
        let mut s = String::new();
        i += 1;
        loop {
            let c = *chars.get(i).unwrap_or_else(|| {
                panic!("a string literal in the source ran off the end; this guard cannot read it")
            });
            match c {
                '"' => {
                    i += 1;
                    break;
                }
                '\\' => {
                    i += 1;
                    match chars.get(i) {
                        Some('"') => s.push('"'),
                        Some('\\') => s.push('\\'),
                        Some('\n') => {
                            // Rust's line continuation: the newline and the next line's indentation
                            // are not part of the string.
                            i += 1;
                            while matches!(chars.get(i), Some(c) if c.is_whitespace()) {
                                i += 1;
                            }
                            continue;
                        }
                        other => panic!(
                            "this guard does not understand the escape \\{} in a sentence it has to \
                             compare with the watchdog's copy. Teach it the escape or keep the \
                             sentence plain.",
                            other.map_or("<end>".to_owned(), |c| c.to_string())
                        ),
                    }
                    i += 1;
                }
                _ => {
                    s.push(c);
                    i += 1;
                }
            }
        }
        out.push(s);
    }
    out
}

/// One sentence the hub can write into `hub.health` for one leg, and whether it means "this half is
/// working".
#[derive(Debug)]
struct Sentence {
    text: String,
    healthy: bool,
    /// Where it comes from, so a failure names the line to go and look at.
    origin: String,
}

/// Every sentence `heartbeat.rs` and `bot.rs` can put on the note's three lines.
///
/// Returned as (phone line, agents' door, update stream). The three `Leg::of` arguments are, in order, what the leg
/// says when it has never been good, when it is fresh, and when it is stale; the fresh and stale
/// ones are prefixes the code completes with an age, so they are completed here too — the watchdog
/// matches the whole sentence, not the prefix.
fn the_sentences_the_hub_can_write() -> (Vec<Sentence>, Vec<Sentence>, Vec<Sentence>) {
    let heartbeat = read(HEARTBEAT);
    let bot = read(BOT);

    let mut legs: Vec<Vec<Sentence>> = Vec::new();
    for (nth, at) in heartbeat
        .match_indices("Leg::of(")
        .map(|(i, _)| i)
        .enumerate()
    {
        let args = call_args(&heartbeat, at).unwrap_or_else(|| {
            panic!(
                "{HEARTBEAT}: a Leg::of call does not close; this guard cannot read its sentences"
            )
        });
        let literals = string_literals(args);
        assert_eq!(
            literals.len(),
            3,
            "{HEARTBEAT}: a Leg::of call carries {} sentences, not the three this guard knows how \
             to classify (never / fresh / stale). If a fourth shape of sentence exists, the \
             watchdog has to learn it and so does this guard.",
            literals.len()
        );
        legs.push(vec![
            Sentence {
                text: literals[0].clone(),
                healthy: false,
                origin: format!("{HEARTBEAT} Leg::of #{nth}, the never-yet-good sentence"),
            },
            Sentence {
                text: format!("{} 12 seconds ago", literals[1]),
                healthy: true,
                origin: format!("{HEARTBEAT} Leg::of #{nth}, the fresh sentence"),
            },
            Sentence {
                text: format!("{} 5 minutes ago", literals[2]),
                healthy: false,
                origin: format!("{HEARTBEAT} Leg::of #{nth}, the stale sentence"),
            },
        ]);
    }
    assert_eq!(
        legs.len(),
        3,
        "{HEARTBEAT}: expected exactly three legs (the phone line, the agents' door and the update \
         stream) built by Leg::of, found {}. A fourth leg is a fourth line in the note, and the \
         watchdog reads three.",
        legs.len()
    );
    // Source order: phone line, door, update stream — the order `Health::verdict` builds them in,
    // which is the order they are written onto the note. Popping reverses it.
    let mut updates = legs.pop().expect("three legs");
    let mut door = legs.pop().expect("three legs");
    let mut phone = legs.pop().expect("three legs");

    // The phone line's own trouble sentence, worded where the fact is set.
    let at = heartbeat
        .find("pub fn the_phone_line_did_not_answer")
        .unwrap_or_else(|| {
            panic!(
                "{HEARTBEAT}: the_phone_line_did_not_answer is gone; this guard is looking for \
                    the phone line's trouble sentence and cannot find where it is worded"
            )
        });
    let body_end = at
        + heartbeat[at..].find("\n    }").unwrap_or_else(|| {
            panic!(
                "{HEARTBEAT}: the_phone_line_did_not_answer does not end where this guard expects"
            )
        });
    let troubles = string_literals(&heartbeat[at..body_end]);
    assert_eq!(
        troubles.len(),
        1,
        "{HEARTBEAT}: the_phone_line_did_not_answer holds {} sentences, not one",
        troubles.len()
    );
    phone.push(Sentence {
        text: troubles[0].clone(),
        healthy: false,
        origin: format!("{HEARTBEAT} the_phone_line_did_not_answer"),
    });

    // Every way the door can be shut, and every way a knock can fail. Both are worded in bot.rs,
    // where the reason is known; heartbeat.rs only carries them.
    let mut door_troubles: Vec<(String, String)> = Vec::new();
    for at in bot.match_indices("Door::Shut(").map(|(i, _)| i) {
        let args =
            call_args(&bot, at).unwrap_or_else(|| panic!("{BOT}: a Door::Shut does not close"));
        for lit in string_literals(args) {
            door_troubles.push((lit, format!("{BOT} Door::Shut")));
        }
    }
    assert!(
        door_troubles.len() >= 3,
        "{BOT}: only {} ways the door can be shut carry a sentence. Every one of them is a way the \
         operator's agents reach nobody while his phone still answers, and each needs words the \
         watchdog can repeat.",
        door_troubles.len()
    );
    let mut knock_sentences = 0;
    for at in bot
        .match_indices("the_door_is_not_answering(")
        .map(|(i, _)| i)
    {
        let args = call_args(&bot, at)
            .unwrap_or_else(|| panic!("{BOT}: a the_door_is_not_answering call does not close"));
        for lit in string_literals(args) {
            knock_sentences += 1;
            door_troubles.push((lit, format!("{BOT} the_door_is_not_answering")));
        }
    }
    assert!(
        knock_sentences >= 2,
        "{BOT}: the knock at the door words fewer than two failures. It can fail to reach the \
         socket at all and it can reach one nobody is accepting on, and those are different \
         sentences to the operator."
    );
    for (text, origin) in door_troubles {
        door.push(Sentence {
            text,
            healthy: false,
            origin,
        });
    }

    // Every way Telegram can refuse to hand the updates over. Worded in bot.rs beside the error it
    // is reading, exactly as the door's are — a second copy of this bot holding the update slot and
    // a hub being turned away for any other reason are different mornings and the same silence.
    let mut refusals = 0;
    for at in bot
        .match_indices("the_update_stream_was_refused(")
        .map(|(i, _)| i)
    {
        let args = call_args(&bot, at).unwrap_or_else(|| {
            panic!("{BOT}: a the_update_stream_was_refused call does not close")
        });
        for lit in string_literals(args) {
            refusals += 1;
            updates.push(Sentence {
                text: lit,
                healthy: false,
                origin: format!("{BOT} the_update_stream_was_refused"),
            });
        }
    }
    assert!(
        refusals >= 2,
        "{BOT}: the update stream words fewer than two refusals. A second copy of this bot taking \
         his taps is a thing he can go and fix in ten seconds and any other refusal is not, so \
         they are different sentences to the operator."
    );

    (phone, door, updates)
}

// ── pulling the same sentences out of the shell ──────────────────────────────────────────────────

/// One `case` pattern in the watchdog, as a literal prefix and the state it assigns.
#[derive(Debug)]
struct Pattern {
    prefix: String,
    healthy: bool,
    line: usize,
}

/// The watchdog's classification of one leg's line: `case "$said_phone" in … esac`.
///
/// Anything in that block this cannot parse is a failure. A pattern silently skipped is a sentence
/// this guard would then declare unmatched — or worse, one it would let drift.
fn how_the_watchdog_reads(variable: &str, state_var: &str) -> Vec<Pattern> {
    let script = read(WATCHDOG);
    let head = format!("case \"${variable}\" in");
    let at = script.find(&head).unwrap_or_else(|| {
        panic!(
            "{WATCHDOG}: there is no `{head}` block. This guard reads the watchdog's own \
             classification of the hub's sentences and cannot find it."
        )
    });
    let before = script[..at].lines().count();
    let end = at
        + script[at..].find("\n    esac").unwrap_or_else(|| {
            panic!("{WATCHDOG}: the `{head}` block does not end where this guard expects")
        });

    let mut out = Vec::new();
    for (i, line) in script[at..end].lines().enumerate().skip(1) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (pattern, action) = line.split_once(american_paren()).unwrap_or_else(|| {
            panic!(
                "{WATCHDOG}:{}: this line inside `{head}` is not `<pattern>) <var>=<state> ;;` and \
                 this guard will not guess what it does: {line}",
                before + i + 1
            )
        });
        // `'…'*` — the trailing `*` is the glob and belongs outside the quotes, so it comes off
        // first. What is left must be a quoted literal: an unquoted one would be globbed by the
        // shell and could match sentences nobody wrote.
        let quoted = pattern.trim().strip_suffix('*').unwrap_or_else(|| {
            panic!(
                "{WATCHDOG}:{}: the pattern {pattern} does not end in `*`, so it would only match a \
                 sentence with no age on the end of it.",
                before + i + 1
            )
        });
        let prefix = quoted
            .strip_prefix('\'')
            .and_then(|p| p.strip_suffix('\''))
            .or_else(|| quoted.strip_prefix('"').and_then(|p| p.strip_suffix('"')))
            .unwrap_or_else(|| {
                panic!(
                    "{WATCHDOG}:{}: the pattern {quoted} is not a quoted literal. An unquoted glob \
                     here would match sentences nobody wrote.",
                    before + i + 1
                )
            })
            .to_owned();
        let healthy = match action.trim().trim_end_matches(";;").trim() {
            a if a == format!("{state_var}=good") => true,
            a if a == format!("{state_var}=bad") => false,
            other => panic!(
                "{WATCHDOG}:{}: `{other}` is neither `{state_var}=good` nor `{state_var}=bad`. A \
                 third state is one this guard cannot check against the hub.",
                before + i + 1
            ),
        };
        out.push(Pattern {
            prefix,
            healthy,
            line: before + i + 1,
        });
    }
    assert!(
        !out.is_empty(),
        "{WATCHDOG}: the `{head}` block matches nothing at all, so every sentence the hub writes \
         would read as one it cannot understand"
    );
    out
}

/// The `)` that closes a `case` pattern. Named, because a bare `")"` in the split above reads like
/// punctuation rather than the thing being looked for.
fn american_paren() -> char {
    ')'
}

// ── the pins ─────────────────────────────────────────────────────────────────────────────────────

/// Every sentence the hub can write is one the watchdog reads the same way — and every sentence the
/// watchdog knows is one the hub can write.
///
/// Both directions, because they fail differently. A hub sentence the script does not know makes an
/// outage read as total and quotes a healthy half as the fault; a script literal the hub never
/// writes is a rule that will never fire, which is how a classification quietly stops covering the
/// case it was written for.
#[test]
fn the_watchdog_and_the_hub_use_the_same_sentences_for_the_same_half() {
    let (phone, door, updates) = the_sentences_the_hub_can_write();
    let mut problems: Vec<String> = Vec::new();

    for (leg, sentences, patterns) in [
        (
            "the phone line",
            &phone,
            how_the_watchdog_reads("said_phone", "phone_state"),
        ),
        (
            "the agents' door",
            &door,
            how_the_watchdog_reads("said_door", "door_state"),
        ),
        (
            "the update stream",
            &updates,
            how_the_watchdog_reads("said_updates", "updates_state"),
        ),
    ] {
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for s in sentences {
            let hits: Vec<&Pattern> = patterns
                .iter()
                .filter(|p| s.text.starts_with(&p.prefix) || p.prefix.starts_with(&s.text))
                .collect();
            match hits.as_slice() {
                [] => problems.push(format!(
                    "{WATCHDOG} does not recognise a sentence the hub writes for {leg}: \
                     \"{}\" (from {}). It would read it as a hub it cannot understand and say \
                     nothing about which half failed.",
                    s.text, s.origin
                )),
                [hit] => {
                    used.insert(hit.line);
                    if hit.healthy != s.healthy {
                        problems.push(format!(
                            "{WATCHDOG}:{} reads \"{}\" as {}, and {} writes it to mean {}. The \
                             alarm would name the wrong half.",
                            hit.line,
                            s.text,
                            if hit.healthy { "healthy" } else { "unwell" },
                            s.origin,
                            if s.healthy { "healthy" } else { "unwell" },
                        ));
                    }
                }
                many => problems.push(format!(
                    "{WATCHDOG} has {} patterns matching the one sentence \"{}\" ({}): lines {}. \
                     Which one wins is then an accident of order.",
                    many.len(),
                    s.text,
                    s.origin,
                    many.iter()
                        .map(|p| p.line.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            }
        }
        for p in &patterns {
            if !used.contains(&p.line) {
                problems.push(format!(
                    "{WATCHDOG}:{} matches \"{}…\" for {leg}, and the hub writes no such sentence. \
                     A rule that can never fire is one that has stopped covering whatever it was \
                     written for.",
                    p.line, p.prefix
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "the hub and the watchdog no longer say the same thing:\n  - {}",
        problems.join("\n  - ")
    );
}

/// The stamp is what the verdict earned, and the verdict is both legs.
///
/// The whole of item 4 in two lines of source: a Bot API round trip on its own used to write the
/// file, so a hub whose socket never opened reported health for a week. Pinned as text because the
/// failure is a one-word edit — an `||` for an `&&`, or a second caller of `stamp` that does not ask.
#[test]
fn a_stamp_is_written_only_where_the_verdict_earned_it_and_a_verdict_needs_both_legs() {
    let heartbeat = read(HEARTBEAT);

    let at = heartbeat
        .find("pub fn earned(&self) -> bool")
        .expect("heartbeat.rs no longer has Verdict::earned; the stamp's one rule has moved");
    let body = &heartbeat[at..at + 200];
    assert!(
        body.contains("self.phone_line.is_fresh() && self.door.is_fresh()"),
        "Verdict::earned no longer requires BOTH legs. One leg is the defect item 4 exists for: \
         Telegram answering while the agents' door is shut is the state that looked green for a \
         week. Found: {body}"
    );

    let at = heartbeat
        .find("pub fn stamp_if(")
        .expect("heartbeat.rs no longer has stamp_if; something else is deciding to stamp");
    let body = &heartbeat[at..at + 300];
    assert!(
        body.contains("if !verdict.earned() {") && body.contains("return Ok(false)"),
        "stamp_if no longer refuses an unearned verdict. Withholding is the ONLY signal the \
         watchdog can hear. Found: {body}"
    );

    // The binary must have exactly one way to touch that file, and it must be the one that asks.
    let bot = read(BOT);
    assert!(
        !bot.replace("stamp_if", "").contains(".stamp("),
        "{BOT} stamps the heartbeat directly. Every stamp goes through stamp_if, or a caller that \
         has not asked will report health the hub cannot prove."
    );
}

/// The four numbers the two languages share, and the relations between them.
///
/// They are duplicated on purpose — the watchdog may not depend on this binary, which is why it can
/// still speak when this binary cannot — so the relations are what has to be checked. Slowing the
/// hub's tick without moving the rest is the realistic edit, and it degrades the alarm silently.
#[test]
fn the_windows_the_hub_and_the_watchdog_keep_are_still_in_proportion_to_one_another() {
    fn secs(text: &str, what: &str, needle: &str, open: &str, close: &str) -> u64 {
        let at = text
            .find(needle)
            .unwrap_or_else(|| panic!("{what}: `{needle}` is gone; this guard cannot read it"));
        let rest = &text[at..];
        let from = rest.find(open).unwrap_or_else(|| {
            panic!("{what}: `{needle}` no longer has a value this guard can read")
        }) + open.len();
        let to = from
            + rest[from..]
                .find(close)
                .unwrap_or_else(|| panic!("{what}: `{needle}`'s value does not end"));
        rest[from..to]
            .trim()
            .parse()
            .unwrap_or_else(|e| panic!("{what}: `{needle}` is not a whole number of seconds ({e})"))
    }

    let fresh_for = secs(
        &read(HEARTBEAT),
        HEARTBEAT,
        "pub const FRESH_FOR",
        "from_secs(",
        ")",
    );
    let tick = secs(&read(BOT), BOT, "const WATCHDOG_TICK", "from_secs(", ")");
    let script = read(WATCHDOG);
    let stale_after = secs(&script, WATCHDOG, "STALE_AFTER=\"$", ":-", "}");
    let note_trusted = secs(&script, WATCHDOG, "NOTE_TRUSTED_FOR=\"$", ":-", "}");
    let doctor_stale = secs(
        &read(DOCTOR),
        DOCTOR,
        "const WATCHDOG_STALE_AFTER",
        "=",
        ";",
    );

    assert_eq!(
        fresh_for,
        2 * tick,
        "a fact stays fresh for {fresh_for}s while the hub asks every {tick}s. Two ticks is what \
         makes one missed answer not an alarm and two an alarm; any other ratio changes that \
         without anybody deciding to."
    );
    assert_eq!(
        note_trusted,
        2 * tick,
        "the watchdog believes hub.health for {note_trusted}s while the hub rewrites it every \
         {tick}s. Wider, and a dead hub's last words are read as a live report — it says \"the bot \
         is still answering Telegram\" when there is no bot at all."
    );
    assert!(
        stale_after >= fresh_for + tick,
        "the alarm fires after {stale_after}s of silence, but a leg that fails is stamped over for \
         up to {fresh_for}s and the next tick is {tick}s after that. A narrower window alarms about \
         a hub that has not had a chance to withhold anything yet."
    );
    assert_eq!(
        doctor_stale, stale_after,
        "`herdr-tg doctor` says the alarm fires after {doctor_stale}s and the watchdog fires after \
         {stale_after}s. The operator reads doctor to find out whether his phone should have \
         buzzed."
    );
}

/// `1min`, `10s`, `2min` — the only shapes this project's timer uses, in seconds.
///
/// A shape it does not know is a failure, not a zero: reading an unfamiliar interval as nothing
/// would quietly shrink the wait this guard computes and let a false promise through.
fn systemd_seconds(value: &str, what: &str) -> u64 {
    let v = value.trim();
    let (n, mult) = match () {
        _ if v.ends_with("min") => (v.trim_end_matches("min"), 60),
        _ if v.ends_with('s') => (v.trim_end_matches('s'), 1),
        _ => panic!("{what}: `{v}` is not an interval this guard knows how to read"),
    };
    n.trim()
        .parse::<u64>()
        .unwrap_or_else(|e| panic!("{what}: `{v}` has no whole number in it ({e})"))
        * mult
}

/// One directive out of a systemd unit, or a failure saying this guard is blind.
fn directive(text: &str, key: &str, what: &str) -> String {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(&format!("{key}=")))
        .next_back()
        .unwrap_or_else(|| panic!("{what}: there is no {key}= in it"))
        .trim()
        .to_owned()
}

/// The small numbers a comment writes as words, and the same numbers written as digits.
fn a_number_in_words(word: &str) -> Option<u64> {
    const WORDS: [&str; 13] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ];
    WORDS
        .iter()
        .position(|w| *w == word)
        .map(|n| n as u64)
        .or_else(|| word.parse().ok())
}

/// The wait the hub's own comments promise the operator has to be one its numbers can meet.
///
/// These comments are how the next person decides whether a report of "my phone never buzzed" is a
/// bug or a wait, and how a dispatcher's restart policy picks its own timeout. They said three
/// minutes; four numbers in three files add up to nearly five, and every one of them can move
/// without anybody thinking of the sentence. So the sentence is derived from them here instead of
/// being trusted.
#[test]
fn the_wait_the_hub_promises_before_his_phone_buzzes_is_one_its_own_numbers_can_meet() {
    let bot = read(BOT);
    let script = read(WATCHDOG);
    let timer = read(TIMER);

    // A leg that fails the instant after a good answer is stamped over by the tick that follows —
    // it is still fresh then — and withheld by the one after that, so the last stamp is one whole
    // tick late. The alarm then wants the staleness window on top of THAT, and it only looks when
    // the timer lets it.
    let tick = {
        let at = bot
            .find("const WATCHDOG_TICK")
            .expect("bot.rs no longer has WATCHDOG_TICK; this guard cannot compute the wait");
        let from = at + bot[at..].find("from_secs(").expect("a value") + "from_secs(".len();
        let to = from + bot[from..].find(')').expect("a value that ends");
        bot[from..to].trim().parse::<u64>().expect("whole seconds")
    };
    let stale_after = {
        let at = script
            .find("STALE_AFTER=\"$")
            .expect("the watchdog no longer has STALE_AFTER");
        let from = at + script[at..].find(":-").expect("a default") + 2;
        let to = from + script[from..].find('}').expect("a default that ends");
        script[from..to]
            .trim()
            .parse::<u64>()
            .expect("whole seconds")
    };
    let period = systemd_seconds(&directive(&timer, "OnUnitActiveSec", TIMER), TIMER);
    let accuracy = systemd_seconds(&directive(&timer, "AccuracySec", TIMER), TIMER);
    let worst = tick + stale_after + period + accuracy;
    let honest_minutes = worst.div_ceil(60);

    // Every "within <n> minute(s)" in bot.rs is a promise to the reader. A leading "about" is
    // skipped so the sentence can be written the way a person says it.
    let mut promises = Vec::new();
    for at in bot.match_indices("within ").map(|(i, _)| i) {
        let rest = &bot[at + "within ".len()..];
        let mut words = rest.split_whitespace();
        let mut first = words.next().unwrap_or_default();
        if first == "about" {
            first = words.next().unwrap_or_default();
        }
        let Some(n) = a_number_in_words(first) else {
            continue; // "within a second", "within one conversation" — not a promise of minutes
        };
        if words.next().is_some_and(|w| w.starts_with("minute")) {
            promises.push((bot[..at].lines().count(), n));
        }
    }
    assert!(
        !promises.is_empty(),
        "{BOT} promises the operator no wait at all before his phone buzzes. That is allowed — a \
         promise nobody made cannot be wrong — but this guard then proves nothing, so it says so."
    );
    for (line, promised) in promises {
        assert!(
            promised * 60 >= worst,
            "{BOT}:{line} tells the reader his phone buzzes within {promised} minutes. The hub's \
             own numbers give {worst}s — {tick}s before the last stamp is withheld, {stale_after}s \
             of silence before the alarm, and up to {period}s + {accuracy}s before the alarm looks \
             — which is about {honest_minutes} minutes. A wait stated shorter than it is turns a \
             working alarm into a bug report."
        );
    }
}

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
//! The hub writes `hub.health`: a word, then one sentence per half. The watchdog reads those three
//! sentences ONLY to word an alarm it has already decided to raise — but wording it wrong is not a
//! small failure. Rename the sentence that means "this half is fine" and the script stops
//! recognising it, so every outage is reported as total AND the alarm quotes, as the evidence of
//! the fault, the line saying that half is healthy. It was proved silent:
//! rewording `the phone line answered` in `heartbeat.rs` *and its assertions* — a coherent tidy-up,
//! which is how this actually happens — left the Rust suite green and the shell suite green, and
//! turned a dead door into an alarm saying the phone line was down and quoting the hub saying it had
//! answered twelve seconds ago.
//!
//! # Five halves, three per plane
//!
//! A hub reaches him one of two ways and holds three halves either way: what carries an agent's
//! words to him (his phone line, or the app's copy of what agents say), the agents' door — the
//! same half, same writer and same sentences on both — and what carries his own words back to an
//! agent (the stream his taps come down, or the sweep of the drop his answers land in). Five
//! halves in all, and the watchdog has a `case` block for each.
//!
//! **Which plane a note came from is decided by which of those blocks recognised its first line**,
//! so a sentence of one half that any OTHER half's patterns also match would put the alarm on the
//! wrong plane and give him a sentence about a phone he does not use. That is checked here too.
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

/// Where the hub's facts and their sentences are written.
const HEARTBEAT: &str = "crates/kickoff-channel/src/heartbeat.rs";
/// Where the door's troubles are worded, and where the tick that rewrites the note lives.
const BOT: &str = "crates/kickoff-channel/src/bot.rs";
/// Where the answers sweep's refusals are worded — beside the code that is refused, exactly as the
/// door's are worded beside the code that opens it.
const HUB: &str = "crates/kickoff-channel/src/hub.rs";
/// The other language. Its literals are the second copy this guard exists to hold still.
const WATCHDOG: &str = "deploy/herdr-tg-watchdog.sh";
/// The one place in this binary that reads the stamp back, and holds a copy of the alarm's window.
const DOCTOR: &str = "crates/kickoff-channel/src/cmd/doctor.rs";
/// How often the alarm is allowed to look, which is the last term in the wait he actually gets.
const TIMER: &str = "deploy/kickoff-channel-watchdog.timer";
/// The unit that brings the hub back, which is what keeps a failed boot's note young enough to read.
const SERVICE: &str = "deploy/kickoff-channel.service";

/// The workspace root: `crates/kickoff-channel/` → up two.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kickoff-channel sits two levels below the workspace root")
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

/// One half of a control plane, as the two languages name it.
///
/// The Rust side is a function name rather than a position in the file, which is what it used to
/// be: with three halves the order they appeared in was a workable way to tell them apart, and
/// with five — two of them on the other plane — it is a guard holding the wrong sentences against
/// the wrong `case` block and reporting agreement.
struct Half {
    /// What a failure here calls this half, in the words a person would use.
    leg: &'static str,
    /// The function in `heartbeat.rs` that words its three sentences for the note.
    worded_by: &'static str,
    /// The variable the watchdog reads its line into, and the state it assigns.
    said: &'static str,
    state: &'static str,
}

/// Every half either plane has. The door is one half and appears once: same writer, same
/// sentences, same `case` block, whichever way the hub reaches him.
const HALVES: [Half; 5] = [
    Half {
        leg: "the phone line",
        worded_by: "the_phone_line_says",
        said: "said_phone",
        state: "phone_state",
    },
    Half {
        leg: "the agents' door",
        worded_by: "the_agents_door_says",
        said: "said_door",
        state: "door_state",
    },
    Half {
        leg: "the stream his taps come down",
        worded_by: "the_stream_of_his_taps_says",
        said: "said_updates",
        state: "updates_state",
    },
    Half {
        leg: "the app's copy of what agents say",
        worded_by: "the_ring_says",
        said: "said_ring",
        state: "ring_state",
    },
    Half {
        leg: "the sweep of his answers",
        worded_by: "the_answers_sweep_says",
        said: "said_sweep",
        state: "sweep_state",
    },
];

/// The body of a function in an `impl` block, by name, or a failure saying this guard is blind.
fn body_of(text: &str, what: &str, name: &str) -> String {
    let at = text.find(&format!("fn {name}")).unwrap_or_else(|| {
        panic!(
            "{what}: there is no `fn {name}`. This guard reads that function's sentences and holds \
             them against the watchdog's copy of them, and it will not guess where they went."
        )
    });
    let end = at
        + text[at..]
            .find("\n    }")
            .unwrap_or_else(|| panic!("{what}: `fn {name}` does not end where this guard expects"));
    text[at..end].to_owned()
}

/// Every sentence the hub can put on this half's line of the note.
///
/// Two shapes of leg, and a third is a failure rather than a guess. A leg worded by `Leg::of` says
/// one thing when it has never been good, one when it is fresh and one when it is stale, and the
/// last two are prefixes the code completes with an age — so they are completed here too, because
/// the watchdog matches the whole sentence and not the prefix. A leg worded by
/// `Leg::of_what_is_true` is a state and has no age at all: one sentence for "nobody has said",
/// one for well, one for unwell.
fn the_sentences_one_half_can_write(half: &Half) -> Vec<Sentence> {
    let heartbeat = read(HEARTBEAT);
    let body = body_of(&heartbeat, HEARTBEAT, half.worded_by);
    let by_the_clock = body.contains("Leg::of(");
    let by_its_state = body.contains("Leg::of_what_is_true(");
    assert!(
        by_the_clock != by_its_state,
        "{HEARTBEAT}: `fn {}` words {} with {}. This guard knows two shapes of leg — one measured \
         by a clock and one that is a state — and a third is one the watchdog has to learn too.",
        half.worded_by,
        half.leg,
        if by_the_clock {
            "both shapes at once"
        } else {
            "neither shape"
        }
    );
    let needle = if by_the_clock {
        "Leg::of("
    } else {
        "Leg::of_what_is_true("
    };
    let at = body.find(needle).expect("just found above");
    let args = call_args(&body, at).unwrap_or_else(|| {
        panic!(
            "{HEARTBEAT}: the call wording {} does not close; this guard cannot read its sentences",
            half.leg
        )
    });
    let literals = string_literals(args);
    assert_eq!(
        literals.len(),
        3,
        "{HEARTBEAT}: {} is worded with {} sentences, not the three this guard knows how to \
         classify. A fourth shape of sentence is one the watchdog has to learn and so does this \
         guard.",
        half.leg,
        literals.len()
    );
    // The never-yet-good sentence is in the same place either way, and it is NOT healthy: a hub
    // that has said nothing about a half has not proved it.
    let mut out = vec![Sentence {
        text: literals[0].clone(),
        healthy: false,
        origin: format!(
            "{HEARTBEAT} fn {}, the nothing-said-yet sentence",
            half.worded_by
        ),
    }];
    if by_the_clock {
        out.push(Sentence {
            text: format!("{} 12 seconds ago", literals[1]),
            healthy: true,
            origin: format!("{HEARTBEAT} fn {}, the fresh sentence", half.worded_by),
        });
        out.push(Sentence {
            text: format!("{} 5 minutes ago", literals[2]),
            healthy: false,
            origin: format!("{HEARTBEAT} fn {}, the stale sentence", half.worded_by),
        });
    } else {
        out.push(Sentence {
            text: literals[1].clone(),
            healthy: true,
            origin: format!("{HEARTBEAT} fn {}, the well sentence", half.worded_by),
        });
        out.push(Sentence {
            text: literals[2].clone(),
            healthy: false,
            origin: format!("{HEARTBEAT} fn {}, the unwell sentence", half.worded_by),
        });
    }
    out.extend(the_troubles_of(half));
    out
}

/// The named failures this half can report, worded where the reason is known rather than where the
/// sentence is carried.
///
/// Each list has a floor, because the floor is the property: two ways a thing fails that a person
/// would act on differently are two sentences, and collapsing them into one is how an alarm starts
/// sending him to the wrong machine.
fn the_troubles_of(half: &Half) -> Vec<Sentence> {
    let mut out: Vec<Sentence> = Vec::new();
    let mut take = |text: String, origin: String| {
        out.push(Sentence {
            text,
            healthy: false,
            origin,
        })
    };
    match half.leg {
        "the phone line" => {
            let heartbeat = read(HEARTBEAT);
            let troubles = string_literals(&body_of(
                &heartbeat,
                HEARTBEAT,
                "the_phone_line_did_not_answer",
            ));
            assert_eq!(
                troubles.len(),
                1,
                "{HEARTBEAT}: the_phone_line_did_not_answer holds {} sentences, not one",
                troubles.len()
            );
            take(
                troubles[0].clone(),
                format!("{HEARTBEAT} the_phone_line_did_not_answer"),
            );
        }
        "the agents' door" => {
            // Every way the door can be shut, and every way a knock can fail. Both are worded in
            // bot.rs, where the reason is known; heartbeat.rs only carries them.
            let bot = read(BOT);
            let mut shut = 0;
            for at in bot.match_indices("Door::Shut(").map(|(i, _)| i) {
                let args = call_args(&bot, at)
                    .unwrap_or_else(|| panic!("{BOT}: a Door::Shut does not close"));
                for lit in string_literals(args) {
                    shut += 1;
                    take(lit, format!("{BOT} Door::Shut"));
                }
            }
            assert!(
                shut >= 3,
                "{BOT}: only {shut} ways the door can be shut carry a sentence. Every one of them \
                 is a way the operator's agents reach nobody while his phone still answers, and \
                 each needs words the watchdog can repeat."
            );
            let mut knocks = 0;
            for at in bot
                .match_indices("the_door_is_not_answering(")
                .map(|(i, _)| i)
            {
                let args = call_args(&bot, at).unwrap_or_else(|| {
                    panic!("{BOT}: a the_door_is_not_answering call does not close")
                });
                for lit in string_literals(args) {
                    knocks += 1;
                    take(lit, format!("{BOT} the_door_is_not_answering"));
                }
            }
            assert!(
                knocks >= 2,
                "{BOT}: the knock at the door words fewer than two failures. It can fail to reach \
                 the socket at all and it can reach one nobody is accepting on, and those are \
                 different sentences to the operator."
            );
        }
        "the stream his taps come down" => {
            // Every way Telegram can refuse to hand the updates over. Worded in bot.rs beside the
            // error it is reading — a second copy of this bot holding the update slot and a hub
            // being turned away for any other reason are different mornings and the same silence.
            let bot = read(BOT);
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
                    take(lit, format!("{BOT} the_update_stream_was_refused"));
                }
            }
            assert!(
                refusals >= 2,
                "{BOT}: the update stream words fewer than two refusals. A second copy of this bot \
                 taking his taps is a thing he can go and fix in ten seconds and any other refusal \
                 is not, so they are different sentences to the operator."
            );
        }
        "the app's copy of what agents say" => {
            // None, and that is the shape of this half rather than an omission: it is a state with
            // two values, and the sentence for the unwell one is the whole of what it can say.
        }
        "the sweep of his answers" => {
            // Worded in hub.rs, beside the code that is refused, exactly as the door's are worded
            // beside the code that opens it.
            let hub = read(HUB);
            let mut refusals = 0;
            for at in hub.match_indices("Sweep::WasRefused(").map(|(i, _)| i) {
                let args = call_args(&hub, at)
                    .unwrap_or_else(|| panic!("{HUB}: a Sweep::WasRefused does not close"));
                for lit in string_literals(args) {
                    refusals += 1;
                    take(lit, format!("{HUB} Sweep::WasRefused"));
                }
            }
            assert!(
                refusals >= 2,
                "{HUB}: the answers sweep words fewer than two refusals. A drop this hub may not \
                 trust and a drop it cannot list are different mornings and the same silence — \
                 every answer he gives going nowhere."
            );
        }
        other => panic!("this guard has no idea where {other}'s named failures are worded"),
    }
    out
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
    let mut problems: Vec<String> = Vec::new();

    for half in &HALVES {
        let leg = half.leg;
        let sentences = the_sentences_one_half_can_write(half);
        let patterns = how_the_watchdog_reads(half.said, half.state);
        let sentences = &sentences;
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

/// No sentence of one half is read as another half's — which is how the alarm tells the planes
/// apart.
///
/// The note's three lines are read by position, and the first line is his phone line on one plane
/// and the app's copy of what agents say on the other. The watchdog decides WHICH by seeing which
/// of those two `case` blocks recognised the line. So a sentence both blocks match is not a near
/// miss: it is an app box sent an alarm about a phone he does not use, or a phone box told its app
/// has stopped — with the closing line naming a fix for the wrong machine, on the one message that
/// has to be trusted.
///
/// It is also what keeps the door's two neighbours honest. The door's own sentences are quoted
/// under whichever half failed, and a door sentence that a sweep pattern also matched would put
/// the door's words under the answers line of the alarm.
#[test]
fn no_sentence_of_one_half_is_read_as_another_halfs() {
    let every_block: Vec<(&Half, Vec<Pattern>)> = HALVES
        .iter()
        .map(|h| (h, how_the_watchdog_reads(h.said, h.state)))
        .collect();
    let mut problems: Vec<String> = Vec::new();

    for half in &HALVES {
        for s in the_sentences_one_half_can_write(half) {
            for (other, patterns) in &every_block {
                if std::ptr::eq(*other, half) {
                    continue;
                }
                for p in patterns {
                    if s.text.starts_with(&p.prefix) {
                        problems.push(format!(
                            "\"{}\" ({}) is {}'s sentence, and {WATCHDOG}:{} reads it as {}'s. \
                             The alarm works out which way the hub reaches him from exactly this, \
                             so it would name the wrong half on the wrong plane.",
                            s.text, s.origin, half.leg, p.line, other.leg
                        ));
                    }
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "two halves' sentences have grown together:\n  - {}",
        problems.join("\n  - ")
    );
}

/// The stamp is what the verdict earned, and the verdict is every leg it holds.
///
/// The whole of item 4 in two lines of source: a Bot API round trip on its own used to write the
/// file, so a hub whose socket never opened reported health for a week. Pinned as text because the
/// failure is a one-word edit — an `any` for an `all`, a leg indexed out of the list, or a second
/// caller of `stamp` that does not ask.
#[test]
fn a_stamp_is_written_only_where_the_verdict_earned_it_and_a_verdict_needs_every_leg_it_holds() {
    let heartbeat = read(HEARTBEAT);

    let body = body_of(&heartbeat, HEARTBEAT, "earned(&self) -> bool");
    assert!(
        body.contains("self.legs.iter().all(Leg::is_fresh)"),
        "Verdict::earned is no longer every leg the verdict holds. A subset is the defect item 4 \
         exists for: Telegram answering while the agents' door was shut is the state that looked \
         green for a week, and an app-plane hub whose answers sweep had died would look just as \
         green. Found: {body}"
    );
    assert!(
        !body.contains(".any("),
        "Verdict::earned is an `any` over its legs, so ONE working half now stamps for all of \
         them. Found: {body}"
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
    let secs = secs_between;

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
    let doctor = read(DOCTOR);
    let doctor_stale = secs(&doctor, DOCTOR, "const WATCHDOG_STALE_AFTER", "=", ";");
    let disarm_expires = secs(&script, WATCHDOG, "DISARM_EXPIRES=\"$", ":-", "}");
    let doctor_disarm = secs(&doctor, DOCTOR, "const WATCHDOG_DISARM_EXPIRES", "=", ";");
    let doctor_note = secs(&doctor, DOCTOR, "const WATCHDOG_NOTE_TRUSTED_FOR", "=", ";");
    let doctor_quiet = secs(
        &doctor,
        DOCTOR,
        "const WATCHDOG_QUIET_FOR_TOO_LONG",
        "=",
        ";",
    );
    let checks_every = systemd_seconds(&directive(&read(TIMER), "OnUnitActiveSec", TIMER), TIMER);

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
        "`kickoff-channel doctor` says the alarm fires after {doctor_stale}s and the watchdog fires after \
         {stale_after}s. The operator reads doctor to find out whether his phone should have \
         buzzed."
    );
    assert_eq!(
        doctor_disarm, disarm_expires,
        "`kickoff-channel doctor` says a silence wears off after {doctor_disarm}s and the watchdog lets \
         it run for {disarm_expires}s. Drift here has doctor calling a switched-off alarm one that \
         will buzz him, which is the morning this whole reading was added to prevent."
    );
    assert_eq!(
        doctor_note, note_trusted,
        "`kickoff-channel doctor` believes the hub's note for {doctor_note}s and the watchdog believes it \
         for {note_trusted}s. Wider, and doctor quotes a dead hub's last words in the present \
         tense while the alarm has already stopped doing so."
    );
    assert_eq!(
        doctor_quiet,
        3 * checks_every,
        "`kickoff-channel doctor` calls the alarm stopped after {doctor_quiet}s of not looking, and the \
         timer runs it every {checks_every}s. Three missed checks is what makes that a stopped \
         unit rather than a slow minute; any other multiple says 'the alarm has stopped' about one \
         that is simply between checks, or stays silent about one that really has."
    );
}

/// A hub that dies on the way up leaves a note naming the half that failed, and the alarm reads
/// that note only while it is young. Nothing inside the binary keeps it young — the process is
/// gone. What keeps it young is this unit bringing the hub back to write it again.
///
/// Without that, a box whose phone line is down alarms with "the herd has gone quiet, and it is
/// not saying which part stopped" instead of naming the phone line, which is the difference
/// between a machine he has to go and look at and one he already knows the answer for. So the
/// coupling is checked rather than trusted: restart always, and sooner than the alarm stops
/// believing what the last boot managed to say.
#[test]
fn a_boot_that_fails_is_restarted_before_the_alarm_stops_believing_the_note_it_left() {
    let unit = read(SERVICE);
    let restart = directive(&unit, "Restart", SERVICE);
    assert_eq!(
        restart, "always",
        "{SERVICE} restarts `{restart}`. A hub that cannot reach Telegram writes down which half \
         is down and exits; only a restart rewrites that note, and the alarm reads it for ninety \
         seconds. Stop restarting and the operator gets `the herd has gone quiet` about a machine \
         that told him exactly what was wrong."
    );
    let restart_after = systemd_seconds(&directive(&unit, "RestartSec", SERVICE), SERVICE);
    let note_trusted = secs_between(&read(WATCHDOG), WATCHDOG, "NOTE_TRUSTED_FOR=\"$", ":-", "}");
    assert!(
        restart_after < note_trusted,
        "{SERVICE} waits {restart_after}s before trying again and the alarm believes the note for \
         {note_trusted}s. A wait as long as that window leaves the note stale between attempts, so \
         a check landing in the gap has a hub that named its own failure and an alarm that will \
         not say it."
    );
}

/// `1min`, `10s`, `2min`, `5` — the only shapes this project's units use, in seconds.
///
/// A shape it does not know is a failure, not a zero: reading an unfamiliar interval as nothing
/// would quietly shrink the wait this guard computes and let a false promise through. A bare
/// number is seconds to systemd, and `RestartSec=5` is written that way.
fn systemd_seconds(value: &str, what: &str) -> u64 {
    let v = value.trim();
    let (n, mult) = match () {
        _ if v.ends_with("min") => (v.trim_end_matches("min"), 60),
        _ if v.ends_with('s') => (v.trim_end_matches('s'), 1),
        _ if v.chars().all(|c| c.is_ascii_digit()) && !v.is_empty() => (v, 1),
        _ => panic!("{what}: `{v}` is not an interval this guard knows how to read"),
    };
    n.trim()
        .parse::<u64>()
        .unwrap_or_else(|e| panic!("{what}: `{v}` has no whole number in it ({e})"))
        * mult
}

/// One number out of a literal in either language, however it is punctuated.
///
/// The same reader the proportions test uses, as a free function because two guards now depend on
/// the watchdog's own constants and a second copy of this would be the drift it exists to catch.
fn secs_between(text: &str, what: &str, needle: &str, open: &str, close: &str) -> u64 {
    let at = text
        .find(needle)
        .unwrap_or_else(|| panic!("{what}: `{needle}` is gone; this guard cannot read it"));
    let rest = &text[at..];
    let from = rest
        .find(open)
        .unwrap_or_else(|| panic!("{what}: `{needle}` no longer has a value this guard can read"))
        + open.len();
    let to = from
        + rest[from..]
            .find(close)
            .unwrap_or_else(|| panic!("{what}: `{needle}`'s value does not end"));
    // `86_400` in Rust and `86400` in shell are the same number said two ways, and a guard that
    // could not read one of them would be a guard nobody could keep readable.
    rest[from..to]
        .trim()
        .replace('_', "")
        .parse()
        .unwrap_or_else(|e| panic!("{what}: `{needle}` is not a whole number of seconds ({e})"))
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

// ── the fourth fact: the door the operator's app reaches this machine through ────────────────────
//
// Every leg of the hub's stamp on the app plane is the hub watching itself — it writes the ring, it
// accepts at the agents' door, it lists the place his answers land — and the whole of the distance
// between the operator and those three files is a DIFFERENT program. Proved by running it: a hub
// started with no `kickoff-door` binary on the box at all stamped a green file within a minute and
// went on stamping, with an armed watchdog silent beside it, while the app was dark.
//
// So the watchdog asks the user manager, and these three hold the arrangement together: the hub
// says in its own file that it does not watch this, the script watches a unit this repo really
// ships, and the asking never becomes a telling.

/// The hub's health module, where the stamp's whole meaning is written down.
const DOOR_UNIT_FILE_PREFIX: &str = "deploy/";

/// Everything before the first `#` on each line, which for this file is its code.
///
/// An approximation, and a safe one in this direction: a `#` inside a string truncates a line
/// early, so the scan can only ever look at LESS code than there is — it cannot invent a call. The
/// header of that script discusses `systemctl --user enable` at length, so reading its comments as
/// code would make the guard below fail on the very paragraph explaining why it passes.
fn shell_code(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .map(|(i, line)| {
            let code = match line.find('#') {
                Some(at) => &line[..at],
                None => line,
            };
            (i + 1, code.to_owned())
        })
        .collect()
}

/// **The hub says in its own file that no leg of it watches the door the app reads through.**
///
/// The limit that is named is the limit that survives a refactor; the limit that is merely true is
/// the one somebody widens a sentence past on a quiet afternoon. This module already does exactly
/// this for "a tap that arrived was acted on", and the second process is a bigger gap than that
/// one: it is every word the operator would ever see.
#[test]
fn the_hub_writes_down_that_no_leg_of_its_stamp_watches_the_door_the_app_reads_through() {
    let hub = read(HEARTBEAT);
    let doc: String = hub
        .lines()
        .take_while(|l| l.starts_with("//!") || l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !doc.is_empty(),
        "{HEARTBEAT} no longer opens with a module doc, so this guard has read nothing"
    );
    assert!(
        doc.contains("kickoff-door"),
        "{HEARTBEAT} never names the program that stands between the operator and every file this \
         module's app plane reports on. A hub with no door at all earns every leg of that stamp, \
         which was proved by running one — and a reader who is not told that reads a green stamp \
         as a working app."
    );
    assert!(
        doc.contains("herdr-tg-watchdog.sh"),
        "{HEARTBEAT} names the gap without naming what closes it, so the next reader has to \
         discover for himself whether anything does"
    );
}

/// **The watchdog watches a unit this repo actually ships.**
///
/// A unit name is a string in a shell script and a file in `deploy/`, and nothing but this holds
/// the two together. It is not a hypothetical: the app plane's own unit was called `kickoff-hub`
/// for an hour, collided with another organisation's live service on this box, and was renamed —
/// and every reference to it had to be found by hand. A watchdog pointed at a unit that does not
/// exist asks about it, is told nothing, and stays quiet for ever.
#[test]
fn the_watchdog_watches_a_door_unit_this_repo_actually_ships() {
    let script = read(WATCHDOG);
    let at = script.find("DOOR_UNIT=\"$").unwrap_or_else(|| {
        panic!(
            "{WATCHDOG} no longer names the door's unit, so nothing on this box is watching the \
             program the operator's app reaches it through"
        )
    });
    let from = at + script[at..].find(":-").expect("a default") + 2;
    let to = from + script[from..].find('}').expect("a default that ends");
    let unit = script[from..to].trim();
    let shipped = workspace_root().join(format!("{DOOR_UNIT_FILE_PREFIX}{unit}"));
    assert!(
        shipped.exists(),
        "{WATCHDOG} watches `{unit}`, and this repo ships no such unit ({}). The alarm would ask \
         the user manager about a name nobody installed and be told nothing, for ever.",
        shipped.display()
    );
}

/// **The watchdog asks the user manager and never tells it anything.**
///
/// This script decides, words and throttles an alarm; it restarts nothing and kills nothing, and
/// that line is older than the door check. Asking systemd a question does not cross it — telling
/// systemd to do something does, and the two are one command apart. A `systemctl --user restart`
/// slipped in beside the question would turn the one thing that can still speak when the hub
/// cannot into another thing that acts on the box in the dark.
#[test]
fn the_watchdog_only_asks_the_user_manager_about_the_door_and_never_tells_it_anything() {
    let script = read(WATCHDOG);
    // Questions. Anything else — start, stop, restart, kill, enable, disable, reset-failed,
    // daemon-reload — is an act, and an act here is this script trying to fix what it may only
    // report.
    const ASKING: [&str; 4] = ["is-enabled", "is-active", "is-failed", "show"];
    let mut asked = 0;
    for (line, code) in shell_code(&script) {
        let Some(at) = code.find("systemctl") else {
            continue;
        };
        // `command -v systemctl` asks whether the program is there at all and runs nothing, which
        // is how the script stays silent on a box that has no user manager to ask. Counting it as
        // a call would read its redirection as the verb.
        if code[..at].trim_end().ends_with("command -v") {
            continue;
        }
        asked += 1;
        let verb = code[at + "systemctl".len()..]
            .split_whitespace()
            // Flags and redirections are not the verb. A redirection read as one would name
            // `>/dev/null` as the thing this script told systemd to do.
            .find(|w| !w.starts_with('-') && !w.starts_with('>') && !w.starts_with('<'))
            .unwrap_or("");
        assert!(
            ASKING.contains(&verb),
            "{WATCHDOG}:{line} says `systemctl … {verb}`, which is not a question. This script \
             reports and never acts, and the whole of its independence is that it cannot take the \
             box down with the thing it watches."
        );
    }
    assert!(
        asked > 0,
        "{WATCHDOG} asks the user manager nothing at all, so nothing on this box watches the \
         program the operator's app reaches the hub through. This guard reports a clean scan only \
         when there was something to scan."
    );
}

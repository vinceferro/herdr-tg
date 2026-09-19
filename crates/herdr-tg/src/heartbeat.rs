//! The hub's half of the watchdog contract.
//!
//! `deploy/herdr-tg-watchdog.sh` watches one file and buzzes the operator's phone when it stops
//! being touched. It shares no code and no process with this binary — that is the point of it, and
//! it means the two are joined only by two files and by the contract written at the top of that
//! script. This module is that contract, in code, so the two cannot drift apart silently.
//!
//! # The three facts, and why any two of them were never enough
//!
//! There are three halves to the control plane — the arithmetic is wrong and the point stands —
//! and they fail independently: what carries an agent's words to him, the door agents arrive at,
//! and what carries his own words back to an agent. The stamp used to mean "the Bot API
//! answered", which is a third of a product — a hub whose socket never opened answered it every
//! forty-five seconds while every agent on the box was talking to nobody, and the one thing
//! watching stayed quiet about it. Then it meant that and the door, which still left the third: a
//! dispatcher wedged behind a stuck handler, or a second copy of this bot holding the update slot
//! and answering every `getUpdates` with a conflict, keeps both of the first two facts perfectly
//! true while every tap the operator makes dies in silence. So [`Verdict::earned`] is all three
//! legs, every time.
//!
//! # Two planes, three legs either way
//!
//! A hub reaches him one of two ways ([`Plane`]), and the three halves are the same three halves
//! on both — it is the thing filling each of them that differs:
//!
//! | half | on his phone | in the app |
//! | --- | --- | --- |
//! | what reaches him | the phone line out to Telegram | the ring, where an agent's words are written for the app to read |
//! | the agents' door | the accept loop — the same writer and the same sentences either way |
//! | what reaches an agent | the stream his taps come back down | the sweep of the drop his answers land in |
//!
//! **A hub with no messaging app earns the stamp, and that is the whole point of the plane being
//! carried on the verdict.** The watchdog arms itself on the note as well as on the stamp, so a
//! plane that wrote the note every tick and could never stamp would buzz his phone every minute,
//! for ever, on a box where nothing is wrong — and an alarm he has learned to ignore is worse than
//! no alarm, because the morning it is right looks exactly like the six hundred mornings it was
//! not. So the verdict holds the legs its own plane really has, and [`Verdict::earned`] is an
//! `all` over every one of them. Never a subset: a subset is how a hub with a dead socket stamped
//! itself green for a week.
//!
//! **The two planes' first legs are not equally strong, and the table hides it.** On his phone
//! that leg is a real round trip to the thing that reaches him: Telegram answered this process,
//! just now, over the network. In the app it is this process writing its own file. Every leg on
//! the app's row is inside this one program, and the whole of the distance between the operator
//! and these files is somebody else — see the section below, which exists because that gap was
//! left implied for a week and a hub with no door at all stamped itself green.
//!
//! # The contract, and why each clause is load-bearing
//!
//! **Stamped only from the live work loop.** A stamp emitted by a detached timer would keep
//! reporting health for a process whose loops had wedged — a hub that is dead to the operator and
//! healthy to the watchdog. So the door's half is written by the accept loop itself
//! (`bot::hold_the_door`), and the tick only asks.
//!
//! **Updated in place, never removed.** Not even on a clean shutdown. An absent file used to mean
//! "the hub has never run", which is also what a tidy shutdown produces — and that reading turned
//! the alarm off permanently while looking exactly like correct silence. The watchdog arms itself
//! the first time it sees a hub's files, so deleting them is an *alarm*, not a disarm.
//!
//! **Never on a tmpfs.** The path comes from `$XDG_STATE_HOME` or `~/.local/state`, both of which
//! survive a reboot. A stamp under `/run` would be wiped at boot, and a wiped stamp on a
//! never-yet-armed watchdog is silence forever.
//!
//! # Withholding is the signal, which is why there is a second file
//!
//! The watchdog reads a modification time. A hub that wrote "I am unwell" into the stamp would
//! refresh that modification time on every tick and the alarm would never fire again — so an
//! unearned verdict writes **nothing** to `hub.heartbeat`. Silence is the whole signal, and a
//! watchdog installed before any of this existed therefore starts alarming on a dead control plane
//! with no update at all.
//!
//! Which half stopped still has to be sayable, because the operator's next move differs — a dead
//! phone line is a machine to go and look at, a dead door is a hub to restart while the phone in his
//! hand keeps working and tells him nothing is wrong. That goes in `hub.health` beside the stamp,
//! rewritten every tick whichever way the verdict went. It is read to *word* an alarm, never to
//! decide one: a hub sick enough to be withholding its stamp is exactly the hub whose account of
//! itself must not be allowed to overrule the stamp.
//!
//! These two files are the source of truth for the hub's health. `herdr-tg doctor` and the watchdog
//! are readers of them; neither is a second place the answer is kept.
//!
//! # What the third leg proves, and what it still does not
//!
//! It is two facts in one, because the two ways this half dies are opposite shapes. A hub that has
//! **stopped looking** — its dispatcher wedged behind a stuck handler, or the sweep of the answers
//! drop dead in a task nobody is watching — makes the leg go stale like any other. A hub that is
//! **looking and being refused** — a second copy of this bot holding the update slot, or a drop
//! this hub cannot read — never goes stale at all, so a run of refusals that outlasts the
//! freshness window is watched in its own right. One refused call is a blip and is tolerated; a
//! wall of them is the outage, and the two are told apart by nothing but how long the run has been
//! going on. Both planes' third leg is the same shape ([`GoingRound`]) for exactly that reason.
//!
//! What it still does not prove is that a tap which arrived was **acted on**. The hub is seen
//! going for updates and seen being refused; a handler that took one and hung would go on looking
//! healthy here until the wedge backed up far enough to stop the stream being driven. That is
//! narrower than it sounds — the dispatcher stops pulling from the stream as soon as one
//! conversation's worker will not take another update, so the common wedge does reach this leg —
//! but it is not the same claim, and claiming it would be the exact failure the watchdog exists to
//! prevent: a healthy-looking report from something that is not.
//!
//! # What NO leg of the app's plane proves: the second process
//!
//! **Every leg on that plane is this process watching itself.** The ring is a file this hub
//! writes; the door is a socket this hub accepts on; the sweep is a directory this hub lists. All
//! three can be perfectly true on a box where the operator's app has been dark since it was
//! installed, because the whole of the distance between him and these three files is a DIFFERENT
//! program — `kickoff-door`, with its own unit, its own binary, its own port and its own
//! credential read per request. It dies on its own: a port already taken, a binary missing after a
//! partial install, a token rotated out from under it, a crash loop. **None of that is anything
//! this module can see, and this module does not pretend otherwise.**
//!
//! It was proved by running it rather than argued: `serve --to app` started on a box with no
//! `kickoff-door` binary anywhere stamped `hub.heartbeat` within a minute and went on stamping,
//! with all three legs green, while nothing whatever stood between the operator and an empty
//! screen. That is this file's own oldest mistake — the one the module opens with, a hub whose
//! socket never opened answering its liveness check every forty-five seconds — recurring exactly
//! one process boundary further out.
//!
//! **The hub must not close it, and the closing is deliberately somewhere else.** This binary binds
//! nothing and dials nothing but the one host `summarize.rs` proves is on this machine; a hub that
//! health-checked an HTTP port would be a hub that had learned what a port is, which is the seam
//! `crates/hub-proto` and `transport.rs` exist to keep shut. Nor can the gap be inferred from what
//! the door leaves behind here: every trace it writes into the state home — a file in the answers
//! drop, a read of the write door's credential — appears only when the OPERATOR acts, so "nobody
//! has touched this for ten minutes" is a man asleep and a dead door wearing the same face, and
//! alarming on it is the crying wolf this whole file exists to avoid.
//!
//! What watches it instead is `deploy/herdr-tg-watchdog.sh`, which is outside this process by
//! design and in the same user manager as the door: it asks systemd whether the door's own unit is
//! running, on every check, and only where the operator enabled it. That is a fourth fact, it is
//! not a leg of this verdict, and it is nothing to do with the stamp — which is the point. The
//! stamp says this hub is serving. It has never said, and does not now say, that anybody can
//! reach it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::config::Plane;

/// How long one of the three facts stays good enough to stamp on.
///
/// Two of the hub's ticks (`bot::WATCHDOG_TICK`, forty-five seconds). That ratio is what makes one
/// missed answer not an alarm and two an alarm, and
/// `tests/the_heartbeat_is_earned_not_scheduled.rs` fails if either number moves without the other.
pub const FRESH_FOR: Duration = Duration::from_secs(90);

/// How long after one refused call another still counts as part of the same run of refusals.
///
/// Deliberately NOT [`FRESH_FOR`], and the gap between the two numbers is the whole point. It has
/// to be wider than the client library's own retry ceiling — teloxide waits up to sixty-four
/// seconds between attempts — or a solid wall of refusals would arrive spaced far enough apart to
/// read as a string of unrelated blips and the leg would never fail at all. It has to be narrower
/// than the window a run must outlast before it counts, or two unrelated blips just under that
/// window apart chain into one "sustained" outage that never happened — and a link dropping a call
/// every eighty-nine seconds would chain for ever and take the hub off the air for as long as it
/// lasted.
const A_LATER_REFUSAL_JOINS_THE_SAME_RUN_FOR: Duration = Duration::from_secs(70);

/// One half of the control plane: when it was last known good, and what went wrong since.
///
/// The two are kept apart on purpose. A failure does not un-say the last success — that is what
/// gives the leg its ninety seconds of tolerance, so a single missed answer at three in the morning
/// is not a phone call.
#[derive(Clone, Copy, Debug, Default)]
struct Fact {
    last_good: Option<Instant>,
    trouble: Option<&'static str>,
}

impl Fact {
    /// Good recently enough to stamp on. Only `last_good` decides this: see the type's own note.
    fn is_fresh(&self, now: Instant) -> bool {
        self.last_good
            .is_some_and(|t| now.saturating_duration_since(t) < FRESH_FOR)
    }

    fn it_worked(&mut self, now: Instant) {
        self.last_good = Some(now);
        // Cleared, or a leg that recovered would go on explaining a failure it has left behind and
        // send him to look at something that is working.
        self.trouble = None;
    }

    fn it_did_not(&mut self, why: &'static str) {
        self.trouble = Some(why);
    }
}

/// The third fact on either plane: a loop that is seen going round, and seen being turned away.
///
/// On his phone it is the stream his taps come back down; in the app it is the sweep of the drop
/// his answers land in. One type for both, because their two failure shapes are the same two
/// shapes and getting either of them wrong costs him the same thing — every word he says.
///
/// It is not a [`Fact`] because those two ways are opposite and only one of them is an absence. A
/// dispatcher that has wedged, or a sweep whose task has died, stops *going round*, which is a
/// fact going stale like any other; a second copy of this bot holding the update slot, or a drop
/// this hub cannot read, makes every pass come back refused and the going round never stops — so
/// the run of refusals has to be watched in its own right, or the leg would report health right
/// through the outage it exists to name.
#[derive(Clone, Copy, Debug, Default)]
struct GoingRound {
    /// When the loop was last seen going round.
    looked: Option<Instant>,
    /// When the run of refusals it is in now began, when it last spoke, and why. A run rather than
    /// a count, because one refused call at three in the morning is a blip and a solid minute and a
    /// half of them is an outage, and how long it has gone on is the only thing that tells them
    /// apart.
    refused_since: Option<Instant>,
    refused_last: Option<Instant>,
    why: Option<&'static str>,
}

impl GoingRound {
    fn it_went_round(&mut self, now: Instant) {
        self.looked = Some(now);
    }

    /// The loop got somewhere: one of his taps came out the far end of the stream, or one sweep of
    /// the drop finished a whole pass. The only thing that ends a run of refusals early, because
    /// it is the only proof that the far end is answering THIS hub and not another one.
    fn it_got_somewhere(&mut self, now: Instant) {
        self.looked = Some(now);
        self.refused_since = None;
        self.refused_last = None;
        self.why = None;
    }

    fn it_was_refused(&mut self, now: Instant, why: &'static str) {
        // A refusal close behind the last one continues that run; one long after it starts a new
        // one, so two unrelated blips an evening apart are never read as an outage that lasted all
        // evening. Why this window is its own number, and not the freshness window, is on the
        // constant.
        let continues = self.refused_last.is_some_and(|t| {
            now.saturating_duration_since(t) < A_LATER_REFUSAL_JOINS_THE_SAME_RUN_FOR
        });
        if !continues {
            self.refused_since = Some(now);
        }
        self.refused_last = Some(now);
        self.why = Some(why);
    }

    /// A run of refusals that began longer ago than a fact stays fresh and is still going on now.
    ///
    /// Both halves are load-bearing, and one refusal on its own can never satisfy them together —
    /// its run began and last spoke in the same instant, so the moment the first is true the second
    /// is not. Without the first, one refused call would take the hub off the air; without the
    /// second, an outage that ended would be reported for ever, because nothing but the loop
    /// getting somewhere can prove the far end is well and a quiet forum sends nothing for hours.
    fn is_a_sustained_refusal(&self, now: Instant) -> bool {
        self.refused_since
            .is_some_and(|t| now.saturating_duration_since(t) >= FRESH_FOR)
            && self
                .refused_last
                .is_some_and(|t| now.saturating_duration_since(t) < FRESH_FOR)
    }

    /// The third fact in the shape the door's is read in, so one [`Leg::of`] words them both.
    fn as_it_stands(&self, now: Instant) -> Fact {
        if self.is_a_sustained_refusal(now) {
            // No `last_good`: a leg refused every time it looks is not fresh however recently it
            // looked, and the sentence he needs is the refusal, not how long ago it last tried.
            Fact {
                last_good: None,
                trouble: self.why,
            }
        } else {
            Fact {
                last_good: self.looked,
                trouble: None,
            }
        }
    }
}

/// What one leg has to say for itself, in the words that reach the operator.
///
/// The sentence and the verdict are made together so they cannot disagree: the watchdog decides
/// which half to name by reading this sentence, and a sentence that said "unwell" for a leg the
/// stamp counted as fresh would name the wrong half of a real outage.
#[derive(Clone, Debug)]
pub struct Leg {
    said: String,
    fresh: bool,
}

impl Leg {
    /// Word up one leg: the sentence for a leg that has never been good, the one for a fresh leg,
    /// and the one for a leg that was good and is not any more. The last two are completed with an
    /// age.
    ///
    /// The three are passed in rather than derived, because they are the sentences a person reads
    /// and each half needs its own; the second copy of them lives in the watchdog's `case` blocks,
    /// and `tests/the_heartbeat_is_earned_not_scheduled.rs` holds the two copies together.
    fn of(
        fact: &Fact,
        now: Instant,
        never_good: &str,
        was_good: &str,
        no_longer_good: &str,
    ) -> Self {
        match fact.last_good {
            Some(t) if fact.is_fresh(now) => Self {
                said: format!(
                    "{was_good} {}",
                    how_long_ago(now.saturating_duration_since(t))
                ),
                fresh: true,
            },
            // A named failure beats an age: "the door could not be opened" tells him what to do and
            // "it last let something through five minutes ago" does not.
            _ => Self {
                said: match (fact.trouble, fact.last_good) {
                    (Some(why), _) => why.to_owned(),
                    (None, Some(t)) => format!(
                        "{no_longer_good} {}",
                        how_long_ago(now.saturating_duration_since(t))
                    ),
                    (None, None) => never_good.to_owned(),
                },
                fresh: false,
            },
        }
    }

    /// Word up one leg whose evidence is a STATE rather than a time: what the thing itself
    /// answers when it is asked whether it is working.
    ///
    /// Deliberately not [`Leg::of`], and the difference is the honest part. **This leg proves less
    /// than the other two and the code says so rather than dressing it up as a heartbeat.** The
    /// other two are witnessed by a loop that has to keep turning to stay fresh; this one is one
    /// question asked of the thing that does the work, and a thing that answers "yes" while wedged
    /// would be believed. It has no age for the same reason it has no window: a ring that was
    /// opened and has had nothing to write all afternoon is perfectly well, and a freshness window
    /// would call a quiet box dead every ninety seconds.
    ///
    /// Three sentences, none of them completed with an age: the one for a hub that has not said
    /// either way yet — which is where every hub starts, and it is NOT good — the one for well,
    /// and the one for unwell. The second copy of them lives in the watchdog's `case` blocks, and
    /// `tests/the_heartbeat_is_earned_not_scheduled.rs` holds the two copies together.
    fn of_what_is_true(said_so: Option<bool>, unheard: &str, well: &str, unwell: &str) -> Self {
        match said_so {
            Some(true) => Self {
                said: well.to_owned(),
                fresh: true,
            },
            Some(false) => Self {
                said: unwell.to_owned(),
                fresh: false,
            },
            None => Self {
                said: unheard.to_owned(),
                fresh: false,
            },
        }
    }

    /// The one sentence about this half, for the note, the journal and `doctor`.
    pub fn said(&self) -> &str {
        &self.said
    }

    /// Whether this half counted as working at the moment the verdict was taken.
    pub fn is_fresh(&self) -> bool {
        self.fresh
    }
}

/// "12 seconds ago", "5 minutes ago", "2 hours ago" — the way a person says it.
///
/// Seconds run to two minutes so that a leg inside its ninety-second freshness window can never be
/// described in minutes: the watchdog tells a working half from a broken one by the shape of this
/// sentence, and "one minute ago" on a healthy leg would read as the stale wording.
fn how_long_ago(since: Duration) -> String {
    let s = since.as_secs();
    if s < 120 {
        how_many(s, "second")
    } else if s < 7200 {
        how_many(s / 60, "minute")
    } else {
        how_many(s / 3600, "hour")
    }
}

fn how_many(n: u64, unit: &str) -> String {
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// The three facts of ONE plane, as they stood at one moment, with a sentence for each.
#[derive(Clone, Debug)]
pub struct Verdict {
    /// Which way the hub these facts belong to reaches him.
    ///
    /// Carried rather than worked out, for the same reason it is a required argument at the
    /// terminal: a verdict that guessed its own plane would guess wrong exactly when a leg is
    /// down, which is the one moment anything reads it.
    plane: Plane,
    /// The three halves, in the order they are written onto the note — which is the order the
    /// watchdog reads them in, and the reason they are an array rather than three named fields.
    /// Every leg this plane holds is on this list, so [`Verdict::earned`] cannot be an `all` over
    /// a subset without somebody visibly indexing past it.
    legs: [Leg; 3],
}

impl Verdict {
    /// Whether this hub may stamp the watchdog's file.
    ///
    /// Every leg the verdict holds, never a subset. This single line is the whole of the defect:
    /// an `any` here, a leg dropped from it, or a second caller that stamps without asking, is a
    /// hub reporting health for a control plane a third of which is dead — and nothing downstream
    /// can tell.
    pub fn earned(&self) -> bool {
        self.legs.iter().all(Leg::is_fresh)
    }

    /// Every half, in the order they go onto the note.
    pub fn legs(&self) -> &[Leg] {
        &self.legs
    }

    /// What carries an agent's words to him: the phone line, or the ring the app reads.
    pub fn what_reaches_him(&self) -> &Leg {
        &self.legs[0]
    }

    /// The agents' door's half — the one half both planes have, in the same place on the note.
    pub fn door(&self) -> &Leg {
        &self.legs[1]
    }

    /// What carries his own words back to an agent: the stream his taps come down, or the sweep
    /// of the drop his answers land in.
    pub fn what_reaches_an_agent(&self) -> &Leg {
        &self.legs[2]
    }

    /// Why the stamp is being withheld, in plain words, or `None` when it is not.
    ///
    /// For the journal. The operator never sees this line — his sentence is the watchdog's — but
    /// whoever reads the journal after the fact needs to know which outage this was.
    pub fn why_withheld(&self) -> Option<String> {
        // Built up rather than matched, because three legs are eight combinations and seven of
        // them are an outage — a table of that size is where the one nobody wrote gets a sentence
        // meant for its neighbour.
        let mut broken: Vec<&str> = Vec::new();
        if !self.what_reaches_him().is_fresh() {
            // The one clause the plane changes, because it is the one half that is a different
            // thing on each: a dead phone line is a phone that will not ring, and a ring nothing
            // is writing is an app that will never show him another question.
            broken.push(match self.plane {
                Plane::Telegram => "he cannot be reached on his phone",
                Plane::App => "nothing an agent says is reaching his app",
            });
        }
        if !self.door().is_fresh() {
            broken.push("no agent can reach him");
        }
        if !self.what_reaches_an_agent().is_fresh() {
            broken.push("nothing he taps or types is getting through to an agent");
        }
        if broken.is_empty() {
            None
        } else {
            Some(broken.join(", and "))
        }
    }

    /// The word on the note's first line. Plain words: a person reads this one at 3am.
    fn word(&self) -> &'static str {
        if self.earned() {
            "serving"
        } else {
            "not serving"
        }
    }
}

/// Every fact as the hub learns them, shared between the loops that earn them and the tick.
///
/// Both planes' facts live here, and only the plane's own three are ever read into a verdict: one
/// `Health` means one hub, and one hub is on one plane for the whole of its life. A hub on the
/// phone line simply never says anything about the ring or the sweep, and those legs would be
/// exactly as unearned as they are true.
///
/// It is a small lock rather than atomics because a fact is a time *and* a sentence, and the pair
/// has to move together — a reader that caught the sentence of one failure beside the timestamp of
/// another would report a half that never existed.
#[derive(Debug, Default)]
pub struct Health {
    facts: Mutex<Facts>,
}

#[derive(Debug, Default)]
struct Facts {
    phone_line: Fact,
    door: Fact,
    /// The accept loop's own count of connections it took. Not connections *dialled*: the kernel
    /// completes a connect into the backlog of a listener nobody is accepting on, so only what came
    /// out the far end is proof that the loop is still turning.
    came_through: u64,
    updates: GoingRound,
    /// Whether the ring — the app's copy of everything an agent says — is being written, and
    /// `None` until the hub has said either way.
    ///
    /// A state and not a time, because it is one. Nothing about it is a heartbeat: see
    /// [`Leg::of_what_is_true`] for what that costs and why it is still worth a leg.
    ring: Option<bool>,
    answers: GoingRound,
}

impl Facts {
    /// The phone line's half, worded.
    ///
    /// Each half is worded in its own small function so that
    /// `tests/the_heartbeat_is_earned_not_scheduled.rs` can find one by name and hold its exact
    /// sentences against the watchdog's copy of them. Pulled out of one long `verdict` where they
    /// could only be told apart by the order they appeared in, which is not a thing a guard should
    /// be trusting when getting it wrong means the alarm names the wrong half.
    fn the_phone_line_says(&self, now: Instant) -> Leg {
        Leg::of(
            &self.phone_line,
            now,
            "the phone line has not answered since this hub started",
            "the phone line answered",
            "the phone line last answered",
        )
    }

    /// The agents' door's half, worded. **The same sentences on both planes, from the same
    /// writer** — the accept loop — because it is the same door and the same failure, and giving
    /// it a second set of words would be two things to keep in step with the watchdog instead of
    /// one.
    fn the_agents_door_says(&self, now: Instant) -> Leg {
        Leg::of(
            &self.door,
            now,
            "the agents' door has let nothing through since this hub started",
            "the agents' door let a connection through",
            "the agents' door last let a connection through",
        )
    }

    /// The stream his taps come back down, worded. The phone line's plane only.
    fn the_stream_of_his_taps_says(&self, now: Instant) -> Leg {
        Leg::of(
            &self.updates.as_it_stands(now),
            now,
            "the hub has not looked for your taps since it started",
            "the hub looked for your taps",
            "the hub last looked for your taps",
        )
    }

    /// The ring's half, worded. The app's plane only.
    ///
    /// **Its sentences say "written down", never "reaching him", and that is exact rather than
    /// coy.** All this half knows is that the hub's own file is open and its last line landed. Who
    /// reads that file, whether the program that serves it to his app is running, and whether his
    /// app is showing him anything are three facts nothing in this process witnesses — see the
    /// module's last section. A reader who turns this leg into "his app is fine" is making the
    /// claim this wording refuses to make for him.
    fn the_ring_says(&self) -> Leg {
        Leg::of_what_is_true(
            self.ring,
            "the hub has not begun writing down what agents say for the app",
            "what agents say is being written down for the app",
            "what agents say cannot be written down for the app to read",
        )
    }

    /// The sweep of the drop his answers land in, worded. The app's plane only.
    fn the_answers_sweep_says(&self, now: Instant) -> Leg {
        Leg::of(
            &self.answers.as_it_stands(now),
            now,
            "the hub has not gone to collect your answers since it started",
            "the hub went to collect your answers",
            "the hub last went to collect your answers",
        )
    }
}

impl Health {
    pub fn new() -> Self {
        Self::default()
    }

    /// The facts, recovering a poisoned lock rather than panicking on it.
    ///
    /// A panic in the accept loop is precisely the outage this module exists to report; taking the
    /// hub's health reporting down with it would turn a describable failure into silence.
    fn facts(&self) -> MutexGuard<'_, Facts> {
        self.facts.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Telegram answered a real round trip just now.
    pub fn the_phone_line_answered(&self, now: Instant) {
        self.facts().phone_line.it_worked(now);
    }

    /// Telegram did not answer. Its last success stands until it goes stale.
    pub fn the_phone_line_did_not_answer(&self) {
        self.facts()
            .phone_line
            .it_did_not("the phone line did not answer when the hub last called it");
    }

    /// The door is not letting connections through, and this is why, in the operator's words.
    ///
    /// The reason is worded by the caller, in `bot.rs`, because that is where it is known: a door
    /// that never opened, a forum that was never configured and a socket nobody is accepting on are
    /// three different mornings and one silence.
    pub fn the_door_is_not_answering(&self, why: &'static str) {
        self.facts().door.it_did_not(why);
    }

    /// A connection came out the far end of the accept loop. The one writer of the door's half.
    pub fn a_connection_came_through_the_door(&self, now: Instant) {
        let mut facts = self.facts();
        facts.came_through = facts.came_through.saturating_add(1);
        facts.door.it_worked(now);
    }

    /// The hub went to Telegram for his taps and typed lines.
    ///
    /// Written by the dispatcher's own update stream as it is driven, never by the tick. A stream
    /// nobody is pulling from any more is exactly the wedge this fact exists for, and a timer
    /// would go on reporting health straight through it.
    pub fn the_hub_looked_for_updates(&self, now: Instant) {
        self.facts().updates.it_went_round(now);
    }

    /// One of his taps or typed lines came out of that stream.
    pub fn an_update_reached_the_hub(&self, now: Instant) {
        self.facts().updates.it_got_somewhere(now);
    }

    /// Telegram would not hand the updates over, and this is why, in the operator's words.
    ///
    /// Worded by the caller, in `bot.rs`, for the same reason the door's failures are: a second
    /// copy of this bot holding the update slot and a hub that cannot reach Telegram at all are two
    /// different mornings and one silence.
    pub fn the_update_stream_was_refused(&self, now: Instant, why: &'static str) {
        self.facts().updates.it_was_refused(now, why);
    }

    /// The ring is open for this run and its last line landed, so what agents say is reaching the
    /// app's copy of the conversation.
    ///
    /// Asked of the ring itself by the tick, which is the whole of what this leg is: a state read,
    /// not a loop seen turning. It is named as the weaker leg on [`Leg::of_what_is_true`] rather
    /// than quietly given a timestamp that would make it look like the other two.
    pub fn the_ring_is_writing(&self) {
        self.facts().ring = Some(true);
    }

    /// The ring is closed for this run, or its last line could not be written.
    ///
    /// Either way the operator's copy of everything has stopped growing, and an operator who
    /// cannot see a question cannot answer one — so this is a leg of the stamp and not a warning.
    pub fn the_ring_is_not_writing(&self) {
        self.facts().ring = Some(false);
    }

    /// One whole pass of the drop his answers land in finished.
    ///
    /// Written by the sweep's own loop as it goes round, never by the tick — a loop nobody is
    /// driving any more is exactly the wedge this fact exists for, and a timer voting on its
    /// behalf would report health straight through it. A pass that FINISHED, and not one that
    /// started: the two early returns in the sweep are refusals, and they say so below.
    pub fn the_answers_were_swept(&self, now: Instant) {
        self.facts().answers.it_got_somewhere(now);
    }

    /// The sweep ran and could do nothing, and this is why, in the operator's words.
    ///
    /// Worded by the caller, in `hub.rs`, for the same reason the door's failures are worded in
    /// `bot.rs`: that is where the reason is known, and a drop this hub may not trust and a drop
    /// it cannot list are two different mornings and one silence.
    pub fn the_answers_sweep_was_refused(&self, now: Instant, why: &'static str) {
        let mut facts = self.facts();
        // BOTH, and that is the difference from the update stream, where the stream's own poll
        // writes the first half. The loop is still going round — so the leg must not read as one
        // whose task has died — and it is getting nowhere, which is what makes a run of refusals
        // an outage in its own right.
        facts.answers.it_went_round(now);
        facts.answers.it_was_refused(now, why);
    }

    /// How many connections the accept loop has taken. The knock watches this, and nothing else.
    pub fn connections_that_came_through(&self) -> u64 {
        self.facts().came_through
    }

    /// The three facts of one plane, worded, as they stand now.
    ///
    /// The plane is passed in rather than remembered here, because the one caller that knows it is
    /// the one the operator told at the terminal, and a `Health` that carried a default would be a
    /// plane reachable by omission — the exact shape `Config::load` refuses for the same reason.
    pub fn verdict(&self, plane: Plane, now: Instant) -> Verdict {
        let facts = self.facts();
        // The door sits in the middle on both planes, and is built by the same call either way.
        let legs = match plane {
            Plane::Telegram => [
                facts.the_phone_line_says(now),
                facts.the_agents_door_says(now),
                facts.the_stream_of_his_taps_says(now),
            ],
            Plane::App => [
                facts.the_ring_says(),
                facts.the_agents_door_says(now),
                facts.the_answers_sweep_says(now),
            ],
        };
        Verdict { plane, legs }
    }
}

/// What the hub last wrote down about itself: the word, then one sentence per leg.
///
/// The three sentences are read by POSITION, which is what lets a reader that knows nothing about
/// planes quote them back. The field names are the phone line's names for those positions, because
/// that is the plane every reader of this type was written for; on the app's plane the first
/// carries the ring's sentence and the last the answers sweep's, and a reader that only quotes
/// them — which is all `doctor` and the watchdog do — is right either way. Anything that starts
/// deciding something from one of them has to learn the plane first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhatTheNoteSaid {
    pub word: String,
    pub phone_line: String,
    pub door: String,
    pub updates: String,
}

/// The file the watchdog watches, and the note beside it.
#[derive(Clone, Debug)]
pub struct Heartbeat {
    path: PathBuf,
}

impl Heartbeat {
    /// Points at an explicit file. Tests use this; the binary uses [`Heartbeat::default_path`].
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$XDG_STATE_HOME/herdr-tg/hub.heartbeat`, else `~/.local/state/…`.
    ///
    /// [`crate::lock::state_dir`], which is the crate's only derivation of it, and the same path
    /// the watchdog's unit hard-codes. (It used to say "the same derivation as the audit log and
    /// the routing state"; both of those files are gone.)
    ///
    /// The unit names it explicitly rather than relying on `$XDG_STATE_HOME`,
    /// because a `--user` service does not inherit the login shell's environment and the two would
    /// otherwise resolve differently — the hub stamping one file while the watchdog watched
    /// another, each of them correct and the pair useless.
    pub fn default_path() -> PathBuf {
        crate::lock::state_dir().join("hub.heartbeat")
    }

    /// The file being stamped.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The note beside it, where which-half lives. Same directory, always.
    pub fn note_path(&self) -> PathBuf {
        self.path.with_file_name("hub.health")
    }

    /// Stamp the watchdog's file, but only if every leg earned it. `Ok(false)` means withheld.
    ///
    /// The only way this binary touches that file. Withholding is the signal the watchdog can hear,
    /// so anything that stamped without asking would be an alarm quietly turned off.
    pub fn stamp_if(&self, verdict: &Verdict) -> io::Result<bool> {
        if !verdict.earned() {
            return Ok(false);
        }
        self.stamp()?;
        Ok(true)
    }

    /// Written in place rather than through a temporary and a rename. The watchdog reads only the
    /// modification time, so a torn write cannot mislead it, and an in-place write cannot produce
    /// the instant of absence that a create-and-rename briefly can.
    fn stamp(&self) -> io::Result<()> {
        self.in_the_state_dir()?;
        fs::write(&self.path, "serving\n")
    }

    /// Write down which half is unwell — every tick, whichever way the verdict went.
    ///
    /// Green notes matter as much as red ones: a note left behind by the last outage would send him
    /// to look at a door that has been open again for an hour.
    pub fn note(&self, verdict: &Verdict) -> io::Result<()> {
        self.in_the_state_dir()?;
        // Built from the list rather than from three named legs, so that the plane's own three are
        // what land on the note and no plane can quietly write one of the other's.
        let mut said = format!("{}\n", verdict.word());
        for leg in verdict.legs() {
            said.push_str(leg.said());
            said.push('\n');
        }
        fs::write(self.note_path(), said)
    }

    /// The four lines of the note, for a reader that is not the watchdog.
    ///
    /// `None` when the file is absent, short or empty — a torn read must read as "cannot say", never
    /// as "serving": this is the one file whose optimistic misreading is the failure it exists to
    /// report. A note left by a hub from before the third leg existed is three lines and reads as
    /// "cannot say" too, which is right: it cannot say anything about the leg it had never heard of,
    /// and the running hub rewrites it on its next tick.
    pub fn what_it_said(&self) -> Option<WhatTheNoteSaid> {
        let text = fs::read_to_string(self.note_path()).ok()?;
        let mut lines = text.lines();
        let said = WhatTheNoteSaid {
            word: lines.next()?.trim().to_string(),
            phone_line: lines.next()?.trim().to_string(),
            door: lines.next()?.trim().to_string(),
            updates: lines.next()?.trim().to_string(),
        };
        if said.word.is_empty()
            || said.phone_line.is_empty()
            || said.door.is_empty()
            || said.updates.is_empty()
        {
            return None;
        }
        Some(said)
    }

    /// How long ago this was stamped, or `None` if it has never been stamped or cannot be read.
    ///
    /// Only for `herdr-tg doctor`. The watchdog does its own reading, on purpose: a hub that can
    /// answer "am I alive?" is answering the one question it is not a witness to.
    pub fn age(&self) -> Option<std::time::Duration> {
        let modified = fs::metadata(&self.path).ok()?.modified().ok()?;
        std::time::SystemTime::now().duration_since(modified).ok()
    }

    fn in_the_state_dir(&self) -> io::Result<()> {
        match self.path.parent() {
            Some(dir) => crate::conversations::private_state_dir(dir).map(|_| ()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temp dir")
    }

    /// A hub in the state the operator's box is normally in: every half answering.
    fn a_healthy_hub(now: Instant) -> Health {
        let health = Health::new();
        every_half_answers(&health, now);
        health
    }

    /// All three halves, answering at one moment. Used to keep the two halves a test is NOT about
    /// out of its way: they go stale in ninety seconds, so a test that walks the clock past that
    /// without refreshing them proves nothing about the leg it meant to break.
    fn every_half_answers(health: &Health, now: Instant) {
        health.the_phone_line_answered(now);
        health.a_connection_came_through_the_door(now);
        health.the_hub_looked_for_updates(now);
    }

    /// What the hub says when a second copy of it is holding the update slot. Worded in `bot.rs`;
    /// quoted here because the sentence is what the operator gets.
    const A_SECOND_COPY: &str =
        "another copy of this bot is taking your taps, so none of them reach the agents here";

    /// Item 4, in one test. Telegram answering used to be the whole of the stamp, so a hub whose
    /// socket never opened reported health every forty-five seconds while every agent on the box
    /// was talking to nobody — and the one thing watching stayed quiet.
    #[test]
    fn the_heartbeat_is_earned_only_when_the_phone_line_and_the_agents_door_have_both_answered() {
        let t0 = Instant::now();

        let health = Health::new();
        health.the_phone_line_answered(t0);
        health.the_hub_looked_for_updates(t0);
        health.the_door_is_not_answering("the agents' door could not be opened");
        assert!(
            !health.verdict(Plane::Telegram, t0).earned(),
            "Telegram answering is not proof that any agent can reach him"
        );

        let health = Health::new();
        health.a_connection_came_through_the_door(t0);
        health.the_hub_looked_for_updates(t0);
        health.the_phone_line_did_not_answer();
        assert!(
            !health.verdict(Plane::Telegram, t0).earned(),
            "a door nobody can be told about through is not a control plane either"
        );

        assert!(a_healthy_hub(t0).verdict(Plane::Telegram, t0).earned());
    }

    /// One missed answer is tolerated for ninety seconds so a blip does not wake him at night; the
    /// second missed tick is the alarm. Both halves keep the same window.
    #[test]
    fn a_half_that_answered_a_moment_ago_is_still_good_and_one_that_answered_two_minutes_ago_is_not()
     {
        let t0 = Instant::now();
        let health = a_healthy_hub(t0);
        health.the_phone_line_did_not_answer();

        assert!(
            health
                .verdict(Plane::Telegram, t0 + Duration::from_secs(40))
                .earned(),
            "one unanswered call became an alarm; the window exists so that it does not"
        );
        let late = health.verdict(Plane::Telegram, t0 + FRESH_FOR);
        assert!(!late.earned());
        assert_eq!(
            late.what_reaches_him().said(),
            "the phone line did not answer when the hub last called it",
            "the note has to say what went wrong, not how long ago it last went right"
        );
    }

    /// Withholding is the whole signal. A word written into the stamp would refresh its
    /// modification time, and the alarm the watchdog is built on would never fire again.
    #[test]
    fn a_withheld_stamp_says_which_leg_failed_and_leaves_the_watchdogs_file_untouched() {
        let d = tmp();
        let hb = Heartbeat::new(d.path().join("hub.heartbeat"));
        let t0 = Instant::now();

        let health = Health::new();
        health.the_phone_line_answered(t0);
        health.the_door_is_not_answering(
            "no forum is configured, so no agent has anywhere to reach you",
        );
        let verdict = health.verdict(Plane::Telegram, t0);

        assert!(!hb.stamp_if(&verdict).expect("the state dir is writable"));
        assert!(
            !hb.path().exists(),
            "a hub whose door never opened stamped the watchdog's file anyway"
        );

        hb.note(&verdict).expect("the note is written");
        assert_eq!(
            fs::read_to_string(hb.note_path()).expect("the note is readable"),
            "not serving\nthe phone line answered 0 seconds ago\nno forum is configured, so no \
             agent has anywhere to reach you\nthe hub has not looked for your taps since it \
             started\n"
        );

        // And the hub that recovers stamps again, in the same file.
        let verdict = a_healthy_hub(t0).verdict(Plane::Telegram, t0);
        assert!(hb.stamp_if(&verdict).expect("stamps"));
        assert_eq!(
            fs::read_to_string(hb.path()).expect("stamped"),
            "serving\n",
            "the stamp's contents are for a human with `cat`; the watchdog reads the mtime"
        );
    }

    /// A leg that came back must stop explaining the outage it has left behind, or `doctor` sends
    /// him to look at a door that has been open again for an hour.
    #[test]
    fn a_leg_that_recovered_stops_explaining_the_failure_it_has_left_behind() {
        let t0 = Instant::now();
        let health = a_healthy_hub(t0);
        health.the_door_is_not_answering("nothing is answering at the agents' door");
        // Still inside the window, so the stamp stands; the sentence is the one being checked.
        health.a_connection_came_through_the_door(t0 + Duration::from_secs(1));

        let back = health.verdict(Plane::Telegram, t0 + Duration::from_secs(1));
        assert!(back.earned());
        assert_eq!(
            back.door().said(),
            "the agents' door let a connection through 0 seconds ago"
        );
        assert_eq!(back.why_withheld(), None);

        // And once it has gone stale again, which is the branch where the leftover sentence would
        // actually be read: a fresh leg never looks at `trouble` at all, so the property this test
        // is named for lives past the freshness window and nowhere else. What he must get here is
        // an age — the door has not answered for a while — and never the failure it recovered from
        // an hour ago, which would send him to look at a door that was fixed.
        let long_after = t0 + Duration::from_secs(1) + FRESH_FOR + Duration::from_secs(60);
        assert_eq!(
            health.verdict(Plane::Telegram, long_after).door().said(),
            "the agents' door last let a connection through 2 minutes ago"
        );
    }

    /// Two blips an evening apart must never read as one outage that lasted all evening.
    ///
    /// Nothing but an inbound update ends a run of refusals early, and a quiet forum sends none for
    /// hours — so the only thing keeping two unrelated refusals apart is how long a run goes on
    /// believing a later one belongs to it. Set that window as wide as the freshness window and one
    /// refusal every eighty-nine seconds — a link dropping in and out, not a contrivance — chains
    /// for ever, and the hub is off the air for as long as it lasts while it is going for his taps
    /// every second in between and being answered.
    #[test]
    fn two_blips_further_apart_than_a_run_survives_are_two_blips_and_not_one_sustained_outage() {
        let t0 = Instant::now();
        let health = a_healthy_hub(t0);
        health.the_update_stream_was_refused(t0, A_SECOND_COPY);

        // The stream driven every second all the way between them: the hub is asking, and getting
        // an answer, for the whole minute and a half.
        let apart = FRESH_FOR - Duration::from_secs(1);
        for s in 1..=apart.as_secs() {
            health.the_hub_looked_for_updates(t0 + Duration::from_secs(s));
        }
        health.the_update_stream_was_refused(t0 + apart, A_SECOND_COPY);

        // Where the false reading shows: the run would have "begun" at the first blip, so the
        // moment that is ninety seconds old the leg calls itself sustainedly refused.
        let at = t0 + FRESH_FOR + Duration::from_secs(5);
        every_half_answers(&health, at);
        let verdict = health.verdict(Plane::Telegram, at);
        assert!(
            verdict.what_reaches_an_agent().is_fresh(),
            "two blips a minute and a half apart, with the stream driven every second in between, \
             were read as a sustained outage: {}",
            verdict.what_reaches_an_agent().said()
        );
        assert!(verdict.earned(), "and the stamp was withheld over it");
    }

    /// And the other edge of the same window: a wall of refusals is still one wall.
    ///
    /// The window that keeps two unrelated blips apart is the same window that holds a real outage
    /// together, so it can be got wrong in both directions. teloxide waits up to sixty-four seconds
    /// between attempts on a refused long poll, so a second copy of this bot holding the update
    /// slot produces refusals spaced that far apart and no closer. Narrow the window past that and
    /// every one of them starts a fresh run, the leg never fails at all, and the alarm this whole
    /// leg exists for is silent.
    #[test]
    fn refusals_arriving_no_faster_than_the_long_polls_own_backoff_are_still_one_sustained_outage()
    {
        let t0 = Instant::now();
        let health = a_healthy_hub(t0);
        // teloxide's retry ceiling. Anything the hub can see is spaced by at most this.
        let backoff = Duration::from_secs(64);

        let mut at = t0;
        for _ in 0..5 {
            every_half_answers(&health, at);
            health.the_update_stream_was_refused(at, A_SECOND_COPY);
            at += backoff;
        }
        every_half_answers(&health, at);
        health.the_update_stream_was_refused(at, A_SECOND_COPY);

        let verdict = health.verdict(Plane::Telegram, at);
        assert_eq!(
            verdict.what_reaches_an_agent().said(),
            A_SECOND_COPY,
            "five minutes of nothing but refusals read as five unrelated blips, and the outage \
             this leg exists to name was not reported at all"
        );
        assert!(!verdict.earned());
    }

    /// The count is what the knock watches, so it must move for a connection the loop *took* and
    /// for nothing else.
    #[test]
    fn the_door_counts_connections_that_came_through_and_never_anything_else() {
        let t0 = Instant::now();
        let health = Health::new();
        assert_eq!(health.connections_that_came_through(), 0);
        health.the_phone_line_answered(t0);
        health.the_door_is_not_answering("nothing is answering at the agents' door");
        assert_eq!(
            health.connections_that_came_through(),
            0,
            "something other than the accept loop moved the door's count"
        );
        health.a_connection_came_through_the_door(t0);
        assert_eq!(health.connections_that_came_through(), 1);
    }

    /// A hub that has only just started has never been good at anything, and saying "0 seconds ago"
    /// there would be a green reading invented out of nothing.
    #[test]
    fn a_hub_that_has_only_just_started_says_so_rather_than_claiming_a_fresh_answer() {
        let t0 = Instant::now();
        let verdict = Health::new().verdict(Plane::Telegram, t0);
        assert!(!verdict.earned());
        assert_eq!(
            verdict.what_reaches_him().said(),
            "the phone line has not answered since this hub started"
        );
        assert_eq!(
            verdict.door().said(),
            "the agents' door has let nothing through since this hub started"
        );
        assert_eq!(
            verdict.what_reaches_an_agent().said(),
            "the hub has not looked for your taps since it started"
        );
    }

    /// The watchdog tells a working half from a broken one by the shape of the sentence, and it
    /// reads `the phone line answered …` as working. A fresh leg described in minutes would fall
    /// through every pattern it knows.
    #[test]
    fn a_leg_inside_its_freshness_window_is_never_described_in_minutes() {
        let t0 = Instant::now();
        let health = a_healthy_hub(t0);
        let almost_stale = health.verdict(Plane::Telegram, t0 + FRESH_FOR - Duration::from_secs(1));
        assert!(almost_stale.earned());
        assert!(
            almost_stale
                .what_reaches_him()
                .said()
                .ends_with("seconds ago"),
            "a fresh leg was worded in minutes: {}",
            almost_stale.what_reaches_him().said()
        );
        assert_eq!(how_long_ago(Duration::from_secs(1)), "1 second ago");
        assert_eq!(how_long_ago(Duration::from_secs(300)), "5 minutes ago");
        assert_eq!(how_long_ago(Duration::from_secs(7200)), "2 hours ago");
    }

    /// The note is written in place and read by two other programs, so a reader can arrive
    /// mid-write. Half a note reads as "cannot say" — never as the healthy word.
    #[test]
    fn half_a_note_is_read_as_nothing_rather_than_as_health() {
        let d = tmp();
        let hb = Heartbeat::new(d.path().join("hub.heartbeat"));
        assert_eq!(
            hb.what_it_said(),
            None,
            "a note nobody has written was read"
        );

        fs::write(
            hb.note_path(),
            "serving\nthe phone line answered 1 second ago\nthe agents' door let a connection \
             through 1 second ago\n",
        )
        .expect("writes half a note");
        assert_eq!(
            hb.what_it_said(),
            None,
            "a note missing the line about his taps was read as a whole one — which is also the \
             shape a hub from before that leg existed leaves behind"
        );

        hb.note(&a_healthy_hub(Instant::now()).verdict(Plane::Telegram, Instant::now()))
            .expect("writes the note");
        let said = hb.what_it_said().expect("a whole note reads back");
        assert_eq!(said.word, "serving");
        assert!(said.phone_line.starts_with("the phone line answered"));
        assert!(
            said.door
                .starts_with("the agents' door let a connection through")
        );
        assert!(said.updates.starts_with("the hub looked for your taps"));
    }

    #[test]
    fn stamping_creates_the_directory_and_the_file() {
        let d = tmp();
        let hb = Heartbeat::new(d.path().join("nested").join("hub.heartbeat"));
        let now = Instant::now();
        assert!(
            hb.stamp_if(&a_healthy_hub(now).verdict(Plane::Telegram, now))
                .expect("stamps")
        );
        assert!(hb.path().exists());
        hb.note(&a_healthy_hub(now).verdict(Plane::Telegram, now))
            .expect("notes");
        assert!(hb.note_path().exists());
    }

    #[test]
    fn stamping_twice_updates_the_same_file_rather_than_replacing_it() {
        // The watchdog arms itself the first time it sees this file and treats its disappearance
        // as an alarm. A stamp that unlinked and recreated would open a window where the file is
        // absent — brief, but the watchdog runs every sixty seconds forever.
        let d = tmp();
        let hb = Heartbeat::new(d.path().join("hub.heartbeat"));
        let now = Instant::now();
        let verdict = a_healthy_hub(now).verdict(Plane::Telegram, now);
        hb.stamp_if(&verdict).expect("stamps");
        let first = fs::metadata(hb.path()).expect("metadata").ino_or_len();
        hb.stamp_if(&verdict).expect("stamps again");
        let second = fs::metadata(hb.path()).expect("metadata").ino_or_len();
        assert_eq!(
            first, second,
            "the stamp replaced the file instead of updating it"
        );
    }

    #[test]
    fn the_default_path_is_the_one_the_watchdog_unit_names() {
        // Pinned against the literal in deploy/herdr-tg-watchdog.service. If either moves, the hub
        // stamps one file while the watchdog watches another and both look perfectly healthy.
        let d = tmp();
        // SAFETY-FREE: this test sets an env var, so it must not run beside another that reads it.
        // There is exactly one such test, and this is it.
        unsafe { std::env::set_var("XDG_STATE_HOME", d.path()) };
        let p = Heartbeat::default_path();
        unsafe { std::env::remove_var("XDG_STATE_HOME") };
        assert_eq!(p, d.path().join("herdr-tg").join("hub.heartbeat"));
    }

    #[test]
    fn a_never_stamped_heartbeat_has_no_age_rather_than_a_zero_one() {
        let d = tmp();
        let hb = Heartbeat::new(d.path().join("hub.heartbeat"));
        assert_eq!(hb.age(), None);
        let now = Instant::now();
        hb.stamp_if(&a_healthy_hub(now).verdict(Plane::Telegram, now))
            .expect("stamps");
        assert!(hb.age().expect("an age") < Duration::from_secs(5));
    }

    /// The third leg, in one test. A `get_me` round trip and an open door say nothing about
    /// whether the hub is still being handed the operator's taps — and the two commonest ways it
    /// stops being handed them leave both of the other legs perfectly true.
    #[test]
    fn the_verdict_is_green_only_when_telegram_answered_and_the_socket_accepted_and_the_long_poll_is_not_failing()
     {
        let t0 = Instant::now();

        let health = Health::new();
        health.the_phone_line_answered(t0);
        health.a_connection_came_through_the_door(t0);
        assert!(
            !health.verdict(Plane::Telegram, t0).earned(),
            "a hub that has never once gone for his taps was called healthy because the other two \
             halves answered"
        );

        health.the_hub_looked_for_updates(t0);
        assert!(health.verdict(Plane::Telegram, t0).earned());
    }

    /// The failure this leg was built for, and the blip it must not mistake for it.
    ///
    /// A second copy of this bot on the box takes the update slot, and Telegram answers this one's
    /// every `getUpdates` with a conflict. Its phone line answers, its door accepts, and every tap
    /// the operator makes goes to the other copy — so the stamp has to stop, and it may not stop
    /// for one refused call at three in the morning.
    #[test]
    fn a_long_poll_refused_with_a_conflict_withholds_the_heartbeat_and_a_single_transient_error_does_not_reach_the_alarm_window()
     {
        let t0 = Instant::now();
        let tick = Duration::from_secs(45);

        // A wall of them. The hub goes on looking the whole time — that is what makes this leg
        // different from every other, and why "it looked recently" cannot be the whole of it.
        let refused = Health::new();
        let mut t = t0;
        every_half_answers(&refused, t);
        refused.the_update_stream_was_refused(t, A_SECOND_COPY);
        t += tick;
        every_half_answers(&refused, t);
        refused.the_update_stream_was_refused(t, A_SECOND_COPY);
        assert!(
            refused.verdict(Plane::Telegram, t).earned(),
            "a minute of refusals is a squeeze, not an outage; withholding this early is a phone \
             call the operator did not need"
        );

        t += tick;
        every_half_answers(&refused, t);
        refused.the_update_stream_was_refused(t, A_SECOND_COPY);
        let verdict = refused.verdict(Plane::Telegram, t);
        assert!(
            !verdict.earned(),
            "every tap he made was going to another copy of this bot and the hub stamped itself \
             green: {}",
            verdict.what_reaches_an_agent().said()
        );
        assert_eq!(
            verdict.what_reaches_an_agent().said(),
            A_SECOND_COPY,
            "the note has to say which of the three halves failed, in words he can act on"
        );
        assert_eq!(
            verdict.why_withheld(),
            Some("nothing he taps or types is getting through to an agent".to_owned())
        );

        // And the blip. One refused call, then a hub working perfectly for five more minutes.
        let blip = Health::new();
        let mut t = t0;
        every_half_answers(&blip, t);
        blip.the_update_stream_was_refused(t, A_SECOND_COPY);
        for _ in 0..7 {
            t += tick;
            every_half_answers(&blip, t);
            assert!(
                blip.verdict(Plane::Telegram, t).earned(),
                "one refused call took the hub off the air {} seconds later; the window exists so \
                 that it does not",
                (t - t0).as_secs()
            );
        }
    }

    /// The other way this half dies, and the one with no error to report at all: the dispatcher
    /// stops pulling from the update stream. Nothing is refused, nothing is logged, and the hub
    /// simply stops going for his taps — so the leg has to go stale on its own.
    #[test]
    fn a_hub_that_has_stopped_going_for_updates_stops_earning_the_stamp_however_well_its_other_halves_answer()
     {
        let t0 = Instant::now();
        let health = a_healthy_hub(t0);

        let mut t = t0;
        for _ in 0..2 {
            t += Duration::from_secs(45);
            // Everything but the update stream keeps working, which is the whole difficulty.
            health.the_phone_line_answered(t);
            health.a_connection_came_through_the_door(t);
        }

        let verdict = health.verdict(Plane::Telegram, t);
        assert!(
            !verdict.earned(),
            "a hub that had not gone for an update in a minute and a half went on stamping itself \
             green"
        );
        assert!(
            verdict
                .what_reaches_an_agent()
                .said()
                .starts_with("the hub last looked for your taps"),
            "the note does not say that the hub has stopped collecting them: {}",
            verdict.what_reaches_an_agent().said()
        );
    }

    /// An outage that ended has to stop being reported, and there are two ways it can end. One of
    /// his taps arriving is proof at once; a quiet forum sends none, so a run of refusals that has
    /// simply stopped must lapse on its own or the hub would be off the air until it was restarted.
    #[test]
    fn a_run_of_refusals_stops_counting_when_a_tap_arrives_and_again_when_it_has_simply_stopped() {
        let t0 = Instant::now();
        let tick = Duration::from_secs(45);

        let arrived = Health::new();
        let mut t = t0;
        for _ in 0..3 {
            every_half_answers(&arrived, t);
            arrived.the_update_stream_was_refused(t, A_SECOND_COPY);
            t += tick;
        }
        every_half_answers(&arrived, t);
        assert!(
            !arrived.verdict(Plane::Telegram, t).earned(),
            "the outage is not being reported at all"
        );
        arrived.an_update_reached_the_hub(t);
        assert!(
            arrived.verdict(Plane::Telegram, t).earned(),
            "one of his taps came through and the hub went on reporting an outage that had ended"
        );

        let lapsed = Health::new();
        let mut t = t0;
        for _ in 0..3 {
            every_half_answers(&lapsed, t);
            lapsed.the_update_stream_was_refused(t, A_SECOND_COPY);
            t += tick;
        }
        // The other copy is gone. Nothing is refused any more and nobody is typing.
        for _ in 0..3 {
            every_half_answers(&lapsed, t);
            t += tick;
        }
        assert!(
            lapsed.verdict(Plane::Telegram, t).earned(),
            "the refusals stopped and the hub stayed off the air, so his phone buzzes about an \
             outage that is over: {}",
            lapsed
                .verdict(Plane::Telegram, t)
                .what_reaches_an_agent()
                .said()
        );
    }

    // ── the app plane ───────────────────────────────────────────────────────────────────────────

    /// An app-plane hub in the state the operator's box is normally in: every half answering.
    fn an_app_hub_that_is_well(now: Instant) -> Health {
        let health = Health::new();
        the_other_two_app_halves_answer(&health, now);
        health.the_answers_were_swept(now);
        health
    }

    /// The ring and the door, answering at one moment — the two halves the sweep's own tests are
    /// NOT about. The sweep is left out on purpose: a completed pass is the one thing that ends a
    /// run of refusals early, so a helper that took one would wipe out the very state the refusal
    /// tests are walking the clock through.
    fn the_other_two_app_halves_answer(health: &Health, now: Instant) {
        health.the_ring_is_writing();
        health.a_connection_came_through_the_door(now);
    }

    /// What the hub says when the drop his answers land in cannot be listed. Worded in `hub.rs`;
    /// quoted here because the sentence is what the operator gets.
    const THE_DROP_CANNOT_BE_READ: &str =
        "the place your answers arrive could not be read, so none of them are getting through";

    /// The whole of the app plane's stamp, in one test.
    ///
    /// A hub with no messaging app has no phone line and no stream of taps from one, and it must
    /// still earn the stamp — because the watchdog arms on the note as well as on the stamp, so a
    /// hub that writes the note every tick and can never stamp is an alarm going off every minute,
    /// for ever, on a box where nothing is wrong.
    #[test]
    fn a_hub_that_reaches_him_through_the_app_earns_its_stamp_without_a_phone_line() {
        let t0 = Instant::now();
        let health = an_app_hub_that_is_well(t0);
        assert!(
            health.verdict(Plane::App, t0).earned(),
            "a hub whose every half is answering could not stamp, so the alarm buzzes for ever: {}",
            health
                .verdict(Plane::App, t0)
                .why_withheld()
                .unwrap_or_default()
        );

        // And the phone line's legs are not quietly making up the numbers. Nothing here has ever
        // answered a messaging service or gone for a tap, and both of those legs would be stale.
        let phone = health.verdict(Plane::Telegram, t0);
        assert!(
            !phone.earned(),
            "the same facts earned the PHONE plane's stamp, so the app's legs are standing in for \
             legs this hub does not have"
        );
    }

    /// The hole this leg closes, and it is open on a Telegram box today: nothing whatsoever
    /// watches the sweep of the answers drop.
    ///
    /// A sweep task that died takes every tap and every typed line he makes in the app with it,
    /// in silence — and the two legs that already existed stay perfectly true while it happens,
    /// because agents go on connecting and what they say goes on being written down.
    #[test]
    fn a_hub_whose_answers_sweep_has_stopped_goes_stale_even_though_its_door_is_open() {
        let t0 = Instant::now();
        let health = an_app_hub_that_is_well(t0);

        let mut t = t0;
        for _ in 0..2 {
            t += Duration::from_secs(45);
            // Everything but the sweep keeps working, which is the whole difficulty.
            the_other_two_app_halves_answer(&health, t);
        }

        let verdict = health.verdict(Plane::App, t);
        assert!(
            !verdict.earned(),
            "a hub that had not been to collect an answer in a minute and a half went on stamping \
             itself green"
        );
        assert!(
            verdict
                .what_reaches_an_agent()
                .said()
                .starts_with("the hub last went to collect your answers"),
            "the note does not say that his answers have stopped being collected: {}",
            verdict.what_reaches_an_agent().said()
        );
    }

    /// The other shape of the same failure, and the one an absence cannot catch: the sweep is
    /// running and getting nowhere.
    ///
    /// The drop is a directory another process writes into, so it can be there and unreadable —
    /// a mode that was widened and cannot be narrowed again, a filesystem gone read-only. The
    /// loop goes round every second, refuses every time, and never goes stale, so the run of
    /// refusals has to be watched in its own right. One refused pass is a blip; a minute and a
    /// half of nothing else is the outage.
    #[test]
    fn a_sweep_that_is_looking_and_being_refused_is_watched_in_its_own_right() {
        let t0 = Instant::now();
        let tick = Duration::from_secs(45);
        let health = an_app_hub_that_is_well(t0);

        let mut t = t0;
        health.the_answers_sweep_was_refused(t, THE_DROP_CANNOT_BE_READ);
        t += tick;
        the_other_two_app_halves_answer(&health, t);
        health.the_answers_sweep_was_refused(t, THE_DROP_CANNOT_BE_READ);
        assert!(
            health.verdict(Plane::App, t).earned(),
            "a minute of refused passes is a squeeze, not an outage; withholding this early is a \
             phone call the operator did not need"
        );

        t += tick;
        the_other_two_app_halves_answer(&health, t);
        health.the_answers_sweep_was_refused(t, THE_DROP_CANNOT_BE_READ);
        let verdict = health.verdict(Plane::App, t);
        assert!(
            !verdict.earned(),
            "every answer he gave was going nowhere and the hub stamped itself green: {}",
            verdict.what_reaches_an_agent().said()
        );
        assert_eq!(
            verdict.what_reaches_an_agent().said(),
            THE_DROP_CANNOT_BE_READ,
            "the note has to say which of the three halves failed, in words he can act on"
        );
        assert_eq!(
            verdict.why_withheld(),
            Some("nothing he taps or types is getting through to an agent".to_owned())
        );

        // And the blip it must not mistake for that: one refused pass, then a hub sweeping
        // perfectly for five more minutes.
        let blip = an_app_hub_that_is_well(t0);
        let mut t = t0;
        blip.the_answers_sweep_was_refused(t, THE_DROP_CANNOT_BE_READ);
        for _ in 0..7 {
            t += tick;
            the_other_two_app_halves_answer(&blip, t);
            blip.the_answers_were_swept(t);
            assert!(
                blip.verdict(Plane::App, t).earned(),
                "one refused pass took the hub off the air {} seconds later; the window exists so \
                 that it does not",
                (t - t0).as_secs()
            );
        }
    }

    /// The app plane's first leg, which is a state and not a heartbeat.
    ///
    /// A hub that cannot write down what agents say has an operator who will never see a question
    /// again, however well its door and its sweep are doing — so it is a leg of the stamp like any
    /// other, and withholding is the only way the alarm hears about it.
    #[test]
    fn a_hub_that_cannot_write_down_what_agents_say_earns_no_stamp_however_open_its_door_is() {
        let t0 = Instant::now();
        let health = an_app_hub_that_is_well(t0);
        health.the_ring_is_not_writing();

        let verdict = health.verdict(Plane::App, t0);
        assert!(
            !verdict.earned(),
            "nothing an agent said would ever reach him and the hub stamped itself green"
        );
        assert_eq!(
            verdict.what_reaches_him().said(),
            "what agents say cannot be written down for the app to read"
        );
        assert_eq!(
            verdict.why_withheld(),
            Some("nothing an agent says is reaching his app".to_owned())
        );

        // A hub that has said nothing about it yet is not green either: an unproved leg has never
        // been an earned one, and at boot this is the state every leg is in.
        assert!(!Health::new().verdict(Plane::App, t0).earned());
        assert_eq!(
            Health::new()
                .verdict(Plane::App, t0)
                .what_reaches_him()
                .said(),
            "the hub has not begun writing down what agents say for the app"
        );
    }

    /// The limit this module names rather than implies, held as a fact instead of a paragraph.
    ///
    /// Every leg of the app's plane is this process watching itself, so all three are green on a
    /// box with no `kickoff-door` binary on it at all — which was proved by starting `serve --to
    /// app` on exactly such a box and watching it stamp. That is not a defect in this type and the
    /// assertions below are the CORRECT answers; what it is, is the whole of why something outside
    /// this process has to watch the door's own unit. The test exists so that anybody who later
    /// decides to make a green app stamp mean "his app is working" has to delete a stated property
    /// to do it, rather than quietly widen a sentence.
    #[test]
    fn a_fully_earned_app_stamp_says_nothing_about_the_program_the_operators_app_actually_reaches()
    {
        let t0 = Instant::now();
        // Nothing here is a door program, and nothing here could be: `Health` is told about a file
        // being written, a connection being accepted and a directory being listed, and there is no
        // third party on the box for any of the three.
        let health = an_app_hub_that_is_well(t0);
        let verdict = health.verdict(Plane::App, t0);

        assert!(verdict.earned());
        assert_eq!(verdict.why_withheld(), None);
        assert_eq!(
            verdict.what_reaches_him().said(),
            "what agents say is being written down for the app",
            "the leg claims more than the hub witnessed: it knows its own file was written and \
             nothing whatever about who reads it"
        );
        // And the wording is the guard, so it is asserted as one: a sentence that promised reach
        // would be a hub answering for a program it has never touched.
        for leg in verdict.legs() {
            assert!(
                !leg.said().contains("reaching"),
                "a leg of the app's plane promises the operator reach, which no leg of it can \
                 witness: {}",
                leg.said()
            );
        }
    }

    /// Same-inode check without pulling in a platform trait at the call site.
    trait InoOrLen {
        fn ino_or_len(&self) -> u64;
    }
    impl InoOrLen for fs::Metadata {
        fn ino_or_len(&self) -> u64 {
            use std::os::unix::fs::MetadataExt;
            self.ino()
        }
    }
}

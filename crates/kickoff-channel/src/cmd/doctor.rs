//! `kickoff-channel doctor` — is this bridge's view of herdr still valid?

use herdr_client::{HerdrClient, KNOWN_PROTOCOL, MIN_SUPPORTED_PROTOCOL};
use serde_json::{Map, Value, json};

use crate::heartbeat::Heartbeat;

/// How stale the stamp may be before the watchdog treats the hub as dead.
///
/// Mirrors `HERDR_TG_STALE_AFTER` in `deploy/herdr-tg-watchdog.sh`. Duplicated rather than shared
/// on purpose — the watchdog must have no dependency on this binary, which is the whole reason it
/// can still speak when this binary cannot. The cost of that independence is this constant, so it
/// is named here rather than buried in a comparison.
const WATCHDOG_STALE_AFTER: u64 = 180;

/// How long a silence lasts before the watchdog goes back to watching by itself.
///
/// Mirrors `HERDR_TG_DISARM_EXPIRES` in the same script, for the same reason and at the same cost.
/// It matters here because a silence he set yesterday and forgot is indistinguishable, from the
/// hub's own files, from a watchdog that is watching him closely.
const WATCHDOG_DISARM_EXPIRES: u64 = 86_400;

/// How long the watchdog may go without looking before its silence is the thing worth reporting.
///
/// Its timer runs it every minute (`deploy/kickoff-channel-watchdog.timer`), and it writes down every
/// check, so three missed ones is not a slow minute — it is a disabled unit, a login manager that
/// stopped the user's timers at logout, or a machine that was asleep. All three leave every file
/// the watchdog ever wrote sitting there saying "armed", which is why this is read and not assumed.
const WATCHDOG_QUIET_FOR_TOO_LONG: u64 = 180;

/// How old the hub's note may be before it stops being an account of a hub that is still here.
///
/// Mirrors `HERDR_TG_NOTE_TRUSTED_FOR` in the same script, for the same reason and at the same
/// cost. A running hub rewrites this file every tick; a hub that was killed leaves its last one
/// lying there for ever, with three sentences whose ages were true at the moment it wrote them.
/// Quoting those is telling him a dead hub answered Telegram twelve seconds ago. The watchdog has
/// refused to quote a note this old since the note existed; this command had no such rule.
const WATCHDOG_NOTE_TRUSTED_FOR: u64 = 90;

/// What the watchdog itself has written down, beside the hub's own two files.
///
/// Every field here is read off a file the WATCHDOG wrote. That is the whole point: the hub's
/// stamp and note say what the hub thinks of itself, and nothing about whether anything is
/// listening. This command used to answer "is the alarm armed?" and "would it go off?" from the
/// hub's files alone, which reports an armed alarm on a machine the watchdog was never installed
/// on — the commonest machine there is — and sends him away from the keyboard believing his phone
/// will buzz when it cannot.
struct WhatTheWatchdogWrote {
    /// A file the WATCHDOG writes exists, so one has really run on this machine.
    ///
    /// `watchdog.disarmed` does not count towards this: it is the one file in the set the operator
    /// creates by hand, on the alarm's own printed instruction, so it says what he wants and
    /// nothing about what is installed.
    has_run_here: bool,
    /// It has seen a hub to watch and will alarm when that hub goes quiet.
    armed: bool,
    /// When it last ran a check. It writes this every time, so a large number is a stopped timer.
    checked: Option<std::time::Duration>,
    /// Someone told it to stay quiet, and the silence has not worn off yet.
    silenced: bool,
    /// How long ago the silence was asked for, so a reader can judge the line above for itself.
    silenced_for: Option<std::time::Duration>,
    /// It wrote down that it sent an alarm — before sending it, so this is a record and not a plan.
    alarm_already_sent: bool,
    /// How much longer that written-down moment has to run, so the judgement below is checkable.
    window_ends_in: Option<std::time::Duration>,
    /// It has written down a moment before which it judges nothing, and that moment is still ahead.
    ///
    /// The script does this whenever the gap between its own checks says it was not running — a
    /// closed lid, a resume, a timer that was off — because the stamp ages on the wall clock while
    /// the checks do not. The hub gets one whole window to stamp before anything is alarmed about,
    /// and every check inside that window sends nothing however dead the hub looks.
    holding_the_hub_a_window: bool,
}

impl WhatTheWatchdogWrote {
    /// The watchdog keeps its own state in the same directory as the hub's stamp, so the stamp's
    /// directory is where to look — including in a test, which must never read the real one.
    fn beside(hb: &Heartbeat) -> Self {
        let dir = hb
            .path()
            .parent()
            .map(|d| d.to_path_buf())
            .unwrap_or_default();
        let armed = dir.join("watchdog.armed").exists();
        let tick = dir.join("watchdog.tick");
        let checked = age_of(&tick);
        let disarm = dir.join("watchdog.disarmed");
        let disarm_age = age_of(&disarm);
        let alarm_already_sent = dir.join("watchdog.latch").exists();
        Self {
            // Every file counted here is one the WATCHDOG writes. `watchdog.disarmed` is
            // deliberately not among them: the operator creates that one by hand, on the alarm's
            // own printed instruction, so it says he wants silence and nothing whatever about
            // whether anything is installed to break it.
            has_run_here: armed || checked.is_some() || alarm_already_sent,
            armed,
            checked,
            // A silence whose age cannot be read counts as a silence. The mistake that costs him a
            // morning is promising a buzz that never comes, so an unreadable file resolves toward
            // "you will hear nothing", never toward "something will tell you".
            silenced: disarm.exists()
                && disarm_age.is_none_or(|d| d.as_secs() < WATCHDOG_DISARM_EXPIRES),
            silenced_for: disarm_age.filter(|_| disarm.exists()),
            alarm_already_sent,
            window_ends_in: the_window_it_wrote_down(&tick)
                .and_then(|deadline| deadline.checked_sub(seconds_since_1970()))
                .filter(|left| *left > 0)
                .map(std::time::Duration::from_secs),
            holding_the_hub_a_window: the_window_it_wrote_down(&tick)
                .is_some_and(|deadline| deadline > seconds_since_1970()),
        }
    }

    /// Whether anything has looked recently enough for "its next check" to mean anything.
    ///
    /// The alarm only ever fires on a check, so this is what every promise about one is worth.
    fn is_looking(&self) -> bool {
        self.checked
            .is_some_and(|d| d.as_secs() < WATCHDOG_QUIET_FOR_TOO_LONG)
    }
}

/// The moment the watchdog wrote down as the one before which it judges nothing.
///
/// `watchdog.tick` is two numbers: when it last looked, and the deadline it is holding to. Read
/// rather than assumed, because a promise that the next check will alarm is false for the whole of
/// that window and this is the only place the deadline is written down.
///
/// A file with no second number is not a guess either way — the script reads a missing one as zero
/// and goes on to judge the hub — so `None` here mirrors what it would actually do.
fn the_window_it_wrote_down(tick: &std::path::Path) -> Option<u64> {
    let text = std::fs::read_to_string(tick).ok()?;
    text.split_whitespace().nth(1)?.parse().ok()
}

/// Wall-clock seconds, which is the clock the watchdog's deadline is written in.
fn seconds_since_1970() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How long ago a file was last written, or `None` if it is not there or cannot be read.
fn age_of(path: &std::path::Path) -> Option<std::time::Duration> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    std::time::SystemTime::now().duration_since(modified).ok()
}

/// "12 seconds", "5 minutes", "9 hours" — he reads this on a phone.
///
/// The same three bands as the hub's own note (`heartbeat::how_long_ago`), because one line can
/// carry both and "540 minutes" beside "2 hours ago" is a number he has to divide before he knows
/// whether this happened after dinner or before breakfast.
fn how_long(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 120 {
        format!("{secs} seconds")
    } else if secs < 7200 {
        format!("{} minutes", secs / 60)
    } else {
        format!("{} hours", secs / 3600)
    }
}

/// The same, said about the past.
fn how_long_ago(d: std::time::Duration) -> String {
    format!("{} ago", how_long(d))
}

/// Has the hub stopped saying it is serving — the one state the watchdog alarms on.
///
/// This is worked out, not read: it is this command's reading of the hub's own files, in the same
/// terms the watchdog uses. A missing stamp is only quiet if something says a hub was here at all
/// — either the hub's note, or the watchdog's record that it once saw a stamp. Otherwise this is a
/// machine no hub has run on, which is silence and not an outage.
fn the_hub_has_gone_quiet(
    stamped: Option<std::time::Duration>,
    has_note: bool,
    wd: &WhatTheWatchdogWrote,
) -> bool {
    match stamped {
        Some(age) => age.as_secs() >= WATCHDOG_STALE_AFTER,
        None => has_note || wd.armed,
    }
}

/// What is watching this machine, what the hub says about itself, and what follows from the two.
///
/// `doctor` is the command an operator runs at the keyboard when his phone has NOT buzzed and he
/// suspects it should have, so the first thing this line has to settle is whether anything would
/// have buzzed it. That is read from the watchdog's own files and never inferred: the watchdog is
/// installed separately from this binary, and a machine where it was never installed looks, from
/// the hub's files, exactly like one where it is watching closely.
///
/// The hub's half of the line stays as it was. The stamp's age alone describes a dead process and
/// a dead door in the same words, and those are different mornings: one is a machine to go and
/// look at, the other is a hub to restart while the phone in his hand keeps working and tells him
/// nothing is wrong. So the hub's own note is read too — every half of it, because "and what about
/// the other one?" is the next question, and the third half is the one whose failure looks most
/// like health from anywhere else.
fn watchdog_line(hb: &Heartbeat) -> String {
    let wd = WhatTheWatchdogWrote::beside(hb);
    let said = hb.what_it_said();
    let stamped = hb.age();
    let quiet = the_hub_has_gone_quiet(stamped, said.is_some(), &wd);

    // Observed. Every arm of this is a file the watchdog wrote.
    let watching = if !wd.has_run_here {
        "nothing is watching this hub — no alarm has ever run on this machine".to_owned()
    } else if wd.silenced {
        "the alarm has been silenced; it goes back to watching by itself within a day".to_owned()
    } else if wd.armed {
        match wd.checked {
            // Said before anything about the hub, because it changes what every word after it is
            // worth: an alarm that is not looking will not tell him about any of it.
            Some(d) if d.as_secs() >= WATCHDOG_QUIET_FOR_TOO_LONG => format!(
                "the alarm is armed but last looked {}, and it should be looking every minute — \
                 either it has been stopped or this machine was asleep",
                how_long_ago(d)
            ),
            Some(d) => format!("the alarm is watching, and last looked {}", how_long_ago(d)),
            None => "the alarm is watching".to_owned(),
        }
    } else {
        match wd.checked {
            Some(d) => format!(
                "the alarm is installed and last looked {}, and has not found a hub to watch yet",
                how_long_ago(d)
            ),
            None => "the alarm is installed and has not found a hub to watch yet".to_owned(),
        }
    };

    // The hub's own two files, which say nothing about whether anyone is listening.
    let hub = match stamped {
        // No stamp. A hub whose socket will not bind, or that has no forum, answers Telegram every
        // forty-five seconds and withholds every stamp for its whole life — so an empty stamp is
        // not an empty machine whenever the note beside it says a hub has been here.
        None if said.is_some() => {
            "a hub has run here and has never once been able to say it is serving".to_owned()
        }
        // The alarm remembers a stamp it can no longer find, which is a wiped state directory or a
        // path that moved. Both an outage and an empty-looking machine, and calling it empty sends
        // him hunting a setup mistake while the alarm that remembers is already going off.
        None if wd.armed => {
            "the hub's stamp has gone missing since the alarm last saw it".to_owned()
        }
        None => "no hub has ever run here".to_owned(),
        Some(age) if age.as_secs() < WATCHDOG_STALE_AFTER => {
            format!("the hub last said it was serving {}", how_long_ago(age))
        }
        // Not "since a hub answered": the stamp is withheld when ANY leg stops, so a hub that is
        // answering Telegram perfectly well can be the reason this number is large.
        Some(age) => format!(
            "it has been {} since the hub could say it was serving",
            how_long(age)
        ),
    };

    // What follows — and only ever in the words the two halves above support. "Your phone should
    // have buzzed" was this line's old ending, said on the strength of the stamp's age alone: on a
    // machine with no watchdog it was false, and on one that had already buzzed him it was a guess
    // that happened to be right.
    let consequence = match () {
        _ if !quiet => "",
        _ if !wd.has_run_here => ", and nothing here will tell you",
        _ if wd.silenced => ", and nothing will be sent while it is silenced",
        _ if wd.alarm_already_sent => ", and the alarm has already gone out to your phone",
        // An alarm only goes off on a check. Saying "its next check" about something that has not
        // looked for four hours put a contradiction inside one sentence — the half before the
        // semicolon said it may have been stopped — and sent him away waiting for a buzz.
        _ if !wd.is_looking() => ", and nothing will be sent until it starts looking again",
        // And it holds off deliberately after a resume: the first check back gives the hub a whole
        // window to stamp, so the next two or three send nothing about a hub that really is dead.
        _ if wd.holding_the_hub_a_window => {
            ", and it is giving the hub one more window to come back before it sends anything"
        }
        _ => ", and its next check should send you the alarm",
    };

    // The note's three sentences carry ages frozen at the moment the hub wrote them, and nothing
    // rewrites the file once that hub is gone. Past the window the watchdog itself trusts, they
    // stop being what the hub says and become the last thing it managed to say — so they are told
    // as that, and never quoted in the present tense of a process that is not running.
    match (said, age_of(&hb.note_path())) {
        (Some(said), Some(d)) if d.as_secs() < WATCHDOG_NOTE_TRUSTED_FOR => format!(
            "{watching}; {hub}{consequence}; {}, {}, and {}",
            said.phone_line, said.door, said.updates
        ),
        (Some(said), Some(d)) => format!(
            "{watching}; {hub}{consequence}; the last thing the hub said about itself was {}, and \
             it said it was {}",
            how_long_ago(d),
            said.word
        ),
        (Some(said), None) => format!(
            "{watching}; {hub}{consequence}; the last thing the hub said about itself is that it \
             was {}",
            said.word
        ),
        (None, _) => format!("{watching}; {hub}{consequence}"),
    }
}

/// The same answer for a machine — a Kickoff controller decides whether to restart the hub on
/// this, so `observed` and `inferred` are kept apart in the shape as well as in the words. Nothing
/// under `observed` is worked out, and the one judgement lives under `inferred` where a reader can
/// see it is one.
fn watchdog_json(hb: &Heartbeat) -> Value {
    let wd = WhatTheWatchdogWrote::beside(hb);
    let said = hb.what_it_said();
    let stamped = hb.age().map(|d| d.as_secs());
    let quiet = the_hub_has_gone_quiet(hb.age(), said.is_some(), &wd);
    // A watchdog that has never run here cannot alarm however dead the hub is, and neither can one
    // that has been silenced. Both were reported as alarming until this was read rather than
    // assumed, which is a restart a controller performs for an alarm nobody will ever receive —
    // or, worse, one it never performs because it believed the operator had already been told.
    // A watchdog that has not looked for four hours, and one that has written down that it is
    // giving the hub a window back after a resume, both send nothing on their next check — so a
    // controller told an alarm was coming waits for one that is not.
    let would_alarm =
        wd.has_run_here && !wd.silenced && wd.is_looking() && !wd.holding_the_hub_a_window && quiet;
    // `null` when the note cannot be read — absent, torn, or written by a hub that has never run.
    // A reader that treated a missing note as health would be reporting the one thing this file
    // exists to deny.
    let health = match &said {
        Some(said) => json!({
            "word": said.word,
            "phone_line": said.phone_line,
            "door": said.door,
            "updates": said.updates,
        }),
        None => Value::Null,
    };
    // Nothing rewrites the note once the hub that wrote it is gone, so past the window the
    // watchdog itself trusts this is not a reading of a live hub — it is a dead hub's last words,
    // with three ages frozen inside them. A reader keying on `observed.health.word == "serving"`
    // is exactly the reader this block was split out for, so a stale note is moved out of the way
    // rather than left sitting under a heading that promises fact.
    let note_age = age_of(&hb.note_path());
    let note_is_current = note_age.is_some_and(|d| d.as_secs() < WATCHDOG_NOTE_TRUSTED_FOR);
    let (observed_health, last_thing_said) = if note_is_current {
        (health.clone(), Value::Null)
    } else {
        (Value::Null, health.clone())
    };
    json!({
        "heartbeat": hb.path().display().to_string(),
        "stale_after_seconds": WATCHDOG_STALE_AFTER,
        // Read off the files the watchdog and the hub wrote. Facts, and nothing else.
        "observed": {
            "watchdog_has_run_here": wd.has_run_here,
            "armed": wd.armed,
            "checked_seconds_ago": wd.checked.map(|d| d.as_secs()),
            "silenced": wd.silenced,
            "silenced_seconds_ago": wd.silenced_for.map(|d| d.as_secs()),
            "alarm_already_sent": wd.alarm_already_sent,
            "holding_the_hub_a_window": wd.holding_the_hub_a_window,
            "window_ends_in_seconds": wd.window_ends_in.map(|d| d.as_secs()),
            "stamped_seconds_ago": stamped,
            "note_seconds_ago": note_age.map(|d| d.as_secs()),
            "health": observed_health,
            "last_thing_the_hub_said": last_thing_said,
        },
        // This command's reading of those facts. Not a report from the watchdog, which has an
        // opinion of its own and is the only thing that can actually send him anything.
        "inferred": {
            "the_hub_has_gone_quiet": quiet,
            "would_alarm": would_alarm,
        },
        // Kept where a reader written before the split looks for them, with the corrected values:
        // dropping the keys would have turned a wrong answer into a missing one. `health` here is
        // the note as read, however old — `observed.note_seconds_ago` is what says whether a hub
        // is still saying it.
        "armed": wd.armed,
        "stamped_seconds_ago": stamped,
        "would_alarm": would_alarm,
        "health": health,
    })
}

/// Handshake, then report what the server said and what this client makes of it.
///
/// This is the command an operator runs from a phone when something is wrong, so it answers the
/// three questions in order: **which socket**, **which server**, **do they agree**. It is also the
/// only command whose entire job is the version policy — a server below
/// [`MIN_SUPPORTED_PROTOCOL`] exits **4** here with a message naming the protocol, which is exactly
/// what proof gate 6 drives with a mock server pinned at protocol 19.
///
/// The handshake must be re-run on every event-stream reconnect, not only at boot: this server
/// advertises `live_handoff`, so herdr can replace its own binary underneath a running bridge
/// without the socket path ever changing.
pub(crate) async fn run(client: &HerdrClient, json: bool) -> anyhow::Result<()> {
    let handshake = client.handshake().await?;
    let socket = client.socket_path().display().to_string();

    if json {
        // Deliberately NOT an RPC envelope: there is no `doctor` method, so wrapping this in a
        // `{"result":{"type":…}}` would invent a wire shape herdr does not have. The server's own
        // pong is nested verbatim under `server` instead, capabilities included — a capability
        // this client was not built for must still reach the operator.
        let mut doc = Map::new();
        doc.insert("socket".to_owned(), Value::String(socket));
        doc.insert(
            "client".to_owned(),
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "known_protocol": KNOWN_PROTOCOL,
                "min_protocol": MIN_SUPPORTED_PROTOCOL,
            }),
        );
        doc.insert("server".to_owned(), serde_json::to_value(&handshake.pong)?);
        doc.insert(
            "compatibility".to_owned(),
            Value::String(handshake.compatibility.as_str().to_owned()),
        );
        doc.insert(
            "ahead_by".to_owned(),
            Value::from(handshake.compatibility.ahead_by()),
        );
        // Review minor: `server_newer` alone does not distinguish a routine `herdr update` from a
        // herdr this client has never been run against. A machine reader gets the bit too.
        doc.insert(
            "far_ahead".to_owned(),
            Value::from(handshake.compatibility.is_far_ahead()),
        );
        doc.insert(
            "watchdog".to_owned(),
            watchdog_json(&Heartbeat::new(Heartbeat::default_path())),
        );
        return super::print_json(&Value::Object(doc));
    }

    println!("socket         {socket}");
    println!(
        "server         herdr {}, protocol {}",
        handshake.version(),
        handshake.protocol()
    );
    println!(
        "client         kickoff-channel {}, built for protocol {KNOWN_PROTOCOL} (minimum {MIN_SUPPORTED_PROTOCOL})",
        env!("CARGO_PKG_VERSION")
    );
    // "unknown additions are survivable" is an earned claim for a routine `herdr update` and an
    // unearned one for a herdr this client has never seen. Past FAR_AHEAD_PROTOCOLS, say so — the
    // operator reads this line on a phone, and it must not sound calmer than the facts warrant.
    match handshake.compatibility.ahead_by() {
        0 => println!("compatibility  {}", handshake.compatibility.as_str()),
        by if handshake.compatibility.is_far_ahead() => println!(
            "compatibility  {} (server is {by} protocol revisions ahead — FAR ahead of the {KNOWN_PROTOCOL} this client was built and tested against. It will run, bucketing what it cannot decode, but its behaviour here is UNVERIFIED and it may be dropping real asks. Rebuild kickoff-channel against this herdr.)",
            handshake.compatibility.as_str(),
        ),
        by => println!(
            "compatibility  {} (server is {by} protocol revision{} ahead; unknown additions are survivable)",
            handshake.compatibility.as_str(),
            if by == 1 { "" } else { "s" }
        ),
    }
    println!(
        "watchdog       {}",
        watchdog_line(&Heartbeat::new(Heartbeat::default_path()))
    );
    match handshake.capabilities() {
        None => println!("capabilities   (none advertised)"),
        Some(caps) => {
            let mut rendered = vec![
                format!("live_handoff={}", caps.live_handoff),
                format!("detached_server_daemon={}", caps.detached_server_daemon),
            ];
            // Capabilities this client was not built for are shown, not dropped: `doctor` exists
            // to tell the operator what is actually there.
            rendered.extend(caps.extra.iter().map(|(k, v)| format!("{k}={v}")));
            println!("capabilities   {}", rendered.join(" "));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A state directory holding a stamp of the given age and, optionally, the hub's own note.
    fn staged(stamped_secs_ago: Option<u64>, note: Option<&str>) -> (tempfile::TempDir, Heartbeat) {
        let d = tempfile::tempdir().expect("a temp dir");
        let hb = Heartbeat::new(d.path().join("hub.heartbeat"));
        if let Some(age) = stamped_secs_ago {
            fs::write(hb.path(), "serving\n").expect("stamps");
            let when = std::time::SystemTime::now() - std::time::Duration::from_secs(age);
            let f = fs::File::options()
                .write(true)
                .open(hb.path())
                .expect("opens the stamp");
            f.set_modified(when).expect("ages the stamp");
        }
        if let Some(text) = note {
            fs::write(hb.note_path(), text).expect("writes the note");
        }
        (d, hb)
    }

    /// `doctor` is the one thing he can run at the keyboard when his phone has buzzed — or when it
    /// has NOT and he suspects it should have. Telling him only how old the stamp is describes a
    /// dead process and a dead door in exactly the same words, and those are different mornings.
    #[test]
    fn doctor_says_which_half_of_the_hub_is_unwell_and_not_only_how_old_the_stamp_is() {
        let (_d, hb) = staged(
            Some(420),
            Some(
                "not serving\nthe phone line answered 12 seconds ago\nthe agents' door has let \
                 nothing through since this hub started\nthe hub looked for your taps 12 seconds \
                 ago\n",
            ),
        );

        let line = watchdog_line(&hb);
        assert!(
            line.contains("the agents' door has let nothing through since this hub started"),
            "doctor did not say which half the hub says is unwell: {line}"
        );
        assert!(
            line.contains("the phone line answered"),
            "doctor said nothing about the other half, so he cannot tell a dead door from a dead \
             process: {line}"
        );

        let json = watchdog_json(&hb);
        let health = &json["health"];
        assert_eq!(
            health["word"], "not serving",
            "the word is missing from --json: {json}"
        );
        assert!(
            health["door"]
                .as_str()
                .is_some_and(|s| s.contains("let nothing through")),
            "--json carries no sentence for the half that failed: {json}"
        );
        assert!(
            health["updates"]
                .as_str()
                .is_some_and(|s| s.contains("the hub looked for your taps")),
            "--json says nothing about whether his taps are still reaching the hub, which is the \
             half whose failure looks most like health from anywhere else: {json}"
        );
    }

    /// A hub whose taps are going to a second copy of itself is a hub answering Telegram, holding
    /// its door open, and doing the operator no good at all. `doctor` is where he finds that out
    /// at the keyboard, so the sentence has to be there and it has to be the hub's own.
    #[test]
    fn doctor_says_so_when_the_hub_is_no_longer_being_handed_his_taps() {
        let (_d, hb) = staged(
            Some(30),
            Some(
                "not serving\nthe phone line answered 2 seconds ago\nthe agents' door let a \
                 connection through 2 seconds ago\nanother copy of this bot is taking your taps, \
                 so none of them reach the agents here\n",
            ),
        );

        let line = watchdog_line(&hb);
        assert!(
            line.contains("another copy of this bot is taking your taps"),
            "doctor read a note saying his taps go somewhere else and said nothing about it: \
             {line}"
        );
        assert_eq!(
            watchdog_json(&hb)["health"]["updates"],
            "another copy of this bot is taking your taps, so none of them reach the agents here"
        );
    }

    /// The note is written in place and read by something else entirely, so a reader can arrive
    /// mid-write. Half a note must read as "cannot say" — never as the healthy word, which is the
    /// one misreading this file exists to prevent.
    #[test]
    fn a_note_caught_half_written_is_reported_as_nothing_rather_than_as_health() {
        let (_d, hb) = staged(
            Some(10),
            Some(
                "serving\nthe phone line answered 1 second ago\nthe agents' door let a connection \
                 through 1 second ago\n",
            ),
        );
        assert_eq!(
            watchdog_json(&hb)["health"],
            Value::Null,
            "a torn note was reported as a health reading"
        );
        assert!(
            !watchdog_line(&hb).contains("the phone line"),
            "a torn note was quoted as though it were whole"
        );
    }

    /// A hub that never earns a stamp is exactly the box the alarm was extended to cover, and it
    /// is the one this command used to call empty.
    ///
    /// A socket that will not bind, or no forum configured, means the hub answers Telegram every
    /// forty-five seconds and withholds every stamp for its whole life. The watchdog arms on the
    /// note alone for that reason and alarms. `doctor` looked at the stamp only, found nothing, and
    /// told him no hub had ever run here and nothing was watching — on the one box where something
    /// was watching and had already buzzed his phone. It is also what a Kickoff controller reads to
    /// decide whether to act.
    ///
    /// The watchdog's own files are staged too, because that is the machine being described: it
    /// arms off the note when there is no stamp to arm off, and writing down that it has armed is
    /// how it says so. Reading the note and announcing an armed watchdog without them was this
    /// command answering for a process it had not looked at.
    #[test]
    fn a_hub_that_has_run_all_week_without_ever_earning_a_stamp_is_not_reported_as_a_box_nothing_ran_on()
     {
        let (_d, hb) = staged(
            None,
            Some(
                "not serving\nthe phone line answered 2 seconds ago\nthe agents' door could not \
                 be opened\nthe hub looked for your taps 2 seconds ago\n",
            ),
        );
        a_watchdog_has_run_here(&hb);

        let line = watchdog_line(&hb);
        assert!(
            !line.contains("no hub has ever run here"),
            "a hub that has run all week and never earned a stamp was reported as a box no hub \
             has ever run on: {line}"
        );
        assert!(
            line.contains("the agents' door could not be opened"),
            "and the one sentence saying what is wrong was dropped with it: {line}"
        );

        let json = watchdog_json(&hb);
        assert_eq!(
            json["armed"], true,
            "the watchdog arms on this note and alarms; --json told its reader nothing was \
             watching: {json}"
        );
        assert_eq!(
            json["would_alarm"], true,
            "the alarm has already gone off on this box and --json said it would not: {json}"
        );
    }

    /// A box where nothing has ever run must not grow a health block out of nowhere.
    #[test]
    fn a_machine_no_hub_has_ever_run_on_says_so_and_invents_no_health() {
        let (_d, hb) = staged(None, None);
        assert_eq!(watchdog_json(&hb)["health"], Value::Null);
        assert!(watchdog_line(&hb).contains("no hub has ever run here"));
    }

    /// The watchdog writes this the first time it sees a hub to watch, and it is the only proof
    /// that anything is watching at all.
    fn a_watchdog_has_run_here(hb: &Heartbeat) {
        let dir = hb.path().parent().expect("a state dir");
        fs::write(dir.join("watchdog.armed"), "").expect("arms the watchdog");
        fs::write(dir.join("watchdog.tick"), "0 0\n").expect("records a check");
    }

    /// Someone told it to stay quiet. It wears off on its own, and until it does nothing is sent.
    fn the_watchdog_was_silenced(hb: &Heartbeat) {
        let dir = hb.path().parent().expect("a state dir");
        fs::write(dir.join("watchdog.disarmed"), "").expect("silences the watchdog");
    }

    /// Written before the alarm is sent, so it is the record that one went out.
    fn the_watchdog_has_already_alarmed(hb: &Heartbeat) {
        let dir = hb.path().parent().expect("a state dir");
        fs::write(dir.join("watchdog.latch"), "1\n").expect("latches the alarm");
    }

    /// The whole point of the line: he is at the keyboard because his phone did NOT buzz. Telling
    /// him the alarm is armed because the hub's own files exist is the one answer that sends him
    /// away believing something is watching a box where nothing is — the watchdog is installed
    /// separately, and on most machines it has never been installed at all.
    #[test]
    fn a_box_where_no_watchdog_has_ever_run_is_not_reported_as_one_something_is_watching() {
        let (_d, hb) = staged(
            Some(420),
            Some(
                "not serving\nthe phone line last answered 7 minutes ago\nthe agents' door last \
                 let a connection through 7 minutes ago\nthe hub last looked for your taps 7 \
                 minutes ago\n",
            ),
        );

        let json = watchdog_json(&hb);
        assert_eq!(
            json["armed"], false,
            "doctor called the alarm armed without reading one file the watchdog writes: {json}"
        );
        assert_eq!(
            json["would_alarm"], false,
            "doctor said an alarm would go off on a machine with no watchdog on it: {json}"
        );

        let line = watchdog_line(&hb);
        assert!(
            line.contains("nothing is watching this hub"),
            "doctor did not say the one thing he came to find out — that nothing here will tell \
             him when the hub goes quiet: {line}"
        );
    }

    /// A silenced alarm is a machine that will stay quiet however dead the hub is. Reporting it as
    /// one that would alarm is how he waits all afternoon for a buzz that was switched off.
    #[test]
    fn a_watchdog_that_was_silenced_is_never_reported_as_one_that_would_alarm() {
        let (_d, hb) = staged(
            Some(420),
            Some(
                "not serving\nthe phone line last answered 7 minutes ago\nthe agents' door last \
                 let a connection through 7 minutes ago\nthe hub last looked for your taps 7 \
                 minutes ago\n",
            ),
        );
        a_watchdog_has_run_here(&hb);
        the_watchdog_was_silenced(&hb);

        let json = watchdog_json(&hb);
        assert_eq!(
            json["would_alarm"], false,
            "the alarm was switched off and doctor said it would go off: {json}"
        );
        let line = watchdog_line(&hb);
        assert!(
            line.contains("silenced"),
            "doctor never said the alarm had been silenced, which is the whole reason his phone \
             is quiet: {line}"
        );
    }

    /// "Your phone should have buzzed" was worked out from the stamp's age alone. Whether one was
    /// actually sent is written down by the watchdog itself, and reading it is the difference
    /// between telling him what happened and telling him what ought to have.
    #[test]
    fn doctor_says_the_alarm_has_gone_out_only_when_the_watchdog_wrote_down_that_it_did() {
        let (_d, hb) = staged(Some(420), None);
        a_watchdog_has_run_here(&hb);
        assert!(
            !watchdog_line(&hb).contains("already"),
            "doctor claimed an alarm had gone out with nothing on this machine saying one did: {}",
            watchdog_line(&hb)
        );
        assert_eq!(
            watchdog_json(&hb)["observed"]["alarm_already_sent"],
            false,
            "the fact a reader acts on was missing from --json: {}",
            watchdog_json(&hb)
        );

        the_watchdog_has_already_alarmed(&hb);
        let line = watchdog_line(&hb);
        assert!(
            line.contains("already"),
            "the watchdog recorded that it sent him the alarm and doctor did not say so: {line}"
        );
        assert_eq!(
            watchdog_json(&hb)["observed"]["alarm_already_sent"],
            true,
            "the record of a sent alarm never reached --json: {}",
            watchdog_json(&hb)
        );
    }

    /// A wiped state directory, a tidy-up, a path that moved: the stamp is gone and the alarm has
    /// seen one here before, which is the one shape that is BOTH an outage and an empty-looking
    /// machine. Saying "no hub has ever run here" about it sends him looking for a setup mistake
    /// instead of the hub that stopped — and the watchdog, which remembers, is already alarming.
    #[test]
    fn a_stamp_that_has_gone_missing_since_the_alarm_saw_one_is_not_called_a_machine_no_hub_ran_on()
    {
        let (_d, hb) = staged(None, None);
        a_watchdog_has_run_here(&hb);

        let line = watchdog_line(&hb);
        assert!(
            !line.contains("no hub has ever run here"),
            "the alarm remembers a hub stamping here and doctor called this an untouched \
             machine: {line}"
        );
        assert!(
            line.contains("the hub's stamp has gone missing"),
            "doctor did not say the one thing that explains both the alarm and the empty \
             directory: {line}"
        );
    }

    /// Age the hub's own note, which is written in place and never rewritten once the hub dies.
    fn the_note_was_written(hb: &Heartbeat, secs_ago: u64) {
        let f = fs::File::options()
            .write(true)
            .open(hb.note_path())
            .expect("opens the note");
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs_ago))
            .expect("ages the note");
    }

    /// The watchdog writes this every single check, so its age is how long since anything looked.
    fn the_watchdog_last_looked(hb: &Heartbeat, secs_ago: u64) {
        let tick = hb.path().with_file_name("watchdog.tick");
        let f = fs::File::options()
            .write(true)
            .open(&tick)
            .expect("opens the watchdog's own record of its last check");
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs_ago))
            .expect("ages the record of the last check");
    }

    /// The second field of the tick file: the moment before which the watchdog judges nothing.
    fn the_watchdog_is_giving_the_hub_a_window_until(hb: &Heartbeat, secs_from_now: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_secs();
        fs::write(
            hb.path().with_file_name("watchdog.tick"),
            format!("{now} {}\n", now + secs_from_now),
        )
        .expect("records a check with a grace deadline");
    }

    /// A hub that was killed leaves its last note on disk, and nothing rewrites it: the process
    /// that would is gone. Its sentences carry ages frozen at the moment it wrote them, so a
    /// three-day-old note still says "the phone line answered 12 seconds ago" — and quoting that
    /// is doctor telling him a dead hub is answering Telegram right now. The watchdog refuses to
    /// quote a note this old for exactly this reason; doctor had no such rule.
    #[test]
    fn a_note_a_dead_hub_left_behind_is_not_quoted_as_though_the_hub_were_still_saying_it() {
        let (_d, hb) = staged(
            Some(3 * 86_400),
            Some(
                "serving\nthe phone line answered 12 seconds ago\nthe agents' door let a \
                 connection through 12 seconds ago\nthe hub looked for your taps 12 seconds ago\n",
            ),
        );
        a_watchdog_has_run_here(&hb);
        the_note_was_written(&hb, 3 * 86_400);

        let line = watchdog_line(&hb);
        assert!(
            !line.contains("the phone line answered 12 seconds ago"),
            "doctor quoted a three-day-old note in the present tense: {line}"
        );
        assert!(
            line.contains("the last thing the hub said about itself"),
            "doctor dropped the stale note and said nothing in its place, so he cannot tell a hub \
             that died mid-sentence from one that never spoke: {line}"
        );

        let json = watchdog_json(&hb);
        assert_eq!(
            json["observed"]["health"],
            Value::Null,
            "a word a hub said three days before it died was published under `observed`, which \
             this command promises is fact: {json}"
        );
        assert_eq!(
            json["observed"]["last_thing_the_hub_said"]["word"], "serving",
            "the stale note was dropped instead of being moved somewhere a reader can see it is \
             stale: {json}"
        );
        assert_eq!(
            json["observed"]["note_seconds_ago"],
            3 * 86_400,
            "nothing in --json said how old the note is, so a reader cannot tell for itself: \
             {json}"
        );
    }

    /// The alarm only fires on a check, so a promise about "its next check" is worth exactly what
    /// the checks are worth. A stopped timer, a logout that took the user's units with it, a
    /// machine that was asleep: every one leaves `watchdog.armed` sitting there, and doctor said
    /// in one breath that the alarm may have been stopped and that its next check would buzz him.
    #[test]
    fn a_watchdog_that_has_stopped_looking_is_not_promised_to_send_the_alarm_on_its_next_check() {
        let (_d, hb) = staged(Some(420), None);
        a_watchdog_has_run_here(&hb);
        the_watchdog_last_looked(&hb, 4 * 3600);

        let line = watchdog_line(&hb);
        assert!(
            !line.contains("its next check should send you the alarm"),
            "doctor said the alarm may have been stopped and then promised its next check would \
             send it: {line}"
        );
        assert!(
            line.contains("nothing will be sent until it starts looking again"),
            "doctor left him with no idea whether to expect anything at all: {line}"
        );
        assert_eq!(
            watchdog_json(&hb)["would_alarm"],
            false,
            "a controller was told an alarm was coming from a watchdog that has not looked for \
             four hours: {}",
            watchdog_json(&hb)
        );
    }

    /// The first check after a resume finds a four-hour-old stamp from a hub that is perfectly
    /// well and has simply not been scheduled yet, so the watchdog writes down a deadline and
    /// gives the hub one full window to stamp before judging it. Three checks can pass in that
    /// window sending nothing, and doctor promised the very next one would buzz him.
    #[test]
    fn a_watchdog_still_giving_the_hub_its_window_back_is_not_promised_to_alarm_on_its_next_check()
    {
        let (_d, hb) = staged(Some(420), None);
        a_watchdog_has_run_here(&hb);
        the_watchdog_is_giving_the_hub_a_window_until(&hb, 120);

        let line = watchdog_line(&hb);
        assert!(
            !line.contains("its next check should send you the alarm"),
            "the alarm is holding off until the hub has had a chance to stamp, and doctor \
             promised the next check would send it: {line}"
        );
        assert_eq!(
            watchdog_json(&hb)["would_alarm"],
            false,
            "--json promised an alarm the watchdog has written down that it is not going to send \
             yet: {}",
            watchdog_json(&hb)
        );
        assert!(
            watchdog_json(&hb)["observed"]["window_ends_in_seconds"]
                .as_u64()
                .is_some_and(|left| left > 0 && left <= 120),
            "nothing in --json said how much of that window is left, so a reader has to take the \
             judgement on trust: {}",
            watchdog_json(&hb)
        );
    }

    /// `watchdog.disarmed` is the one file in the set no watchdog ever writes: the operator
    /// creates it by hand, on the alarm's own printed instruction. Counting it as proof that a
    /// watchdog has run here describes a silence that will wear off on a machine where nothing was
    /// ever installed and nothing will ever watch.
    #[test]
    fn a_file_only_the_operator_writes_is_not_read_as_proof_something_is_watching() {
        let (_d, hb) = staged(Some(420), None);
        the_watchdog_was_silenced(&hb);

        let json = watchdog_json(&hb);
        assert_eq!(
            json["observed"]["watchdog_has_run_here"], false,
            "a file only the operator writes was read as proof a watchdog has run here: {json}"
        );
        let line = watchdog_line(&hb);
        assert!(
            line.contains("no alarm has ever run on this machine"),
            "doctor described a silence that wears off on a box where nothing will ever watch: \
             {line}"
        );
    }

    /// He reads this on a phone. "540 minutes" is a number he has to divide before he knows
    /// whether this happened after dinner or before breakfast, and the note beside it already says
    /// hours — so one line said both.
    #[test]
    fn an_outage_that_has_lasted_a_night_is_said_in_hours_and_not_in_hundreds_of_minutes() {
        let (_d, hb) = staged(Some(9 * 3600), None);
        a_watchdog_has_run_here(&hb);
        the_watchdog_last_looked(&hb, 4 * 3600);

        let line = watchdog_line(&hb);
        assert!(
            line.contains("9 hours") && line.contains("4 hours"),
            "a night's outage and a stopped alarm were both counted out in minutes: {line}"
        );
    }

    /// The timer is what makes the watchdog a watchdog. A stopped one — the unit disabled, linger
    /// off since the last logout, the box asleep — leaves every file it ever wrote sitting there
    /// saying "armed", and reading those files without reading WHEN it last looked reports a
    /// watching alarm on a machine where nothing has looked since Tuesday.
    #[test]
    fn a_watchdog_that_has_not_looked_for_hours_is_not_reported_as_one_checking_every_minute() {
        let (_d, hb) = staged(Some(30), None);
        a_watchdog_has_run_here(&hb);
        let tick = hb.path().with_file_name("watchdog.tick");
        let f = fs::File::options()
            .write(true)
            .open(&tick)
            .expect("opens the watchdog's own record of its last check");
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(4 * 3600))
            .expect("ages it four hours");

        let line = watchdog_line(&hb);
        assert!(
            line.contains("should be looking every minute"),
            "nothing has looked at this hub for four hours and doctor described an alarm that is \
             watching it: {line}"
        );
    }
}

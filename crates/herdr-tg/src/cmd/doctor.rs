//! `herdr-tg doctor` — is this bridge's view of herdr still valid?

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

/// What the watchdog would make of this machine right now.
///
/// `doctor` is the command an operator runs from a phone when something is wrong, and "is the
/// thing that would have told me still working?" belongs in that answer. It reads the stamp
/// directly rather than asking the watchdog, because the watchdog is a timer with no interface —
/// and because a hub reporting on its own liveness is answering the one question it cannot witness.
///
/// The stamp's age alone describes a dead process and a dead door in exactly the same words, and
/// those are different mornings: one is a machine to go and look at, the other is a hub to restart
/// while the phone in his hand keeps working and tells him nothing is wrong. So the hub's own note
/// is read too — every half of it, because "and what about the other one?" is the next question,
/// and the third half is the one whose failure looks most like health from anywhere else.
fn watchdog_line(hb: &Heartbeat) -> String {
    let said = hb.what_it_said();
    let age = match hb.age() {
        // No stamp. Which of the two boxes this is depends entirely on the note beside it: a hub
        // whose socket will not bind, or that has no forum, answers Telegram every forty-five
        // seconds and withholds every stamp for its whole life — and the watchdog arms on the note
        // alone for exactly that case and has already buzzed his phone. Reading the stamp alone
        // told him nothing was watching on the one box where something was.
        None if said.is_some() => {
            "armed; a hub has run here and has never once been able to say it is serving".to_owned()
        }
        None => return "no hub has ever run here, so the alarm is not armed yet".to_owned(),
        Some(age) if age.as_secs() < WATCHDOG_STALE_AFTER => {
            format!(
                "armed; the hub last said it was serving {} seconds ago",
                age.as_secs()
            )
        }
        // Not "since a hub answered": the stamp is withheld when EITHER half stops, so a hub that
        // is answering Telegram perfectly well can be the reason this number is large.
        Some(age) => format!(
            "armed, and it has been {} minutes since the hub could say it was serving — your phone should have buzzed",
            age.as_secs() / 60
        ),
    };
    match said {
        Some(said) => format!(
            "{age}; {}, {}, and {}",
            said.phone_line, said.door, said.updates
        ),
        None => age,
    }
}

fn watchdog_json(hb: &Heartbeat) -> Value {
    let age = hb.age().map(|d| d.as_secs());
    let said = hb.what_it_said();
    // Both of these follow the watchdog's own two ways of arming, not the stamp alone. It arms the
    // first time it sees EITHER of the hub's files, so a hub that has run all week and never
    // earned a stamp is armed and already alarming — and this is the answer a Kickoff controller
    // acts on, so getting it wrong here is a restart that never happens.
    let armed = age.is_some() || said.is_some();
    let would_alarm = match age {
        Some(a) => a >= WATCHDOG_STALE_AFTER,
        None => said.is_some(),
    };
    json!({
        "heartbeat": hb.path().display().to_string(),
        "armed": armed,
        "stamped_seconds_ago": age,
        "stale_after_seconds": WATCHDOG_STALE_AFTER,
        "would_alarm": would_alarm,
        // `null` when the note cannot be read — absent, torn, or written by a hub that has never
        // run. A reader that treated a missing note as health would be reporting the one thing this
        // file exists to deny.
        "health": said.map(|said| json!({
            "word": said.word,
            "phone_line": said.phone_line,
            "door": said.door,
            "updates": said.updates,
        })),
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
        "client         herdr-tg {}, built for protocol {KNOWN_PROTOCOL} (minimum {MIN_SUPPORTED_PROTOCOL})",
        env!("CARGO_PKG_VERSION")
    );
    // "unknown additions are survivable" is an earned claim for a routine `herdr update` and an
    // unearned one for a herdr this client has never seen. Past FAR_AHEAD_PROTOCOLS, say so — the
    // operator reads this line on a phone, and it must not sound calmer than the facts warrant.
    match handshake.compatibility.ahead_by() {
        0 => println!("compatibility  {}", handshake.compatibility.as_str()),
        by if handshake.compatibility.is_far_ahead() => println!(
            "compatibility  {} (server is {by} protocol revisions ahead — FAR ahead of the {KNOWN_PROTOCOL} this client was built and tested against. It will run, bucketing what it cannot decode, but its behaviour here is UNVERIFIED and it may be dropping real asks. Rebuild herdr-tg against this herdr.)",
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
}

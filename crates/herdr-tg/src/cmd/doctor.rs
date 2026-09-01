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
fn watchdog_line() -> String {
    let hb = Heartbeat::new(Heartbeat::default_path());
    match hb.age() {
        None => "no hub has ever run here, so the alarm is not armed yet".to_owned(),
        Some(age) if age.as_secs() < WATCHDOG_STALE_AFTER => {
            format!("armed; a hub answered {} seconds ago", age.as_secs())
        }
        Some(age) => format!(
            "armed, and it has been {} minutes since a hub answered — your phone should have buzzed",
            age.as_secs() / 60
        ),
    }
}

fn watchdog_json() -> Value {
    let hb = Heartbeat::new(Heartbeat::default_path());
    let age = hb.age().map(|d| d.as_secs());
    json!({
        "heartbeat": hb.path().display().to_string(),
        "armed": age.is_some(),
        "stamped_seconds_ago": age,
        "stale_after_seconds": WATCHDOG_STALE_AFTER,
        "would_alarm": age.is_some_and(|a| a >= WATCHDOG_STALE_AFTER),
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
        doc.insert("watchdog".to_owned(), watchdog_json());
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
    println!("watchdog       {}", watchdog_line());
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

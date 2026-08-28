//! `herdr-tg doctor` — is this bridge's view of herdr still valid?

use herdr_client::{HerdrClient, KNOWN_PROTOCOL, MIN_SUPPORTED_PROTOCOL};
use serde_json::{Map, Value, json};

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

//! The four read-only subcommands, and the one helper they share.
//!
//! There is no fifth module here and there must not be: `herdr-client`'s
//! `tests/no_live_write_call_site.rs` greps this whole directory and fails the suite if any file
//! under it so much as names one of herdr's three write RPCs — the ones that type keystrokes into
//! the operator's real terminals. Not a call, not an import, not a subcommand, not a TODO.

pub(crate) mod doctor;
pub(crate) mod read;
pub(crate) mod status;
pub(crate) mod watch;

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::{Map, Value};

/// Rebuild the RPC envelope around a payload that came back through the typed client.
///
/// **This is the proof surface, and the re-serialization is the point.** The payload is a Rust
/// struct that `herdr-client` decoded from the wire; `serde_json::to_value` turns it back into
/// JSON *from the model*, so any field herdr emitted that the client does not model is simply
/// absent here and the gate-3 diff goes red. A passthrough of the bytes off the socket would make
/// that gate prove nothing about the decoder — it would only prove that `cat` works.
///
/// Shape: `{"id":…,"result":{"type":<result_type>,<payload_key>:<payload>}}`, matching
/// `herdr api snapshot` exactly, because `scripts/normalize.jq` starts at `.result.snapshot`.
pub(crate) fn envelope<T: Serialize>(
    result_type: &str,
    payload_key: &str,
    payload: &T,
) -> anyhow::Result<Value> {
    let mut result = Map::new();
    result.insert("type".to_owned(), Value::String(result_type.to_owned()));
    result.insert(payload_key.to_owned(), serde_json::to_value(payload)?);

    let mut envelope = Map::new();
    envelope.insert("id".to_owned(), Value::String(next_envelope_id()));
    envelope.insert("result".to_owned(), Value::Object(result));
    Ok(Value::Object(envelope))
}

/// A locally-minted envelope id.
///
/// **Not the id that went out on the wire.** `herdr-client` mints request ids internally and never
/// surfaces them, deliberately: herdr echoes the id on a semantic refusal and blanks it to `""` on
/// a parse/routing one, so RPC is correlated by the connection, never by the id. This value exists
/// only so the envelope we print has the same *shape* as herdr's; nothing reads it, here or in the
/// proof.
fn next_envelope_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "herdr-tg-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// One compact JSON document plus a newline, on stdout.
///
/// Compact rather than pretty: this is a machine surface (`jq` on the other end of a pipe), and
/// the human surfaces are the tables.
pub(crate) fn print_json(value: &Value) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, value)?;
    out.write_all(b"\n")?;
    Ok(())
}

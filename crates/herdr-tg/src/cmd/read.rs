//! `herdr-tg read` — one pane's visible screen.

use std::io::Write;
use std::num::NonZeroU32;

use anyhow::Context;
use herdr_client::{HerdrClient, PaneId};

/// Read a pane, `source: "visible"` always.
///
/// The source is not a parameter here and cannot become one: `herdr-client` exposes
/// `read_visible` / `read_visible_tail` and nothing else, because `recent` and `recent_unwrapped`
/// harvest-scroll the operator's real viewport whenever `lines > viewport_rows` — they would make
/// the operator's screen move while a background poll ran.
///
/// Without `--json` the text goes to stdout **raw**: no added newline, no trimming. Proof gate 4
/// compares it with `cmp` against `herdr pane read --source visible --format text`, and the
/// "socket text has one more newline than the CLI" belief is a `jq -r` artifact, not a real
/// difference — `jq -j` is byte-identical, 3/3 runs.
pub(crate) async fn run(
    client: &HerdrClient,
    pane: &str,
    lines: Option<NonZeroU32>,
    json: bool,
) -> anyhow::Result<()> {
    client.handshake().await?;

    let pane = PaneId::new(pane);
    let read = match lines {
        Some(n) => client.read_visible_tail(&pane, n).await?,
        None => client.read_visible(&pane).await?,
    };

    if json {
        let envelope = super::envelope("pane_read", "read", &read)
            .context("re-serializing the pane read into its RPC envelope")?;
        super::print_json(&envelope)
    } else {
        // Bytes, not `print!` with a format string: the text may end without a newline, may
        // contain `{}`, and must reach stdout exactly as herdr sent it.
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        out.write_all(read.text.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use herdr_client::PaneRead;

    /// The `--json` surface is what proof gate 4 asserts `source == "visible"` and
    /// `truncated == false` from, so both keys must survive the re-serialization.
    #[test]
    fn the_read_envelope_keeps_source_and_truncated() {
        let raw = include_str!("../../../herdr-client/tests/fixtures/pane_read.json");
        let value: serde_json::Value = serde_json::from_str(raw).expect("fixture is JSON");
        let read: PaneRead =
            serde_json::from_value(value["result"]["read"].clone()).expect("fixture decodes");

        let envelope = super::super::envelope("pane_read", "read", &read)
            .expect("a decoded read re-serializes");
        assert_eq!(envelope["result"]["type"], "pane_read");
        assert_eq!(
            envelope["result"]["read"]["source"], "visible",
            "gate 4 reads this key to prove the client never asks for `recent`"
        );
        assert_eq!(envelope["result"]["read"]["truncated"], false);
        assert_eq!(
            envelope["result"]["read"]["text"], value["result"]["read"]["text"],
            "the text must survive the round trip byte for byte"
        );
    }
}

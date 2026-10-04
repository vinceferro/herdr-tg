//! The `wip` and `dispatch` frames, as this product consumes them.
//!
//! # Why these types live here and not in `hub-proto`
//!
//! The wire contract crate "knows nothing about herdr, kickoff, claude or panes, and must not
//! learn" — and a `wip` frame is lanes, agents and proofs from end to end. The governing idea in
//! `docs/INTERFACES.md` is that the hub's vocabulary is conversation, never orchestration; the
//! moment `hub-proto` grows a `LaneStatus`, it stops being a multiplexer's contract and becomes
//! kickoff's second implementation. These are **consumer-side view models**: another
//! organisation's lane board, decoded by the one surface that renders it, byte-matching their
//! field names so the two sides can reconcile against one document.
//!
//! # The contract these shapes come from
//!
//! The shapes are the omnibus-dispatch spec (§3.2 `wip`, §3.3 `dispatch` + receipt), which the
//! frame-owning half is landing as `hub/FLEET-WIP-FRAMES.md` in their repo. Until that document
//! lands, this file is built against the spec's shapes and field names exactly; where the spec
//! leaves optionality unstated, this file fails closed (a frame that does not say `full: true`
//! is not a snapshot, and the board refuses it rather than guessing). When their contract doc
//! arrives, reconcile DOWN to it — never invent a parallel vocabulary beside it.
//!
//! # Skew is normal
//!
//! The same law as seam ①: an unknown field is ignored (serde's default), an unknown frame kind
//! never reaches these types (the seam routes on `t` and skips what it does not know), and a
//! `status` this file does not model stays a string — rendered verbatim, never mislabelled as a
//! status it resembles. A lane status herdr adds without a release must not crash a strip that
//! is already running.

use std::fmt::Write as _;

/// One `wip` frame — a whole lane board, stamped by whoever relayed it.
///
/// Field names are the spec's, byte for byte. `Serialize` is derived only so tests can round-trip
/// fixtures; nothing in this product emits a `wip` frame upstream — an agent cannot mint WIP any
/// more than it can mint its own `from`, and for the same reason.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct WipFrame {
    pub v: u8,
    pub id: String,
    pub t: String,
    /// The lane the hub stamped the speaker with. The strip does not act on it; it is kept so
    /// the strip can say which voice the board came from when that becomes a question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    /// Snapshot vs delta. The relay mints snapshots (the spec chose snapshot-with-seq over
    /// deltas precisely to kill missed-delta bugs); `None` means the frame did not say, and a
    /// frame that does not say it is a snapshot is refused by the board rather than merged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<bool>,
    pub lanes: Vec<WipLane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// PROPOSED, PENDING CONTRACT — the triage-provenance field (the coordinator's composition
    /// proposal of 2026-10-04). No frame-owning contract carries it yet, so it is optional in
    /// the strictest sense: **absent means absent**, the strip renders nothing for it, and a
    /// build that has never heard of it parses the same frame byte for byte. When their
    /// contract adopts (or rejects) the field, reconcile this to whatever they land — including
    /// deleting it outright.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triage: Option<TriageProvenance>,
}

/// One lane row — exactly the `lanes-snapshot` row shape the spec pinned (§3.2: "exactly the
/// lanes-snapshot row shape").
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct WipLane {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub respawns: Option<u64>,
    /// Whether a proof command was declared for the lane. `done` is only reachable through a
    /// declared proof that passed, and `unverified` means none was declared — a claim of
    /// *nothing*, which the renderer must never dress as success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_min: Option<u64>,
}

/// The proposed triage provenance: was a triage question answered from state the machine already
/// held, or did it wake the coordinator — and with how much confidence.
///
/// `source` stays a string for the same forward-safe reason `status` does: the value set is the
/// proposal's, not a contract's, and an unknown source renders as itself rather than as the
/// nearest known one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TriageProvenance {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

/// One `dispatch` frame — the control surface. This product COMPOSES these (the strip's one
/// affordance) and never executes one: executing belongs to the frame owner's dispatchd, reached
/// through their hub, behind their dispatch-capable token. The strip hands the frame to its seam
/// and renders the receipt that comes back, which is the whole of its duty.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DispatchFrame {
    pub v: u8,
    /// `disp-<ulid>`, minted locally ([`super::seam::mint_dispatch_id`]) or pinned by a
    /// scripted replay so a fixture receipt can name it back.
    pub id: String,
    pub t: String,
    /// `spawn | steer | prioritize | stop`. This strip composes `spawn` only — the first slice's
    /// one affordance. The field stays a string because the receipt path must carry frames this
    /// build would never mint.
    pub action: String,
    /// steer/prioritize/stop; absent for spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    /// spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// spawn — a path on the box, or `--Text` inline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deps: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_cmd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The receipt for a dispatch — the existing ack envelope plus `verdict`.
///
/// `ref` is Rust-adjacent enough to need renaming; on the wire it is `ref`, byte for byte.
/// The verdict vocabulary is the spec's: `accepted | rejected | held`, with `why` naming the
/// refusal (`no_worker | over_budget | unknown_agent | bad_task_ref | not_permitted |
/// lane_busy`). Both stay strings: an unknown verdict or why renders as itself, because a
/// strip that mapped an unfamiliar refusal onto a familiar one would be lying about whose
/// refusal it was.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DispatchReceipt {
    pub t: String,
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// Everything a fixture line can be once routed on `t`.
pub(crate) enum Down {
    Wip(WipFrame),
    Ack(DispatchReceipt),
}

/// Route one replayed line on its `t`.
///
/// Returns `Ok(None)` for a frame kind this build does not know — the skew-is-normal law: the
/// line was read, it is not an error, and it is not this strip's business. A line that names a
/// known kind but will not decode into it is an `Err` carried to the caller, which says it out
/// loud and continues: silent continuation would hide a contract change behind a strip that
/// looks alive.
pub(crate) fn route_down(line: &str) -> anyhow::Result<Option<Down>> {
    let value: serde_json::Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => return Err(anyhow::anyhow!("not JSON: {err}")),
    };
    match value.get("t").and_then(serde_json::Value::as_str) {
        Some("wip") => {
            let frame: WipFrame = serde_json::from_value(value)
                .map_err(|err| anyhow::anyhow!("a wip frame that does not parse: {err}"))?;
            Ok(Some(Down::Wip(frame)))
        }
        Some("ack") => {
            let receipt: DispatchReceipt = serde_json::from_value(value)
                .map_err(|err| anyhow::anyhow!("an ack that does not parse: {err}"))?;
            Ok(Some(Down::Ack(receipt)))
        }
        _ => Ok(None),
    }
}

/// Seconds since the epoch, now.
pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

/// Parse the one timestamp shape the lane board emits: `2026-10-04T12:59:00Z`.
///
/// Hand-rolled on purpose. Pulling a date crate in for sixteen fixed-width digits is a
/// dependency this repo would carry for one fixture parser, and the shape is pinned by the
/// frame contract — anything that is not exactly this (offsets, missing `Z`, a two-digit year)
/// returns `None` and the renderer falls back to the frame's own `age_min`, said as an
/// approximation, rather than guessing at a timestamp it did not understand.
pub(crate) fn parse_rfc3339_utc(stamp: &str) -> Option<i64> {
    let bytes = stamp.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' || bytes.last() != Some(&b'Z') {
        return None;
    }
    let digits: Option<Vec<u64>> = (0..19)
        .filter(|&i| i != 4 && i != 7 && i != 10 && i != 13 && i != 16)
        .map(|i| (bytes[i] as char).to_digit(10).map(u64::from))
        .collect();
    let digits = digits?;
    // Fourteen digits: yyyy mm dd hh mm ss. Composed rather than sliced-and-parsed so a wrong
    // length cannot quietly read one field as its neighbour's.
    let (year, month, day) = (
        digits[0] as i64 * 1000 + digits[1] as i64 * 100 + digits[2] as i64 * 10 + digits[3] as i64,
        digits[4] * 10 + digits[5],
        digits[6] * 10 + digits[7],
    );
    let (hour, minute, second) = (
        digits[8] * 10 + digits[9],
        digits[10] * 10 + digits[11],
        digits[12] * 10 + digits[13],
    );
    // A 25th hour is not a parse error this function repairs; it is a timestamp it refuses.
    // Same for a month or day of zero, which no calendar this frame comes from ever wrote.
    if hour > 23 || minute > 59 || second > 59 || !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + (hour * 3600 + minute * 60 + second) as i64)
}

/// Days from 1970-01-01 for a civil date — Howard Hinnant's `days_from_civil`, reproduced
/// because it is the whole of the calendar arithmetic this file needs.
fn days_from_civil(year: i64, month: u64, day: u64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = (month + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year as i64;
    era * 146_097 + day_of_era - 719_468
}

/// A duration in seconds, in the words a person reads at a glance: `47s`, `12m`, `3h 4m`.
pub(crate) fn spell_age(secs: i64) -> String {
    let secs = secs.max(0);
    let mut out = String::new();
    if secs < 60 {
        let _ = write!(out, "{secs}s");
    } else if secs < 3600 {
        let _ = write!(out, "{}m", secs / 60);
    } else {
        let _ = write!(out, "{}h {}m", secs / 3600, (secs % 3600) / 60);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's own example row (§3.2), byte for byte, decodes and round-trips. This is the
    /// reconcile-down anchor: if the frame-owning half's contract lands with different field
    /// names, THIS test is the one that goes red, and it goes red against the spec's bytes —
    /// which is the disagreement being looked for, not a fixture drifting from the code.
    #[test]
    fn the_spec_s_own_example_frame_decodes_field_for_field() {
        let spec = r#"{"v":1,"id":"wip-41","t":"wip","lane":"fleet","full":true,"lanes":[{"id":"lane-1004-125932-1368366","short":"…","agent":"builder","status":"running","respawns":1,"proof":true,"updated":"2026-10-04T12:59:00Z","age_min":12}],"total":7,"shown":7,"truncated":0,"seq":41}"#;
        let Some(Down::Wip(frame)) = route_down(spec).expect("the spec's frame is a wip frame")
        else {
            panic!("routed as something other than wip");
        };
        assert_eq!(frame.v, 1);
        assert_eq!(frame.id, "wip-41");
        assert_eq!(frame.lane.as_deref(), Some("fleet"));
        assert_eq!(frame.full, Some(true));
        assert_eq!(frame.total, Some(7));
        assert_eq!(frame.shown, Some(7));
        assert_eq!(frame.truncated, Some(0));
        assert_eq!(frame.seq, Some(41));
        let lane = &frame.lanes[0];
        assert_eq!(lane.id, "lane-1004-125932-1368366");
        assert_eq!(lane.agent.as_deref(), Some("builder"));
        assert_eq!(lane.status, "running");
        assert_eq!(lane.respawns, Some(1));
        assert_eq!(lane.proof, Some(true));
        assert_eq!(lane.updated.as_deref(), Some("2026-10-04T12:59:00Z"));
        assert_eq!(lane.age_min, Some(12));
        assert_eq!(frame.triage, None, "no triage on the wire means no triage");

        // And back: what this build models is what it would say, no more.
        let back = serde_json::to_string(&frame).expect("round-trips");
        assert!(
            back.contains("\"id\":\"wip-41\"") && back.contains("\"seq\":41"),
            "the round trip lost the spec's own field names: {back}"
        );
    }

    /// Unknown fields are ignored, unknown kinds are skipped, and neither is an error — the
    /// skew law that keeps a strip alive through the frame owner's releases.
    #[test]
    fn a_frame_from_a_later_contract_parses_without_learning_its_new_fields() {
        let later = r#"{"v":1,"id":"wip-99","t":"wip","full":true,"lanes":[],"plan_visibility":"slice-1.5","seq":99}"#;
        let Some(Down::Wip(frame)) = route_down(later).expect("still a wip frame") else {
            panic!("routed as something other than wip");
        };
        assert_eq!(frame.id, "wip-99");
        assert!(frame.lanes.is_empty());

        assert!(
            route_down(r#"{"v":1,"id":"x-1","t":"a-kind-this-build-never-heard-of"}"#)
                .expect("an unknown kind is not an error")
                .is_none(),
            "an unknown frame kind is skipped, not refused"
        );
    }

    /// The receipt shape: `ref` on the wire, `why` optional, verdict verbatim.
    #[test]
    fn a_rejected_receipt_carries_its_ref_and_its_why_byte_for_byte() {
        let Some(Down::Ack(receipt)) = route_down(
            r#"{"t":"ack","ref":"disp-01JAFX","verdict":"rejected","why":"over_budget"}"#,
        )
        .expect("an ack is an ack") else {
            panic!("routed as something other than ack");
        };
        assert_eq!(receipt.ref_id, "disp-01JAFX");
        assert_eq!(receipt.verdict, "rejected");
        assert_eq!(receipt.why.as_deref(), Some("over_budget"));

        let out = serde_json::to_string(&receipt).expect("round-trips");
        assert!(
            out.contains(r#""ref":"disp-01JAFX""#),
            "the wire name is ref, and the round trip must keep it: {out}"
        );
    }

    /// The proposed triage field: present when present, absent when absent, unknown sources kept
    /// as strings.
    #[test]
    fn the_proposed_triage_field_is_wholly_optional() {
        let with = r#"{"v":1,"id":"wip-60","t":"wip","full":true,"lanes":[],"seq":60,"triage":{"source":"state","confidence":0.87}}"#;
        let Some(Down::Wip(frame)) = route_down(with).expect("still a wip frame") else {
            panic!("routed as something other than wip");
        };
        let triage = frame.triage.as_ref().expect("the field is on the wire");
        assert_eq!(triage.source, "state");
        assert_eq!(triage.confidence, Some(0.87));

        let without = r#"{"v":1,"id":"wip-61","t":"wip","full":true,"lanes":[],"seq":61}"#;
        let Some(Down::Wip(frame)) = route_down(without).expect("still a wip frame") else {
            panic!("routed as something other than wip");
        };
        assert_eq!(frame.triage, None);
    }

    /// Timestamps: the one shape that parses, and a refusal — not a guess — for the rest.
    #[test]
    fn timestamps_parse_exactly_the_shape_the_board_emits_and_refuse_everything_else() {
        assert_eq!(
            parse_rfc3339_utc("2026-10-04T12:59:00Z"),
            Some(1_791_118_740),
            "a known-good epoch for a known-good stamp; if this fails the calendar math regressed"
        );
        for not_this_shape in [
            "2026-10-04T12:59:00+02:00",
            "2026-10-04 12:59:00Z",
            "26-10-04T12:59:00Z",
            "2026-10-04T25:59:00Z",
            "",
        ] {
            assert_eq!(
                parse_rfc3339_utc(not_this_shape),
                None,
                "{not_this_shape:?} must be refused, not repaired"
            );
        }
    }

    #[test]
    fn ages_are_spelled_the_way_a_person_reads_them() {
        assert_eq!(spell_age(0), "0s");
        assert_eq!(spell_age(47), "47s");
        assert_eq!(spell_age(12 * 60), "12m");
        assert_eq!(spell_age(3 * 3600 + 4 * 60), "3h 4m");
    }
}

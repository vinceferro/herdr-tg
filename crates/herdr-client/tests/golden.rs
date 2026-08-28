//! Golden tests: the typed model against the SHAPE herdr really sent, captured by
//! `scripts/capture-fixtures.sh` from the live herd on 2026-08-28 (herdr 0.8.2 / protocol 20)
//! and then de-identified in place by `scripts/scrub-fixtures.py`.
//!
//! **What is real and what is not.** The capture is real, and everything these tests read is the
//! real thing: the envelope nesting, the key names, the key ORDER, the types, and — critically —
//! exactly which optional fields herdr chose to omit on which object (`wA:p1` carries no
//! `agent_session`; `wE:p1`'s `AgentInfo` carries no `screen_detection_skipped`). What is
//! synthetic is the *content* of the human-identifying values: working directories, terminal
//! titles, agent session ids, terminal ids, and the whole visible text of the captured pane.
//! This repository is public and a fixture is committed forever, so the operator's screen and
//! project names do not go in it. The scrub is value-only and structure-preserving by
//! construction, which is what keeps [`snapshot_roundtrip_loses_nothing`] meaningful.
//!
//! # Why this file exists, and what it is a stand-in for
//!
//! The live proof's gate 3 sandwich-diffs `herdr-tg status --json` against `herdr api snapshot`.
//! That gate runs on the operator's laptop, by hand, against a live herd. This file is that gate
//! made **offline and deterministic**, so a modelling omission fails at `cargo test` on a box with
//! no herdr socket at all instead of only on the lap at the slice's done-boundary.
//!
//! [`snapshot_roundtrip_loses_nothing`] is the load-bearing one: deserialize the captured snapshot,
//! re-serialize, compare as `serde_json::Value`, assert **zero** field loss. When it goes red it
//! prints the exact JSON pointers that differ — read that list before touching the model, because
//! the cause is nearly always a field herdr emits that we drop, and the list names it.
//!
//! Every test here is offline. No socket, no network, no `HERDR_*`.
//!
//! # Spec rows -> where they live (build order step 5)
//!
//! | spec row | status |
//! |---|---|
//! | `snapshot_roundtrip_loses_nothing` | here, green, and verified non-vacuous (deleting `AgentInfo::screen_detection_skipped` turns it red and names the field) |
//! | `absent_optional_fields_do_not_serialize_as_null` | here, green |
//! | `agent_status_unrecognized_round_trips_verbatim` | here, green |
//! | `pane_read_revision_is_zero_while_pane_info_revision_is_not` | here, green |
//!
//! Two tests BEYOND the spec's four rows, both labelled as such at their definition:
//! `pane_read_roundtrip_loses_nothing` and `done_is_a_real_observed_status_on_this_host`.

use std::collections::BTreeSet;

use herdr_client::{AgentStatus, PaneInfo, PaneRead, SessionSnapshot};
use serde_json::{Value, json};

const SNAPSHOT_LINE: &str = include_str!("fixtures/snapshot.json");
const PANE_READ_LINE: &str = include_str!("fixtures/pane_read.json");

/// The captured `session.snapshot` reply, whole.
fn snapshot_reply() -> Value {
    serde_json::from_str(SNAPSHOT_LINE).expect("snapshot fixture is well-formed JSON")
}

/// The captured `pane.read` reply, whole.
fn pane_read_reply() -> Value {
    serde_json::from_str(PANE_READ_LINE).expect("pane_read fixture is well-formed JSON")
}

// ── the field-loss diff ─────────────────────────────────────────────────────────────────────────

/// Every JSON pointer at which `wire` and `ours` disagree, as human-readable lines.
///
/// Deliberately explicit about the THREE ways a round trip can lose: a key we dropped, a key we
/// invented, and a value we mangled. `assert_eq!` on two 9 KB `Value`s prints a wall of JSON that
/// hides which one it was.
fn field_diff(wire: &Value, ours: &Value) -> Vec<String> {
    fn walk(path: &str, wire: &Value, ours: &Value, out: &mut Vec<String>) {
        match (wire, ours) {
            (Value::Object(w), Value::Object(o)) => {
                let keys: BTreeSet<&String> = w.keys().chain(o.keys()).collect();
                for k in keys {
                    let p = format!("{path}/{k}");
                    match (w.get(k), o.get(k)) {
                        (Some(wv), Some(ov)) => walk(&p, wv, ov, out),
                        (Some(wv), None) => {
                            out.push(format!("DROPPED  {p}  (herdr sent {wv}, we emit nothing)"))
                        }
                        (None, Some(ov)) => {
                            out.push(format!("INVENTED {p}  (herdr sent nothing, we emit {ov})"))
                        }
                        (None, None) => unreachable!("key came from one of the two maps"),
                    }
                }
            }
            (Value::Array(w), Value::Array(o)) => {
                if w.len() != o.len() {
                    out.push(format!(
                        "LENGTH   {path}  (herdr sent {} items, we emit {})",
                        w.len(),
                        o.len()
                    ));
                    return;
                }
                for (i, (wv, ov)) in w.iter().zip(o.iter()).enumerate() {
                    walk(&format!("{path}/{i}"), wv, ov, out);
                }
            }
            (w, o) if w != o => out.push(format!("CHANGED  {path}  ({w} -> {o})")),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk("", wire, ours, &mut out);
    out
}

/// Panics with the drop list rather than with 9 KB of JSON.
fn assert_lossless(what: &str, wire: &Value, ours: &Value) {
    let diff = field_diff(wire, ours);
    assert!(
        diff.is_empty(),
        "{what}: the round trip lost or changed {} field(s):\n  {}",
        diff.len(),
        diff.join("\n  ")
    );
}

/// Every JSON pointer whose value is `null`, anywhere in `v`.
fn null_paths(v: &Value) -> Vec<String> {
    fn walk(path: &str, v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Null => out.push(path.to_owned()),
            Value::Object(m) => {
                for (k, vv) in m {
                    walk(&format!("{path}/{k}"), vv, out);
                }
            }
            Value::Array(a) => {
                for (i, vv) in a.iter().enumerate() {
                    walk(&format!("{path}/{i}"), vv, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk("", v, &mut out);
    out
}

// ── the tests ───────────────────────────────────────────────────────────────────────────────────

/// **THE test.** Zero field loss across the captured snapshot's real shape, and across
/// the whole reply envelope it arrived in.
///
/// This is also where `terminal_title` / `terminal_title_stripped` decoding is proven: both are
/// volatile (opencode retitles every 20–40 s) so the live proof's normalizer drops them from its
/// diff, which leaves this fixture as the only place their modelling is checked at all.
#[test]
fn snapshot_roundtrip_loses_nothing() {
    let wire = snapshot_reply();
    let raw = wire["result"]["snapshot"].clone();
    assert!(
        raw.is_object(),
        "fixture must nest the payload under result.snapshot"
    );

    let typed: SessionSnapshot =
        serde_json::from_value(raw.clone()).expect("the captured snapshot must decode");
    let ours = serde_json::to_value(&typed).expect("SessionSnapshot must serialize");

    assert_lossless("session.snapshot", &raw, &ours);

    // And the same claim one level up: the client's `status --json` re-serializes the FULL
    // envelope, so a lossless payload inside a mangled envelope would still fail the live gate.
    let mut rebuilt = wire.clone();
    rebuilt["result"]["snapshot"] = ours;
    assert_lossless("the session.snapshot envelope", &wire, &rebuilt);

    // Non-vacuity: a diff over an empty model would also be empty if we compared nothing.
    assert_eq!(typed.protocol, 20);
    assert_eq!(typed.version, "0.8.2");
    assert_eq!(typed.panes.len(), 6);
    assert_eq!(typed.agents.len(), 6);
    assert_eq!(typed.workspaces.len(), 6);
    assert_eq!(typed.tabs.len(), 6);
    assert_eq!(typed.layouts.len(), 6);
    assert!(
        typed.panes.iter().any(|p| p.terminal_title.is_some()),
        "terminal_title decoding is only proven here; the fixture must still carry one"
    );
}

/// The `pane.read` half of the same claim. Beyond the spec's four rows, and cheap: `pane.read` is
/// the other method whose payload the binary re-serializes verbatim under `--json`.
#[test]
fn pane_read_roundtrip_loses_nothing() {
    let wire = pane_read_reply();
    let raw = wire["result"]["read"].clone();

    let typed: PaneRead =
        serde_json::from_value(raw.clone()).expect("the captured pane.read must decode");
    let ours = serde_json::to_value(&typed).expect("PaneRead must serialize");

    assert_lossless("pane.read", &raw, &ours);
    assert_eq!(typed.source_name(), "visible");
    assert!(!typed.text.is_empty());
}

/// herdr OMITS unset optionals; it does not send `null`. A client that emits `"label":null` breaks
/// the live proof's diff for a purely cosmetic reason and puts the word `null` in a Telegram
/// message body.
#[test]
fn absent_optional_fields_do_not_serialize_as_null() {
    let wire = snapshot_reply();
    let raw = wire["result"]["snapshot"].clone();

    // The fixture itself is null-free — the premise this whole rule rests on.
    assert_eq!(
        null_paths(&raw),
        Vec::<String>::new(),
        "the captured snapshot must contain no nulls at all"
    );

    let typed: SessionSnapshot = serde_json::from_value(raw).unwrap();
    let ours = serde_json::to_value(&typed).unwrap();
    assert_eq!(
        null_paths(&ours),
        Vec::<String>::new(),
        "re-serializing must not introduce a single null"
    );

    // And specifically the five fields absent on all 6 live panes.
    for pane in ours["panes"].as_array().unwrap() {
        for k in ["label", "title", "tokens", "state_labels", "display_agent"] {
            assert!(
                pane.get(k).is_none(),
                "pane {} re-serialized an absent field `{k}` as {:?}",
                pane["pane_id"],
                pane.get(k)
            );
        }
    }

    // A hand-built pane with every optional `None` must serialize to EXACTLY the 7 required keys.
    // The map/vec case is the one that bites: a bare `#[serde(default)] BTreeMap` re-serializes as
    // `{}` where herdr emitted nothing.
    let bare = PaneInfo {
        pane_id: "w9:p1".into(),
        terminal_id: "term_x".to_owned(),
        workspace_id: "w9".into(),
        tab_id: "w9:t1".into(),
        focused: false,
        agent_status: AgentStatus::Idle,
        revision: 0,
        agent: None,
        display_agent: None,
        label: None,
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        cwd: None,
        foreground_cwd: None,
        agent_session: None,
        scroll: None,
        state_labels: None,
        tokens: None,
    };
    let ours = serde_json::to_value(&bare).unwrap();
    let keys: BTreeSet<&str> = ours
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "pane_id",
            "terminal_id",
            "workspace_id",
            "tab_id",
            "focused",
            "agent_status",
            "revision",
        ]),
        "an all-`None` pane must emit only the 7 required keys, and `state_labels` must not \
         appear as `{{}}`"
    );
}

/// `#[serde(other)]` compiles, looks right, and DISCARDS the wire string — it re-serializes an
/// unmodelled status as the literal `"unrecognized"`, which would corrupt the client's own `--json`
/// output the day herdr adds a status. This test is what forbids it.
#[test]
fn agent_status_unrecognized_round_trips_verbatim() {
    let decoded: AgentStatus = serde_json::from_str(r#""reticulating""#).unwrap();
    assert_eq!(
        decoded,
        AgentStatus::Unrecognized("reticulating".to_owned())
    );
    assert_ne!(
        decoded,
        AgentStatus::Unknown,
        "`unrecognized` (this client is behind) and `unknown` (herdr cannot tell) are different \
         answers and neither is ever a push"
    );

    let reserialized = serde_json::to_string(&decoded).unwrap();
    assert_eq!(reserialized, r#""reticulating""#);
    assert_ne!(
        reserialized, r#""unrecognized""#,
        "this is EXACTLY what `#[serde(other)]` emits; if you see it, the manual impl was replaced"
    );
    assert!(
        decoded.is_indeterminate(),
        "an unmodelled status must never trigger a push"
    );

    // Every modelled variant round-trips too.
    for wire in ["idle", "working", "blocked", "done", "unknown"] {
        let s = AgentStatus::from_wire(wire);
        assert!(
            !matches!(s, AgentStatus::Unrecognized(_)),
            "{wire} must be modelled"
        );
        assert_eq!(s.as_str(), wire);
        assert_eq!(serde_json::to_string(&s).unwrap(), json!(wire).to_string());
    }

    // And it survives a whole-snapshot round trip, not just a bare string: patch one pane's status
    // to an invented value and require the snapshot to come back byte-identical.
    let mut raw = snapshot_reply()["result"]["snapshot"].clone();
    raw["panes"][0]["agent_status"] = json!("reticulating");
    raw["agents"][0]["agent_status"] = json!("reticulating");
    let typed: SessionSnapshot = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(
        typed.panes[0].agent_status,
        AgentStatus::Unrecognized("reticulating".to_owned())
    );
    assert_lossless(
        "a snapshot carrying an unmodelled status",
        &raw,
        &serde_json::to_value(&typed).unwrap(),
    );

    // `agent_panes()` filters both indeterminate answers out, so the patched agent drops out.
    assert_eq!(typed.agent_panes().count(), 5);
}

/// `PaneReadResult.revision` is a hard-coded 0 stub while `PaneInfo.revision` is live (and indexes
/// the retained `pane_updated` backlog, not output changes). Pinned from the fixtures so a later
/// reader cannot get the asymmetry backwards.
#[test]
fn pane_read_revision_is_zero_while_pane_info_revision_is_not() {
    let read: PaneRead =
        serde_json::from_value(pane_read_reply()["result"]["read"].clone()).unwrap();
    assert_eq!(
        read.revision, 0,
        "pane.read's revision is a stub and has only ever been 0"
    );
    assert!(
        !read.truncated,
        "a full visible read of a 63-row viewport is not truncated"
    );

    let snap: SessionSnapshot =
        serde_json::from_value(snapshot_reply()["result"]["snapshot"].clone()).unwrap();
    let pane = snap
        .pane(&read.pane_id)
        .expect("the read pane is in the snapshot");
    assert_eq!(
        pane.revision, 5,
        "the same pane's PaneInfo.revision is live"
    );
    assert!(
        snap.panes.iter().any(|p| p.revision != 0),
        "at least one pane must carry a non-zero revision or this test proves nothing"
    );

    // The other half of the asymmetry, from the same fixture: a full `visible` read returns
    // viewport_rows - 1 newlines (62 on a 63-row viewport).
    let rows = pane
        .scroll
        .as_ref()
        .expect("pane w9:p1 carries scroll info")
        .viewport_rows;
    assert_eq!(rows, 63);
    assert_eq!(read.line_count() as u64, rows - 1);
    assert_eq!(read.trimmed_tail(0), "");
    assert_eq!(
        read.trimmed_tail(10_000),
        read.text,
        "a short text is returned whole"
    );
    // The captured text ENDS with a newline, so the borrowed tail keeps that terminator: n lines
    // back is n newlines, not n-1. Counted by splitting rather than by counting '\n' so the
    // assertion says what it means.
    for n in [1usize, 5, 62] {
        let tail = read.trimmed_tail(n);
        assert_eq!(
            tail.trim_end_matches('\n').split('\n').count(),
            n,
            "trimmed_tail({n}) must yield exactly {n} lines"
        );
        assert!(
            read.text.ends_with(tail),
            "the tail must be a suffix of the text, borrowed"
        );
    }
    assert_eq!(
        read.trimmed_tail(62),
        read.text,
        "the whole 62-line read is its own tail"
    );
}

/// **Beyond the spec's four rows, and it exists to pin a correction.** The build spec states that
/// `done` "has never been observed on this host". The captured fixture disagrees: `wD:p1` carries
/// `agent_status: "done"` on its `PaneInfo`, on its `AgentInfo`, and on workspace `wD`.
///
/// The distinction that survives: the VALUE is real and decodes; the TRANSITION into it is still
/// unobserved, and PLAN.md's second push trigger depends on the transition, not the value. Keeping
/// this as a test means the next person to read "never observed" in a doc has a fixture to check it
/// against.
#[test]
fn done_is_a_real_observed_status_on_this_host() {
    let snap: SessionSnapshot =
        serde_json::from_value(snapshot_reply()["result"]["snapshot"].clone()).unwrap();

    let done: Vec<&str> = snap
        .panes
        .iter()
        .filter(|p| p.agent_status == AgentStatus::Done)
        .map(|p| p.pane_id.as_str())
        .collect();
    assert_eq!(
        done,
        ["wD:p1"],
        "the captured herd had exactly one `done` pane"
    );

    assert_eq!(
        snap.agent(&"wD:p1".into()).map(|a| &a.agent_status),
        Some(&AgentStatus::Done),
        "the AgentInfo agrees with the PaneInfo"
    );
    assert!(
        !AgentStatus::Done.is_indeterminate(),
        "`done` is a determinate answer — it is a push trigger, not an 'I do not know'"
    );
    // And it round-trips as itself, not as `unknown`.
    assert_eq!(
        serde_json::to_string(&AgentStatus::Done).unwrap(),
        r#""done""#
    );
}

/// **Review minor, closed 2026-08-28.** Seven modelled optional fields had never been decoded from
/// bytes that carried a value — not live (a census of the running herd finds 0 occurrences of each),
/// not in any checked-in fixture. Five of them (`tokens`, `title`, `state_labels`,
/// `interactive_ready`, `launch_pending`) were ALSO on `scripts/normalize.jq`'s drop list, so the
/// live proof's gate 3 deleted them from BOTH sides before comparing: if one of the seven decoded
/// to the wrong type, nothing anywhere in this repo would have noticed. They were schema-verified
/// and byte-unverified.
///
/// The other half of that fix moved the five out of normalize.jq's drop list, so a future herdr
/// that starts emitting one turns gate 3 RED instead of silently dropping it. This is the offline
/// half: it exercises the decode itself, on a box with no herdr socket.
///
/// **These values are SYNTHETIC and hand-built, and deliberately not added to the fixture files** —
/// a fixture is a record of what herdr really sent, and inventing content for it would make
/// `snapshot_roundtrip_loses_nothing` a test of this author's imagination. Every shape below is
/// grounded in the checked-in schema (`fixtures/herdr-schema-p20.json`,
/// `schemas.success_response.$defs.{PaneInfo,AgentInfo}`), NOT guessed:
///
/// | field | schema | model |
/// |---|---|---|
/// | `tokens` | `object`, `additionalProperties: string`, `propertyNames ^[A-Za-z0-9_-]{1,32}$` | `Option<BTreeMap<String,String>>` |
/// | `state_labels` | `object`, `additionalProperties: string` | `Option<BTreeMap<String,String>>` |
/// | `title` | `["string","null"]` | `Option<String>` |
/// | `display_agent` | `["string","null"]` | `Option<String>` |
/// | `name` (AgentInfo only) | `["string","null"]` | `Option<String>` |
/// | `interactive_ready` (AgentInfo only) | `boolean` | `Option<bool>` |
/// | `launch_pending` (AgentInfo only) | `boolean` | `Option<bool>` |
#[test]
fn unobserved_optional_fields_decode_from_bytes() {
    let wire = snapshot_reply();
    let mut raw = wire["result"]["snapshot"].clone();

    // Premise: these really are unobserved in the captured fixture. If a future recapture starts
    // carrying them, this test stops being the only coverage and should say so out loud.
    for k in [
        "tokens",
        "title",
        "state_labels",
        "interactive_ready",
        "launch_pending",
        "display_agent",
        "name",
    ] {
        assert_eq!(
            raw.to_string().matches(&format!("\"{k}\"")).count(),
            0,
            "the captured fixture now carries `{k}` — update this test's premise"
        );
    }

    let pane_extra = json!({
        "tokens":        {"input": "1234", "output": "567", "cache_read": "89"},
        "title":         "a pane title herdr has never yet sent",
        "state_labels":  {"phase": "compiling", "detail": "3/7 crates"},
        "display_agent": "Claude Opus"
    });
    let agent_extra = json!({
        "tokens":            {"input": "42", "output": "7"},
        "title":             "an agent title herdr has never yet sent",
        "state_labels":      {"phase": "awaiting-input"},
        "display_agent":     "Claude Opus",
        "name":              "opus-1",
        "interactive_ready": true,
        "launch_pending":    false
    });

    for (k, v) in pane_extra.as_object().unwrap() {
        raw["panes"][0][k] = v.clone();
    }
    for (k, v) in agent_extra.as_object().unwrap() {
        raw["agents"][0][k] = v.clone();
    }
    // `tokens` is on WorkspaceInfo too, and is the one field of the seven that a workspace carries.
    raw["workspaces"][0]["tokens"] = json!({"input": "999"});

    let typed: SessionSnapshot =
        serde_json::from_value(raw.clone()).expect("all seven decode, none is a type error");

    // The decode is what is under test — assert the TYPED values, not just that it parsed.
    let pane = &typed.panes[0];
    assert_eq!(
        pane.tokens.as_ref().expect("pane.tokens decoded")["input"],
        "1234"
    );
    assert_eq!(pane.tokens.as_ref().unwrap().len(), 3);
    assert_eq!(
        pane.title.as_deref(),
        Some("a pane title herdr has never yet sent")
    );
    assert_eq!(
        pane.state_labels.as_ref().expect("pane.state_labels")["phase"],
        "compiling"
    );
    assert_eq!(pane.display_agent.as_deref(), Some("Claude Opus"));

    let agent = &typed.agents[0];
    assert_eq!(
        agent.tokens.as_ref().expect("agent.tokens decoded")["output"],
        "7"
    );
    assert_eq!(
        agent.title.as_deref(),
        Some("an agent title herdr has never yet sent")
    );
    assert_eq!(
        agent.state_labels.as_ref().expect("agent.state_labels")["phase"],
        "awaiting-input"
    );
    assert_eq!(agent.display_agent.as_deref(), Some("Claude Opus"));
    assert_eq!(agent.name.as_deref(), Some("opus-1"));
    assert_eq!(
        agent.interactive_ready,
        Some(true),
        "a boolean, not a string — and `Some(false)` must not collapse to `None`"
    );
    assert_eq!(agent.launch_pending, Some(false));
    assert_eq!(
        typed.workspaces[0].tokens.as_ref().expect("ws.tokens")["input"],
        "999"
    );

    // And the round trip loses none of them — the property gate 3 now enforces on the live wire.
    let ours = serde_json::to_value(&typed).unwrap();
    assert_lossless("snapshot + the seven unobserved optionals", &raw, &ours);
    assert_eq!(
        null_paths(&ours),
        Vec::<String>::new(),
        "`launch_pending: false` must re-serialize as `false`, never as `null`"
    );
}

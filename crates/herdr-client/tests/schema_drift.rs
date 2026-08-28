//! Drift tests: the checked-in 255,484-byte `herdr api schema --json` dump against what this client
//! actually believes.
//!
//! # What this file buys back
//!
//! `proto/model.rs` deliberately carries **no** `#[serde(deny_unknown_fields)]` anywhere: `PaneInfo`
//! gained 7 fields and `AgentInfo` 5 between protocol 16 and 20, and a client carrying that
//! attribute would have hard-failed on every snapshot the moment the operator ran `herdr update` —
//! an infinite crash loop under `Restart=always` on a machine whose operator has only a phone.
//!
//! The loudness that trade gives up is bought back HERE, in the one place we control: a red
//! `cargo test` naming the exact field, on the operator's terms, instead of a missed ask at 2 a.m.
//! When a test in this file goes red the schema has moved — read what it says, decide, and update
//! BOTH the model and the assertion. Never just the assertion.
//!
//! # The schema is not gospel, and the tests say so where it matters
//!
//! It under-declares (91 methods here, 92 accepted on the wire — `pane.graphics.stream` is missing)
//! and over-declares (`EventMatch` lists 19 variants while `events.wait` rejects all but one). So
//! **presence** in the dump is meaningful; **absence** needs a live check before anyone acts on it.
//!
//! Offline: reads a checked-in file, touches no socket.
//!
//! # Spec rows -> where they live (build order step 5)
//!
//! | spec row | status |
//! |---|---|
//! | `fixture_is_the_protocol_we_target` | here, green |
//! | `every_method_we_call_still_exists` | here, green (9 names; ALL nine consts are anchored to these same literals by `src/client.rs::tests::the_method_and_tag_consts_are_the_wire_names_schema_drift_checks` since step 8) |
//! | `every_result_tag_we_assert_still_exists` | here, green |
//! | `agent_status_variants_match_ours` | here, green (both vocabularies) |
//! | `required_fields_we_treat_as_mandatory_are_still_required` | here, green (6 types, counts AND model-level enforcement) |
//! | `only_three_subscription_variants_require_pane_id` | here, green |
//! | `subscription_event_kinds_are_still_the_dot_form_three` | here, green |
//!
//! One test beyond the spec's seven rows, implementing the note under its table:
//! `shared_defs_are_identical_once_ref_prefixes_are_normalized`.

use std::collections::BTreeSet;

use herdr_client::{
    AgentInfo, AgentStatus, KNOWN_PROTOCOL, PaneAgentState, PaneInfo, PaneRead, SessionSnapshot,
    TabInfo, WorkspaceInfo,
};
use serde_json::Value;

const SCHEMA: &str = include_str!("fixtures/herdr-schema-p20.json");

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the schema fixture is well-formed JSON")
}

/// A `$defs` entry from one of the five sub-schemas.
fn def(schema: &Value, sub: &str, name: &str) -> Value {
    schema["schemas"][sub]["$defs"][name].clone()
}

/// The `required` list of a `success_response` def, as a set.
fn required(schema: &Value, name: &str) -> BTreeSet<String> {
    def(schema, "success_response", name)["required"]
        .as_array()
        .unwrap_or_else(|| panic!("{name} must declare `required`"))
        .iter()
        .map(|v| v.as_str().expect("a required entry is a string").to_owned())
        .collect()
}

/// The `properties` keys of a `success_response` def, as a set.
fn properties(schema: &Value, name: &str) -> BTreeSet<String> {
    def(schema, "success_response", name)["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("{name} must declare `properties`"))
        .keys()
        .cloned()
        .collect()
}

/// A string enum def's values, in declaration order.
fn enum_values(schema: &Value, sub: &str, name: &str) -> Vec<String> {
    def(schema, sub, name)["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("{sub}/{name} must be a string enum"))
        .iter()
        .map(|v| v.as_str().expect("an enum value is a string").to_owned())
        .collect()
}

// ── the tests ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn fixture_is_the_protocol_we_target() {
    let s = schema();
    assert_eq!(
        s["protocol"].as_u64(),
        Some(u64::from(KNOWN_PROTOCOL)),
        "the checked-in schema is not the protocol this client was built against; re-capture it \
         with scripts/capture-fixtures.sh and re-read every test in this file"
    );
    assert_eq!(s["schema_version"].as_u64(), Some(1));
    assert_eq!(
        s["schemas"].as_object().map(|m| m.len()),
        Some(5),
        "five sub-schemas: error_response, event, request, subscription_event, success_response"
    );
}

/// The nine methods this client calls.
///
/// **Comment required, per the spec, and it is load-bearing:** the dump is NOT a complete method
/// list. The wire accepts 92 methods; the dump declares 91, and `pane.graphics.stream` is the one
/// missing. So a name PRESENT here is real, but a name ABSENT here still needs a live check before
/// anyone concludes the method is gone.
///
/// **Why these are still literals after step 6.** Driving the list off `<T as Request>::METHOD`
/// is impossible from an integration test: every request type is `pub(crate)`, and making them
/// public to feed a test would widen the crate's API for no product reason. The drift hole is
/// closed from the other side instead — `src/client.rs::tests::`
/// `the_method_and_tag_consts_are_the_wire_names_schema_drift_checks` asserts each
/// `Request::METHOD` / `Request::RESULT_TAG` const equals the literal used here, in a file that
/// CAN name the private types. A rename therefore fails there, not silently past here.
/// Since build order step 8 that anchor covers all nine methods, the three writes included.
#[test]
fn every_method_we_call_still_exists() {
    let s = schema();
    let declared: BTreeSet<String> = s["schemas"]["request"]["oneOf"]
        .as_array()
        .expect("request.oneOf")
        .iter()
        .map(|v| {
            v["properties"]["method"]["const"]
                .as_str()
                .expect("every request variant pins a method const")
                .to_owned()
        })
        .collect();
    assert_eq!(
        declared.len(),
        91,
        "the dump declares 91 methods (the wire accepts 92)"
    );

    for method in [
        "ping",
        "session.snapshot",
        "pane.read",
        "events.subscribe",
        "agent.list",
        "pane.list",
        "pane.send_text",
        "pane.send_keys",
        "pane.send_input",
    ] {
        assert!(
            declared.contains(method),
            "this client calls `{method}` and the schema no longer declares it"
        );
    }

    // The removal that motivates MIN_SUPPORTED_PROTOCOL: `agent.send` existed at protocol 16 and is
    // gone at 20. If it ever comes back, the fan-out reasoning in PLAN.md needs revisiting.
    assert!(
        !declared.contains("agent.send"),
        "`agent.send` is back; it was removed between protocol 16 and 20 and its absence is why \
         a missing method is modelled as unrepairable"
    );
}

/// The seven `result.type` tags this client asserts on. `client::call` compares the tag before
/// unwrapping, so a tag that vanishes turns every call of that method into `UnexpectedResult`.
#[test]
fn every_result_tag_we_assert_still_exists() {
    let s = schema();
    let tags: BTreeSet<String> =
        s["schemas"]["success_response"]["$defs"]["ResponseResult"]["oneOf"]
            .as_array()
            .expect("ResponseResult.oneOf")
            .iter()
            .map(|v| {
                v["properties"]["type"]["const"]
                    .as_str()
                    .expect("every result variant pins a type const")
                    .to_owned()
            })
            .collect();
    assert_eq!(tags.len(), 58, "58 ResponseResult tags");

    for tag in [
        "pong",
        "session_snapshot",
        "pane_read",
        "subscription_started",
        "agent_list",
        "pane_list",
        "ok",
    ] {
        assert!(
            tags.contains(tag),
            "this client expects the `{tag}` result tag"
        );
    }
}

/// A new agent status must fail HERE, by name, rather than silently becoming
/// `AgentStatus::Unrecognized` in production and never triggering a push.
#[test]
fn agent_status_variants_match_ours() {
    let s = schema();
    let declared = enum_values(&s, "success_response", "AgentStatus");
    assert_eq!(
        declared,
        vec!["idle", "working", "blocked", "done", "unknown"],
        "herdr's read-side agent status vocabulary changed; model the new value in AgentStatus \
         (and decide whether it may ever push) before updating this list"
    );
    // The link back to our code: every declared value must be MODELLED, not caught by the
    // catch-all.
    for v in &declared {
        let ours = AgentStatus::from_wire(v);
        assert!(
            !matches!(ours, AgentStatus::Unrecognized(_)),
            "the schema declares `{v}` and this client does not model it"
        );
        assert_eq!(ours.as_str(), v);
    }
    // ...and the two "we do not know" answers are exactly `unknown` plus the catch-all.
    assert!(AgentStatus::from_wire("unknown").is_indeterminate());
    assert!(AgentStatus::from_wire("a_status_from_2027").is_indeterminate());
    for v in ["idle", "working", "blocked", "done"] {
        assert!(!AgentStatus::from_wire(v).is_indeterminate());
    }

    // The WRITE side is a different, smaller vocabulary: no `done`.
    let declared = enum_values(&s, "request", "PaneAgentState");
    assert_eq!(declared, vec!["idle", "working", "blocked", "unknown"]);
    assert!(
        !declared.iter().any(|v| v == "done"),
        "`done` is herdr's own derivation from a `seen` bit no client can set; if it becomes \
         reportable, PLAN.md's second push trigger changes shape"
    );
    for v in &declared {
        let ours: PaneAgentState =
            serde_json::from_value(Value::String(v.clone())).expect("must decode");
        assert_eq!(ours.as_str(), v);
    }
}

/// The `required` lists this client leans on — and specifically that `state_change_seq` is STILL
/// not required, which is the whole justification for it being `Option<u64>`.
///
/// Each count is checked against the schema, and then each field is checked against the MODEL by
/// deleting it from the real fixture and requiring the decode to fail. A count alone would pass
/// against a struct that had quietly made everything optional.
#[test]
fn required_fields_we_treat_as_mandatory_are_still_required() {
    let s = schema();

    let expected: [(&str, usize, usize); 6] = [
        ("PaneInfo", 19, 7),
        ("AgentInfo", 22, 7),
        ("WorkspaceInfo", 10, 8),
        ("TabInfo", 7, 7),
        ("SessionSnapshot", 10, 7),
        ("PaneReadResult", 8, 8),
    ];
    for (name, props, req) in expected {
        assert_eq!(properties(&s, name).len(), props, "{name} property count");
        assert_eq!(required(&s, name).len(), req, "{name} required count");
    }

    // `layouts` is REQUIRED on the snapshot — which is why it is carried (opaquely) rather than
    // dropped: a client that omitted it would emit a snapshot herdr's own schema calls invalid.
    assert!(required(&s, "SessionSnapshot").contains("layouts"));

    // THE one that keeps `Option<u64>` honest.
    let agent_req = required(&s, "AgentInfo");
    assert!(
        !agent_req.contains("state_change_seq"),
        "`state_change_seq` became required; a bare u64 is now safe, but check the default-0 \
         semantics before changing the model"
    );
    assert_eq!(
        def(&s, "success_response", "AgentInfo")["properties"]["state_change_seq"]["default"]
            .as_u64(),
        Some(0),
        "the schema still declares default 0 — which is exactly the value a bare `u64` would \
         silently collapse every pane to"
    );

    // The model half: deleting a required field must FAIL the decode.
    let snapshot_line: Value =
        serde_json::from_str(include_str!("fixtures/snapshot.json")).unwrap();
    let snap = snapshot_line["result"]["snapshot"].clone();
    let pane = snap["panes"][0].clone();

    // One block per type: `required` says the field must be there, so the model must refuse a
    // payload without it. A count assertion alone would pass against a struct that had quietly
    // made every field `Option`.
    macro_rules! required_is_enforced {
        ($ty:ty, $def:expr, $sample:expr) => {{
            let sample: Value = $sample;
            let req = required(&s, $def);
            assert!(!req.is_empty());
            for field in req {
                let mut broken = sample.clone();
                broken
                    .as_object_mut()
                    .expect("the sample is an object")
                    .remove(&field);
                assert!(
                    serde_json::from_value::<$ty>(broken).is_err(),
                    "`{}.{}` is required by the schema but the model decodes without it",
                    $def,
                    field
                );
            }
            // The sample itself must still decode, or the loop above proves nothing.
            serde_json::from_value::<$ty>(sample).expect("the untouched sample must decode");
        }};
    }

    let read_line: Value = serde_json::from_str(include_str!("fixtures/pane_read.json")).unwrap();
    required_is_enforced!(PaneInfo, "PaneInfo", pane.clone());
    required_is_enforced!(AgentInfo, "AgentInfo", snap["agents"][0].clone());
    required_is_enforced!(
        WorkspaceInfo,
        "WorkspaceInfo",
        snap["workspaces"][0].clone()
    );
    required_is_enforced!(TabInfo, "TabInfo", snap["tabs"][0].clone());
    required_is_enforced!(SessionSnapshot, "SessionSnapshot", snap.clone());
    required_is_enforced!(
        PaneRead,
        "PaneReadResult",
        read_line["result"]["read"].clone()
    );
    // ...and deleting the NOT-required one must still decode, and must not resurrect as 0.
    let mut without_seq = snap.clone();
    for agent in without_seq["agents"].as_array_mut().unwrap() {
        agent.as_object_mut().unwrap().remove("state_change_seq");
    }
    let typed: SessionSnapshot =
        serde_json::from_value(without_seq).expect("an agent without state_change_seq must decode");
    assert!(
        typed.agents.iter().all(|a| a.state_change_seq.is_none()),
        "an absent state_change_seq must stay absent, never collapse to Some(0)"
    );
    assert!(
        !serde_json::to_value(&typed.agents[0])
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("state_change_seq"),
        "and it must not be re-emitted"
    );
}

/// If a GLOBAL agent-status subscription ever appears, this test fails and tells slice 3 it can
/// drop the per-pane fan-out that currently costs one connection per agent pane.
#[test]
fn only_three_subscription_variants_require_pane_id() {
    let s = schema();
    let variants = s["schemas"]["request"]["$defs"]["Subscription"]["oneOf"]
        .as_array()
        .expect("Subscription.oneOf");
    assert_eq!(variants.len(), 27, "27 subscription variants");

    let needs_pane_id: BTreeSet<String> = variants
        .iter()
        .filter(|v| {
            v["required"]
                .as_array()
                .is_some_and(|r| r.iter().any(|x| x.as_str() == Some("pane_id")))
        })
        .map(|v| {
            v["properties"]["type"]["const"]
                .as_str()
                .expect("every subscription variant pins a type const")
                .to_owned()
        })
        .collect();

    assert_eq!(
        needs_pane_id,
        BTreeSet::from([
            "pane.output_matched".to_owned(),
            "pane.agent_status_changed".to_owned(),
            "pane.scroll_changed".to_owned(),
        ]),
        "the set of pane-scoped subscriptions changed; `pane.agent_status_changed` needing a \
         pane_id is the reason there is no global status feed and the reason the roster's \
         `pane.updated` backlog is a trap rather than an alternative"
    );

    // Every variant is dot-form, and `pane.updated` — the one with the stale replay backlog — is
    // still globally subscribable, which is what makes it dangerous rather than merely useless.
    for v in variants {
        let name = v["properties"]["type"]["const"].as_str().unwrap();
        assert!(name.contains('.'), "subscription `{name}` is not dot-form");
    }
    assert!(!needs_pane_id.contains("pane.updated"));
}

/// The schema's own record of the two-envelope-family split: SUBSCRIPTION names are dot-form,
/// EVENT names are snake_case, and they are different lists.
#[test]
fn subscription_event_kinds_are_still_the_dot_form_three() {
    let s = schema();

    let sub_kinds = enum_values(&s, "subscription_event", "SubscriptionEventKind");
    assert_eq!(
        sub_kinds,
        vec![
            "pane.output_matched",
            "pane.agent_status_changed",
            "pane.scroll_changed"
        ]
    );

    let event_kinds = enum_values(&s, "event", "EventKind");
    assert_eq!(event_kinds.len(), 26, "26 lifecycle event kinds");
    for k in &event_kinds {
        assert!(
            !k.contains('.') && k.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "lifecycle event kind `{k}` is not snake_case — the two-family split has moved"
        );
    }
    for k in &sub_kinds {
        assert!(
            !event_kinds.contains(k),
            "`{k}` now appears in BOTH families; the decoder dispatches on `event` precisely \
             because these lists are disjoint"
        );
    }

    // The two names the product actually depends on.
    assert!(sub_kinds.iter().any(|k| k == "pane.agent_status_changed"));
    assert!(event_kinds.iter().any(|k| k == "pane_updated"));
    assert!(
        event_kinds.iter().any(|k| k == "workspace_focused"),
        "tests/events.rs uses `workspace_focused` as its REAL unmodelled kind; it is a genuine \
         event name and must stay one for that test to mean anything"
    );
    // The event family also carries `pane_agent_status_changed` in snake_case. Note it is NOT the
    // frame the product's trigger arrives on: a filtered subscription answers dot-form with no
    // `data.type` at all. Both exist; the decoder must handle both.
    assert!(event_kinds.iter().any(|k| k == "pane_agent_status_changed"));

    // `EventKind` is duplicated into `success_response`; they must not diverge.
    assert_eq!(
        enum_values(&s, "success_response", "EventKind"),
        event_kinds,
        "the two copies of EventKind disagree"
    );
}

/// The dump repeats 27 `$defs` names across sub-schemas with sub-schema-local `$ref` prefixes.
/// They are IDENTICAL once the prefix is normalized — which is what licenses every test above to
/// read `PaneInfo`, `AgentStatus` and friends from `success_response` alone and treat the answer as
/// true for the event stream too.
///
/// Without the rewrite this comparison red-flags on every run, so the normalization lives here
/// rather than in a reader's head.
#[test]
fn shared_defs_are_identical_once_ref_prefixes_are_normalized() {
    fn normalize(v: &Value) -> Value {
        match v {
            Value::String(s) => {
                // "#/schemas/<sub>/$defs/X" -> "#/$defs/X"
                if let Some(rest) = s.strip_prefix("#/schemas/") {
                    if let Some((_sub, tail)) = rest.split_once("/$defs/") {
                        return Value::String(format!("#/$defs/{tail}"));
                    }
                }
                v.clone()
            }
            Value::Array(a) => Value::Array(a.iter().map(normalize).collect()),
            Value::Object(m) => {
                Value::Object(m.iter().map(|(k, vv)| (k.clone(), normalize(vv))).collect())
            }
            _ => v.clone(),
        }
    }

    let s = schema();
    let subs: Vec<String> = s["schemas"].as_object().unwrap().keys().cloned().collect();

    let mut shared = 0usize;
    let mut names: BTreeSet<String> = BTreeSet::new();
    for sub in &subs {
        if let Some(defs) = s["schemas"][sub]["$defs"].as_object() {
            names.extend(defs.keys().cloned());
        }
    }
    for name in names {
        let seen: Vec<Value> = subs
            .iter()
            .filter_map(|sub| s["schemas"][sub]["$defs"].get(&name).map(normalize))
            .collect();
        if seen.len() < 2 {
            continue;
        }
        shared += 1;
        assert!(
            seen.windows(2).all(|w| w[0] == w[1]),
            "`{name}` differs between sub-schemas even after normalizing $ref prefixes; the \
             single-model assumption no longer holds for it"
        );
    }
    assert_eq!(
        shared, 27,
        "27 def names appear in more than one sub-schema"
    );
}

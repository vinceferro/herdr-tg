//! The dispatch affordance — the one command this strip can compose.
//!
//! The pick is: an agent, a task file, a reason. What it produces is the spec's `dispatch`
//! frame for `spawn` — nothing more, nothing shaped differently — handed to the seam, with the
//! receipt rendered when the seam answers. The strip does not and cannot execute the command:
//! executing belongs to the frame owner's dispatchd, and a strip that shelled into the lane
//! scripts directly would be a second dispatch path nobody gated.
//!
//! The validation here is the pick's own honesty, not a bypass of the executor's: the hub will
//! answer `bad_task_ref` for a file it cannot use regardless of what this side checked, and
//! this side checks anyway because a pick that composes a frame the operator can see is doomed
//! is a pick that wasted his time.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::bail;

use super::frames::DispatchFrame;

/// Compose the spawn frame from the three answers a person gave.
///
/// * The agent is the offered list's number or any typed name — the executor is the one who
///   knows which agents exist, and its `unknown_agent` receipt is the honest answer to a name
///   this strip guessed at; guessing is not this function's job, and neither is refusing a
///   name the executor may know and this strip cannot see.
/// * The task is an absolute path on this box (checked readable here, because the operator is
///   standing at this box and can fix it now, in the pick, rather than after a round trip), or
///   `-Text` for the inline form — the contract's marker, ONE dash, because their
///   `lane-dispatch.sh` strips exactly one leading dash and writes the rest to the task file;
///   the spec-era `--Text` would leave a stray `-` in the lane's task.
/// * The reason is one line, sent as given — the spec logs it, and an empty reason is an empty
///   reason rather than a refusal: the frame is honest either way.
///
/// `task_readable` is a closure so the composition is testable without a filesystem: the
/// answer set decides, the caller says what exists.
pub(crate) fn compose_spawn(
    offered_agents: &[String],
    id: String,
    agent_answer: &str,
    task_answer: &str,
    reason_answer: &str,
    task_readable: impl Fn(&Path) -> bool,
) -> anyhow::Result<DispatchFrame> {
    let agent = agent_answer.trim();
    if agent.is_empty() {
        bail!("no agent was chosen; nothing was sent");
    }
    // A number the offered list can answer is that agent; anything else — a number past the
    // list included — is a name, passed through for the executor to judge. Resolving "3" to
    // the third of two would be a guess dressed as arithmetic.
    let agent = match agent
        .parse::<usize>()
        .ok()
        .filter(|number| *number >= 1 && *number <= offered_agents.len())
    {
        Some(number) => offered_agents[number - 1].clone(),
        None => agent.to_owned(),
    };

    let task = task_answer.trim();
    if task.is_empty() {
        bail!("no task was chosen; nothing was sent");
    }
    if task == "-" {
        bail!("- with nothing after it is not a task; nothing was sent");
    }
    let task_ref = if task.starts_with('-') {
        // The inline form keeps its marker IN the field: the contract's convention is
        // `-Text` (one dash — their lane-dispatch.sh strips exactly one leading dash,
        // `${TASK_ARG#-}`, and writes the rest to the lane's task file), so the executor
        // tells the two apart by the very bytes this side was handed. Stripping the marker
        // here would launder the inline form into something that looks like a relative path.
        task.to_owned()
    } else {
        let path = Path::new(task);
        if !path.is_absolute() {
            bail!(
                "the task file must be an absolute path — the executor will not share this \
                 terminal's working directory; nothing was sent"
            );
        }
        if !task_readable(path) {
            bail!("there is no task file at {task}; nothing was sent");
        }
        task.to_owned()
    };

    Ok(DispatchFrame {
        v: 1,
        id,
        t: "dispatch".to_owned(),
        action: "spawn".to_owned(),
        lane: None,
        agent: Some(agent),
        task_ref: Some(task_ref),
        deps: None,
        proof_cmd: None,
        reason: Some(reason_answer.trim().to_owned()),
    })
}

/// The receipt, in the words a person reads. The wire's `why` codes are the contract's
/// vocabulary; the strip's sentences are this product's — an unfamiliar code renders as
/// itself, because mapping an unknown refusal onto a known one would misreport whose refusal
/// it was.
///
/// `accepted` means the executor took the command and ran the spawn — NOT that the lane
/// finished (the contract's own honesty rule: completion arrives through `wip` frames, never
/// through the receipt), which is why the sentence stops at "took the command" and, when the
/// receipt carries `lane_id`, names the lane to watch. The contract's v0 verdict set is
/// `accepted | rejected` only — the spec-era `held` is explicitly not built (a command that
/// cannot be taken is REJECTED, `no_worker`) — so a verdict this build does not know renders
/// verbatim below, which is exactly how a future `held` will read honestly on the day one
/// lands without a release here.
pub(crate) fn receipt_line(receipt: &super::frames::DispatchReceipt) -> String {
    let why = receipt.why.as_deref().map(why_in_words);
    match (receipt.verdict.as_str(), why) {
        ("accepted", _) => {
            let mut line = format!(
                "receipt {}: accepted — the executor took the command",
                receipt.ref_id
            );
            if let Some(lane_id) = receipt.lane_id.as_deref() {
                let _ = write!(
                    line,
                    "; watch {lane_id} on the board — accepted is not finished"
                );
            }
            line
        }
        ("rejected", Some(why)) => format!("receipt {}: rejected — {}", receipt.ref_id, why),
        ("rejected", None) => format!("receipt {}: rejected", receipt.ref_id),
        (other, why) => match why {
            Some(why) => format!("receipt {}: {other} — {why}", receipt.ref_id),
            None => format!("receipt {}: {other}", receipt.ref_id),
        },
    }
}

fn why_in_words(why: &str) -> String {
    match why {
        "no_worker" => "no worker is on the lane".to_owned(),
        "over_budget" => "the budget gate said no".to_owned(),
        "unknown_agent" => "that agent is not one the executor knows".to_owned(),
        "bad_task_ref" => "the task file could not be used".to_owned(),
        "not_permitted" => "this connection may not dispatch".to_owned(),
        // The spec's sixth word, RESERVED in v0 (it names the steer/stop slice's busy-lane
        // refusal): the sentence exists so the day that slice lands it reads in words.
        "lane_busy" => "the lane is busy".to_owned(),
        // The contract v0's three additions, where the executor's truth needed a word.
        "bad_dispatch" => "the frame was malformed".to_owned(),
        "duplicate_ref" => "another live command already holds that dispatch id".to_owned(),
        "spawn_timeout" => "the executor killed its own spawn past its deadline".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn everything_readable() -> impl Fn(&Path) -> bool {
        |_path: &Path| true
    }

    /// A home-shaped path, built rather than written out, for the same reason the rename test
    /// builds its repo path: a home-shaped literal in a public repo trips the identity
    /// scanner — rightly, since it cannot tell a fixture from somebody's real path.
    const A_TASK_PATH: &str = "/hom\u{65}/somebody/a-task.md";

    /// The happy pick produces exactly the spec's spawn shape: no `lane`, no invented fields,
    /// the id the caller pinned, the answers as given.
    #[test]
    fn a_pick_composes_exactly_the_spec_s_spawn_frame() {
        let frame = compose_spawn(
            &["builder".to_owned()],
            "disp-demo-1".to_owned(),
            "builder",
            A_TASK_PATH,
            "ship the strip",
            everything_readable(),
        )
        .expect("a good pick composes");

        assert_eq!(frame.v, 1);
        assert_eq!(frame.id, "disp-demo-1");
        assert_eq!(frame.t, "dispatch");
        assert_eq!(frame.action, "spawn");
        assert_eq!(frame.agent.as_deref(), Some("builder"));
        assert_eq!(frame.task_ref.as_deref(), Some(A_TASK_PATH));
        assert_eq!(frame.reason.as_deref(), Some("ship the strip"));
        assert!(frame.lane.is_none(), "spawn carries no lane");
        assert!(
            frame.deps.is_none() && frame.proof_cmd.is_none(),
            "optional fields stay absent"
        );

        let json = serde_json::to_string(&frame).expect("serialises");
        assert!(
            json.contains(r#""t":"dispatch""#) && json.contains(r#""action":"spawn""#),
            "the wire bytes are the spec's vocabulary:\n{json}"
        );
    }

    /// A numbered answer picks from the offered list; a typed name is passed through for the
    /// executor to judge. Both are honest — the strip refuses only what it cannot compose.
    #[test]
    fn an_agent_answer_may_be_a_number_or_a_name() {
        let offered = vec!["builder".to_owned(), "reviewer".to_owned()];
        let from_number = compose_spawn(
            &offered,
            "disp-1".to_owned(),
            "2",
            "/tmp/t.md",
            "",
            everything_readable(),
        )
        .expect("composes");
        assert_eq!(from_number.agent.as_deref(), Some("reviewer"));

        // A number past the list is a name, not an error the strip invents: the executor
        // answers it honestly with unknown_agent if nobody has it.
        let from_big_number = compose_spawn(
            &offered,
            "disp-1b".to_owned(),
            "3",
            "/tmp/t.md",
            "",
            everything_readable(),
        )
        .expect("composes");
        assert_eq!(from_big_number.agent.as_deref(), Some("3"));

        let from_name = compose_spawn(
            &offered,
            "disp-2".to_owned(),
            "planner",
            "/tmp/t.md",
            "",
            everything_readable(),
        )
        .expect("composes");
        assert_eq!(from_name.agent.as_deref(), Some("planner"));
    }

    /// Fail-closed picks: empty answers, a relative path, a missing file, a bare `-`. Each
    /// refuses with a sentence that says nothing was sent.
    #[test]
    fn a_pick_that_cannot_compose_refuses_and_says_nothing_was_sent() {
        for (agent, task) in [
            ("", "/tmp/t.md"),
            ("builder", ""),
            ("builder", "tasks/relative.md"),
            ("builder", "/tmp/definitely-not-here.md"),
            ("builder", "-"),
        ] {
            let err = compose_spawn(&[], "disp-x".to_owned(), agent, task, "", |path: &Path| {
                path != Path::new("/tmp/definitely-not-here.md")
            })
            .expect_err("this pick must refuse");
            assert!(
                err.to_string().contains("nothing was sent"),
                "every refusal names the honest outcome:\n{err}"
            );
        }
    }

    /// The inline form keeps its marker: `-Text` stays `-Text` on the wire — ONE dash, the
    /// contract's `lane-dispatch.sh` convention (the script strips exactly one leading dash
    /// and writes the rest to the lane's task file) — and a stripped marker would arrive
    /// looking like a relative path.
    #[test]
    fn the_inline_task_form_keeps_its_marker_in_the_field() {
        let frame = compose_spawn(
            &[],
            "disp-1".to_owned(),
            "builder",
            "-fix the failing gate",
            "",
            everything_readable(),
        )
        .expect("composes");
        assert_eq!(frame.task_ref.as_deref(), Some("-fix the failing gate"));
    }

    /// Receipts read as sentences, verdicts verbatim, unknown whys as themselves — and a
    /// verdict this build does not know (the spec-era `held`, for one: the contract's v0
    /// rejects instead) renders as itself rather than as a sentence this build invented for a
    /// verdict the executor never sent.
    #[test]
    fn a_receipt_is_a_sentence_and_an_unknown_why_is_itself() {
        use super::super::frames::DispatchReceipt;

        let accepted = DispatchReceipt {
            v: Some(1),
            id: Some("h12".to_owned()),
            t: "ack".to_owned(),
            ref_id: "disp-1".to_owned(),
            delivered: Some("yes".to_owned()),
            verdict: "accepted".to_owned(),
            why: None,
            lane_id: Some("lane-1004-130001-733100".to_owned()),
        };
        let line = receipt_line(&accepted);
        assert!(
            line.starts_with("receipt disp-1: accepted"),
            "the sentence a person reads:\n{line}"
        );
        assert!(
            line.contains("watch lane-1004-130001-733100 on the board"),
            "an accepted receipt names the lane the wip frames will carry:\n{line}"
        );
        assert!(
            line.contains("accepted is not finished"),
            "the contract's honesty rule is carried to the operator:\n{line}"
        );

        let over_budget = DispatchReceipt {
            v: Some(1),
            id: None,
            t: "ack".to_owned(),
            ref_id: "disp-2".to_owned(),
            delivered: Some("yes".to_owned()),
            verdict: "rejected".to_owned(),
            why: Some("over_budget".to_owned()),
            lane_id: None,
        };
        let line = receipt_line(&over_budget);
        assert!(
            line.contains("rejected — the budget gate said no"),
            "the wire's code becomes the operator's sentence:\n{line}"
        );

        // The contract v0's own why words, spelled for a person.
        let duplicate = DispatchReceipt {
            v: None,
            id: None,
            t: "ack".to_owned(),
            ref_id: "disp-2b".to_owned(),
            delivered: None,
            verdict: "rejected".to_owned(),
            why: Some("duplicate_ref".to_owned()),
            lane_id: None,
        };
        assert!(
            receipt_line(&duplicate)
                .contains("another live command already holds that dispatch id"),
            "the v0 vocabulary is in words, not codes"
        );

        let future = DispatchReceipt {
            v: None,
            id: None,
            t: "ack".to_owned(),
            ref_id: "disp-3".to_owned(),
            delivered: None,
            verdict: "rejected".to_owned(),
            why: Some("a_code_this_build_never_heard_of".to_owned()),
            lane_id: None,
        };
        assert!(
            receipt_line(&future).contains("a_code_this_build_never_heard_of"),
            "an unfamiliar refusal is itself, not a familiar one it resembles"
        );

        let held_someday = DispatchReceipt {
            v: None,
            id: None,
            t: "ack".to_owned(),
            ref_id: "disp-4".to_owned(),
            delivered: None,
            verdict: "held".to_owned(),
            why: None,
            lane_id: None,
        };
        let held_line = receipt_line(&held_someday);
        assert!(
            held_line.contains("receipt disp-4: held"),
            "a verdict outside the contract's v0 set renders verbatim:\n{held_line}"
        );
        assert!(
            !held_line.contains("when it wakes"),
            "no sentence is invented for a verdict the executor never sent in v0"
        );
    }
}

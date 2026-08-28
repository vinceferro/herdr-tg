//! `herdr-tg status` — the herd, human or as the full RPC envelope.

use anyhow::{Context, bail};
use herdr_client::{HerdrClient, SessionSnapshot};

use crate::render;

/// Fetch the snapshot, optionally narrow it to one workspace, then render or emit it.
///
/// The handshake runs first, on every invocation. It costs one tiny RPC and buys the thing a
/// phone-only operator cannot debug remotely: a server older than this client understands exits
/// **4** with a message naming the protocol, instead of surfacing as a confusing decode failure
/// three fields deep in a snapshot.
pub(crate) async fn run(
    client: &HerdrClient,
    json: bool,
    workspace: Option<&str>,
) -> anyhow::Result<()> {
    client.handshake().await?;
    let mut snapshot = client.snapshot().await?;

    if let Some(selector) = workspace {
        narrow_to_workspace(&mut snapshot, selector)?;
    }

    if json {
        let envelope = super::envelope("session_snapshot", "snapshot", &snapshot)
            .context("re-serializing the snapshot into its RPC envelope")?;
        super::print_json(&envelope)
    } else {
        print!("{}", render::herd_table(&snapshot));
        Ok(())
    }
}

/// Keep one workspace and everything that belongs to it; drop the rest.
///
/// D2 is one bot per workspace, so this is the shape slice 2 wants. Two decisions worth stating:
///
/// - The result is still a **complete, valid `session_snapshot`** — `--json` must not degrade into
///   a bespoke shape the moment a filter is applied, or the proof surface and the product surface
///   stop being the same code path.
/// - `focused_*_id` is cleared when it points outside the retained set. Reporting a focused pane
///   that is not in the table would be a quiet lie about which pane the operator is looking at.
///
/// `layouts` is carried through verbatim: it is opaque to this client by design (the spec is
/// explicit that pane geometry is not modelled), and filtering something we cannot parse would be
/// guesswork.
pub(crate) fn narrow_to_workspace(
    snapshot: &mut SessionSnapshot,
    selector: &str,
) -> anyhow::Result<()> {
    let Some(target) = snapshot
        .workspaces
        .iter()
        .find(|ws| ws.workspace_id.as_str() == selector || ws.label == selector)
        .map(|ws| ws.workspace_id.clone())
    else {
        let known: Vec<String> = snapshot
            .workspaces
            .iter()
            .map(|ws| format!("{} ({})", ws.workspace_id, ws.label))
            .collect();
        bail!(
            "no workspace matches {selector:?} by id or label; this herd has: {}",
            if known.is_empty() {
                "none".to_owned()
            } else {
                known.join(", ")
            }
        );
    };

    snapshot.workspaces.retain(|ws| ws.workspace_id == target);
    snapshot.tabs.retain(|tab| tab.workspace_id == target);
    snapshot.panes.retain(|pane| pane.workspace_id == target);
    snapshot.agents.retain(|agent| agent.workspace_id == target);

    if snapshot.focused_workspace_id.as_ref() != Some(&target) {
        snapshot.focused_workspace_id = None;
    }
    let kept_tab = snapshot
        .focused_tab_id
        .as_ref()
        .is_some_and(|id| snapshot.tabs.iter().any(|tab| &tab.tab_id == id));
    if !kept_tab {
        snapshot.focused_tab_id = None;
    }
    let kept_pane = snapshot
        .focused_pane_id
        .as_ref()
        .is_some_and(|id| snapshot.panes.iter().any(|pane| &pane.pane_id == id));
    if !kept_pane {
        snapshot.focused_pane_id = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> SessionSnapshot {
        let raw = include_str!("../../../herdr-client/tests/fixtures/snapshot.json");
        let value: serde_json::Value = serde_json::from_str(raw).expect("fixture is JSON");
        serde_json::from_value(value["result"]["snapshot"].clone()).expect("fixture decodes")
    }

    #[test]
    fn narrowing_by_id_and_by_label_select_the_same_workspace() {
        let mut by_id = snapshot();
        narrow_to_workspace(&mut by_id, "wA").expect("wA is in the fixture");
        let mut by_label = snapshot();
        narrow_to_workspace(&mut by_label, "desktop-lab").expect("its label is in the fixture");

        assert_eq!(by_id.workspaces.len(), 1);
        assert_eq!(
            by_id.workspaces[0].workspace_id,
            by_label.workspaces[0].workspace_id
        );
        assert!(by_id.panes.iter().all(|p| p.workspace_id.as_str() == "wA"));
        assert!(by_id.agents.iter().all(|a| a.workspace_id.as_str() == "wA"));
        assert!(by_id.tabs.iter().all(|t| t.workspace_id.as_str() == "wA"));
    }

    /// The narrowed view must not claim a pane is focused when that pane is no longer in it.
    #[test]
    fn focus_pointers_outside_the_kept_workspace_are_cleared() {
        let full = snapshot();
        let focused_ws = full
            .focused_workspace_id
            .clone()
            .expect("the fixture has a focused workspace");
        let other = full
            .workspaces
            .iter()
            .map(|ws| ws.workspace_id.clone())
            .find(|id| id != &focused_ws)
            .expect("the fixture has more than one workspace");

        let mut narrowed = snapshot();
        narrow_to_workspace(&mut narrowed, other.as_str()).expect("workspace exists");
        assert_eq!(narrowed.focused_workspace_id, None);
        assert_eq!(narrowed.focused_pane_id, None);
        assert_eq!(narrowed.focused_tab_id, None);
    }

    #[test]
    fn an_unknown_workspace_is_an_error_that_lists_the_real_ones() {
        let mut snap = snapshot();
        let err = narrow_to_workspace(&mut snap, "not-a-workspace")
            .expect_err("an unknown selector must not silently return an empty herd");
        let message = err.to_string();
        assert!(message.contains("not-a-workspace"), "{message}");
        assert!(message.contains("acme-monorepo"), "{message}");
    }

    /// A narrowed snapshot is still a snapshot: `--json` must not degrade into a bespoke shape.
    #[test]
    fn a_narrowed_snapshot_still_round_trips_as_a_session_snapshot() {
        let mut snap = snapshot();
        narrow_to_workspace(&mut snap, "wD").expect("wD is in the fixture");
        let envelope = super::super::envelope("session_snapshot", "snapshot", &snap)
            .expect("a decoded snapshot re-serializes");
        assert_eq!(envelope["result"]["type"], "session_snapshot");
        let back: SessionSnapshot = serde_json::from_value(envelope["result"]["snapshot"].clone())
            .expect("the emitted envelope decodes as a SessionSnapshot");
        assert_eq!(back.panes.len(), 1);
        assert_eq!(back.protocol, 20);
    }
}

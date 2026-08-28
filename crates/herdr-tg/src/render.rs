//! The human herd table — what an operator reads when they are not piping into `jq`.
//!
//! Deliberately not a general table library: six panes on a phone screen, one line each, in
//! **array order**. herdr's `tabs` and `panes` arrays are ordered; sorting them by `number` would
//! silently disagree with what the operator sees in their own terminal.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use herdr_client::{Event, PaneInfo, RosterEvent, SessionSnapshot, WorkspaceId};

/// One line per pane, `*` marking focus.
///
/// ```text
/// herd: 6 workspaces, 6 panes   (herdr 0.8.2, protocol 20)
/// * w9:p1  acme-monorepo  idle     opencode  OC | sample task one
///   wE:p1  bridge-tg      blocked  claude    sample task six
/// ```
///
/// Column widths are measured from the data rather than hard-coded, so a long workspace label
/// cannot push the status column out of alignment and make the table unreadable — status is the
/// column the operator is actually scanning.
pub fn herd_table(snap: &SessionSnapshot) -> String {
    let labels: BTreeMap<&WorkspaceId, &str> = snap
        .workspaces
        .iter()
        .map(|ws| (&ws.workspace_id, ws.label.as_str()))
        .collect();

    let rows: Vec<Row<'_>> = snap
        .panes
        .iter()
        .map(|pane| Row {
            focused: pane.focused,
            pane_id: pane.pane_id.as_str(),
            workspace: labels
                .get(&pane.workspace_id)
                .copied()
                .unwrap_or_else(|| pane.workspace_id.as_str()),
            status: pane.agent_status.as_str(),
            agent: agent_name(pane),
            title: title(pane),
        })
        .collect();

    let mut out = String::new();
    let _ = writeln!(
        out,
        "herd: {} workspace{}, {} pane{}   (herdr {}, protocol {})",
        snap.workspaces.len(),
        plural(snap.workspaces.len()),
        snap.panes.len(),
        plural(snap.panes.len()),
        snap.version,
        snap.protocol,
    );

    if rows.is_empty() {
        // Not an error: an empty herd is a real, reportable state (herdr is up, nothing is
        // running). Saying so beats printing a bare header the operator has to interpret.
        out.push_str("  (no panes)\n");
        return out;
    }

    let w_pane = width(rows.iter().map(|r| r.pane_id));
    let w_ws = width(rows.iter().map(|r| r.workspace));
    let w_status = width(rows.iter().map(|r| r.status));
    let w_agent = width(rows.iter().map(|r| r.agent));

    for row in &rows {
        let marker = if row.focused { '*' } else { ' ' };
        let _ = write!(
            out,
            "{marker} {:<w_pane$}  {:<w_ws$}  {:<w_status$}  {:<w_agent$}",
            row.pane_id, row.workspace, row.status, row.agent,
        );
        if row.title.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "  {}", row.title);
        }
    }
    out
}

/// One decoded event, one line, **named by its wire `event` string**.
///
/// The wire name is not decoration: the stream multiplexes two incompatible envelope encodings on
/// one connection — lifecycle frames arrive snake_case (`pane_updated`) and the product's one
/// load-bearing event arrives dot-form (`pane.agent_status_changed`) with no `type` inside `data`
/// at all. Printing the family each frame came from is what makes proof gate 5 able to assert that
/// the dot-form decoder ran, and what makes a mis-decode visible to a human reading the output
/// rather than silent.
pub fn event_line(event: &Event) -> String {
    match event {
        Event::AgentStatus(changed) => {
            let mut line = format!(
                "pane.agent_status_changed  {}  {}  workspace={}",
                changed.pane_id,
                changed.agent_status.as_str(),
                changed.workspace_id,
            );
            if let Some(agent) = changed
                .display_agent
                .as_deref()
                .or(changed.agent.as_deref())
            {
                let _ = write!(line, " agent={agent}");
            }
            if let Some(title) = changed.title.as_deref() {
                let _ = write!(line, " title={title:?}");
            }
            line
        }
        // NOTE: no status is printed for the roster family, and there is none to print — the
        // decoded `RosterEvent` structurally cannot carry one. `pane.updated` replays an ageing
        // backlog on every connect, each frame holding a HISTORICAL status, so a line that showed
        // it here would read exactly like a fresh edge.
        Event::Roster(roster) => roster_line(roster),
        Event::ScrollChanged(data) => format!("pane.scroll_changed  {data}"),
        Event::OutputMatched(data) => format!("pane.output_matched  {data}"),
        Event::Unrecognized { event, data } => format!("unrecognized:{event}  {data}"),
        // `Event` is #[non_exhaustive]: a kind modelled by a later slice must not silently render
        // as nothing.
        other => format!("unprintable event  {other:?}"),
    }
}

fn roster_line(roster: &RosterEvent) -> String {
    match roster {
        RosterEvent::PaneCreated {
            pane_id,
            workspace_id,
        } => {
            format!("pane_created  {pane_id}  workspace={workspace_id}")
        }
        RosterEvent::PaneUpdated {
            pane_id,
            workspace_id,
        } => {
            format!("pane_updated  {pane_id}  workspace={workspace_id}")
        }
        RosterEvent::PaneClosed {
            pane_id,
            workspace_id,
        } => {
            format!("pane_closed  {pane_id}  workspace={workspace_id}")
        }
        RosterEvent::PaneExited {
            pane_id,
            workspace_id,
        } => {
            format!("pane_exited  {pane_id}  workspace={workspace_id}")
        }
        // `previous_pane_id` is the whole value of this frame: a moved pane gets a NEW id and the
        // old one stops resolving while the agent is still alive.
        RosterEvent::PaneMoved {
            previous_pane_id,
            pane_id,
            workspace_id,
        } => {
            format!("pane_moved  {previous_pane_id} -> {pane_id}  workspace={workspace_id}")
        }
        RosterEvent::PaneAgentDetected {
            pane_id,
            workspace_id,
            agent,
            released,
        } => {
            format!(
                "pane_agent_detected  {pane_id}  workspace={workspace_id} agent={} released={}",
                agent.as_deref().unwrap_or("-"),
                released.map_or("-".to_owned(), |r| r.to_string()),
            )
        }
        RosterEvent::WorkspaceClosed { workspace_id } => {
            format!("workspace_closed  workspace={workspace_id}")
        }
        other => format!("roster event  {other:?}"),
    }
}

struct Row<'a> {
    focused: bool,
    pane_id: &'a str,
    workspace: &'a str,
    status: &'a str,
    agent: &'a str,
    title: &'a str,
}

/// Which agent, as herdr would name it. `-` when herdr has detected none — an honest blank rather
/// than a guess, because "no agent detected" and "an agent we cannot name" are different states.
fn agent_name(pane: &PaneInfo) -> &str {
    pane.display_agent
        .as_deref()
        .or(pane.agent.as_deref())
        .unwrap_or("-")
}

/// The pane's own title, preferring the stripped form.
///
/// `terminal_title` carries the agent's decoration (`✳ `) and `terminal_title_stripped` does not;
/// on a phone the decoration is noise. Both are agent-authored only *while* an agent owns the
/// pane — a shell prompt otherwise — and both are volatile, which is why the live proof drops them
/// and an offline fixture test proves they decode.
fn title(pane: &PaneInfo) -> &str {
    pane.terminal_title_stripped
        .as_deref()
        .or(pane.terminal_title.as_deref())
        .or(pane.title.as_deref())
        .unwrap_or("")
}

/// Column width in `char`s, not bytes: workspace labels and titles are UTF-8 and a byte width
/// would misalign every row after the first non-ASCII one.
fn width<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    values.map(|v| v.chars().count()).max().unwrap_or(0)
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture is a real snapshot captured from the live herd — real envelope, real key set,
    /// real optional-field presence — with the identifying VALUES replaced by
    /// `scripts/scrub-fixtures.py`. So this still pins the rendering against a shape herdr
    /// actually emitted rather than against a hand-written struct, without committing the
    /// operator's project names to a public repo.
    fn snapshot() -> SessionSnapshot {
        let raw = include_str!("../../herdr-client/tests/fixtures/snapshot.json");
        let value: serde_json::Value = serde_json::from_str(raw).expect("fixture is JSON");
        serde_json::from_value(value["result"]["snapshot"].clone()).expect("fixture decodes")
    }

    #[test]
    fn table_renders_one_line_per_pane_in_array_order() {
        let snap = snapshot();
        let table = herd_table(&snap);
        let lines: Vec<&str> = table.lines().collect();

        assert_eq!(
            lines.len(),
            1 + snap.panes.len(),
            "header plus one line per pane"
        );
        assert!(
            lines[0].starts_with("herd: 6 workspaces, 6 panes"),
            "{}",
            lines[0]
        );
        assert!(lines[0].contains("protocol 20"), "{}", lines[0]);

        for (line, pane) in lines[1..].iter().zip(&snap.panes) {
            assert!(
                line.contains(pane.pane_id.as_str()),
                "row order must follow the panes array: {line}"
            );
        }
    }

    /// The status column is the payload. If a rename ever drops it, the table still "renders" —
    /// so it is asserted by value, per pane.
    #[test]
    fn every_row_carries_its_pane_status_and_workspace_label() {
        let snap = snapshot();
        let table = herd_table(&snap);
        for (line, pane) in table.lines().skip(1).zip(&snap.panes) {
            assert!(line.contains(pane.agent_status.as_str()), "{line}");
        }
        assert!(
            table.contains("acme-monorepo"),
            "workspace labels, not ids:\n{table}"
        );
        assert!(table.contains("notes-linkmap"), "{table}");
    }

    /// The focus marker is what tells the operator which pane is theirs. Exactly one pane is
    /// focused in the fixture.
    #[test]
    fn focus_marker_is_on_the_focused_pane_only() {
        let snap = snapshot();
        let table = herd_table(&snap);
        let marked: Vec<&str> = table
            .lines()
            .skip(1)
            .filter(|l| l.starts_with('*'))
            .collect();
        let focused: Vec<&str> = snap
            .panes
            .iter()
            .filter(|p| p.focused)
            .map(|p| p.pane_id.as_str())
            .collect();
        assert_eq!(marked.len(), focused.len());
        for (line, id) in marked.iter().zip(&focused) {
            assert!(line.contains(id), "{line}");
        }
    }

    /// The gate-5 assertion, offline: both envelope families must render under their own wire
    /// names, from real captured bytes. If the dot-form decoder ever regressed, the status line
    /// would stop appearing here before it stopped appearing in the live proof.
    #[test]
    fn both_event_families_render_under_their_wire_names() {
        let frames = include_str!("../../herdr-client/tests/fixtures/events-mixed.ndjson");
        let mut lines = Vec::new();
        for frame in frames.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(event) = herdr_client::proto::event::decode_event(frame) {
                lines.push(event_line(&event));
            }
        }
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("pane.agent_status_changed  ")),
            "the dot-form family must render:\n{lines:#?}"
        );
        assert!(
            lines.iter().any(|l| l.starts_with("pane_updated  ")),
            "the snake_case roster family must render:\n{lines:#?}"
        );
    }

    /// The roster family carries a historical `agent_status` on the wire and the decoded type has
    /// nowhere to put it. The rendered line must not reintroduce one by any other route.
    #[test]
    fn a_roster_line_never_shows_a_status() {
        let frames = include_str!("../../herdr-client/tests/fixtures/events-mixed.ndjson");
        for frame in frames.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(event) = herdr_client::proto::event::decode_event(frame) else {
                continue;
            };
            if let Event::Roster(_) = &event {
                let line = event_line(&event);
                for status in ["idle", "working", "blocked", "done"] {
                    assert!(
                        !line.contains(status),
                        "roster line leaked a status: {line}"
                    );
                }
            }
        }
    }

    /// A herd with no panes must say so rather than printing a bare header.
    #[test]
    fn an_empty_herd_says_so() {
        let mut snap = snapshot();
        snap.panes.clear();
        snap.workspaces.clear();
        let table = herd_table(&snap);
        assert!(table.contains("0 workspaces, 0 panes"), "{table}");
        assert!(table.contains("(no panes)"), "{table}");
    }
}

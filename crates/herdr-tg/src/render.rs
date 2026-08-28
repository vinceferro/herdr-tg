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

// ── The Telegram view ────────────────────────────────────────────────────────────────────────────

/// The herd, rendered for a phone.
///
/// [`herd_table`] is 92 characters wide on a real herd. Inside Telegram's `<pre>` block that wraps
/// into unreadable ribbon on a phone — the operator's word for it was "a bit cryptic". This view
/// throws away the tabulation and organises by the only question that matters on a phone: **is
/// anything waiting for me?**
///
/// Three deliberate choices:
///
/// - **Blocked first, always.** `blocked` is the one status that needs a human. Sorting by pane id
///   or array order buries the ask among shells. The whole product exists to surface that line.
/// - **Shell panes collapse to a count.** A pane with no agent is never an ask, and its "title" is
///   the shell prompt — which on this box is `user@host:~/path`, i.e. pure noise that also leaks the
///   operator's username into a chat message.
/// - **Emoji carry the status.** They survive Telegram's font stack, are scannable at a glance, and
///   cost no horizontal room. The word is kept beside them so the meaning does not depend on
///   recognising a colour.
///
/// Returns **escaped HTML**, ready to send — every piece of agent-authored text (workspace labels,
/// pane titles) goes through [`escape_html`], because an agent can print anything.
pub fn herd_telegram(snap: &SessionSnapshot) -> String {
    let labels: BTreeMap<&WorkspaceId, &str> = snap
        .workspaces
        .iter()
        .map(|ws| (&ws.workspace_id, ws.label.as_str()))
        .collect();

    let mut out = String::new();
    let _ = writeln!(
        out,
        "<b>{} workspace{} · {} pane{}</b>",
        snap.workspaces.len(),
        plural(snap.workspaces.len()),
        snap.panes.len(),
        plural(snap.panes.len()),
    );

    // Panes with no agent are shells. Counted, never listed.
    let (agentic, shells): (Vec<&PaneInfo>, Vec<&PaneInfo>) = snap
        .panes
        .iter()
        .partition(|p| p.agent.is_some() || p.display_agent.is_some());

    if agentic.is_empty() && shells.is_empty() {
        out.push_str("\n(no panes)");
        return out;
    }

    // Group order IS the priority order. `blocked` is what the operator opened the message for.
    for (status, emoji, heading) in [
        ("blocked", "🔴", "BLOCKED — waiting on you"),
        ("working", "🟡", "WORKING"),
        ("done", "✅", "DONE"),
        ("idle", "💤", "IDLE"),
        ("unknown", "❔", "UNKNOWN"),
    ] {
        let group: Vec<&&PaneInfo> = agentic
            .iter()
            .filter(|p| p.agent_status.as_str() == status)
            .collect();
        if group.is_empty() {
            continue;
        }
        let _ = write!(out, "\n\n{emoji} <b>{heading}</b>");
        for pane in group {
            let ws = labels
                .get(&pane.workspace_id)
                .copied()
                .unwrap_or_else(|| pane.workspace_id.as_str());
            let focus = if pane.focused { " ←" } else { "" };
            let _ = write!(
                out,
                "\n<b>{}</b> · <code>{}</code>{}",
                escape_html(ws),
                escape_html(pane.pane_id.as_str()),
                focus
            );
            let t = title(pane);
            if !t.is_empty() {
                let _ = write!(out, "\n  <i>{}</i>", escape_html(&clip(t, 60)));
            }
        }
    }

    if !shells.is_empty() {
        let _ = write!(
            out,
            "\n\n▫️ {} shell pane{}",
            shells.len(),
            plural(shells.len())
        );
    }
    out
}

/// Trim to `max` characters on a word boundary where one is near, with an ellipsis.
///
/// Character-based, not byte-based: pane titles routinely carry emoji and box-drawing glyphs, and
/// slicing a multi-byte char would panic in the middle of answering the operator.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    let cut = head.rfind(' ').filter(|i| *i > max * 2 / 3);
    let kept = match cut {
        Some(i) => &head[..i],
        None => head.as_str(),
    };
    format!("{}…", kept.trim_end())
}

/// Escape the three characters Telegram's HTML parse mode treats as markup.
///
/// Runs on every piece of agent-authored text that reaches a message body. A pane title is whatever
/// an agent decided to print, so it is untrusted by construction.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
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

    // ── the Telegram view ────────────────────────────────────────────────────────────────────

    /// THE property of this view: whatever is BLOCKED appears before anything else.
    ///
    /// `blocked` is the only status that needs a human, and the whole product exists to put that
    /// line in front of the operator. Array order or pane-id order buries it among shells.
    #[test]
    fn blocked_panes_come_first() {
        let out = herd_telegram(&snapshot());
        let blocked = out.find("BLOCKED").expect("the fixture has a blocked pane");
        for later in ["WORKING", "DONE", "IDLE", "shell pane"] {
            if let Some(i) = out.find(later) {
                assert!(blocked < i, "{later} appeared before BLOCKED:\n{out}");
            }
        }
    }

    /// Shell panes are never an ask, and their "title" is a shell prompt — on this box literally
    /// `user@host:~/path`, which would leak the operator's username into a chat message.
    #[test]
    fn shell_panes_are_counted_never_listed() {
        // The scrubbed fixture happens to carry an agent on every pane, so a shell pane is made
        // here rather than assumed. Asserting against the fixture as-is passed VACUOUSLY: the
        // count was simply absent, and the test only went red once it built the case it claims
        // to cover.
        let mut snap = snapshot();
        let shell = snap.panes.first_mut().expect("the fixture has panes");
        shell.agent = None;
        shell.display_agent = None;
        shell.terminal_title = Some("user@some-host:~/Projects/secret-thing".into());
        shell.terminal_title_stripped = Some("user@some-host:~/Projects/secret-thing".into());
        let shell_pane_id = shell.pane_id.as_str().to_string();

        let out = herd_telegram(&snap);
        assert!(out.contains("1 shell pane"), "the count is missing:\n{out}");
        assert!(
            !out.contains(&shell_pane_id),
            "a shell pane was listed individually:\n{out}"
        );
        assert!(
            !out.contains('@'),
            "a shell prompt reached the body — that leaks a username:\n{out}"
        );
        assert!(
            !out.contains("secret-thing"),
            "a shell pane's cwd reached the message body:\n{out}"
        );
    }

    /// Every line must fit a phone. `herd_table` is 92 characters wide, which is what made the
    /// operator call the first version "a bit cryptic".
    #[test]
    fn no_line_is_wider_than_a_phone_screen() {
        for line in herd_telegram(&snapshot()).lines() {
            // Measure the visible text, not the markup the client renders away.
            let visible = line
                .replace("<b>", "")
                .replace("</b>", "")
                .replace("<i>", "")
                .replace("</i>", "")
                .replace("<code>", "")
                .replace("</code>", "");
            assert!(
                visible.chars().count() <= 48,
                "{} chars is too wide for a phone: {visible:?}",
                visible.chars().count()
            );
        }
    }

    #[test]
    fn agent_authored_text_is_escaped() {
        assert_eq!(escape_html("<b>&</b>"), "&lt;b&gt;&amp;&lt;/b&gt;");
        // The rendered view must never contain a raw metacharacter from the data side.
        let out = herd_telegram(&snapshot());
        assert!(!out.contains("<script"));
    }

    #[test]
    fn clip_is_char_safe_and_marks_what_it_cut() {
        assert_eq!(clip("short", 60), "short");
        let long = "a ".repeat(80);
        let out = clip(&long, 60);
        assert!(out.chars().count() <= 61, "clip overran: {out:?}");
        assert!(out.ends_with('…'));
        // Multi-byte input must not panic mid-char.
        let wide = "◑日本語—".repeat(40);
        assert!(clip(&wide, 60).chars().count() <= 61);
    }

    #[test]
    fn an_empty_herd_says_so_rather_than_printing_a_bare_header() {
        let mut snap = snapshot();
        snap.panes.clear();
        snap.workspaces.clear();
        let out = herd_telegram(&snap);
        assert!(out.contains("(no panes)"), "{out}");
    }
}

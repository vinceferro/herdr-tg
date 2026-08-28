//! The domain model herdr returns: the session snapshot and everything reachable from it, plus
//! `pane.read`'s result.
//!
//! # THE SERIALIZATION RULE, and it is not stylistic
//!
//! herdr **omits** unset optional fields rather than emitting `null` (verified: a `select(.value ==
//! null)` sweep over the whole live snapshot returns `[]`; `label` / `title` / `tokens` /
//! `state_labels` / `display_agent` are absent on all 6 panes). So EVERY non-required field here is
//! `Option<T>` carrying `#[serde(default, skip_serializing_if = "Option::is_none")]` — **maps and
//! vecs included**. A bare `#[serde(default)] BTreeMap` would re-serialize as `{}` where herdr
//! emitted nothing, failing the proof's snapshot diff for a purely cosmetic reason; and a
//! `"label":null` would appear verbatim in a Telegram message body.
//!
//! # No `deny_unknown_fields`, anywhere — a deliberate uptime-over-loudness trade
//!
//! `PaneInfo` gained 7 fields and `AgentInfo` 5 between protocol 16 and 20. A p16-era client
//! carrying that attribute would have hard-failed on **every** snapshot the moment the operator ran
//! `herdr update` — under `Restart=always` + `StartLimitIntervalSec=0`, an infinite crash loop on a
//! machine whose operator has only a phone. The loudness is bought back in one place we control:
//! `tests/schema_drift.rs` reads the checked-in schema and fails `cargo test` by name.
//!
//! Residual risk, stated plainly: a field herdr *renames* becomes a silent `None`, caught only if it
//! is in a `required` list the drift test asserts.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Once;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ids::{PaneId, TabId, WorkspaceId};

// ── agent status ────────────────────────────────────────────────────────────────────────────────

/// One agent's status, as herdr sees it. Read everywhere; the product's only push trigger.
///
/// **MANUAL (de)serialize, deliberately.** `#[serde(other)]` compiles and looks right, but it
/// DISCARDS the wire string: verified by compiling both, `"reticulating"` re-serializes as the
/// literal `"unrecognized"` under `#[serde(other)]` and as `"reticulating"` under this impl. The
/// former would silently corrupt the client's own `--json` output — and therefore the proof's
/// snapshot diff — the day herdr adds a status. Agent-detection manifests are REMOTELY versioned
/// (claude on 2026.08.21.1, opencode on 2026.06.10.1, 20 manifests on this host) and gain values
/// without a herdr release, so the catch-all is not optional.
///
/// [`AgentStatus::Unrecognized`] is deliberately distinct from herdr's own [`AgentStatus::Unknown`]:
/// one means "this herdr is newer than this client", the other means "herdr does not know what this
/// agent is doing". Neither is ever a push.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    /// "Idle after work the operator has not *seen*".
    ///
    /// **CORRECTION to the build spec (which says "never observed on this host").** It WAS
    /// observed, in the very snapshot this crate is tested against: `tests/fixtures/snapshot.json`
    /// carries `agent_status: "done"` on pane `wD:p1`, on its `AgentInfo`, and on workspace `wD`.
    /// So the value is real on protocol 20 and reachable without a `pane.report_agent` that can
    /// even express it. What remains true, and is the part that matters, is that the TRANSITION is
    /// unproven end to end: the `seen` bit herdr derives `done` from is not readable from any API
    /// the bridge uses, so the bridge cannot predict when an idle pane will surface as `done` and
    /// cannot verify that the never-call-focus rule held. Slice 3 still needs one real observation
    /// of the transition (not merely of the value) before it pushes on it.
    Done,
    Unknown,
    /// A status this client was not built for, carried through **verbatim**.
    Unrecognized(String),
}

static UNRECOGNIZED_STATUS_WARNED: Once = Once::new();

impl AgentStatus {
    /// The wire string. Round-trips: `from_wire(s.as_str()) == s` for every variant, including
    /// [`AgentStatus::Unrecognized`].
    pub fn as_str(&self) -> &str {
        match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Working => "working",
            AgentStatus::Blocked => "blocked",
            AgentStatus::Done => "done",
            AgentStatus::Unknown => "unknown",
            AgentStatus::Unrecognized(s) => s,
        }
    }

    /// Never fails. An unmodelled value becomes [`AgentStatus::Unrecognized`] and warns **once**
    /// per process — a per-frame warning would be its own denial of service on a bridge that is
    /// meant to run unattended for weeks.
    pub fn from_wire(s: &str) -> Self {
        match s {
            "idle" => AgentStatus::Idle,
            "working" => AgentStatus::Working,
            "blocked" => AgentStatus::Blocked,
            "done" => AgentStatus::Done,
            "unknown" => AgentStatus::Unknown,
            other => {
                let owned = other.to_owned();
                UNRECOGNIZED_STATUS_WARNED.call_once(|| {
                    tracing::warn!(
                        status = %owned,
                        "herdr reported an agent status this client does not model; carrying it \
                         through verbatim and never pushing on it"
                    );
                });
                AgentStatus::Unrecognized(owned)
            }
        }
    }

    /// True for [`AgentStatus::Unknown`] and [`AgentStatus::Unrecognized`] — the two "we do not
    /// actually know" answers. Neither may ever trigger a push.
    pub fn is_indeterminate(&self) -> bool {
        matches!(self, AgentStatus::Unknown | AgentStatus::Unrecognized(_))
    }
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for AgentStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(AgentStatus::from_wire(&String::deserialize(d)?))
    }
}

/// The WRITE-side status vocabulary (`pane.report_agent`). Verified distinct from [`AgentStatus`]:
/// there is **no `done`** — `done` is herdr's own derivation from a `seen` bit no client can set.
/// Modelled so the asymmetry is visible in the types; `pane.report_agent` is out of scope for
/// slice 1 and this crate exposes no method that sends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneAgentState {
    Idle,
    Working,
    Blocked,
    Unknown,
}

impl PaneAgentState {
    /// The wire string.
    pub fn as_str(&self) -> &'static str {
        match self {
            PaneAgentState::Idle => "idle",
            PaneAgentState::Working => "working",
            PaneAgentState::Blocked => "blocked",
            PaneAgentState::Unknown => "unknown",
        }
    }
}

// ── the snapshot ────────────────────────────────────────────────────────────────────────────────

/// `session.snapshot`'s payload. `required = [version, protocol, workspaces, tabs, panes, layouts,
/// agents]` — **including `layouts`**, which is why it is carried (opaquely) rather than dropped:
/// a client that omitted it would emit a snapshot herdr's own schema calls invalid.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// herdr's version string, e.g. `"0.8.2"`.
    pub version: String,
    /// A JSON **number** on the wire, not a string.
    pub protocol: u32,
    pub workspaces: Vec<WorkspaceInfo>,
    /// Render in ARRAY order; never sort by `number`.
    pub tabs: Vec<TabInfo>,
    pub panes: Vec<PaneInfo>,
    /// Carried lossless and **opaque**. Deliberately not modelled: the client is explicitly NOT
    /// proven to parse pane geometry, and the proof's normalizer drops this key. If a later slice
    /// renders the layout, that becomes a real hole and needs its own types plus its own proof.
    pub layouts: Vec<serde_json::Value>,
    pub agents: Vec<AgentInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_tab_id: Option<TabId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_pane_id: Option<PaneId>,
}

impl SessionSnapshot {
    /// The pane with this id, if it is still in the herd.
    pub fn pane(&self, id: &PaneId) -> Option<&PaneInfo> {
        self.panes.iter().find(|p| &p.pane_id == id)
    }

    /// The agent occupying this pane, if any.
    pub fn agent(&self, id: &PaneId) -> Option<&AgentInfo> {
        self.agents.iter().find(|a| &a.pane_id == id)
    }

    /// Panes with a detected agent whose status is neither [`AgentStatus::Unknown`] nor
    /// [`AgentStatus::Unrecognized`] — the set the live proof's event gate picks from, and the set
    /// slice 3 fans its per-pane subscriptions out over (there is no global agent-status
    /// subscription, so the fan-out is the only way to see the product's one trigger).
    pub fn agent_panes(&self) -> impl Iterator<Item = &AgentInfo> {
        self.agents
            .iter()
            .filter(|a| !a.agent_status.is_indeterminate())
    }

    /// The workspace with this id.
    pub fn workspace(&self, id: &WorkspaceId) -> Option<&WorkspaceInfo> {
        self.workspaces.iter().find(|w| &w.workspace_id == id)
    }
}

/// A pane. **19 schema properties, 7 required** (verified against the checked-in schema).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane_id: PaneId,
    pub terminal_id: String,
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
    pub focused: bool,
    pub agent_status: AgentStatus,
    /// **NOT an output-change detector and NOT a state-change counter.** It indexes the retained
    /// `pane_updated` backlog — verified climbing 6 → 18 during a replay while the pane sat static.
    /// Do not build change detection on it. Contrast [`PaneRead::revision`], which is a hard-zero
    /// stub.
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Live on 6/6 panes (`"OC | Omarchy tooling shipping"`). Agent-authored only WHILE an agent
    /// owns the pane — a shell prompt otherwise. Volatile (opencode retitles every 20–40 s), so the
    /// live proof drops it and `tests/golden.rs` proves its decoding against the fixture instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_title_stripped: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground_cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<AgentSessionInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll: Option<PaneScrollInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_labels: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<BTreeMap<String, String>>,
}

/// An agent. **22 schema properties, the same 7 required as [`PaneInfo`]**, plus 5 agent-only
/// fields.
///
/// NOTE the two fields [`PaneInfo`] has and this does NOT: `label` and `scroll`. The schema is the
/// authority here (`AgentInfo.properties` has 22 keys and neither of those is among them); adding
/// them would emit keys herdr never sends.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentInfo {
    pub terminal_id: String,
    pub agent_status: AgentStatus,
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub focused: bool,
    /// Same caveat as [`PaneInfo::revision`]: a backlog index, not a change detector.
    pub revision: u64,
    /// Monotonic per-pane state-change counter — the dedupe key slices 3/4 want.
    ///
    /// **`Option<u64>`, never a bare `u64`.** Verified `"default": 0` and **not** in
    /// `AgentInfo.required`, so it can legitimately be absent; a bare `u64` with `#[serde(default)]`
    /// would silently collapse the key to 0 for every pane and make the dedupe fire on nothing.
    /// It is also NOT carried on the status EVENT, so keying a dedupe on it costs an extra
    /// `agent.get` with a race. Slice 1 surfaces it and takes no position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_change_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_pending: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_detection_skipped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_title_stripped: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground_cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<AgentSessionInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_labels: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<BTreeMap<String, String>>,
}

/// A workspace. **10 schema properties, 8 required.**
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub workspace_id: WorkspaceId,
    pub number: u32,
    pub label: String,
    pub focused: bool,
    pub pane_count: u32,
    pub tab_count: u32,
    pub active_tab_id: TabId,
    pub agent_status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorkspaceWorktreeInfo>,
}

/// The git worktree a workspace was opened from. Absent on all 6 live workspaces; modelled because
/// the schema declares it and an unmodelled field is a silent field-loss in the round trip.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceWorktreeInfo {
    pub repo_key: String,
    pub repo_name: String,
    pub repo_root: PathBuf,
    pub checkout_path: PathBuf,
    pub is_linked_worktree: bool,
}

/// A tab. **7 schema properties, all 7 required.**
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabInfo {
    pub tab_id: TabId,
    pub workspace_id: WorkspaceId,
    pub number: u32,
    pub label: String,
    pub focused: bool,
    pub pane_count: u32,
    pub agent_status: AgentStatus,
}

/// How herdr resolved the agent session occupying a pane. **4 properties, all 4 required.**
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSessionInfo {
    /// e.g. `"herdr:opencode"`.
    pub source: String,
    /// e.g. `"opencode"`.
    pub agent: String,
    /// `"id"` or `"path"` (schema `AgentSessionRefKind`). Kept as a `String`: it is a detail of the
    /// detection manifest, which is remotely versioned, and nothing in this product branches on it.
    pub kind: String,
    pub value: String,
}

/// A pane's scrollback position. **3 properties, all 3 required.**
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaneScrollInfo {
    pub offset_from_bottom: u64,
    pub max_offset_from_bottom: u64,
    /// The viewport height. A full `visible` read returns `viewport_rows − 1` newlines (verified 62
    /// on a 63-row viewport, 5/5 panes).
    pub viewport_rows: u64,
}

// ── pane.read ───────────────────────────────────────────────────────────────────────────────────

/// `pane.read`'s result. **`PaneReadResult.required` is 8 fields** — the published doc lists 3.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaneRead {
    pub pane_id: PaneId,
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
    /// **`pub(crate)`, not `pub`** — read it through [`PaneRead::source_name`].
    ///
    /// The spec writes this field `pub`, which does not compile against a `pub(crate)`
    /// [`ReadSource`]: rustc's `private_interfaces` lint (deny, under the `-D warnings` gate)
    /// rejects a `pub` field whose type is less visible. Of the two ways out — widening
    /// `ReadSource` to `pub`, or narrowing the field — narrowing is the one that keeps the
    /// load-bearing property intact: `Recent` must stay unnameable outside this crate. It still
    /// serializes normally, so the `--json` surface is unchanged.
    pub(crate) source: ReadSource,
    pub format: ReadFormat,
    pub text: String,
    /// **ALWAYS 0 — a hard-coded stub**, while [`PaneInfo::revision`] is live. Verified: pane
    /// revisions 5/18/5/9/26/8 in `pane.list` while `pane.read` returned 0 on every call. Named
    /// here so a later reader cannot get the asymmetry backwards.
    pub revision: u64,
    /// True when the RETURNED text is shorter than what the source held. Asking for MORE lines than
    /// exist is satisfied silently with `truncated: false` (verified: `lines=200` on a 63-row
    /// viewport returned the full text, `truncated: false`). So this means "I clipped", **not**
    /// "I clamped to the viewport".
    pub truncated: bool,
}

impl PaneRead {
    /// The number of newlines in `text`, matching herdr's own `lines` parameter semantics.
    ///
    /// This is deliberately a **newline count**, not a "number of lines" in the usual sense: a full
    /// `visible` read of a 63-row viewport returns 62 (verified, 5/5 panes), and sizing a Telegram
    /// excerpt off the wrong one of those two numbers is an off-by-one nobody catches by eye.
    pub fn line_count(&self) -> usize {
        self.text.bytes().filter(|b| *b == b'\n').count()
    }

    /// The last `max_lines` newline-delimited lines of `text`, borrowed — the tail is what an ask
    /// lives in, and copying a 6 KB read per poll is pure waste.
    ///
    /// `max_lines == 0` yields `""`. A shorter text is returned whole.
    pub fn trimmed_tail(&self, max_lines: usize) -> &str {
        if max_lines == 0 {
            return "";
        }
        // Ignore a single trailing newline so a full read of N lines is not counted as N+1.
        let body = self.text.strip_suffix('\n').unwrap_or(&self.text);
        let mut seen = 0usize;
        for (idx, b) in body.bytes().enumerate().rev() {
            if b == b'\n' {
                seen += 1;
                if seen == max_lines {
                    return &self.text[idx + 1..];
                }
            }
        }
        &self.text
    }

    /// The wire name of the source this text came from (`"visible"` for everything this crate can
    /// produce). Present so callers can report the source without `ReadSource` itself becoming
    /// public API.
    pub fn source_name(&self) -> &'static str {
        self.source.as_str()
    }
}

/// Where `pane.read` read from.
///
/// **`pub(crate)`, and there is deliberately NO `Default` impl anywhere in this crate.** The
/// `herdr pane read` CLI defaults to `recent` — the harvest-scrolling source, which physically
/// moves the operator's viewport when `lines > viewport_rows`. Any Rust `Default` that could yield
/// it would move the operator's screen on every background poll. The client exposes only
/// `read_visible` / `read_visible_tail`, so `Recent` / `RecentUnwrapped` / `Detection` are
/// unreachable from outside this crate by construction, not by convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadSource {
    Visible,
    Recent,
    RecentUnwrapped,
    Detection,
}

impl ReadSource {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ReadSource::Visible => "visible",
            ReadSource::Recent => "recent",
            ReadSource::RecentUnwrapped => "recent_unwrapped",
            ReadSource::Detection => "detection",
        }
    }
}

/// The text encoding `pane.read` returned. Public: choosing `ansi` is harmless (it does not scroll
/// anything), and slice 3 may want it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadFormat {
    Text,
    Ansi,
}

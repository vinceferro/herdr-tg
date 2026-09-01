//! The socket the bridges connect to, and everything that decides what they may do.
//!
//! # Authority flows one way
//!
//! A bridge speaks; the hub decides. No frame a bridge sends carries addressing — no chat, no
//! topic, no project name — because the hub already knows which connection is which project. It
//! learned that by resolving a **secret**, not by reading a field, which is why a bridge cannot
//! claim another project's topic no matter what it puts on the wire.
//!
//! # Four gates, all fail-closed, in this order
//!
//! 1. **Who is on the other end.** `SO_PEERCRED` off the connection. A different uid is closed
//!    without a reply — an answer, even a refusal, is information. Mode 0600 on the socket already
//!    implies this; reading the credential back means the check survives a permissions mistake, and
//!    it yields the pid the single-claim rule needs.
//! 2. **Which project.** The secret resolves to one, in constant time, over the whole registry.
//!    The `project_id` on the wire is not consulted.
//! 3. **Switched on.** Enrolled and disabled is a real state.
//! 4. **Exactly one live connection per project.** A second is refused, never a takeover. A
//!    takeover is what bridge-murder felt like from the inside: the incumbent kept running and
//!    quietly stopped being heard. If the incumbent's pid is gone from `/proc` it is evicted
//!    instead — a crashed worker must not lock its own project out until someone finds a keyboard.
//!
//! # Live is not the same as connected
//!
//! A channel plugin that is not allowlisted boots and exits in about a tenth of a second. From the
//! process table that is indistinguishable from a healthy worker, and the hub would happily create
//! a topic and greet a bridge that had already gone. So a project becomes **live** only after
//! `hello`, a settling window, and one answered `ping` — and the topic is created at that moment,
//! not before. An invisible topic full of nothing is the same as no topic to the person looking
//! for it.
//!
//! # Nothing is sent without a record of it first
//!
//! [`HubAudit`] writes `sent` before a send and the outcome after, so a dangling `sent` means the
//! process died mid-write and nothing else. It deliberately mirrors `audit.rs` rather than
//! extending it: that file's subject is a pane and a keystroke, and the hub has neither.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hub_proto::{
    AskId, AskOption, BridgeFrame, Delivered, Envelope, FrameId, HubFrame, Limits, MsgId, OptionId,
    ProjectId, RefusedReason, VERSION,
};
use serde::{Deserialize, Serialize};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, mpsc};

use crate::registry::Registry;

/// How long after `hello` the hub waits for a `pong` before calling a project live.
pub const DEFAULT_SETTLE: Duration = Duration::from_secs(5);

/// What the hub will accept from one connection.
pub const LIMITS: Limits = Limits {
    max_frame: hub_proto::MAX_FRAME_BYTES,
    max_text: 3500,
    frames_per_min: 60,
};

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The surface the hub speaks to the operator through.

/// What became of one attempt to put a message in front of the operator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendOutcome {
    /// It landed, and this is its message id.
    Sent(MsgId),
    /// It landed, but it was shortened first. The bridge is told, because the bridge is the only
    /// party that can decide to say less next time.
    Clamped(MsgId),
    /// The chat's budget refused it, and this is how long to wait.
    ///
    /// Sending anyway is how a bot earns a 429, and a 429 on a shared bot punishes every project
    /// rather than the one that caused it.
    TooFast(Duration),
    /// The topic is gone. Telegram never says so with a service message and offers no way to list
    /// topics, so this is the only way the hub finds out. Handled as a rebinding, exactly once —
    /// never retried as if it were a transient, which would swallow the project's messages.
    TopicGone,
    /// Telegram refused it, and said why.
    Refused(String),
    /// It went out and could not be checked. Never retried when the message carried buttons.
    Unseen,
}

/// Telegram, behind a trait, so the hub's decisions can be tested without one.
pub trait Surface: Send + Sync + 'static {
    /// Create the project's topic and return its id.
    fn create_topic(
        &self,
        title: &str,
        icon_color: u8,
    ) -> impl std::future::Future<Output = anyhow::Result<i32>> + Send;

    /// Put a message in a topic, with buttons if there are any.
    fn send(
        &self,
        topic_id: i32,
        text: &str,
        buttons: &[AskOption],
    ) -> impl std::future::Future<Output = SendOutcome> + Send;

    /// Strip a stale keyboard and append a note saying what happened to the question.
    ///
    /// No design that read a rendered screen could ever do this: a screen cannot tell you that a
    /// question stopped being asked.
    fn retire_buttons(
        &self,
        topic_id: i32,
        msg_id: &MsgId,
        note: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The tap ledger.

/// What was written down beside a message that carried buttons.
///
/// A tap is resolved against this, never against a button's position. Position is how a button
/// reading "Reject" once confirmed "Allow always".
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AskRecord {
    pub project: ProjectId,
    pub ask_id: AskId,
    pub topic_id: i32,
    /// The options exactly as the bridge minted them, labels included, so the hub can say what was
    /// chosen in the words the operator actually read.
    pub options: Vec<AskOption>,
    /// Which run of the worker asked. A tap on a menu drawn for a session that has since restarted
    /// is refused with a reason, rather than answered into a process that never asked.
    pub instance: String,
}

/// Why a tap did not become an answer. Every one of these is said out loud in the topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapRefusal {
    /// Nothing was written down beside that message.
    NoRecord,
    /// The button is not one of the ones written down.
    NotAnOption,
    /// The project that asked is not connected now.
    NotConnected,
    /// It asked, then restarted. The question belongs to a process that no longer exists.
    Restarted,
    /// The tap came from a chat this bot does not answer.
    NotYours,
}

impl TapRefusal {
    /// What the operator reads. No ids, no enum names, no "None".
    pub fn say(&self) -> &'static str {
        match self {
            Self::NoRecord => "I have no record of that question, so I will not answer it for you.",
            Self::NotAnOption => {
                "That button is not one of the answers I wrote down for this question."
            }
            Self::NotConnected => {
                "That project is not connected right now, so there is nobody to tell."
            }
            Self::Restarted => {
                "That question belonged to a session that has since restarted. Ask again and it will come back."
            }
            Self::NotYours => "",
        }
    }
}

/// Every open question, keyed by the message the operator can see.
///
/// Persisted, because a restart must not turn every live keyboard into a button that does nothing.
#[derive(Debug, Default)]
pub struct AskLedger {
    path: PathBuf,
    records: BTreeMap<String, AskRecord>,
}

/// `chat:message` — flat, so it survives a JSON round trip without a tuple-key encoding.
///
/// **Keyed on the chat as well as the message.** Every supergroup numbers its reply threads with
/// plain message ids drawn from one counter, which is how a swipe-to-reply on a direct message
/// once reached a forum pane. The chat is half the key, not decoration.
fn ledger_key(chat_id: i64, msg_id: &MsgId) -> String {
    format!("{chat_id}:{}", msg_id.as_str())
}

impl AskLedger {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let records = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self { path, records }
    }

    pub fn default_path() -> PathBuf {
        crate::lock::state_dir().join("asks.json")
    }

    /// Write down what a message means, before anyone can tap it.
    pub fn record(
        &mut self,
        chat_id: i64,
        msg_id: &MsgId,
        record: AskRecord,
    ) -> std::io::Result<()> {
        self.records.insert(ledger_key(chat_id, msg_id), record);
        self.save()
    }

    pub fn get(&self, chat_id: i64, msg_id: &MsgId) -> Option<&AskRecord> {
        self.records.get(&ledger_key(chat_id, msg_id))
    }

    /// Forget a question that has been answered or withdrawn.
    pub fn forget(&mut self, chat_id: i64, msg_id: &MsgId) -> std::io::Result<()> {
        self.records.remove(&ledger_key(chat_id, msg_id));
        self.save()
    }

    /// Every message still carrying a live keyboard for one ask, so it can be retired.
    pub fn messages_for(&self, project: &ProjectId, ask_id: &AskId) -> Vec<(i64, MsgId)> {
        self.records
            .iter()
            .filter(|(_, r)| &r.project == project && &r.ask_id == ask_id)
            .filter_map(|(k, _)| {
                let (chat, msg) = k.split_once(':')?;
                Some((chat.parse().ok()?, MsgId::new(msg)))
            })
            .collect()
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(&self.records)?;
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(&body)?;
            f.flush()?;
        }
        fs::rename(&tmp, &self.path)
    }
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The audit.

/// Sent-before, outcome-after, nothing dangles.
#[derive(Debug, Clone)]
pub struct HubAudit {
    path: PathBuf,
}

impl HubAudit {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        crate::lock::state_dir().join("hub.audit.log")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Recorded BEFORE the send. A `sent` with no outcome after it means the process died in the
    /// middle of writing, and means nothing else — which is the only reason the pair is useful.
    pub fn sent(&self, project: &ProjectId, topic_id: i32, bytes: usize) -> std::io::Result<()> {
        self.line(&format!(
            "sent\tproject={project}\ttopic={topic_id}\tbytes={bytes}"
        ))
    }

    pub fn outcome(&self, project: &ProjectId, outcome: &SendOutcome) -> std::io::Result<()> {
        let word = match outcome {
            SendOutcome::Sent(id) => format!("delivered\tmessage={id}"),
            SendOutcome::Clamped(id) => format!("delivered\tmessage={id}\tclipped=yes"),
            SendOutcome::TooFast(d) => format!("shed\tretry_after_ms={}", d.as_millis()),
            SendOutcome::TopicGone => "topic-gone".to_owned(),
            SendOutcome::Refused(why) => format!("refused\twhy={why}"),
            SendOutcome::Unseen => "unseen".to_owned(),
        };
        self.line(&format!("{word}\tproject={project}"))
    }

    /// A branch that sends nothing still writes a line, so silence in this file always means the
    /// process stopped rather than that the hub decided something quietly.
    pub fn refused(&self, project: &ProjectId, why: &str) -> std::io::Result<()> {
        self.line(&format!("refused\tproject={project}\twhy={why}"))
    }

    fn line(&self, body: &str) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut f = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&self.path)?;
        // Re-asserted: `.mode()` applies only at creation, so a file that was once world-readable
        // would stay that way for the life of the machine.
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600));
        writeln!(f, "{}\t{body}", now_iso())
    }
}

fn now_iso() -> String {
    // Seconds since the epoch. Not pretty, and deliberately dependency-free: a log line's job here
    // is ordering and correlation, and a date crate is a supply-chain decision for a timestamp.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("t={secs}")
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Claims.

/// A live connection, and how to reach it.
#[derive(Debug)]
struct Claim {
    pid: u32,
    instance: String,
    tx: mpsc::Sender<Envelope<HubFrame>>,
}

/// Is a pid still a process on this machine?
///
/// The evict-a-corpse rule depends on this being a fact rather than a hope. `/proc/<pid>` is the
/// fact; a signal-0 probe would answer "yes" for a pid this user does not own.
fn pid_is_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The socket.

/// `/run/user/<uid>/kickoff/hub.sock`.
///
/// Derived on both sides, never configured. `XDG_RUNTIME_DIR` is not on kickoff's list of variables
/// that survive its `env -i` boundary, so a bridge started by a worker would not see it — the two
/// would derive different paths and neither would be wrong.
pub fn socket_path() -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    PathBuf::from(format!("/run/user/{uid}/kickoff")).join("hub.sock")
}

/// Bind the listener, with the directory and the socket locked down before anything can connect.
///
/// A stale socket file from a previous run is removed first. That is safe because the hub lock is
/// already held by this process — nothing else can be listening — and skipping it would make a
/// crash require a manual `rm` from a keyboard.
pub fn bind(path: &Path) -> anyhow::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Why a connection was turned away before it became a project.
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Admitted(ProjectId),
    /// Closed without a reply. A refusal is information, and this one is not owed.
    ClosedSilently,
    Refused(RefusedReason),
}

/// The hub: one socket, one registry, one claim per project.
pub struct Hub<S: Surface> {
    pub surface: Arc<S>,
    pub registry: Arc<Mutex<Registry>>,
    pub ledger: Arc<Mutex<AskLedger>>,
    pub audit: Arc<HubAudit>,
    claims: Arc<Mutex<BTreeMap<ProjectId, Claim>>>,
    /// One outbound budget per chat, because Telegram's ceiling is per chat and forum topics do
    /// not get one of their own. Six busy projects share it.
    budgets: Arc<Mutex<crate::queue::Budgets>>,
    /// Which chats this bot answers. Checked first, before any state is touched.
    allowed_chats: Arc<Vec<i64>>,
    /// The one forum every topic lives in. Routing is a single rule — topic, inside this chat —
    /// and every other rule this bridge used to have is deleted rather than tested against.
    forum_chat: i64,
    settle: Duration,
}

impl<S: Surface> Hub<S> {
    pub fn new(
        surface: Arc<S>,
        registry: Registry,
        ledger: AskLedger,
        audit: HubAudit,
        allowed_chats: Vec<i64>,
        forum_chat: i64,
    ) -> Self {
        Self {
            surface,
            registry: Arc::new(Mutex::new(registry)),
            ledger: Arc::new(Mutex::new(ledger)),
            audit: Arc::new(audit),
            claims: Arc::new(Mutex::new(BTreeMap::new())),
            budgets: Arc::new(Mutex::new(crate::queue::Budgets::default())),
            allowed_chats: Arc::new(allowed_chats),
            forum_chat,
            settle: DEFAULT_SETTLE,
        }
    }

    /// Shorten the live-window for tests. Test-only on purpose: five seconds is the number that
    /// separates a real bridge from a channel plugin that booted and exited, and a knob for it in
    /// production is a knob someone eventually turns down to zero to make a flake go away.
    #[cfg(test)]
    pub fn with_settle(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }

    /// Decide whether a `hello` may become a project.
    ///
    /// Split out from the connection so the decision can be tested without a socket, and so the
    /// order of the gates is visible in one place.
    pub async fn admit(
        &self,
        peer_uid: u32,
        our_uid: u32,
        hello: &BridgeFrame,
        version: u16,
    ) -> Admission {
        if peer_uid != our_uid {
            tracing::warn!(
                peer_uid,
                "a connection from another user was closed without a reply"
            );
            return Admission::ClosedSilently;
        }
        if version != VERSION {
            return Admission::Refused(RefusedReason::VersionSkew);
        }
        let BridgeFrame::Hello { token, pid, .. } = hello else {
            // The first frame must be `hello`. Anything else is a bridge that does not speak this
            // protocol, and letting it continue would mean guessing which project it is.
            return Admission::Refused(RefusedReason::UnknownProject);
        };

        let (id, enabled) = {
            let registry = self.registry.lock().await;
            match registry.resolve(token) {
                None => return Admission::Refused(RefusedReason::UnknownProject),
                Some(p) => (p.id.clone(), p.enabled),
            }
        };
        if !enabled {
            return Admission::Refused(RefusedReason::NotEnabled);
        }

        let claims = self.claims.lock().await;
        if let Some(incumbent) = claims.get(&id)
            && pid_is_alive(incumbent.pid)
        {
            tracing::warn!(
                project = %id, incumbent = incumbent.pid, arriving = pid,
                "a second bridge tried to take a project that is already connected"
            );
            return Admission::Refused(RefusedReason::AlreadyClaimed);
        }
        Admission::Admitted(id)
    }

    /// Is this chat one the bot answers?
    ///
    /// Runs first, before command parsing and before any state is touched. Empty answers nobody:
    /// the opposite convention would turn a misconfiguration into an open bot.
    pub fn chat_is_allowed(&self, chat_id: i64) -> bool {
        self.allowed_chats.contains(&chat_id)
    }

    /// Turn a tap into an answer, or into a sentence saying why not.
    pub async fn resolve_tap(
        &self,
        chat_id: i64,
        msg_id: &MsgId,
        option_id: &OptionId,
    ) -> Result<(ProjectId, AskId, OptionId), TapRefusal> {
        // The allowlist first. A tap from a chat this bot does not answer must not even reach the
        // ledger — and it gets silence, not a refusal, because a refusal is a reply.
        if !self.chat_is_allowed(chat_id) {
            return Err(TapRefusal::NotYours);
        }

        let record = {
            let ledger = self.ledger.lock().await;
            ledger.get(chat_id, msg_id).cloned()
        };
        let Some(record) = record else {
            return Err(TapRefusal::NoRecord);
        };
        if !record.options.iter().any(|o| &o.option_id == option_id) {
            return Err(TapRefusal::NotAnOption);
        }

        let claims = self.claims.lock().await;
        let Some(claim) = claims.get(&record.project) else {
            return Err(TapRefusal::NotConnected);
        };
        if claim.instance != record.instance {
            return Err(TapRefusal::Restarted);
        }
        Ok((record.project, record.ask_id, option_id.clone()))
    }

    /// Hand a frame to a project's live connection.
    pub async fn deliver(&self, project: &ProjectId, frame: HubFrame) -> bool {
        let claims = self.claims.lock().await;
        let Some(claim) = claims.get(project) else {
            return false;
        };
        let env = Envelope::new(FrameId::new(format!("h{}", next_frame_seq())), frame);
        claim.tx.send(env).await.is_ok()
    }

    /// Register a live connection, evicting a corpse if one is holding the project.
    pub async fn claim(
        &self,
        project: ProjectId,
        pid: u32,
        instance: String,
        tx: mpsc::Sender<Envelope<HubFrame>>,
    ) {
        let mut claims = self.claims.lock().await;
        if let Some(old) = claims.get(&project)
            && !pid_is_alive(old.pid)
        {
            tracing::info!(project = %project, dead = old.pid, "evicting a bridge that is no longer running");
        }
        claims.insert(project, Claim { pid, instance, tx });
    }

    /// Drop a connection's claim, but only if it is still the one holding it.
    ///
    /// The guard matters: a bridge that was evicted and then finished shutting down would otherwise
    /// remove its successor's claim on the way out, leaving a live worker unreachable.
    pub async fn release(&self, project: &ProjectId, pid: u32) {
        let mut claims = self.claims.lock().await;
        if claims.get(project).is_some_and(|c| c.pid == pid) {
            claims.remove(project);
        }
    }

    /// Test-only. Production code asks by trying to deliver, which is the question it actually has;
    /// a separate "is it connected" read would be a fact that could be stale by the time it is used.
    #[cfg(test)]
    pub async fn is_claimed(&self, project: &ProjectId) -> bool {
        self.claims.lock().await.contains_key(project)
    }

    /// The topic a project's messages go in, created and greeted on first use.
    ///
    /// Created here rather than at `hello` for one reason: a topic with no messages is invisible in
    /// Telegram's topic list, so a topic created for a bridge that then vanished is the same as no
    /// topic at all to the person looking for it — except that it is now bound.
    pub async fn topic_for(&self, project: &ProjectId) -> anyhow::Result<i32> {
        let (existing, title, colour) = {
            let registry = self.registry.lock().await;
            let p = registry
                .get(project)
                .ok_or_else(|| anyhow::anyhow!("that project is not enrolled"))?;
            (p.topic_id, p.title.clone(), p.icon_color)
        };
        if let Some(id) = existing {
            return Ok(id);
        }
        let id = self.surface.create_topic(&title, colour).await?;
        self.registry.lock().await.bind_topic(project, id)?;
        // Greeted immediately, in the same breath as being created. The greeting is what makes the
        // topic appear in the list at all.
        let _ = self
            .surface
            .send(id, &format!("{title} is connected."), &[])
            .await;
        Ok(id)
    }

    /// Send into a project's topic, with the audit around it and one rebinding if the topic is gone.
    pub async fn say(&self, project: &ProjectId, text: &str, buttons: &[AskOption]) -> SendOutcome {
        let topic_id = match self.topic_for(project).await {
            Ok(id) => id,
            Err(e) => {
                let _ = self.audit.refused(project, &e.to_string());
                return SendOutcome::Refused(e.to_string());
            }
        };
        // The budget is spent BEFORE the audit line, so a send the budget refused never produces a
        // `sent` record. A dangling `sent` has to keep meaning exactly one thing: the process died
        // mid-write.
        if let Err(crate::queue::RetryAfter(wait)) = self
            .budgets
            .lock()
            .await
            .take(self.forum_chat, std::time::Instant::now())
        {
            let outcome = SendOutcome::TooFast(wait);
            let _ = self.audit.outcome(project, &outcome);
            return outcome;
        }

        // Clipped here rather than by the surface, because whether anything was lost is a fact the
        // BRIDGE has to be told, and only this side is holding the ack.
        let (text, clamped) = crate::queue::fit(text, crate::queue::MAX_TEXT);
        let text = text.as_str();

        let _ = self.audit.sent(project, topic_id, text.len());
        let mut outcome = self.surface.send(topic_id, text, buttons).await;

        if outcome == SendOutcome::TopicGone {
            // Exactly once, and never as a retry: Telegram gives no service message when a topic is
            // deleted and no way to list them, so this is first-class rebinding. Treating it as a
            // transient would make the project's messages disappear quietly and forever.
            tracing::warn!(project = %project, "the topic is gone; making a new one");
            let _ = self.registry.lock().await.unbind_topic(project);
            if let Ok(fresh) = self.topic_for(project).await {
                let _ = self.audit.sent(project, fresh, text.len());
                outcome = self.surface.send(fresh, text, buttons).await;
            }
        }
        if clamped && let SendOutcome::Sent(id) = outcome {
            outcome = SendOutcome::Clamped(id);
        }
        let _ = self.audit.outcome(project, &outcome);
        outcome
    }

    /// Handle one bridge, from `hello` to the connection closing.
    ///
    /// The order here is the design, so it is worth reading as one sequence: identify the peer,
    /// admit or refuse, admit BEFORE creating anything, prove the far end is really there, and only
    /// then create the topic that makes the project visible.
    pub async fn serve_connection(self: Arc<Self>, stream: UnixStream) -> anyhow::Result<()> {
        let peer = peer_uid(&stream)?;
        let (rx_half, mut tx_half) = stream.into_split();
        let mut reader = hub_proto::FrameReader::new(rx_half);

        let Some(first) = reader.next::<BridgeFrame>().await? else {
            // Connected and said nothing. Not an error and not worth a line per occurrence.
            return Ok(());
        };

        let project = match self.admit(peer, our_uid(), &first.payload, first.v).await {
            // No reply at all. A refusal would confirm that something is listening here.
            Admission::ClosedSilently => return Ok(()),
            Admission::Refused(reason) => {
                let env = Envelope::new(FrameId::new("h-refused"), HubFrame::Refused { reason });
                let _ = hub_proto::write_frame(&mut tx_half, &env).await;
                return Ok(());
            }
            Admission::Admitted(id) => id,
        };

        let BridgeFrame::Hello { instance, pid, .. } = first.payload.clone() else {
            unreachable!("admit only admits a hello");
        };

        let title = {
            let registry = self.registry.lock().await;
            registry
                .get(&project)
                .map(|p| p.title.clone())
                .unwrap_or_default()
        };

        // The writer is a task of its own so that a slow Telegram send can never block reading the
        // socket. A bridge that cannot be read is a bridge whose `bye` is missed.
        let (tx, mut outbox) = mpsc::channel::<Envelope<HubFrame>>(64);
        let writer = tokio::spawn(async move {
            while let Some(frame) = outbox.recv().await {
                if hub_proto::write_frame(&mut tx_half, &frame).await.is_err() {
                    break;
                }
            }
        });

        self.claim(project.clone(), pid, instance.clone(), tx.clone())
            .await;

        // Admitted, with no topic yet. See `HubFrame::Welcome` for why that is not an omission.
        let _ = tx
            .send(Envelope::new(
                FrameId::new(format!("h{}", next_frame_seq())),
                HubFrame::Welcome {
                    project: title,
                    topic_id: None,
                    limits: LIMITS,
                },
            ))
            .await;

        // Prove the far end is really there before anything is created for it. The ping's own
        // envelope id is the nonce; a pong naming it is the proof.
        let ping_id = FrameId::new(format!("h{}", next_frame_seq()));
        let _ = tx
            .send(Envelope::new(ping_id.clone(), HubFrame::Ping))
            .await;

        let live = tokio::time::timeout(self.settle, async {
            while let Ok(Some(frame)) = reader.next::<BridgeFrame>().await {
                if let BridgeFrame::Pong { r#ref } = &frame.payload
                    && r#ref == &ping_id
                {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or(false);

        if !live {
            tracing::warn!(
                project = %project,
                "a bridge connected and did not answer; it is probably not allowed to talk to me"
            );
            let _ = self.audit.refused(&project, "connected but never answered");
            self.release(&project, pid).await;
            writer.abort();
            return Ok(());
        }

        // Live. NOW the topic exists, and the greeting is what makes it visible in the list.
        if let Err(e) = self.topic_for(&project).await {
            tracing::error!(project = %project, error = %e, "could not make a topic for a live project");
        }

        // The claim is released on EVERY way out of this loop, not only the tidy one.
        //
        // It used to be released after a `?`, which meant a read error skipped it entirely — and a
        // read error is the ORDINARY way a bridge goes away. A peer that closes with bytes still
        // unread in its receive buffer makes the kernel send an RST, and the hub's next read fails
        // with a connection reset rather than a clean end-of-file. A bridge that has not been
        // reading its acks does exactly that. The cost of getting this wrong is a project holding a
        // claim nobody is behind: its worker restarts, is refused as already-claimed, and the
        // operator is left with a project that has gone quiet for no visible reason.
        loop {
            match reader.next::<BridgeFrame>().await {
                Ok(None) => break,
                Err(e) => {
                    tracing::debug!(project = %project, error = %e, "the bridge's connection ended");
                    break;
                }
                Ok(Some(frame)) => {
                    let ack_ref = frame.id.clone();
                    let (delivered, why) = self.handle(&project, &instance, frame.payload).await;
                    let _ = tx
                        .send(Envelope::new(
                            FrameId::new(format!("h{}", next_frame_seq())),
                            HubFrame::Ack {
                                r#ref: ack_ref,
                                delivered,
                                why,
                            },
                        ))
                        .await;
                }
            }
        }

        self.release(&project, pid).await;
        writer.abort();
        Ok(())
    }

    /// One frame from a bridge. Returns what to put in its ack.
    ///
    /// **Every frame gets exactly one ack.** A rejected send used to be a single error log and a
    /// drop, which already lost 5,164 characters of a real agent's longest message. Backpressure
    /// now reaches the only party that can do anything about it.
    async fn handle(
        &self,
        project: &ProjectId,
        instance: &str,
        frame: BridgeFrame,
    ) -> (Delivered, Option<hub_proto::AckWhy>) {
        match frame {
            BridgeFrame::Say { text, .. } | BridgeFrame::Done { text } => {
                self.say_and_ack(project, &text, &[]).await
            }
            BridgeFrame::Ask {
                ask_id,
                text,
                options,
            } => {
                let options = options.unwrap_or_default();
                let outcome = self.say(project, &text, &options).await;

                // The record is written for the message that actually exists. Recording before the
                // send would leave a ledger entry for a message nobody can see; recording against a
                // guessed id would let a tap resolve against the wrong question, which is the exact
                // shape of the defect where a button reading "Reject" confirmed "Allow always".
                if let SendOutcome::Sent(msg_id) | SendOutcome::Clamped(msg_id) = &outcome {
                    let topic_id = self
                        .registry
                        .lock()
                        .await
                        .get(project)
                        .and_then(|p| p.topic_id)
                        .unwrap_or_default();
                    let record = AskRecord {
                        project: project.clone(),
                        ask_id,
                        topic_id,
                        options,
                        instance: instance.to_owned(),
                    };
                    if let Err(e) = self
                        .ledger
                        .lock()
                        .await
                        .record(self.forum_chat, msg_id, record)
                    {
                        // A keyboard whose meaning was not written down must not stay tappable, so
                        // this is loud. `resolve_tap` will refuse it, which is the fail-closed half.
                        tracing::error!(
                            project = %project, error = %e,
                            "sent a question but could not write down what its buttons mean"
                        );
                    }
                }
                self.ack_for(&outcome)
            }
            BridgeFrame::AskResolved {
                ask_id,
                how,
                outcome,
            } => {
                self.retire(project, &ask_id, how, outcome.as_deref()).await;
                (Delivered::Yes, None)
            }
            // Liveness and bookkeeping. Acked so that "every frame gets exactly one" stays true
            // without exception, which is what makes a missing ack mean something.
            BridgeFrame::Beat { .. } | BridgeFrame::Ack { .. } | BridgeFrame::Pong { .. } => {
                (Delivered::Yes, None)
            }
            BridgeFrame::Bye { .. } => (Delivered::Yes, None),
            BridgeFrame::Hello { .. } => {
                // A second hello on a live connection. Not a takeover and not an error worth
                // closing over; it is simply not a thing this protocol has.
                (Delivered::No, Some(hub_proto::AckWhy::TelegramRefused))
            }
            BridgeFrame::Unknown => (Delivered::Yes, None),
        }
    }

    async fn say_and_ack(
        &self,
        project: &ProjectId,
        text: &str,
        options: &[AskOption],
    ) -> (Delivered, Option<hub_proto::AckWhy>) {
        let outcome = self.say(project, text, options).await;
        self.ack_for(&outcome)
    }

    /// Turn what actually happened into the three-valued answer the bridge branches on.
    ///
    /// `Unseen` is not a hedge. Telegram has no idempotency key, so a send that times out may or
    /// may not have landed and there is no way to ask. Reporting that as `No` would invite a retry,
    /// and a retried question with buttons is two live menus for one question, both tappable
    /// forever — a misfire this system would have manufactured for itself.
    fn ack_for(&self, outcome: &SendOutcome) -> (Delivered, Option<hub_proto::AckWhy>) {
        match outcome {
            SendOutcome::Sent(_) => (Delivered::Yes, None),
            SendOutcome::Clamped(_) => (Delivered::Yes, Some(hub_proto::AckWhy::Clamped)),
            SendOutcome::TooFast(_) => (Delivered::No, Some(hub_proto::AckWhy::TooFast)),
            SendOutcome::TopicGone => (Delivered::No, Some(hub_proto::AckWhy::NoTopic)),
            SendOutcome::Refused(_) => (Delivered::No, Some(hub_proto::AckWhy::TelegramRefused)),
            SendOutcome::Unseen => (Delivered::Unseen, None),
        }
    }

    /// Strip a stale keyboard, because a screen could never tell you a question stopped being asked.
    async fn retire(
        &self,
        project: &ProjectId,
        ask_id: &AskId,
        how: hub_proto::AskEnd,
        outcome: Option<&str>,
    ) {
        let note = match (how, outcome) {
            (hub_proto::AskEnd::Answered, Some(o)) => format!("answered at the terminal — {o}"),
            (hub_proto::AskEnd::Answered, None) => "answered at the terminal".to_owned(),
            (hub_proto::AskEnd::Withdrawn, _) => "no longer being asked".to_owned(),
            (hub_proto::AskEnd::Timeout, _) => "timed out".to_owned(),
        };
        let targets = { self.ledger.lock().await.messages_for(project, ask_id) };
        let topic = self
            .registry
            .lock()
            .await
            .get(project)
            .and_then(|p| p.topic_id);
        for (chat, msg) in targets {
            if let Some(topic) = topic {
                let _ = self.surface.retire_buttons(topic, &msg, &note).await;
            }
            let _ = self.ledger.lock().await.forget(chat, &msg);
        }
    }
}

/// Per-process frame counter. Opaque and monotonic is all the protocol asks for.
fn next_frame_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Read the uid of whoever is on the other end of a connection.
pub fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    let cred = rustix::net::sockopt::socket_peercred(stream)?;
    Ok(cred.uid.as_raw())
}

/// This process's uid, for comparison against the peer's.
pub fn our_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

#[cfg(test)]
mod tests;

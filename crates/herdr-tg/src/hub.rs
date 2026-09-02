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

use std::collections::{BTreeMap, BTreeSet};
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

/// The longest a send will wait its turn in the pacing queue before shedding instead.
///
/// Not a rate limit — the rate limits live in `queue.rs`. This bounds how long the connection's
/// read loop can be blocked behind its own outgoing message, because a bridge whose socket goes
/// unread for minutes is a bridge whose `bye` is missed and whose claim lingers. Ten seconds is
/// nine more than the one-second rhythm needs and far less than the per-minute ceiling implies, so
/// a burst that fits under the ceiling drains and a genuine flood still sheds.
pub const MAX_PACE_WAIT: Duration = Duration::from_secs(10);

/// Telegram's own ceiling on a button's `callback_data`. Not ours to raise.
///
/// The `h|` prefix comes out of this budget, which is why the check is `len + 2`.
pub const CALLBACK_DATA_MAX: usize = 64;

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

    /// Strip a stale keyboard AND say what happened to the question.
    ///
    /// No design that read a rendered screen could ever do this: a screen cannot tell you that a
    /// question stopped being asked. The original text comes back in because the message has to
    /// keep saying what was asked — a body replaced by a bare note reads as the bot having lost the
    /// question, which is the opposite of the reassurance this is for.
    fn retire_buttons(
        &self,
        topic_id: i32,
        msg_id: &MsgId,
        original: &str,
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
    /// What the question said. Kept so that retiring the keyboard can leave the question visible
    /// with its outcome beside it, rather than replacing it with a bare note.
    #[serde(default)]
    pub text: String,
    /// Which run of the worker asked. A tap on a menu drawn for a session that has since restarted
    /// is refused with a reason, rather than answered into a process that never asked.
    pub instance: String,
    /// What was already answered, if anything.
    ///
    /// The record's presence used to BE the authorisation, which was fine only while the record was
    /// deleted the instant a tap landed. Once a failed retirement started keeping the record — so
    /// the keyboard could be retired later — presence stopped meaning "unanswered", and the still
    /// live keyboard on the operator's phone became re-tappable: a second tap delivered a second,
    /// contradicting `Choice` into an agent that had already been answered. Worse the other way
    /// round, an agent that answered at its own terminal could still be sent a phone tap.
    ///
    /// So authorisation is this field, and the record's presence is only retirement bookkeeping.
    #[serde(default)]
    pub answered: Option<OptionId>,
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
    /// It has already been answered — from the phone or at the terminal — and the keyboard is only
    /// still there because taking it away failed.
    AlreadyAnswered,
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
            Self::AlreadyAnswered => {
                "That one has already been answered. I have not sent anything."
            }
            Self::NotYours => "",
        }
    }
}

/// What happened when a tap that reached nobody was taken back.
///
/// Three, because the operator reads a different sentence for each and two of them are opposites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Withdrawal {
    /// The keyboard came off and the record went with it.
    Retired,
    /// Telegram refused the edit. The menu is still live — and it can be tapped again, because the
    /// answer was taken back off the question rather than the record being thrown away.
    StillOnHisPhone,
    /// There was nothing written down any more. The project's own side finished with the question
    /// while the tap was in flight, which is not far-fetched: the delivery that failed may well
    /// have failed because that same bridge's outbox was full.
    NothingLeftToTakeBack,
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
    /// Read the ledger, or start empty — and say so when starting empty was not the plan.
    ///
    /// This used to swallow both the read error and the parse error with `.ok()`. The cost of that
    /// silence is specific: every keyboard already on the operator's phone becomes a button that
    /// answers "I have no record of that question", with nothing anywhere explaining why. Starting
    /// empty is still the right behaviour — refusing to boot would take the whole fleet's channel
    /// down over one bad byte — but it must not be quiet.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let records = match fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<BTreeMap<String, AskRecord>>(&raw) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(
                        error = %e, path = %path.display(),
                        "the open-questions ledger could not be read. Every keyboard already on \
                         the operator's phone will refuse; those questions need asking again."
                    );
                    BTreeMap::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "the open-questions ledger is unreadable");
                BTreeMap::new()
            }
        };
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

    /// Write down that a question has been answered, so it cannot be answered again.
    ///
    /// The record stays — the keyboard may still need retiring — but it stops authorising anything.
    pub fn mark_answered(
        &mut self,
        chat_id: i64,
        msg_id: &MsgId,
        option_id: &OptionId,
    ) -> std::io::Result<()> {
        if let Some(r) = self.records.get_mut(&ledger_key(chat_id, msg_id)) {
            r.answered = Some(option_id.clone());
        }
        self.save()
    }

    /// Take the answer back off a question, so its keyboard can be tapped again.
    ///
    /// For the one case where an answer was written down and then reached nobody. The record has to
    /// stay — it is the only handle anything has on a keyboard that is still live — so what is
    /// undone is the authorisation, not the record.
    pub fn mark_unanswered(&mut self, chat_id: i64, msg_id: &MsgId) -> std::io::Result<()> {
        if let Some(r) = self.records.get_mut(&ledger_key(chat_id, msg_id)) {
            r.answered = None;
        }
        self.save()
    }

    /// Forget a question that has been answered or withdrawn.
    pub fn forget(&mut self, chat_id: i64, msg_id: &MsgId) -> std::io::Result<()> {
        self.records.remove(&ledger_key(chat_id, msg_id));
        self.save()
    }

    /// Every message still carrying a live keyboard for one ask, so it can be retired.
    ///
    /// **Scoped to the session that asked it.** A bridge mints its ask ids from a counter that
    /// starts over with the process, so the first question of every session carries the same
    /// string — and a session that dies with its opening question still open leaves that record
    /// behind, because nothing prunes. Matching on the ask id alone therefore reached across
    /// sessions: a new session answering its own first question rewrote the dead one's question on
    /// the operator's phone with an outcome that belonged to a different question, and then deleted
    /// the record that proved it had ever been asked.
    pub fn messages_for(
        &self,
        project: &ProjectId,
        instance: &str,
        ask_id: &AskId,
    ) -> Vec<(i64, MsgId)> {
        self.matching(|r| &r.project == project && r.instance == instance && &r.ask_id == ask_id)
    }

    /// Every question left open by some run of this project OTHER than the one named, so a worker
    /// that never came back does not leave a keyboard on the operator's phone that nothing will
    /// ever take away.
    ///
    /// "Other than this one" is the whole of it, and it is asked when a bridge arrives rather than
    /// when one leaves. A claim is exclusive, so at the moment one is granted every other run of
    /// this project is provably not connected. Asked the other way round — at `release` — the
    /// answer would be wrong: a bridge keeps its instance across a reconnect, so a session that
    /// drops and comes straight back is still waiting for exactly those answers, and taking their
    /// keyboards away would be a live question removed from his phone.
    ///
    /// An already-answered record is left alone. Its keyboard is still live only because taking it
    /// away failed, and the outcome written on it is the true one — replacing that with a note
    /// about a restart would be the same misinformation from the other direction.
    pub fn open_for_other_instances(
        &self,
        project: &ProjectId,
        instance: &str,
    ) -> Vec<(i64, MsgId)> {
        self.matching(|r| &r.project == project && r.instance != instance && r.answered.is_none())
    }

    /// The chat and message behind every record the predicate accepts.
    fn matching(&self, mut want: impl FnMut(&AskRecord) -> bool) -> Vec<(i64, MsgId)> {
        self.records
            .iter()
            .filter(|(_, r)| want(r))
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
    /// Whose turn it is to send. Held for the whole of one send's pacing wait, so that projects
    /// queue for the rhythm instead of racing for it — see `send_into` for what racing cost.
    send_permit: Arc<Mutex<()>>,
    /// The one-line gist put above a question, when one is configured.
    ///
    /// `None` unless the operator has set it up, and that default matters: a gist is the only thing
    /// in this binary that sends an agent's words to a model, so it is off until someone says
    /// otherwise. `summarize.rs` proves the endpoint is on this machine before a single character
    /// of the agent's text is on the wire.
    ///
    /// One call site, agent to operator, never the reverse.
    gist: Option<Arc<crate::summarize::Summarizer>>,
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
            send_permit: Arc::new(Mutex::new(())),
            gist: crate::summarize::Summarizer::from_env().map(Arc::new),
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
            let mut registry = self.registry.lock().await;
            // Fresh, every admission. A secret rotated at the terminal has to take effect on the
            // next connection, not on the next restart — otherwise rotating a LEAKED secret leaves
            // the leaked one working and locks the honest bridge out, which is the opposite of what
            // rotation is for.
            //
            // A failed re-read leaves the previous map in place and admits against THAT. The
            // alternative — refusing everyone because one read failed — turns a transient file
            // error into a fleet-wide outage, and the alternative before that, adopting an empty
            // map, silently un-enrolled every project.
            if let Err(e) = registry.reread() {
                tracing::error!(
                    error = %e,
                    "could not re-read the project registry; admitting against the last good copy"
                );
            }
            match registry.resolve(token) {
                None => return Admission::Refused(RefusedReason::UnknownProject),
                Some(p) => (p.id.clone(), p.enabled),
            }
        };
        if !enabled {
            return Admission::Refused(RefusedReason::NotEnabled);
        }

        let _ = pid;
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
        if record.answered.is_some() {
            return Err(TapRefusal::AlreadyAnswered);
        }

        // The connection is checked BEFORE the record is marked, so a tap that could not be
        // delivered leaves the question answerable — refusing it and then closing it would burn the
        // operator's only way to answer.
        {
            let claims = self.claims.lock().await;
            let Some(claim) = claims.get(&record.project) else {
                return Err(TapRefusal::NotConnected);
            };
            if claim.instance != record.instance {
                return Err(TapRefusal::Restarted);
            }
        }

        // Marked HERE, under the ledger lock, as part of resolving. Doing it after the caller has
        // delivered would leave a window in which a second tap resolves too — and the window is
        // exactly as long as a Telegram round trip, on a keyboard the operator is still looking at.
        {
            let mut ledger = self.ledger.lock().await;
            match ledger.get(chat_id, msg_id) {
                None => return Err(TapRefusal::NoRecord),
                Some(fresh) if fresh.answered.is_some() => {
                    return Err(TapRefusal::AlreadyAnswered);
                }
                Some(_) => {}
            }
            if let Err(e) = ledger.mark_answered(chat_id, msg_id, option_id) {
                // Fail closed: if the answer cannot be written down, it must not be sent. An
                // unrecorded answer is one that can be given again.
                tracing::error!(error = %e, "could not write down that a question was answered");
                return Err(TapRefusal::NoRecord);
            }
        }
        Ok((record.project, record.ask_id, option_id.clone()))
    }

    /// Hand a frame to a project's live connection.
    ///
    /// The sender is CLONED out and the guard dropped before anything is awaited. Holding the
    /// claims lock across the send was a fleet-wide stall waiting to happen: the outbox is bounded,
    /// so one bridge that stopped reading would park this `.await` with the lock held, and every
    /// other project's admit, claim, release and deliver would queue behind it. One wedged bridge,
    /// every project silent.
    ///
    /// `try_send` rather than `send`, for the same reason from the other direction: a full outbox
    /// means that bridge is not keeping up, and the honest answer is "not delivered" now rather
    /// than an await that might never finish.
    pub async fn deliver(&self, project: &ProjectId, frame: HubFrame) -> bool {
        let tx = {
            let claims = self.claims.lock().await;
            match claims.get(project) {
                None => return false,
                Some(claim) => claim.tx.clone(),
            }
        };
        let env = Envelope::new(FrameId::new(format!("h{}", next_frame_seq())), frame);
        match tx.try_send(env) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(project = %project, error = %e, "a bridge is not keeping up; not delivered");
                false
            }
        }
    }

    /// Take the project, or refuse — the check and the reservation in ONE critical section.
    ///
    /// This used to be two: `admit` looked for a live incumbent, dropped the lock, and `claim`
    /// inserted unconditionally some awaits later. Two bridges arriving inside that window were
    /// both admitted and the second silently replaced the first — measured at roughly one round in
    /// three when the two `hello`s land within about 100 µs on a multi-thread runtime, which is the
    /// runtime this binary builds. The consequence is the exact failure gate 4 exists to prevent:
    /// two bridges live on one project, both posting into one topic, and a tap on the incumbent's
    /// still-open question refused with "that session has since restarted" while it is sitting
    /// there waiting for the answer.
    ///
    /// A dead incumbent is evicted rather than honoured: a worker that crashed must not lock its
    /// own project out until someone finds a keyboard.
    pub async fn claim(
        &self,
        project: ProjectId,
        pid: u32,
        instance: String,
        tx: mpsc::Sender<Envelope<HubFrame>>,
    ) -> Result<(), RefusedReason> {
        {
            let mut claims = self.claims.lock().await;
            if let Some(old) = claims.get(&project) {
                if pid_is_alive(old.pid) {
                    tracing::warn!(
                        project = %project, incumbent = old.pid, arriving = pid,
                        "a second bridge tried to take a project that is already connected"
                    );
                    return Err(RefusedReason::AlreadyClaimed);
                }
                tracing::info!(project = %project, dead = old.pid, "evicting a bridge that is no longer running");
            }
            claims.insert(project.clone(), Claim { pid, instance, tx });
        }
        Ok(())
    }

    /// Take the keyboard off every question a run of this project OTHER than this one left open.
    ///
    /// Run when a bridge ARRIVES, because that is the one moment the hub can prove those sessions
    /// are gone: a claim is exclusive, so nothing else holds this project now. Every other place it
    /// could have gone is wrong. At `release` a bridge that merely lost its socket would have its
    /// live questions taken off the phone, because a bridge keeps its instance across a reconnect
    /// and is still waiting for those answers. At eviction — which is where this started — it fires
    /// only when a claim was left behind by a pid that is no longer running, and every ordinary way
    /// a bridge goes away reaches `release` first, so the successor finds an empty claims map and
    /// nothing is ever swept. Sessions open when the hub itself restarted are missed the same way.
    ///
    /// Doing it here also means a retirement Telegram refused is tried again by the next session,
    /// which matters because a record left behind by a failed edit has nothing else that would ever
    /// come back to it.
    ///
    /// **Spawned, never awaited on the handshake.** It is one Telegram edit per abandoned question
    /// and nothing bounds how many there are; awaited before `Welcome`, six of them measured 1.8
    /// seconds, and the bridge cannot do anything at all until `Welcome` arrives — it holds
    /// everything the agent says until then, and starts dropping it after sixty-four. The session
    /// paying that would be the one that just came back.
    async fn retire_what_other_sessions_left(&self, project: &ProjectId, instance: &str) {
        let targets = {
            self.ledger
                .lock()
                .await
                .open_for_other_instances(project, instance)
        };
        if targets.is_empty() {
            return;
        }
        // What is said is that the session restarted, and nothing more. Never an outcome — no
        // question retired here was ever answered, and saying otherwise is the exact misinformation
        // the instance filter exists to stop.
        self.retire_each(
            project,
            targets,
            "the session that asked this restarted, so it is not waiting for an answer any more",
        )
        .await;
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

    /// Give the hub a different outbound budget. Test-only.
    ///
    /// The real limits are one message a second and eighteen a minute, so a flow test at those
    /// values would spend a second per message. The limits themselves are tested at their real
    /// values — `queue.rs`'s own tests, and `pacing_waits_but_a_real_flood_is_shed` below.
    #[cfg(test)]
    pub fn with_budget(self, per_minute: u32, min_gap: Duration) -> Self {
        Self {
            budgets: Arc::new(Mutex::new(crate::queue::Budgets::new(per_minute, min_gap))),
            ..self
        }
    }

    /// Test-only, and it stays that way. The DELIVERY path must never ask this: it asks by trying
    /// to deliver, which is the question it actually has, and a "is it connected" read taken a
    /// moment earlier is a fact that can already be wrong by the time it is acted on.
    ///
    /// [`Self::connected_ids`] answers a different question — what to show a person — and there a
    /// snapshot is the honest answer rather than a stale one.
    #[cfg(test)]
    pub async fn is_claimed(&self, project: &ProjectId) -> bool {
        self.claims.lock().await.contains_key(project)
    }

    /// Which projects have a bridge on the socket right now, for a human reading a list.
    ///
    /// A snapshot, deliberately, and it is the right shape for this one caller: by the time he has
    /// read the message anything in it may have changed, and he knows that about a status list. The
    /// alternative on offer was worse than stale — the list rendered a project's topic binding,
    /// which is permanent from its first connection onward and says nothing whatever about now.
    pub async fn connected_ids(&self) -> BTreeSet<ProjectId> {
        self.claims.lock().await.keys().cloned().collect()
    }

    /// Which project owns a topic, if any.
    ///
    /// A message typed in a topic belongs to that project and to no other. This is the whole of
    /// routing: rule 0 and nothing else. Every supergroup numbers its reply threads from one
    /// counter, which is how a swipe-reply on a direct message once reached a forum pane — deleting
    /// the other rules deletes that failure rather than testing against it.
    pub async fn project_for_topic(&self, topic_id: i32) -> Option<ProjectId> {
        self.registry
            .lock()
            .await
            .all()
            .find(|p| p.topic_id == Some(topic_id))
            .map(|p| p.id.clone())
    }

    /// Hand the operator's own words to a project, verbatim and exactly once.
    ///
    /// **Opaque.** The hub does not parse it, does not act on it, and does not let it name
    /// anything. Inbound content selects; it never names. What the agent receives is a MESSAGE in
    /// its own turn — never a keystroke — which is why the operator's phone and his laptop are no
    /// longer two writers fighting over one keyboard.
    ///
    /// Returns whether it was delivered, so the caller can say so rather than guess. A project that
    /// is not connected is told to the operator visibly, in the topic, and never queued: a message
    /// held for a worker that may never return is a message he believes was sent.
    pub async fn relay(
        &self,
        project: &ProjectId,
        chat_id: i64,
        user_id: i64,
        msg_id: &MsgId,
        text: &str,
    ) -> bool {
        if !self.chat_is_allowed(chat_id) {
            return false;
        }
        let delivered = self
            .deliver(
                project,
                HubFrame::Message {
                    msg_id: msg_id.clone(),
                    text: text.to_owned(),
                    from: hub_proto::From { chat_id, user_id },
                    in_reply_to_ask: None,
                },
            )
            .await;
        let _ = if delivered {
            self.audit
                .outcome(project, &SendOutcome::Sent(msg_id.clone()))
        } else {
            self.audit.refused(project, "the project was not connected")
        };
        delivered
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
        // A bind that fails must not be reported as a topic. Returning Ok here meant the next
        // message created ANOTHER topic, and the one before it was orphaned — one new empty topic
        // per message, for as long as the registry stayed unreadable, with every message dropped.
        if let Err(e) = self.registry.lock().await.bind_topic(project, id) {
            tracing::error!(
                project = %project, topic = id, error = %e,
                "made a topic and could not write it down; it is orphaned and no message will be \
                 sent until the registry is readable again"
            );
            return Err(e.into());
        }
        // Greeted immediately, in the same breath as being created — the greeting is what makes the
        // topic appear in the list at all. Through the SAME budgeted, audited path as everything
        // else: a greeting that skipped the budget was a writer the ceiling could not see, and one
        // that skipped the audit was a send with no record, which is the one thing the audit
        // discipline exists to make impossible.
        let _ = self
            .send_into(project, id, &format!("{title} is connected."), &[])
            .await;
        Ok(id)
    }

    /// The one place a message actually goes out: budget, clip, audit, send, audit.
    ///
    /// Every hub-owned write goes through here. Anything that bypassed it would be a writer the
    /// per-chat ceiling cannot see — and the ceiling is per chat, so an unmetered writer does not
    /// cost itself, it costs whichever project happens to send next.
    async fn send_into(
        &self,
        project: &ProjectId,
        topic_id: i32,
        text: &str,
        buttons: &[AskOption],
    ) -> SendOutcome {
        // PACE, then shed — and pace in a QUEUE, not as a crowd.
        //
        // Telegram's limit has two halves: no more than one message a second, and about twenty a
        // minute. The first is a rhythm — wait a beat and the message still arrives — the second is
        // a real ceiling. Only the ceiling should ever lose a message.
        //
        // The first attempt at this let each sender sleep once and then shed. That reads fine and
        // is wrong the moment there are two projects: every waiter sleeps the SAME second, they all
        // wake together, one wins the token, and the rest have already used their single sleep and
        // fall through to the shed. Measured on the real code: ten projects saying one thing each
        // produced two sends and eight sheds, with sixteen of the eighteen per-minute tokens
        // unspent. Six bridges opening with a question left five agents blocked and four topics
        // bound-but-empty, which Telegram does not show in the topic list at all.
        //
        // The permit is what makes it a queue. `tokio::sync::Mutex` hands it out in order, so each
        // sender waits its own turn once instead of racing the others for one token. The deadline
        // bounds the whole thing, because this is called from the connection's read loop and a
        // bridge whose socket goes unread for minutes is a bridge whose `bye` is missed.
        // The deadline starts BEFORE the permit is acquired, because the queue is most of the wait.
        // Starting it afterwards bounded only the sleep, so a bridge behind nine others could sit in
        // the read loop for a minute and then still be told it was too fast.
        let give_up_at = std::time::Instant::now() + MAX_PACE_WAIT;
        let turn = match tokio::time::timeout(MAX_PACE_WAIT, self.send_permit.lock()).await {
            Ok(t) => t,
            Err(_) => {
                let outcome = SendOutcome::TooFast(MAX_PACE_WAIT);
                let _ = self.audit.outcome(project, &outcome);
                return outcome;
            }
        };
        loop {
            let verdict = {
                let mut budgets = self.budgets.lock().await;
                budgets.take(self.forum_chat, std::time::Instant::now())
            };
            match verdict {
                Ok(()) => break,
                Err(crate::queue::Refusal::Gap(wait))
                    if std::time::Instant::now() + wait <= give_up_at =>
                {
                    tokio::time::sleep(wait).await;
                }
                Err(refusal) => {
                    // Either the per-minute ceiling — a real limit — or a queue so long that
                    // waiting longer would cost the connection more than the message is worth.
                    let outcome = SendOutcome::TooFast(refusal.wait());
                    let _ = self.audit.outcome(project, &outcome);
                    return outcome;
                }
            }
        }

        // The permit is released HERE, before the network call. It exists to order the waiting, not
        // to serialise Telegram: holding it across `surface.send` made one slow round trip a pause
        // for every other project's read loop.
        drop(turn);

        // Clipped here rather than by the surface, because whether anything was lost is a fact the
        // BRIDGE has to be told, and only this side is holding the ack.
        let (text, clamped) = crate::queue::fit(text, crate::queue::MAX_TEXT);
        let _ = self.audit.sent(project, topic_id, text.len());
        let mut outcome = self.surface.send(topic_id, &text, buttons).await;
        if clamped && let SendOutcome::Sent(id) = outcome {
            outcome = SendOutcome::Clamped(id);
        }
        let _ = self.audit.outcome(project, &outcome);
        outcome
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
        let outcome = self.send_into(project, topic_id, text, buttons).await;

        if outcome == SendOutcome::TopicGone {
            // Exactly once, and never as a retry: Telegram gives no service message when a topic is
            // deleted and no way to list them, so this is first-class rebinding. Treating it as a
            // transient would make the project's messages disappear quietly and forever.
            tracing::warn!(project = %project, "the topic is gone; making a new one");
            let _ = self.registry.lock().await.unbind_topic(project);
            if let Ok(fresh) = self.topic_for(project).await {
                return self.send_into(project, fresh, text, buttons).await;
            }
        }
        outcome
    }

    /// Handle one bridge, from `hello` to the connection closing.
    ///
    /// The order here is the design, so it is worth reading as one sequence: identify the peer,
    /// admit or refuse, admit BEFORE creating anything, prove the far end is really there, and only
    /// then create the topic that makes the project visible.
    pub async fn serve_connection(self: Arc<Self>, stream: UnixStream) -> anyhow::Result<()> {
        // The kernel's own answer, both fields. The pid a bridge puts in its `hello` is a number it
        // chose; this one is a fact about the process on the other end of THIS socket. Gate 4's
        // liveness check runs on it, so a bridge cannot make itself look dead — or make an
        // incumbent look dead — by reporting a pid that is not its own.
        let peer = peer_cred(&stream)?;
        let (rx_half, mut tx_half) = stream.into_split();
        let mut reader = hub_proto::FrameReader::new(rx_half);

        // A connection that never says hello used to sit here forever, holding a task and a file
        // descriptor. Nothing had authenticated at that point, so anything that can reach the
        // socket could park as many as it liked until the process ran out of descriptors — and the
        // accept loop's first error would then have been fatal.
        let first = match tokio::time::timeout(self.settle, reader.next::<BridgeFrame>()).await {
            Err(_) => {
                tracing::debug!("a connection was opened and never said anything");
                return Ok(());
            }
            Ok(Err(hub_proto::ProtoError::Oversize { max })) => {
                // Told, not just closed. A bridge that knows its frame was too big can split it;
                // a bridge handed a closed socket can only guess.
                let env = Envelope::new(
                    FrameId::new("h-refused"),
                    HubFrame::Refused {
                        reason: RefusedReason::FrameTooLarge,
                    },
                );
                let _ = hub_proto::write_frame(&mut tx_half, &env).await;
                tracing::warn!(max, "a bridge's first frame was over the ceiling");
                return Ok(());
            }
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "a connection ended before it said hello");
                return Ok(());
            }
            Ok(Ok(None)) => return Ok(()),
            Ok(Ok(Some(f))) => f,
        };

        let project = match self
            .admit(peer.uid, our_uid(), &first.payload, first.v)
            .await
        {
            // No reply at all. A refusal would confirm that something is listening here.
            Admission::ClosedSilently => return Ok(()),
            Admission::Refused(reason) => {
                let env = Envelope::new(FrameId::new("h-refused"), HubFrame::Refused { reason });
                let _ = hub_proto::write_frame(&mut tx_half, &env).await;
                return Ok(());
            }
            Admission::Admitted(id) => id,
        };

        let BridgeFrame::Hello {
            instance,
            pid: claimed_pid,
            ..
        } = first.payload.clone()
        else {
            unreachable!("admit only admits a hello");
        };
        if claimed_pid != peer.pid {
            // Not fatal — a bridge behind a wrapper legitimately does not know its own outermost
            // pid. It IS worth a line, because the audit trail should record which number was
            // believed and which was merely offered.
            tracing::debug!(
                claimed = claimed_pid,
                actual = peer.pid,
                "a bridge reported a pid that is not the one on its socket; using the socket's"
            );
        }
        let pid = peer.pid;

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

        if let Err(reason) = self
            .claim(project.clone(), pid, instance.clone(), tx.clone())
            .await
        {
            let env = Envelope::new(FrameId::new("h-refused"), HubFrame::Refused { reason });
            let _ = tx.send(env).await;
            // Give the writer a moment to put the refusal on the wire before the task is dropped;
            // a refusal nobody receives is the same as the silent takeover this replaced.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            writer.abort();
            let _ = self
                .audit
                .refused(&project, "another bridge already holds this project");
            return Ok(());
        }

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

        // Anything the bridge says before its pong is KEPT, not dropped on the floor.
        //
        // A bridge that opens with a question — which is the whole point of the product — used to
        // have it read, discarded, and never acked. No message, no record, no reply, and the agent
        // sitting blocked on an answer that could never come. Buffered here and replayed the
        // moment the project is live.
        let mut waiting: Vec<Envelope<BridgeFrame>> = Vec::new();
        let mut waiting_bytes = 0usize;
        let mut overflowed = false;
        let live = tokio::time::timeout(self.settle, async {
            loop {
                match reader.next::<BridgeFrame>().await {
                    Ok(Some(frame)) => {
                        if let BridgeFrame::Pong { r#ref } = &frame.payload
                            && r#ref == &ping_id
                        {
                            return true;
                        }
                        // BOUNDED, by count and by bytes. An unbounded Vec here let one connection
                        // hand the hub as much as it could write in the settling window — measured
                        // at 18 MB in 450 ms — before it had proved it was even there. The count
                        // matches the outbox's own 64.
                        waiting_bytes += frame_cost(&frame.payload);
                        if waiting.len() >= 64 || waiting_bytes > 4 * hub_proto::MAX_FRAME_BYTES {
                            overflowed = true;
                            return false;
                        }
                        waiting.push(frame);
                    }
                    // A line this build cannot DECODE is one bad frame, not a dead peer — and it is
                    // exactly what a bridge one version ahead sends. The post-pong loop survives it;
                    // this one used to end the connection and then audit it as "never answered",
                    // which blames the bridge for the hub's own strictness.
                    Err(hub_proto::ProtoError::Decode { source, len }) => {
                        tracing::warn!(len, error = %source, "a frame this build cannot read, before the pong; ignoring it");
                        continue;
                    }
                    _ => return false,
                }
            }
        })
        .await
        .unwrap_or(false);

        if !live {
            // Two different failures, said differently. A bridge that filled the buffer is talking
            // too much before it has proved it is there; one that said nothing is probably a channel
            // plugin that is not allowlisted, which boots and exits in about a tenth of a second.
            // Reporting the first as the second sends the operator looking in the wrong place.
            let why = if overflowed {
                "sent more before answering than the hub will hold for it"
            } else {
                "connected but never answered; it is probably not allowed to talk to me"
            };
            tracing::warn!(project = %project, why, "a bridge did not become live");
            let _ = self.audit.refused(&project, why);
            self.release(&project, pid).await;
            writer.abort();
            return Ok(());
        }

        // Live. NOW the topic exists, and the greeting is what makes it visible in the list.
        if let Err(e) = self.topic_for(&project).await {
            tracing::error!(project = %project, error = %e, "could not make a topic for a live project");
        }

        // Whatever the last run of this project left open comes off the phone now — in a task of
        // its own, so this session's own first question is never queued behind the cleanup of one
        // that is already gone. Nothing else reads those records, so it does not matter whether it
        // finishes before or after anything below.
        {
            let hub = Arc::clone(&self);
            let project = project.clone();
            let instance = instance.clone();
            tokio::spawn(async move {
                hub.retire_what_other_sessions_left(&project, &instance)
                    .await;
            });
        }

        for frame in waiting {
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
                // A line that will not DECODE is one bad frame, not a dead peer — and this is
                // exactly what a bridge one version ahead sends. Tearing the connection down for it
                // makes an additive change on the other side a project that goes silent. The
                // transport failures do end it, because after those there is nothing to read.
                Err(hub_proto::ProtoError::Decode { source, len }) => {
                    tracing::warn!(project = %project, len, error = %source, "a frame this build cannot read; ignoring it");
                    continue;
                }
                Err(hub_proto::ProtoError::Oversize { max }) => {
                    tracing::warn!(project = %project, max, "a frame over the ceiling; refusing it");
                    let _ = tx
                        .send(Envelope::new(
                            FrameId::new(format!("h{}", next_frame_seq())),
                            HubFrame::Refused {
                                reason: RefusedReason::FrameTooLarge,
                            },
                        ))
                        .await;
                    let _ = self
                        .audit
                        .refused(&project, "a frame was over the size ceiling");
                    // Queued is not sent. `break` used to fall straight into `writer.abort()`,
                    // which destroyed this refusal before the writer task could put it on the wire
                    // — so the bridge got the closed socket the refusal existed to replace. A
                    // bridge told its frame was too big can split it; one handed a dead socket can
                    // only guess. Dropping `tx` ends the writer's loop; the timeout is there
                    // because a peer that has stopped reading must not hold this open forever.
                    // Released FIRST. Waiting on the writer while still holding the claim meant a
                    // bridge doing the documented thing — split the frame, reconnect — was refused
                    // `already_claimed` for two seconds by the connection it had just been told to
                    // abandon. And releasing drops the claim's own `Sender`, so the writer's channel
                    // really closes and it ends at once rather than at the timeout.
                    self.release(&project, pid).await;
                    drop(tx);
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), writer).await;
                    return Ok(());
                }
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

                // A one line summary above the question, when one is configured. The operator reads
                // this on a phone and a long question is a wall of text he has to open before he
                // can decide; the gist is what makes the notification itself useful.
                //
                // It NEVER replaces the question. A summary standing in for what was actually said
                // is a defect this repo has already shipped once, and the eight voice rules that
                // came out of it start with that one.
                let text = match &self.gist {
                    None => text,
                    Some(g) => {
                        let summarised = g.one_line(&text).await;
                        // The trip-off is ONE WAY and it has to be said out loud, once. Until this
                        // existed the only place it was ever said was the journal, which is not on
                        // the phone he is reading — so a refusal that protected him and a gateway
                        // that had merely gone quiet looked identical from the outside: summaries
                        // simply stopped, with nothing saying why or that they were not coming back.
                        if g.newly_off() {
                            let _ = self
                                .say(project, "I have stopped summarising. Questions still reach you in full.", &[])
                                .await;
                        }
                        match summarised {
                            None => text,
                            Some(line) => format!("{line}\n\n{text}"),
                        }
                    }
                };

                // Telegram gives a button 64 bytes of `callback_data` and no more. The option id is
                // minted by the BRIDGE, so it is agent-authored and arbitrary: too long and the API
                // refuses the whole message, leaving an agent blocked on a question that was never
                // asked. A `|` is worse than that — it survives the send and then splits wrong on
                // the way back, so the tap resolves to nothing while looking perfectly fine.
                //
                // Refused here, where the bridge can still be told and can ask again differently.
                if let Some(bad) = options.iter().find(|o| {
                    let data = o.option_id.as_str();
                    data.contains('|') || data.len() + 2 > CALLBACK_DATA_MAX
                }) {
                    tracing::warn!(
                        project = %project, option = %bad.option_id,
                        "a question's answer ids will not fit in a Telegram button; refusing it"
                    );
                    let _ = self.audit.refused(
                        project,
                        "an answer id was too long or contained a separator",
                    );
                    // One plain line where the operator can see it: an agent blocked on a question
                    // that never arrived is the failure this product exists to prevent, and it must
                    // not be visible only in a log.
                    let _ = self
                        .say(
                            project,
                            "This project asked me something I could not put on a button, so it is \
                             still waiting. Answer it at the terminal.",
                            &[],
                        )
                        .await;
                    return (Delivered::No, Some(hub_proto::AckWhy::TelegramRefused));
                }

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
                        // The CLIPPED text, because that is what the operator is actually looking
                        // at. Storing the original meant the retirement rebuilt the message from
                        // text longer than the one that was sent — and a retirement body over
                        // Telegram's 4096 is an edit that fails, which leaves the answered keyboard
                        // live and still offering choices that have already been made.
                        text: crate::queue::fit(&text, crate::queue::MAX_TEXT).0,
                        answered: None,
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
                self.retire(project, instance, &ask_id, how, outcome.as_deref())
                    .await;
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

    /// The operator answered from his phone: take the keyboard away and say what he chose.
    ///
    /// This used to just delete the ledger record. That left the keyboard live forever — the record
    /// the retirement needed was gone, so the `ask_resolved` that followed found nothing to retire —
    /// and a menu that stays tappable after it has been answered is the second live menu this
    /// design went out of its way not to manufacture.
    pub async fn answered_from_phone(&self, chat_id: i64, msg_id: &MsgId, label: &str) {
        let record = { self.ledger.lock().await.get(chat_id, msg_id).cloned() };
        let Some(record) = record else {
            let _ = self.ledger.lock().await.forget(chat_id, msg_id);
            return;
        };
        let retired = self
            .surface
            .retire_buttons(
                record.topic_id,
                msg_id,
                &record.text,
                &format!("answered from your phone — {label}"),
            )
            .await;
        // The record is forgotten only when the buttons are actually gone. Forgetting first left a
        // live keyboard with nothing behind it: still tappable, still offering a choice already
        // made, and the next tap answered "I have no record of that question". Fail closed — the
        // order is "the buttons are gone, therefore the record may go", never the reverse.
        match retired {
            Ok(()) => {
                let _ = self.ledger.lock().await.forget(chat_id, msg_id);
            }
            Err(e) => tracing::error!(
                error = %e, project = %record.project,
                "answered from the phone but the keyboard is still there; leaving the record so it \
                 can be retired later"
            ),
        }
    }

    /// A tap resolved, and then the project could not be told. Take the question back.
    ///
    /// A tap is written down as answered BEFORE the caller delivers, and it has to be: the window
    /// between the two is a Telegram round trip on a keyboard the operator is still looking at, and
    /// a second tap inside it would deliver twice. The cost was that a delivery which then failed
    /// burned the question for good — nothing ever cleared `answered` — so he was told first that
    /// the project was not connected (false when it was merely behind, which is the outbox-full
    /// case this code documents as expected) and then, on the same button, that it had already been
    /// answered and nothing had been sent. Only the second half of that was true.
    ///
    /// **Nothing went out on this path.** `deliver` reports false only when there is no connection
    /// or the frame never entered the outbox, never after a frame has gone, so withdrawing cannot
    /// contradict something an agent has already been told.
    ///
    /// **Returns which of the three actually happened**, because the caller has to say so and the
    /// sentences are not interchangeable. Two would not do: `deliver` reports false when the
    /// bridge's outbox is full, and that bridge is LIVE — it can be sending `ask_resolved` for this
    /// very question in the same instant, which retires the keyboard and forgets the record. So
    /// "there was nothing left to take back" is a real outcome on this path and not a defensive
    /// branch, and reporting it as "the buttons are still there" would be a plain untruth. The record follows the same order as every
    /// other retirement here: it goes only once the buttons have. Forgetting it either way looked
    /// safe — nothing was sent, so nothing can be contradicted — but the record is also the only
    /// handle any LATER retirement has on that message, so throwing it away while the menu is
    /// still live means nothing can ever take the menu off: not the session's own withdrawal, not
    /// a timeout, not the next session. What is undone instead is the authorisation, so the
    /// still-live keyboard can be tapped again rather than answering "that was already answered".
    pub async fn withdraw_undelivered(&self, chat_id: i64, msg_id: &MsgId) -> Withdrawal {
        let record = { self.ledger.lock().await.get(chat_id, msg_id).cloned() };
        let Some(record) = record else {
            return Withdrawal::NothingLeftToTakeBack;
        };
        match self
            .surface
            .retire_buttons(
                record.topic_id,
                msg_id,
                &record.text,
                "not sent — the project could not be reached",
            )
            .await
        {
            Ok(()) => {
                let _ = self.ledger.lock().await.forget(chat_id, msg_id);
                Withdrawal::Retired
            }
            Err(e) => {
                tracing::error!(
                    error = %e, project = %record.project,
                    "a tap reached nobody and its keyboard is still on his phone"
                );
                let _ = self.ledger.lock().await.mark_unanswered(chat_id, msg_id);
                Withdrawal::StillOnHisPhone
            }
        }
    }

    /// Strip a stale keyboard, because a screen could never tell you a question stopped being asked.
    ///
    /// `instance` is which run of the worker is saying so, and it is half the address: ask ids
    /// repeat across sessions, so an outcome matched on the ask id alone landed on a question
    /// another session was still waiting on.
    async fn retire(
        &self,
        project: &ProjectId,
        instance: &str,
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
        let targets = {
            self.ledger
                .lock()
                .await
                .messages_for(project, instance, ask_id)
        };
        self.retire_each(project, targets, &note).await;
    }

    /// Take the keyboard off each of these messages and leave the note in its place.
    async fn retire_each(&self, project: &ProjectId, targets: Vec<(i64, MsgId)>, note: &str) {
        for (chat, msg) in targets {
            // The record carries both the topic and the question's own words, so the retirement
            // does not have to go back to the registry for one and cannot leave the other out.
            let record = { self.ledger.lock().await.get(chat, &msg).cloned() };
            let retired = match &record {
                Some(record) => {
                    self.surface
                        .retire_buttons(record.topic_id, &msg, &record.text, note)
                        .await
                }
                None => Ok(()),
            };
            // Same order as a tap: the record goes only once the keyboard has. A retirement that
            // failed and forgot anyway leaves a menu that outlives the question it belonged to.
            match retired {
                Ok(()) => {
                    let _ = self.ledger.lock().await.forget(chat, &msg);
                }
                Err(e) => tracing::error!(
                    error = %e, project = %project,
                    "a question stopped being asked but its keyboard is still there"
                ),
            }
        }
    }
}

/// Roughly what one buffered frame costs to hold, for the pre-pong bound.
///
/// The text is the whole of it in practice; the rest is a fixed handful of bytes. Exact accounting
/// would mean encoding a frame this side is about to hand straight to `handle`, which is a cost
/// paid on every frame to make a bound slightly tighter.
fn frame_cost(frame: &BridgeFrame) -> usize {
    match frame {
        BridgeFrame::Say { text, .. }
        | BridgeFrame::Done { text }
        | BridgeFrame::Ask { text, .. } => text.len() + 64,
        _ => 64,
    }
}

/// Per-process frame counter. Opaque and monotonic is all the protocol asks for.
fn next_frame_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Who is on the other end of a connection, according to the kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerCred {
    pub uid: u32,
    /// The process on the other end of THIS socket. Not the pid the bridge says it is: gate 4's
    /// liveness check runs on this one, so a wrong number here would let a bridge make an incumbent
    /// look dead and take its project.
    pub pid: u32,
}

/// Read the credentials of whoever is on the other end of a connection.
pub fn peer_cred(stream: &UnixStream) -> std::io::Result<PeerCred> {
    let cred = rustix::net::sockopt::socket_peercred(stream)?;
    Ok(PeerCred {
        uid: cred.uid.as_raw(),
        pid: cred.pid.as_raw_nonzero().get() as u32,
    })
}

/// This process's uid, for comparison against the peer's.
pub fn our_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

#[cfg(test)]
mod tests;

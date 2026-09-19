//! Who may connect, and what their topic is called.
//!
//! # The registry is the only source of a project's name
//!
//! `hello` carries a project id and a token. It does **not** carry a display name, and this module
//! is why that is safe: the hub resolves the **token** to a project and takes the title, the
//! routing key and the audit subject from here. A bridge cannot name itself, so it cannot claim
//! another project's topic — and the field on the wire that looks like a name is ignored on
//! purpose. `a_bridge_cannot_claim_another_projects_identity` pins that.
//!
//! # Enrolment is terminal-only
//!
//! Nothing that arrives over Telegram or over the socket can add a project, mint a token, flip
//! `enabled`, or let a person speak. Inbound content selects from what the machine already knows;
//! it never names something new. The only way in is `herdr-tg enroll <repo>`, run by a human at a
//! keyboard — the only way off, or back on, is `herdr-tg disable <repo>` / `enable <repo>` at the
//! same keyboard — and the only way a person is let into a project's conversations is
//! `herdr-tg allow <repo> <user>` there too. `nothing_inbound_can_add_a_person.rs` pins that the
//! bot and the hub do not so much as name the setter.
//!
//! # What is stored, and what is not
//!
//! The token itself is never written here — only a SHA-256 of it. A registry file that leaks tells
//! an attacker which projects exist and nothing they can connect with. The secret lives in the
//! project's own tree at `<repo>/.kickoff/hub.token`, mode 0600, where the bridge can read it and
//! `.gitignore` keeps it out of a public remote.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use hub_proto::{LaneId, ProjectId};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::hub::Addr;

/// Where a project's bridge finds its own secret, relative to the repo root.
pub const TOKEN_FILE: &str = ".kickoff/hub.token";

/// Longest topic title Telegram will take from us, and the point titles are clipped to.
const MAX_TITLE: usize = 48;

/// Telegram offers exactly six topic colours. Six projects colour-coded for free, stable forever,
/// with no decision for the operator to make.
const ICON_COLOURS: u8 = 6;

/// One enrolled project.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: ProjectId,
    /// What the topic is called. Derived from the repo's directory name, never hand-authored:
    /// fixing a topic title should not need a laptop and a JSON edit.
    pub title: String,
    /// The canonical repo path. For the audit record and for `herdr-tg projects`.
    pub repo: PathBuf,
    /// Lowercase hex of the SHA-256 of the enrolment secret. Never the secret.
    pub token_sha256: String,
    /// Enrolled but switched off is a real state: it is how a project is retired without losing
    /// its topic and its history.
    pub enabled: bool,
    /// Bound on first live connection, not at enrolment. A topic with no messages is invisible in
    /// Telegram's topic list, so one created early is the same as no topic to the person looking.
    pub topic_id: Option<i32>,
    /// `0..6`. Derived, so it is stable across restarts and across machines.
    pub icon_color: u8,
    /// One topic per lane of this project, bound the first time that lane goes live.
    ///
    /// It lives HERE, inside the project the secret already resolved to, rather than in a file of
    /// its own. Three reasons, and the first is the one that matters: `bind_topic` already carries
    /// the whole read-modify-write discipline this needs — the flock, the re-read, the atomic
    /// rename, the per-process temp name — and that discipline exists because two processes write
    /// this file. A second state file would have to reproduce every part of it. Second, the reverse
    /// lookup from a topic stays one scan over one store instead of two lookups with a rule about
    /// which wins. Third, a lane row landing inside an already-resolved project is a write a bridge
    /// cannot use to add a project or flip `enabled`, so the trust boundary is exactly where it was.
    ///
    /// `#[serde(default)]` because the file on the operator's box was written by a build that had
    /// never heard of a lane, and a registry that would not parse refuses every project on the box.
    ///
    /// **Nothing prunes this**, and that is the accepted price of a topic per lane with retirement
    /// out of scope — not an oversight. Measured rather than feared: a lane row costs 40 bytes, so
    /// twelve a day is 15 KB a month and about 175 KB a year, and a full parse of a 360-lane file
    /// takes half a millisecond on an admission that happens a dozen times a day. He will never
    /// notice this file. What he WILL notice, in this order, is the forum topic list on his phone,
    /// then the per-chat delivery budget on a dispatch day — and `asks.json`, which is rewritten
    /// whole on every ask and every tap and is where the real growth is guarded. See
    /// `AskLedger::save`.
    #[serde(default)]
    pub lane_topics: BTreeMap<LaneId, i32>,
    /// The people who may speak in THIS project's conversations — its own topic and every one of
    /// its lanes' — and nowhere else. Telegram user ids.
    ///
    /// Beside the people who may speak anywhere (the bot's own list, which comes from the
    /// configuration and is never in this file), this is how a room admits its own people: a
    /// customer or a teammate who belongs in one project's conversations does not thereby belong
    /// in every other project's. Set only by `herdr-tg allow <repo> <user>` at a terminal, and
    /// read by the hub for every typed line and every tap.
    ///
    /// `#[serde(default)]` for the same reason `lane_topics` has it: the file on the operator's
    /// box was written by a build that had never heard of this, and a registry that will not parse
    /// refuses every project on the box.
    #[serde(default)]
    pub allowed_users: BTreeSet<i64>,
}

/// A room minted at a terminal that nothing has connected as yet: a row and a secret and nothing
/// he can see, hidden from every list until its first live connection binds it a topic.
///
/// Decided from two facts every writer keeps — the id's shape and the topic — and NOT from a
/// flag. The first version carried a `vacant` field, and the hub running on the box was built
/// before that field existed: it rewrites this file on every topic bind, serialising only the
/// fields it knows, so the first lane `hello` anywhere on the box after a `grant` dropped the
/// flag from every row and every room reached the phone as a row of nothing, for good.
pub fn is_nothing_yet(p: &Project) -> bool {
    is_room(&p.id) && p.topic_id.is_none()
}

/// What a lane's topic is called, so the operator can pick it out of a list on a phone.
///
/// The PROJECT comes first, so a lane sorts and reads under the project it belongs to; he will have
/// the project's own topic and several of its lanes side by side. The lane comes second, and when it
/// will not fit it is clipped from the LEFT, because real lane names share a `lane-<date>-` head and
/// differ only in the tail — clipped the other way, every lane of one project reads identically and
/// the list stops telling him anything.
pub fn lane_title(project_title: &str, lane: &LaneId) -> String {
    const SEP: &str = " · ";
    /// Enough tail left to tell two lanes of one project apart, even when the project's own title
    /// has taken nearly all the room.
    const LANE_TAIL_MIN: usize = 10;

    /// How much of a lane name is worth showing before a phone's own truncation takes over.
    ///
    /// Without a cap the clip below never fires for a real name — `lane-<MMDD>-<HHMMSS>-<pid>` is
    /// 24 characters and the room left by a short project title is 37 — so the shared `lane-<date>-`
    /// head survived whole and every distinguishing byte sat at the right-hand end, which is exactly
    /// what a list row removes. Capped, the clip always fires and the tail leads.
    const LANE_SHOWN_MAX: usize = 14;

    let sep = SEP.chars().count();
    // The project's title is kept WHOLE where it fits, because it is the half that groups the row.
    // It is clipped only to leave the lane a readable tail.
    let head_room = MAX_TITLE.saturating_sub(sep + LANE_TAIL_MIN);
    let head: String = project_title.chars().take(head_room).collect();

    let room = MAX_TITLE
        .saturating_sub(head.chars().count() + sep)
        .min(LANE_SHOWN_MAX);
    let lane_chars: Vec<char> = lane.as_str().chars().collect();
    if lane_chars.len() <= room {
        return format!("{head}{SEP}{lane}");
    }
    let keep = room.saturating_sub(1);
    let tail: String = lane_chars[lane_chars.len() - keep..].iter().collect();
    format!("{head}{SEP}…{tail}")
}

/// What a switch at the terminal changed: the row it named, and the rooms switched with it.
#[derive(Debug)]
pub struct Switched {
    pub named: Project,
    pub rooms: Vec<Project>,
}

/// Every enrolled project, and the file they live in.
#[derive(Debug)]
pub struct Registry {
    path: PathBuf,
    projects: BTreeMap<ProjectId, Project>,
}

/// What went wrong enrolling a project.
#[derive(Debug)]
pub enum EnrolError {
    /// The path is not a directory this machine can see.
    NoSuchRepo { repo: PathBuf },
    /// The registry or the token file could not be written.
    Io {
        what: String,
        source: std::io::Error,
    },
    /// The system refused to give us random bytes. Fail closed: a predictable token is worse than
    /// no token, and quietly falling back to a weaker source is how that happens.
    NoRandomness,
    /// The existing registry cannot be read, so enrolling would replace every other project with
    /// this one. Refused — and told how to recover, because refusing without a way forward turns a
    /// bad byte into a locked door.
    Unreadable { path: PathBuf, why: String },
    /// The folder is inside one that is already enrolled.
    ///
    /// Refused, not warned. Allowing it made one repository into two projects with two separate
    /// chats, and which one a question landed in depended on the folder the session happened to
    /// start in — the hub cannot tell them apart, because it resolves a connection by its secret
    /// and throws the `repo` it was sent away. It also wrote a second secret one level down, where
    /// the guard that checks whether git would commit it was looking for a `.git` that is only ever
    /// at the top.
    InsideAnotherProject {
        repo: PathBuf,
        parent: PathBuf,
        title: String,
    },
    /// The new secret reached the repo and the list of projects did not, so that project's bridge
    /// will be refused until this is run again.
    ///
    /// Its own variant because silence here is the worst outcome there is: nothing else on the box
    /// would ever explain why a project that worked yesterday is turned away.
    HalfWritten { repo: PathBuf, why: String },
    /// Asked to switch a project on or off, and nothing is enrolled at that path.
    ///
    /// Refused rather than enrolled on the spot: switching on is not a way in. Enrolment mints a
    /// secret and writes it into a tree, and a command that quietly did that when it was only asked
    /// to flip a switch is a second door with no guard on it.
    NotEnrolled { repo: PathBuf },
    /// `open` on a repo enrolled the old way: a row and a repo token, but no copy of the secret
    /// where the channel keeps one. `open` cannot make one — it holds only the hash, and minting a
    /// fresh secret would take the live bridge off the air — so it names the verb that copies the
    /// existing bytes across.
    EnrolledTheOldWay { repo: PathBuf },
    /// `open` on a project whose channel copy is not the secret the hub knows. Nothing presenting
    /// it is admitted, so "already open" would be a lie; the verb that mends it depends on
    /// whether the repo still holds the current bytes.
    ChannelCopyStale {
        repo: PathBuf,
        repo_is_current: bool,
    },
    /// `open` wrote the secret and could not save the list of projects. Nothing is on the air yet,
    /// so this is a leftover directory rather than a lockout; the verb to run again is named.
    HalfOpened { repo: PathBuf, why: String },
    /// The book has no room for what was asked.
    BookFull {
        title: String,
        vacant: usize,
        asked: usize,
    },
    /// The rooms' secrets and slots were written, the list of projects could not be saved, and
    /// they were taken back again: nothing was granted.
    HalfGranted { title: String, why: String },
    /// Asked for rooms of something that is itself a room.
    NotASeed { id: ProjectId },
    /// Asked to let something speak that is not a person: a group's id, a zero, a chat.
    ///
    /// Refused rather than stored, because a number on this list that can never match a sender
    /// is one the operator believes has let somebody in — and a group's id in particular reads as
    /// "everyone in that group", which is precisely the grant this list exists to make impossible.
    NotAPerson { given: i64 },
}

impl std::fmt::Display for EnrolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchRepo { repo } => write!(
                f,
                "there is no directory at {} — enrol the project by its own folder",
                repo.display()
            ),
            Self::Io { what, source } => write!(f, "could not write {what}: {source}"),
            Self::NoRandomness => write!(
                f,
                "this machine would not give me random bytes, so I will not invent a secret"
            ),
            Self::InsideAnotherProject {
                repo,
                parent,
                title,
            } => write!(
                f,
                "{} is inside {}, which is already enrolled as \"{title}\". Enrolling it as well \
                 would split one project across two chats, and it would leave a second secret in a \
                 folder git is watching. Enrol the project by its own top folder instead:\n\
                 \n    herdr-tg enroll {}",
                repo.display(),
                parent.display(),
                parent.display()
            ),
            Self::HalfWritten { repo, why } => write!(
                f,
                "the new secret for {} was written, but the list of projects could not be saved — \
                 so the copies no longer match what the hub knows and its bridge will be turned \
                 away until you run this again:\n\
                 \n    herdr-tg enroll {}\n\nWhat went wrong saving the list: {why}",
                repo.display(),
                repo.display()
            ),
            Self::Unreadable { path, why } => write!(
                f,
                "I cannot read the list of enrolled projects at {}, so enrolling now would replace \
                 every other project with this one. Move that file aside and enrol them all again:\n\
                 \n    mv {} {}.broken\n\nWhat went wrong reading it: {why}",
                path.display(),
                path.display(),
                path.display()
            ),
            Self::NotEnrolled { repo } => write!(
                f,
                "nothing is enrolled at {}, so there is nothing to change. See what is enrolled \
                 with:  herdr-tg projects",
                repo.display()
            ),
            Self::NotAPerson { given } => write!(
                f,
                "{}",
                crate::config::not_a_person("the user to allow", *given)
            ),
            Self::EnrolledTheOldWay { repo } => write!(
                f,
                "{} is already enrolled, with its secret in the repo and no copy where the channel \
                 keeps one. Opening it afresh would mint a new secret and turn its running bridge \
                 away. Copy the secret it has across instead:\n\n    herdr-tg adopt-secrets --apply",
                repo.display()
            ),
            Self::ChannelCopyStale {
                repo,
                repo_is_current: true,
            } => write!(
                f,
                "{} was opened before, but the copy of its secret the channel keeps is not the one \
                 the hub knows — a rotation by a build from before conversations existed rewrites \
                 the repo's copy and leaves the channel's behind — so every new session here is \
                 turned away. The repo still holds the current secret; copy it across:\n\n    \
                 herdr-tg adopt-secrets --apply",
                repo.display()
            ),
            Self::ChannelCopyStale {
                repo,
                repo_is_current: false,
            } => write!(
                f,
                "{} was opened before, but neither the copy of its secret the channel keeps nor \
                 the repo's is one the hub knows, so nothing can connect as it. Rotate its secret:\n\
                 \n    herdr-tg enroll {}",
                repo.display(),
                repo.display()
            ),
            Self::HalfOpened { repo, why } => write!(
                f,
                "the secret for {} was written where the channel keeps it, but the list of \
                 projects could not be saved, so nothing can connect as it yet. Run this again:\n\
                 \n    herdr-tg open {}\n\nWhat went wrong saving the list: {why}",
                repo.display(),
                repo.display()
            ),
            Self::BookFull {
                title,
                vacant,
                asked,
            } => write!(
                f,
                "{title} already has {vacant} vacant room{} and its book holds {} at once, so {asked} \
                 more cannot be granted. A room becomes taken when a dispatcher starts something as \
                 it; grant again once the book has pages free.",
                if *vacant == 1 { "" } else { "s" },
                crate::conversations::BOOK
            ),
            Self::HalfGranted { title, why } => write!(
                f,
                "the rooms' secrets and slots were written for {title}, but the list of projects \
                 could not be saved, so they were taken back again and nothing was granted. Grant \
                 again once the list can be saved. What went wrong saving it: {why}"
            ),
            Self::NotASeed { id } => write!(
                f,
                "{id} is a room, and a room does not get rooms of its own. Grant them to the \
                 project it belongs to, by that project's folder."
            ),
        }
    }
}

impl std::error::Error for EnrolError {}

impl Registry {
    /// `<state dir>/projects.json`.
    pub fn default_path() -> PathBuf {
        crate::lock::state_dir().join("projects.json")
    }

    /// Re-read the file into this handle, discarding what was in memory.
    ///
    /// The registry is not the hub's private state: `herdr-tg enroll` writes it from a terminal
    /// while the hub is running. A boot-time snapshot made two things wrong at once. Rotating a
    /// leaked secret had no effect until a restart — the leaked one kept working and the honest
    /// bridge was refused — and any write the hub made afterwards (binding a topic) put its stale
    /// map back over the file, erasing a project the operator had just enrolled.
    ///
    /// It is a few kilobytes, and it is read on admission and before every write, both of which are
    /// rare. Cheap enough that a cache would be an optimisation nobody asked for and a staleness
    /// bug somebody eventually finds.
    pub fn reread(&mut self) -> Result<(), EnrolError> {
        // Adopted ONLY on a clean read. The first version of this called `load`, which reports an
        // unreadable or corrupt file by returning an EMPTY map — correct for a constructor, and
        // catastrophic here: one transient failed read un-enrolled every project in the shared
        // handle, and the very next `save()` wrote that emptiness over the file. The hub would have
        // erased the enrolments it exists to protect, then refused every project, with nothing
        // anywhere saying why.
        //
        // A read that fails leaves what was already in memory alone and says so. The caller then
        // refuses to write, because a registry that cannot be read is exactly the state in which
        // nothing should be written over it.
        match read_projects(&self.path) {
            // A file that has DISAPPEARED is not an empty registry. For `load` it is — nothing has
            // been enrolled yet — but on a handle that already holds projects it is a loss, and
            // adopting it would un-enrol everything and then write the emptiness back.
            Ok(projects) if projects.is_empty() && !self.projects.is_empty() => {
                Err(EnrolError::Io {
                    what: "the registry".to_owned(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "the registry file is gone or empty, but this process knows of enrolled projects",
                    ),
                })
            }
            Ok(projects) => {
                self.projects = projects;
                Ok(())
            }
            Err(source) => Err(EnrolError::Io {
                what: "the registry".to_owned(),
                source,
            }),
        }
    }

    /// Read the registry, or start empty.
    ///
    /// A file that cannot be parsed starts empty **and says so loudly**. Refusing to boot on a
    /// corrupt registry would take the whole fleet's channel down over one bad byte; starting empty
    /// refuses every project instead, which is fail-closed, visible in the log, and recoverable by
    /// re-enrolling. Neither is good, and the loud line is what stops the bad one being silent.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        match Self::try_load(&path) {
            Ok(registry) => registry,
            Err(e) => {
                tracing::error!(
                    error = %e, path = %path.display(),
                    "the project registry could not be read, so NO project can connect until it is \
                     re-enrolled. The file has been left exactly as it is."
                );
                Self {
                    path,
                    projects: BTreeMap::new(),
                }
            }
        }
    }

    /// Read the registry, or say why it cannot be read.
    ///
    /// For a reader that must not guess. `load`'s start-empty is right for the hub's boot and wrong
    /// for the inventory another org's dispatcher reads: there, a corrupt or unreadable file came
    /// out as `[]` with a clean exit, which a machine capturing stdout reads as "no project exists
    /// and no topic exists". A file that is not there is still an empty registry — nothing has been
    /// enrolled yet — because that is what it means.
    pub fn try_load(path: impl Into<PathBuf>) -> Result<Self, EnrolError> {
        let path = path.into();
        match read_projects(&path) {
            Ok(projects) => Ok(Self { path, projects }),
            Err(source) => Err(EnrolError::Unreadable {
                why: source.to_string(),
                path,
            }),
        }
    }

    /// Resolve a secret to the project it belongs to.
    ///
    /// **Every enrolled project is compared, and the comparison does not stop early.** Returning as
    /// soon as a byte differs would make the time this takes depend on how much of a guess was
    /// right, which is a slow but real way to learn a token one byte at a time. `subtle` is here
    /// for exactly that, and the loop deliberately does not `break`.
    ///
    /// The `project_id` a bridge sends is not consulted. The token is the whole of the identity —
    /// a bridge that names another project still lands on its own.
    pub fn resolve(&self, token: &str) -> Option<&Project> {
        let want = sha256_hex(token.as_bytes());
        let mut found: Option<&Project> = None;
        for p in self.projects.values() {
            let hit = p.token_sha256.as_bytes().ct_eq(want.as_bytes());
            if bool::from(hit) {
                found = Some(p);
            }
        }
        found
    }

    /// A project by id, for the paths that already know which one they mean.
    pub fn get(&self, id: &ProjectId) -> Option<&Project> {
        self.projects.get(id)
    }

    /// Every project, for `herdr-tg projects` and for the digest.
    pub fn all(&self) -> impl Iterator<Item = &Project> {
        self.projects.values()
    }

    /// The project a conversation belongs to: a seed is its own, and a room's is the row whose
    /// folder it was granted in.
    ///
    /// This is the one place that relation is worked out, so that nothing else has to. A room has
    /// no folder of its own — `grant` mints it in the seed's repo — so the shared path is the only
    /// thing on file that ties the two rows together, and everybody who needed the relation was
    /// joining on that path themselves. A path is a fact about this machine; the id is the name.
    ///
    /// It falls back to the ROOM ITSELF when NO seed row holds that folder, rather than guessing
    /// at the nearest one. That state is reachable — a hand-edited file, a seed row removed while
    /// its rooms stayed — and the two other answers are both worse: picking whichever row is
    /// nearest would relate one org's conversation to another org's project, and answering with
    /// nothing reads as "this is a seed", which it is not. A room that stands alone stands for
    /// itself until a terminal relates it again.
    ///
    /// **What this does NOT promise, said plainly because the fallback reads like it does.** The
    /// answer is only ever as good as the shared folder, and the folder can lie in two ways that
    /// no fallback here can see. A seed re-enrolled at a MOVED folder leaves its old row on file,
    /// so its rooms match THAT and this answers with a live id for a checkout nobody works in —
    /// the fallback never fires, because a row does hold the folder. And a project enrolled at a
    /// folder another one vacated mints the same id from the same path, takes the old row over,
    /// and inherits the first project's rooms along with its topic. Both are pinned as tests in
    /// `cmd/projects.rs`, and both are the same debt: a room's row does not store the seed it was
    /// granted from, and a seed's id is a hash of its path (see `seed_row_for`). The migration
    /// that ends them is a stored `seed` on every room row, written at `grant`; until it lands,
    /// this relation is honest exactly while a project stays where it was enrolled.
    /// `docs/CAPABILITIES.md` OPEN 3 carries it, and both documents describe the behaviour above
    /// rather than the one the fallback suggests.
    pub fn seed_of<'a>(&'a self, id: &'a ProjectId) -> &'a ProjectId {
        if !is_room(id) {
            return id;
        }
        let Some(room) = self.projects.get(id) else {
            return id;
        };
        self.projects
            .values()
            .find(|p| !is_room(&p.id) && p.repo == room.repo)
            .map(|p| &p.id)
            .unwrap_or(id)
    }

    /// Which topic a conversation's messages already go to, if it has one.
    ///
    /// A conversation, not a project: a lane's topic is its own, and answering with the project's
    /// would put a worktree's rolling context in the project's own topic — the thing a topic per
    /// lane exists to stop.
    pub fn topic_of(&self, addr: &Addr) -> Option<i32> {
        let p = self.projects.get(&addr.project)?;
        match &addr.lane {
            None => p.topic_id,
            Some(lane) => p.lane_topics.get(lane).copied(),
        }
    }

    /// The lowest topic number anything here is bound to — every conversation's own and every
    /// lane's — or nothing when no conversation has one yet.
    ///
    /// For a carrier that has no forum to ask and mints its own numbers downward instead. That
    /// carrier holds its counter in memory, so without a floor read back off this file at start a
    /// restart would hand the next brand-new conversation a number an older one is still bound
    /// to — and two conversations on one number share every question written down against it.
    ///
    /// Across ALL of them and not just the project being asked about, because the numbers are a
    /// box's, not a project's: a second conversation minting from a floor it read off its own row
    /// alone would start again where the first already is.
    pub fn lowest_topic_bound(&self) -> Option<i32> {
        self.projects
            .values()
            .flat_map(|p| {
                p.topic_id
                    .into_iter()
                    .chain(p.lane_topics.values().copied())
            })
            .min()
    }

    /// Remember which topic a conversation's messages go to.
    pub fn bind_topic(&mut self, addr: &Addr, topic_id: i32) -> Result<(), EnrolError> {
        // Read-modify-write under the lock, never write-what-I-remember. Writing the in-memory map
        // would put this process's snapshot back over anything enrolled at the terminal since it
        // started — and a read that FAILED must abandon the write rather than write what it could
        // not read.
        let _held = self.hold()?;
        self.reread()?;
        if let Some(p) = self.projects.get_mut(&addr.project) {
            match &addr.lane {
                None => p.topic_id = Some(topic_id),
                Some(lane) => {
                    p.lane_topics.insert(lane.clone(), topic_id);
                }
            }
        }
        self.save()
    }

    /// Forget a topic that Telegram says is gone, so the next send creates a new one.
    ///
    /// Deleting a forum topic emits no service message and there is no way to list topics, so the
    /// hub learns about it from `message thread not found` on a send. That is a rebinding, handled
    /// once and deliberately — never retried as if it were a transient network error, which would
    /// swallow the project's messages forever.
    pub fn unbind_topic(&mut self, addr: &Addr) -> Result<(), EnrolError> {
        let _held = self.hold()?;
        self.reread()?;
        if let Some(p) = self.projects.get_mut(&addr.project) {
            match &addr.lane {
                None => p.topic_id = None,
                Some(lane) => {
                    p.lane_topics.remove(lane);
                }
            }
        }
        self.save()
    }

    /// Switch a project off, or back on, saying everything it switched. Terminal-only, like
    /// everything else that changes who may connect: no message and no frame reaches this.
    ///
    /// The project is found by its repo path, canonicalised when the folder still exists and taken
    /// as written when it does not — a project whose tree has been deleted is exactly the one worth
    /// switching off, and refusing because the folder is gone would leave it the only project on
    /// the box that cannot be.
    ///
    /// By repo path it is the SEED AND EVERY ROOM of it. Rooms live in their seed's repo, and
    /// "I switched it off" has to mean the project: the version that reached the seed alone left
    /// N rooms holding their claims and delivering, each switchable only by an id the lists hide
    /// while it has no topic, and nothing he read said so. A room's own id switches that room
    /// and nothing else — one function of the business, off, while the rest of the organisation
    /// talks. Everything switched is returned so the terminal can say it.
    ///
    /// Writing the flag is only half of what "off" means. The hub reads `enabled` at `hello`, so
    /// this alone turns away the NEXT connection and does nothing to one already on the socket. The
    /// other half — dropping a live connection — is the hub's, which watches this file for exactly
    /// that. See `Hub::drop_connections_of_switched_off_projects`.
    pub fn switch(&mut self, which: &Path, enabled: bool) -> Result<Switched, EnrolError> {
        let _held = self.hold()?;
        self.reread().map_err(|e| EnrolError::Unreadable {
            path: self.path.clone(),
            why: e.to_string(),
        })?;
        let Some(id) = self.select(which) else {
            return Err(EnrolError::NotEnrolled {
                repo: which.to_path_buf(),
            });
        };
        let named = {
            let p = self.projects.get_mut(&id).expect("selected from this map");
            p.enabled = enabled;
            p.clone()
        };
        let mut rooms = Vec::new();
        if !is_room(&named.id) {
            for p in self.projects.values_mut() {
                if is_room(&p.id) && p.repo == named.repo {
                    p.enabled = enabled;
                    rooms.push(p.clone());
                }
            }
        }
        self.save()?;
        Ok(Switched { named, rooms })
    }

    /// Whether `enrol` would write the repo's own copy of the secret for this folder.
    ///
    /// A rotation rewrites the places the secret already lives and no new one. A folder nobody
    /// has enrolled gets both — the repo's copy is what a bridge from before conversations
    /// existed reads — and so does a project enrolled the older way whose repo still holds its
    /// copy. A project opened with `open`, or one whose repo copy has been taken away, holds its
    /// secret where the channel keeps it and nowhere else, and a rotation keeps it that way:
    /// putting the file back into the tree would undo the one thing `open` is for, on the day a
    /// leaked secret made rotating it matter. The git guard at the door asks this first, so a
    /// rotation that writes nothing into the repo is not refused for what git would commit.
    pub fn would_write_repo_copy(&self, repo: &Path) -> bool {
        let Ok(canonical) = repo.canonicalize() else {
            return true;
        };
        let Some(existing) = self.projects.values().find(|p| p.repo == canonical) else {
            return true;
        };
        if existing.repo.join(TOKEN_FILE).exists() {
            return true;
        }
        !matches!(self.home().read_secret(&existing.id), Ok(Some(_)))
    }

    /// Let a person speak in this project's conversations, or stop them. Terminal-only, like the
    /// switch: no message, no tap and no frame reaches this, and `nothing_inbound_can_add_a_person`
    /// fails the build if the bot or the hub ever names it.
    ///
    /// The same read-modify-write as `switch`, found by the same path rule — the seed alone,
    /// because a room's people are its own and never inherited — and live for the
    /// running hub within about a second — it watches this file and re-reads it on change, and it
    /// answers "may this person speak here" from the copy it holds. No restart.
    ///
    /// Only a person: a group's id is refused with the reason. See `EnrolError::NotAPerson`.
    pub fn set_may_speak(
        &mut self,
        repo: &Path,
        user: i64,
        may: bool,
    ) -> Result<Project, EnrolError> {
        if !crate::config::is_a_persons_id(user) {
            return Err(EnrolError::NotAPerson { given: user });
        }
        let _held = self.hold()?;
        self.reread().map_err(|e| EnrolError::Unreadable {
            path: self.path.clone(),
            why: e.to_string(),
        })?;
        let Some(id) = self.select(repo) else {
            return Err(EnrolError::NotEnrolled {
                repo: repo.to_path_buf(),
            });
        };
        let p = self.projects.get_mut(&id).expect("selected from this map");
        if may {
            p.allowed_users.insert(user);
        } else {
            p.allowed_users.remove(&user);
        }
        let project = p.clone();
        self.save()?;
        Ok(project)
    }

    /// Where this registry lives, for the one reader that has to notice it changing.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Which row a terminal verb means by `which`.
    ///
    /// A conversation id names that row outright — it is how a room is reached, since a room has
    /// no folder of its own. Anything else is a folder and names the SEED enrolled there, never
    /// one of its rooms: rooms share their seed's repo, and the first row in id order is a `c-`
    /// row whenever there is one, so `herdr-tg disable <repo>` matched by path alone would have
    /// switched off a room and left the seed talking.
    ///
    /// Canonicalised when the folder still exists and taken as written when it does not — a project
    /// whose tree has been deleted is exactly the one worth switching off. An id is read as an id
    /// only when no folder of that name is there to be meant instead.
    fn select(&self, which: &Path) -> Option<ProjectId> {
        if let Some(s) = which.to_str()
            && crate::conversations::is_conversation_id(s)
            && !which.is_dir()
        {
            let id = ProjectId::new(s);
            return self.projects.contains_key(&id).then_some(id);
        }
        let wanted = which.canonicalize().unwrap_or_else(|_| which.to_path_buf());
        self.projects
            .values()
            .find(|p| p.repo == wanted && !is_room(&p.id))
            .map(|p| p.id.clone())
    }

    /// The channel home beside this registry. One home: the state directory the registry lives in
    /// is where every conversation's secret lives too.
    fn home(&self) -> crate::conversations::ChannelHome {
        crate::conversations::ChannelHome::at(self.path.parent().unwrap_or(Path::new(".")))
    }

    /// The canonical folder and the id a seed enrolled there has — or would have — after the one
    /// check both doors make: a folder BELOW an enrolled one is refused.
    ///
    /// Both paths are canonical, so this is a real containment test and not a string prefix that
    /// would call `/srv/app2` a child of `/srv/app`. Re-enrolling the same folder is a rotation
    /// and stays allowed; only a folder below an enrolled one is refused.
    fn seed_row_for(&self, repo: &Path) -> Result<(PathBuf, ProjectId), EnrolError> {
        let repo = repo.canonicalize().map_err(|_| EnrolError::NoSuchRepo {
            repo: repo.to_path_buf(),
        })?;
        if !repo.is_dir() {
            return Err(EnrolError::NoSuchRepo { repo });
        }
        // Never a room: rooms share their seed's repo and a `c-` id sorts before a `p-` one, so
        // the first match was a vacant room hidden from every list, and the refusal told him his
        // folder was inside a project he had never seen. The seed is always there when its rooms
        // are, so the check loses nothing.
        if let Some(parent) = self
            .projects
            .values()
            .find(|p| !is_room(&p.id) && repo != p.repo && repo.starts_with(&p.repo))
        {
            return Err(EnrolError::InsideAnotherProject {
                repo,
                parent: parent.repo.clone(),
                title: parent.title.clone(),
            });
        }
        // Minted from the canonical path, never from a counter. A recycled counter silently
        // inheriting a dead agent's topic is a defect this repo has already shipped once.
        //
        // That the hash is OF the path is an implementation detail and not a promise: nothing may
        // derive an id from a folder, and nothing may read a folder back out of one. It stays a
        // hash because it is what makes a re-enrolment of the same folder keep its topic and its
        // history, which is the whole point of a stable id. The debt it leaves is real and named
        // here rather than pretended away: a seed enrolled at a MOVED folder gets a new id, and
        // its rooms — which relate to it by that folder, see `seed_of` — are left pointing at the
        // row it used to be. Random minting plus a `seed` stored on every room row at `grant` is
        // the migration that ends it, and it has not been done.
        let id = ProjectId::new(format!(
            "p-{}",
            &sha256_hex(repo.as_os_str().as_encoded_bytes())[..12]
        ));
        Ok((repo, id))
    }

    /// A seed's row for a fresh secret, carrying forward everything a re-enrolment keeps.
    fn mint_seed(&self, repo: PathBuf, id: ProjectId, secret: &str) -> Project {
        let title = self.unique_title(&repo, &id);
        let icon_color = colour_of(&id);
        let before = self.projects.get(&id);
        Project {
            id: id.clone(),
            title,
            repo,
            token_sha256: sha256_hex(secret.as_bytes()),
            // A re-enrolment keeps the switch where the operator left it. This was `true`
            // outright, so rotating a leaked secret — the documented reason to re-run `enroll` —
            // silently switched a project he had turned off back on, and nothing in the output
            // said so.
            enabled: before.is_none_or(|p| p.enabled),
            // A re-enrolment keeps the topic. The whole point of a stable id is that history
            // survives.
            topic_id: before.and_then(|p| p.topic_id),
            icon_color,
            // And its lanes' topics, for the same reason it keeps its own: rotating a secret must
            // not scatter a day's worktrees into a second set of topics beside the first.
            lane_topics: before.map(|p| p.lane_topics.clone()).unwrap_or_default(),
            // And the project's people: each of them was let in by a decision at a keyboard, and
            // rotating a secret is not a decision about who may speak. Dropped here, a rotation
            // would silently shut a room's own people out — and nothing in the output would say so.
            allowed_users: before.map(|p| p.allowed_users.clone()).unwrap_or_default(),
        }
    }

    /// Enrol a repo, or rotate an already-enrolled one's secret.
    ///
    /// Terminal-only. Returns the secret exactly once, because it is never stored anywhere this
    /// process can read it back.
    ///
    /// The secret lands where the channel keeps it, always — and in the repo too, where a bridge
    /// from before conversations existed still looks, whenever the repo already holds a copy or
    /// nothing has enrolled this folder before. Both, because a bridge may read either — the
    /// channel's copy is what the ladder finds first, the repo's is what the bridge in the
    /// operator's own running session reads on every redial — and a rotation that rewrote only
    /// one would leave the other presenting bytes the registry no longer knows: `bad_token`,
    /// permanent, on an honest bridge. A project that holds no repo copy — opened with `open`, or
    /// one whose copy was taken away — is rotated where the channel keeps it and nowhere else, so
    /// a rotation never puts a token back into a tree (`would_write_repo_copy`).
    pub fn enrol(&mut self, repo: &Path) -> Result<(Project, String), EnrolError> {
        // Another process may have written since this handle was made — including the hub, which
        // binds topics while it runs. Enrolling must not undo that, and must not proceed at all if
        // the existing registry cannot be read: enrolling over a file we could not parse would
        // replace every other project with this one.
        let _held = self.hold()?;
        self.reread().map_err(|e| EnrolError::Unreadable {
            path: self.path.clone(),
            why: e.to_string(),
        })?;
        let (repo, id) = self.seed_row_for(repo)?;
        let repo_copy_too = self.would_write_repo_copy(&repo);
        let secret = fresh_secret()?;
        let project = self.mint_seed(repo.clone(), id.clone(), &secret);

        // THE SECRET GOES DOWN FIRST, and the list of projects second. The other order took a
        // project off the air whenever the second step failed: the registry already held the hash
        // of a secret that had never been written, the repo still held the old one, and the old one
        // no longer resolved — so the bridge was refused on every reconnect and nothing on the box
        // said why. A full disk, a read-only mount, or a `.kickoff` left root-owned by one sudo
        // run is all it takes.
        //
        // With two places, the same discipline needs one more step. The link goes first, because
        // it names only the id and changes no secret. Then the channel's copy, then the repo's —
        // and a repo write that fails puts the channel's copy BACK to what it was, so that the
        // failure still changes nothing at all: no hash is saved, both files hold the bytes they
        // held, and the secret already in use goes on working.
        let home = self.home();
        let channel = |what: &str| {
            let what = format!("{what} where the channel keeps it");
            move |source| EnrolError::Io {
                what: what.clone(),
                source,
            }
        };
        home.link_repo(&repo, &id)
            .map_err(channel("the project's link"))?;
        let before = home
            .read_secret(&id)
            .map_err(channel("the project's secret"))?;
        home.write_secret(&id, &secret)
            .map_err(channel("the project's secret"))?;
        if repo_copy_too && let Err(e) = write_token_file(&repo, &secret) {
            let put_back = match &before {
                Some(old) => home.write_secret(&id, old).map(|_| ()),
                None => home.remove_secret(&id),
            };
            return Err(match put_back {
                Ok(()) => e,
                // The one shape that leaves the two places disagreeing. Said in as many words,
                // because nothing else on the box would ever explain it.
                Err(r) => EnrolError::Io {
                    what: format!(
                        "the project's token file — and the channel's copy of the secret could not \
                         be put back afterwards ({r}), so the two copies now differ; run this again"
                    ),
                    source: match e {
                        EnrolError::Io { source, .. } => source,
                        other => std::io::Error::other(other.to_string()),
                    },
                },
            });
        }
        self.projects.insert(id, project.clone());
        // Reached only when the secret IS on disk, so this failure is the one direction that can
        // still leave a project unable to connect. It says so rather than reporting a write error,
        // because the operator has to know to run the command again.
        self.save().map_err(|e| EnrolError::HalfWritten {
            repo: repo.clone(),
            why: e.to_string(),
        })?;
        Ok((project, secret))
    }

    /// Open a repo as a conversation, writing nothing into it.
    ///
    /// The row is the one `enrol` would mint — same id, from the same canonical path — so the two
    /// doors agree about which project a repo is; only where the secret lands differs. Opening a
    /// conversation that is already open changes nothing and says so: rotation is `enrol`'s job,
    /// and a door that rotated on a second visit would take a live bridge off the air. A repo
    /// enrolled the old way, with no copy of its secret in the channel, is refused and told the
    /// verb that copies it across — and so is one whose channel copy is there but STALE, because
    /// "already open" over bytes the hub would refuse is a lie every new session in that repo
    /// pays for: it presents the stale copy and is turned away for good.
    pub fn open(&mut self, repo: &Path) -> Result<(Project, bool), EnrolError> {
        let _held = self.hold()?;
        self.reread().map_err(|e| EnrolError::Unreadable {
            path: self.path.clone(),
            why: e.to_string(),
        })?;
        let (repo, id) = self.seed_row_for(repo)?;
        let home = self.home();
        let channel = |what: &str| {
            let what = format!("{what} where the channel keeps it");
            move |source| EnrolError::Io {
                what: what.clone(),
                source,
            }
        };
        if let Some(existing) = self.projects.get(&id) {
            let existing = existing.clone();
            let Some(channel_copy) = home
                .read_secret(&id)
                .map_err(channel("the project's secret"))?
            else {
                return Err(EnrolError::EnrolledTheOldWay { repo });
            };
            // Hashed, not merely found. A rotation typed with a build from before conversations
            // existed rewrites the repo's copy and the hash and knows nothing of the channel's,
            // and the copy it leaves behind is the one every new bridge presents first.
            if sha256_hex(channel_copy.trim().as_bytes()) != existing.token_sha256 {
                let repo_is_current = fs::read_to_string(repo.join(TOKEN_FILE))
                    .is_ok_and(|s| sha256_hex(s.trim().as_bytes()) == existing.token_sha256);
                return Err(EnrolError::ChannelCopyStale {
                    repo,
                    repo_is_current,
                });
            }
            // The link is re-asserted, so a home whose link went missing is mended by the one
            // verb somebody would reach for.
            home.link_repo(&repo, &id)
                .map_err(channel("the project's link"))?;
            return Ok((existing, false));
        }
        let secret = fresh_secret()?;
        let project = self.mint_seed(repo.clone(), id.clone(), &secret);
        // Secret before registry, the same order `enrol` keeps and for the same reason.
        home.link_repo(&repo, &id)
            .map_err(channel("the project's link"))?;
        home.write_secret(&id, &secret)
            .map_err(channel("the project's secret"))?;
        self.projects.insert(id, project.clone());
        self.save().map_err(|e| EnrolError::HalfOpened {
            repo: repo.clone(),
            why: e.to_string(),
        })?;
        Ok((project, true))
    }

    /// Mint `rooms` rooms for a seed: complete conversations, each with a row, a secret where the
    /// channel keeps it, and a vacant slot in the seed's book. Nothing is written into any repo.
    ///
    /// A room is a sibling of its seed — its own claim, its own topic, its own people — in the
    /// seed's repo, because it has no folder of its own. Its id is random: there is no path to
    /// derive one from, and a counter is the shape that once recycled a dead agent's topic. The
    /// book is bounded so that a leaked grant directory is worth a known number of rooms and no
    /// more; a taken slot is spent, never recycled, and refilling mints fresh numbers.
    pub fn grant(&mut self, seed: &Path, rooms: usize) -> Result<Vec<Project>, EnrolError> {
        let _held = self.hold()?;
        self.reread().map_err(|e| EnrolError::Unreadable {
            path: self.path.clone(),
            why: e.to_string(),
        })?;
        let seed = match self.select(seed) {
            Some(id) if is_room(&id) => return Err(EnrolError::NotASeed { id }),
            Some(id) => self
                .projects
                .get(&id)
                .cloned()
                .expect("selected from this map"),
            None => {
                return Err(EnrolError::NotEnrolled {
                    repo: seed.to_path_buf(),
                });
            }
        };
        let home = self.home();
        let channel = |what: &str| {
            let what = format!("{what} where the channel keeps it");
            move |source| EnrolError::Io {
                what: what.clone(),
                source,
            }
        };
        // A vacant slot naming a room the list has never heard of is a page nothing can use: the
        // hub admits on the hash, and there is none. Left by a grant whose save failed, it held a
        // page of the book and could be TAKEN — a dispatcher then started an engine whose hello
        // nobody would admit. Taken back here, under the lock, before the book is counted; a slot
        // already taken is spent whatever became of its row, because its number may name a topic.
        for slot in home
            .slots(&seed.id)
            .map_err(channel("the project's book"))?
        {
            if !slot.taken && !self.projects.contains_key(&slot.room) {
                home.remove_slot(&seed.id, slot.number)
                    .map_err(channel("the project's book"))?;
                home.remove_conversation(&slot.room)
                    .map_err(channel("the room's secret"))?;
            }
        }
        let vacant = home
            .slots(&seed.id)
            .map_err(channel("the project's book"))?
            .iter()
            .filter(|s| !s.taken)
            .count();
        if vacant + rooms > crate::conversations::BOOK {
            return Err(EnrolError::BookFull {
                title: seed.title.clone(),
                vacant,
                asked: rooms,
            });
        }
        let mut minted: Vec<Project> = Vec::with_capacity(rooms);
        let mut slots: Vec<u32> = Vec::with_capacity(rooms);
        // Everything this call put on disk, taken back again — for a save that fails, so the
        // failure changes nothing: no row, no slot, no credential nobody minted a row for.
        let unwind = |minted: &[Project], slots: &[u32]| -> Result<(), std::io::Error> {
            for n in slots {
                home.remove_slot(&seed.id, *n)?;
            }
            for p in minted {
                home.remove_conversation(&p.id)?;
            }
            Ok(())
        };
        for _ in 0..rooms {
            let id = loop {
                let id = crate::conversations::mint_room_id().ok_or(EnrolError::NoRandomness)?;
                if !self.projects.contains_key(&id) && !minted.iter().any(|p| p.id == id) {
                    break id;
                }
            };
            let secret = fresh_secret()?;
            // Secret and slot first, row second — the same order as everywhere else. A slot is
            // written only once the secret is, so a book never names a room nothing can be.
            if let Err(e) = home.write_secret(&id, &secret) {
                let _ = unwind(&minted, &slots);
                return Err(channel("the room's secret")(e));
            }
            let slot = match home.add_slot(&seed.id, &id) {
                Ok(slot) => slot,
                Err(e) => {
                    let _ = home.remove_conversation(&id);
                    let _ = unwind(&minted, &slots);
                    return Err(channel("the project's book")(e));
                }
            };
            slots.push(slot.number);
            minted.push(Project {
                id,
                // Told from its seed by its slot number, until whoever takes it writes a title.
                // Slot numbers are never reused, so this cannot collide with another room's.
                title: room_title(&seed.title, slot.number),
                repo: seed.repo.clone(),
                token_sha256: sha256_hex(secret.as_bytes()),
                enabled: true,
                topic_id: None,
                // The seed's colour, so an organisation's conversations read as one block in the
                // forum list — the same reason a lane takes its project's.
                icon_color: seed.icon_color,
                lane_topics: BTreeMap::new(),
                // A room's people are its own, let in one by one; the seed's are not inherited.
                allowed_users: BTreeSet::new(),
            });
        }
        for p in &minted {
            self.projects.insert(p.id.clone(), p.clone());
        }
        if let Err(e) = self.save() {
            for p in &minted {
                self.projects.remove(&p.id);
            }
            let why = match unwind(&minted, &slots) {
                Ok(()) => e.to_string(),
                // The one shape that leaves something behind. Said in as many words, because a
                // secret with no row is a credential nothing on the box would ever explain.
                Err(r) => format!(
                    "{e} — and taking the rooms' secrets and slots back failed too ({r}), so \
                     some may still be on disk under the channel's home; the next grant takes \
                     back any slot naming a room the list does not hold"
                ),
            };
            return Err(EnrolError::HalfGranted {
                title: seed.title.clone(),
                why,
            });
        }
        Ok(minted)
    }

    /// Repo basename, sanitised and clipped; a 6-hex suffix if another path already took it.
    fn unique_title(&self, repo: &Path, id: &ProjectId) -> String {
        let base: String = repo
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_owned())
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            .take(MAX_TITLE)
            .collect();
        let base = if base.is_empty() {
            "project".to_owned()
        } else {
            base
        };

        let taken = self
            .projects
            .values()
            .any(|p| p.title == base && &p.id != id);
        if !taken {
            return base;
        }
        // Two different repos with the same folder name. The suffix comes from the id, so it is
        // the same every time rather than depending on who enrolled first.
        let suffix = &sha256_hex(id.as_str().as_bytes())[..6];
        let room = MAX_TITLE.saturating_sub(suffix.len() + 1);
        format!("{}-{suffix}", &base[..base.len().min(room)])
    }

    /// Hold the registry's lock for a read-modify-write.
    ///
    /// Two processes write this file — the running hub, binding topics, and `herdr-tg enroll` at a
    /// terminal — and temp-and-rename is only atomic against a reader, not against another
    /// read-modify-write. Without this an enrolment the operator was told had succeeded could be
    /// overwritten by a topic binding that had read the file a moment earlier.
    ///
    /// Blocking, deliberately. The hold is one small read and one rename, and a lock that could
    /// fail would put the caller straight back into the race it was taken to prevent.
    pub(crate) fn hold(&self) -> Result<fs::File, EnrolError> {
        let path = self.path.with_extension("lock");
        let io = |source| EnrolError::Io {
            what: "the registry lock".to_owned(),
            source,
        };
        if let Some(dir) = path.parent() {
            crate::conversations::private_state_dir(dir).map_err(io)?;
        }
        let f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(io)?;
        rustix::fs::flock(&f, rustix::fs::FlockOperation::LockExclusive)
            .map_err(|e| io(std::io::Error::from(e)))?;
        Ok(f)
    }

    /// Atomic temp-and-rename at 0600.
    ///
    /// The state must survive a kill at any instant: a half-written registry that replaced a good
    /// one would refuse every project on the next boot, and the operator would have no way to tell
    /// that from a token problem.
    fn save(&self) -> Result<(), EnrolError> {
        let io = |what: &str| {
            let what = what.to_owned();
            move |source| EnrolError::Io {
                what: what.clone(),
                source,
            }
        };
        if let Some(dir) = self.path.parent() {
            crate::conversations::private_state_dir(dir).map_err(io("the state directory"))?;
        }
        // A temp name per process. Both writers — the running hub and `herdr-tg enroll` at a
        // terminal — used the SAME `projects.json.tmp`, so two saves could interleave inside one
        // file and the rename that followed published whatever the loser had written.
        let tmp = self
            .path
            .with_extension(format!("json.tmp.{}", std::process::id()));
        let body = serde_json::to_vec_pretty(&self.projects).map_err(|e| EnrolError::Io {
            what: "the registry".into(),
            source: e.into(),
        })?;
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)
                .map_err(io("the registry"))?;
            f.write_all(&body).map_err(io("the registry"))?;
            f.flush().map_err(io("the registry"))?;
        }
        fs::rename(&tmp, &self.path).map_err(io("the registry"))
    }
}

/// Thirty-two random bytes as hex. Fail closed: a predictable token is worse than no token, and
/// quietly falling back to a weaker source is how that happens.
fn fresh_secret() -> Result<String, EnrolError> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|_| EnrolError::NoRandomness)?;
    Ok(hex(&secret))
}

/// One of Telegram's six topic colours, derived from the id so it is stable across restarts and
/// across machines.
fn colour_of(id: &ProjectId) -> u8 {
    (u8::from_str_radix(&sha256_hex(id.as_str().as_bytes())[..2], 16).unwrap_or(0)) % ICON_COLOURS
}

/// A room, as opposed to a seed: minted at random, sharing its seed's repo.
pub(crate) fn is_room(id: &ProjectId) -> bool {
    id.as_str().starts_with("c-")
}

/// What a room is called until whoever takes it writes a title: its seed and its slot number,
/// clipped to what a topic title takes.
fn room_title(seed: &str, slot: u32) -> String {
    let tail = format!("-room-{slot:03}");
    let room = MAX_TITLE.saturating_sub(tail.len());
    let head: String = seed.chars().take(room).collect();
    format!("{head}{tail}")
}

/// Write the project's own copy of its secret, readable by nobody else.
fn write_token_file(repo: &Path, secret: &str) -> Result<(), EnrolError> {
    let path = repo.join(TOKEN_FILE);
    let io = |what: &str| {
        let what = what.to_owned();
        move |source| EnrolError::Io {
            what: what.clone(),
            source,
        }
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io("the project's .kickoff directory"))?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .map_err(io("the project's token file"))?;
    f.write_all(secret.as_bytes())
        .map_err(io("the project's token file"))?;
    f.flush().map_err(io("the project's token file"))?;
    // Re-asserted rather than trusted: an existing file keeps its old mode through `.mode()`,
    // which only applies at creation. A token that was once 0644 would stay 0644 forever.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(io("the project's token file"))
}

/// Read the registry file. An ABSENT file is an empty registry; anything else is an error.
///
/// The distinction is the whole point. "Not there yet" and "there and unreadable" look identical
/// once both become an empty map, and treating the second as the first is how a registry gets
/// erased by the process that was meant to be protecting it.
fn read_projects(path: &Path) -> std::io::Result<BTreeMap<ProjectId, Project>> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(e),
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(d: &tempfile::TempDir) -> Registry {
        Registry::load(d.path().join("projects.json"))
    }

    fn repo(d: &tempfile::TempDir, name: &str) -> PathBuf {
        let p = d.path().join(name);
        fs::create_dir_all(&p).expect("make a repo");
        p
    }

    #[test]
    fn a_rotation_that_could_not_write_the_new_secret_leaves_the_old_one_still_working() {
        // The registry was saved with the NEW hash first, and the secret written second. When that
        // write failed the repo still held the OLD secret — which no longer resolved — and the new
        // one existed nowhere at all, so the project could never connect again. All the operator
        // was told was "could not write the project's token file", which reads like a command that
        // did nothing, and nothing anywhere else would ever explain why its bridge was refused.
        let d = tempfile::tempdir().expect("tmp");
        let repo = repo(&d, "herdr-tg");
        let mut r = reg(&d);
        let (_, old) = r.enrol(&repo).expect("enrols");

        // A token file this process cannot replace. This is one of the shapes the operator can
        // actually reach — alongside a full disk, a read-only mount, and a `.kickoff` left
        // root-owned by a sudo run — and it needs no exotic setup to arrange.
        let token = repo.join(TOKEN_FILE);
        fs::set_permissions(&token, fs::Permissions::from_mode(0o400)).expect("chmod");

        let failed = r.enrol(&repo).expect_err(
            "this test needs a rotation whose write fails, and the write went through — if this \
             is running as root, file permissions do not apply and this test cannot arrange it",
        );

        // Read back from disk, because rereading it is exactly what the hub does before it admits
        // a bridge.
        let after = Registry::load(d.path().join("projects.json"));
        let on_disk = fs::read_to_string(&token).expect("the old secret is still readable");
        assert_eq!(
            on_disk, old,
            "the failed rotation still replaced the secret in the repo"
        );
        assert!(
            after.resolve(&old).is_some(),
            "the failed rotation took the project down with it: the secret in its repo no longer \
             resolves, the new one was never written anywhere, and what the operator was told was \
             only: {failed}"
        );
    }

    #[test]
    fn an_enrolled_project_is_resolved_by_its_secret_and_nothing_else() {
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let (p, secret) = r.enrol(&repo(&d, "herdr-tg")).expect("enrols");
        let found = r.resolve(&secret).expect("the secret resolves");
        assert_eq!(found.id, p.id);
        assert!(r.resolve("not-the-secret").is_none());
    }

    #[test]
    fn the_secret_is_never_written_into_the_registry() {
        // A registry that leaks must tell an attacker which projects exist and nothing they can
        // connect with.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let (_, secret) = r.enrol(&repo(&d, "herdr-tg")).expect("enrols");
        let on_disk = fs::read_to_string(d.path().join("projects.json")).expect("readable");
        assert!(
            !on_disk.contains(&secret),
            "the secret itself is in the registry file"
        );
        assert!(on_disk.contains(&sha256_hex(secret.as_bytes())));
    }

    #[test]
    fn the_project_keeps_its_own_secret_at_six_hundred() {
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "herdr-tg");
        let (_, secret) = r.enrol(&repo).expect("enrols");
        let token_path = repo.join(TOKEN_FILE);
        assert_eq!(fs::read_to_string(&token_path).expect("readable"), secret);
        let mode = fs::metadata(&token_path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "mode {:o} lets others read a project's secret",
            mode & 0o777
        );
    }

    #[test]
    fn re_enrolling_rotates_the_secret_and_keeps_the_topic() {
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "herdr-tg");
        let (p1, s1) = r.enrol(&repo).expect("enrols");
        r.bind_topic(&Addr::project_itself(p1.id.clone()), 77)
            .expect("binds");
        let (p2, s2) = r.enrol(&repo).expect("re-enrols");

        assert_eq!(p1.id, p2.id, "a stable id is the whole point");
        assert_ne!(s1, s2, "re-enrolling must rotate the secret");
        assert!(r.resolve(&s1).is_none(), "the old secret still works");
        assert_eq!(
            p2.topic_id,
            Some(77),
            "the topic and its history must survive"
        );
    }

    #[test]
    fn re_enrolling_a_switched_off_project_does_not_switch_it_back_on() {
        // Re-running `enroll` is the documented answer to a leaked secret, and `enabled: true` was
        // hardcoded on that path — so rotating the secret of a project the operator had switched
        // off silently switched it back on, with nothing in the output saying so. Off is a decision
        // he made at a keyboard, and a rotation must carry it forward the way it carries the topic.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "herdr-tg");
        r.enrol(&repo).expect("enrols");
        let off = r.switch(&repo, false).expect("switches off").named;
        assert!(!off.enabled, "the setter did not switch it off");

        let (rotated, _) = r.enrol(&repo).expect("re-enrols");
        assert!(
            !rotated.enabled,
            "rotating the secret switched a switched-off project back on"
        );
        // And on disk, which is what the hub reads at the next hello.
        let after = Registry::load(d.path().join("projects.json"));
        assert!(
            !after.get(&rotated.id).expect("still enrolled").enabled,
            "the file says it is on again"
        );

        // Switching it back on is its own deliberate act, and it holds too.
        let on = r.switch(&repo, true).expect("switches on").named;
        assert!(on.enabled);
    }

    #[test]
    fn switching_a_project_nobody_enrolled_is_refused_in_plain_words_rather_than_enrolling_it() {
        // Switching on is not a way in. A command that quietly enrolled when asked to flip a switch
        // would be a second door beside the one that has the guard on it.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let said = r
            .switch(&repo(&d, "never-enrolled"), true)
            .expect_err("refused")
            .to_string();
        assert!(said.contains("nothing is enrolled"), "{said}");
        assert!(said.contains("herdr-tg projects"), "{said}");
        for jargon in ["Err", "NotEnrolled", "canonical", "None", "Option"] {
            assert!(
                !said.contains(jargon),
                "jargon reached the operator: {said}"
            );
        }
        assert_eq!(r.all().count(), 0, "a switch enrolled something");
    }

    #[test]
    fn a_project_whose_folder_is_gone_can_still_be_switched_off() {
        // The project whose tree was deleted is exactly the one worth switching off, and it is the
        // one a path check would refuse: its folder no longer canonicalises.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "gone-soon");
        let (p, _) = r.enrol(&repo).expect("enrols");
        fs::remove_dir_all(&repo).expect("delete the tree");
        let off = r
            .switch(&p.repo, false)
            .expect("a deleted folder can still be switched off")
            .named;
        assert!(!off.enabled);
    }

    #[test]
    fn the_id_comes_from_the_path_so_it_is_the_same_on_every_machine() {
        let d = tempfile::tempdir().expect("tmp");
        let repo = repo(&d, "herdr-tg");
        let mut a = Registry::load(d.path().join("a.json"));
        let mut b = Registry::load(d.path().join("b.json"));
        let (pa, _) = a.enrol(&repo).expect("enrols");
        let (pb, _) = b.enrol(&repo).expect("enrols");
        assert_eq!(pa.id, pb.id, "the id must not depend on enrolment order");
    }

    #[test]
    fn a_title_is_the_folder_name_with_nothing_a_topic_cannot_carry() {
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let (p, _) = r.enrol(&repo(&d, "herdr tg!!/../ok")).unwrap_or_else(|_| {
            // A path with a slash is a different directory, not a strange name; fall back to a
            // name that is strange but legal on disk.
            r.enrol(&repo(&d, "herdr tg!!")).expect("enrols")
        });
        assert!(
            p.title
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')),
            "a title reached Telegram with something in it we did not sanitise: {}",
            p.title
        );
        assert!(p.title.len() <= MAX_TITLE);
    }

    #[test]
    fn two_repos_with_the_same_folder_name_get_different_titles() {
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let one = repo(&d, "one/api");
        let two = repo(&d, "two/api");
        let (a, _) = r.enrol(&one).expect("enrols");
        let (b, _) = r.enrol(&two).expect("enrols");
        assert_ne!(a.id, b.id);
        assert_ne!(a.title, b.title, "two projects would share one topic name");
    }

    #[test]
    fn a_colour_is_one_of_the_six_telegram_allows() {
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        for n in 0..12 {
            let (p, _) = r.enrol(&repo(&d, &format!("proj{n}"))).expect("enrols");
            assert!(
                p.icon_color < ICON_COLOURS,
                "colour {} is not one of six",
                p.icon_color
            );
        }
    }

    #[test]
    fn a_registry_written_before_lanes_existed_still_loads_every_project() {
        // The file on the operator's box was written by a build that had never heard of a lane, and
        // an upgrade must not look like a corrupt file. The property below — a bad byte refuses
        // everyone rather than refusing to boot — is the RIGHT answer to a bad byte and the WRONG
        // one to an ordinary upgrade: every project on the box would be turned away, with the only
        // explanation on a log line nobody is reading.
        let d = tempfile::tempdir().expect("tmp");
        let path = d.path().join("projects.json");
        let secret = {
            let mut r = Registry::load(&path);
            let (_, s) = r.enrol(&repo(&d, "herdr-tg")).expect("enrols");
            s
        };
        // Exactly what yesterday's file looks like: every key this build writes except the one it
        // learned today. Removed as JSON rather than by deleting a line, because deleting the last
        // line of an object leaves a trailing comma — a CORRUPT file, which is the other property
        // entirely and would let this pass for the wrong reason.
        let mut doc: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("readable")).expect("json");
        for (_, project) in doc.as_object_mut().expect("an object").iter_mut() {
            project
                .as_object_mut()
                .expect("a project")
                .remove("lane_topics")
                .expect("this build wrote the key this test is here to remove");
        }
        let older = serde_json::to_string_pretty(&doc).expect("json");
        assert!(!older.contains("lane_topics"), "{older}");
        fs::write(&path, &older).expect("write");

        let r = Registry::load(&path);
        assert_eq!(r.all().count(), 1, "an upgrade un-enrolled every project");
        assert!(
            r.resolve(&secret).is_some(),
            "a registry written before lanes existed refuses the project it holds"
        );
    }

    #[test]
    fn a_corrupt_registry_refuses_everyone_rather_than_refusing_to_boot() {
        let d = tempfile::tempdir().expect("tmp");
        let path = d.path().join("projects.json");
        fs::write(&path, "{ this is not json").expect("write");
        let r = Registry::load(&path);
        assert!(r.all().next().is_none());
        assert!(r.resolve("anything").is_none());
        // The bad file is left alone: it is the only copy of what was enrolled, and overwriting it
        // would turn a recoverable morning into a lost one.
        assert_eq!(
            fs::read_to_string(&path).expect("still there"),
            "{ this is not json"
        );
    }

    #[test]
    fn a_registry_survives_a_round_trip_through_the_file() {
        let d = tempfile::tempdir().expect("tmp");
        let path = d.path().join("projects.json");
        let secret = {
            let mut r = Registry::load(&path);
            let (_, s) = r.enrol(&repo(&d, "herdr-tg")).expect("enrols");
            s
        };
        let r = Registry::load(&path);
        assert!(
            r.resolve(&secret).is_some(),
            "a restart lost every enrolment"
        );
    }

    #[test]
    fn the_registry_file_is_not_readable_by_anyone_else() {
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        r.enrol(&repo(&d, "herdr-tg")).expect("enrols");
        let mode = fs::metadata(d.path().join("projects.json"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "mode {:o}", mode & 0o777);
    }

    #[test]
    fn a_folder_inside_an_enrolled_project_cannot_become_a_second_project() {
        // A session started in `crates/` used to be told to enrol `crates/`, and it worked: one
        // repository became two projects with two chats, a question landed in whichever one the
        // session had started under, and a second secret was written a level down where the guard
        // that checks whether git would commit it does not look.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let top = repo(&d, "herdr-tg");
        r.enrol(&top).expect("enrols the project");
        let inside = repo(&d, "herdr-tg/crates");
        let said = r.enrol(&inside).expect_err("refused").to_string();
        assert!(said.contains("herdr-tg"), "{said}");
        assert!(said.contains("herdr-tg enroll"), "{said}");
        for jargon in ["Err", "InsideAnotherProject", "parent", "canonical", "None"] {
            assert!(
                !said.contains(jargon),
                "jargon reached the operator: {said}"
            );
        }
        // The top folder still rotates, because that is not the same folder.
        r.enrol(&top)
            .expect("re-enrolling the project itself still works");
        assert_eq!(r.all().count(), 1);
    }

    #[test]
    fn enrolling_something_that_is_not_a_directory_is_refused_in_plain_words() {
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let said = r
            .enrol(&d.path().join("nope"))
            .expect_err("refused")
            .to_string();
        assert!(said.contains("no directory"), "{said}");
        for jargon in ["Err", "NoSuchRepo", "canonicalize", "Option"] {
            assert!(
                !said.contains(jargon),
                "jargon reached the operator: {said}"
            );
        }
    }

    #[test]
    fn re_enrolling_keeps_a_projects_allowed_people() {
        // Re-running `enroll` is the documented answer to a leaked secret, and it carries the
        // switch and the topics forward for exactly this reason: each was a decision made at a
        // keyboard, and rotating a secret is not a decision about them. A room's people are the
        // same kind of decision. Dropped on rotation, every customer and teammate let into the
        // room would be shut out in silence, and nothing in the output would say so.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "herdr-tg");
        r.enrol(&repo).expect("enrols");
        const GUEST: i64 = 555_001;
        let with = r.set_may_speak(&repo, GUEST, true).expect("lets in");
        assert!(
            with.allowed_users.contains(&GUEST),
            "the setter did not let the guest in"
        );

        let (rotated, _) = r.enrol(&repo).expect("re-enrols");
        assert!(
            rotated.allowed_users.contains(&GUEST),
            "rotating the secret shut the project's own people out"
        );
        // And on disk, which is what the hub reads.
        let after = Registry::load(d.path().join("projects.json"));
        assert!(
            after
                .get(&rotated.id)
                .expect("still enrolled")
                .allowed_users
                .contains(&GUEST),
            "the file says the guest is gone"
        );

        // Shutting someone out is its own deliberate act, and it holds too.
        let without = r.set_may_speak(&repo, GUEST, false).expect("shuts out");
        assert!(!without.allowed_users.contains(&GUEST));
        let (rotated, _) = r.enrol(&repo).expect("re-enrols");
        assert!(
            rotated.allowed_users.is_empty(),
            "a rotation let a shut-out guest back in"
        );
    }

    #[test]
    fn a_registry_written_before_people_existed_still_loads_every_project() {
        // The same upgrade property `lane_topics` has, for the same file on the same box: a
        // registry that will not parse refuses every project, and an ordinary upgrade must not
        // look like a corrupt file.
        let d = tempfile::tempdir().expect("tmp");
        let path = d.path().join("projects.json");
        let secret = {
            let mut r = Registry::load(&path);
            let (_, s) = r.enrol(&repo(&d, "herdr-tg")).expect("enrols");
            s
        };
        let mut doc: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("readable")).expect("json");
        for (_, project) in doc.as_object_mut().expect("an object").iter_mut() {
            project
                .as_object_mut()
                .expect("a project")
                .remove("allowed_users")
                .expect("this build wrote the key this test is here to remove");
        }
        let older = serde_json::to_string_pretty(&doc).expect("json");
        fs::write(&path, &older).expect("write");

        let r = Registry::load(&path);
        assert_eq!(r.all().count(), 1, "an upgrade un-enrolled every project");
        let p = r.resolve(&secret).expect("the project still resolves");
        assert!(
            p.allowed_users.is_empty(),
            "a project from before people existed has people"
        );
    }

    #[test]
    fn only_a_person_can_be_let_into_a_project_never_a_group() {
        // A group's id on the list reads as "everyone in that group", which is the one grant this
        // list exists to make impossible — and it could never match a sender anyway, so the
        // operator would believe he had let people in and nobody would be. Refused with the
        // reason, in plain words, and nothing is written.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let here = repo(&d, "herdr-tg");
        r.enrol(&here).expect("enrols");
        for not_a_person in [0i64, -1009] {
            let said = r
                .set_may_speak(&here, not_a_person, true)
                .expect_err("a group was let in")
                .to_string();
            assert!(said.contains("not a person"), "{said}");
            for jargon in ["Err", "NotAPerson", "i64", "Option", "None"] {
                assert!(
                    !said.contains(jargon),
                    "jargon reached the operator: {said}"
                );
            }
        }
        let on_disk = Registry::load(d.path().join("projects.json"));
        assert!(
            on_disk.all().all(|p| p.allowed_users.is_empty()),
            "a refusal still wrote something"
        );

        // And a project nobody enrolled is a refusal in plain words, never an enrolment.
        let said = r
            .set_may_speak(&repo(&d, "never-enrolled"), 7, true)
            .expect_err("refused")
            .to_string();
        assert!(said.contains("nothing is enrolled"), "{said}");
        assert_eq!(r.all().count(), 1, "letting someone in enrolled something");
    }

    /// The channel home beside the registry, which is where a secret lives now.
    fn home(d: &tempfile::TempDir) -> crate::conversations::ChannelHome {
        crate::conversations::ChannelHome::at(d.path())
    }

    #[test]
    fn a_secret_minted_for_one_conversation_can_never_resolve_to_another() {
        // The RED test the design says to write first. There is no two-source `resolve` here to
        // get wrong — the channel holds bytes and the registry holds hashes — which is the point,
        // and this is what proves it rather than asserting it. The only failure that matters in
        // this area is the silent one: one agent's question in another agent's topic.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let seed_repo = repo(&d, "org");
        let (seed, _) = r.open(&seed_repo).expect("opens");
        let rooms = r.grant(&seed_repo, 3).expect("grants");
        let home = home(&d);
        let mut seen = std::collections::BTreeSet::new();
        for p in std::iter::once(&seed).chain(rooms.iter()) {
            let bytes = home.read_secret(&p.id).expect("reads").expect("a secret");
            assert!(
                seen.insert(bytes.clone()),
                "two conversations share one secret"
            );
            let resolved = r.resolve(&bytes).expect("resolves");
            assert_eq!(
                resolved.id, p.id,
                "{}'s secret resolved to {}",
                p.id, resolved.id
            );
        }
        // The repo's own link names the seed, never a room.
        assert_eq!(
            home.linked(&seed_repo).expect("reads"),
            Some(seed.id.clone())
        );
        // And on disk, which is what the hub reads at hello.
        let after = Registry::load(d.path().join("projects.json"));
        for room in &rooms {
            let bytes = home
                .read_secret(&room.id)
                .expect("reads")
                .expect("a secret");
            let resolved = after.resolve(&bytes).expect("resolves");
            assert_eq!(resolved.id, room.id);
            assert_ne!(resolved.id, seed.id, "a room's secret resolved to its seed");
        }
    }

    #[test]
    fn a_room_opened_at_a_terminal_gets_a_conversation_with_no_directory_of_its_own() {
        // A room is a SIBLING of its seed: its own row, its own secret, its own claim and topic —
        // and the seed's repo, because it has no directory of its own and needs none. Nothing is
        // written into that repo for it: not a token, not a folder.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let seed_repo = repo(&d, "org");
        let (seed, _) = r.open(&seed_repo).expect("opens");
        let rooms = r.grant(&seed_repo, 2).expect("grants");
        let home = home(&d);
        assert_eq!(rooms.len(), 2);
        for room in &rooms {
            assert!(
                crate::conversations::is_conversation_id(room.id.as_str())
                    && room.id.as_str().starts_with("c-"),
                "a room's id is not a conversation id: {}",
                room.id
            );
            assert_eq!(room.repo, seed.repo, "a room lives in its seed's repo");
            assert!(
                is_nothing_yet(room),
                "a room nobody has connected as is not nothing yet"
            );
            assert!(room.enabled && room.topic_id.is_none());
            assert!(home.read_secret(&room.id).expect("reads").is_some());
            assert_ne!(room.title, seed.title, "a room is named as its seed");
        }
        assert!(
            fs::read_dir(&seed_repo).expect("readable").next().is_none(),
            "granting rooms wrote something into the repo"
        );
        let slots = home.slots(&seed.id).expect("slots");
        assert_eq!(
            slots.iter().map(|s| s.room.clone()).collect::<Vec<_>>(),
            rooms.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
            "the book does not name the rooms in the order they were minted"
        );
        assert!(slots.iter().all(|s| !s.taken));
        // The rows are on disk, where the hub reads them, and the seed is untouched.
        let after = Registry::load(d.path().join("projects.json"));
        assert_eq!(after.all().count(), 3);
        assert_eq!(after.get(&seed.id).expect("the seed").title, seed.title);
    }

    #[test]
    fn the_book_holds_sixteen_slots_and_only_a_terminal_can_refill_it() {
        // Sixteen VACANT rooms at once, per seed: enough for a week of function proposals, and a
        // bound on what a leaked grant directory is worth. A taken slot is spent, not recycled —
        // its number is never reused, because the room it named may hold a topic and a history —
        // so refilling mints new numbers. The terminal half of the property is pinned in `cmd`.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let seed_repo = repo(&d, "org");
        let (seed, _) = r.open(&seed_repo).expect("opens");
        let full = r.grant(&seed_repo, 16).expect("a full book");
        assert_eq!(full.len(), 16);
        let said = r
            .grant(&seed_repo, 1)
            .expect_err("a seventeenth vacant room")
            .to_string();
        assert!(said.contains("16"), "{said}");
        for jargon in ["Err", "BOOK", "vacant:", "Option", "None"] {
            assert!(
                !said.contains(jargon),
                "jargon reached the operator: {said}"
            );
        }
        assert_eq!(
            r.all().count(),
            17,
            "a refused grant still minted something"
        );

        // A dispatcher takes a slot; the book has a page free again, and the new room gets a
        // number nobody has had.
        let home = home(&d);
        let book = home.grants().join(seed.id.as_str());
        fs::rename(book.join("000.vacant"), book.join("000.taken")).expect("take");
        let more = r.grant(&seed_repo, 1).expect("one page is free");
        assert_eq!(more.len(), 1);
        let slots = home.slots(&seed.id).expect("slots");
        assert_eq!(slots.len(), 17);
        assert_eq!(slots.last().expect("a slot").number, 16);
        assert_eq!(slots.iter().filter(|s| !s.taken).count(), 16);

        // Rooms are granted to a SEED. A path nobody enrolled is a refusal in plain words, and so
        // is a room's own id: a room does not get rooms.
        let said = r
            .grant(&repo(&d, "nobody"), 1)
            .expect_err("refused")
            .to_string();
        assert!(said.contains("nothing is enrolled"), "{said}");
        assert!(
            r.grant(Path::new(full[0].id.as_str()), 1).is_err(),
            "a room was granted rooms of its own"
        );
    }

    #[test]
    fn rotating_a_seed_rewrites_both_places_its_secret_lives() {
        // Once the channel holds a copy, a rotation that rewrote only the repo file would leave
        // the channel presenting stale bytes — `bad_token`, permanent, on the honest bridge that
        // reads the channel first. So `enrol` writes both places, and the link that lets a bridge
        // find the channel's copy at all.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "herdr-tg");
        let home = home(&d);
        let (p1, s1) = r.enrol(&repo).expect("enrols");
        assert_eq!(
            home.read_secret(&p1.id).expect("reads").as_deref(),
            Some(s1.as_str()),
            "enrolling did not put the secret where the channel keeps it"
        );
        assert_eq!(
            fs::read_to_string(repo.join(TOKEN_FILE)).expect("readable"),
            s1
        );
        assert_eq!(home.linked(&repo).expect("reads"), Some(p1.id.clone()));

        let (p2, s2) = r.enrol(&repo).expect("rotates");
        assert_eq!(p1.id, p2.id);
        assert_eq!(
            home.read_secret(&p2.id).expect("reads").as_deref(),
            Some(s2.as_str()),
            "the rotation left the channel's copy holding the old bytes"
        );
        assert_eq!(
            fs::read_to_string(repo.join(TOKEN_FILE)).expect("readable"),
            s2
        );
        assert!(r.resolve(&s2).is_some() && r.resolve(&s1).is_none());
    }

    #[test]
    fn a_rotation_that_could_not_write_the_repo_copy_leaves_both_places_as_they_were() {
        // The two-place write has two places to fail. The first copy written must be put back
        // when the second cannot be, or the channel holds bytes nothing resolves while the repo
        // holds bytes that do, and which one a bridge presents depends on which it reads first.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "herdr-tg");
        let home = home(&d);
        let (p, old) = r.enrol(&repo).expect("enrols");
        let token = repo.join(TOKEN_FILE);
        fs::set_permissions(&token, fs::Permissions::from_mode(0o400)).expect("chmod");
        r.enrol(&repo)
            .expect_err("this test needs a rotation whose repo write fails");
        assert_eq!(
            home.read_secret(&p.id).expect("reads").as_deref(),
            Some(old.as_str()),
            "the failed rotation left the channel's copy holding bytes nothing resolves"
        );
        assert_eq!(fs::read_to_string(&token).expect("readable"), old);
        assert!(
            Registry::load(d.path().join("projects.json"))
                .resolve(&old)
                .is_some()
        );
    }

    #[test]
    fn switching_a_project_by_repo_switches_its_seed_and_every_room_of_it_and_a_room_by_id_only_itself()
     {
        // Rooms share their seed's repo, and every verb once selected a row by repo path — the
        // first row in id order, which a `c-` id sorts BEFORE a `p-` one — so `herdr-tg disable
        // <repo>` switched off one room and left the seed talking. The fix that followed made the
        // path select the seed ALONE, and that narrowed "off" without saying so: N rooms in the
        // same repo kept their claims and kept delivering, each switchable only by an id the
        // lists hide while it has no topic. "I switched it off" has to mean the project: by repo,
        // the seed and every room of it; a room's own id switches that room and nothing else.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let seed_repo = repo(&d, "org");
        let (seed, _) = r.open(&seed_repo).expect("opens");
        let rooms = r.grant(&seed_repo, 2).expect("grants");

        let off = r.switch(&seed_repo, false).expect("switches off");
        assert_eq!(off.named.id, seed.id, "the switch landed on a room");
        assert!(!r.get(&seed.id).expect("the seed").enabled);
        assert!(
            rooms
                .iter()
                .all(|room| !r.get(&room.id).expect("the room").enabled),
            "a room of a switched-off project is still on the air"
        );
        // As sets: the rooms come back in id order, and ids are random.
        assert_eq!(
            off.rooms
                .iter()
                .map(|p| p.id.clone())
                .collect::<std::collections::BTreeSet<_>>(),
            rooms
                .iter()
                .map(|p| p.id.clone())
                .collect::<std::collections::BTreeSet<_>>(),
            "what was switched with the seed is not said"
        );
        // On disk, which is what the hub reads.
        let on_disk = Registry::load(d.path().join("projects.json"));
        assert!(
            on_disk.all().all(|p| !p.enabled),
            "the file still has a room on"
        );

        // Back on by repo: the seed and its rooms.
        let on = r.switch(&seed_repo, true).expect("switches on");
        assert_eq!(on.rooms.len(), 2);
        assert!(r.all().all(|p| p.enabled));

        // A room by its own id: that room, and nothing else.
        let off = r
            .switch(Path::new(rooms[0].id.as_str()), false)
            .expect("a room is switched by its id");
        assert_eq!(off.named.id, rooms[0].id);
        assert!(
            off.rooms.is_empty(),
            "switching a room switched something else"
        );
        assert!(r.get(&seed.id).expect("the seed").enabled);
        assert!(r.get(&rooms[1].id).expect("the other room").enabled);
        let off = r.switch(&seed_repo, false).expect("switches off");
        assert_eq!(off.named.id, seed.id);
        assert!(r.all().all(|p| !p.enabled));

        // A room's PEOPLE are its own — let in one by one, never inherited — so by repo a guest
        // reaches the seed alone.
        r.switch(&seed_repo, true).expect("switches on");
        const GUEST: i64 = 555_001;
        let with = r.set_may_speak(&seed_repo, GUEST, true).expect("lets in");
        assert_eq!(with.id, seed.id, "the guest was let into a room");
        assert!(
            rooms
                .iter()
                .all(|room| r.get(&room.id).expect("the room").allowed_users.is_empty())
        );
        let with = r
            .set_may_speak(Path::new(rooms[1].id.as_str()), GUEST, true)
            .expect("a room admits its own people");
        assert_eq!(with.id, rooms[1].id);
    }

    #[test]
    fn opening_a_repo_writes_nothing_into_it_and_opening_it_again_changes_nothing() {
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "herdr-tg");
        let home = home(&d);
        let (p, created) = r.open(&repo).expect("opens");
        assert!(created);
        assert!(
            !repo.join(".kickoff").exists(),
            "opening a repo wrote into it"
        );
        let first = home.read_secret(&p.id).expect("reads").expect("a secret");
        assert_eq!(home.linked(&repo).expect("reads"), Some(p.id.clone()));
        // The id is the one `enrol` mints for the same folder, so the two doors agree. Enrolled
        // into a registry with a home of its OWN, or that enrolment's channel copy would land on
        // this one's.
        let elsewhere = d.path().join("another-box");
        fs::create_dir_all(&elsewhere).expect("dir");
        let (by_enrol, _) = Registry::load(elsewhere.join("projects.json"))
            .enrol(&repo)
            .expect("enrols");
        assert_eq!(by_enrol.id, p.id);
        fs::remove_dir_all(repo.join(".kickoff")).expect("undo what enrol wrote");

        let (again, created) = r.open(&repo).expect("opens again");
        assert!(!created, "opening an open conversation minted something");
        assert_eq!(again.id, p.id);
        assert_eq!(
            home.read_secret(&p.id).expect("reads").as_deref(),
            Some(first.as_str()),
            "opening again rotated the secret under a live bridge"
        );
        assert_eq!(r.all().count(), 1);

        // A repo enrolled the OLD way — a row and a repo token, no channel copy — is refused,
        // and told the verb that copies its secret across, because `open` cannot: it holds only
        // the hash, and minting a fresh secret would take the live bridge off the air.
        let old = super::tests::repo(&d, "old-way");
        let (old_p, _) = r.enrol(&old).expect("enrols");
        home.remove_secret(&old_p.id)
            .expect("as a box from before this change");
        let said = r.open(&old).expect_err("refused").to_string();
        assert!(said.contains("adopt-secrets"), "{said}");
        for jargon in ["Err", "None", "Option", "canonical"] {
            assert!(
                !said.contains(jargon),
                "jargon reached the operator: {said}"
            );
        }
        assert!(
            home.read_secret(&old_p.id).expect("reads").is_none(),
            "a refusal still wrote a secret"
        );
    }

    #[test]
    fn enrolling_a_folder_below_a_seed_with_rooms_names_the_seed_and_never_a_room() {
        // The containment check finds the first row in id order whose repo is a parent of the
        // folder. Rooms share their seed's repo and a `c-` id sorts before a `p-` one, so the
        // refusal named whichever room sorted first — a row that is vacant and hidden from every
        // list, so he was told his folder was inside a project he had never seen.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let org = repo(&d, "org");
        let (seed, _) = r.open(&org).expect("opens");
        r.grant(&org, 2).expect("grants");
        let inside = repo(&d, "org/sub");
        let said = r.enrol(&inside).expect_err("refused").to_string();
        assert!(
            said.contains(&format!("\"{}\"", seed.title)),
            "the refusal does not name the seed: {said}"
        );
        assert!(
            !said.contains("room"),
            "the refusal names a room he has never seen: {said}"
        );
        let said = r.open(&inside).expect_err("refused").to_string();
        assert!(!said.contains("room"), "{said}");
    }

    #[test]
    fn opening_a_project_whose_channel_copy_is_stale_says_so_and_names_the_verb_that_mends_it() {
        // `open` on an existing row asked only whether a channel copy EXISTS, never whether it
        // hashes to what the hub knows. A stale copy — bytes nobody can be admitted with — was
        // reported as an open conversation, the link re-asserted, and "Nothing was changed",
        // while every new session in that repo presented the stale bytes and was turned away for
        // good. The likeliest way to get one: a rotation typed with the binary from before
        // conversations existed, which rewrites the repo's copy and the hash and knows nothing of
        // the channel's.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "rot");
        let home = home(&d);
        let (p, current) = r.enrol(&repo).expect("enrols");
        home.write_secret(&p.id, &"0".repeat(64))
            .expect("the channel's copy, gone stale");

        // The repo still holds the current bytes: the migration verb copies them across.
        let said = r.open(&repo).expect_err("refused").to_string();
        assert!(
            said.contains("adopt-secrets --apply"),
            "the verb that mends it is not named: {said}"
        );
        assert!(
            !said.contains("already open") && !said.contains("Nothing was changed"),
            "a stale copy was reported as open: {said}"
        );
        for jargon in ["Err", "None", "Option", "sha256", "token_sha256"] {
            assert!(
                !said.contains(jargon),
                "jargon reached the operator: {said}"
            );
        }
        assert_eq!(
            home.read_secret(&p.id).expect("reads").as_deref(),
            Some("0".repeat(64).as_str()),
            "a refusal rewrote the channel's copy"
        );

        // Neither copy is current: nothing on the box can connect, and only a rotation mends it.
        fs::write(repo.join(TOKEN_FILE), "1".repeat(64)).expect("write");
        let said = r.open(&repo).expect_err("refused").to_string();
        assert!(
            said.contains("herdr-tg enroll"),
            "the rotation is not named: {said}"
        );
        assert!(!said.contains("adopt-secrets"), "{said}");

        // And a channel copy that IS current is what "already open" means.
        home.write_secret(&p.id, &current).expect("mended by hand");
        let (again, created) = r.open(&repo).expect("opens");
        assert!(!created);
        assert_eq!(again.id, p.id);
    }

    #[test]
    fn a_grant_whose_list_could_not_be_saved_leaves_no_slot_and_no_secret_behind() {
        // `grant` wrote each room's secret and its vacant slot BEFORE saving the rows, and when
        // the save failed it unwound nothing: N secrets and N slots with no row, which the book
        // counted as vacant, so the ceiling was spent on rooms that did not exist and a
        // dispatcher taking one got a hello nobody would admit. `enrol` puts its copy back when
        // its second write fails; this is the same discipline.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let org = repo(&d, "org");
        let (seed, _) = r.open(&org).expect("opens");
        let home = home(&d);
        let before = fs::read(d.path().join("projects.json")).expect("the registry");

        // A directory where the registry's temporary file goes, so the save cannot open it. The
        // temp name is per process, so this blocks exactly this process's next save.
        let blocker = d
            .path()
            .join(format!("projects.json.tmp.{}", std::process::id()));
        fs::create_dir_all(&blocker).expect("the blocker");
        let said = r
            .grant(&org, 2)
            .expect_err("a grant whose list cannot be saved")
            .to_string();
        fs::remove_dir(&blocker).expect("unblock");

        assert!(
            said.contains("nothing was granted") || said.contains("taken back"),
            "the refusal does not say the rooms were taken back: {said}"
        );
        assert_eq!(
            fs::read(d.path().join("projects.json")).expect("the registry"),
            before,
            "the failed grant changed the list"
        );
        let slots = home.slots(&seed.id).expect("slots");
        assert!(
            slots.is_empty(),
            "a failed grant left slots behind: {slots:?}"
        );
        let left: Vec<String> = fs::read_dir(home.conversations())
            .expect("readable")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("c-"))
            .collect();
        assert!(
            left.is_empty(),
            "a failed grant left secrets behind: {left:?}"
        );

        // The book is whole: sixteen can still be granted.
        let full = r.grant(&org, 16).expect("a full book after a failed grant");
        assert_eq!(full.len(), 16);
    }

    #[test]
    fn a_vacant_slot_whose_room_has_no_row_never_holds_a_page_of_the_book() {
        // A slot naming a room the registry has never heard of — left by a crash between the
        // slot and the save — is a page nothing can use: the hub admits on the hash, and there is
        // none. It is taken back at the next grant rather than counted, so a phantom can neither
        // fill the book nor be taken by a dispatcher and refused `unknown_project`.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let org = repo(&d, "org");
        let (seed, _) = r.open(&org).expect("opens");
        let home = home(&d);
        let phantom = ProjectId::new("c-0000000000ff");
        home.write_secret(&phantom, "a-secret-with-no-row")
            .expect("the phantom's secret");
        home.add_slot(&seed.id, &phantom)
            .expect("the phantom's slot");
        // A slot a dispatcher already TOOK is spent, whatever became of its row: its number is
        // never reused, because the room it named may have a topic and a history.
        let taken = ProjectId::new("c-0000000000ee");
        home.add_slot(&seed.id, &taken).expect("a slot");
        let book = home.grants().join(seed.id.as_str());
        fs::rename(book.join("001.vacant"), book.join("001.taken")).expect("take");

        let full = r.grant(&org, 16).expect("a phantom must not hold a page");
        assert_eq!(full.len(), 16);
        let slots = home.slots(&seed.id).expect("slots");
        assert!(
            !slots.iter().any(|s| s.room == phantom),
            "the phantom is still in the book: {slots:?}"
        );
        assert!(
            slots.iter().any(|s| s.room == taken && s.taken),
            "a taken slot was swept: {slots:?}"
        );
        assert_eq!(slots.iter().filter(|s| !s.taken).count(), 16);
        assert!(
            home.read_secret(&phantom).expect("reads").is_none(),
            "a credential nobody minted a row for is still on disk"
        );
        assert!(
            slots.iter().all(|s| s.number != 1 || s.taken),
            "a taken number was reused: {slots:?}"
        );
    }

    #[test]
    fn rotating_an_opened_project_writes_nothing_into_its_repo() {
        // `open` collects the prize: no file in the repo. Rotation is `enrol`, and `enrol` wrote
        // BOTH places, always — so rotating a leaked secret on an opened project put the token
        // back into the tree, on the very day it mattered, and in a repo git would commit it from
        // it was refused outright instead. A rotation rewrites the places the secret already
        // lives and no new one: an opened project rotates where the channel keeps it, an adopted
        // one rotates both until its repo copy is taken away.
        let d = tempfile::tempdir().expect("tmp");
        let mut r = reg(&d);
        let repo = repo(&d, "org");
        let home = home(&d);
        let (p, _) = r.open(&repo).expect("opens");
        let old = home.read_secret(&p.id).expect("reads").expect("a secret");
        assert!(
            !r.would_write_repo_copy(&repo),
            "an opened project would get a token"
        );

        let (rotated, fresh) = r.enrol(&repo).expect("rotates");
        assert_eq!(rotated.id, p.id);
        assert!(
            !repo.join(".kickoff").exists(),
            "rotating an opened project wrote a token into its repo"
        );
        assert_eq!(
            home.read_secret(&p.id).expect("reads").as_deref(),
            Some(fresh.as_str())
        );
        assert!(r.resolve(&fresh).is_some() && r.resolve(&old).is_none());

        // An adopted project — both copies present — rotates both.
        let adopted = super::tests::repo(&d, "adopted");
        let (a, _) = r.enrol(&adopted).expect("enrols the old way, both places");
        assert!(r.would_write_repo_copy(&adopted));
        let (_, s2) = r.enrol(&adopted).expect("rotates");
        assert_eq!(
            fs::read_to_string(adopted.join(TOKEN_FILE)).expect("readable"),
            s2
        );
        assert_eq!(
            home.read_secret(&a.id).expect("reads").as_deref(),
            Some(s2.as_str())
        );
        // And once its repo copy is taken away, a rotation does not put one back.
        fs::remove_dir_all(adopted.join(".kickoff")).expect("as remove-repo-secret leaves it");
        assert!(!r.would_write_repo_copy(&adopted));
        r.enrol(&adopted).expect("rotates channel-only");
        assert!(!adopted.join(".kickoff").exists());
        // A folder nobody enrolled gets both, as it always did.
        assert!(r.would_write_repo_copy(&super::tests::repo(&d, "fresh")));
    }
}
